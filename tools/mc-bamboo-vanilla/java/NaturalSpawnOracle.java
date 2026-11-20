// 自然刷怪的加权选择和组大小对拍：不启动服务端、不读存档、不接受 EULA。
//
// 对每个群系的每个 `MobCategory`，用同一个种子的官方 `RandomSource` 跑若干次：
// `WeightedList.getRandom` 挑出一项（`nextInt(总权重)` 后累减权重），再用这一项的
// `IntProvider.sample` 取组大小（官方刷怪表里只有常量和均匀分布）。另外把 0..n 的列表
// 交给官方 `Util.shuffle`，和 Bamboo 的打乱逐项比较。
//
// 一行 JSON：`{"type":"pick","biome":"minecraft:plains","category":"creature","picks":[[type,count],...],"seed":N}`
// 或 `{"type":"shuffle","size":8,"order":[...],"seed":N}`。
//
// Natural-spawn weighted selection and group size, without a server, a save or the
// EULA. For every biome and `MobCategory`, run vanilla `WeightedList.getRandom`
// (nextInt(total weight) then walk the cumulative weights) and `IntProvider.sample`
// with a seeded `RandomSource`. Also shuffle 0..n with vanilla `Util.shuffle`.
// One JSON line per case.

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.util.Util;
import net.minecraft.util.random.WeightedList;
import net.minecraft.world.attribute.EnvironmentAttributes;
import net.minecraft.world.entity.MobCategory;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.MobSpawnSettings;

public final class NaturalSpawnOracle {
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final int PICKS_PER_CASE = 8;
    private static final int SHUFFLE_SIZE = 16;
    private static final long[] SEEDS = {0L, 1L, 2L, 7L, 42L, -1L, 12345L};

    private NaturalSpawnOracle() {
    }

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
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                     new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
             Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            Registry<Biome> biomes = registries.lookupOrThrow(Registries.BIOME);
            for (Identifier name : biomes.keySet().stream().sorted().toList()) {
                Biome biome = biomes.getValue(name);
                // 26.3 的刷怪设置是群系的环境属性 `NATURAL_MOB_SPAWNS`，按 overlay 叠在空表上。
                MobSpawnSettings settings = biome.getAttributes()
                        .applyModifier(EnvironmentAttributes.NATURAL_MOB_SPAWNS, MobSpawnSettings.EMPTY);
                for (MobCategory category : MobCategory.values()) {
                    WeightedList<MobSpawnSettings.SpawnerData> spawns = settings.getMobsToSpawn(category);
                    if (spawns.isEmpty()) {
                        continue;
                    }
                    for (long seed : SEEDS) {
                        out.write(pickCase(name, category, spawns, seed).toString());
                        out.write('\n');
                    }
                }
            }
            for (long seed : SEEDS) {
                out.write(shuffleCase(seed).toString());
                out.write('\n');
            }
            out.flush();
        }
    }

    private static JsonObject pickCase(Identifier biome, MobCategory category,
            WeightedList<MobSpawnSettings.SpawnerData> spawns, long seed) {
        RandomSource random = RandomSource.create(seed);
        JsonArray picks = new JsonArray();
        for (int i = 0; i < PICKS_PER_CASE; i++) {
            Optional<MobSpawnSettings.SpawnerData> picked = spawns.getRandom(random);
            if (picked.isEmpty()) {
                picks.add("empty");
                continue;
            }
            MobSpawnSettings.SpawnerData data = picked.get();
            int count = data.count().sample(random);
            JsonArray pick = new JsonArray();
            pick.add(net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(data.type()).toString());
            pick.add(count);
            picks.add(pick);
        }
        JsonObject line = new JsonObject();
        line.addProperty("type", "pick");
        line.addProperty("biome", biome.toString());
        line.addProperty("category", category.getName());
        line.add("picks", picks);
        line.addProperty("seed", seed);
        line.addProperty("after", seedAfter(random));
        return line;
    }

    private static JsonObject shuffleCase(long seed) {
        RandomSource random = RandomSource.create(seed);
        List<Integer> order = new ArrayList<>();
        for (int i = 0; i < SHUFFLE_SIZE; i++) {
            order.add(i);
        }
        Util.shuffle(order, random);
        JsonArray array = new JsonArray();
        for (int value : order) {
            array.add(value);
        }
        JsonObject line = new JsonObject();
        line.addProperty("type", "shuffle");
        line.addProperty("size", SHUFFLE_SIZE);
        line.add("order", array);
        line.addProperty("seed", seed);
        return line;
    }

    /** 官方 `RandomSource` 的内部状态不能直接读，改用「再取一个 nextInt」当指纹。 */
    private static int seedAfter(RandomSource random) {
        return random.nextInt(1000);
    }

    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool()).join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }
}
