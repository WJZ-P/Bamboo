import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Predicate;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
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
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;

/**
 * 用官方 26.3 的 Structure.GenerationContext 对拍 findValidGenerationPoint 的通用判定：
 * couldValidBiomeExistOnTopOfChunkCenter、getFirstOccupiedHeight / 四角高度、
 * isValidBiome，外加 createStructures 的加权选择序列和「起点 + 选中的结构 + 是否有效」组合。
 *
 * <p>按官方 WorldLoader 前半段读注册表（和 StructureRingsOracle 一样），不启动服务端、
 * 不读存档、不接受 EULA。queries.txt 每行 "seed x z"，由 Rust 侧生成并传进来。
 *
 * <p>输出：SETS/STRUCTURES 两张名字表，BIOME（每个 structure 的合法群系名），
 * 以及每条查询的 H（中心 + 四角高度）、Q（每结构的整列 + 起点群系判定）、
 * S（每个结构集的加权尝试序列）、D（顺序组合出的选中结构与有效性）。
 *
 * <p>Compare the generic gates of findValidGenerationPoint against vanilla 26.3's
 * Structure.GenerationContext: couldValidBiomeExistOnTopOfChunkCenter,
 * getFirstOccupiedHeight / corner heights, isValidBiome, plus createStructures' weighted
 * selection order and the composed "start + selected structure + valid" triple.
 * Loads registries like StructureRingsOracle; no server, no save, no EULA.
 *
 * <p>Usage: StructureValidityOracle &lt;worldgen.json&gt; &lt;queries.txt&gt;
 */
public final class StructureValidityOracle {
    // 六个 random_spread 结构集，都没有 exclusion zone、不需要 concentric_rings。
    // Six random_spread sets, none with an exclusion zone, none needing concentric_rings.
    private static final String[] SETS = {
        "minecraft:villages", "minecraft:desert_pyramids", "minecraft:igloos",
        "minecraft:jungle_temples", "minecraft:swamp_huts", "minecraft:ocean_ruins",
    };
    private static final int SIZE_X = 16;
    private static final int SIZE_Z = 16;

    // 一条查询的三段坐标拼成一个字符串键。
    // A string key from the three coordinates of a query.
    private static String key(long seed, int cx, int cz) {
        return seed + " " + cx + " " + cz;
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException(
                    "usage: StructureValidityOracle <worldgen.json> <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            run(registries, Path.of(args[0]), Path.of(args[1]));
        }
    }

    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(
            CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }

