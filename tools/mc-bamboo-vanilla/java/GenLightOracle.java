import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.TreeMap;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.DataLayer;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.LightChunk;
import net.minecraft.world.level.chunk.LightChunkGetter;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.lighting.LevelLightEngine;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;

/**
 * 生成期光照对拍：用官方的 {@code LevelLightEngine} 给一片 3×3 的方块算
 * {@code initializeLight} + {@code light}（生成期那两步），把中心区块逐格的天空光、
 * 方块光，连同这片 3×3 的方块状态 ID 一起输出；Bamboo 侧拿同一份方块喂
 * {@code generate_light}，逐格比较。不启动服务端、不读存档、不接受 EULA。
 *
 * <p>题目是一片确定性合成场景：中心 3×3 铺一层实心地面，再随机撒光源（萤石等）、
 * 透光/遮光方块（水、玻璃、台阶）和挖空的洞，外圈 5×5 全是空气。外圈给光引擎一个
 * 已加载的空邻域（天空光 15、无方块光），正好等于 Bamboo {@code generate_light}
 * 对视图之外 {@code Chunk::empty} 的读法；空气不影响中心结果，因为光最多走 15 格。
 *
 * <p>输出一行一个 JSON：header、每道题一条 genlight（palette 是官方
 * {@code BLOCK_STATE_REGISTRY.getId()} 的状态 ID，chunks 是中心 3×3 九宫格各方格在
 * palette 里的下标，sky/block 是中心柱逐格光照，顺序都是 y 从下到上、x 0..15、z 0..15）、end。
 *
 * <p>Generate-time light oracle: drive the official {@code LevelLightEngine}'s
 * {@code initializeLight} + {@code light} steps over a 3×3 of blocks and emit the
 * center column's per-cell sky/block light, together with the 3×3 block state IDs,
 * so Bamboo's {@code generate_light} is checked on the exact same blocks. No server,
 * no save, no EULA. Scenes are deterministic synthetic terrain (solid ground +
 * scattered emitters, filters, shaped blocks, carved caves) with an all-air 5×5
 * outer ring matching Bamboo's air-outside-the-view boundary. One JSON line each:
 * header, genlight per scene, end.
 */
