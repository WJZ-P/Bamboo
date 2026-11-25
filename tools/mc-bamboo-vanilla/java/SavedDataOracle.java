// 用官方 codec 读 Bamboo 写出的 saved data（游戏规则、世界边界、世界时钟、袭击表、
// 超平坦和若干噪声 world_gen_settings.dat），报告解码结果；读不动就非 0 退出。
// 只加载官方类、不启动服务端、不接受 EULA。
//
// Decode Bamboo's saved-data files (game rules, world border, world clocks,
// raids, a flat and several noise world_gen_settings.dat) with the official
// codecs and report the result; exit non-zero when a file cannot be decoded.
// No server, no EULA.

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
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.clock.ClockState;
import net.minecraft.world.clock.PackedClockStates;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.gamerules.GameRule;
import net.minecraft.world.level.gamerules.GameRuleMap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.WorldGenSettings;
import net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorSettings;
import net.minecraft.world.level.levelgen.FlatLevelSource;
import net.minecraft.world.entity.raid.Raids;

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
        if (args.length < 5) {
            throw new IllegalArgumentException("usage: SavedDataOracle <game_rules.dat> <world_border.dat> <world_clocks.dat> <raids.dat> <flat_world_gen_settings.dat> [noise_world_gen_settings.dat ...]");
        }

        CompoundTag genRoot = NbtIo.readCompressed(Path.of(args[4]), NbtAccounter.unlimitedHeap());
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

        // 额外传入的每份 world_gen_settings.dat 都必须是官方 WorldGenSettings.CODEC
        // 能解析的 minecraft:noise 生成器；把解析出的形状报告给 Rust 侧逐项核对。
        // Every extra world_gen_settings.dat must be a minecraft:noise generator that
        // the official WorldGenSettings.CODEC accepts; report its decoded shape.
        JsonArray noiseGens = new JsonArray();
        for (int i = 5; i < args.length; i++) {
            CompoundTag noiseRoot = NbtIo.readCompressed(Path.of(args[i]), NbtAccounter.unlimitedHeap());
            Tag noiseData = noiseRoot.get("data");
            if (noiseData == null) {
                throw new IllegalStateException("noise world_gen_settings.dat has no data tag");
            }
            WorldGenSettings parsed = WorldGenSettings.CODEC.parse(ops, noiseData).getOrThrow();
            var noiseStem = parsed.dimensions().get(net.minecraft.world.level.dimension.LevelStem.OVERWORLD);
            if (noiseStem.isEmpty() || !(noiseStem.get().generator() instanceof NoiseBasedChunkGenerator noise)) {
                throw new IllegalStateException("noise world_gen_settings.dat overworld is not minecraft:noise");
            }
            boolean overworldPreset = noise.getBiomeSource() instanceof MultiNoiseBiomeSource mn
                    && mn.stable(MultiNoiseBiomeSourceParameterLists.OVERWORLD);
            JsonObject ng = new JsonObject();
            ng.addProperty("generator", "minecraft:noise");
            ng.addProperty("settings", noise.generatorSettings().unwrapKey()
                    .map(k -> k.identifier().toString()).orElse(""));
            ng.addProperty("biome_source", overworldPreset ? "minecraft:multi_noise" : "other");
            ng.addProperty("preset", overworldPreset ? "minecraft:overworld" : "other");
            ng.addProperty("seed", parsed.options().seed());
            ng.addProperty("generate_structures", parsed.options().generateStructures());
            ng.addProperty("bonus_chest", parsed.options().generateBonusChest());
            noiseGens.add(ng);
        }

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

        // world_clocks.dat：PackedClockStates.CODEC 是注册名 → ClockState 的 map。
        // 用 RegistryOps 才能把 minecraft:overworld / minecraft:the_end 解成 Holder，
        // 这正是 ServerClockManager.TYPE 的 codec 在读档时用的 ops。
        // world_clocks.dat: PackedClockStates.CODEC maps registry names to
        // ClockState. RegistryOps is needed to resolve the WorldClock holders.
        CompoundTag clocksRoot = NbtIo.readCompressed(Path.of(args[2]), NbtAccounter.unlimitedHeap());
        Tag clocksData = clocksRoot.get("data");
        if (clocksData == null) {
            throw new IllegalStateException("world_clocks.dat has no data tag");
        }
        DataResult<PackedClockStates> parsedClocks = PackedClockStates.CODEC.parse(ops, clocksData);
        PackedClockStates clocks = parsedClocks.getOrThrow();
        JsonArray clocksArray = new JsonArray();
        for (var entry : clocks.clocks().entrySet()) {
            Identifier id = entry.getKey().unwrapKey()
                    .map(k -> k.identifier())
                    .orElseThrow(() -> new IllegalStateException("world clock has no registry key"));
            ClockState state = entry.getValue();
            JsonObject clock = new JsonObject();
            clock.addProperty("name", id.toString());
            clock.addProperty("total_ticks", state.totalTicks());
            clock.addProperty("partial_tick", state.partialTick());
            clock.addProperty("rate", state.rate());
            clock.addProperty("paused", state.paused());
            clocksArray.add(clock);
        }

        // raids.dat：先让官方 `Raids.CODEC` 解一遍（坏状态名等会让 getOrThrow 抛错），
        // 再从同一份 data 标签逐字段报告，Rust 侧按写出的示例逐项断言。
        // raids.dat: the official `Raids.CODEC` must parse it first (a bad status
        // name makes getOrThrow throw); then report every field from the data tag.
        CompoundTag raidsRoot = NbtIo.readCompressed(Path.of(args[3]), NbtAccounter.unlimitedHeap());
        Tag raidsData = raidsRoot.get("data");
        if (raidsData == null) {
            throw new IllegalStateException("raids.dat has no data tag");
        }
        Raids.CODEC.parse(NbtOps.INSTANCE, raidsData).getOrThrow();
        JsonObject raids = new JsonObject();
        raids.addProperty("next_id", ((CompoundTag) raidsData).getIntOr("next_id", 0));
        raids.addProperty("tick", ((CompoundTag) raidsData).getIntOr("tick", 0));
        JsonArray raidEntries = new JsonArray();
        for (Tag raw : ((CompoundTag) raidsData).getListOrEmpty("raids")) {
            CompoundTag entry = (CompoundTag) raw;
            JsonObject one = new JsonObject();
            one.addProperty("id", entry.getIntOr("id", 0));
            one.addProperty("started", entry.getBooleanOr("started", false));
            one.addProperty("active", entry.getBooleanOr("active", false));
            one.addProperty("ticks_active", entry.getLongOr("ticks_active", 0));
            one.addProperty("raid_omen_level", entry.getIntOr("raid_omen_level", 0));
            one.addProperty("groups_spawned", entry.getIntOr("groups_spawned", 0));
            one.addProperty("cooldown_ticks", entry.getIntOr("cooldown_ticks", 0));
            one.addProperty("post_raid_ticks", entry.getIntOr("post_raid_ticks", 0));
            one.addProperty("total_health", entry.getFloatOr("total_health", 0));
            one.addProperty("group_count", entry.getIntOr("group_count", 0));
            one.addProperty("status", entry.getStringOr("status", ""));
            JsonArray center = new JsonArray();
            for (int coordinate : entry.getIntArray("center").orElse(new int[0])) {
                center.add(coordinate);
            }
            one.add("center", center);
            JsonArray heroes = new JsonArray();
            for (Tag hero : entry.getListOrEmpty("heroes_of_the_village")) {
                heroes.add(net.minecraft.core.UUIDUtil.uuidFromIntArray(
                        ((net.minecraft.nbt.IntArrayTag) hero).getAsIntArray()).toString());
            }
            one.add("heroes", heroes);
            raidEntries.add(one);
        }
        raids.add("entries", raidEntries);

        JsonObject result = new JsonObject();
        result.add("raids", raids);
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
        result.add("world_clocks", clocksArray);
        result.add("world_gen_settings", result0);
        result.add("noise_generators", noiseGens);

        try (Writer out = new OutputStreamWriter(stdout, StandardCharsets.UTF_8)) {
            out.write(result.toString());
            out.flush();
        }
        }
    }
}
