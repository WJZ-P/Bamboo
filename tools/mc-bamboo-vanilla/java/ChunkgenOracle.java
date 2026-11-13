import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.function.Supplier;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.QuartPos;
import net.minecraft.core.SectionPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;

/**
 * 一个 3×3 的假 {@code WorldGenLevel}。
 *
 * <p>只实现 {@code applyBiomeDecoration} 和已移植地物真正用到的方法，每个方法都照
 * {@code WorldGenRegion} 的字节码（{@code docs/worldgen-features-plan.md} 4.1）；
 * 没实现的方法一律抛 {@code UnsupportedOperationException}，让没覆盖到的路径立刻暴露，
 * 不返回悄悄算错的默认值。
 */
final class FeatureLevelHandler implements InvocationHandler {
    private final ChunkPos center;
    private final long seed;
    private final int minY;
    private final int height;
    private final int seaLevel;
    private final TerrainChunkView[] chunks;
    private final RegistryAccess registries;
    private final BiomeManager biomeManager;
    private final net.minecraft.util.RandomSource random;
    private final net.minecraft.world.ticks.WorldGenTickAccess<net.minecraft.world.level.block.Block> blockTicks;
    private final net.minecraft.world.ticks.WorldGenTickAccess<net.minecraft.world.level.material.Fluid> fluidTicks;
    private Supplier<String> currentlyGenerating;
    private long nextSubTick;


    FeatureLevelHandler(ChunkPos center, long seed, NoiseGeneratorSettings settings,
            TerrainChunkView[] chunks, RegistryAccess registries, BiomeManager biomeManager,
            RandomState randomState) {
        this.center = center;
        this.seed = seed;
        this.minY = settings.noiseSettings().minY();
        this.height = settings.noiseSettings().height();
        this.seaLevel = settings.seaLevel();
        this.chunks = chunks;
        this.registries = registries;
        this.biomeManager = biomeManager;
        // WorldGenRegion 构造 106-130：randomState 的 worldgen_region_random factory
        // 在中心区块的 origin 上取一个随机源。地物放置本身不用它，但 getRandom() 要给对。
        this.random = randomState
                .getOrCreateRandomFactory(Identifier.withDefaultNamespace("worldgen_region_random"))
                .at(center.getWorldPosition());
        // WorldGenRegion 构造 5-35：两个 WorldGenTickAccess 把计划刻转发给对应区块的容器。
        this.blockTicks = new net.minecraft.world.ticks.WorldGenTickAccess<>(
                pos -> chunkAt(pos).chunk().getBlockTicks());
        this.fluidTicks = new net.minecraft.world.ticks.WorldGenTickAccess<>(
                pos -> chunkAt(pos).chunk().getFluidTicks());
    }

