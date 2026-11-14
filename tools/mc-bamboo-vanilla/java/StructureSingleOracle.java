import com.mojang.datafixers.util.Either;
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
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;
import net.minecraft.world.level.levelgen.structure.structures.BuriedTreasurePieces;
import net.minecraft.world.level.levelgen.structure.structures.DesertPyramidPiece;
import net.minecraft.world.level.levelgen.structure.structures.JungleTemplePiece;
import net.minecraft.world.level.levelgen.structure.structures.SwampHutPiece;

/**
 * 用官方 26.3 的结构件类直接对拍四个「代码生成」单件结构的结构件布局：
 * 沙漠神殿、丛林神殿、女巫小屋、埋藏的宝藏。
 *
 * <p>不启动服务端、不读存档、不接受 EULA、不加载 worldgen 注册表、不读 .nbt 模板：
 * 只需 {@code Bootstrap.bootStrap()} 让 {@code BuiltInRegistries.STRUCTURE_PIECE} 就绪。
 * 官方 {@code Structure.findGenerationPoint} 里的 {@code onTopOfChunkCenter} 只算一个
 * 起点高度（terrain，属 structure-validity 对拍），而 {@code generatePieces} 造结构件时
 * 只用到 {@code context.random()}（{@code makeRandom(seed, chunkPos)}）、区块最小方块坐标
 * 和常量尺寸，起点高度不进包围盒。这里按 {@code generatePieces} 的编排用官方构造器直接
 * 造结构件，再用 {@code createTag} 按统一格式 dump。
 *
 * <p>queries.txt 每行 "{@code kind seed cx cz}"。输出每个查询一段：{@code Q} 头带
 * 结构件数，随后若干 {@code P} 行。
 *
 * <p>Compare the piece layout of the four code-built single-piece structures against
 * vanilla 26.3, calling the official piece constructors directly. No server, save, EULA,
 * worldgen registry or .nbt templates; only {@code Bootstrap.bootStrap()}. Each query
 * seeds its own WorldgenRandom via {@code setLargeFeatureSeed}. Pieces are dumped via
 * {@code createTag} in one canonical format.
 *
 * <p>Usage: StructureSingleOracle &lt;queries.txt&gt;
 */
public final class StructureSingleOracle {
    // createTag 里由基类写的四个键，不进 extras。
    // Four base keys written by StructurePiece.createTag; excluded from extras.
    private static final java.util.Set<String> BASE_KEYS =
            java.util.Set.of("id", "BB", "O", "GD");

    private static final StructurePieceSerializationContext CTX =
            new StructurePieceSerializationContext(null, null, null);

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: StructureSingleOracle <queries.txt>");
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
            List<String> pieces = generate(kind, seed, cx, cz);
            out.append("Q ").append(kind).append(' ').append(seed).append(' ').append(cx)
                    .append(' ').append(cz).append(' ').append(pieces.size()).append('\n');
            for (String piece : pieces) {
                out.append(piece).append('\n');
            }
        }
        System.out.print(out);
    }

    /** 照各结构 {@code generatePieces} 的编排，用官方构造器造结构件并 dump。 */
    private static List<String> generate(String kind, long seed, int cx, int cz) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, cx, cz);
        ChunkPos chunkPos = new ChunkPos(cx, cz);
        StructurePiecesBuilder builder = new StructurePiecesBuilder();
        switch (kind) {
            case "desert_pyramid" -> builder.addPiece(new DesertPyramidPiece(
                    random, chunkPos.getMinBlockX(), chunkPos.getMinBlockZ()));
            case "jungle_temple" -> builder.addPiece(new JungleTemplePiece(
                    random, chunkPos.getMinBlockX(), chunkPos.getMinBlockZ()));
            case "swamp_hut" -> builder.addPiece(new SwampHutPiece(
                    random, chunkPos.getMinBlockX(), chunkPos.getMinBlockZ()));
            case "buried_treasure" -> builder.addPiece(new BuriedTreasurePieces.BuriedTreasurePiece(
                    new BlockPos(chunkPos.getBlockX(9), 90, chunkPos.getBlockZ(9))));
            default -> throw new IllegalArgumentException("unknown kind " + kind);
        }
        return builder.build().pieces().stream().map(StructureSingleOracle::dump).collect(Collectors.toList());
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
