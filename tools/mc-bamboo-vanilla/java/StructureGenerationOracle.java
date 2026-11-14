import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
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
import net.minecraft.tags.BiomeTags;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.block.Rotation;
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
 * 对拍四个代码生成结构的 {@code findGenerationPoint} 前置判定与「这个起点是否真的生成」。
 *
 * <p>Java 侧用官方 {@code Structure.GenerationContext} 直接调
 * {@code Structure.findValidGenerationPoint}（= findGenerationPoint 再 filter isValidBiome），
 * 并逐项 dump 各结构的专用判定：恶地矿井 getBaseHeight(WORLD_SURFACE_WG)、海底神殿
 * getBiomesWithin 周边海洋判定 + OCEAN_FLOOR_WG、要塞起点、林地府邸四角高度 +
 * startPos.getY() &lt; 60。Rust 侧用生产纯函数复算，逐字段比较。
 *
 * <p>不启动服务端、不读存档、不接受 EULA。所有结构件的生成 consumer 在
 * {@code findValidGenerationPoint} 里都不会被触发，所以 context 的
 * structureTemplateManager 传 null 也没问题。
 *
 * <p>Compare the four code-built structures' {@code findGenerationPoint} gates and
 * "whether this start really generates" against vanilla. Java builds a real
 * {@code Structure.GenerationContext} and calls {@code Structure.findValidGenerationPoint};
 * Rust recomputes each field with production pure functions. No server, save or EULA.
 * Piece-generation consumers are never invoked by {@code findValidGenerationPoint}, so a
 * null structureTemplateManager is fine.
 *
 * <p>Usage: StructureGenerationOracle &lt;queries.txt&gt;
 */