    @Override
    public Object invoke(Object proxy, Method method, Object[] args) throws Throwable {
        String name = method.getName();
        int arity = args == null ? 0 : args.length;
        switch (name) {
            case "getBlockState":
                return chunkAt((BlockPos) args[0]).chunk().getBlockState((BlockPos) args[0]);
            case "getFluidState":
                return chunkAt((BlockPos) args[0]).chunk().getFluidState((BlockPos) args[0]);
            case "setBlock": {
                // LevelWriter 的 default 重载会转调 4 参数的 setBlock(pos,state,flags,recursionLeft)。
                BlockPos pos = (BlockPos) args[0];
                BlockState state = (BlockState) args[1];
                int flags = arity >= 3 ? (Integer) args[2] : 3;
                return setBlock(pos, state, flags);
            }
            case "setBlockAndUpdate":
                return setBlock((BlockPos) args[0], (BlockState) args[1], 3);
            case "removeBlock":
                return setBlock((BlockPos) args[0], Blocks.AIR.defaultBlockState(), 3);
            case "destroyBlock": {
                BlockPos pos = (BlockPos) args[0];
                if (getBlockState(pos).isAir()) {
                    return false;
                }
                return setBlock(pos, Blocks.AIR.defaultBlockState(), 3);
            }
            case "ensureCanWrite":
                return withinWriteZone((BlockPos) args[0]);
            case "isWithinWriteZone":
                return withinWriteZone((BlockPos) args[0]);
            case "setCurrentlyGenerating":
                @SuppressWarnings("unchecked")
                Supplier<String> generating = (Supplier<String>) args[0];
                this.currentlyGenerating = generating;
                return null;
            case "getChunk": {
                if (arity == 1 && args[0] instanceof BlockPos pos) {
                    return chunkAt(pos).chunk();
                }
                int x = (Integer) args[0];
                int z = (Integer) args[1];
                return chunk(x, z).chunk();
            }
            case "hasChunk":
            case "hasChunkAt": {
                if (arity >= 1 && args[0] instanceof BlockPos pos) {
                    return withinWriteZone(pos);
                }
                int x = (Integer) args[0];
                int z = (Integer) args[1];
                return Math.max(Math.abs(x - center.x()), Math.abs(z - center.z())) <= 1;
            }
            case "getHeight": {
                // LevelHeightAccessor.getHeight() 无参数；LevelReader.getHeight(type,pos)
                // 和 WorldGenRegion.getHeight(type,x,z) 是另外两个重载。
                if (arity == 0) {
                    return this.height;
                }
                if (arity == 3) {
                    Heightmap.Types type = (Heightmap.Types) args[0];
                    int x = (Integer) args[1];
                    int z = (Integer) args[2];
                    // WorldGenRegion.getHeight: ChunkAccess.getHeight(type, x&15, z&15) + 1
                    return chunkAt(x, z).chunk().getHeight(type, x & 15, z & 15) + 1;
                }
                if (arity == 2 && args[0] instanceof Heightmap.Types type
                        && args[1] instanceof BlockPos pos) {
                    return chunkAt(pos).chunk().getHeight(type, pos.getX() & 15, pos.getZ() & 15) + 1;
                }
                throw new UnsupportedOperationException("getHeight " + arity);
            }
            case "getMinY":
                return this.minY;
            case "getMaxY":
                return this.minY + this.height - 1;
            case "getMinSectionY":
                return SectionPos.blockToSectionCoord(this.minY);
            case "getMaxSectionY":
                return SectionPos.blockToSectionCoord(this.minY + this.height - 1);
            case "getSectionsCount":
                return SectionPos.blockToSectionCoord(this.minY + this.height - 1)
                        - SectionPos.blockToSectionCoord(this.minY) + 1;
            case "getSectionIndex":
                return SectionPos.blockToSectionCoord((Integer) args[0])
                        - SectionPos.blockToSectionCoord(this.minY);
            case "getSectionIndexFromSectionY":
                return (Integer) args[0] - SectionPos.blockToSectionCoord(this.minY);
            case "getSectionYFromSectionIndex":
                return (Integer) args[0] + SectionPos.blockToSectionCoord(this.minY);
            case "isOutsideBuildHeight": {
                int y = args[0] instanceof BlockPos pos ? pos.getY() : (Integer) args[0];
                return y < this.minY || y > this.minY + this.height - 1;
            }
            case "getSeaLevel":
                return this.seaLevel;
            case "scheduleTick": {
                // ScheduledTickAccess 的默认实现：createTick 然后 getBlockTicks().schedule。
                // 本轮只比方块，但树叶的 updateShape 会调它，不能抛异常。
                BlockPos pos = (BlockPos) args[0];
                Object target = args[1];
                int delay = (Integer) args[2];
                if (target instanceof net.minecraft.world.level.block.Block block) {
                    this.blockTicks.schedule(arity == 4
                            ? new net.minecraft.world.ticks.ScheduledTick<>(block, pos, delay,
                                    (net.minecraft.world.ticks.TickPriority) args[3],
                                    this.nextSubTick++)
                            : new net.minecraft.world.ticks.ScheduledTick<>(block, pos, delay,
                                    this.nextSubTick++));
                } else if (target instanceof net.minecraft.world.level.material.Fluid fluid) {
                    this.fluidTicks.schedule(arity == 4
                            ? new net.minecraft.world.ticks.ScheduledTick<>(fluid, pos, delay,
                                    (net.minecraft.world.ticks.TickPriority) args[3],
                                    this.nextSubTick++)
                            : new net.minecraft.world.ticks.ScheduledTick<>(fluid, pos, delay,
                                    this.nextSubTick++));
                } else {
                    throw new UnsupportedOperationException("scheduleTick target " + target);
                }
                return null;
            }
            case "getBlockTicks":
                return this.blockTicks;
            case "getFluidTicks":
                return this.fluidTicks;
            case "nextSubTickCount":
                // 本轮不比计划刻的时间，但返回单调值，避免 0 让某些判定走捷径。
                return this.nextSubTick++;
            case "getSeed":
                return this.seed;
            case "getRandom":
                return this.random;
            case "getBiome":
                // LevelReader.getBiome: BiomeManager.getBiome(pos)，不是 quart 格直读。
                return this.biomeManager.getBiome((BlockPos) args[0]);
            case "getNoiseBiome": {
                int qx = (Integer) args[0];
                int qy = (Integer) args[1];
                int qz = (Integer) args[2];
                ChunkAccess chunk = chunk(QuartPos.toSection(qx), QuartPos.toSection(qz)).chunk();
                return chunk.getNoiseBiome(qx, qy, qz);
            }
            case "getUncachedNoiseBiome":
                return this.biomeManager.getNoiseBiomeAtQuart((Integer) args[0], (Integer) args[1],
                        (Integer) args[2]);
            case "getBiomeManager":
                return this.biomeManager;
            case "registryAccess":
                return this.registries;
            case "getLevel":
                return proxy;
            case "isClientSide":
                return false;
            case "isStateAtPosition": {
                @SuppressWarnings("unchecked")
                java.util.function.Predicate<BlockState> predicate =
                        (java.util.function.Predicate<BlockState>) args[1];
                return predicate.test(getBlockState((BlockPos) args[0]));
            }
            case "isFluidAtPosition": {
                @SuppressWarnings("unchecked")
                java.util.function.Predicate<net.minecraft.world.level.material.FluidState> predicate =
                        (java.util.function.Predicate<net.minecraft.world.level.material.FluidState>) args[1];
                return predicate.test(getFluidState((BlockPos) args[0]));
            }
            case "getHeightmapPos": {
                // LevelReader.getHeightmapPos：x/z 不变，y 换成 getHeight(type,x,z)。
                Heightmap.Types type = (Heightmap.Types) args[0];
                BlockPos pos = (BlockPos) args[1];
                return new BlockPos(pos.getX(),
                        chunkAt(pos).chunk().getHeight(type, pos.getX() & 15, pos.getZ() & 15) + 1,
                        pos.getZ());
            }
            case "isEmptyBlock":
                return getBlockState((BlockPos) args[0]).isAir();
            case "getBlockEntity":
                return chunkAt((BlockPos) args[0]).chunk().getBlockEntity((BlockPos) args[0]);
            case "markPosForPostProcessing":
                chunkAt((BlockPos) args[0]).chunk().markPosForPostProcessing((BlockPos) args[0]);
                return null;
            case "getCenter":
                return this.center;
            case "toString":
                return "FeatureLevelHandler[" + center.x() + "," + center.z() + "]";
            case "hashCode":
                return System.identityHashCode(proxy);
            case "equals":
                return proxy == args[0];
            default:
                throw new UnsupportedOperationException(
                        "WorldGenLevel." + name + " is not implemented in the chunkgen oracle");
        }
    }

