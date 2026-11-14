import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Set;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
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
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.structures.BuriedTreasurePieces;
import net.minecraft.world.level.levelgen.structure.structures.DesertPyramidPiece;
import net.minecraft.world.level.levelgen.structure.structures.JungleTemplePiece;
import net.minecraft.world.level.levelgen.structure.structures.SwampHutPiece;

/**
 * 在合成地形的假世界里跑代码生成单件结构的官方 {@code postProcess}，逐方块 dump 写前写后的差异。
 *
 * <p>不启动服务端、不读存档、不接受 EULA、不加载 worldgen 注册表、不生成地形：
 * 地形完全由一条公式决定（见 {@link #fillTerrain}），Rust 侧用同一条公式造一样的区块，
 * 所以两边比较的是 {@code postProcess} 本身，不含地形差异。
 *
 * <p>女巫和猫是实体，需要真实 {@code ServerLevel}；代理给不了，{@code postProcess} 在那一步
 * 抛出的异常被捕获（方块已在此前写完），查询头里的 {@code entityAbort} 记录是否发生过。
 *
 * <p>输出每个查询一段：{@code Q kind seed cx cz count entityAbort}，随后若干
 * {@code B x y z stateId} 行（只含与写前不同的格，按 y / x / z 排序）。
 *
 * <p>Runs the official {@code postProcess} of the code-built single-piece structures on
 * synthetic terrain and dumps the block differences. No server, save, EULA, worldgen registry
 * or terrain generation: the terrain is a pure formula (see {@link #fillTerrain}) that the Rust
 * side reproduces exactly, so the comparison isolates {@code postProcess}. The witch and cat
 * need a real {@code ServerLevel} that a proxy cannot provide, so an exception from that step
 * is caught (blocks are already written) and flagged by {@code entityAbort}.
 *
 * <p>Usage: StructurePiecePlaceOracle &lt;queries.txt&gt;
 */
public final class StructurePiecePlaceOracle {
    static final int MIN_Y = -64;
    static final int HEIGHT = 384;

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: StructurePiecePlaceOracle <queries.txt>");
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
        LevelHeightAccessor heightAccessor = LevelHeightAccessor.create(MIN_Y, HEIGHT);
        PalettedContainerFactory containerFactory = PalettedContainerFactory.create(registries);
        List<String> lines = Files.readAllLines(queriesPath, StandardCharsets.UTF_8);
        StringBuilder out = new StringBuilder();
        for (String line : lines) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\s+");
            String kind = p[0];
            long seed = Long.parseLong(p[1]);
            int cx = Integer.parseInt(p[2]);
            int cz = Integer.parseInt(p[3]);
            ChunkPos pos = new ChunkPos(cx, cz);
            ProtoChunk chunk = new ProtoChunk(pos, UpgradeData.EMPTY, heightAccessor,
                    containerFactory, null);
            fillTerrain(chunk);
            chunk.setPersistedStatus(ChunkStatus.BIOMES);
            // 高度图必须由官方 prime：postProcess 的 updateAverageGroundHeight 读它。
            // The official prime must fill the heightmaps: postProcess reads them.
            Heightmap.primeHeightmaps(chunk, Set.of(
                    Heightmap.Types.MOTION_BLOCKING_NO_LEAVES,
                    Heightmap.Types.MOTION_BLOCKING,
                    Heightmap.Types.WORLD_SURFACE_WG,
                    Heightmap.Types.OCEAN_FLOOR_WG));

            WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(
                    StructurePiecePlaceOracle.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class},
                    new PieceLevelHandler(chunk, heightAccessor, seed));
            StructureManager structures =
                    new StructureManager(level, new WorldOptions(seed, false, false), null);

            StructurePiece piece = buildPiece(kind, seed, pos);
            TreeMap<BlockPos, BlockState> before = snapshot(chunk);
            BoundingBox chunkBb = new BoundingBox(pos.getMinBlockX(), MIN_Y + 1, pos.getMinBlockZ(),
                    pos.getMinBlockX() + 15, MIN_Y + HEIGHT - 1, pos.getMinBlockZ() + 15);
            WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(seed));
            boolean entityAbort = false;
            try {
                piece.postProcess(level, structures, null, random, chunkBb, pos, BlockPos.ZERO);
            } catch (Throwable t) {
                entityAbort = true;
                // stderr 会混进 oracle 的 stdout 解析，所以写成带前缀的日志行，Rust 侧跳过。
                // stderr would pollute the oracle's stdout parsing, so log through stdout
                // with a prefix the Rust side skips.
                System.out.println("T " + kind + " " + seed + " " + cx + " " + cz + " "
                        + t.getClass().getName());
            }
            // 诊断行：结构件包围盒平移后的范围，用来判断 `lowest_ground_height` 是否把结构
            // 推出了区块。只在 `PIECE_PLACE_DEBUG` 打开时输出。
            // Diagnostic: the piece box after the height shift, to see whether
            // `lowest_ground_height` pushed it out of the chunk. Only with `PIECE_PLACE_DEBUG`.
            if (System.getenv("PIECE_PLACE_DEBUG") != null) {
                var box = piece.getBoundingBox();
                System.out.println("D " + kind + " " + seed + " " + cx + " " + cz + " "
                        + box.minX() + " " + box.minY() + " " + box.minZ() + " "
                        + box.maxX() + " " + box.maxY() + " " + box.maxZ());
            }
            TreeMap<BlockPos, BlockState> after = snapshot(chunk);

            TreeMap<BlockPos, BlockState> changed = new TreeMap<>();
            for (var entry : after.entrySet()) {
                BlockState old = before.get(entry.getKey());
                if (old == null || !old.equals(entry.getValue())) {
                    changed.put(entry.getKey(), entry.getValue());
                }
            }
            for (var key : before.keySet()) {
                if (!after.containsKey(key)) {
                    changed.put(key, Blocks.AIR.defaultBlockState());
                }
            }
            out.append("Q ").append(kind).append(' ').append(seed).append(' ').append(cx)
                    .append(' ').append(cz).append(' ').append(changed.size()).append(' ')
                    .append(entityAbort ? 1 : 0).append('\n');
            for (var entry : changed.entrySet()) {
                BlockPos at = entry.getKey();
                out.append("B ").append(at.getX()).append(' ').append(at.getY()).append(' ')
                        .append(at.getZ()).append(' ')
                        .append(Block.BLOCK_STATE_REGISTRY.getId(entry.getValue())).append('\n');
            }
        }
        System.out.print(out);
    }

    /**
     * 合成地形：每一列的地表高度 `surface(x,z) = 64 + floorMod(x*31 + z*17, 5) - 2`，
     * `surface` 是沙子、`surface-1` 是砂岩、更下面是石头，再往上全是空气。
     *
     * <p>Rust 侧的 {@code synthetic_terrain} 用完全相同的公式，所以两边的方块和高度图逐格一致。
     *
     * <p>Synthetic terrain: `surface(x,z) = 64 + floorMod(x*31 + z*17, 5) - 2`; the surface is
     * sand, `surface-1` is sandstone, below is stone and above is air. The Rust
     * `synthetic_terrain` uses the identical formula, so both sides match block for block.
     */
    static void fillTerrain(ProtoChunk chunk) {
        ChunkPos pos = chunk.getPos();
        for (int x = 0; x < 16; x++) {
            for (int z = 0; z < 16; z++) {
                int worldX = pos.getMinBlockX() + x;
                int worldZ = pos.getMinBlockZ() + z;
                int surface = 64 + Math.floorMod(worldX * 31 + worldZ * 17, 5) - 2;
                for (int y = MIN_Y; y < surface - 1; y++) {
                    chunk.setBlockState(new BlockPos(worldX, y, worldZ),
                            Blocks.STONE.defaultBlockState(), 0);
                }
                chunk.setBlockState(new BlockPos(worldX, surface - 1, worldZ),
                        Blocks.SANDSTONE.defaultBlockState(), 0);
                chunk.setBlockState(new BlockPos(worldX, surface, worldZ),
                        Blocks.SAND.defaultBlockState(), 0);
            }
        }
    }

    /**
     * 按 kind 造结构件，起点坐标和随机源和官方 {@code generatePieces} 一致：
     * `makeRandom(seed, chunkPos)` = `WorldgenRandom(LegacyRandomSource(0))` 再
     * `setLargeFeatureSeed(seed, cx, cz)`，Rust 侧 `pieces::make_random` 是同一套。
     *
     * Build the piece with the same start and random source as vanilla `generatePieces`:
     * `makeRandom(seed, chunkPos)` = `WorldgenRandom(LegacyRandomSource(0))` followed by
     * `setLargeFeatureSeed(seed, cx, cz)`, which is what the Rust `pieces::make_random` does.
     */
    private static StructurePiece buildPiece(String kind, long seed, ChunkPos pos) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, pos.x(), pos.z());
        int x = pos.getMinBlockX();
        int z = pos.getMinBlockZ();
        return switch (kind) {
            case "swamp_hut" -> new SwampHutPiece(random, x, z);
            case "desert_pyramid" -> new DesertPyramidPiece(random, x, z);
            case "jungle_temple" -> new JungleTemplePiece(random, x, z);
            case "buried_treasure" -> new BuriedTreasurePieces.BuriedTreasurePiece(
                    new BlockPos(pos.getBlockX(9), 90, pos.getBlockZ(9)));
            default -> throw new IllegalArgumentException("unknown kind " + kind);
        };
    }

    /**
     * `WorldGenRegion.getRandom()`：`randomState.getOrCreateRandomFactory("worldgen_region_random")`
     * 在区块中心 `getWorldPosition()` 上的随机源。
     *
     * <p>和 Rust 侧 `region_random` 走同一条 Legacy 位置派生：先用
     * `WorldgenRandom(LegacyRandomSource(worldSeed))` 取一次 `nextLong()` 当工厂种子，再
     * `LegacyPositionalRandomFactory.at(x, 0, z)`。
     *
     * <p>The `WorldGenRegion` random source: `randomState.getOrCreateRandomFactory(
     * "worldgen_region_random")` at the chunk centre. Uses the same Legacy positional
     * derivation as the Rust `region_random`.
     */
    private static net.minecraft.util.RandomSource regionRandom(long worldSeed, ChunkPos center) {
        WorldgenRandom source = new WorldgenRandom(new LegacyRandomSource(worldSeed));
        long factorySeed = source.nextLong();
        return new LegacyRandomSource.LegacyPositionalRandomFactory(factorySeed)
                .at(center.getWorldPosition());
    }

    /** 逐格快照：只收非空气方块，省内存。 */
    private static TreeMap<BlockPos, BlockState> snapshot(ProtoChunk chunk) {        TreeMap<BlockPos, BlockState> blocks = new TreeMap<>();
        ChunkPos pos = chunk.getPos();
        for (int y = MIN_Y; y < MIN_Y + HEIGHT; y++) {
            for (int x = 0; x < 16; x++) {
                for (int z = 0; z < 16; z++) {
                    BlockPos at = new BlockPos(pos.getMinBlockX() + x, y, pos.getMinBlockZ() + z);
                    BlockState state = chunk.getBlockState(at);
                    if (!state.isAir()) {
                        blocks.put(at, state);
                    }
                }
            }
        }
        return blocks;
    }

    /** 假 {@code WorldGenLevel}：只实现 postProcess 用到的方法，其余抛异常。 */
    static final class PieceLevelHandler implements InvocationHandler {
        private final ProtoChunk chunk;
        private final LevelHeightAccessor heightAccessor;
        private final long seed;
        private final ChunkPos center;

        PieceLevelHandler(ProtoChunk chunk, LevelHeightAccessor heightAccessor, long seed) {
            this.chunk = chunk;
            this.heightAccessor = heightAccessor;
            this.seed = seed;
            this.center = chunk.getPos();
        }

        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            String name = method.getName();
            switch (name) {
                case "getBlockState":
                    return this.chunk.getBlockState((BlockPos) args[0]);
                case "getFluidState":
                    return this.chunk.getFluidState((BlockPos) args[0]);
                case "setBlock": {
                    BlockPos pos = (BlockPos) args[0];
                    BlockState state = (BlockState) args[1];
                    this.chunk.setBlockState(pos, state, 2);
                    return true;
                }
                case "setBlockAndUpdate":
                    this.chunk.setBlockState((BlockPos) args[0], (BlockState) args[1], 2);
                    return true;
                case "getHeight": {
                    int arity = args == null ? 0 : args.length;
                    if (arity == 0) {
                        return HEIGHT;
                    }
                    if (arity == 3) {
                        Heightmap.Types type = (Heightmap.Types) args[0];
                        int x = (Integer) args[1];
                        int z = (Integer) args[2];
                        return this.chunk.getHeight(type, x & 15, z & 15) + 1;
                    }
                    Heightmap.Types type = (Heightmap.Types) args[0];
                    BlockPos pos = (BlockPos) args[1];
                    return this.chunk.getHeight(type, pos.getX() & 15, pos.getZ() & 15) + 1;
                }
                case "getHeightmapPos": {
                    Heightmap.Types type = (Heightmap.Types) args[0];
                    BlockPos pos = (BlockPos) args[1];
                    // LevelReader.getHeightmapPos 调 this.getHeight，即 WorldGenRegion.getHeight 的 +1 版本。
                            // LevelReader.getHeightmapPos calls this.getHeight, i.e. WorldGenRegion.getHeight with +1.
                            return new BlockPos(pos.getX(),
                            this.chunk.getHeight(type, pos.getX() & 15, pos.getZ() & 15) + 1, pos.getZ());
                }
                case "getMinY":
                    return MIN_Y;
                case "getMaxY":
                    return MIN_Y + HEIGHT - 1;
                case "getSeaLevel":
                    return 63;
                case "getChunk":
                    return this.chunk;
                case "getBlockEntity":
                    return null;
                case "markPosForPostProcessing":
                case "scheduleTick":
                    return null;
                case "getLevel":
                case "getMinecraftWorld":
                    // 实体路径要真实 ServerLevel；返回 proxy 让后续调用抛异常并被捕获。
                    // The entity path needs a real ServerLevel; the proxy makes it throw.
                    return proxy;
                case "isClientSide":
                    return false;
                case "getRandom":
                    // `WorldGenRegion.getRandom()`：
                    // `randomState.getOrCreateRandomFactory("worldgen_region_random").at(center)`。
                    // 这里照同一条 Legacy 位置派生复算。
                    // `WorldGenRegion.getRandom()`:
                    // `randomState.getOrCreateRandomFactory("worldgen_region_random").at(center)`,
                    // recomputed with the same Legacy positional derivation.
                    return regionRandom(this.seed, this.center);
                case "getSeed":
                    return this.seed;
                case "getCenter":
                    return this.chunk.getPos();
                case "getBiomeManager":
                    return new BiomeManager(null, 0L);
                case "toString":
                    return "PieceLevelHandler[" + this.chunk.getPos() + "]";
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                default:
                    throw new UnsupportedOperationException(
                            "WorldGenLevel." + name + " is not implemented");
            }
        }
    }
}
