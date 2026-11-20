// 超平坦对拍：官方 codec、平坦地形、结构起点/引用及单中心 3×3 FEATURES。
// 不启动服务端，不接受 EULA。
//
// Superflat oracle: vanilla codec, terrain, structure starts/references, and
// one center's 3×3 FEATURES view. No live server or EULA.

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
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
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagKey;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.levelgen.FlatLevelSource;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.flat.FlatLayerInfo;
import net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorSettings;
import net.minecraft.world.level.levelgen.structure.StructureSet;
import net.minecraft.world.level.levelgen.structure.placement.RandomSpreadStructurePlacement;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;

public final class FlatWorldOracle {
    private static final int OUTPUT_BUFFER = 4 << 20;
    /** 覆盖默认预设、全部五个字段、0 层、features/lakes/structure_overrides 各种组合。 */
    private static final String[] CASES = {
        "{\"layers\":[{\"block\":\"minecraft:bedrock\",\"height\":1},{\"block\":\"minecraft:dirt\",\"height\":2},{\"block\":\"minecraft:grass_block\",\"height\":1}],\"biome\":\"minecraft:plains\"}",
        "{\"layers\":[{\"block\":\"minecraft:stone\",\"height\":3}],\"biome\":\"minecraft:desert\",\"features\":false,\"lakes\":false}",
        "{\"layers\":[{\"block\":\"minecraft:stone\",\"height\":2},{\"block\":\"minecraft:glass\",\"height\":0},{\"block\":\"minecraft:sand\",\"height\":1}],\"biome\":\"minecraft:the_void\",\"features\":true,\"lakes\":true,\"structure_overrides\":[\"minecraft:villages\",\"minecraft:desert_pyramids\"]}",
        "{\"layers\":[{\"block\":\"minecraft:deepslate\",\"height\":1},{\"block\":\"minecraft:tuff\",\"height\":4}],\"biome\":\"minecraft:plains\",\"structure_overrides\":[\"minecraft:igloos\"]}",
        "{\"layers\":[{\"block\":\"minecraft:grass_block\",\"height\":1}],\"biome\":\"minecraft:plains\",\"features\":true}",
    };