    private BlockState getBlockState(BlockPos pos) {
        return chunkAt(pos).chunk().getBlockState(pos);
    }

    private net.minecraft.world.level.material.FluidState getFluidState(BlockPos pos) {
        return chunkAt(pos).chunk().getFluidState(pos);
    }

    /** WorldGenRegion.setBlock（691）+ ensureCanWrite（635）。flags 不能丢。 */
    private boolean setBlock(BlockPos pos, BlockState state, int flags) {
        if (!withinWriteZone(pos)) {
            return false;
        }
        ChunkAccess chunk = chunkAt(pos).chunk();
        BlockState old = chunk.setBlockState(pos, state, flags);
        if (state.hasBlockEntity()) {
            // PROTOCHUNK 分支（44-101 / 104-167）：写一份 DUMMY NBT，
            // getBlockEntity 之后再补真正的方块实体。本轮只比方块，够用。
            CompoundTag tag = new CompoundTag();
            tag.putInt("x", pos.getX());
            tag.putInt("y", pos.getY());
            tag.putInt("z", pos.getZ());
            tag.putString("id", "DUMMY");
            chunk.setBlockEntityNbt(tag);
        } else if (old != null && old.hasBlockEntity()) {
            chunk.removeBlockEntity(pos);
        }
        if ((flags & 16) == 0) {
            BlockPos post = state.getPostProcessPos(chunk, pos);
            if (post != null) {
                chunkAt(post).chunk().markPosForPostProcessing(post);
            }
        }
        return true;
    }

    private boolean withinWriteZone(BlockPos pos) {
        return withinWriteZone(SectionPos.blockToSectionCoord(pos.getX()),
                SectionPos.blockToSectionCoord(pos.getZ()));
    }

    /** WorldGenRegion.isWithinWriteZone（612）：写半径 1，即 3×3。 */
    private boolean withinWriteZone(int cx, int cz) {
        return Math.abs(cx - center.x()) <= 1 && Math.abs(cz - center.z()) <= 1;
    }

    private TerrainChunkView chunkAt(BlockPos pos) {
        return chunkAt(pos.getX(), pos.getZ());
    }

    private TerrainChunkView chunkAt(int x, int z) {
        return chunk(SectionPos.blockToSectionCoord(x), SectionPos.blockToSectionCoord(z));
    }

    private TerrainChunkView chunk(int cx, int cz) {
        int dx = cx - center.x();
        int dz = cz - center.z();
        if (dx < -1 || dx > 1 || dz < -1 || dz > 1) {
            throw new IllegalStateException("read outside the 3x3 write zone: " + cx + "," + cz);
        }
        return this.chunks[(dz + 1) * 3 + dx + 1];
    }

    /** 代理里能拿到的 ProtoChunk 包装，避免和 Oracle 里的私有 record 绑死。 */
    interface TerrainChunkView {
        ProtoChunk chunk();
    }
}

