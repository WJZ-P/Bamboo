import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
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
 * 用官方的光照引擎给 Bamboo 的光照出题和给答案。
 *
 * <p>不启动服务端。一道题是一片 7×7 个真正的 ProtoChunk，先按官方生成区块的流程铺好超平坦地层、
 * 登记有方块的区块段（initializeLight）、点亮光源（lightChunk），再分几个 tick 改中间 3×3 个区块里的方块。
 * 改方块走 ProtoChunk.setBlockState，它自己调用光照引擎的 checkBlock 和 updateSectionStatus。
 * 光照引擎是官方的 LevelLightEngine，外面套一层和 ThreadedLevelLightEngine 一样的任务队列：
 * 任务攒够 1000 个就算一轮，一个 tick 结束时把剩下的算完。只是不开线程，结果是确定的。
 *
 * <p>每个 tick 之后写出：49 个区块柱 52 个光照段的哈希、onLightUpdate 报告的段（按 ChunkHolder.sectionLightChanged
 * 换成每个区块两个位图）、随机位置和改过的位置上的 getLightValue。改动影响不到外面两圈，所以外圈没加载的区块不影响结果。
 *
 * <p>输出一行一个 JSON：header（版本、模板柱的哈希）、scene（一道题的全部 tick）、end。
 *
 * <p>哈希是 64 位 FNV-1a，依次喂入天空光和方块光的 26 段：不存光照写 0；没分配数组写 1 和默认值；
 * 有数组写 2 和 2048 个字节。--detail 时再给每段一个描述：A 不存，F 加默认值，D 加数组的哈希。
 */