    private FlatWorldOracle() {
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
            RegistryOps<JsonElement> ops = RegistryOps.create(JsonOps.INSTANCE, registries);
            for (String input : CASES) {
                JsonObject line = runCase(input, ops, registries);
                write(out, line);
            }
            runDecorationCases(registries, resources, ops, out);
            out.flush();
        }
    }

    private static void write(Writer out, JsonObject line) throws Exception {
        out.write(line.toString());
        out.write('\n');
    }

    private static void runDecorationCases(RegistryAccess.Frozen registries,
            CloseableResourceManager resources, RegistryOps<JsonElement> ops, Writer out)
            throws Exception {
        var fixture = new ChunkgenOracle.FlatFixture(registries, resources, out);
        var village = (RandomSpreadStructurePlacement) registries
                .lookupOrThrow(Registries.STRUCTURE_SET)
                .getValue(net.minecraft.resources.Identifier.withDefaultNamespace("villages")).placement();
        var villagePos = village.getPotentialStructureChunk(0L, 0, 0);
        String villageInput = "{\"layers\":[{\"block\":\"minecraft:bedrock\",\"height\":1},"
                + "{\"block\":\"minecraft:dirt\",\"height\":2},"
                + "{\"block\":\"minecraft:grass_block\",\"height\":1}],"
                + "\"biome\":\"minecraft:plains\",\"structure_overrides\":[\"minecraft:villages\"]}";
        emitDecoration(out, ops, registries, fixture, "village", villageInput, villagePos, true);
        emitDecoration(out, ops, registries, fixture, "structures-disabled", villageInput, villagePos, false);
        emitDecoration(out, ops, registries, fixture, "empty-overrides",
                villageInput.replace("[\"minecraft:villages\"]", "[]"), villagePos, true);
        emitDecoration(out, ops, registries, fixture, "void-platform-and-deferred-layer",
                "{\"layers\":[{\"block\":\"minecraft:snow\",\"height\":1}],\"biome\":\"minecraft:the_void\",\"features\":true}",
                new ChunkPos(0, 0), false);
        emitDecoration(out, ops, registries, fixture, "plains-decoration",
                "{\"layers\":[{\"block\":\"minecraft:dirt\",\"height\":127},"
                        + "{\"block\":\"minecraft:grass_block\",\"height\":1}],"
                        + "\"biome\":\"minecraft:plains\",\"features\":true}",
                new ChunkPos(0, 0), false);
        emitDecoration(out, ops, registries, fixture, "lava-lake",
                "{\"layers\":[{\"block\":\"minecraft:stone\",\"height\":160}],\"biome\":\"minecraft:plains\",\"lakes\":true}",
                lakeChunk(), false);
    }

    // 找一个确实通过表面湖 rarity 的区块；Rust 侧还要求输出里有湖的熔岩。
    // Find a chunk that passes the surface-lake rarity roll; Rust also requires actual lava.
    private static ChunkPos lakeChunk() {
        var random = new WorldgenRandom(new XoroshiroRandomSource(0L));
        for (int x = 0; x < 4096; x++) {
            var pos = new ChunkPos(x, 0);
            long seed = random.setDecorationSeed(0L, pos.getMinBlockX(), pos.getMinBlockZ());
            random.setFeatureSeed(seed, 1, 1);
            if (random.nextFloat() < 1.0F / 200) {
                return pos;
            }
        }
        throw new IllegalStateException("no surface lava lake candidate");
    }

    private static void emitDecoration(Writer out, RegistryOps<JsonElement> ops,
            RegistryAccess.Frozen registries, ChunkgenOracle.FlatFixture fixture, String name,
            String input, ChunkPos pos, boolean structures) throws Exception {
        var line = runCase(input, ops, registries);
        var settings = FlatLevelGeneratorSettings.CODEC
                .parse(ops, JsonParser.parseString(input)).getOrThrow();
        line.addProperty("case", name);
        line.addProperty("x", pos.x());
        line.addProperty("z", pos.z());
        line.addProperty("generateStructures", structures);
        fixture.decorate(new FlatLevelSource(settings), 0L, pos, structures, line);
        write(out, line);
    }

    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(
            CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(
                        resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups =
                net.minecraft.tags.TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool()).join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }

    private static JsonObject runCase(String input, RegistryOps<JsonElement> ops,
            RegistryAccess.Frozen registries) throws Exception {
        JsonElement json = JsonParser.parseString(input);
        FlatLevelGeneratorSettings settings =
                FlatLevelGeneratorSettings.CODEC.parse(ops, json).getOrThrow();
        // 用官方 codec 重新编码，读出 features / lakes（没有公开的 getter）。
        // Re-encode with the official codec to read features / lakes (no public getter).
        JsonObject canonical = FlatLevelGeneratorSettings.CODEC
                .encodeStart(ops, settings).getOrThrow().getAsJsonObject();
        boolean features = canonical.get("features").getAsBoolean();
        boolean lakes = canonical.get("lakes").getAsBoolean();

        JsonObject line = new JsonObject();
        line.addProperty("type", "flat");
        line.addProperty("input", input);
        line.addProperty("features", features);
        line.addProperty("lakes", lakes);
        line.addProperty("biome", settings.getBiome().unwrapKey().orElseThrow()
                .identifier().toString());

        JsonArray layers = new JsonArray();
        for (FlatLayerInfo info : settings.getLayersInfo()) {
            JsonObject entry = new JsonObject();
            entry.addProperty("block",
                    BuiltInRegistries.BLOCK.getKey(info.getBlockState().getBlock()).toString());
            entry.addProperty("height", info.getHeight());
            layers.add(entry);
        }
        line.add("layers", layers);
        settings.structureOverrides().ifPresent(set ->
                line.add("structure_overrides", decodedOverrides(set)));

        // 官方 FlatLevelSource.buildTerrain：只填地层，不受 features/lakes/结构影响。
        // Official FlatLevelSource.buildTerrain: fills layers only, unaffected by
        // features / lakes / structures.
        FlatLevelSource generator = new FlatLevelSource(settings);
        generator.validate();
        LevelHeightAccessor height = LevelHeightAccessor.create(-64, 384);
        ChunkPos pos = new ChunkPos(0, 0);
        ProtoChunk chunk = new ProtoChunk(pos, UpgradeData.EMPTY, height,
                PalettedContainerFactory.create(registries), null);
        generator.buildTerrain(chunk, Blender.empty(), null, null, null, null,
                java.util.Set.of()).join();
        addBlocks(line, chunk);
        return line;
    }

    /** 把 HolderSet 规范化成字符串列表（#tag 或注册名列表），和 Bamboo 侧一致。 */
    /** Normalize a HolderSet into a string list (#tag or ids), matching Bamboo. */
    private static JsonArray decodedOverrides(HolderSet<StructureSet> set) {
        JsonArray result = new JsonArray();
        Optional<TagKey<StructureSet>> tag = set.unwrapKey();
        if (tag.isPresent()) {
            result.add("#" + tag.get().location().toString());
        } else {
            set.stream().forEach(holder ->
                    holder.unwrapKey().ifPresent(k -> result.add(k.identifier().toString())));
        }
        return result;
    }

    /** palette + blocks，顺序 y、x、z，和 chunkgen / gen-world 一样。 */
    /** palette + blocks in y, x, z order, like chunkgen / gen-world. */
    private static void addBlocks(JsonObject line, ChunkAccess chunk) {
        List<BlockState> palette = new ArrayList<>();
        java.util.Map<BlockState, Integer> ids = new java.util.HashMap<>();
        JsonArray blocks = new JsonArray();
        int minY = chunk.getMinY();
        for (int y = 0; y < chunk.getHeight(); y++) {
            for (int x = 0; x < 16; x++) {
                for (int z = 0; z < 16; z++) {
                    BlockState state = chunk.getBlockState(new BlockPos(x, minY + y, z));
                    Integer id = ids.get(state);
                    if (id == null) {
                        id = palette.size();
                        palette.add(state);
                        ids.put(state, id);
                    }
                    blocks.add(id);
                }
            }
        }
        JsonArray names = new JsonArray();
        for (BlockState state : palette) {
            names.add(state.toString());
        }
        line.add("palette", names);
        line.add("blocks", blocks);
    }
}
