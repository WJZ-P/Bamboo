import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.nbt.ByteArrayTag;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.LongArrayTag;
import net.minecraft.nbt.NumericTag;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.pieces.PiecesContainer;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;
import net.minecraft.world.level.levelgen.structure.structures.BuriedTreasurePieces;
import net.minecraft.world.level.levelgen.structure.structures.DesertPyramidPiece;
import net.minecraft.world.level.levelgen.structure.structures.JungleTemplePiece;
import net.minecraft.world.level.levelgen.structure.structures.SwampHutPiece;

/**
 * 对拍区块 NBT 的 `structures` 键：官方 `SerializableChunkData.packStructureData` /
 * `StructureStart.createTag` / `PiecesContainer.save` / `StructurePiece.createTag`。
 *
 * <p>Rust 写 cases.json（一组 `{chunk, starts, refs}`），本类逐条：
 * <ul>
 *   <li>用官方 `CompoundTag` 原语按 `packStructureData` 组装 `structures`：
 *       `starts` 里每个起点按 `createTag` 写 `id/ChunkX/ChunkZ/references/Children`
 *       （空 `Children` 只写 `id=INVALID`）；`Children` 每条用官方
 *       `piece.createTag(CTX)` 生成（含 `BB/O/GD` 和子类 `addAdditionalSaveData`）；
 *       `References` 用 `putLongArray`。
 *   <li>`canonical(structures)`：按键排序的规范 JSON，供 Rust 逐项比较。
 *   <li>回读证明：把 `Children` 交给官方 `PiecesContainer.load(CTX)`，再 `save(CTX)`
 *       重新序列化，和原 `Children` 规范比较（`reloadEqual`），证明官方能读；
 *       并报告装入的结构件数（`reloadedPieces`）。
 * </ul>
 *
 * <p>起点标签和 `structures` 外壳是官方几个 `putXxx` 的直接拼装（`createTag` /
 * `packStructureData` 本身就是这些原语），真正复杂的结构件标签 100% 走官方
 * `piece.createTag`。不启动服务端、不读存档、不接受 EULA、不加载 worldgen 注册表：
 * 只需 `Bootstrap.bootStrap()` 让 `BuiltInRegistries.STRUCTURE_PIECE` 就绪。
 *
 * <p>Compare the chunk NBT `structures` key against vanilla. Rust writes cases.json;
 * for each case we build `structures` with official `CompoundTag` primitives (the pieces
 * come from the official `piece.createTag`), dump a key-sorted canonical JSON, then prove
 * official readability via `PiecesContainer.load` + `save` round-trip. Bootstrap only.
 *
 * <p>Usage: StructureSaveOracle &lt;cases.json&gt; &lt;out.json&gt;
 */
