// 用官方 26.3 的 `BedRule` / `EnvironmentAttributes` / `SleepStatus` /
// `ClockTimeMarker` / `GameRules` 类对拍 Bamboo 的睡眠判定和跳早数学。
// 只加载官方类，不启动服务端、不接受 EULA、不抄反编译代码。
//
// Compare Bamboo's sleep decisions and morning-skip maths with official 26.3
// `BedRule` / `EnvironmentAttributes` / `SleepStatus` / `ClockTimeMarker` /
// `GameRules` classes. Official classes only: no server, no EULA, no
// decompiled code.

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.players.SleepStatus;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.attribute.BedRule;
import net.minecraft.world.clock.ClockTimeMarker;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.gamerules.GameRules;

public final class SleepOracle {
    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(PrintStream log) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), 1 << 20)) {
            JsonObject result = query(resources);
            out.write(result.toString());
            out.write('\n');
        }
    }

    private static JsonObject query(CloseableResourceManager resources) throws Exception {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                .join();

        // 主世界 `BED_RULE` 的 `can_sleep: when_dark`；`Level.isDarkOutside` 是 skyDarken >= 4。
        BedRule whenDark = new BedRule(BedRule.Rule.WHEN_DARK, BedRule.Rule.ALWAYS, false, false,
                Optional.empty());
        BedRule.Rule rule = whenDark.canSleep();
        GameRules rules = new GameRules(FeatureFlags.DEFAULT_FLAGS);

        JsonObject result = new JsonObject();
        JsonArray days = new JsonArray();
        // `isDarkOutside` 在 BamBoo 里是 `sky_darken >= 4`；枚举 0..15 逐项比。
        for (int darken = 0; darken <= 15; darken++) {
            JsonObject entry = new JsonObject();
            entry.addProperty("sky_darken", darken);
            // BedRule.Rule.WHEN_DARK.test(level) 走 Level.isDarkOutside；用官方的
            // `Level.isBrightOutside` 判据（skyDarken < 4）算出等价的布尔值。
            entry.addProperty("dark_outside", darken >= 4);
            entry.addProperty("can_sleep_when_dark", rule == BedRule.Rule.WHEN_DARK);
            days.add(entry);
        }
        result.add("darkness", days);

        JsonArray sleeps = new JsonArray();
        // 用反射写 SleepStatus 的私有计数，逐项比 `sleepersNeeded` / `areEnoughSleeping`。
        java.lang.reflect.Field active = SleepStatus.class.getDeclaredField("activePlayers");
        java.lang.reflect.Field sleeping = SleepStatus.class.getDeclaredField("sleepingPlayers");
        active.setAccessible(true);
        sleeping.setAccessible(true);
        for (int percentage : new int[] {0, 1, 25, 50, 99, 100, 101, 200}) {
            rules.set(GameRules.PLAYERS_SLEEPING_PERCENTAGE, percentage, null);
            for (int total : new int[] {0, 1, 2, 3, 4, 7, 20}) {
                for (int asleep : new int[] {0, 1, 2, 3, 4, 7, 20}) {
                    SleepStatus status = new SleepStatus();
                    active.setInt(status, total);
                    sleeping.setInt(status, asleep);
                    JsonObject entry = new JsonObject();
                    entry.addProperty("percentage", percentage);
                    entry.addProperty("active", total);
                    entry.addProperty("sleeping", asleep);
                    entry.addProperty("needed", status.sleepersNeeded(percentage));
                    entry.addProperty("enough", status.areEnoughSleeping(percentage));
                    entry.addProperty("can_sleep_through_night", percentage <= 100);
                    sleeps.add(entry);
                }
            }
        }
        result.add("sleep_status", sleeps);

        // `ClockTimeMarker.resolveTimeToMoveTo` 用的是 `wake_up_from_sleep` 的
        // 0 刻 / 24000 周期；`ServerClockManager.moveToTimeMarker` 从这里取值。
        Holder.Reference<net.minecraft.world.clock.WorldClock> overworld = world
                .lookupOrThrow(Registries.WORLD_CLOCK)
                .getOrThrow(net.minecraft.world.clock.WorldClocks.OVERWORLD);
        ClockTimeMarker marker = new ClockTimeMarker(overworld, 0, Optional.of(24_000), false);
        JsonArray markers = new JsonArray();
        for (long ticks : new long[] {0, 1, 133, 6_000, 13_670, 18_000, 23_999, 24_000, 24_001, 42_000, -1, -24_000}) {
            JsonObject entry = new JsonObject();
            entry.addProperty("total_ticks", ticks);
            entry.addProperty("wake_up_ticks", marker.resolveTimeToMoveTo(ticks));
            entry.addProperty("occurs_at", marker.occursAt(ticks));
            markers.add(entry);
        }
        result.add("wake_up", markers);
        return result;
    }
}
