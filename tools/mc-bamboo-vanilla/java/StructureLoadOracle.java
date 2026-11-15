import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.Tag;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.pieces.PiecesContainer;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;
import net.minecraft.world.level.levelgen.structure.structures.MineshaftPieces;
import net.minecraft.world.level.levelgen.structure.structures.MineshaftStructure;
import net.minecraft.world.level.levelgen.structure.structures.OceanMonumentPieces;
import net.minecraft.world.level.levelgen.structure.structures.OceanMonumentStructure;
import net.minecraft.world.level.levelgen.structure.structures.StrongholdPieces;

/**
 * 对拍结构存档的读回路径：官方 `PiecesContainer.load` /
 * `OceanMonumentStructure.regeneratePiecesAfterLoad` 对 `structure::save` 的 Rust 移植。
 *
 * <p>Rust 写 queries.txt，每条一行 `kind seed cx cz`。本类对每条查询：
 * <ol>
 *   <li>按官方结构件构造器造出结构件，用 `createTag` 得到存档标签；
 *   <li>`PiecesContainer.load(children)` 读回，把读回的结构件 dump 成 `L` 行；
 *   <li>海底神�殿额外调 `OceanMonumentStructure.regeneratePiecesAfterLoad`，把重建后的
 *       结构件与反射列出的 `childPieces` 也 dump 成 `R` 行。
 * </ol>
 * Rust 侧用生产代码复算，逐行比。只 Bootstrap，不启服务端、不读存档、不接受 EULA。
 *
 * <p>Oracle for the read-back path of structure saving: vanilla
 * `PiecesContainer.load` / `OceanMonumentStructure.regeneratePiecesAfterLoad`
 * against the Rust port in `structure::save`. Each query builds pieces with the
 * official constructors, dumps their `createTag`, reloads them through
 * `PiecesContainer.load`, and (for the ocean monument) dumps the pieces rebuilt by
 * `regeneratePiecesAfterLoad`. Bootstrap only.
 *
 * <p>Usage: StructureLoadOracle &lt;queries.txt&gt;
 */
public final class StructureLoadOracle {
    private static final int SEA_LEVEL = 63;
    private static final int MIN_Y = -64;
    private static final StructurePieceSerializationContext CTX =
            new StructurePieceSerializationContext(null, null, null);
    private static final java.util.Set<String> BASE_KEYS =
            java.util.Set.of("id", "BB", "O", "GD");

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: StructureLoadOracle <queries.txt>");
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

            ListTag children = new ListTag();
            for (StructurePiece piece : buildPieces(kind, seed, cx, cz)) {
                children.add(piece.createTag(CTX));
            }
            PiecesContainer loaded = PiecesContainer.load(children, CTX);

            out.append("Q ").append(kind).append(' ').append(seed).append(' ').append(cx)
                    .append(' ').append(cz).append(' ').append(children.size()).append(' ')
                    .append(loaded.pieces().size()).append('\n');
            for (StructurePiece piece : loaded.pieces()) {
                out.append(dump("L", piece)).append('\n');
            }

            if (kind.startsWith("ocean_monument")) {
                PiecesContainer regenerated = OceanMonumentStructure.regeneratePiecesAfterLoad(
                        new ChunkPos(cx, cz), seed, loaded);
                List<String> lines = new ArrayList<>();
                for (StructurePiece piece : regenerated.pieces()) {
                    lines.add(dump("R", piece));
                    lines.addAll(childLines("R", piece));
                }
                out.append("S ").append(lines.size()).append('\n');
                for (String value : lines) {
                    out.append(value).append('\n');
                }
            }
        }
        System.out.print(out);
    }

    /** 用官方构造器造结构件（编排与 StructurePiecesOracle 一致）。 */
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
                    // surfaceHeight 取固定值，只要求两侧用同一个输入。
                    // Use one fixed surfaceHeight; both sides must share the input.
                    int targetY = SEA_LEVEL;
                    builder.offsetPiecesVertically(
                            targetY - builder.getBoundingBox().getCenter().getY());
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
                        int pos = random.nextInt(pending.size());
                        StructurePiece piece = pending.remove(pos);
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
            case "ocean_monument" -> {
                random.setLargeFeatureSeed(seed, cx, cz);
                int west = cx * 16 - 29;
                int north = cz * 16 - 29;
                Direction orientation = Direction.Plane.HORIZONTAL.getRandomDirection(random);
                builder.addPiece(new OceanMonumentPieces.MonumentBuilding(
                        random, west, north, orientation));
            }
            default -> throw new IllegalArgumentException("unknown kind " + kind);
        }
        return builder.build().pieces();
    }

    /** 把一个结构件按统一格式 dump：`<prefix> id bb6 O GD 子类键...`。 */
    private static String dump(String prefix, StructurePiece piece) {
        String id = BuiltInRegistries.STRUCTURE_PIECE.getKey(piece.getType()).toString();
        BoundingBox bb = piece.getBoundingBox();
        Direction orientation = piece.getOrientation();
        int o = orientation == null ? -1 : orientation.get2DDataValue();
        int gd = piece.getGenDepth();

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

        return prefix + " " + id + " " + bb.minX() + " " + bb.minY() + " " + bb.minZ() + " "
                + bb.maxX() + " " + bb.maxY() + " " + bb.maxZ() + " " + o + " " + gd + " "
                + extraToken;
    }

    /** `MonumentBuilding` 的 `childPieces` 是私有字段，用反射列出来一起比。 */
    private static List<String> childLines(String prefix, StructurePiece piece) {
        if (!(piece instanceof OceanMonumentPieces.MonumentBuilding building)) {
            return List.of();
        }
        try {
            java.lang.reflect.Field field =
                    OceanMonumentPieces.MonumentBuilding.class.getDeclaredField("childPieces");
            field.setAccessible(true);
            List<String> out = new ArrayList<>();
            for (Object child : (List<?>) field.get(building)) {
                out.add(dump(prefix, (StructurePiece) child));
            }
            return out;
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("cannot read MonumentBuilding.childPieces", e);
        }
    }

    /** 子类存档值的标准串，数字带 NBT 类型前缀（和 StructurePiecesOracle 一致）。 */
    private static String value(Tag tag) {
        if (tag instanceof IntArrayTag array) {
            StringBuilder sb = new StringBuilder();
            for (int v : array.getAsIntArray()) {
                if (!sb.isEmpty()) {
                    sb.append('_');
                }
                sb.append(v);
            }
            return sb.toString();
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
        java.util.Optional<Number> number = tag.asNumber();
        if (number.isPresent()) {
            return tagTypeName(tag) + ":" + number.get().longValue();
        }
        java.util.Optional<String> string = tag.asString();
        if (string.isPresent()) {
            return string.get();
        }
        return "?";
    }

    private static String tagTypeName(Tag tag) {
        return switch (tag.getId()) {
            case 1 -> "B";
            case 2 -> "S";
            case 3 -> "I";
            case 4 -> "L";
            case 5 -> "F";
            case 6 -> "D";
            default -> "?";
        };
    }
}