public final class StructureSaveOracle {
    private static final StructurePieceSerializationContext CTX =
            new StructurePieceSerializationContext(null, null, null);
    private static final Gson GSON = new Gson();

    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException("usage: StructureSaveOracle <cases.json> <out.json>");
        }
        // 抢在日志框架初始化之前把 stdout 让给 stderr，结果只写文件，避免日志污染。
        // Move vanilla logs to stderr before bootstrap; results go only to the file.
        PrintStream log = new PrintStream(new FileOutputStream(FileDescriptor.err), true, StandardCharsets.UTF_8);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        JsonObject root = JsonParser.parseString(
                Files.readString(Path.of(args[0]), StandardCharsets.UTF_8)).getAsJsonObject();
        JsonArray cases = root.getAsJsonArray("cases");
        JsonArray results = new JsonArray();
        for (var element : cases) {
            results.add(handle(element.getAsJsonObject()));
        }
        Files.writeString(Path.of(args[1]), GSON.toJson(results), StandardCharsets.UTF_8);
    }

    /** 组装一条 case 的 `structures`，输出规范 JSON 和回读结果。 */
    private static JsonObject handle(JsonObject c) {
        JsonArray startArray = c.getAsJsonArray("starts");
        CompoundTag starts = new CompoundTag();
        int reloadedPieces = 0;
        boolean reloadEqual = true;

        for (var element : startArray) {
            JsonObject s = element.getAsJsonObject();
            String id = s.get("id").getAsString();
            JsonArray pieceArray = s.getAsJsonArray("pieces");
            CompoundTag startTag = new CompoundTag();

            if (pieceArray.isEmpty()) {
                // StructureStart.INVALID_START → createTag 只写 id=INVALID。
                startTag.putString("id", "INVALID");
            } else {
                ListTag children = new ListTag();
                for (var pieceElement : pieceArray) {
                    JsonObject p = pieceElement.getAsJsonObject();
                    for (StructurePiece piece : buildPieces(
                            p.get("kind").getAsString(),
                            p.get("seed").getAsLong(),
                            p.get("px").getAsInt(),
                            p.get("pz").getAsInt())) {
                        children.add(piece.createTag(CTX));
                    }
                }
                startTag.putString("id", id);
                startTag.putInt("ChunkX", s.get("cx").getAsInt());
                startTag.putInt("ChunkZ", s.get("cz").getAsInt());
                startTag.putInt("references", s.get("refs").getAsInt());
                startTag.put("Children", children);

                // 官方 load + save 往返：证明 PiecesContainer.load 能读 Children。
                // Official load + save round-trip: prove PiecesContainer.load can read Children.
                PiecesContainer loaded = PiecesContainer.load(children, CTX);
                Tag reloaded = loaded.save(CTX);
                reloadEqual &= canonical(reloaded).equals(canonical(children));
                reloadedPieces += loaded.pieces().size();
            }
            starts.put(id, startTag);
        }

        CompoundTag references = new CompoundTag();
        JsonObject refs = c.getAsJsonObject("refs");
        for (var entry : refs.entrySet()) {
            long[] values = new long[entry.getValue().getAsJsonArray().size()];
            JsonArray array = entry.getValue().getAsJsonArray();
            for (int i = 0; i < values.length; i++) {
                values[i] = Long.parseLong(array.get(i).getAsString());
            }
            // packStructureData：空 LongSet 不写。
            // packStructureData: empty LongSet is skipped.
            if (values.length > 0) {
                references.putLongArray(entry.getKey(), values);
            }
        }

        CompoundTag structures = new CompoundTag();
        structures.put("starts", starts);
        structures.put("References", references);

        JsonObject out = new JsonObject();
        out.add("canonical", canonical(structures));
        out.addProperty("reloadEqual", reloadEqual);
        out.addProperty("reloadedPieces", reloadedPieces);
        return out;
    }

    /** 照各结构 generatePieces 的编排用官方构造器造结构件（和 StructureSingleOracle 一致）。 */
    private static List<StructurePiece> buildPieces(String kind, long seed, int px, int pz) {
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        random.setLargeFeatureSeed(seed, px, pz);
        ChunkPos chunkPos = new ChunkPos(px, pz);
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
        return builder.build().pieces();
    }

    /** CompoundTag 用 HashMap，键序不确定：转成按键排序的规范 JSON 再比（同 StorageOracle）。 */
    private static JsonObject canonical(Tag tag) {
        JsonObject out = new JsonObject();
        out.addProperty("type", tag.getId());
        switch (tag.getId()) {
            case 1, 2, 3, 4, 5, 6 -> out.addProperty("value", String.valueOf(((NumericTag) tag).box()));
            case 8 -> out.addProperty("value", ((StringTag) tag).value());
            case 7 -> {
                JsonArray values = new JsonArray();
                for (byte value : ((ByteArrayTag) tag).getAsByteArray()) {
                    values.add((int) value);
                }
                out.add("values", values);
            }
            case 11 -> {
                JsonArray values = new JsonArray();
                for (int value : ((IntArrayTag) tag).getAsIntArray()) {
                    values.add(value);
                }
                out.add("values", values);
            }
            case 12 -> {
                JsonArray values = new JsonArray();
                for (long value : ((LongArrayTag) tag).getAsLongArray()) {
                    values.add(value);
                }
                out.add("values", values);
            }
            case 9 -> {
                JsonArray values = new JsonArray();
                for (Tag value : (ListTag) tag) {
                    values.add(canonical(value));
                }
                out.add("values", values);
            }
            case 10 -> {
                JsonObject values = new JsonObject();
                List<String> keys = new ArrayList<>(((CompoundTag) tag).keySet());
                keys.sort(String::compareTo);
                for (String key : keys) {
                    values.add(key, canonical(((CompoundTag) tag).get(key)));
                }
                out.add("values", values);
            }
            default -> throw new IllegalArgumentException("tag type " + tag.getId());
        }
        return out;
    }
}
