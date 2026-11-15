import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
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
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.PalettedContainerRO;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.pieces.PiecesContainer;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;
import net.minecraft.world.level.levelgen.structure.structures.MineshaftPieces;
import net.minecraft.world.level.levelgen.structure.structures.MineshaftStructure;
import net.minecraft.world.level.levelgen.structure.structures.StrongholdPieces;
import net.minecraft.world.level.material.FluidState;

/**
 * 对拍结构件的 {@code postProcess}：在 JVM 里造一个不启动服务端的假 {@code WorldGenLevel}，
 * 用官方 {@code StructurePiece.postProcess} 写方块，再逐方块输出。
 *
 * <p>假世界是一张按需创建的 {@code ProtoChunk} 表，初始方块由 {@code fill} 参数决定
 * （`air` / `band` / `stone` / `caves`），用来覆盖「内部 / 外部」、支撑、柱子 / 铁链、
 * 液体这些分支。{@code WorldGenLevel} 用 {@code Proxy} 实现，语义照 {@code WorldGenRegion}；
 * 没实现的方法抛异常，让没覆盖到的路径立刻暴露。
 *
 * <p>Rust 写 queries.txt，每条一行
 * {@code <kind> <seed> <cx> <cz> <fill>}；本类按官方编排生成结构件，然后对每个结构件
 * 在它碰到的每个区块里调一次 {@code postProcess}，输出 {@code Q} 头、{@code L} 行的
 * 结构件清单、{@code P} 行的逐次调用写入次数（设了 `STRUCTURE_PLACE_TRACE` 时还有
 * {@code D} 行的逐次写入）和 {@code B} 之后每个写过的位置的最终状态。
 *
 * <p>Oracle for {@code postProcess}: builds a fake serviceless {@code WorldGenLevel} over
 * lazily created {@code ProtoChunk}s, runs the official {@code StructurePiece.postProcess}
 * once per chunk each piece touches, and dumps the piece list, every invocation's write
 * count (plus its ordered writes under `STRUCTURE_PLACE_TRACE`) and the final state of
 * every written position. Unimplemented level methods throw so uncovered paths surface.
 *
 * <p>Usage: StructurePlaceOracle &lt;queries.txt&gt;
 */
public final class StructurePlaceOracle {
    private static final int SEA_LEVEL = 63;
    private static final int MIN_Y = -64;
    private static final StructurePieceSerializationContext CTX =
            new StructurePieceSerializationContext(null, null, null);
    private static net.minecraft.server.level.ServerLevel serverLevel;
    /** 林地府邸喂入的 `startY`，必须和 Rust 侧 `MANSION_START_Y` 一致。 */
    // The `startY` fed to the woodland mansion; must match Rust's `MANSION_START_Y`.
    private static final int MANSION_START_Y = 64;
    /** 林地府邸的模板要 `StructureTemplateManager` 来读；只建一次。 */
    // The mansion's templates come from a `StructureTemplateManager`, built once.
    private static net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager
            templateManager;
    /** `WoodlandMansionStructure`，只为调它的 `afterPlace`。 */
    // The `WoodlandMansionStructure`, only for its `afterPlace`.
    private static net.minecraft.world.level.levelgen.structure.Structure mansionStructure;
    private static final java.util.Set<String> BASE_KEYS =
            java.util.Set.of("id", "BB", "O", "GD");