public final class LightOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final int MIN_LIGHT_SECTION = (MIN_Y >> 4) - 1;
    private static final int LIGHT_SECTIONS = HEIGHT / 16 + 2;
    /** 一道题铺的区块半径。 */
    private static final int RADIUS = 3;
    /** 改方块的范围：中间 3×3 个区块。 */
    private static final int EDIT_MIN = -16;
    private static final int EDIT_MAX = 31;
    /** 抽查 getLightValue 的范围：中间 5×5 个区块。 */
    private static final int SAMPLE_MIN = -32;
    private static final int SAMPLE_MAX = 47;
    private static final int SAMPLES_PER_TICK = 64;
    /** ThreadedLevelLightEngine 一批最多跑的任务数。 */
    private static final int TASKS_PER_BATCH = 1000;
    private static final long FNV_OFFSET = 0xcbf29ce484222325L;
    private static final long FNV_PRIME = 0x100000001b3L;
    private static final int OUTPUT_BUFFER = 4 << 20;

    private LightOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        // 官方的后台线程不是守护线程，不主动退出进程就不会结束。
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

    /** 和 WorldLoader.load 的前半段相同。区块的生物群系容器要用世界层的注册表。 */
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

    private record Options(int scenes, long seed, int only, boolean detail) {
        static Options parse(String[] args) {
            int scenes = 1000;
            long seed = 1;
            int only = -1;
            boolean detail = false;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--scenes" -> scenes = Integer.parseInt(args[++i]);
                    case "--seed" -> seed = Long.parseLong(args[++i]);
                    case "--scene" -> only = Integer.parseInt(args[++i]);
                    case "--detail" -> detail = true;
                    default -> throw new IllegalArgumentException("unknown option " + args[i]);
                }
            }
            return new Options(scenes, seed, only, detail);
        }
    }

    /** 出题用的方块，按光照性质分好类。 */
    private static final class Palettes {
        final List<BlockState> all = new ArrayList<>();
        final List<BlockState> emitters = new ArrayList<>();
        final List<BlockState> shaped = new ArrayList<>();
        final List<BlockState> filters = new ArrayList<>();
        final List<BlockState> clear = new ArrayList<>();
        final List<BlockState> opaque = new ArrayList<>();

        Palettes() {
            for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
                // 洞穴空气和虚空空气放进空区块段时，ProtoChunk 和 LevelChunk 的处理不同，题目里不用。
                if (state.isAir()) {
                    continue;
                }
                all.add(state);
                if (state.getLightEmission() > 0) {
                    emitters.add(state);
                }
                if (state.canOcclude() && state.useShapeForLightOcclusion()) {
                    shaped.add(state);
                } else if (state.getLightDampening() >= 15) {
                    opaque.add(state);
                } else if (state.getLightDampening() > 0) {
                    filters.add(state);
                } else {
                    clear.add(state);
                }
            }
        }

        /** 一道题用的几种方块：每类至少一种，再加两种随便什么。 */
        List<BlockState> pick(Random random) {
            List<BlockState> palette = new ArrayList<>();
            for (List<BlockState> kind : List.of(emitters, emitters, shaped, shaped, filters, clear, opaque, all, all)) {
                palette.add(kind.get(random.nextInt(kind.size())));
            }
            palette.add(Blocks.STONE.defaultBlockState());
            return palette;
        }
    }

    private static final class Oracle {
        private final PalettedContainerFactory containers;
        private final Options options;
        private final Writer out;
        private final Palettes palettes = new Palettes();

        Oracle(PalettedContainerFactory containers, Options options, Writer out) {
            this.containers = containers;
            this.options = options;
            this.out = out;
        }

        void run() throws IOException {
            World template = new World(containers);
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            header.addProperty("radius", RADIUS);
            header.addProperty("template", hex(template.columnHash(0, 0)));
            write(header);

            int written = 0;
            for (int index = 0; index < options.scenes(); index++) {
                if (options.only() >= 0 && index != options.only()) {
                    continue;
                }
                write(scene(index));
                written++;
            }

            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            end.addProperty("scenes", written);
            write(end);
        }

        private void write(JsonObject line) throws IOException {
            out.write(line.toString());
            out.write('\n');
        }

        private JsonObject scene(int index) {
            Random random = new Random(options.seed() * 1_000_003L + index);
            List<BlockState> palette = palettes.pick(random);
            World world = new World(containers);
            Edits generator = new Edits(random, palette);
            JsonArray ticks = new JsonArray();
            int tickCount = 1 + random.nextInt(6);
            for (int tick = 0; tick < tickCount; tick++) {
                ticks.add(world.runTick(generator.next(), random, options.detail()));
            }
            JsonObject scene = new JsonObject();
            scene.addProperty("type", "scene");
            scene.addProperty("index", index);
            scene.add("ticks", ticks);
            return scene;
        }
    }

    private record Edit(BlockPos pos, BlockState state) {
    }

    /**
     * 一道题每个 tick 的改动：几块方块、屋顶、零散的方块、挖空、天上的方块、拆掉天上的方块、挖穿世界底部。
     * 天上的方块记下来，拆的时候整段又变空，区块段不再存光照的那条路才走得到。
     */
    private static final class Edits {
        private final Random random;
        private final List<BlockState> palette;
        private final List<BlockPos> sky = new ArrayList<>();

        Edits(Random random, List<BlockState> palette) {
            this.random = random;
            this.palette = palette;
        }

        List<Edit> next() {
            List<Edit> edits = new ArrayList<>();
            int operations = 1 + random.nextInt(3);
            for (int i = 0; i < operations; i++) {
                int roll = random.nextInt(100);
                BlockState state = palette.get(random.nextInt(palette.size()));
                if (roll < 25) {
                    box(edits, random, state, MIN_Y + random.nextInt(24), 1 + random.nextInt(10), 1 + random.nextInt(5));
                } else if (roll < 42) {
                    box(edits, random, state, MIN_Y + 4 + random.nextInt(16), 4 + random.nextInt(21), 1);
                } else if (roll < 64) {
                    scatter(edits, random, palette, MIN_Y + random.nextInt(20), 20);
                } else if (roll < 78) {
                    box(edits, random, Blocks.AIR.defaultBlockState(), MIN_Y + random.nextInt(24),
                            1 + random.nextInt(10), 1 + random.nextInt(5));
                } else if (roll < 87) {
                    int base = random.nextBoolean() ? MIN_Y + HEIGHT - 40 : 100;
                    int start = edits.size();
                    scatter(edits, random, palette, base + random.nextInt(40), 0);
                    edits.subList(start, edits.size()).forEach(edit -> sky.add(edit.pos()));
                } else if (roll < 94) {
                    sky.forEach(pos -> edits.add(new Edit(pos, Blocks.AIR.defaultBlockState())));
                    sky.clear();
                } else {
                    box(edits, random, Blocks.AIR.defaultBlockState(), MIN_Y, 1 + random.nextInt(4), 4);
                }
            }
            return edits;
        }

        /** 一个横向边长 `size`、高 `height` 的长方体，落在改方块的范围里。 */
        private static void box(List<Edit> edits, Random random, BlockState state, int y0, int size, int height) {
            int x0 = EDIT_MIN + random.nextInt(EDIT_MAX - EDIT_MIN + 1);
            int z0 = EDIT_MIN + random.nextInt(EDIT_MAX - EDIT_MIN + 1);
            int sizeZ = Math.max(1, size + random.nextInt(5) - 2);
            for (int x = x0; x < x0 + size && x <= EDIT_MAX; x++) {
                for (int y = y0; y < y0 + height && y < MIN_Y + HEIGHT; y++) {
                    for (int z = z0; z < z0 + sizeZ && z <= EDIT_MAX; z++) {
                        edits.add(new Edit(new BlockPos(x, y, z), state));
                    }
                }
            }
        }

        /** 零散的方块，高度在 `y` 上下 `spread` 格里。 */
        private static void scatter(List<Edit> edits, Random random, List<BlockState> palette, int y, int spread) {
            int count = 1 + random.nextInt(24);
            for (int i = 0; i < count; i++) {
                int x = EDIT_MIN + random.nextInt(EDIT_MAX - EDIT_MIN + 1);
                int z = EDIT_MIN + random.nextInt(EDIT_MAX - EDIT_MIN + 1);
                int blockY = Math.min(MIN_Y + HEIGHT - 1, y + (spread == 0 ? 0 : random.nextInt(spread)));
                edits.add(new Edit(new BlockPos(x, blockY, z), palette.get(random.nextInt(palette.size()))));
            }
        }
    }

    /** 一片区块和它们的光照引擎，官方的 LightChunkGetter。 */
    private static final class World implements LightChunkGetter {
        private final Map<Long, ProtoChunk> chunks = new HashMap<>();
        private final BatchedLightEngine engine;
        /** onLightUpdate 报告的段：区块 → 天空光和方块光的位图。 */
        private final TreeMap<Long, int[]> changes = new TreeMap<>();
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

        World(PalettedContainerFactory containers) {
            engine = new BatchedLightEngine(this);
            for (int x = -RADIUS; x <= RADIUS; x++) {
                for (int z = -RADIUS; z <= RADIUS; z++) {
                    ProtoChunk chunk = new ProtoChunk(new ChunkPos(x, z), UpgradeData.EMPTY, level, containers, null);
                    fillFlat(chunk, x, z);
                    chunks.put(ChunkPos.pack(x, z), chunk);
                }
            }
            for (ProtoChunk chunk : chunks.values()) {
                initializeLight(chunk);
            }
            engine.flush();
            for (ProtoChunk chunk : chunks.values()) {
                engine.propagateLightSources(chunk.getPos());
                chunk.setPersistedStatus(ChunkStatus.LIGHT);
            }
            engine.flush();
            changes.clear();
        }

        /** 官方超平坦的默认地层：基岩、两层泥土、草方块。 */
        private static void fillFlat(ProtoChunk chunk, int chunkX, int chunkZ) {
            BlockState[] layers = {
                Blocks.BEDROCK.defaultBlockState(),
                Blocks.DIRT.defaultBlockState(),
                Blocks.DIRT.defaultBlockState(),
                Blocks.GRASS_BLOCK.defaultBlockState()
            };
            for (int y = 0; y < layers.length; y++) {
                for (int x = 0; x < 16; x++) {
                    for (int z = 0; z < 16; z++) {
                        chunk.setBlockState(new BlockPos(chunkX * 16 + x, MIN_Y + y, chunkZ * 16 + z), layers[y], 0);
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

        /** ServerChunkCache.onLightUpdate 和 ChunkHolder.sectionLightChanged：只记 -5 到 21 段。 */
        @Override
        public void onLightUpdate(LightLayer layer, SectionPos pos) {
            int index = pos.y() - MIN_LIGHT_SECTION;
            if (index < 0 || index > LIGHT_SECTIONS) {
                return;
            }
            int[] bits = changes.computeIfAbsent(ChunkPos.pack(pos.x(), pos.z()), key -> new int[2]);
            bits[layer == LightLayer.SKY ? 0 : 1] |= 1 << index;
        }

        JsonObject runTick(List<Edit> edits, Random random, boolean detail) {
            changes.clear();
            JsonArray editList = new JsonArray();
            for (Edit edit : edits) {
                BlockPos pos = edit.pos();
                chunks.get(ChunkPos.pack(pos.getX() >> 4, pos.getZ() >> 4)).setBlockState(pos, edit.state(), 3);
                editList.add(numbers(pos.getX(), pos.getY(), pos.getZ(), Block.BLOCK_STATE_REGISTRY.getId(edit.state())));
            }
            engine.flush();

            JsonObject tick = new JsonObject();
            tick.add("edits", editList);
            tick.add("columns", columns(detail));
            tick.add("changes", changeList());
            tick.add("samples", samples(edits, random));
            return tick;
        }

        private JsonArray columns(boolean detail) {
            JsonArray columns = new JsonArray();
            for (int x = -RADIUS; x <= RADIUS; x++) {
                for (int z = -RADIUS; z <= RADIUS; z++) {
                    if (detail) {
                        JsonArray sections = new JsonArray();
                        for (LightLayer layer : new LightLayer[] {LightLayer.SKY, LightLayer.BLOCK}) {
                            for (int index = 0; index < LIGHT_SECTIONS; index++) {
                                sections.add(describe(section(layer, x, z, index)));
                            }
                        }
                        columns.add(sections);
                    } else {
                        columns.add(hex(columnHash(x, z)));
                    }
                }
            }
            return columns;
        }

        private JsonArray changeList() {
            JsonArray list = new JsonArray();
            changes.forEach((key, bits) -> list.add(numbers(ChunkPos.getX(key), ChunkPos.getZ(key), bits[0], bits[1])));
            return list;
        }

        private JsonArray samples(List<Edit> edits, Random random) {
            List<BlockPos> positions = new ArrayList<>();
            for (int i = 0; i < SAMPLES_PER_TICK; i++) {
                positions.add(new BlockPos(
                        SAMPLE_MIN + random.nextInt(SAMPLE_MAX - SAMPLE_MIN + 1),
                        MIN_Y - 16 + random.nextInt(HEIGHT + 32),
                        SAMPLE_MIN + random.nextInt(SAMPLE_MAX - SAMPLE_MIN + 1)));
            }
            for (int i = 0; i < Math.min(edits.size(), SAMPLES_PER_TICK); i++) {
                positions.add(edits.get(random.nextInt(edits.size())).pos().above());
            }
            JsonArray samples = new JsonArray();
            for (BlockPos pos : positions) {
                samples.add(numbers(pos.getX(), pos.getY(), pos.getZ(),
                        engine.getLayerListener(LightLayer.SKY).getLightValue(pos),
                        engine.getLayerListener(LightLayer.BLOCK).getLightValue(pos)));
            }
            return samples;
        }

        private DataLayer section(LightLayer layer, int x, int z, int index) {
            return engine.getLayerListener(layer).getDataLayerData(SectionPos.of(x, MIN_LIGHT_SECTION + index, z));
        }

        long columnHash(int x, int z) {
            long hash = FNV_OFFSET;
            for (LightLayer layer : new LightLayer[] {LightLayer.SKY, LightLayer.BLOCK}) {
                for (int index = 0; index < LIGHT_SECTIONS; index++) {
                    DataLayer data = section(layer, x, z, index);
                    if (data == null) {
                        hash = mix(hash, 0);
                    } else if (data.isDefinitelyHomogenous()) {
                        hash = mix(mix(hash, 1), filledValue(data));
                    } else {
                        hash = mix(hash, 2);
                        for (byte value : data.getData()) {
                            hash = mix(hash, value & 0xff);
                        }
                    }
                }
            }
            return hash;
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

        /** ThreadedLevelLightEngine.runUpdate：最多 1000 个任务，然后算一轮。 */
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

    private static int filledValue(DataLayer data) {
        for (int value = 0; value < 16; value++) {
            if (data.isDefinitelyFilledWith(value)) {
                return value;
            }
        }
        throw new IllegalStateException("homogeneous layer without a value");
    }

    private static String describe(DataLayer data) {
        if (data == null) {
            return "A";
        }
        if (data.isDefinitelyHomogenous()) {
            return "F" + filledValue(data);
        }
        long hash = FNV_OFFSET;
        for (byte value : data.getData()) {
            hash = mix(hash, value & 0xff);
        }
        return "D" + hex(hash);
    }

    private static long mix(long hash, int value) {
        return (hash ^ value) * FNV_PRIME;
    }

    private static String hex(long value) {
        return String.format("%016x", value);
    }

    private static JsonArray numbers(int... values) {
        JsonArray array = new JsonArray();
        for (int value : values) {
            array.add(value);
        }
        return array;
    }
}
