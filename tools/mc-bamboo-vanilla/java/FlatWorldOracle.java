// 官方 `FlatLevelGeneratorSettings.CODEC` + `FlatLevelSource.buildTerrain` 对拍。
// 对一组 generator-settings JSON：用官方 codec 解析（验证全部五个字段），再用官方
// FlatLevelSource 生成区块，输出解码后的字段和逐方块快照给 Rust 侧比较。
// 只加载官方类、在 JVM 内造 ProtoChunk，不启动服务端、不读存档、不接受 EULA。
//
// Vanilla `FlatLevelGeneratorSettings.CODEC` + `FlatLevelSource.buildTerrain`
// oracle. For each generator-settings JSON: parse with the official codec
// (validating all five fields), generate a chunk with the official FlatLevelSource,
// and emit the decoded fields and a per-block snapshot for the Rust side.
// Only official classes, in-JVM ProtoChunks; no server, no save, no EULA.

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
import net.minecraft.core.Holder;
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
                out.write(line.toString());
                out.write('\n');
            }
            out.flush();
        }
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
        line.add("structure_overrides", decodedOverrides(settings.structureOverrides()));

        // 官方 FlatLevelSource.buildTerrain：只填地层，不受 features/lakes/结构影响。
        // Official FlatLevelSource.buildTerrain: fills layers only, unaffected by
        // features / lakes / structures.
        FlatLevelSource generator = new FlatLevelSource(settings);
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
    private static JsonArray decodedOverrides(Optional<HolderSet<StructureSet>> overrides) {
        JsonArray result = new JsonArray();
        if (overrides.isEmpty()) {
            return result;
        }
        HolderSet<StructureSet> set = overrides.get();
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
