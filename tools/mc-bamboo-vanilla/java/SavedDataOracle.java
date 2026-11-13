// 用官方 codec 读 Bamboo 写出的两份 saved data，报告解码结果；读不动就非 0 退出。
// 只加载官方类、不启动服务端、不接受 EULA。
//
// Decode Bamboo's two saved-data files with the official codecs and report the
// result; exit non-zero when a file cannot be decoded. No server, no EULA.

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.serialization.DataResult;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.gamerules.GameRule;
import net.minecraft.world.level.gamerules.GameRuleMap;
import net.minecraft.world.level.levelgen.WorldGenSettings;
import net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorSettings;
import net.minecraft.world.level.levelgen.FlatLevelSource;

public final class SavedDataOracle {
    private static JsonObject result0;

    private SavedDataOracle() {
    }

    /** 和 LightOracle 一样，只加载世界层注册表，不构造 ServerLevel。 */
    private static RegistryAccess.Frozen loadRegistries(CloseableResourceManager resources) {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        // 官方日志默认写 stdout，抢在日志框架初始化之前换掉，否则会混进 JSON。
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
        RegistryAccess.Frozen registries = loadRegistries(resources);
        RegistryOps<Tag> ops = RegistryOps.create(NbtOps.INSTANCE, registries);
        if (args.length != 3) {
            throw new IllegalArgumentException("usage: SavedDataOracle <game_rules.dat> <world_border.dat> <world_gen_settings.dat>");
        }

        CompoundTag genRoot = NbtIo.readCompressed(Path.of(args[2]), NbtAccounter.unlimitedHeap());
        Tag genData = genRoot.get("data");
        if (genData == null) {
            throw new IllegalStateException("world_gen_settings.dat has no data tag");
        }
        // 官方 codec 必须能解出这份文件；主世界生成器要是 minecraft:flat。
        WorldGenSettings settings = WorldGenSettings.CODEC.parse(ops, genData).getOrThrow();
        var stem = settings.dimensions().get(net.minecraft.world.level.dimension.LevelStem.OVERWORLD);
        if (stem.isEmpty() || !(stem.get().generator() instanceof FlatLevelSource flat)) {
            throw new IllegalStateException("overworld generator is not minecraft:flat");
        }
        JsonObject gen = new JsonObject();
        JsonArray layers = new JsonArray();
        for (var info : flat.settings().getLayersInfo()) {
            Identifier id = BuiltInRegistries.BLOCK.getKey(info.getBlockState().getBlock());
            JsonObject layer = new JsonObject();
            layer.addProperty("block", id.toString());
            layer.addProperty("height", info.getHeight());
            layers.add(layer);
        }
        gen.add("layers", layers);
        gen.addProperty("biome", registries.lookupOrThrow(Registries.BIOME)
                .getKey(flat.settings().getBiome().value()).toString());
        result0 = gen;

        CompoundTag gameRulesRoot = NbtIo.readCompressed(Path.of(args[0]), NbtAccounter.unlimitedHeap());
        Tag gameRulesData = gameRulesRoot.get("data");
        if (gameRulesData == null) {
            throw new IllegalStateException("game_rules.dat has no data tag");
        }
        DataResult<GameRuleMap> parsedRules = GameRuleMap.CODEC.parse(NbtOps.INSTANCE, gameRulesData);
        GameRuleMap rules = parsedRules.getOrThrow();

        CompoundTag borderRoot = NbtIo.readCompressed(Path.of(args[1]), NbtAccounter.unlimitedHeap());
        Tag borderData = borderRoot.get("data");
        if (borderData == null) {
            throw new IllegalStateException("world_border.dat has no data tag");
        }
        DataResult<WorldBorder.Settings> parsedBorder = WorldBorder.Settings.CODEC.parse(NbtOps.INSTANCE, borderData);
        WorldBorder.Settings border = parsedBorder.getOrThrow();

        JsonObject result = new JsonObject();
        JsonArray rulesArray = new JsonArray();
        for (GameRule<?> rule : rules.keySet()) {
            Identifier id = BuiltInRegistries.GAME_RULE.getKey(rule);
            Object value = rules.get(rule);
            JsonObject entry = new JsonObject();
            entry.addProperty("name", id == null ? "" : id.toString());
            if (value instanceof Boolean b) {
                entry.addProperty("type", "bool");
                entry.addProperty("value", b);
            } else if (value instanceof Integer i) {
                entry.addProperty("type", "int");
                entry.addProperty("value", i);
            }
            rulesArray.add(entry);
        }
        result.add("game_rules", rulesArray);

        JsonObject b = new JsonObject();
        b.addProperty("center_x", border.centerX());
        b.addProperty("center_z", border.centerZ());
        b.addProperty("damage_per_block", border.damagePerBlock());
        b.addProperty("safe_zone", border.safeZone());
        b.addProperty("warning_blocks", border.warningBlocks());
        b.addProperty("warning_time", border.warningTime());
        b.addProperty("size", border.size());
        b.addProperty("lerp_time", border.lerpTime());
        b.addProperty("lerp_target", border.lerpTarget());
        result.add("world_border", b);
        result.add("world_gen_settings", result0);

        try (Writer out = new OutputStreamWriter(stdout, StandardCharsets.UTF_8)) {
            out.write(result.toString());
            out.flush();
        }
        }
    }
}
