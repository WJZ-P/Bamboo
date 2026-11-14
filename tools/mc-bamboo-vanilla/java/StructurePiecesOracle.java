import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.TreeMap;
import java.util.stream.Collectors;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.Tag;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;
import net.minecraft.world.level.levelgen.structure.structures.MineshaftPieces;
import net.minecraft.world.level.levelgen.structure.structures.MineshaftStructure;
import net.minecraft.world.level.levelgen.structure.structures.OceanMonumentPieces;
import net.minecraft.world.level.levelgen.structure.structures.StrongholdPieces;

/**
 * 用官方 26.3 的结构件类直接对拍四个多件式结构的「结构件列表」布局。
 *
 * <p>不启动服务端、不读存档、不接受 EULA、不加载 worldgen 注册表：只需要
 * {@code Bootstrap.bootStrap()} 让 {@code BuiltInRegistries.STRUCTURE_PIECE} 就绪。
 * 每条查询自己用 {@code setLargeFeatureSeed(seed, cx, cz)} 播一个 WorldgenRandom，
 * 再照各 {@code Structure.findGenerationPoint} / {@code generatePieces} 的编排
 * 调用官方结构件类，最后用 {@code createTag} 把每个结构件按统一格式 dump 出来。
 * 两边的随机源、结构件类型、包围盒、朝向、genDepth 和子类参数逐项比较。
 *
 * <p>queries.txt 每行 "{@code kind seed cx cz arg}"，kind 见下，arg 是喂给布局的
 * 外部输入（恶地的 surfaceHeight；其余为 0）。输出每个查询一段：{@code Q} 头带
 * 结构件数，随后若干 {@code P} 行。
 *
 * <p>Compare the piece-list layout of the four code-built multi-piece structures
 * against vanilla 26.3, calling the official piece classes directly. No server,
 * save, EULA or worldgen registry load; only {@code Bootstrap.bootStrap()}. Each
 * query seeds its own WorldgenRandom via {@code setLargeFeatureSeed} and mirrors
 * the {@code findGenerationPoint} / {@code generatePieces} orchestration. Pieces
 * are dumped via {@code createTag} in one canonical format.
 *
 * <p>Usage: StructurePiecesOracle &lt;queries.txt&gt;
 */
public final class StructurePiecesOracle {
    private static final int SEA_LEVEL = 63;
    private static final int MIN_Y = -64;
    // 和 Rust 侧一致的重试安全上限；正常种子远在此之前命中传送门房间。
    // Retry safety cap matching the Rust side; normal seeds hit the portal room well before it.
    private static final int MAX_TRIES = 128;

    // createTag 里由基类写的四个键，不进 extras。
    // Four base keys written by StructurePiece.createTag; excluded from extras.
    private static final java.util.Set<String> BASE_KEYS =
            java.util.Set.of("id", "BB", "O", "GD");

