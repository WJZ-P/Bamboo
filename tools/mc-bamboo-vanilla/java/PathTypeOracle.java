import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.CollisionGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.level.pathfinder.PathType;
import net.minecraft.world.level.pathfinder.PathfindingContext;
import net.minecraft.world.level.pathfinder.WalkNodeEvaluator;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;

/**
 * 对拍每个方块状态的寻路类型表。
 *
 * <p>只走官方的数据包加载流程绑定静态注册表的标签，不启动服务端、不读存档、不接受 EULA。
 * 输出三行 JSON：
 * header 是版本和状态数，case 是三个并列的数组，下标都是全局方块状态 ID，end 是收尾计数。
 *
 * <p>`self` 是官方 `WalkNodeEvaluator.getPathTypeFromState` 的结果，`neighbor` 是同一个状态
 * 作为 `checkNeighbourBlocks` 的唯一邻居时把节点改成什么。`dynamic` 是官方
 * `Block.hasDynamicShape`：形状依赖世界或者方块实体的状态标成 true，另外两项写成 null，
 * 表里对应「需要运行时算」。
 *
 * <p>Java oracle for the per-state path type table. Only the official data pack
 * loader binds static registry tags: no server, no save, no EULA. The three
 * arrays in the case line are indexed by the global block state id.
 */
public final class PathTypeOracle {
    private static final int OUTPUT_BUFFER = 1 << 20;
    /** 26 个邻居的中心，`-1..1` 里去掉竖直的那一列（dx == 0 && dz == 0）。 */
    /** Centres of the 26 neighbours: `-1..1` minus the vertical column. */
    private static final int[][] OFFSETS = offsets();
    private static final BlockPos CENTRE = new BlockPos(1, 1, 1);

