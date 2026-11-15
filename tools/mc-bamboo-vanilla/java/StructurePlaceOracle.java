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
 * <p>假世界是一个 3×3 的 {@code ProtoChunk}，初始方块由 {@code fill} 参数决定
 * （`stone` / `air` / `stone_with_ceiling`），用来覆盖「内部 / 外部」、「顶上有没有东西」
 * 这些分支。{@code WorldGenLevel} 用 {@code Proxy} 实现，语义照 {@code WorldGenRegion}；
 * 没实现的方法抛异常，让没覆盖到的路径立刻暴露。
 *
 * <p>Rust 写 queries.txt，每条一行
 * {@code <kind> <seed> <cx> <cz> <fill>}；本类按官方编排生成结构件，然后对每个结构件
 * 调 {@code postProcess}（`chunkBB` 取结构件包围盒所在的整个区块），输出
 * {@code Q} 头、{@code L} 行的结构件清单和 {@code B} 行的方块快照。
 *
 * <p>Oracle for {@code postProcess}: builds a fake serviceless {@code WorldGenLevel} over a
 * 3×3 {@code ProtoChunk} grid, runs the official {@code StructurePiece.postProcess}, and
 * dumps every block. Unimplemented level methods throw so uncovered paths surface.
 *
 * <p>Usage: StructurePlaceOracle &lt;queries.txt&gt;
 */
