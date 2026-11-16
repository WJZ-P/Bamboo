import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.structures.JungleTemplePiece;
import net.minecraft.world.level.storage.loot.LootTable;

/**
 * 跑一次官方 {@code JungleTemplePiece.postProcess}，把它调用的绘制 helper 逐条记下来。
 *
 * <p>不启动服务端、不读存档、不接受 EULA、不加载数据包注册表：用 {@code Bootstrap.bootStrap()}
 * 初始化方块和状态，再用一个只回答 {@code getHeightmapPos} / {@code getBlockState} 的
 * {@code WorldGenLevel} 代理，让 {@code updateAverageGroundHeight} 走完，随后所有 helper
 * 都被子类拦截，不会真的写世界。导出的是调用序列，不是官方源码。
 *
 * <p>{@code createDispenser} 内部会经 {@code placeBlock} 放发射器；这里按官方的顺序
 * 先记 {@code dispenser}，再补记那次 {@code placeBlock}，Rust 侧把这一对合成一条指令
 * （和 Bamboo 的执行方式一致）。
 *
 * <p>输出一行一条 JSON，顺序即调用顺序：{@code moss} / {@code air} / {@code one} /
 * {@code chest} / {@code dispenser}。方块状态写成 {@code minecraft:x[k=v,...]}，
 * 属性和 {@code BlockState.toString()} 一样按官方 {@code StateDefinition} 的顺序排列。
 *
 * <p>Record the paint helper calls of one vanilla {@code JungleTemplePiece.postProcess} run.
 * No server, save, EULA or data pack registries: only the built-in block registry is
 * bootstrapped, and a minimal {@code WorldGenLevel} proxy lets
 * {@code updateAverageGroundHeight} finish. All helpers are intercepted by the subclass,
 * so no world writes happen. The output is the call sequence, not vanilla source code.
 *
 * <p>Usage: JungleTempleOpsExtractor &lt;output.json&gt;
 */
public final class JungleTempleOpsExtractor {
    private JungleTempleOpsExtractor() {
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: JungleTempleOpsExtractor <output.json>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        JsonArray ops = new JsonArray();
        ChunkPos chunk = new ChunkPos(0, 0);
        // 包围盒盖住整个结构件：所有 helper 的坐标都要照原样记下来，不能被 isInside 裁掉。
        // The box covers the whole piece: no helper coordinate may be clipped by isInside.
        BoundingBox box = new BoundingBox(-256, -64, -256, 256, 319, 256);
        WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(
                JungleTempleOpsExtractor.class.getClassLoader(),
                new Class<?>[] {WorldGenLevel.class},
                new PieceLevelHandler());
        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
        new RecordingPiece(ops, random, chunk.getMinBlockX(), chunk.getMinBlockZ())
                .postProcess(level, null, null, random, box, chunk, BlockPos.ZERO);
        Files.writeString(Path.of(args[0]),
                new GsonBuilder().create().toJson(ops) + "\n", StandardCharsets.UTF_8);
    }

    /** 一个方块状态：注册名和按官方属性顺序排列的 {@code k=v}。 */
    private static JsonObject state(BlockState state) {
        JsonObject out = new JsonObject();
        StringBuilder text = new StringBuilder();
        text.append(BuiltInRegistries.BLOCK.getKey(state.getBlock())).append('[');
        boolean first = true;
        for (Property<?> property : state.getProperties()) {
            if (!first) {
                text.append(',');
            }
            first = false;
            text.append(property.getName()).append('=').append(value(state, property));
        }
        text.append(']');
        out.addProperty("state", text.toString());
        return out;
    }

    private static <T extends Comparable<T>> String value(BlockState state, Property<T> property) {
        return property.getName(state.getValue(property));
    }

    private static String table(ResourceKey<LootTable> key) {
        return key.identifier().toString();
    }

    /**
     * 覆盖全部绘制 helper。基类 {@code generateBox} 里的每格 {@code placeBlock} 调用不会
     * 进报告：拦截 {@code generateBox} 后直接记录整盒，Bamboo 侧再按官方的 y → x → z
     * 循环展开。
     */
    private static final class RecordingPiece extends JungleTemplePiece {
        private final JsonArray ops;

