import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.densityfunction.DensityBuffer;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.TerrainAdjustment;
import net.minecraft.world.level.levelgen.structure.pools.JigsawJunction;
import net.minecraft.world.level.levelgen.structure.pools.StructureTemplatePool;

/**
 * 用官方 26.3 的 {@code Beardifier} 直接对拍纯密度贡献。
 *
 * <p>{@code Beardifier} 有一个 {@code @VisibleForTesting public} 构造器
 * {@code Beardifier(List<Rigid>, List<JigsawJunction>, BoundingBox affectedBox)}，
 * 所以不启动服务端、不读存档、不加载 worldgen 注册表，直接自造
 * {@code Rigid} / {@code JigsawJunction} 组装 beardifier，再逐点比
 * {@code sampleValue} 和 {@code sampleVolume} 的 float 位。
 *
 * <p>queries.txt 每行是一个用例（见 Rust 侧 {@code beardifier.rs} 的格式说明）：
 * 一段 {@code R} 盒子、一段 {@code J} junction、一个 {@code A} affectedBox（或 none）、
 * 若干 {@code S} 采样点、一个 {@code V} 体积。输出 {@code SV}（单点值）和
 * {@code VOL}（整块 sampleVolume 缓冲）两类行，Rust 侧逐位比较。
 *
 * <p>Compare the pure density contribution against vanilla 26.3's {@code Beardifier}
 * via its {@code @VisibleForTesting public} constructor. No server, save or registry.
 * Output: {@code SV} (single-point value) and {@code VOL} (whole sampleVolume buffer)
 * lines, compared bit for bit by the Rust {@code beardifier.rs} tool.
 *
 * <p>Usage: BeardifierOracle &lt;queries.txt&gt;
 */
public final class BeardifierOracle {
    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: BeardifierOracle <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        List<String> lines = Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8);
        StringBuilder out = new StringBuilder();
        int i = 0;
        int caseId = 0;
        while (i < lines.size()) {
            String header = lines.get(i++);
            if (header.isBlank()) {
                continue;
            }
            if (!header.startsWith("Q ")) {
                throw new IllegalStateException("expected Q header, got: " + header);
            }
            String[] h = header.trim().split("\\s+");
            int nRigid = Integer.parseInt(h[2]);
            int nJunc = Integer.parseInt(h[3]);

            List<Beardifier.Rigid> rigids = new ArrayList<>();
            for (int r = 0; r < nRigid; r++) {
                String[] f = fields(lines.get(i++), "R");
                BoundingBox box = new BoundingBox(
                        Integer.parseInt(f[1]), Integer.parseInt(f[2]), Integer.parseInt(f[3]),
                        Integer.parseInt(f[4]), Integer.parseInt(f[5]), Integer.parseInt(f[6]));
                TerrainAdjustment adj = TerrainAdjustment.values()[Integer.parseInt(f[7])];
                int gld = Integer.parseInt(f[8]);
                rigids.add(new Beardifier.Rigid(box, adj, gld));
            }
            List<JigsawJunction> junctions = new ArrayList<>();
            for (int j = 0; j < nJunc; j++) {
                String[] f = fields(lines.get(i++), "J");
                junctions.add(new JigsawJunction(
                        Integer.parseInt(f[1]), Integer.parseInt(f[2]), Integer.parseInt(f[3]),
                        0, StructureTemplatePool.Projection.RIGID));
            }
            String[] a = fields(lines.get(i++), "A");
            BoundingBox affectedBox = a[1].equals("none")
                    ? null
                    : new BoundingBox(
                            Integer.parseInt(a[1]), Integer.parseInt(a[2]), Integer.parseInt(a[3]),
                            Integer.parseInt(a[4]), Integer.parseInt(a[5]), Integer.parseInt(a[6]));
            Beardifier beardifier = new Beardifier(rigids, junctions, affectedBox);

            // sampleValue 的采样点。
            // sampleValue sample points.
            List<int[]> points = new ArrayList<>();
            while (i < lines.size() && lines.get(i).startsWith("S ")) {
                String[] f = fields(lines.get(i++), "S");
                points.add(new int[]{Integer.parseInt(f[1]), Integer.parseInt(f[2]), Integer.parseInt(f[3])});
            }
            out.append("PTS ").append(caseId).append(' ').append(points.size()).append('\n');
            for (int p = 0; p < points.size(); p++) {
                int[] pt = points.get(p);
                float value = beardifier.sampleValue(null, pt[0], pt[1], pt[2]);
                out.append("SV ").append(caseId).append(' ').append(p).append(' ')
                        .append(Float.floatToRawIntBits(value)).append('\n');
            }

            // sampleVolume 一整块缓冲。
            // A whole sampleVolume buffer.
            String[] v = fields(lines.get(i++), "V");
            DensityVolume volume = new DensityVolume(
                    Integer.parseInt(v[1]), Integer.parseInt(v[2]), Integer.parseInt(v[3]),
                    Integer.parseInt(v[4]), Integer.parseInt(v[5]), Integer.parseInt(v[6]),
                    Integer.parseInt(v[7]), Integer.parseInt(v[8]), Integer.parseInt(v[9]));
            DensityBuffer buffer = DensityBuffer.createUnpooled(volume.size());
            beardifier.sampleVolume(null, buffer, volume);
            out.append("VOL ").append(caseId).append(' ').append(volume.size());
            for (int k = 0; k < volume.size(); k++) {
                out.append(' ').append(Float.floatToRawIntBits(buffer.get(k)));
            }
            out.append('\n');
            caseId++;
        }
        System.out.print(out);
    }

    private static String[] fields(String line, String tag) {
        String[] f = line.trim().split("\\s+");
        if (!f[0].equals(tag)) {
            throw new IllegalStateException("expected " + tag + " line, got: " + line);
        }
        return f;
    }
}
