import it.unimi.dsi.fastutil.longs.LongOpenHashSet;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Random;

/**
 * 生成 `java.util.HashSet<BlockPos>` 的插入序列与最终迭代顺序，给 Rust 侧
 * `feature::java_set::JavaPosSet` 逐条对拍。
 *
 * `BlockPos` 的 hashCode / equals 与官方 Vec3i 一致（见 Vec3i.hashCode 字节码）；
 * 这里用一个同形状的类，避免拉起整个服务端的注册表。坐标刻意制造同桶碰撞，
 * 让桶内节点数超过 8、容量从小扩到 64 以上，覆盖 treeifyBin / putTreeVal /
 * TreeNode.split 这几条路径。植被 patch 的地表集合只做 add，所以这里也只做 add。
 *
 * 协议：每个用例三行，`OPS` 是依次 add 的坐标，`ORDER` 是最终的迭代顺序。
 * 坐标用空格分隔、条目用分号分隔；`+` 表示 add。
 */
public class JavaSetOracle {
    private static final class P {
        final int x;
        final int y;
        final int z;

        P(int x, int y, int z) {
            this.x = x;
            this.y = y;
            this.z = z;
        }

        @Override
        public int hashCode() {
            return (y + z * 31) * 31 + x;
        }

        @Override
        public boolean equals(Object other) {
            return other instanceof P p && p.x == x && p.y == y && p.z == z;
        }

        @Override
        public String toString() {
            return x + " " + y + " " + z;
        }
    }

    private static void emit(StringBuilder out, char op, P p) {
        out.append(op).append(p.x).append(' ').append(p.y).append(' ').append(p.z).append(';');
    }

    public static void main(String[] args) {
        int cases = 80;
        boolean longSet = false;
        for (int i = 0; i < args.length; i++) {
            if (args[i].equals("--cases") && i + 1 < args.length) {
                cases = Integer.parseInt(args[++i]);
            } else if (args[i].equals("--longset")) {
                longSet = true;
            }
        }
        if (longSet) {
            System.out.print(longSetCases(cases));
            return;
        }
        StringBuilder out = new StringBuilder();
        for (int c = 0; c < cases; c++) {
            Random random = new Random(c * 7919L + 12345L);
            int count = 3 + random.nextInt(220);
            int pattern = c % 8;
            HashSet<P> set = new HashSet<>();
            StringBuilder ops = new StringBuilder("OPS ");
            // 打乱插入顺序的「同桶碰撞」用例：`putTreeVal` 的树父节点不再是链表尾，
            // 必须把新节点插在父节点之后，而不是追加到桶尾。
            // Shuffled same-bin collisions: `putTreeVal`'s tree parent is no longer
            // the list tail, so the new node must be spliced after its parent.
            List<P> shuffled = null;
            if (pattern == 7) {
                shuffled = new ArrayList<>();
                for (int i = 0; i < count; i++) {
                    shuffled.add(new P(i * 16, 62, 0));
                }
                Collections.shuffle(shuffled, random);
            }
            for (int i = 0; i < count; i++) {
                P p;
                if (pattern == 7) {
                    p = shuffled.get(i);
                } else {
                    int x;
                    int y;
                    int z;
                    switch (pattern) {
                        case 0 -> { x = i * 16; y = 62; z = 0; }
                        case 1 -> { x = random.nextInt(64); y = random.nextInt(20) - 10; z = random.nextInt(64); }
                        case 2 -> { x = random.nextInt(8) * 16; y = random.nextInt(4) * 32 + 40; z = random.nextInt(8); }
                        case 3 -> { x = i * 16 + random.nextInt(3); y = 62; z = 0; }
                        case 4 -> { x = random.nextInt(512) - 256; y = random.nextInt(128) - 64; z = random.nextInt(512) - 256; }
                        case 5 -> { x = i * 32; y = 62; z = i % 2; }
                        default -> { x = random.nextInt(32); y = 62; z = 0; }
                    }
                    p = new P(x, y, z);
                }
                set.add(p);
                emit(ops, '+', p);
            }
            out.append("CASE ").append(c).append('\n');
            out.append(ops).append('\n');
            out.append("ORDER ");
            for (P p : set) {
                out.append(p.x).append(' ').append(p.y).append(' ').append(p.z).append(';');
            }
            out.append('\n');
        }
        System.out.print(out);
    }

    /**
     * `LongOpenHashSet` 的插入序列与最终迭代顺序，给 Rust 侧
     * `feature::java_long_set::JavaLongSet` 逐条对拍。键 0 有专槽；负数、会冲突的
     * 键（低掩码位相同）、跨过 24 / 48 / 96 等扩容边界的数量都要覆盖。
     *
     * Protocol per case: {@code CASE n}, {@code OPS v;v;...}, {@code ORDER v;v;...}.
     */
    private static String longSetCases(int cases) {
        StringBuilder out = new StringBuilder();
        for (int c = 0; c < cases; c++) {
            Random random = new Random(c * 7919L + 12345L);
            int count = 3 + random.nextInt(220);
            int pattern = c % 8;
            LongOpenHashSet set = new LongOpenHashSet();
            StringBuilder ops = new StringBuilder("OPS ");
            for (int i = 0; i < count; i++) {
                long value;
                switch (pattern) {
                    case 0 -> value = i * 4_294_967_296L;             // 低 32 位恒 0
                    case 1 -> value = random.nextLong();
                    case 2 -> value = -random.nextInt(1 << 20) - 1L;   // 负数packed键
                    case 3 -> value = (i % 40) * 32L;                  // 掩码冲突
                    case 4 -> value = (long) random.nextInt(1 << 16) << 32
                            | (random.nextInt(1 << 16) & 0xffff_ffffL);
                    case 5 -> value = i == 0 ? 0L : random.nextLong();
                    case 6 -> value = (i / 2) * 1_000_003L;            // 重复键
                    default -> value = i * 32L + (i % 3);              // 连号 + 少量冲突
                }
                set.add(value);
                ops.append(value).append(';');
            }
            out.append("CASE ").append(c).append('\n');
            out.append(ops).append('\n');
            out.append("ORDER ");
            for (long value : set) {
                out.append(value).append(';');
            }
            out.append('\n');
        }
        return out.toString();
    }
}