/**
 * 区块级对拍：在 JVM 里用官方的 {@code ChunkGenerator}、{@code RandomState} 和
 * {@code ProtoChunk} 生成同一个区块到 NOISE、SURFACE、CARVERS 三个阶段，输出逐方块、
 * 逐 quart 格的结果。不启动服务端、不读存档、不接受 EULA。
 *
 * <p>官方 26.3 没有单独的 NOISE/SURFACE/CARVERS 状态：{@code terrain} 一步在同一个
 * {@code NoiseChunk} 上依次跑 {@code doFill}、{@code buildSurface}、{@code generateCarvers}。
 * 这里反射调这三个私有方法，参数与 {@code buildTerrain} 一致；{@code WorldGenRegion}
 * 传 null，让 {@code generateCarvers} 走 {@code getBiomeGenerationSettingsForCarver} 分支。
 *
 * <p>输出一行一个 JSON：header、每个区块一条 biomes、最多三条方块快照（noise/surface/
 * carvers），最后 end。方块按调色板压缩：palette 是方块状态的名字，blocks 是每个方块在
 * palette 里的下标，顺序是 y 从下到上、x 从 0 到 15、z 从 0 到 15。群系同理，顺序是
 * 段从下到上，段内 x、y、z 各 4 格。
 */
public final class ChunkgenOracle {
    private static final int OUTPUT_BUFFER = 4 << 20;
    /** 固定几个种子，覆盖 0、负数和大数；其余按 salt 随机。 */
    private static final long[] FIXED_SEEDS = {0L, -1L, 1L, 1234567890123456789L, -8765432109876543210L};
    /** 固定几个区块，覆盖原点、海洋、山地和深层洞穴区域；其余按 salt 随机。 */
    private static final int[][] FIXED_CHUNKS = {
        {0, 0}, {1, 0}, {0, 1}, {-1, -1}, {8, -13}, {100, 100}, {-200, 375}, {1234, -5678}
    };