public final class GenLightOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final int WIDTH = 16;
    /** 每柱逐格数：y(384) × x(16) × z(16)。 Cells per column. */
    private static final int CELLS = HEIGHT * WIDTH * WIDTH;
    /** 5×5 邻域，中心 (0,0) 在下标 2。 5×5 neighbourhood, (0,0) at index 2. */
    private static final int RADIUS = 2;
    private static final int GRID = RADIUS * 2 + 1;
    private static final int CENTER = RADIUS;
    /** ThreadedLevelLightEngine 一批最多跑的任务数。 Tasks per batch. */
    private static final int TASKS_PER_BATCH = 1000;
    private static final int OUTPUT_BUFFER = 4 << 20;

    private GenLightOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        // 官方后台线程不是守护线程，不主动退出进程就不会结束。
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        // 官方日志默认写 stdout，要抢在日志框架初始化之前换掉，否则日志会混进 JSON。
        System.setOut(log);
        Options options = Options.parse(args);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            PalettedContainerFactory containers = PalettedContainerFactory.create(registries);
            new Oracle(containers, options, out).run();
        }
    }

    /** 和 WorldLoader.load 的前半段相同（与 LightOracle.java 一致）。 Load world registries. */
    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(CloseableResourceManager resources) {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }

    private record Options(int scenes, long seed) {
        static Options parse(String[] args) {
            int scenes = 4;
            long seed = 1;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--scenes" -> scenes = Integer.parseInt(args[++i]);
                    case "--seed" -> seed = Long.parseLong(args[++i]);
                    default -> throw new IllegalArgumentException("unknown option " + args[i]);
                }
            }
            return new Options(scenes, seed);
        }
    }

    /** 一道题用的方块：实心地面、光源、透光方块、形状遮光方块。 Scene block palette. */
    private static final class Palette {
        final BlockState[] blocks = {
            Blocks.STONE.defaultBlockState(),
            Blocks.DEEPSLATE.defaultBlockState(),
            Blocks.DIRT.defaultBlockState(),
            Blocks.GLOWSTONE.defaultBlockState(),
            Blocks.SEA_LANTERN.defaultBlockState(),
            Blocks.WATER.defaultBlockState(),
            Blocks.GLASS.defaultBlockState(),
            Blocks.ICE.defaultBlockState(),
            Blocks.STONE_SLAB.defaultBlockState(),
            Blocks.OAK_STAIRS.defaultBlockState(),
        };
        final int ground;
        final int[] emitters = {3, 4};
        final int[] scatter = {5, 6, 7, 8, 9};

        Palette() {
            ground = 0;
        }

        int id(int index) {
            return Block.BLOCK_STATE_REGISTRY.getId(blocks[index]);
        }
    }

    private static final class Oracle {
        private final PalettedContainerFactory containers;
        private final Options options;
        private final Writer out;
        private final Palette palette = new Palette();
        private long checks;

        Oracle(PalettedContainerFactory containers, Options options, Writer out) {
            this.containers = containers;
            this.options = options;
            this.out = out;
        }

        void run() throws Exception {
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            header.addProperty("minY", MIN_Y);
            header.addProperty("height", HEIGHT);
            header.addProperty("cells", CELLS);
            write(header);

            for (int index = 0; index < options.scenes(); index++) {
                write(scene(index));
            }

            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            end.addProperty("scenes", options.scenes());
            end.addProperty("checks", this.checks);
            write(end);
            this.out.flush();
        }

        private JsonObject scene(int index) {
            Random random = new Random(options.seed() * 1_000_003L + index);
            LightWorld world = new LightWorld(containers, palette, random);

            // palette：中心 3×3 用到的全部状态 ID（空气 0 恒在下标 0）。
            // Palette: every state id used in the center 3x3 (air 0 always index 0).
            LinkedHashMap<Integer, Integer> ids = new LinkedHashMap<>();
            ids.put(0, 0);
            JsonArray paletteArray = new JsonArray();
            paletteArray.add(0);
            for (int gz = CENTER - 1; gz <= CENTER + 1; gz++) {
                for (int gx = CENTER - 1; gx <= CENTER + 1; gx++) {
                    for (int state : world.states[gx][gz]) {
                        if (!ids.containsKey(state)) {
                            ids.put(state, ids.size());
                            paletteArray.add(state);
                        }
                    }
                }
            }

            JsonArray chunks = new JsonArray();
            for (int gz = CENTER - 1; gz <= CENTER + 1; gz++) {
                for (int gx = CENTER - 1; gx <= CENTER + 1; gx++) {
                    JsonObject entry = new JsonObject();
                    entry.addProperty("dx", gx - CENTER);
                    entry.addProperty("dz", gz - CENTER);
                    JsonArray blocks = new JsonArray();
                    for (int state : world.states[gx][gz]) {
                        blocks.add(ids.get(state));
                    }
                    entry.add("blocks", blocks);
                    chunks.add(entry);
                }
            }

            // 中心 (0,0) 柱逐格天空光 / 方块光。 Center column per-cell sky/block light.
            JsonArray sky = new JsonArray();
            JsonArray block = new JsonArray();
            for (int y = 0; y < HEIGHT; y++) {
                for (int x = 0; x < WIDTH; x++) {
                    for (int z = 0; z < WIDTH; z++) {
                        BlockPos pos = new BlockPos(x, MIN_Y + y, z);
                        sky.add(world.engine.getLayerListener(LightLayer.SKY).getLightValue(pos));
                        block.add(world.engine.getLayerListener(LightLayer.BLOCK).getLightValue(pos));
                        this.checks += 2;
                    }
                }
            }

            JsonObject line = new JsonObject();
            line.addProperty("type", "genlight");
            line.addProperty("index", index);
            line.add("palette", paletteArray);
            line.add("chunks", chunks);
            line.add("sky", sky);
            line.add("block", block);
            return line;
        }

        private void write(JsonObject line) throws Exception {
            this.out.write(line.toString());
            this.out.write('\n');
        }
    }

    /** 一片 5×5 区块和它们的光照引擎，官方的 {@code LightChunkGetter}。 */
    private static final class LightWorld implements LightChunkGetter {
        private final Map<Long, ProtoChunk> chunks = new HashMap<>();
        private final BatchedLightEngine engine;
        /** 每格的状态 ID（空气 0），按网格下标 [gx][gz][cell]。 Per-cell state ids by grid. */
        private final int[][][] states = new int[GRID][GRID][];
        private final BlockGetter level = new BlockGetter() {
            @Override
            public BlockEntity getBlockEntity(BlockPos pos) {
                return null;
            }

            @Override
            public BlockState getBlockState(BlockPos pos) {
                return Blocks.AIR.defaultBlockState();
            }

            @Override
            public FluidState getFluidState(BlockPos pos) {
                return Fluids.EMPTY.defaultFluidState();
            }

            @Override
            public int getHeight() {
                return HEIGHT;
            }

            @Override
            public int getMinY() {
                return MIN_Y;
            }
        };

        LightWorld(PalettedContainerFactory containers, Palette palette, Random random) {
            for (int gx = 0; gx < GRID; gx++) {
                for (int gz = 0; gz < GRID; gz++) {
                    states[gx][gz] = new int[CELLS];
                }
            }
            buildScene(palette, random);
            engine = new BatchedLightEngine(this);
            for (int gx = 0; gx < GRID; gx++) {
                for (int gz = 0; gz < GRID; gz++) {
                    int cx = gx - CENTER;
                    int cz = gz - CENTER;
                    ProtoChunk chunk = new ProtoChunk(new ChunkPos(cx, cz), UpgradeData.EMPTY, level, containers, null);
                    fillChunk(chunk, cx, cz, states[gx][gz], containers);
                    chunks.put(ChunkPos.pack(cx, cz), chunk);
                }
            }
            // ChunkStatusTasks.initializeLight：登记非空段、算天空光源表。
            for (ProtoChunk chunk : chunks.values()) {
                initializeLight(chunk);
            }
            engine.flush();
            // ChunkStatusTasks.light：逐个点亮光源，跑到底。
            for (ProtoChunk chunk : chunks.values()) {
                engine.propagateLightSources(chunk.getPos());
                chunk.setPersistedStatus(ChunkStatus.LIGHT);
            }
            engine.flush();
        }

        /** 确定性合成场景：中心 3×3 实心地面 + 光源 / 透光 / 形状方块 / 挖洞，外圈全空气。 */
        private void buildScene(Palette palette, Random random) {
            int groundLayer = 40 + random.nextInt(40); // 地面上表面（0 基 y）。
            int ground = palette.id(palette.ground);
            int air = 0;
            for (int gx = CENTER - 1; gx <= CENTER + 1; gx++) {
                for (int gz = CENTER - 1; gz <= CENTER + 1; gz++) {
                    int[] cells = states[gx][gz];
                    for (int y = 0; y < groundLayer; y++) {
                        java.util.Arrays.fill(cells, y * 256, (y + 1) * 256, ground);
                    }
                }
            }
            // 几簇光源放在地面之上的空气里，制造清晰的方块光梯度；
            // 埋在不透明地面里的光源光传不出来，测不到方块光扩散。
            // Emitter clusters sit in the air above ground for a clear block-light
            // halo; ones buried in opaque ground cannot propagate light.
            for (int cluster = 0; cluster < 10; cluster++) {
                int emitter = palette.id(palette.emitters[random.nextInt(palette.emitters.length)]);
                int gx = CENTER - 1 + random.nextInt(3);
                int gz = CENTER - 1 + random.nextInt(3);
                int y = groundLayer + 1 + random.nextInt(32);
                int x = random.nextInt(WIDTH);
                int z = random.nextInt(WIDTH);
                for (int dy = -1; dy <= 1; dy++) {
                    for (int dx = -1; dx <= 1; dx++) {
                        for (int dz = -1; dz <= 1; dz++) {
                            put(gx, gz, y + dy, x + dx, z + dz, emitter);
                        }
                    }
                }
            }
            // 零散的透光 / 形状遮光方块，穿插在地面和空中。
            for (int i = 0; i < 600; i++) {
                int state = palette.id(palette.scatter[random.nextInt(palette.scatter.length)]);
                int gx = CENTER - 1 + random.nextInt(3);
                int gz = CENTER - 1 + random.nextInt(3);
                int y = random.nextInt(HEIGHT);
                put(gx, gz, y, random.nextInt(WIDTH), random.nextInt(WIDTH), state);
            }
            // 挖几条竖井，让天空光钻到地下。 Carve a few vertical shafts.
            for (int shaft = 0; shaft < 6; shaft++) {
                int gx = CENTER - 1 + random.nextInt(3);
                int gz = CENTER - 1 + random.nextInt(3);
                int x = random.nextInt(WIDTH);
                int z = random.nextInt(WIDTH);
                int top = 8 + random.nextInt(groundLayer - 8);
                for (int y = 0; y <= top; y++) {
                    put(gx, gz, y, x, z, air);
                    put(gx, gz, y, (x + 1) & 15, z, air);
                    put(gx, gz, y, x, (z + 1) & 15, air);
                }
            }
        }

        /** 把网格坐标折回本格内（跨 16 边界时挪到相邻中心 3×3 格），越界就丢掉。 */
        private void put(int gx, int gz, int y, int x, int z, int state) {
            if (y < 0 || y >= HEIGHT) {
                return;
            }
            int cellGx = gx + Math.floorDiv(x, WIDTH);
            int cellGz = gz + Math.floorDiv(z, WIDTH);
            int localX = Math.floorMod(x, WIDTH);
            int localZ = Math.floorMod(z, WIDTH);
            if (cellGx < CENTER - 1 || cellGx > CENTER + 1
                    || cellGz < CENTER - 1 || cellGz > CENTER + 1) {
                return;
            }
            states[cellGx][cellGz][y * 256 + localX * WIDTH + localZ] = state;
        }

        /** 把非零状态 ID 的方块写进 ProtoChunk（默认就是空气）。 */
        private static void fillChunk(ProtoChunk chunk, int cx, int cz, int[] cells,
                PalettedContainerFactory containers) {
            for (int y = 0; y < HEIGHT; y++) {
                for (int x = 0; x < WIDTH; x++) {
                    for (int z = 0; z < WIDTH; z++) {
                        int state = cells[y * 256 + x * WIDTH + z];
                        if (state != 0) {
                            BlockState blockState = Block.BLOCK_STATE_REGISTRY.byId(state);
                            chunk.setBlockState(new BlockPos(cx * 16 + x, MIN_Y + y, cz * 16 + z), blockState, 0);
                        }
                    }
                }
            }
        }

        /** ChunkStatusTasks.initializeLight 和 ThreadedLevelLightEngine.initializeLight。 */
        private void initializeLight(ProtoChunk chunk) {
            chunk.initializeLightSources();
            chunk.setLightEngine(engine);
            chunk.setPersistedStatus(ChunkStatus.INITIALIZE_LIGHT);
            LevelChunkSection[] sections = chunk.getSections();
            for (int index = 0; index < sections.length; index++) {
                if (!sections[index].hasOnlyAir()) {
                    int sectionY = chunk.getSectionYFromSectionIndex(index);
                    engine.updateSectionStatus(SectionPos.of(chunk.getPos(), sectionY), false);
                }
            }
            engine.setLightEnabled(chunk.getPos(), false);
            engine.retainData(chunk.getPos(), false);
        }

        @Override
        public LightChunk getChunkForLighting(int x, int z) {
            return chunks.get(ChunkPos.pack(x, z));
        }

        @Override
        public BlockGetter getLevel() {
            return level;
        }
    }

    /** ThreadedLevelLightEngine 的任务队列，不开线程：攒够一批就算，flush 把剩下的算完。 */
    private static final class BatchedLightEngine extends LevelLightEngine {
        private final List<Runnable> tasks = new ArrayList<>();

        BatchedLightEngine(LightChunkGetter chunks) {
            super(chunks, true, true);
        }

        @Override
        public void checkBlock(BlockPos pos) {
            BlockPos immutable = pos.immutable();
            addTask(() -> super.checkBlock(immutable));
        }

        @Override
        public void updateSectionStatus(SectionPos pos, boolean sectionEmpty) {
            addTask(() -> super.updateSectionStatus(pos, sectionEmpty));
        }

        private void addTask(Runnable task) {
            tasks.add(task);
            if (tasks.size() >= TASKS_PER_BATCH) {
                runUpdate();
            }
        }

        private void runUpdate() {
            int count = Math.min(tasks.size(), TASKS_PER_BATCH);
            for (int i = 0; i < count; i++) {
                tasks.get(i).run();
            }
            super.runLightUpdates();
            tasks.subList(0, count).clear();
        }

        void flush() {
            while (!tasks.isEmpty() || hasLightWork()) {
                runUpdate();
            }
        }
    }
}