public final class StructureGenerationOracle {
    private static final int SIZE_X = 16;
    private static final int SIZE_Z = 16;
    private static final int SEA_LEVEL = 63;
    private static final int SURROUND_RADIUS = 29;
    // 五个结构 id；biomes 从 structure 注册表读，Rust 侧也从 worldgen.json 读。
    // Five structure ids; biomes come from the structure registry on both sides.
    private static final String[] KINDS = {
        "minecraft:mineshaft", "minecraft:mineshaft_mesa", "minecraft:stronghold",
        "minecraft:monument", "minecraft:mansion"
    };

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException(
                    "usage: StructureGenerationOracle <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            run(registries, Path.of(args[0]));
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

    private static void run(RegistryAccess registries, Path queriesPath) throws Exception {
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
                        registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        BiomeSource biomeSource = generator.getBiomeSource();
        LevelHeightAccessor heightAccessor = LevelHeightAccessor.create(
                settings.noiseSettings().minY(), settings.noiseSettings().height());

        Registry<Structure> structureReg = registries.lookupOrThrow(Registries.STRUCTURE);
        // 校验五个结构都在注册表里，缺一个就报错（Rust 侧 from_id 也只认这五个）。
        // Verify all five ids exist (Rust's from_id only accepts these five).
        for (String id : KINDS) {
            if (structureReg.getValue(Identifier.parse(id)) == null) {
                throw new IllegalStateException("missing structure " + id);
            }
        }

        List<String> lines = Files.readAllLines(queriesPath, StandardCharsets.UTF_8);
        Map<Long, RandomState> states = new HashMap<>();
        Map<Long, Climate.Sampler> samplers = new HashMap<>();
        for (String line : lines) {
            if (!line.isBlank()) {
                long seed = Long.parseLong(line.trim().split("\\s+")[0]);
                if (!states.containsKey(seed)) {
                    RandomState state = RandomState.create(noiseLookup, seed, settings);
                    states.put(seed, state);
                    samplers.put(seed, state.createClimateSampler(
                            SamplerContext.builder().enableCaches().build()));
                }
            }
        }

        StringBuilder out = new StringBuilder();
        out.append("KINDS ").append(KINDS.length).append(' ')
                .append(String.join(" ", KINDS)).append('\n');
        // BIOME：每个结构的合法群系名集合，Rust 侧标签展开核对。
        // BIOME: allowed biome name set per structure, cross-checked with Rust tag expansion.
        for (String id : KINDS) {
            Structure structure = structureReg.getValue(Identifier.parse(id));
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
            RandomState state = states.get(seed);
            Climate.Sampler sampler = samplers.get(seed);
            ChunkPos pos = new ChunkPos(cx, cz);
            int centerX = pos.getMiddleBlockX();
            int centerZ = pos.getMiddleBlockZ();
            int minX = pos.getMinBlockX();
            int minZ = pos.getMinBlockZ();
            int blockX7 = pos.getBlockX(7);
            int blockZ7 = pos.getBlockZ(7);
            int offsetX9 = pos.getBlockX(9);
            int offsetZ9 = pos.getBlockZ(9);

            // 中心 + 四角 WORLD_SURFACE_WG getFirstOccupiedHeight。
            // Center + four corners WORLD_SURFACE_WG getFirstOccupiedHeight.
            int[] h = {
                getHeight(generator, centerX, centerZ, heightAccessor, state),
                getHeight(generator, minX, minZ, heightAccessor, state),
                getHeight(generator, minX, minZ + SIZE_Z, heightAccessor, state),
                getHeight(generator, minX + SIZE_X, minZ, heightAccessor, state),
                getHeight(generator, minX + SIZE_X, minZ + SIZE_Z, heightAccessor, state),
            };
            // OCEAN_FLOOR_WG getFirstOccupiedHeight。
            int oceanFloor = generator.getFirstOccupiedHeight(centerX, centerZ,
                    Heightmap.Types.OCEAN_FLOOR_WG, heightAccessor, state);
            // getBaseHeight(WORLD_SURFACE_WG) = getFirstOccupiedHeight + 1。
            int base = generator.getBaseHeight(centerX, centerZ,
                    Heightmap.Types.WORLD_SURFACE_WG, heightAccessor, state);

            // 林地府邸的 Rotation.getRandom 与四角高度（getLowestYIn5by5Box）。
            Rotation rotation = rotationFor(seed, cx, cz);
            int lowest = mansionLowestY(generator, heightAccessor, state, blockX7, blockZ7, rotation);
            boolean below60 = lowest < 60;

            // 周边海洋判定与结构无关，每个 (seed,cx,cz) 算一次；用任一 ctx 的
            // biomeResolver（都是 biomeSource.createResolver(climateSampler)）。
            // The surrounding gate is structure-independent; compute once per site using any
            // ctx's biomeResolver (always biomeSource.createResolver(climateSampler)).
            Structure firstStructure = structureReg.getValue(Identifier.parse(KINDS[0]));
            Predicate<Holder<Biome>> sharedPredicate = firstStructure.biomes()::contains;
            Structure.GenerationContext sharedCtx = new Structure.GenerationContext(
                    registries, generator, biomeSource, sampler, state, null, seed, pos,
                    heightAccessor, sharedPredicate);
            boolean sur = surroundingOk(sharedCtx.biomeResolver(), offsetX9, offsetZ9);

            for (String id : KINDS) {
                Structure structure = structureReg.getValue(Identifier.parse(id));
                Predicate<Holder<Biome>> predicate = structure.biomes()::contains;
                Structure.GenerationContext ctx = new Structure.GenerationContext(
                        registries, generator, biomeSource, sampler, state, null, seed, pos,
                        heightAccessor, predicate);
                Optional<Structure.GenerationStub> stub = structure.findValidGenerationPoint(ctx);
                boolean valid = stub.isPresent();
                BlockPos sp = stub.map(Structure.GenerationStub::position).orElse(BlockPos.ZERO);
                // 通用整列 / 单点判定（都用该结构的 validBiome）。
                boolean col = ctx.couldValidBiomeExistOnTopOfChunkCenter();
                boolean col7 = ctx.couldValidBiomeExistInTerrainColumn(blockX7, blockZ7);
                // 规范单点：center, h[0], centerZ 上的 isValidBiome。
                boolean pv = ctx.isValidBiome(new Structure.GenerationStub(
                        new BlockPos(centerX, h[0], centerZ), (StructurePiecesBuilder b) -> {}));

                out.append("R ").append(seed).append(' ').append(cx).append(' ').append(cz).append(' ')
                        .append(id).append(' ').append(bool(valid)).append(' ')
                        .append(sp.getX()).append(' ').append(sp.getY()).append(' ').append(sp.getZ()).append(' ')
                        .append(bool(col)).append(' ').append(bool(col7)).append(' ').append(bool(sur)).append(' ')
                        .append(oceanFloor).append(' ').append(base).append(' ')
                        .append(h[0]).append(' ').append(h[1]).append(' ').append(h[2]).append(' ')
                        .append(h[3]).append(' ').append(h[4]).append(' ')
                        .append(lowest).append(' ').append(bool(below60)).append(' ').append(bool(pv)).append('\n');
            }
        }
        System.out.print(out);
    }

    private static boolean surroundingOk(BiomeResolver resolver, int offsetX, int offsetZ) {
        Set<Holder<Biome>> biomes = resolver.getBiomesWithin(offsetX, SEA_LEVEL, offsetZ, SURROUND_RADIUS);
        for (Holder<Biome> biome : biomes) {
            if (!biome.is(BiomeTags.REQUIRED_OCEAN_MONUMENT_SURROUNDING)) {
                return false;
            }
        }
        return true;
    }

    /** `getLowestYIn5by5Box`：rotation 决定 (±5, ±5) 的四角，取四个 WORLD_SURFACE_WG 高度的最小值。 */
    private static int mansionLowestY(NoiseBasedChunkGenerator generator,
            LevelHeightAccessor heightAccessor, RandomState state, int blockX, int blockZ,
            Rotation rotation) {
        int offsetX = 5;
        int offsetZ = 5;
        if (rotation == Rotation.CLOCKWISE_90) {
            offsetX = -5;
        } else if (rotation == Rotation.CLOCKWISE_180) {
            offsetX = -5;
            offsetZ = -5;
        } else if (rotation == Rotation.COUNTERCLOCKWISE_90) {
            offsetZ = -5;
        }
        int[] corners = {
            getHeight(generator, blockX, blockZ, heightAccessor, state),
            getHeight(generator, blockX, blockZ + offsetZ, heightAccessor, state),
            getHeight(generator, blockX + offsetX, blockZ, heightAccessor, state),
            getHeight(generator, blockX + offsetX, blockZ + offsetZ, heightAccessor, state),
        };
        return Math.min(Math.min(corners[0], corners[1]), Math.min(corners[2], corners[3]));
    }

    private static Rotation rotationFor(long seed, int cx, int cz) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, cx, cz);
        return Rotation.getRandom(random);
    }

    private static int getHeight(ChunkGenerator generator, int x, int z,
            LevelHeightAccessor heightAccessor, RandomState state) {
        return generator.getFirstOccupiedHeight(x, z, Heightmap.Types.WORLD_SURFACE_WG,
                heightAccessor, state);
    }

    private static String bool(boolean value) {
        return value ? "1" : "0";
    }
}
