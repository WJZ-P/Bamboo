import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
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
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.Mirror;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.GenerationStep;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.TerrainAdjustment;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;
import net.minecraft.world.level.levelgen.structure.structures.IglooPieces;
import net.minecraft.world.level.levelgen.structure.structures.OceanRuinPieces;
import net.minecraft.world.level.levelgen.structure.structures.OceanRuinStructure;
import net.minecraft.world.level.levelgen.structure.structures.RuinedPortalPiece;
import net.minecraft.world.level.levelgen.structure.structures.ShipwreckPieces;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.storage.LevelStorageSource;

/**
 * 在合成地形的假世界里跑四个模板结构的官方 {@code postProcess}，逐方块 dump 写入日志。
 *
 * <p>不启动服务端、不读存档、不接受 EULA、不加载 worldgen 注册表。地形完全由
 * {@link #surfaceY} 决定（西半是陆地、东半是海），Rust 侧用同一条公式造一样的区块。
 *
 * <p>queries.txt 每行 {@code kind seed cx cz extra}，{@code extra} 只给废弃传送门用
 * （{@code template|rotation|mirror|placement|properties|originX|originY|originZ}）。
 * 输出每个查询一段：{@code Q} 头 + 若干 {@code B x y z state} 行。
 *
 * <p>Runs the official {@code postProcess} of the four template structures over synthetic
 * terrain and dumps the write log. The terrain is the {@link #surfaceY} formula (land to
 * the west, ocean to the east) that the Rust side reproduces exactly.
 *
 * <p>Usage: StructureTemplatedPlaceOracle &lt;queries.txt&gt;
 */
public final class StructureTemplatedPlaceOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final int SEA_LEVEL = 63;
    private static final int GENERATION_HEIGHT = 90;

    private static StructureTemplateManager templateManager;

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: StructureTemplatedPlaceOracle <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // 溺尸和矿车一样是实体，假世界造不出来；把 factory 换成返回 null，让 `create` 走
        // 「创建失败」分支（官方语义），方块部分不受影响。
        // Drowned are entities the fake world cannot host; swap the factory to return null so
        // `create` takes the vanilla "creation failed" branch, leaving blocks untouched.
        disableEntityCreation();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        StringBuilder out = new StringBuilder();
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            Path temp = Files.createTempDirectory("bamboo-templated-place");
            LevelStorageSource.LevelStorageAccess storage =
                    LevelStorageSource.createDefault(temp).createAccess("templated");
            templateManager = new StructureTemplateManager(resources, storage,
                    DataFixers.getDataFixer(), BuiltInRegistries.BLOCK);
            for (String line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
                if (line.isBlank()) {
                    continue;
                }
                appendCase(registries, line, out);
            }
            storage.close();
        }
        System.out.print(out);
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

    private static void appendCase(RegistryAccess.Frozen registries, String line,
            StringBuilder out) throws Exception {
        String[] p = line.trim().split("\\s+");
        String kind = p[0];
        long seed = Long.parseLong(p[1]);
        int cx = Integer.parseInt(p[2]);
        int cz = Integer.parseInt(p[3]);
        ChunkPos pos = new ChunkPos(cx, cz);

        List<StructurePiece> pieces = buildPieces(kind, seed, cx, cz, p);
        Level level = new Level(pos, registries, seed);
        int aborted = 0;
        // `StructureStart.placeInChunk` 的编排：3×3 每个区块各调一次，`chunkBB` 是该区块的
        // 整列盒，`random` 每区块重播，世界在调用之间保留。`CappedProcessor` 每个区块各挑
        // 一批最多 5 个可疑方块，`updateShape` 也能看到前面区块写下的方块。
        // The `StructureStart.placeInChunk` orchestration: once per chunk of the 3×3 with
        // that chunk's column box and a reseeded random, keeping the world between calls.
        // `CappedProcessor` picks up to five blocks per chunk, and `updateShape` sees the
        // earlier chunks' writes.
        for (int dz = -1; dz <= 1; dz++) {
            for (int dx = -1; dx <= 1; dx++) {
                ChunkPos at = new ChunkPos(cx + dx, cz + dz);
                for (StructurePiece piece : pieces) {
                    if (!piece.getBoundingBox().intersects(chunkBox(at))) {
                        continue;
                    }
                    WorldgenRandom random =
                            new WorldgenRandom(new LegacyRandomSource(seed));
                    try {
                        piece.postProcess(level.proxy(), null, null, random, chunkBox(at), at,
                                new BlockPos(at.getMiddleBlockX(),
                                        piece.getBoundingBox().minY(), at.getMiddleBlockZ()));
                    } catch (Throwable error) {
                        aborted++;
                        System.out.println("T " + kind + " " + seed + " " + cx + " " + cz + " "
                                + error.getClass().getName());
                        java.io.StringWriter sw = new java.io.StringWriter();
                        error.printStackTrace(new java.io.PrintWriter(sw));
                        for (String frame : sw.toString().split("\\n")) {
                            System.out.println("U " + frame.trim());
                        }
                    }
                }
            }
        }
        TreeMap<BlockPos, BlockState> written = level.written;
        out.append("Q ").append(kind).append(' ').append(seed).append(' ').append(cx)
                .append(' ').append(cz).append(' ').append(written.size()).append(' ')
                .append(aborted).append('\n');
        for (var entry : written.entrySet()) {
            BlockPos at = entry.getKey();
            out.append("B ").append(at.getX()).append(' ').append(at.getY()).append(' ')
                    .append(at.getZ()).append(' ')
                    .append(Block.BLOCK_STATE_REGISTRY.getId(entry.getValue())).append('\n');
        }
    }

    /** 按官方编排造结构件。废弃传送门的参数由查询行给出（布局已单独对拍过）。 */
    private static List<StructurePiece> buildPieces(String kind, long seed, int cx, int cz,
            String[] p) throws Exception {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, cx, cz);
        ChunkPos chunkPos = new ChunkPos(cx, cz);
        BlockPos origin = new BlockPos(chunkPos.getMinBlockX(), GENERATION_HEIGHT,
                chunkPos.getMinBlockZ());
        StructurePiecesBuilder builder = new StructurePiecesBuilder();
        switch (kind) {
            case "igloo" -> {
                Rotation rotation = Rotation.getRandom(random);
                IglooPieces.addPieces(templateManager, origin, rotation, builder, random);
            }
            case "shipwreck_ocean", "shipwreck_beached" -> {
                Rotation rotation = Rotation.getRandom(random);
                ShipwreckPieces.addRandomPiece(templateManager, origin, rotation, builder, random,
                        kind.equals("shipwreck_beached"));
            }
            case "ocean_ruin_warm", "ocean_ruin_cold" -> {
                Rotation rotation = Rotation.getRandom(random);
                OceanRuinStructure structure = new OceanRuinStructure(
                        new Structure.StructureSettings(HolderSet.empty(), Map.of(),
                                GenerationStep.Decoration.SURFACE_STRUCTURES, TerrainAdjustment.NONE),
                        kind.equals("ocean_ruin_cold") ? OceanRuinStructure.Type.COLD
                                : OceanRuinStructure.Type.WARM,
                        0.3F, 0.9F);
                OceanRuinPieces.addPieces(templateManager, origin, rotation, builder, random,
                        structure);
            }
            case "ruined_portal" -> builder.addPiece(portalPiece(p));
            default -> throw new IllegalArgumentException("unknown kind " + kind);
        }
        return builder.build().pieces();
    }

    /**
     * {@code extra} 的格式：{@code template|rotation|mirror|placement|cold:mossiness:air:over:vine:black|ox|oy|oz}。
     *
     * <p>The {@code extra} format used for the ruined portal query.
     */
    private static StructurePiece portalPiece(String[] p) {
        String[] fields = p[4].split("\\|");
        String templateName = fields[0];
        Rotation rotation = Rotation.values()[Integer.parseInt(fields[1])];
        Mirror mirror = Mirror.values()[Integer.parseInt(fields[2])];
        RuinedPortalPiece.VerticalPlacement placement =
                RuinedPortalPiece.VerticalPlacement.valueOf(fields[3]);
        String[] props = fields[4].split(":");
        RuinedPortalPiece.Properties properties = new RuinedPortalPiece.Properties(
                props[0].equals("1"), Float.parseFloat(props[1]), props[2].equals("1"),
                props[3].equals("1"), props[4].equals("1"), props[5].equals("1"));
        BlockPos origin = new BlockPos(Integer.parseInt(fields[5]), Integer.parseInt(fields[6]),
                Integer.parseInt(fields[7]));
        net.minecraft.resources.Identifier location =
                net.minecraft.resources.Identifier.parse(templateName);
        StructureTemplate template = templateManager.getOrCreate(location);
        BlockPos pivot = new BlockPos(template.getSize().getX() / 2, 0,
                template.getSize().getZ() / 2);
        return new RuinedPortalPiece(null, templateManager, origin, placement, properties, location,
                template, rotation, mirror, pivot);
    }

    /** 一个只回答生成期会读到的那几个问题的 `WorldData` 代理。 */
    private static Object worldDataProxy() {
        return java.lang.reflect.Proxy.newProxyInstance(
                StructureTemplatedPlaceOracle.class.getClassLoader(),
                new Class<?>[] {net.minecraft.world.level.storage.WorldData.class},
                (self, method, args) -> worldDataAnswer(self, method, args));
    }

    /** 同上，`WritableLevelData`（`Level.levelData` 的静态类型）。 */
    private static Object writableLevelDataProxy() {
        return java.lang.reflect.Proxy.newProxyInstance(
                StructureTemplatedPlaceOracle.class.getClassLoader(),
                new Class<?>[] {net.minecraft.world.level.storage.WritableLevelData.class},
                (self, method, args) -> worldDataAnswer(self, method, args));
    }

    private static Object worldDataAnswer(Object self, java.lang.reflect.Method method,
            Object[] args) {
        return switch (method.getName()) {
            case "enabledFeatures" -> net.minecraft.world.flag.FeatureFlags.VANILLA_SET;
            case "getDifficulty" -> net.minecraft.world.Difficulty.NORMAL;
            case "isDifficultyLocked" -> false;
            case "getGameTime" -> 0L;
            case "getDayTime" -> 0L;
            case "toString" -> "TemplatedPlaceWorldData";
            case "hashCode" -> System.identityHashCode(self);
            case "equals" -> self == args[0];
            default -> throw new UnsupportedOperationException(
                    "WorldData." + method.getName() + " is not implemented");
        };
    }

    /** 在 `type` 及其父类里找叫 `name` 的字段。 */
    private static java.lang.reflect.Field findField(Class<?> type, String name)
            throws NoSuchFieldException {
        for (Class<?> owner = type; owner != null; owner = owner.getSuperclass()) {
            try {
                return owner.getDeclaredField(name);
            } catch (NoSuchFieldException ignored) {
                // 继续往父类找。
                // Keep walking up the hierarchy.
            }
        }
        throw new NoSuchFieldException(name);
    }

    /**
     * 把 `DROWNED` 的 `factory` 换成返回 null：假世界没有真实 `ServerLevel`，实体创建会
     * 在 `enabledFeatures()` 上 NPE。换成 null 后官方 `handleDataMarker` 走
     * `if (drowned != null)` 的假分支——方块（空气 / 水）都不写，两侧一致。
     *
     * Replaces `DROWNED`'s factory with one returning null: the fake world has no real
     * `ServerLevel`, so entity creation NPEs on `enabledFeatures()`. With null the vanilla
     * `if (drowned != null)` branch is skipped, writing no block on either side.
     */
    @SuppressWarnings("unchecked")
    private static void disableEntityCreation() {
        try {
            java.lang.reflect.Field factory =
                    net.minecraft.world.entity.EntityType.class.getDeclaredField("factory");
            factory.setAccessible(true);
            factory.set(net.minecraft.world.entity.EntityTypes.DROWNED,
                    (net.minecraft.world.entity.EntityType.EntityFactory<
                                    net.minecraft.world.entity.Entity>)
                            (type, level) -> null);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot stub the drowned factory", error);
        }
    }

    private static BoundingBox chunkBox(ChunkPos pos) {
        return new BoundingBox(pos.getMinBlockX(), MIN_Y + 1, pos.getMinBlockZ(),
                pos.getMaxBlockX(), MIN_Y + HEIGHT - 1, pos.getMaxBlockZ());
    }

    /**
     * `CappedProcessor` 需要 `level.getLevel().getSeed()`：`ServerLevel.getSeed()` 是
     * `this.server.getWorldGenSettings().options().seed()`。这里用
     * `sun.misc.Unsafe.allocateInstance` 造三个**不跑构造器**的实例（`ServerLevel`、
     * `DedicatedServer`——`MinecraftServer` 唯一的非抽象子类、`WorldGenSettings`），只填这条
     * 链上的字段，其余字段保持默认。
     *
     * `CappedProcessor` needs `level.getLevel().getSeed()`, i.e.
     * `server.worldGenSettings().options().seed()`. Three instances are allocated with
     * `sun.misc.Unsafe.allocateInstance` (no constructors), and only the fields on that
     * chain are filled.
     */
    private static Object serverLevelProxy(long seed) {
        try {
            Class<?> unsafeClass = Class.forName("sun.misc.Unsafe");
            java.lang.reflect.Field theUnsafe = unsafeClass.getDeclaredField("theUnsafe");
            theUnsafe.setAccessible(true);
            Object unsafe = theUnsafe.get(null);
            java.lang.reflect.Method allocate =
                    unsafeClass.getMethod("allocateInstance", Class.class);
            Class<?> optionsType = net.minecraft.world.level.levelgen.WorldOptions.class;
            Object options = allocate.invoke(unsafe, optionsType);
            writeField(options, optionsType, "seed", seed);
            Class<?> settingsType = net.minecraft.world.level.levelgen.WorldGenSettings.class;
            Object settings = allocate.invoke(unsafe, settingsType);
            writeField(settings, settingsType, "options", options);
            Class<?> serverType = net.minecraft.server.dedicated.DedicatedServer.class;
            Object server = allocate.invoke(unsafe, serverType);
            writeField(server, serverType, "worldGenSettings", settings);
            // `EntityType.canSpawn` 还会读 `level.enabledFeatures()` →
            // `server.getWorldData().enabledFeatures()`；`WorldData` 是接口，可以用代理。
            // `EntityType.canSpawn` also reads `level.enabledFeatures()`, i.e.
            // `server.getWorldData().enabledFeatures()`; `WorldData` is an interface, so a
            // proxy works.
            Object worldData = worldDataProxy();
            writeField(server, serverType, "worldData", worldData);
            Class<?> levelType = net.minecraft.server.level.ServerLevel.class;
            Object level = allocate.invoke(unsafe, levelType);
            writeField(level, levelType, "server", server);
            // `levelData` 是 `final` 的 `WritableLevelData`，反射写不进，用
            // `Unsafe.putObject` 绕过 final。
            // `levelData` is a `final WritableLevelData`; reflection refuses, so
            // `Unsafe.putObject` writes it.
            Object levelData = writableLevelDataProxy();
            long offset = ((Number) unsafeClass
                    .getMethod("objectFieldOffset", java.lang.reflect.Field.class)
                    .invoke(unsafe, findField(levelType, "levelData")))
                    .longValue();
            unsafeClass.getMethod("putObject", Object.class, long.class, Object.class)
                    .invoke(unsafe, level, offset, levelData);
            return level;
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot allocate a ServerLevel stand-in", error);
        }
    }

    /** 把 `type` 及其父类里叫 `name` 的字段写成 `value`。 */
    private static void writeField(Object target, Class<?> type, String name, Object value)
            throws ReflectiveOperationException {
        for (Class<?> owner = type; owner != null; owner = owner.getSuperclass()) {
            try {
                java.lang.reflect.Field field = owner.getDeclaredField(name);
                field.setAccessible(true);
                field.set(target, value);
                return;
            } catch (NoSuchFieldException ignored) {
                // 继续往父类找。
                // Keep walking up the hierarchy.
            }
        }
        throw new NoSuchFieldException(name);
    }

    /** 合成地形：西半陆地、东半海。和 Rust 的 {@code surface_y} / {@code ocean} 一致。 */
    static int surfaceY(int x, int z) {
        if (x >= 0) {
            return 48 + Math.floorMod(x * 7 + z * 3, 3);
        }
        return 64 + Math.floorMod(x * 31 + z * 17, 5) - 2;
    }

    static boolean isOcean(int x) {
        return x >= 0;
    }

    private static void fillTerrain(ProtoChunk chunk) {
        ChunkPos pos = chunk.getPos();
        for (int x = 0; x < 16; x++) {
            for (int z = 0; z < 16; z++) {
                int worldX = pos.getMinBlockX() + x;
                int worldZ = pos.getMinBlockZ() + z;
                int surface = surfaceY(worldX, worldZ);
                for (int y = MIN_Y; y < surface - 1; y++) {
                    chunk.setBlockState(new BlockPos(worldX, y, worldZ),
                            Blocks.STONE.defaultBlockState(), 0);
                }
                chunk.setBlockState(new BlockPos(worldX, surface - 1, worldZ),
                        Blocks.SANDSTONE.defaultBlockState(), 0);
                chunk.setBlockState(new BlockPos(worldX, surface, worldZ),
                        Blocks.SAND.defaultBlockState(), 0);
                if (isOcean(worldX)) {
                    for (int y = surface + 1; y <= SEA_LEVEL; y++) {
                        chunk.setBlockState(new BlockPos(worldX, y, worldZ),
                                Blocks.WATER.defaultBlockState(), 0);
                    }
                }
            }
        }
    }

    /** 假世界：3×3 ProtoChunk + {@code WorldGenLevel} 代理。 */
    private static final class Level {
        private final ChunkPos center;
        private final Map<Long, ProtoChunk> chunks = new java.util.HashMap<>();
        private final WorldGenLevel proxy;
        private final Holder<Biome> levelBiome;
        private final RegistryAccess.Frozen registries;
        private final PalettedContainerFactory factory;
        final TreeMap<BlockPos, BlockState> written = new TreeMap<>();
        final long seed;
        int aborted = 0;

        Level(ChunkPos center, RegistryAccess.Frozen registries, long seed) {
            this.center = center;
            this.registries = registries;
            this.seed = seed;
            this.levelBiome = registries.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
            this.factory = PalettedContainerFactory.create(registries);
            this.proxy = (WorldGenLevel) Proxy.newProxyInstance(
                    StructureTemplatedPlaceOracle.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class}, new Handler());
        }

        WorldGenLevel proxy() {
            return proxy;
        }

        private ProtoChunk chunk(int cx, int cz) {
            return chunks.computeIfAbsent((((long) cx) << 32) ^ (cz & 0xFFFF_FFFFL), key -> {
                ProtoChunk chunk = new ProtoChunk(new ChunkPos(cx, cz), UpgradeData.EMPTY,
                        LevelHeightAccessor.create(MIN_Y, HEIGHT), factory, null);
                chunk.setPersistedStatus(ChunkStatus.FEATURES);
                fillTerrain(chunk);
                Heightmap.primeHeightmaps(chunk, Set.of(Heightmap.Types.WORLD_SURFACE_WG,
                        Heightmap.Types.OCEAN_FLOOR_WG, Heightmap.Types.MOTION_BLOCKING,
                        Heightmap.Types.MOTION_BLOCKING_NO_LEAVES, Heightmap.Types.WORLD_SURFACE,
                        Heightmap.Types.OCEAN_FLOOR));
                return chunk;
            });
        }

        private ProtoChunk at(BlockPos pos) {
            return chunk(net.minecraft.core.SectionPos.blockToSectionCoord(pos.getX()),
                    net.minecraft.core.SectionPos.blockToSectionCoord(pos.getZ()));
        }

        private boolean withinWriteZone(BlockPos pos) {
            int dx = net.minecraft.core.SectionPos.blockToSectionCoord(pos.getX()) - center.x();
            int dz = net.minecraft.core.SectionPos.blockToSectionCoord(pos.getZ()) - center.z();
            return Math.abs(dx) <= 1 && Math.abs(dz) <= 1;
        }

        private final class Handler implements InvocationHandler {
            @Override
            public Object invoke(Object self, Method method, Object[] args) {
                String name = method.getName();
                int arity = args == null ? 0 : args.length;
                switch (name) {
                    case "getMinY":
                        return MIN_Y;
                    case "getMaxY":
                        return MIN_Y + HEIGHT - 1;
                    case "getSeaLevel":
                        return SEA_LEVEL;
                    case "getHeight": {
                        if (arity == 0) {
                            return HEIGHT;
                        }
                        Heightmap.Types type = (Heightmap.Types) args[0];
                        int x;
                        int z;
                        if (args[1] instanceof BlockPos pos) {
                            x = pos.getX();
                            z = pos.getZ();
                        } else {
                            x = (Integer) args[1];
                            z = (Integer) args[2];
                        }
                        return at(new BlockPos(x, 0, z)).getHeight(type, x & 15, z & 15) + 1;
                    }
                    case "getHeightmapPos": {
                        Heightmap.Types type = (Heightmap.Types) args[0];
                        BlockPos pos = (BlockPos) args[1];
                        return new BlockPos(pos.getX(),
                                at(pos).getHeight(type, pos.getX() & 15, pos.getZ() & 15) + 1,
                                pos.getZ());
                    }
                    case "getBlockState": {
                        BlockPos pos = (BlockPos) args[0];
                        if (!withinWriteZone(pos)) {
                            return Blocks.AIR.defaultBlockState();
                        }
                        return at(pos).getBlockState(pos);
                    }
                    case "getFluidState": {
                        BlockPos pos = (BlockPos) args[0];
                        if (!withinWriteZone(pos)) {
                            return Blocks.AIR.defaultBlockState().getFluidState();
                        }
                        return at(pos).getFluidState(pos);
                    }
                    case "setBlock":
                    case "setBlockAndUpdate": {
                        BlockPos pos = (BlockPos) args[0];
                        BlockState state = (BlockState) args[1];
                        int flags = name.equals("setBlockAndUpdate") ? 3
                                : arity >= 3 ? (Integer) args[2] : 3;
                        if (!withinWriteZone(pos)) {
                            return false;
                        }
                        at(pos).setBlockState(pos, state, flags);
                        written.put(pos.immutable(), state);
                        return true;
                    }
                    case "getBlockEntity":
                        // 世界生成阶段 ProtoChunk 只有 pending 的 DUMMY tag，读不到方块实体；
                        // 战利品表那一步因此不会取随机数（两侧一致）。
                        // During worldgen a ProtoChunk only has pending DUMMY tags, so the
                        // loot-table step draws nothing (both sides agree).
                        return null;
                    case "getChunk": {
                        if (arity == 1 && args[0] instanceof BlockPos pos) {
                            return at(pos);
                        }
                        return chunk((Integer) args[1], (Integer) args[2]);
                    }
                    case "getRandom":
                        return new LegacyRandomSource(0L);
                    case "getCenter":
                        return center;
                    case "getSeed":
                        return seed;
                    case "getLevelData":
                        return worldDataProxy();
                    case "isClientSide":
                        return false;
                    case "getLevel":
                    case "getMinecraftWorld":
                        // `CappedProcessor.finalizeProcessing` 需要
                        // `level.getLevel().getSeed()`；用 `Unsafe` 分配一个 `DedicatedServer`
                        // （具体的 `MinecraftServer` 子类）并只填 `worldGenSettings`。
                        // `CappedProcessor.finalizeProcessing` needs
                        // `level.getLevel().getSeed()`; an `Unsafe`-allocated `DedicatedServer`
                        // (a concrete `MinecraftServer`) only needs `worldGenSettings` filled.
                        return serverLevelProxy(seed);
                    case "getBiome":
                    case "getNoiseBiome":
                        return levelBiome;
                    case "registryAccess":
                        return registries;
                    case "markPosForPostProcessing":
                    case "scheduleTick":
                    case "updateNeighborsAt":
                    case "updateNeighbourForOutputSignal":
                    case "neighborChanged":
                    case "blockUpdated":
                    case "updateNeighborsAtExceptFromFacing":
                    case "sendBlockUpdated":
                    case "levelEvent":
                    case "playSound":
                    case "addParticle":
                    case "gameEvent":
                    case "destroyBlock":
                    case "removeBlockEntity":
                        // `updateShapeAtEdge` 与 `updateShape` 会调用这些；世界生成阶段它们
                        // 对最终方块没有影响（没有玩家、没有红石），直接吞掉。
                        // `updateShapeAtEdge` and `updateShape` call these; during worldgen
                        // they do not change the resulting blocks (no players, no redstone).
                        return null;
                    case "removeBlock":
                    case "ensureCanWrite":
                    case "isWithinWriteZone":
                        return Boolean.TRUE;
                    case "isEmptyBlock": {
                        BlockPos pos = (BlockPos) args[0];
                        return !withinWriteZone(pos) || at(pos).getBlockState(pos).isAir();
                    }
                    case "isStateAtPosition": {
                        BlockPos pos = (BlockPos) args[0];
                        @SuppressWarnings("unchecked")
                        java.util.function.Predicate<BlockState> predicate =
                                (java.util.function.Predicate<BlockState>) args[1];
                        return predicate.test(at(pos).getBlockState(pos));
                    }
                    case "isFluidAtPosition": {
                        BlockPos pos = (BlockPos) args[0];
                        @SuppressWarnings("unchecked")
                        java.util.function.Predicate<FluidState> predicate =
                                (java.util.function.Predicate<FluidState>) args[1];
                        return predicate.test(at(pos).getFluidState(pos));
                    }
                    case "getBlockIfLoaded":
                    case "getBlockStateIfLoaded":
                        return invoke(self, method, new Object[] {args[0]});
                    case "toString":
                        return "TemplatedPlaceLevel";
                    case "hashCode":
                        return System.identityHashCode(self);
                    case "equals":
                        return self == args[0];
                    default:
                        throw new UnsupportedOperationException(
                                "WorldGenLevel." + name + "/" + arity + " is not implemented");
                }
            }
        }
    }
}
