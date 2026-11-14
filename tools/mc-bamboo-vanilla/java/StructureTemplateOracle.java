import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.stream.Stream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import net.minecraft.nbt.ByteArrayTag;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.LongArrayTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.NumericTag;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;

/**
 * 用官方 NBT 解码器对拍导出的结构模板：逐节点规范化整棵 NBT 树，并校验导出字节
 * 与官方 jar 里的原文完全一致。
 *
 * <p>输入是 Rust 侧把 blob 拆出来的 .nbt 目录和官方 server jar。对每个模板输出一行
 * {@code <id>\tX=<0|1>\t<hash>\t<counts>}：X 是「jar 原文 tag.equals(导出文件 tag)」，
 * hash 是整棵规范化 NBT 树的 FNV-1a，counts 是尺寸 / 调色板 / 方块 / 实体 / 拼图方块数。
 * Rust 侧用 mc-bamboo-nbt 和生产 StructureTemplate 复算同一行，逐模板比较。
 *
 * <p>Compare exported structure templates with vanilla's NBT decoder: normalize the
 * whole tag tree node by node and verify the exported bytes match the jar verbatim.
 * Inputs are the directory of .nbt files split out of the blob and the official jar.
 * One line per template: {@code <id> X=<0|1> <hash> <counts>}. No server, no save,
 * no EULA.
 *
 * <p>Usage: StructureTemplateOracle &lt;templates-dir&gt; &lt;server.jar&gt;
 */
public final class StructureTemplateOracle {
    private StructureTemplateOracle() {
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException(
                    "usage: StructureTemplateOracle <templates-dir> <server.jar>");
        }
        Path dir = Path.of(args[0]);
        Path jar = Path.of(args[1]);

        List<Path> files = new ArrayList<>();
        try (Stream<Path> walk = Files.walk(dir)) {
            walk.filter(p -> p.toString().endsWith(".nbt")).forEach(files::add);
        }
        Collections.sort(files);

        StringBuilder out = new StringBuilder();
        try (ZipFile zip = new ZipFile(jar.toFile())) {
            for (Path file : files) {
                String relative = dir.relativize(file).toString().replace('\\', '/');
                String id = "minecraft:" + relative.substring(0, relative.length() - 4);
                CompoundTag exported = NbtIo.readCompressed(file, NbtAccounter.unlimitedHeap());

                ZipEntry entry = zip.getEntry("data/minecraft/structure/" + relative);
                if (entry == null) {
                    throw new IllegalStateException("no jar entry for " + relative);
                }
                CompoundTag original;
                try (InputStream in = zip.getInputStream(entry)) {
                    original = NbtIo.readCompressed(in, NbtAccounter.unlimitedHeap());
                }
                int x = original.equals(exported) ? 1 : 0;

                out.append(id).append('\t')
                        .append("X=").append(x).append('\t')
                        .append(Long.toHexString(fnv(norm(exported)))).append('\t')
                        .append(counts(exported)).append('\n');
            }
        }
        System.out.print(out);
    }

    /** 把任意 tag 规范化成唯一字符串；compound 键排序，浮点用位模式避免格式差异。 */
    private static String norm(Tag tag) {
        return switch (tag.getId()) {
            case 1 -> "b" + ((NumericTag) tag).byteValue();
            case 2 -> "s" + ((NumericTag) tag).shortValue();
            case 3 -> "i" + ((NumericTag) tag).intValue();
            case 4 -> "l" + ((NumericTag) tag).longValue();
            case 5 -> "f" + Integer.toHexString(
                    Float.floatToRawIntBits(((NumericTag) tag).floatValue()));
            case 6 -> "d" + Long.toHexString(
                    Double.doubleToRawLongBits(((NumericTag) tag).doubleValue()));
            case 7 -> byteArray(((ByteArrayTag) tag).getAsByteArray());
            case 8 -> "q" + ((StringTag) tag).value();
            case 9 -> list((ListTag) tag);
            case 10 -> compound((CompoundTag) tag);
            case 11 -> intArray(((IntArrayTag) tag).getAsIntArray());
            case 12 -> longArray(((LongArrayTag) tag).getAsLongArray());
            default -> "?";
        };
    }

    private static String compound(CompoundTag tag) {
        List<String> keys = new ArrayList<>(tag.keySet());
        Collections.sort(keys);
        StringBuilder sb = new StringBuilder("{");
        for (String key : keys) {
            sb.append(key).append('=').append(norm(tag.get(key))).append(';');
        }
        return sb.append('}').toString();
    }

    private static String list(ListTag tag) {
        StringBuilder sb = new StringBuilder("[");
        for (Tag element : tag) {
            sb.append(norm(element)).append(',');
        }
        return sb.append(']').toString();
    }

    private static String byteArray(byte[] values) {
        StringBuilder sb = new StringBuilder("A[");
        for (byte value : values) {
            sb.append(value).append(',');
        }
        return sb.append(']').toString();
    }

    private static String intArray(int[] values) {
        StringBuilder sb = new StringBuilder("A[");
        for (int value : values) {
            sb.append(value).append(',');
        }
        return sb.append(']').toString();
    }

    private static String longArray(long[] values) {
        StringBuilder sb = new StringBuilder("A[");
        for (long value : values) {
            sb.append(value).append(',');
        }
        return sb.append(']').toString();
    }

    /** 尺寸 / 调色板 / 方块 / 实体 / 拼图方块数，拼图方块数按第一个调色板判定。 */
    private static String counts(CompoundTag tag) {
        ListTag size = tag.getListOrEmpty("size");
        ListTag palettes = tag.getList("palettes").orElse(null);
        int paletteCount = palettes == null ? 1 : palettes.size();
        ListTag first = palettes == null
                ? tag.getListOrEmpty("palette") : palettes.getListOrEmpty(0);
        ListTag blocks = tag.getListOrEmpty("blocks");
        ListTag entities = tag.getListOrEmpty("entities");

        int jigsaws = 0;
        for (int i = 0; i < blocks.size(); i++) {
            CompoundTag block = blocks.getCompoundOrEmpty(i);
            int state = block.getIntOr("state", 0);
            if (state >= 0 && state < first.size()
                    && "minecraft:jigsaw".equals(string(first.getCompoundOrEmpty(state), "id"))) {
                jigsaws++;
            }
        }
        return "S=" + size.getIntOr(0, 0) + "," + size.getIntOr(1, 0) + "," + size.getIntOr(2, 0)
                + ";P=" + paletteCount + ";Pe=" + first.size()
                + ";B=" + blocks.size() + ";E=" + entities.size() + ";J=" + jigsaws;
    }

    private static String string(CompoundTag tag, String key) {
        Tag value = tag.get(key);
        return value instanceof StringTag text ? text.value() : "";
    }

    private static long fnv(String text) {
        long hash = 0xcbf29ce484222325L;
        for (byte value : text.getBytes(StandardCharsets.UTF_8)) {
            hash ^= value & 0xffL;
            hash *= 0x100000001b3L;
        }
        return hash;
    }
}