    private static void run(RegistryAccess registries, Path worldgenPath, Path queriesPath)
            throws Exception {
        HolderLookup.RegistryLookup<net.minecraft.world.level.levelgen.synth.NormalNoise> noiseLookup =
                registries.lookupOrThrow(Registries.NOISE);
        Registry<NoiseGeneratorSettings> settingsReg =
                registries.lookupOrThrow(Registries.NOISE_SETTINGS);
        NoiseGeneratorSettings settings = settingsReg.getValue(Identifier.withDefaultNamespace("overworld"));
        if (settings == null) {
            throw new IllegalStateException("missing minecraft:overworld noise settings");
        }
        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(
                                        Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        BiomeSource biomeSource = generator.getBiomeSource();
        LevelHeightAccessor heightAccessor = LevelHeightAccessor.create(
                settings.noiseSettings().minY(), settings.noiseSettings().height());
        int minY = heightAccessor.getMinY();
        int height = heightAccessor.getHeight();

        Registry<Structure> structureReg = registries.lookupOrThrow(Registries.STRUCTURE);
        JsonObject root;
        try (Reader reader = Files.newBufferedReader(worldgenPath, StandardCharsets.UTF_8)) {
            root = JsonParser.parseReader(reader).getAsJsonObject();
        }
        JsonObject setJson = root.getAsJsonObject("structure_set");

        // 把六个集合里的结构去重，按名字排序，作为 Q 判定覆盖的结构。
        // Dedup and sort the structures of the six sets; these are the Q-covered structures.
        List<String> structures = new ArrayList<>();
        for (String set : SETS) {
            for (JsonElement e : setJson.getAsJsonObject(set).getAsJsonArray("structures")) {
                String id = e.getAsJsonObject().get("structure").getAsString();
                if (!structures.contains(id)) {
                    structures.add(id);
                }
            }
        }
        structures.sort(String::compareTo);

        // 每条查询按种子分组，RandomState / climateSampler 只按种子建一次。
        // Group queries by seed; RandomState / climateSampler are built once per seed.
        List<String> lines = Files.readAllLines(queriesPath, StandardCharsets.UTF_8);
        Map<Long, RandomState> states = new HashMap<>();
        Map<Long, net.minecraft.world.level.biome.Climate.Sampler> samplers = new HashMap<>();
        for (String line : lines) {
            if (!line.isBlank()) {
                long seed = Long.parseLong(line.trim().split("\\s+")[0]);
                if (!states.containsKey(seed)) {
                    RandomState state = RandomState.create(noiseLookup, seed, settings);
                    states.put(seed, state);
                    samplers.put(seed,
                            state.createClimateSampler(SamplerContext.builder().enableCaches().build()));
                }
            }
        }

        // 先算出所有查询的高度、Q 判定和 S 序列，再统一输出（D 要复用它们）。
        // Compute all heights, Q gates and S orders first, then emit (D reuses them).
        Map<String, int[]> heights = new HashMap<>();
        Map<String, int[]> gates = new HashMap<>();      // structure key -> {col, pv}
        Map<String, List<String>> orders = new HashMap<>();
        for (String line : lines) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\s+");
            long seed = Long.parseLong(p[0]);
            int cx = Integer.parseInt(p[1]);
            int cz = Integer.parseInt(p[2]);
            String k = key(seed, cx, cz);
            RandomState state = states.get(seed);
            net.minecraft.world.level.biome.Climate.Sampler sampler = samplers.get(seed);
            ChunkPos pos = new ChunkPos(cx, cz);
            int centerX = pos.getMiddleBlockX();
            int centerZ = pos.getMiddleBlockZ();
            int minX = pos.getMinBlockX();
            int minZ = pos.getMinBlockZ();

            int center = getHeight(generator, centerX, centerZ, heightAccessor, state);
            int[] corners = {
                getHeight(generator, minX, minZ, heightAccessor, state),
                getHeight(generator, minX, minZ + SIZE_Z, heightAccessor, state),
                getHeight(generator, minX + SIZE_X, minZ, heightAccessor, state),
                getHeight(generator, minX + SIZE_X, minZ + SIZE_Z, heightAccessor, state),
            };
            int[] h = new int[5];
            h[0] = center;
            System.arraycopy(corners, 0, h, 1, 4);
            heights.put(k, h);

            for (String id : structures) {
                Structure structure = structureReg.getValue(Identifier.parse(id));
                if (structure == null) {
                    throw new IllegalStateException("missing structure " + id);
                }
                Predicate<Holder<Biome>> predicate = structure.biomes()::contains;
                Structure.GenerationContext ctx = new Structure.GenerationContext(
                        registries, generator, biomeSource, sampler, state, null, seed, pos,
                        heightAccessor, predicate);
                int col = ctx.couldValidBiomeExistOnTopOfChunkCenter() ? 1 : 0;
                BlockPos stub = new BlockPos(centerX, center, centerZ);
                int pv = ctx.isValidBiome(new Structure.GenerationStub(stub,
                        (StructurePiecesBuilder builder) -> {})) ? 1 : 0;
                gates.put(k + " " + id, new int[]{col, pv});
            }

            for (String set : SETS) {
                orders.put(k + " " + set, weightedOrder(setJson, set, seed, cx, cz));
            }
        }

        StringBuilder out = new StringBuilder();
        out.append("SETS ").append(SETS.length).append(' ').append(String.join(",", SETS)).append('\n');
        out.append("STRUCTURES ").append(structures.size()).append(' ')
                .append(String.join(",", structures)).append('\n');
        // BIOME 覆盖全部 structure，顺带核对 Rust 侧标签解析。
        // BIOME covers every structure, cross-checking Rust's tag parsing.
        List<String> allStructures = new ArrayList<>();
        root.getAsJsonObject("structure").entrySet().forEach(e -> allStructures.add(e.getKey()));
        allStructures.sort(String::compareTo);
        for (String id : allStructures) {
            Structure structure = structureReg.getValue(Identifier.parse(id));
            if (structure == null) {
                throw new IllegalStateException("missing structure " + id);
            }
            List<String> names = new ArrayList<>();
            structure.biomes().stream().forEach(h ->
                    names.add(h.unwrapKey().orElseThrow().identifier().toString()));
            names.sort(String::compareTo);
            out.append("BIOME ").append(id).append(' ').append(String.join(",", names)).append('\n');
        }

        for (String line : lines) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\s+");
            long seed = Long.parseLong(p[0]);
            int cx = Integer.parseInt(p[1]);
            int cz = Integer.parseInt(p[2]);
            String k = key(seed, cx, cz);
            int[] h = heights.get(k);
            out.append("H ").append(k).append(' ').append(h[0]).append(' ').append(h[1]).append(' ')
                    .append(h[2]).append(' ').append(h[3]).append(' ').append(h[4]).append('\n');
            for (String id : structures) {
                int[] g = gates.get(k + " " + id);
                out.append("Q ").append(k).append(' ').append(id).append(' ').append(g[0]).append(' ')
                        .append(g[1]).append('\n');
            }
            for (String set : SETS) {
                out.append("S ").append(k).append(' ').append(set).append(' ')
                        .append(String.join(",", orders.get(k + " " + set))).append('\n');
            }
            // D：按集合顺序、每集合内按加权顺序，第一个 col&&pv 的就是选中的结构。
            // D: over sets in order, weighted order inside each, the first col&&pv wins.
            String selectedSet = "-";
            String selected = "-";
            boolean valid = false;
            outer:
            for (String set : SETS) {
                for (String id : orders.get(k + " " + set)) {
                    int[] g = gates.get(k + " " + id);
                    if (g != null && g[0] == 1 && g[1] == 1) {
                        selectedSet = set;
                        selected = id;
                        valid = true;
                        break outer;
                    }
                }
            }
            out.append("D ").append(k).append(' ').append(selectedSet).append(' ').append(selected)
                    .append(' ').append(valid ? 1 : 0).append(' ').append(h[0]).append('\n');
        }
        System.out.print(out);
    }

    private static int getHeight(ChunkGenerator generator, int x, int z,
            LevelHeightAccessor heightAccessor, RandomState state) {
        return generator.getFirstOccupiedHeight(x, z, Heightmap.Types.WORLD_SURFACE_WG,
                heightAccessor, state);
    }

    private static List<String> weightedOrder(JsonObject setJson, String set,
            long seed, int x, int z) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, x, z);
        List<JsonElement> options = new ArrayList<>();
        setJson.getAsJsonObject(set).getAsJsonArray("structures").forEach(options::add);
        int total = 0;
        for (JsonElement option : options) {
            total += option.getAsJsonObject().get("weight").getAsInt();
        }
        List<String> sequence = new ArrayList<>();
        while (!options.isEmpty()) {
            int choice = random.nextInt(total);
            int index = 0;
            for (int i = 0; i < options.size(); i++) {
                choice -= options.get(i).getAsJsonObject().get("weight").getAsInt();
                if (choice < 0) {
                    break;
                }
                index++;
            }
            JsonObject selected = options.get(index).getAsJsonObject();
            sequence.add(selected.get("structure").getAsString());
            total -= selected.get("weight").getAsInt();
            options.remove(index);
        }
        return sequence;
    }
}
