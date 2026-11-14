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
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;
import net.minecraft.world.level.levelgen.structure.structures.IglooPieces;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;

/**
 * 用官方 26.3 的模板结构件类对拍「需要模板的单件结构」的结构件布局
 * （当前覆盖雪屋）。
 *
 * <p>不启动服务端、不读存档、不接受 EULA、不加载 worldgen 注册表。只需要
 * {@code Bootstrap.bootStrap()} 和一份真实 {@code StructureTemplateManager}
 * （包围盒要模板尺寸）。每条查询自己用 {@code setLargeFeatureSeed(seed, cx, cz)}
 * 播一个 WorldgenRandom，再照各 {@code Structure.generatePieces} 的编排调用官方
 * 结构件类，最后用 {@code createTag} 把每个结构件按统一格式 dump 出来。
 *
 * <p>queries.txt 每行 "{@code kind seed cx cz arg}"，arg 留给需要外部输入的 kind
 * （雪屋不用）。输出每个查询一段：{@code Q} 头带结构件数，随后若干 {@code P} 行。
 *
 * <p>Compare the piece layout of templated single-piece structures (currently the
 * igloo) against vanilla 26.3 by calling the official piece classes directly. No
 * server, save, EULA or worldgen registry load; needs {@code Bootstrap.bootStrap()}
 * and a real {@code StructureTemplateManager} (bounding boxes need template sizes).
 *
 * <p>Usage: StructureTemplatedOracle &lt;queries.txt&gt;
 */
public final class StructureTemplatedOracle {
    private static final int GENERATION_HEIGHT = 90;

    // createTag 里由基类写的四个键，不进 extras。
    // Four base keys written by StructurePiece.createTag; excluded from extras.
    private static final java.util.Set<String> BASE_KEYS =
            java.util.Set.of("id", "BB", "O", "GD");

    private static final StructurePieceSerializationContext CTX =
            new StructurePieceSerializationContext(null, null, null);

    private static StructureTemplateManager templateManager;

    private static StructureTemplateManager templateManager() {
        if (templateManager == null) {
            try {
                PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
                repository.reload();
                repository.setSelected(List.of("vanilla"));
                MultiPackResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Path temp = Files.createTempDirectory("bamboo-templated");
                LevelStorageSource.LevelStorageAccess storage =
                        LevelStorageSource.createDefault(temp).createAccess("templated");
                templateManager = new StructureTemplateManager(
                        resources, storage, DataFixers.getDataFixer(), BuiltInRegistries.BLOCK);
            } catch (Exception e) {
                throw new IllegalStateException("cannot build StructureTemplateManager", e);
            }
        }
        return templateManager;
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: StructureTemplatedOracle <queries.txt>");
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
                case "igloo" -> igloo(seed, cx, cz);
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

    /** {@code IglooStructure.generatePieces} 的编排。 */
    private static List<String> igloo(long seed, int cx, int cz) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, cx, cz);
        Rotation rotation = Rotation.getRandom(random);
        ChunkPos chunkPos = new ChunkPos(cx, cz);
        BlockPos startPos = new BlockPos(chunkPos.getMinBlockX(), GENERATION_HEIGHT, chunkPos.getMinBlockZ());
        StructurePiecesBuilder builder = new StructurePiecesBuilder();
        IglooPieces.addPieces(templateManager(), startPos, rotation, builder, random);
        return builder.build().pieces().stream()
                .map(StructureTemplatedOracle::dump)
                .collect(Collectors.toCollection(ArrayList::new));
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