        RecordingPiece(JsonArray ops, RandomSource random, int x, int z) {
            super(random, x, z);
            this.ops = ops;
        }

        @Override
        protected void generateBox(WorldGenLevel level, BoundingBox chunkBox,
                int x0, int y0, int z0, int x1, int y1, int z1, boolean skipAir,
                RandomSource random, StructurePiece.BlockSelector selector) {
            // 只记录 selector 是 MossStoneSelector 的调用；官方这个类里也只用这一种。
            // Only MossStoneSelector is used by this piece; fail loudly if vanilla changes.
            if (!selector.getClass().getName().endsWith("MossStoneSelector")) {
                throw new IllegalStateException("unexpected selector " + selector.getClass());
            }
            JsonObject op = new JsonObject();
            op.addProperty("kind", "moss");
            op.add("from", numbers(x0, y0, z0));
            op.add("to", numbers(x1, y1, z1));
            op.addProperty("skipAir", skipAir);
            this.ops.add(op);
        }

        @Override
        protected void generateAirBox(WorldGenLevel level, BoundingBox chunkBox,
                int x0, int y0, int z0, int x1, int y1, int z1) {
            JsonObject op = new JsonObject();
            op.addProperty("kind", "air");
            op.add("from", numbers(x0, y0, z0));
            op.add("to", numbers(x1, y1, z1));
            this.ops.add(op);
        }

        @Override
        protected void placeBlock(WorldGenLevel level, BlockState state,
                int x, int y, int z, BoundingBox chunkBox) {
            JsonObject op = state(state);
            op.addProperty("kind", "one");
            op.add("at", numbers(x, y, z));
            this.ops.add(op);
        }

        @Override
        protected boolean createChest(WorldGenLevel level, BoundingBox chunkBox,
                RandomSource random, int x, int y, int z, ResourceKey<LootTable> lootTable) {
            JsonObject op = new JsonObject();
            op.addProperty("kind", "chest");
            op.add("at", numbers(x, y, z));
            op.addProperty("table", table(lootTable));
            this.ops.add(op);
            return true;
        }

        @Override
        protected boolean createDispenser(WorldGenLevel level, BoundingBox chunkBox,
                RandomSource random, int x, int y, int z, Direction facing,
                ResourceKey<LootTable> lootTable) {
            JsonObject op = new JsonObject();
            op.addProperty("kind", "dispenser");
            op.add("at", numbers(x, y, z));
            op.addProperty("facing", facing.getName());
            op.addProperty("table", table(lootTable));
            this.ops.add(op);
            // 官方 createDispenser 内部放的那只发射器，作为普通 placeBlock 记下来；
            // Rust 侧会把这两条合成一条指令。
            // Vanilla createDispenser places the dispenser through placeBlock; the Rust
            // side merges the two records into one instruction.
            this.placeBlock(level, net.minecraft.world.level.block.Blocks.DISPENSER
                    .defaultBlockState().setValue(
                            net.minecraft.world.level.block.DispenserBlock.FACING, facing),
                    x, y, z, chunkBox);
            return true;
        }
    }

    private static JsonArray numbers(int x, int y, int z) {
        JsonArray out = new JsonArray();
        out.add(x);
        out.add(y);
        out.add(z);
        return out;
    }

    /** 只实现 {@code updateAverageGroundHeight} 会问到的两个方法。 */
    private static final class PieceLevelHandler implements InvocationHandler {
        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            return switch (method.getName()) {
                // 常量高度让 heightPosition 确定，且不影响任何 helper 的相对坐标。
                // A constant height only fixes heightPosition; helper coordinates stay relative.
                case "getHeightmapPos" -> new BlockPos(((BlockPos) args[1]).getX(), 64,
                        ((BlockPos) args[1]).getZ());
                case "getBlockState" -> net.minecraft.world.level.block.Blocks.AIR
                        .defaultBlockState();
                case "getMinY" -> -64;
                case "getMaxY" -> 319;
                case "toString" -> "JungleTempleOpsExtractor";
                case "hashCode" -> System.identityHashCode(proxy);
                case "equals" -> proxy == args[0];
                default -> throw new UnsupportedOperationException(
                        "WorldGenLevel." + method.getName() + " is not implemented");
            };
        }
    }
}