public final class StructurePlaceOracle {
    private static final int SEA_LEVEL = 63;
    private static final int MIN_Y = -64;
    private static final StructurePieceSerializationContext CTX =
            new StructurePieceSerializationContext(null, null, null);
    private static final java.util.Set<String> BASE_KEYS =
            java.util.Set.of("id", "BB", "O", "GD");

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: StructurePlaceOracle <queries.txt>");
        }
        try {
            run(args[0]);
        } catch (Throwable error) {
            error.printStackTrace();
            throw error;
        }
    }

    private static void run(String queriesFile) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // 矿井 `createChest` 会创建矿车实体；本轮不比实体，但它的随机取数必须照取。
        // 把 `EntityTypes.CHEST_MINECART` 的 `factory` 换成返回 null，让 `create` 走
        // 「创建失败」分支（和 `create` 返回 null 的语义一致），方块不受影响。
        // The mineshaft `createChest` spawns a minecart; entities are out of scope but the
        // draws must happen. Swap the `factory` to return null so `create` fails exactly
        // like a failed entity creation, leaving blocks untouched.
        disableMinecartCreation();

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        StringBuilder out = new StringBuilder();
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            for (String line : Files.readAllLines(Path.of(queriesFile), StandardCharsets.UTF_8)) {
                if (line.isBlank()) {
                    continue;
                }
                appendCase(line, registries, out);
            }
        }
        System.out.print(out);
    }

    @SuppressWarnings("unchecked")
    private static void disableMinecartCreation() {
        try {
            Object type = net.minecraft.world.entity.EntityTypes.CHEST_MINECART;
            java.lang.reflect.Field factory =
                    net.minecraft.world.entity.EntityType.class.getDeclaredField("factory");
            factory.setAccessible(true);
            factory.set(type, (net.minecraft.world.entity.EntityType.EntityFactory<net.minecraft.world.entity.Entity>)
                    (t, level) -> null);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot stub the minecart factory", error);
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
            // 和 `StructureStart.placeInChunk` 一样：对 3×3 每个区块各调一次，
            // `chunkBB` 是该区块的整列盒，`random` 每个区块重新播种。
            // Same as `StructureStart.placeInChunk`: once per chunk with that chunk's
            // full column box, reseeding the random per chunk.
            int aborted = 0;
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    ChunkPos at = new ChunkPos(cx + dx, cz + dz);
                    BoundingBox chunkBB = chunkBox(at);
                    WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
                    random.setLargeFeatureSeed(seed ^ 0x5DEECE66DL, at.x(), at.z());
                    for (StructurePiece piece : pieces) {
                        if (!piece.getBoundingBox().intersects(chunkBB)) {
                            continue;
                        }
                        BlockPos referencePos = new BlockPos(at.getMiddleBlockX(),
                                piece.getBoundingBox().minY(), at.getMiddleBlockZ());
                        try {
                            piece.postProcess(level.proxy(), null, null, random, chunkBB, at,
                                    referencePos);
                        } catch (NullPointerException error) {
                            // 只有矿井 `createChest` 的矿车会走到这里：假世界没有真实
                            // `ServerLevel`。记一次中断，不假装这次覆盖了剩余方块。
                            // Only the mineshaft `createChest` minecart reaches this: the fake
                            // world has no real `ServerLevel`. Count it as an aborted piece
                            // instead of pretending the rest of it was covered.
                            if (error.getStackTrace().length == 0
                                    || !error.getStackTrace()[0].toString()
                                            .contains("EntityType")) {
                                throw error;
                            }
                            aborted++;
                            if (random != null) {
                                // 中断后这条结构件的剩余取数不再发生，和官方不同；
                                // 记录后跳过，不让它污染后续区块的随机序列。
                                // After aborting, the rest of this piece's draws are skipped,
                                // unlike vanilla; recorded and skipped so later chunks' random
                                // sequences are not polluted.
                                random.setLargeFeatureSeed(seed ^ 0x5DEECE66DL, at.x(), at.z());
                            }
                        }
                    }
                }
            }
            out.append("A ").append(aborted).append('\n');
            out.append("B ").append(level.snapshot().size()).append('\n');            for (String block : level.snapshot()) {
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
        private final ChunkPos center;
        private final java.util.Map<Long, ProtoChunk> chunks = new java.util.HashMap<>();
        private final WorldGenLevel proxy;
        private final Holder<Biome> levelBiome;
        private final RegistryAccess.Frozen registries;
        private final String fill;
        /** 所有写过的位置，按首次写入顺序。 */
        private final List<BlockPos> written = new ArrayList<>();
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

        /** 只输出「和初始填充不同」的方块，按 x、y、z 排序；同一位置取最后一次写入。 */
        List<String> snapshot() {
            // 同一位置可能被写多次：用位置做 key 保留最后一次，避免比较器把它当重复项。
            // A position can be written more than once: key by position so the last write
            // wins instead of being deduplicated away by the comparator.
            java.util.TreeMap<String, String> out = new java.util.TreeMap<>((a, b) -> {
                String[] pa = a.split(" ");
                String[] pb = b.split(" ");
                int c = Integer.compare(Integer.parseInt(pa[0]), Integer.parseInt(pb[0]));
                if (c != 0) {
                    return c;
                }
                c = Integer.compare(Integer.parseInt(pa[1]), Integer.parseInt(pb[1]));
                if (c != 0) {
                    return c;
                }
                return Integer.compare(Integer.parseInt(pa[2]), Integer.parseInt(pb[2]));
            });
            for (BlockPos pos : written) {
                BlockState state = at(pos).getBlockState(pos);
                String key = pos.getX() + " " + pos.getY() + " " + pos.getZ();
                // air 家族之间的区分在 `ProtoChunk` 里不可靠：`PalettedContainer` 用
                // `state.isAir()` 判定，cave_air / void_air 都算空气，单值调色板不会真的
                // 落盘。结构件写空气只为挖空，挖空后是 air 还是 cave_air 对结果没有意义，
                // 所以对拍只比非空气方块。
                // `ProtoChunk` cannot reliably tell air family members apart:
                // `PalettedContainer` tests `state.isAir()`, so cave_air and void_air count
                // as air and a single-value palette never stores them. Pieces write air only
                // to carve space, so the oracle compares non-air blocks only.
                if (state.isAir()) {
                    out.remove(key);
                    continue;
                }
                if (!name(state).equals(initialName())) {
                    out.put(key, key + " " + name(state));
                } else {
                    out.remove(key);
                }
            }
            return new ArrayList<>(out.values());
        }

        private String initialName() {
            return fill.equals("air") ? "Block{minecraft:air}"
                    : "Block{minecraft:stone}";
        }

        private static String name(BlockState state) {
            return state.toString();
        }

        private ProtoChunk chunk(int cx, int cz) {
            return chunks.computeIfAbsent((((long) cx) << 32) ^ (cz & 0xFFFF_FFFFL), key -> {
                ProtoChunk chunk = new ProtoChunk(new ChunkPos(cx, cz), UpgradeData.EMPTY,
                        LevelHeightAccessor.create(MIN_Y, 384), factory, null);
                chunk.setPersistedStatus(ChunkStatus.TERRAIN);
                fillChunk(chunk);
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

        private void fillChunk(ProtoChunk chunk) {
            if (fill.equals("air")) {
                return;
            }
            int last = fill.equals("stone_with_ceiling") ? 63 : 62;
            for (int y = 56; y <= last; y++) {
                for (int x = 0; x < 16; x++) {
                    for (int z = 0; z < 16; z++) {
                        var pos = new BlockPos(chunk.getPos().getMinBlockX() + x, y,
                                chunk.getPos().getMinBlockZ() + z);
                        chunk.setBlockState(pos, Blocks.STONE.defaultBlockState(), 0);
                    }
                }
            }
        }

        private final class Handler implements InvocationHandler {
            @Override
            public Object invoke(Object self, Method method, Object[] args) throws Throwable {
                String name = method.getName();
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
                    case "setBlock": {
                        BlockPos pos = (BlockPos) args[0];
                        BlockState state = (BlockState) args[1];
                        int flags = arity >= 3 ? (Integer) args[2] : 3;
                        if (!withinWriteZone(pos)) {
                            return false;
                        }
                        ProtoChunk chunk = at(pos);
                        written.add(pos.immutable());
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
                        return invoke(self, method, new Object[] {args[0], args[1], 3});
                    case "removeBlock":
                        return invoke(self, method,
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
                    case "getBlockEntity":
                        return at((BlockPos) args[0]).getBlockEntity((BlockPos) args[0]);
                    case "scheduleTick":
                        // `placeBlock` 写含流体的方块后会给它排一个 0 刻的 tick。假世界不做
                        // 计划刻，方块本身已经写进去了，直接返回。
                        // `placeBlock` schedules a zero-delay tick after writing a block that
                        // holds fluid. The fake world runs no ticks; the block is already in,
                        // so this is a no-op.
                        return null;
                    case "markPosForPostProcessing":
                        at((BlockPos) args[0]).markPosForPostProcessing((BlockPos) args[0]);
                        return null;
                    case "getCenter":
                        return center;
                    case "getSeed":
                        return 0L;
                    case "isClientSide":
                        return false;
                    case "getLevel":
                        // `createChest` 会 `EntityTypes.CHEST_MINECART.create(level.getLevel(), ...)`，
                        // 那需要真实 `ServerLevel`。假世界给 null 让它 NPE；oracle 捕获后
                        // 把这条结构件标为「中断」，不假装覆盖了那一小段（见汇报）。
                        // `createChest` spawns a minecart through `level.getLevel()`, which needs
                        // a real `ServerLevel`. The fake world returns null so it NPEs; the
                        // oracle catches that, marks the piece aborted and reports it.
                        return null;
                    case "getBiome":
                    case "getNoiseBiome":
                        return levelBiome;
                    case "registryAccess":
                        return registries;
                    case "getMinBuildHeight":
                        return MIN_Y;
                    case "getMaxBuildHeight":
                        return MIN_Y + 384;
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