    public static void main(String[] args) throws Exception {
        System.err.println("PLACE oracle start");
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: StructurePlaceOracle <queries.txt>");
        }
        // `Bootstrap.bootStrap()` 会把 `System.err` 接到日志上，之后打印的栈就看不到了；
        // 先把原来的流留下来。
        // `Bootstrap.bootStrap()` reroutes `System.err` into the logger, which hides stack
        // traces printed afterwards; keep the original stream.
        java.io.PrintStream stderr = System.err;
        try {
            run(args[0]);
        } catch (Throwable error) {
            error.printStackTrace(stderr);
            stderr.flush();
            System.exit(1);
        }
    }

    private static void run(String queriesFile) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        serverLevel = installMinecartStub();

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        StringBuilder out = new StringBuilder();
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            Path temp = Files.createTempDirectory("structure-place-stm");
            var storage = net.minecraft.world.level.storage.LevelStorageSource.createDefault(temp)
                    .createAccess("world");
            Files.createDirectories(storage.getLevelPath(
                    net.minecraft.world.level.storage.LevelResource.GENERATED_DIR));
            templateManager =
                    new net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager(
                            resources, storage, net.minecraft.util.datafix.DataFixers.getDataFixer(),
                            registries.lookupOrThrow(Registries.BLOCK));
            mansionStructure = registries.lookupOrThrow(Registries.STRUCTURE)
                    .getOrThrow(net.minecraft.world.level.levelgen.structure.BuiltinStructures
                            .WOODLAND_MANSION)
                    .value();
            for (String line : Files.readAllLines(Path.of(queriesFile), StandardCharsets.UTF_8)) {
                if (line.isBlank()) {
                    continue;
                }
                appendCase(line, registries, out);
            }
        }
        System.out.print(out);
    }

    /** 不走构造器的矿车：只承接 `setInitialPos` / `setLootTable`，不碰实体内部状态。 */
    private static final class InertMinecartChest
            extends net.minecraft.world.entity.vehicle.minecart.MinecartChest {
        private InertMinecartChest() {
            super(null, null);
        }

        @Override
        public void setInitialPos(double x, double y, double z) {
        }
    }

    /** 不走构造器的远古守卫者：`spawnElder` 调到的方法全部变成空操作。 */
    // Constructor-less elder guardian: everything `spawnElder` calls is a no-op.
    private static final class InertElderGuardian
            extends net.minecraft.world.entity.monster.ElderGuardian {
        private InertElderGuardian() {
            super(null, null);
        }

        @Override
        public double getAttributeValue(
                Holder<net.minecraft.world.entity.ai.attributes.Attribute> attribute) {
            return 0.0;
        }

        @Override
        public void heal(float amount) {
        }

        @Override
        public void snapTo(double x, double y, double z, float yRot, float xRot) {
        }

        @Override
        public BlockPos blockPosition() {
            return BlockPos.ZERO;
        }

        @Override
        public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                net.minecraft.world.level.ServerLevelAccessor level,
                net.minecraft.world.DifficultyInstance difficulty,
                net.minecraft.world.entity.EntitySpawnReason reason,
                net.minecraft.world.entity.SpawnGroupData data) {
            return data;
        }
    }

    /** 不走构造器的唤魔者（府邸的 `Mage` 标记）。 */
    // Constructor-less evoker (the mansion's `Mage` marker).
    private static final class InertEvoker extends net.minecraft.world.entity.monster.illager.Evoker {
        private InertEvoker() {
            super(null, null);
        }

        @Override
        public void setPersistenceRequired() {
        }

        @Override
        public void snapTo(BlockPos pos, float yRot, float xRot) {
        }

        @Override
        public BlockPos blockPosition() {
            return BlockPos.ZERO;
        }

        @Override
        public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                net.minecraft.world.level.ServerLevelAccessor level,
                net.minecraft.world.DifficultyInstance difficulty,
                net.minecraft.world.entity.EntitySpawnReason reason,
                net.minecraft.world.entity.SpawnGroupData data) {
            return data;
        }
    }

    /** 不走构造器的卫道士（府邸的 `Warrior` 标记）。 */
    // Constructor-less vindicator (the mansion's `Warrior` marker).
    private static final class InertVindicator extends net.minecraft.world.entity.monster.illager.Vindicator {
        private InertVindicator() {
            super(null, null);
        }

        @Override
        public void setPersistenceRequired() {
        }

        @Override
        public void snapTo(BlockPos pos, float yRot, float xRot) {
        }

        @Override
        public BlockPos blockPosition() {
            return BlockPos.ZERO;
        }

        @Override
        public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                net.minecraft.world.level.ServerLevelAccessor level,
                net.minecraft.world.DifficultyInstance difficulty,
                net.minecraft.world.entity.EntitySpawnReason reason,
                net.minecraft.world.entity.SpawnGroupData data) {
            return data;
        }
    }

    /** 不走构造器的悦灵（府邸的 `Group of Allays` 标记）。 */
    // Constructor-less allay (the mansion's `Group of Allays` marker).
    private static final class InertAllay extends net.minecraft.world.entity.animal.allay.Allay {
        private InertAllay() {
            super(null, null);
        }

        @Override
        public void setPersistenceRequired() {
        }

        @Override
        public void snapTo(BlockPos pos, float yRot, float xRot) {
        }

        @Override
        public BlockPos blockPosition() {
            return BlockPos.ZERO;
        }

        @Override
        public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                net.minecraft.world.level.ServerLevelAccessor level,
                net.minecraft.world.DifficultyInstance difficulty,
                net.minecraft.world.entity.EntitySpawnReason reason,
                net.minecraft.world.entity.SpawnGroupData data) {
            return data;
        }
    }

    /**
     * 把一个 `EntityType` 的 `factory` 换成「不经构造器分配一个惰性子类」。实体不比，但官方
     * 在实体创建成功之后的那些写方块 / 取数必须照常发生（府邸的生物标记会把那一格写成空气）。
     *
     * <p>Swap an `EntityType`'s `factory` for one that allocates an inert subclass without
     * running a constructor. Entities are not compared, but the block writes and draws
     * vanilla makes once the entity exists must still happen (the mansion's mob markers
     * write air at the marker).
     */
    @SuppressWarnings("unchecked")
    private static void installInertMob(Object type, Class<?> inert)
            throws ReflectiveOperationException {
        sun.misc.Unsafe unsafe = unsafe();
        setField(net.minecraft.world.entity.EntityType.class, "factory", type,
                (net.minecraft.world.entity.EntityType.EntityFactory<net.minecraft.world.entity.Entity>)
                        (t, level) -> {
                            try {
                                return (net.minecraft.world.entity.Entity)
                                        unsafe.allocateInstance(inert);
                            } catch (InstantiationException error) {
                                throw new IllegalStateException(error);
                            }
                        });
    }

    private static sun.misc.Unsafe unsafe() throws ReflectiveOperationException {
        java.lang.reflect.Field field = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        return (sun.misc.Unsafe) field.get(null);
    }

    private static void setField(Class<?> owner, String name, Object target, Object value)
            throws ReflectiveOperationException {
        java.lang.reflect.Field field = owner.getDeclaredField(name);
        field.setAccessible(true);
        field.set(target, value);
    }

    /**
     * 让 `EntityTypes.CHEST_MINECART.create(level.getLevel(), ...)` 在没有服务端的情况下
     * 走完：`getLevel()` 给一个不经构造器分配的 `ServerLevel`（只接上 `enabledFeatures()`
     * 那条链），`factory` 换成返回一个不经构造器分配的矿车。这样官方的
     * `chest != null` 分支照常执行，`random.nextLong()` 照常取，结构件不中断。
     *
     * <p>Lets `EntityTypes.CHEST_MINECART.create(level.getLevel(), ...)` complete without a
     * server: `getLevel()` yields a constructor-less `ServerLevel` wired only for
     * `enabledFeatures()`, and the `factory` yields a constructor-less minecart. Vanilla's
     * `chest != null` branch then runs as usual, `random.nextLong()` is drawn and the piece
     * is never aborted.
     */
    @SuppressWarnings("unchecked")
    private static net.minecraft.server.level.ServerLevel installMinecartStub() {
        try {
            sun.misc.Unsafe unsafe = unsafe();
            Object type = net.minecraft.world.entity.EntityTypes.CHEST_MINECART;
            setField(net.minecraft.world.entity.EntityType.class, "factory", type,
                    (net.minecraft.world.entity.EntityType.EntityFactory<net.minecraft.world.entity.Entity>)
                            (t, level) -> {
                                try {
                                    return (net.minecraft.world.entity.Entity)
                                            unsafe.allocateInstance(InertMinecartChest.class);
                                } catch (InstantiationException error) {
                                    throw new IllegalStateException(error);
                                }
                            });
            net.minecraft.world.level.storage.WorldData data =
                    (net.minecraft.world.level.storage.WorldData) Proxy.newProxyInstance(
                            StructurePlaceOracle.class.getClassLoader(),
                            new Class<?>[] {net.minecraft.world.level.storage.WorldData.class},
                            (self, method, args) -> {
                                if (method.getName().equals("enabledFeatures")) {
                                    return net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS;
                                }
                                throw new UnsupportedOperationException(
                                        "WorldData." + method.getName());
                            });
            Object server = unsafe.allocateInstance(
                    net.minecraft.server.dedicated.DedicatedServer.class);
            setField(net.minecraft.server.MinecraftServer.class, "worldData", server, data);
            installInertMob(net.minecraft.world.entity.EntityTypes.ELDER_GUARDIAN,
                    InertElderGuardian.class);
            installInertMob(net.minecraft.world.entity.EntityTypes.EVOKER, InertEvoker.class);
            installInertMob(net.minecraft.world.entity.EntityTypes.VINDICATOR,
                    InertVindicator.class);
            installInertMob(net.minecraft.world.entity.EntityTypes.ALLAY, InertAllay.class);
            Object fakeLevel = unsafe.allocateInstance(
                    net.minecraft.server.level.ServerLevel.class);
            setField(net.minecraft.server.level.ServerLevel.class, "server", fakeLevel, server);
            // `EntityType.create` 先过 `canSpawn`：怪物在和平难度下不建。给一个只回答难度的
            // `levelData`（普通难度），让远古守卫者 / 唤魔者 / 卫道士照常创建。
            // `EntityType.create` goes through `canSpawn`, which refuses monsters on
            // peaceful. Supply a `levelData` that only answers the difficulty (normal) so the
            // elder guardian, evoker and vindicator are created as usual.
            Object levelData = Proxy.newProxyInstance(
                    StructurePlaceOracle.class.getClassLoader(),
                    new Class<?>[] {net.minecraft.world.level.storage.ServerLevelData.class},
                    (self, method, args) -> {
                        if (method.getName().equals("getDifficulty")) {
                            return net.minecraft.world.Difficulty.NORMAL;
                        }
                        throw new UnsupportedOperationException(
                                "LevelData." + method.getName());
                    });
            setField(net.minecraft.world.level.Level.class, "levelData", fakeLevel, levelData);
            return (net.minecraft.server.level.ServerLevel) fakeLevel;
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot stub the minecart path", error);
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

    private static void appendCase(String line, RegistryAccess.Frozen registries,
            StringBuilder out) throws Exception {
        {
            String[] p = line.trim().split("\\s+");
            String kind = p[0];
            long seed = Long.parseLong(p[1]);
            int cx = Integer.parseInt(p[2]);
            int cz = Integer.parseInt(p[3]);
            String fill = p[4];
            ChunkPos pos = new ChunkPos(cx, cz);

            List<StructurePiece> pieces = buildPieces(kind, seed, cx, cz);
            out.append("Q ").append(kind).append(' ').append(seed).append(' ').append(cx)
                    .append(' ').append(cz).append(' ').append(fill).append(' ')
                    .append(pieces.size()).append('\n');
            for (StructurePiece piece : pieces) {
                out.append(pieceLine(piece)).append('\n');
            }

            Level level = new Level(pos, fill, registries);
            // 和 `StructureStart.placeInChunk` 一样：对结构包围盒碰到的每个区块各调一次
            // （先 z 后 x），`chunkBB` 是该区块的整列盒，`random` 每个区块重新播种，
            // 写区是这个区块的 ±1。
            // Same as `StructureStart.placeInChunk`: once per chunk the structure's bounding
            // box touches (z outer, x inner) with that chunk's full column box, reseeding the
            // random per chunk; the write zone is that chunk plus or minus one.
            boolean trace = System.getenv("STRUCTURE_PLACE_TRACE") != null;
            int invocation = 0;
            BoundingBox all = BoundingBox.encapsulatingBoxes(
                    pieces.stream().map(StructurePiece::getBoundingBox)::iterator).orElseThrow();
            for (int chunkZ = SectionPos.blockToSectionCoord(all.minZ());
                    chunkZ <= SectionPos.blockToSectionCoord(all.maxZ()); chunkZ++) {
                for (int chunkX = SectionPos.blockToSectionCoord(all.minX());
                        chunkX <= SectionPos.blockToSectionCoord(all.maxX()); chunkX++) {
                    ChunkPos at = new ChunkPos(chunkX, chunkZ);
                    level.center = at;
                    BoundingBox chunkBB = chunkBox(at);
                    WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
                    random.setLargeFeatureSeed(seed ^ 0x5DEECE66DL, at.x(), at.z());
                    for (int index = 0; index < pieces.size(); index++) {
                        StructurePiece piece = pieces.get(index);
                        if (!piece.getBoundingBox().intersects(chunkBB)) {
                            continue;
                        }
                        BlockPos referencePos = new BlockPos(at.getMiddleBlockX(),
                                piece.getBoundingBox().minY(), at.getMiddleBlockZ());
                        level.beginPiece();
                        piece.postProcess(level.proxy(), null, null, random, chunkBB, at,
                                referencePos);
                        out.append("P ").append(invocation).append(' ').append(index).append(' ')
                                .append(at.x()).append(' ').append(at.z()).append(' ')
                                .append(level.pieceWrites.size()).append('\n');
                        if (trace) {
                            for (String write : level.pieceWrites) {
                                out.append("D ").append(invocation).append(' ').append(write)
                                        .append('\n');
                            }
                        }
                        invocation++;
                    }
                    if (kind.equals("woodland_mansion")) {
                        // `StructureStart.placeInChunk` 末尾的 `structure.afterPlace(...)`，记成
                        // 一次结构件下标为 `pieces.size()` 的调用。
                        // The `structure.afterPlace(...)` at the end of
                        // `StructureStart.placeInChunk`, recorded as one invocation whose piece
                        // index is `pieces.size()`.
                        level.beginPiece();
                        mansionStructure.afterPlace(level.proxy(), null, null, random, chunkBB, at,
                                new PiecesContainer(pieces));
                        out.append("P ").append(invocation).append(' ').append(pieces.size())
                                .append(' ').append(at.x()).append(' ').append(at.z()).append(' ')
                                .append(level.pieceWrites.size()).append('\n');
                        if (trace) {
                            for (String write : level.pieceWrites) {
                                out.append("D ").append(invocation).append(' ').append(write)
                                        .append('\n');
                            }
                        }
                        invocation++;
                    }
                }
            }
            List<String> snapshot = level.snapshot();
            out.append("B ").append(snapshot.size()).append('\n');
            for (String block : snapshot) {
                out.append(block).append('\n');
            }
        }
    }

    private static BoundingBox chunkBox(ChunkPos pos) {
        return new BoundingBox(pos.getMinBlockX(), MIN_Y, pos.getMinBlockZ(),
                pos.getMaxBlockX(), MIN_Y + 384 - 1, pos.getMaxBlockZ());
    }

    /** 官方编排生成结构件（和 StructurePiecesOracle 一致）。 */
    private static List<StructurePiece> buildPieces(String kind, long seed, int cx, int cz) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        ChunkPos chunkPos = new ChunkPos(cx, cz);
        StructurePiecesBuilder builder = new StructurePiecesBuilder();
        switch (kind) {
            case "ocean_monument" -> {
                // `OceanMonumentStructure.createTopPiece`：只加一个 `MonumentBuilding`，
                // 房间图是它的私有字段，`postProcess` 自己遍历 `childPieces`。
                // `OceanMonumentStructure.createTopPiece`: only a `MonumentBuilding` is added;
                // the room graph is a private field its `postProcess` walks itself.
                random.setLargeFeatureSeed(seed, cx, cz);
                Direction orientation = Direction.Plane.HORIZONTAL.getRandomDirection(random);
                builder.addPiece(new net.minecraft.world.level.levelgen.structure.structures
                        .OceanMonumentPieces.MonumentBuilding(random,
                                chunkPos.getMinBlockX() - 29, chunkPos.getMinBlockZ() - 29,
                                orientation));
            }
            case "woodland_mansion" -> {
                // `WoodlandMansionStructure.findGenerationPoint` / `generatePieces`：先取
                // rotation，再 `generateMansion`。`startY` 是喂入的常数。
                // `WoodlandMansionStructure.findGenerationPoint` / `generatePieces`: the
                // rotation is drawn first, then `generateMansion`. `startY` is a fed constant.
                random.setLargeFeatureSeed(seed, cx, cz);
                net.minecraft.world.level.block.Rotation rotation =
                        net.minecraft.world.level.block.Rotation.getRandom(random);
                BlockPos startPos = new BlockPos(chunkPos.getBlockX(7), MANSION_START_Y,
                        chunkPos.getBlockZ(7));
                List<net.minecraft.world.level.levelgen.structure.structures.WoodlandMansionPieces
                        .WoodlandMansionPiece> mansion = new java.util.LinkedList<>();
                net.minecraft.world.level.levelgen.structure.structures.WoodlandMansionPieces
                        .generateMansion(templateManager, startPos, rotation, mansion, random);
                mansion.forEach(builder::addPiece);
            }
            case "mineshaft_normal", "mineshaft_mesa" -> {
                random.setLargeFeatureSeed(seed, cx, cz);
                random.nextDouble();
                MineshaftStructure.Type type = kind.equals("mineshaft_mesa")
                        ? MineshaftStructure.Type.MESA : MineshaftStructure.Type.NORMAL;
                MineshaftPieces.MineShaftRoom room = new MineshaftPieces.MineShaftRoom(
                        0, random, chunkPos.getBlockX(2), chunkPos.getBlockZ(2), type);
                builder.addPiece(room);
                room.addChildren(room, builder, random);
                if (type == MineshaftStructure.Type.MESA) {
                    // 和 Rust 侧同一个 surfaceHeight 输入（0 表示 <= seaLevel，无随机）。
                    // Same surfaceHeight input as Rust (0 means <= seaLevel, no draw).
                    int surfaceHeight = 0;
                    BlockPos center = builder.getBoundingBox().getCenter();
                    int target = surfaceHeight <= SEA_LEVEL ? SEA_LEVEL
                            : net.minecraft.util.Mth.randomBetweenInclusive(
                                    random, SEA_LEVEL, surfaceHeight);
                    builder.offsetPiecesVertically(target - center.getY());
                } else {
                    builder.moveBelowSeaLevel(SEA_LEVEL, MIN_Y, random, 10);
                }
            }
            case "stronghold" -> {
                int tries = 0;
                StrongholdPieces.StartPiece startRoom;
                while (true) {
                    builder.clear();
                    random.setLargeFeatureSeed(seed + tries++, cx, cz);
                    StrongholdPieces.resetPieces();
                    startRoom = new StrongholdPieces.StartPiece(
                            random, chunkPos.getBlockX(2), chunkPos.getBlockZ(2));
                    builder.addPiece(startRoom);
                    startRoom.addChildren(startRoom, builder, random);
                    List<StructurePiece> pending = startRoom.pendingChildren;
                    while (!pending.isEmpty()) {
                        int index = random.nextInt(pending.size());
                        StructurePiece piece = pending.remove(index);
                        piece.addChildren(startRoom, builder, random);
                    }
                    builder.moveBelowSeaLevel(SEA_LEVEL, MIN_Y, random, 10);
                    if (!builder.isEmpty() && startRoom.portalRoomPiece != null) {
                        break;
                    }
                    if (tries >= 128) {
                        break;
                    }
                }
            }
            default -> throw new IllegalArgumentException("unknown kind " + kind);
        }
        return builder.build().pieces();
    }

    /** 一个结构件的清单行：id、六元包围盒、朝向、genDepth、子类键。 */
    private static String pieceLine(StructurePiece piece) {
        BoundingBox bb = piece.getBoundingBox();
        Direction orientation = piece.getOrientation();
        int o = orientation == null ? -1 : orientation.get2DDataValue();
        CompoundTag tag = piece.createTag(CTX);
        TreeMap<String, String> extras = new TreeMap<>();
        for (var entry : tag.entrySet()) {
            if (!BASE_KEYS.contains(entry.getKey())) {
                extras.put(entry.getKey(), value(entry.getValue()));
            }
        }
        String extraToken = extras.isEmpty() ? "-"
                : extras.entrySet().stream()
                        .map(e -> e.getKey() + "=" + e.getValue())
                        .collect(java.util.stream.Collectors.joining(","));
        return "L " + net.minecraft.core.registries.BuiltInRegistries.STRUCTURE_PIECE
                .getKey(piece.getType()) + " " + bb.minX() + " " + bb.minY() + " " + bb.minZ()
                + " " + bb.maxX() + " " + bb.maxY() + " " + bb.maxZ() + " " + o + " "
                + piece.getGenDepth() + " " + extraToken;
    }

    private static String value(net.minecraft.nbt.Tag tag) {
        if (tag instanceof net.minecraft.nbt.IntArrayTag array) {
            StringBuilder sb = new StringBuilder();
            for (int v : array.getAsIntArray()) {
                if (!sb.isEmpty()) {
                    sb.append('_');
                }
                sb.append(v);
            }
            return sb.toString();
        }
        if (tag instanceof net.minecraft.nbt.ListTag list) {
            StringBuilder sb = new StringBuilder();
            for (net.minecraft.nbt.Tag element : list) {
                if (!sb.isEmpty()) {
                    sb.append('/');
                }
                sb.append(value(element));
            }
            return sb.toString();
        }
        var number = tag.asNumber();
        if (number.isPresent()) {
            return switch (tag.getId()) {
                case 1 -> "B:" + number.get().longValue();
                case 2 -> "S:" + number.get().longValue();
                case 3 -> "I:" + number.get().longValue();
                default -> "" + number.get().longValue();
            };
        }
        var string = tag.asString();
        return string.orElse("?");
    }

    /** 一个假世界：无界的 ProtoChunk 映射 + `WorldGenLevel` 代理。 */
    private static final class Level {
        /** 正在生成的区块：`WorldGenRegion` 的中心，写区是它的 ±1。 */
        private ChunkPos center;
        private final java.util.Map<Long, ProtoChunk> chunks = new java.util.HashMap<>();
        private final WorldGenLevel proxy;
        private final Holder<Biome> levelBiome;
        private final RegistryAccess.Frozen registries;
        private final String fill;
        /** 所有写过的位置。`Set` 去重，`snapshot()` 最后还会排序。 */
        // Every written position. A `Set` avoids duplicates; `snapshot()` sorts anyway.
        private final java.util.Set<BlockPos> written = new java.util.HashSet<>();
        /** `placeBlock` 请求过的流体 tick 位置（本 oracle 不 tick，只确认不抛异常）。 */
        private final List<BlockPos> scheduled = new ArrayList<>();
        /** `WorldGenRegion.getRandom()` 的替身，固定种子。 */
        // Stand-in for `WorldGenRegion.getRandom()`, fixed seed.
        private final net.minecraft.util.RandomSource levelRandom =
                net.minecraft.util.RandomSource.create(0L);
        /** 当前这次结构件调用的写入，按顺序：`x y z state`。 */
        private final List<String> pieceWrites = new ArrayList<>();

        void beginPiece() {
            pieceWrites.clear();
        }

        private final PalettedContainerFactory factory;

        Level(ChunkPos center, String fill, RegistryAccess.Frozen registries) {
            this.center = center;
            this.registries = registries;
            this.fill = fill;
            // 矿井的 `isInInvalidLocation` 会查群系；用平原，保证不在
            // `MINESHAFT_BLOCKING` 标签里（官方标签只含深暗之域）。
            // The mineshaft `isInInvalidLocation` reads the biome; use plains so it is
            // outside `MINESHAFT_BLOCKING` (the tag only holds the deep dark).
            this.levelBiome = registries.lookupOrThrow(Registries.BIOME)
                    .getOrThrow(Biomes.PLAINS);
            this.factory = PalettedContainerFactory.create(registries);
            this.proxy = (WorldGenLevel) Proxy.newProxyInstance(
                    StructurePlaceOracle.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class},
                    new Handler());
        }

        WorldGenLevel proxy() {
            return proxy;
        }

        /**
         * 每个写过的位置的最终状态，按 x、y、z 排序。空气家族统一记成 `AIR`：
         * `ProtoChunk.getBlockState` 对全空气的段直接返回 `Blocks.AIR`，读不出 cave_air。
         *
         * <p>Final state of every written position, sorted by x, y, z. The air family is
         * normalised to `AIR`: `ProtoChunk.getBlockState` returns `Blocks.AIR` for an
         * all-air section, so cave_air cannot be read back reliably.
         */
        List<String> snapshot() {
            List<BlockPos> sorted = new ArrayList<>(written);
            sorted.sort(java.util.Comparator.<BlockPos>comparingInt(BlockPos::getX)
                    .thenComparingInt(BlockPos::getY).thenComparingInt(BlockPos::getZ));
            List<String> out = new ArrayList<>(sorted.size());
            for (BlockPos pos : sorted) {
                out.add(pos.getX() + " " + pos.getY() + " " + pos.getZ() + " "
                        + name(at(pos).getBlockState(pos)));
            }
            return out;
        }

        private static String name(BlockState state) {
            return state.isAir() ? "AIR" : state.toString();
        }

        private ProtoChunk chunk(int cx, int cz) {
            return chunks.computeIfAbsent((((long) cx) << 32) ^ (cz & 0xFFFF_FFFFL), key -> {
                ProtoChunk chunk = new ProtoChunk(new ChunkPos(cx, cz), UpgradeData.EMPTY,
                        LevelHeightAccessor.create(MIN_Y, 384), factory, null);
                fillChunk(chunk);
                // 官方在 FEATURES 阶段时区块的持久状态是 TERRAIN，它的 `heightmapsAfter` 是
                // FINAL 那一组：`OCEAN_FLOOR_WG` 停在地形阶段结束时的值，结构件写方块不会再
                // 更新它。这里在填完初始地形后把它算好，再切到 TERRAIN，得到同样的冻结高度图。
                // During FEATURES the chunk's persisted status is TERRAIN, whose
                // `heightmapsAfter` is the FINAL set: `OCEAN_FLOOR_WG` stays at its
                // end-of-terrain value and piece writes never update it. Prime it after the
                // initial fill, then switch to TERRAIN, to get the same frozen heightmap.
                Heightmap.primeHeightmaps(chunk,
                        java.util.EnumSet.of(Heightmap.Types.OCEAN_FLOOR_WG));
                chunk.setPersistedStatus(ChunkStatus.TERRAIN);
                return chunk;
            });
        }

        private ProtoChunk at(BlockPos pos) {
            return chunk(SectionPos.blockToSectionCoord(pos.getX()),
                    SectionPos.blockToSectionCoord(pos.getZ()));
        }

        private boolean withinWriteZone(BlockPos pos) {
            int dx = SectionPos.blockToSectionCoord(pos.getX()) - center.x();
            int dz = SectionPos.blockToSectionCoord(pos.getZ()) - center.z();
            return Math.abs(dx) <= 1 && Math.abs(dz) <= 1;
        }

        /**
         * 初始世界（Rust 侧 `FlatLevel::initial_block` 是同一个函数）：
         * `air` 全空；`band` 只有 y 56..=63 一层石头；`stone` 是 y <= 100 的实心石头；
         * `caves` 在实心石头里按 4×4×4 的格子挖空气洞和少量水洞。
         *
         * <p>The initial world (same function as Rust's `FlatLevel::initial_block`): `air` is
         * empty; `band` is a stone slab at y 56..=63; `stone` is solid stone up to y 100;
         * `caves` carves air pockets and a few water pockets out of that stone on a 4x4x4
         * grid.
         */
        private BlockState initial(int x, int y, int z) {
            switch (fill) {
                case "air":
                    return null;
                case "band":
                    return y >= 56 && y <= 63 ? Blocks.STONE.defaultBlockState() : null;
                case "stone":
                    return y <= 100 ? Blocks.STONE.defaultBlockState() : null;
                case "caves": {
                    if (y > 100) {
                        return null;
                    }
                    int hash = Math.floorDiv(x, 4) * 73856093 ^ Math.floorDiv(y, 4) * 19349663
                            ^ Math.floorDiv(z, 4) * 83492791;
                    int bucket = Math.floorMod(hash, 61);
                    if (bucket < 12) {
                        return null;
                    }
                    return bucket == 12 ? Blocks.WATER.defaultBlockState()
                            : Blocks.STONE.defaultBlockState();
                }
                case "ocean": {
                    // 海：海床高度按 8×8 的格子在 y 30..=45 之间变化（神殿底是 y 39，所以有的
                    // 地方海床埋住神殿底、有的地方悬空），其上到 y 62 是水，部分 4×4 格子的
                    // 水面是冰。
                    // An ocean: the floor varies over y 30..=45 on an 8x8 grid (the monument's
                    // bottom is y 39, so some of it is buried and some hangs free), water above
                    // it up to y 62, with ice on the surface of some 4x4 cells.
                    int floor = 30 + Math.floorMod(
                            Math.floorDiv(x, 8) * 73856093 ^ Math.floorDiv(z, 8) * 83492791, 16);
                    if (y <= floor) {
                        return Blocks.STONE.defaultBlockState();
                    }
                    if (y > 62) {
                        return null;
                    }
                    boolean ice = y == 62 && Math.floorMod(
                            Math.floorDiv(x, 4) * 19349663 ^ Math.floorDiv(z, 4) * 73856093, 5) == 0;
                    return ice ? Blocks.ICE.defaultBlockState() : Blocks.WATER.defaultBlockState();
                }
                default:
                    throw new IllegalArgumentException("unknown fill " + fill);
            }
        }

        private void fillChunk(ProtoChunk chunk) {
            if (fill.equals("air")) {
                return;
            }
            BlockPos.MutableBlockPos pos = new BlockPos.MutableBlockPos();
            for (int y = MIN_Y; y <= 100; y++) {
                for (int x = 0; x < 16; x++) {
                    for (int z = 0; z < 16; z++) {
                        pos.set(chunk.getPos().getMinBlockX() + x, y,
                                chunk.getPos().getMinBlockZ() + z);
                        BlockState state = initial(pos.getX(), y, pos.getZ());
                        if (state != null) {
                            chunk.setBlockState(pos, state, 0);
                        }
                    }
                }
            }
        }

        private final class Handler implements InvocationHandler {
            @Override
            public Object invoke(Object self, Method method, Object[] args) throws Throwable {
                return call(method.getName(), self, method, args);
            }

            // `setBlockAndUpdate` / `removeBlock` 按名字转给 `setBlock`，不能再用同一个 `method`
            // 递归（那样会无限转回自己）。
            // `setBlockAndUpdate` / `removeBlock` forward to `setBlock` by name; recursing with
            // the same `method` would loop back to themselves forever.
            private Object call(String name, Object self, Method method, Object[] args)
                    throws Throwable {
                int arity = args == null ? 0 : args.length;
                switch (name) {
                    case "getMinY":
                        return MIN_Y;
                    case "getMaxY":
                        return MIN_Y + 384 - 1;
                    case "getMinSectionY":
                        return SectionPos.blockToSectionCoord(MIN_Y);
                    case "getMaxSectionY":
                        return SectionPos.blockToSectionCoord(MIN_Y + 384 - 1);
                    case "getSectionsCount":
                        return SectionPos.blockToSectionCoord(MIN_Y + 384 - 1)
                                - SectionPos.blockToSectionCoord(MIN_Y) + 1;
                    case "getSectionIndex":
                        return SectionPos.blockToSectionCoord((Integer) args[0])
                                - SectionPos.blockToSectionCoord(MIN_Y);
                    case "getSectionIndexFromSectionY":
                        return (Integer) args[0] - SectionPos.blockToSectionCoord(MIN_Y);
                    case "getSectionYFromSectionIndex":
                        return (Integer) args[0] + SectionPos.blockToSectionCoord(MIN_Y);
                    case "isOutsideBuildHeight": {
                        int y = args[0] instanceof BlockPos pos ? pos.getY() : (Integer) args[0];
                        return y < MIN_Y || y > MIN_Y + 384 - 1;
                    }
                    case "getSeaLevel":
                        return SEA_LEVEL;
                    case "getHeight": {
                        if (arity == 0) {
                            return MIN_Y + 1;
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
                        // WorldGenRegion.getHeight: ChunkAccess.getHeight(type, x&15, z&15) + 1
                        return at(new BlockPos(x, 0, z)).getHeight(type, x & 15, z & 15) + 1;
                    }
                    case "getBlockState": {
                        // `WorldGenRegion.getBlockState` 读写区外的区块照样返回真实方块。
                        // `WorldGenRegion.getBlockState` returns real blocks outside the
                        // write zone too.
                        BlockPos pos = (BlockPos) args[0];
                        return at(pos).getBlockState(pos);
                    }
                    case "getFluidState": {
                        BlockPos pos = (BlockPos) args[0];
                        return at(pos).getFluidState(pos);
                    }
                    case "setBlock": {
                        BlockPos pos = (BlockPos) args[0];
                        BlockState state = (BlockState) args[1];
                        int flags = arity >= 3 ? (Integer) args[2] : 3;
                        if (!withinWriteZone(pos)) {
                            return false;
                        }
                        ProtoChunk chunk = at(pos);
                        written.add(pos.immutable());
                        // 逐次写入记传给 `setBlock` 的原状态，cave_air 和 air 不合并。
                        // The ordered trace keeps the exact state handed to `setBlock`;
                        // cave_air and air stay distinct here.
                        pieceWrites.add(pos.getX() + " " + pos.getY() + " " + pos.getZ() + " "
                                + state);
                        BlockState old = chunk.setBlockState(pos, state, flags);
                        if (state.hasBlockEntity()) {
                            CompoundTag tag = new CompoundTag();
                            tag.putInt("x", pos.getX());
                            tag.putInt("y", pos.getY());
                            tag.putInt("z", pos.getZ());
                            tag.putString("id", "DUMMY");
                            chunk.setBlockEntityNbt(tag);
                        } else if (old != null && old.hasBlockEntity()) {
                            chunk.removeBlockEntity(pos);
                        }
                        if ((flags & 16) == 0) {
                            BlockPos post = state.getPostProcessPos(chunk, pos);
                            if (post != null) {
                                at(post).markPosForPostProcessing(post);
                            }
                        }
                        return true;
                    }
                    case "setBlockAndUpdate":
                        return call("setBlock", self, method, new Object[] {args[0], args[1], 3});
                    case "removeBlock":
                        return call("setBlock", self, method,
                                new Object[] {args[0], Blocks.AIR.defaultBlockState(), 3});
                    case "ensureCanWrite":
                    case "isWithinWriteZone":
                        return true;
                    case "getChunk": {
                        if (arity == 1 && args[0] instanceof BlockPos pos) {
                            return at(pos);
                        }
                        return chunk((Integer) args[1], (Integer) args[2]);
                    }
                    case "getBlockEntity": {
                        // 照 `WorldGenRegion.getBlockEntity`：`DUMMY` 占位在第一次读取时
                        // 换成真的方块实体，这样 `createChest` 的 `random.nextLong()` 才会取。
                        // As `WorldGenRegion.getBlockEntity`: the `DUMMY` placeholder becomes a
                        // real block entity on first read, so `createChest` draws its
                        // `random.nextLong()`.
                        BlockPos pos = (BlockPos) args[0];
                        ProtoChunk chunk = at(pos);
                        BlockEntity existing = chunk.getBlockEntity(pos);
                        if (existing != null) {
                            return existing;
                        }
                        CompoundTag tag = chunk.getBlockEntityNbt(pos);
                        BlockState state = chunk.getBlockState(pos);
                        if (tag == null || !state.hasBlockEntity()) {
                            return null;
                        }
                        BlockEntity created = ((net.minecraft.world.level.block.EntityBlock)
                                state.getBlock()).newBlockEntity(pos, state);
                        if (created != null) {
                            chunk.setBlockEntity(created);
                        }
                        return created;
                    }
                    case "addFreshEntity":
                        return true;
                    case "addFreshEntityWithPassengers":
                    case "getCurrentDifficultyAt":
                        // 实体不比；这两个只在实体创建之后被调，返回值没人读。
                        // Entities are not compared; these run only after an entity was
                        // created and nobody reads the result.
                        return null;
                    case "getRandom":
                        // `WorldGenRegion.getRandom()` 是这个区块的位置随机源，和传给
                        // `postProcess` 的 `random` 不是同一个。府邸的悦灵数量、生物的
                        // `finalizeSpawn`、`updateShape` 都从它取，不影响写哪些方块。
                        // `WorldGenRegion.getRandom()` is the chunk's positional source, a
                        // different object from the `random` handed to `postProcess`. The
                        // allay count, mob `finalizeSpawn` and `updateShape` draw from it,
                        // none of which changes which blocks are written.
                        return Level.this.levelRandom;
                    case "isEmptyBlock": {
                        BlockPos pos = (BlockPos) args[0];
                        return at(pos).getBlockState(pos).isAir();
                    }
                    case "holderLookup":
                        return registries.lookupOrThrow(
                                (net.minecraft.resources.ResourceKey) args[0]);
                    case "markPosForPostProcessing":
                        at((BlockPos) args[0]).markPosForPostProcessing((BlockPos) args[0]);
                        return null;
                    case "updateNeighborsAt":
                        // `WorldGenRegion` 不覆写，用的是 `LevelAccessor` 的空默认实现。
                        // `WorldGenRegion` keeps `LevelAccessor`'s empty default.
                        return null;
                    case "environmentAttributes":
                        // 放置路径上只有 `LavaFluid.isFastLava` 读它，只影响流体刻的延迟。
                        // Only `LavaFluid.isFastLava` reads it during placement, and only
                        // for the fluid tick delay.
                        return net.minecraft.world.attribute.EnvironmentAttributeReader.EMPTY;
                    case "getRawBrightness":
                        // 生成期区块还没点光：天空光存储里没有这一列，读出 15；方块光是 0。
                        // A generating chunk is not lit yet: the sky storage has no entry
                        // for the column and reads 15; block light reads 0.
                        return Math.max(0, 15 - (int) args[1]);
                    case "getBrightness":
                        return args[0] == net.minecraft.world.level.LightLayer.SKY ? 15 : 0;
                    case "getCenter":
                        return center;
                    case "getSeed":
                        return 0L;
                    case "isClientSide":
                        return false;
                    case "getLevel":
                        return serverLevel;
                    case "getBiome":
                    case "getNoiseBiome":
                        return levelBiome;
                    case "registryAccess":
                        return registries;
                    case "getMinBuildHeight":
                        return MIN_Y;
                    case "getMaxBuildHeight":
                        return MIN_Y + 384;
                    case "scheduleTick": {
                        // `placeBlock` 在 `setBlock` 后若该位置有流体会调它。本 oracle 不 tick，
                        // 调度本身不改方块、不取世界随机数，所以只记录不抛异常。
                        // `placeBlock` calls this when the written position holds a fluid.
                        // This oracle never ticks; scheduling changes no block and draws no
                        // world randomness, so record it instead of throwing.
                        scheduled.add((BlockPos) args[0]);
                        return null;
                    }
                    case "toString":
                        return "StructurePlaceLevel";
                    case "hashCode":
                        return System.identityHashCode(self);
                    case "equals":
                        return self == args[0];
                    default:
                        throw new UnsupportedOperationException("WorldGenLevel." + name
                                + "/" + arity + " is not implemented in StructurePlaceOracle");
                }
            }
        }

    }
}