    private ChunkgenOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
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
            new Oracle(registries, options, out).run();
        }
    }

    /** 和 WorldgenOracle 一样：按官方 WorldLoader 的前半段读数据包注册表。 */
    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(
            CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }

    /**
     * `through` 是 0..4，对应 biomes / noise / surface / carvers / features。
     * `--steps` 是对拍专用的过渡开关：只跑指定 step（例如 6 = UNDERGROUND_ORES），
     * 其它 step 不调 `placeWithBiomeCheck`。随机数按 `(index, step)` 重新播种，
     * 所以跳过别的 step 不影响留下的 step。最终验收仍要跑完整 11 步。
     */
    private record Options(int seeds, int chunks, long salt, int through, int stepsMask,
            String featureAllowList) {
        static final int ALL_STEPS = (1 << 11) - 1;

        static Options parse(String[] args) {
            int seeds = 3;
            int chunks = 12;
            long salt = 1;
            int through = 3;
            int stepsMask = ALL_STEPS;
            String featureAllowList = null;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--seeds" -> seeds = Integer.parseInt(args[++i]);
                    case "--chunks" -> chunks = Integer.parseInt(args[++i]);
                    case "--salt" -> salt = Long.parseLong(args[++i]);
                    case "--through" -> {
                        String value = args[++i];
                        through = switch (value) {
                            case "biomes" -> 0;
                            case "noise" -> 1;
                            case "surface" -> 2;
                            case "carvers" -> 3;
                            case "features" -> 4;
                            default -> throw new IllegalArgumentException("unknown stage " + value);
                        };
                    }
                    case "--steps" -> {
                        stepsMask = 0;
                        for (String part : args[++i].split(",")) {
                            int step = Integer.parseInt(part.trim());
                            if (step < 0 || step >= 11) {
                                throw new IllegalArgumentException("step out of range " + step);
                            }
                            stepsMask |= 1 << step;
                        }
                    }
                    case "--ore-only" -> featureAllowList = "minecraft:ore";
                    case "--biomes-only" -> through = 0;
                    default -> throw new IllegalArgumentException("unknown option " + args[i]);
                }
            }
            return new Options(seeds, chunks, salt, through, stepsMask, featureAllowList);
        }
    }

    private static final class Oracle {
        private final RegistryAccess registries;
        private final Registry<net.minecraft.world.level.levelgen.synth.NormalNoise> noises;
        private final Registry<NoiseGeneratorSettings> settings;
        private final HolderLookup.RegistryLookup<net.minecraft.world.level.levelgen.synth.NormalNoise> noiseLookup;
        private final Options options;
        private final Writer out;
        private final Method doFill;
        private final Method buildSurface;
        private final Method generateCarvers;
        private final Field globalFluidPicker;
        private final Set<Holder<Biome>> possibleBiomes;
        private long checks;

        Oracle(RegistryAccess registries, Options options, Writer out) throws Exception {
            this.registries = registries;
            this.noises = registries.lookupOrThrow(Registries.NOISE);
            this.settings = registries.lookupOrThrow(Registries.NOISE_SETTINGS);
            this.noiseLookup = registries.lookupOrThrow(Registries.NOISE);
            this.options = options;
            this.out = out;
            this.doFill = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                    "doFill", NoiseChunk.class, ChunkAccess.class);
            this.doFill.setAccessible(true);
            this.buildSurface = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                    "buildSurface", ChunkAccess.class, NoiseChunk.class, RandomState.class,
                    BiomeManager.class, Set.class, MaterialRule.class);
            this.buildSurface.setAccessible(true);
            this.generateCarvers = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                    "generateCarvers", ChunkAccess.class, Blender.class, NoiseChunk.class,
                    RandomState.class, BiomeManager.class,
                    net.minecraft.server.level.WorldGenRegion.class, MaterialRule.class);
            this.generateCarvers.setAccessible(true);
            this.globalFluidPicker = NoiseBasedChunkGenerator.class.getDeclaredField("globalFluidPicker");
            this.globalFluidPicker.setAccessible(true);
            // 正式流程用周围 3x3 区块的群系调色板；这里两边都用「全部群系」，只影响
            // BiomeCondition 的静态优化，逐格判定仍走 BiomeManager。
            this.possibleBiomes = new HashSet<>();
            for (Biome biome : registries.lookupOrThrow(Registries.BIOME)) {
                this.possibleBiomes.add(registries.lookupOrThrow(Registries.BIOME).wrapAsHolder(biome));
            }
        }

        void run() throws Exception {
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            header.addProperty("settings", "minecraft:overworld");
            header.addProperty("preset", "minecraft:overworld");
            header.addProperty("minY", -64);
            header.addProperty("height", 384);
            header.addProperty("seaLevel", 63);
            header.addProperty("through", this.options.through());
            header.addProperty("steps", this.options.stepsMask());
            write(header);

            NoiseGeneratorSettings generatorSettings = this.settings.getValue(Identifier.withDefaultNamespace("overworld"));
            if (generatorSettings == null) {
                throw new IllegalStateException("missing minecraft:overworld noise settings");
            }
            NoiseBasedChunkGenerator generator =
                    new NoiseBasedChunkGenerator(
                            MultiNoiseBiomeSource.createFromPreset(
                                    registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                            .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                            this.settings.wrapAsHolder(generatorSettings));
            applyStepsMask(generator);
            applyFeatureAllowList(generator);
            MaterialRule materialRule = generatorSettings.materialRule().value();
            for (long seed : seeds()) {
                RandomState state = RandomState.create(this.noiseLookup, seed, generatorSettings);
                // 和 createNoiseChunk 一样：新区块没有 BlendingData、没有结构，Beardifier 可以为空。
                java.util.function.Supplier<Aquifer.FluidPicker> picker = instancePicker(generator);
                // 和 WorldGenRegion 一样：uncached resolver + obfuscateSeed(worldSeed)。
                BiomeResolver resolver = ((BiomeSource) generator.getBiomeSource()).createUncachedResolver(state);
                BiomeManager biomeManager = new BiomeManager(resolver, BiomeManager.obfuscateSeed(seed));

                for (int[] chunk : chunks(seed)) {
                    ChunkPos pos = new ChunkPos(chunk[0], chunk[1]);
                    ProtoChunk proto = newChunk(pos, generatorSettings);
                    generator.createBiomes(state, Blender.empty(), null, proto).join();
                    // 真正的流程由 ChunkStatus 设置状态；这里补上，getNoiseBiome 才肯读。
                    proto.setPersistedStatus(ChunkStatus.BIOMES);
                    writeBiomes(seed, pos, proto);
                    if (this.options.through() < 1) {
                        continue;
                    }
                    if (this.options.through() >= 4) {
                        writeFeatures(seed, pos, generator, generatorSettings, state, biomeManager);
                        continue;
                    }

                    // 新区块没有 BlendingData，也没有结构：Beardifier.EMPTY 的填 0 分支。
                    DensityVolume volume = new DensityVolume(
                            16, generatorSettings.noiseSettings().height(), 16,
                            pos.getMinBlockX(), generatorSettings.noiseSettings().minY(), pos.getMinBlockZ());
                    try (NoiseChunk noiseChunk = new NoiseChunk(
                            state, Beardifier.EMPTY, generatorSettings, picker.get(), Blender.empty(), volume)) {
                        this.doFill.invoke(generator, noiseChunk, proto);
                        writeBlocks("noise", seed, pos, proto);
                        if (this.options.through() >= 2) {
                            this.buildSurface.invoke(generator, proto, noiseChunk, state, biomeManager,
                                    this.possibleBiomes, materialRule);
                            writeBlocks("surface", seed, pos, proto);
                        }
                        if (this.options.through() >= 3) {
                            this.generateCarvers.invoke(generator, proto, Blender.empty(), noiseChunk, state,
                                    biomeManager, null, materialRule);
                            writeBlocks("carvers", seed, pos, proto);
                        }
                    }
                }
            }

            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            end.addProperty("checks", this.checks);
            write(end);
            this.out.flush();
        }

        /**
         * `features` 阶段：造 3×3 个跑到 CARVERS 的 ProtoChunk，用 WorldGenLevel 代理
         * 让官方 `applyBiomeDecoration` 在一个不启动服务端的假世界里跑，输出 9 宫格。
         *
         * <p>代理只实现地物真正用到的方法，每个方法的语义都照 WorldGenRegion 的字节码
         * （见 docs/worldgen-features-plan.md 4.1 的对照表）；没实现的方法一律抛异常。
         */
        private void writeFeatures(long seed, ChunkPos pos, NoiseBasedChunkGenerator generator,
                NoiseGeneratorSettings generatorSettings, RandomState randomState,
                BiomeManager biomeManager) throws Exception {
            TerrainChunk[] chunks = new TerrainChunk[9];
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    chunks[(dz + 1) * 3 + dx + 1] = generateTerrain(
                            seed, new ChunkPos(pos.x() + dx, pos.z() + dz), generator,
                            generatorSettings, randomState, biomeManager);
                }
            }
            WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(
                    ChunkgenOracle.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class},
                    new FeatureLevelHandler(pos, seed, generatorSettings, wrapped(chunks),
                            this.registries, biomeManager, randomState));
            // generateStructures=false：applyBiomeDecoration 的结构分支整段跳过。
            StructureManager structures =
                    new StructureManager(level, new WorldOptions(seed, false, false), null);
            generator.applyBiomeDecoration(level, chunks[4].chunk(), structures);

            JsonArray results = new JsonArray();
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    int index = (dz + 1) * 3 + dx + 1;
                    JsonObject entry = new JsonObject();
                    entry.addProperty("dx", dx);
                    entry.addProperty("dz", dz);
                    addBlocks(entry, chunks[index].chunk());
                    results.add(entry);
                }
            }
            JsonObject line = new JsonObject();
            line.addProperty("type", "features");
            line.addProperty("seed", seed);
            line.addProperty("x", pos.x());
            line.addProperty("z", pos.z());
            line.add("chunks", results);
            // 过渡期：某个 step 没跑就明确标出来，最终验收必须 steps == 全部 11 步。
            line.addProperty("steps", this.options.stepsMask());
            write(line);
        }

        /** 一个区块跑到 CARVERS 的结果，加上 primed 高度图。 */
        private record TerrainChunk(ProtoChunk chunk) implements FeatureLevelHandler.TerrainChunkView {
            @Override
            public ProtoChunk chunk() {
                return chunk;
            }
        }

        private static FeatureLevelHandler.TerrainChunkView[] wrapped(TerrainChunk[] chunks) {
            FeatureLevelHandler.TerrainChunkView[] result =
                    new FeatureLevelHandler.TerrainChunkView[chunks.length];
            System.arraycopy(chunks, 0, result, 0, chunks.length);
            return result;
        }

        /** 现有单区块生成流程抽出来，features 阶段要用九次。 */
        private TerrainChunk generateTerrain(long seed, ChunkPos pos, NoiseBasedChunkGenerator generator,
                NoiseGeneratorSettings generatorSettings, RandomState state,
                BiomeManager biomeManager) throws Exception {
            ProtoChunk proto = newChunk(pos, generatorSettings);
            generator.createBiomes(state, Blender.empty(), null, proto).join();
            proto.setPersistedStatus(ChunkStatus.BIOMES);
            DensityVolume volume = new DensityVolume(
                    16, generatorSettings.noiseSettings().height(), 16,
                    pos.getMinBlockX(), generatorSettings.noiseSettings().minY(), pos.getMinBlockZ());
            java.util.function.Supplier<Aquifer.FluidPicker> picker = instancePicker(generator);
            try (NoiseChunk noiseChunk = new NoiseChunk(
                    state, Beardifier.EMPTY, generatorSettings, picker.get(), Blender.empty(), volume)) {
                this.doFill.invoke(generator, noiseChunk, proto);
                this.buildSurface.invoke(generator, proto, noiseChunk, state, biomeManager,
                        this.possibleBiomes, generatorSettings.materialRule().value());
                this.generateCarvers.invoke(generator, proto, Blender.empty(), noiseChunk, state,
                        biomeManager, null, generatorSettings.materialRule().value());
            }
            proto.setPersistedStatus(ChunkStatus.TERRAIN);
            Heightmap.primeHeightmaps(proto, ChunkStatus.FINAL_HEIGHTMAPS);
            return new TerrainChunk(proto);
        }

        /**
         * 过渡开关：把没选中的 step 在每个群系的 `BiomeGenerationSettings.features()`
         * 里换成空 `HolderSet`，这样 `applyBiomeDecoration` 收集到的 `indices` 为空，
         * 自然不会调 `placeWithBiomeCheck`；`featuresPerStep` 和全局 index 保持原样，
         * 留下的 step 的 `setFeatureSeed` 序列和官方完全一致。
         *
         * <p>群系对象是注册表里的共享实例，只在这个 oracle 进程里改，不影响导出数据。
         */
        private void applyStepsMask(NoiseBasedChunkGenerator generator) throws Exception {
            if (this.options.stepsMask() == Options.ALL_STEPS) {
                return;
            }
            Field features = net.minecraft.world.level.biome.BiomeGenerationSettings.class
                    .getDeclaredField("features");
            features.setAccessible(true);
            Registry<Biome> biomes = this.registries.lookupOrThrow(Registries.BIOME);
            for (Biome biome : biomes) {
                net.minecraft.world.level.biome.BiomeGenerationSettings settings =
                        biome.getGenerationSettings();
                List<net.minecraft.core.HolderSet<
                        net.minecraft.world.level.levelgen.placement.PlacedFeature>> steps =
                        new ArrayList<>(settings.features());
                for (int step = 0; step < steps.size(); step++) {
                    if ((this.options.stepsMask() & (1 << step)) == 0) {
                        steps.set(step, net.minecraft.core.HolderSet.empty());
                    }
                }
                features.set(settings, List.copyOf(steps));
                // featureSet 是 memoized supplier，缓存的是旧的 features 列表；换掉它，
                // 否则 BiomeFilter 会按未过滤的列表判 hasFeature。
                Field featureSet = net.minecraft.world.level.biome.BiomeGenerationSettings.class
                        .getDeclaredField("featureSet");
                featureSet.setAccessible(true);
                featureSet.set(settings, com.google.common.base.Suppliers.memoize(
                        () -> List.copyOf(steps).stream()
                                .flatMap(net.minecraft.core.HolderSet::stream)
                                .map(net.minecraft.core.Holder::value)
                                .collect(java.util.stream.Collectors.toSet())));
            }
        }

        /**
         * 过渡开关：只保留配置类型在白名单里的 placed feature。做法和 `--steps` 一样，
         * 把不保留的 HolderSet 换成空集合，全局 index 和 `setFeatureSeed` 参数不变。
         */
        private void applyFeatureAllowList(NoiseBasedChunkGenerator generator) throws Exception {
            String allowed = this.options.featureAllowList();
            if (allowed == null) {
                return;
            }
            Registry<net.minecraft.world.level.levelgen.placement.PlacedFeature> placed =
                    this.registries.lookupOrThrow(Registries.PLACED_FEATURE);
            Set<String> keep = new HashSet<>();
            for (java.util.Map.Entry<net.minecraft.resources.ResourceKey<
                    net.minecraft.world.level.levelgen.placement.PlacedFeature>,
                    net.minecraft.world.level.levelgen.placement.PlacedFeature> entry
                    : placed.entrySet()) {
                net.minecraft.world.level.levelgen.placement.PlacedFeature value = entry.getValue();
                // 用 codec 的类型 ID 比，不用注册名前缀猜。
                if (allowed.equals(featureTypeId(value.feature().value()))) {
                    keep.add(entry.getKey().identifier().toString());
                }
            }
            if (keep.isEmpty()) {
                throw new IllegalStateException("feature allow list kept nothing: " + allowed);
            }
            Field features = net.minecraft.world.level.biome.BiomeGenerationSettings.class
                    .getDeclaredField("features");
            features.setAccessible(true);
            Registry<Biome> biomes = this.registries.lookupOrThrow(Registries.BIOME);
            for (Biome biome : biomes) {
                net.minecraft.world.level.biome.BiomeGenerationSettings settings =
                        biome.getGenerationSettings();
                List<net.minecraft.core.HolderSet<
                        net.minecraft.world.level.levelgen.placement.PlacedFeature>> steps =
                        new ArrayList<>(settings.features());
                for (int step = 0; step < steps.size(); step++) {
                    List<net.minecraft.core.Holder<
                            net.minecraft.world.level.levelgen.placement.PlacedFeature>> kept =
                            new ArrayList<>();
                    for (net.minecraft.core.Holder<
                            net.minecraft.world.level.levelgen.placement.PlacedFeature> holder
                            : steps.get(step)) {
                        String name = holder.unwrapKey().map(k -> k.identifier().toString()).orElse("");
                        if (keep.contains(name)) {
                            kept.add(holder);
                        }
                    }
                    steps.set(step, net.minecraft.core.HolderSet.direct(kept));
                }
                features.set(settings, List.copyOf(steps));
                Field featureSet = net.minecraft.world.level.biome.BiomeGenerationSettings.class
                        .getDeclaredField("featureSet");
                featureSet.setAccessible(true);
                featureSet.set(settings, com.google.common.base.Suppliers.memoize(
                        () -> List.copyOf(steps).stream()
                                .flatMap(net.minecraft.core.HolderSet::stream)
                                .map(net.minecraft.core.Holder::value)
                                .collect(java.util.stream.Collectors.toSet())));
            }
            System.err.println("feature allow list kept " + keep.size() + " placed features");
        }

        /** 官方 `Feature.DIRECT_CODEC` 的 dispatch 类型 ID。 */
        private String featureTypeId(net.minecraft.world.level.levelgen.feature.Feature feature) {
            return net.minecraft.core.registries.BuiltInRegistries.FEATURE_TYPE
                    .getKey(feature.codec()).toString();
        }

        /** globalFluidPicker 是实例字段；buildTerrain 用的就是当前 generator 的那个。 */
        private java.util.function.Supplier<Aquifer.FluidPicker> instancePicker(
                NoiseBasedChunkGenerator generator) throws Exception {
            @SuppressWarnings("unchecked")
            java.util.function.Supplier<Aquifer.FluidPicker> picker =
                    (java.util.function.Supplier<Aquifer.FluidPicker>) this.globalFluidPicker.get(generator);
            return picker;
        }

        private ProtoChunk newChunk(ChunkPos pos, NoiseGeneratorSettings generatorSettings) {
            LevelHeightAccessor height = LevelHeightAccessor.create(
                    generatorSettings.noiseSettings().minY(), generatorSettings.noiseSettings().height());
            return new ProtoChunk(
                    pos, UpgradeData.EMPTY, height, PalettedContainerFactory.create(this.registries), null);
        }

        private void writeBiomes(long seed, ChunkPos pos, ChunkAccess chunk) throws Exception {
            JsonArray sections = new JsonArray();
            int minQuart = QuartPos.fromBlock(chunk.getMinY());
            int count = chunk.getHeight() / 16;
            for (int section = 0; section < count; section++) {
                JsonArray cells = new JsonArray();
                int quartY = minQuart + section * 4;
                for (int x = 0; x < 4; x++) {
                    for (int y = 0; y < 4; y++) {
                        for (int z = 0; z < 4; z++) {
                            Holder<Biome> biome = chunk.getNoiseBiome(x, quartY + y, z);
                            cells.add(biome.unwrapKey().orElseThrow().identifier().toString());
                            this.checks++;
                        }
                    }
                }
                sections.add(cells);
            }
            JsonObject line = new JsonObject();
            line.addProperty("type", "biomes");
            line.addProperty("seed", seed);
            line.addProperty("x", pos.getMinBlockX() >> 4);
            line.addProperty("z", pos.getMinBlockZ() >> 4);
            line.add("sections", sections);
            write(line);
        }

        private void writeBlocks(String stage, long seed, ChunkPos pos, ChunkAccess chunk) throws Exception {
            JsonObject line = new JsonObject();
            line.addProperty("type", stage);
            line.addProperty("seed", seed);
            line.addProperty("x", pos.getMinBlockX() >> 4);
            line.addProperty("z", pos.getMinBlockZ() >> 4);
            addBlocks(line, chunk);
            write(line);
        }

        /** palette + blocks，顺序 y、x、z；features 的 9 宫格也用这个格式。 */
        private void addBlocks(JsonObject line, ChunkAccess chunk) {
            List<BlockState> palette = new ArrayList<>();
            java.util.Map<BlockState, Integer> ids = new java.util.HashMap<>();
            JsonArray blocks = new JsonArray();
            int minY = chunk.getMinY();
            for (int y = 0; y < chunk.getHeight(); y++) {
                for (int x = 0; x < 16; x++) {
                    for (int z = 0; z < 16; z++) {
                        BlockState state = chunk.getSection(chunk.getSectionIndex(minY + y))
                                .getBlockState(x, y & 15, z);
                        Integer id = ids.get(state);
                        if (id == null) {
                            id = palette.size();
                            palette.add(state);
                            ids.put(state, id);
                        }
                        blocks.add(id);
                        this.checks++;
                    }
                }
            }
            JsonArray names = new JsonArray();
            for (BlockState state : palette) {
                names.add(state.toString());
            }
            line.add("palette", names);
            line.add("blocks", blocks);
        }

        private long[] seeds() {
            long[] seeds = new long[this.options.seeds()];
            Random random = new Random(this.options.salt());
            for (int i = 0; i < seeds.length; i++) {
                seeds[i] = i < FIXED_SEEDS.length ? FIXED_SEEDS[i] : random.nextLong();
            }
            return seeds;
        }

        private int[][] chunks(long seed) {
            int count = this.options.chunks();
            int[][] result = new int[count][2];
            Random random = new Random(seed ^ 0x51A7_5EEDL ^ this.options.salt());
            for (int i = 0; i < count; i++) {
                if (i < FIXED_CHUNKS.length) {
                    result[i] = FIXED_CHUNKS[i].clone();
                } else {
                    int bound = switch (i % 3) {
                        case 0 -> 64;
                        case 1 -> 4096;
                        default -> 1_000_000;
                    };
                    result[i][0] = random.nextInt(-bound, bound);
                    result[i][1] = random.nextInt(-bound, bound);
                }
            }
            return result;
        }

        private void write(JsonObject line) throws Exception {
            this.out.write(line.toString());
            this.out.write('\n');
        }
    }
}