    private static final StructurePieceSerializationContext CTX =
            new StructurePieceSerializationContext(null, null, null);

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: StructurePiecesOracle <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\s+");
            String kind = p[0];
            long seed = Long.parseLong(p[1]);
            int cx = Integer.parseInt(p[2]);
            int cz = Integer.parseInt(p[3]);
            int arg = Integer.parseInt(p[4]);
            List<String> pieces = switch (kind) {
                case "mineshaft_normal" -> mineshaft(seed, cx, cz, false, arg);
                case "mineshaft_mesa" -> mineshaft(seed, cx, cz, true, arg);
                case "stronghold" -> stronghold(seed, cx, cz);
                case "ocean_monument" -> oceanMonument(seed, cx, cz);
                default -> throw new IllegalArgumentException("unknown kind " + kind);
            };
            out.append("Q ").append(kind).append(' ').append(seed).append(' ').append(cx)
                    .append(' ').append(cz).append(' ').append(arg).append(' ')
                    .append(pieces.size()).append('\n');
            for (String piece : pieces) {
                out.append(piece).append('\n');
            }
        }
        System.out.print(out);
    }

    /** MineshaftStructure.findGenerationPoint + generatePiecesAndAdjust 的编排。 */
    private static List<String> mineshaft(long seed, int cx, int cz, boolean mesa, int surfaceHeight) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, cx, cz);
        random.nextDouble();
        ChunkPos chunkPos = new ChunkPos(cx, cz);
        MineshaftStructure.Type type = mesa ? MineshaftStructure.Type.MESA : MineshaftStructure.Type.NORMAL;
        StructurePiecesBuilder builder = new StructurePiecesBuilder();
        MineshaftPieces.MineShaftRoom room = new MineshaftPieces.MineShaftRoom(
                0, random, chunkPos.getBlockX(2), chunkPos.getBlockZ(2), type);
        builder.addPiece(room);
        room.addChildren(room, builder, random);

        if (mesa) {
            BlockPos center = builder.getBoundingBox().getCenter();
            int targetY = surfaceHeight <= SEA_LEVEL
                    ? SEA_LEVEL : Mth.randomBetweenInclusive(random, SEA_LEVEL, surfaceHeight);
            builder.offsetPiecesVertically(targetY - center.getY());
        } else {
            builder.moveBelowSeaLevel(SEA_LEVEL, MIN_Y, random, 10);
        }

        return builder.build().pieces().stream().map(StructurePiecesOracle::dump).collect(Collectors.toList());
    }

    /** StrongholdStructure.generatePieces 的重试循环编排（全用公开成员复刻）。 */
    private static List<String> stronghold(long seed, int cx, int cz) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        ChunkPos chunkPos = new ChunkPos(cx, cz);
        StructurePiecesBuilder builder = new StructurePiecesBuilder();
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
                int pos = random.nextInt(pending.size());
                StructurePiece piece = pending.remove(pos);
                piece.addChildren(startRoom, builder, random);
            }
            builder.moveBelowSeaLevel(SEA_LEVEL, MIN_Y, random, 10);
            if (!builder.isEmpty() && startRoom.portalRoomPiece != null) {
                break;
            }
            if (tries >= MAX_TRIES) {
                break;
            }
        }
        return builder.build().pieces().stream().map(StructurePiecesOracle::dump).collect(Collectors.toList());
    }

    /** OceanMonumentStructure.createTopPiece + MonumentBuilding；childPieces 走反射列出。 */
    private static List<String> oceanMonument(long seed, int cx, int cz) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, cx, cz);
        int west = cx * 16 - 29;
        int north = cz * 16 - 29;
        Direction orientation = Direction.Plane.HORIZONTAL.getRandomDirection(random);
        OceanMonumentPieces.MonumentBuilding building =
                new OceanMonumentPieces.MonumentBuilding(random, west, north, orientation);
        StructurePiecesBuilder builder = new StructurePiecesBuilder();
        builder.addPiece(building);
        List<String> out = new ArrayList<>();
        for (StructurePiece p : builder.build().pieces()) {
            out.add(dump(p));
        }
        try {
            Field field = OceanMonumentPieces.MonumentBuilding.class.getDeclaredField("childPieces");
            field.setAccessible(true);
            for (Object child : (List<?>) field.get(building)) {
                out.add(dump((StructurePiece) child));
            }
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("cannot read MonumentBuilding.childPieces", e);
        }
        return out;
    }

    /** 把一个结构件按统一格式 dump 成一行。 */
    private static String dump(StructurePiece piece) {
        String id = BuiltInRegistries.STRUCTURE_PIECE.getKey(piece.getType()).toString();
        BoundingBox bb = piece.getBoundingBox();
        Direction orientation = piece.getOrientation();
        int o = orientation == null ? -1 : orientation.get2DDataValue();
        int gd = piece.getGenDepth();
        int rot = piece.getRotation() == null ? -1 : piece.getRotation().ordinal();
        int mirror = piece.getMirror() == null ? -1 : piece.getMirror().ordinal();

        CompoundTag tag = piece.createTag(CTX);
        TreeMap<String, String> extras = new TreeMap<>();
        for (var entry : tag.entrySet()) {
            String key = entry.getKey();
            if (!BASE_KEYS.contains(key)) {
                extras.put(key, value(entry.getValue()));
            }
        }
        String extraToken = extras.isEmpty() ? "-"
                : extras.entrySet().stream()
                        .map(e -> e.getKey() + "=" + e.getValue())
                        .collect(Collectors.joining(","));

        return "P " + id + " " + bb.minX() + " " + bb.minY() + " " + bb.minZ() + " "
                + bb.maxX() + " " + bb.maxY() + " " + bb.maxZ() + " " + o + " " + gd + " "
                + rot + " " + mirror + " " + extraToken;
    }

    /** createTag 里 extras 的值统一成便于 Rust 复现的标准串。 */
    private static String value(Tag tag) {
        if (tag instanceof IntArrayTag array) {
            return joinInts(array.getAsIntArray());
        }
        if (tag instanceof ListTag list) {
            StringBuilder sb = new StringBuilder();
            for (Tag element : list) {
                if (!sb.isEmpty()) {
                    sb.append('/');
                }
                sb.append(value(element));
            }
            return sb.toString();
        }
        Optional<Number> number = tag.asNumber();
        if (number.isPresent()) {
            return Long.toString(number.get().longValue());
        }
        Optional<String> string = tag.asString();
        if (string.isPresent()) {
            return string.get();
        }
        return "?";
    }

    private static String joinInts(int[] ints) {
        StringBuilder sb = new StringBuilder();
        for (int value : ints) {
            if (!sb.isEmpty()) {
                sb.append('_');
            }
            sb.append(value);
        }
        return sb.toString();
    }
}