    private PathTypeOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(PrintStream log) throws Exception {
        // 官方日志默认写 stdout，要抢在日志框架初始化之前换掉。
        // Vanilla logging writes to stdout; redirect it before the logger starts.
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // 方块/流体标签要经官方的数据包加载流程绑定，寻路类型的分类依赖它们。
        // Block/fluid tags must be bound by the official data pack loader;
        // path type classification depends on them.
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources = new MultiPackResourceManager(
                PackType.SERVER_DATA, repository.openAllSelected())) {
            LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags = TagLoader.loadTagsForExistingRegistries(
                    resources, initial.getLayer(RegistryLayer.STATIC));
            RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
            RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups,
                    RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool()).join();
            staticTags.forEach(Registry.PendingTags::apply);
        }

        int total = Block.BLOCK_STATE_REGISTRY.size();
        JsonObject header = new JsonObject();
        header.addProperty("type", "header");
        header.addProperty("version", SharedConstants.getCurrentVersion().id());
        header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
        header.addProperty("states", total);

        JsonArray self = new JsonArray();
        JsonArray neighbor = new JsonArray();
        JsonArray dynamic = new JsonArray();
        SingleStateView view = new SingleStateView();
        // 官方 PathfindingContext 的构造器要一个 Mob；这里用 Unsafe 造一个不跑构造器的实例，
        // 再把四个 final 字段设成「一格假世界、没有缓存」。这样 checkNeighbourBlocks
        // 调的还是官方原方法，不重写它的分类，也不需要真服务端或玩家。
        // PathfindingContext's constructor needs a Mob. An Unsafe-allocated instance
        // with a one-cell fake world and no cache lets the official
        // checkNeighbourBlocks do the classification without a server or a mob.
        Field unsafeField = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        Object unsafe = unsafeField.get(null);
        var allocate = unsafe.getClass().getMethod("allocateInstance", Class.class);
        PathfindingContext context = (PathfindingContext) allocate.invoke(unsafe, PathfindingContext.class);
        set(context, "level", view);
        set(context, "cache", null);
        set(context, "mobPosition", BlockPos.ZERO);
        set(context, "mutablePos", new BlockPos.MutableBlockPos());

        for (int id = 0; id < total; id++) {
            BlockState state = Block.BLOCK_STATE_REGISTRY.byId(id);
            boolean dynamicShape = state.getBlock().hasDynamicShape();
            dynamic.add(dynamicShape);
            if (dynamicShape) {
                self.add((String) null);
                neighbor.add((String) null);
                continue;
            }
            view.state = state;
            view.centre = BlockPos.ZERO;
            // 官方 protected static 方法，由同包的子类转发出去。
            // Forward the protected static official method from a subclass.
            PathType type = Evaluator.fromState(view, BlockPos.ZERO);
            self.add(type.name());
            view.centre = CENTRE;
            // 官方 checkNeighbourBlocks 按 (dx, dy, dz) 遍历 26 个邻居，跳过竖直那一列。
            // 把中心放在 (1,1,1)，26 个坐标都落在假世界的 3×3×3 里，只有中心那格是这个状态。
            // With the origin at (1,1,1), all 26 read positions land in the fake
            // 3³ view; only the centre has this state and the rest are air.
            PathType changed = PathType.OPEN;
            for (int[] offset : OFFSETS) {
                changed = WalkNodeEvaluator.checkNeighbourBlocks(
                        context, 1 + offset[0], 1 + offset[1], 1 + offset[2], changed);
            }
            neighbor.add(changed.name());
        }

        JsonObject caseLine = new JsonObject();
        caseLine.addProperty("type", "case");
        caseLine.add("self", self);
        caseLine.add("neighbor", neighbor);
        caseLine.add("dynamic", dynamic);
        JsonObject end = new JsonObject();
        end.addProperty("type", "end");
        end.addProperty("states", total);

        try (Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8),
                OUTPUT_BUFFER)) {
            out.write(header.toString());
            out.write('\n');
            out.write(caseLine.toString());
            out.write('\n');
            out.write(end.toString());
            out.write('\n');
        }
        log.println("wrote " + total + " block state path types");
    }

    private static void set(Object target, String name, Object value) throws Exception {
        Field field = PathfindingContext.class.getDeclaredField(name);
        field.setAccessible(true);
        field.set(target, value);
    }

    private static int[][] offsets() {
        List<int[]> result = new ArrayList<>();
        for (int x = -1; x <= 1; x++) {
            for (int y = -1; y <= 1; y++) {
                for (int z = -1; z <= 1; z++) {
                    if (x != 0 || z != 0) {
                        result.add(new int[]{x, y, z});
                    }
                }
            }
        }
        return result.toArray(new int[0][]);
    }

    /** 官方 `WalkNodeEvaluator.getPathTypeFromState` 是 protected static，子类里能直接调。 */
    /** Forward the protected static official method. */
    private static final class Evaluator extends WalkNodeEvaluator {
        static PathType fromState(BlockGetter view, BlockPos pos) {
            return getPathTypeFromState(view, pos);
        }
    }

    /**
     * 3×3×3 的 `CollisionGetter`：中心格是要查的状态，其余是空气。
     * 官方 `getPathTypeFromState` 从 `BlockState` 自己取流体，所以视图的
     * `getFluidState` 一律返回空流体；26 个邻居读到的是空气，正是我们要的分类。
     * 查询自身时中心放在 ZERO，扫描邻居时中心放在 (1,1,1)；两种情况下
     * 官方读的都是同一个 `BlockState`，`BlockPos` 不影响静态状态的形状。
     *
     * <p>A 3³ `CollisionGetter` with the queried state at the centre and air
     * elsewhere. Official `getPathTypeFromState` reads the fluid from the
     * `BlockState` itself, so the view reports empty fluid everywhere and all
     * 26 neighbours look like air. The self query uses ZERO as its centre and
     * the neighbour scan uses (1,1,1); both read the same `BlockState` and
     * static shapes do not depend on the position.
     */
    private static final class SingleStateView implements CollisionGetter {
        private BlockState state;
        private BlockPos centre = BlockPos.ZERO;
        @Override
        public BlockState getBlockState(BlockPos pos) {
            return isCentre(pos) ? state : Blocks.AIR.defaultBlockState();
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            // 官方 `getPathTypeFromState` 从 BlockState 自己取流体，不读这个视图；
            // 26 个邻居都是空气。
            // Official `getPathTypeFromState` reads the fluid from the BlockState
            // itself, not from this view; all 26 neighbours are air.
            return Fluids.EMPTY.defaultFluidState();
        }

        private boolean isCentre(BlockPos pos) {
            return pos.equals(centre);
        }

        @Override
        public BlockEntity getBlockEntity(BlockPos pos) {
            return null;
        }

        @Override
        public int getMinY() {
            return -64;
        }

        @Override
        public int getHeight() {
            return 384;
        }

        @Override
        public WorldBorder getWorldBorder() {
            return null;
        }

        @Override
        public BlockGetter getChunkForCollisions(int x, int z) {
            return this;
        }

        @Override
        public List<VoxelShape> getEntityCollisions(Entity entity, AABB box) {
            return List.of();
        }
    }
}
