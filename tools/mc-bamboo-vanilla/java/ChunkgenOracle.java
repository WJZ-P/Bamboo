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
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
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
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
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
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureSet;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.core.registries.BuiltInRegistries;
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
    /** `WorldGenRegion.uncachedBiomeResolver`：3×3 之外回退到单点气候重算。 */
    private final BiomeResolver uncachedBiomeResolver;
    private final net.minecraft.util.RandomSource random;
    private final net.minecraft.world.ticks.WorldGenTickAccess<net.minecraft.world.level.block.Block> blockTicks;
    private final net.minecraft.world.ticks.WorldGenTickAccess<net.minecraft.world.level.material.Fluid> fluidTicks;
    /** 带结构模式下，`startsForStructure` 要按 17×17 网格取起点/引用。 */
    private final StructureChunkGrid structureGrid;
    /** 生成期实体要的 ServerLevel 替身；无结构模式为 null。 */
    private final net.minecraft.server.level.ServerLevel serverLevel;
    private Supplier<String> currentlyGenerating;
    private long nextSubTick;
    /** 邻居溢出合并对拍：数这个中心有多少次 setBlock 落进目标区块。null 表示不数。 */
    ChunkPos countWritesInto;
    long countedWrites;
    /** 同上，记下落进目标区块的 setBlock 位置（`BlockPos.asLong`）。 */
    final java.util.Set<Long> touchedInTarget = new HashSet<>();


    FeatureLevelHandler(ChunkPos center, long seed, NoiseGeneratorSettings settings,
            TerrainChunkView[] chunks, RegistryAccess registries, BiomeManager biomeManager,
            BiomeResolver uncachedBiomeResolver, RandomState randomState,
            StructureChunkGrid structureGrid,
            net.minecraft.server.level.ServerLevel serverLevel) {
        this.center = center;
        this.seed = seed;
        this.minY = settings.noiseSettings().minY();
        this.height = settings.noiseSettings().height();
        this.seaLevel = settings.seaLevel();
        this.chunks = chunks;
        this.registries = registries;
        this.biomeManager = biomeManager;
        this.uncachedBiomeResolver = uncachedBiomeResolver;
        this.structureGrid = structureGrid;
        this.serverLevel = serverLevel;
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
            case "isWaterAt": {
                // LevelReader.isWaterAt 的默认实现是 `getFluidState(pos).is(WATER)`；
                // 代理必须显式转发，否则孢子花之类的 canSurvive 会直接抛异常。
                // LevelReader.isWaterAt defaults to `getFluidState(pos).is(WATER)`;
                // the proxy has to forward it or canSurvive checks such as the spore
                // blossom's throw.
                BlockPos pos = (BlockPos) args[0];
                return chunkAt(pos).chunk().getFluidState(pos)
                        .is(net.minecraft.tags.FluidTags.WATER);
            }
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
                // `LevelReader.getChunk(x,z,status)` 的 4 参数重载：结构管理器用它
                // 取 STRUCTURE_REFERENCES / STRUCTURE_STARTS 区块，范围是整个半径 8
                // 邻域，不是地物的 3×3 写区。没开结构时保持旧的严格 3×3 行为。
                // LevelReader's 4-arg getChunk: StructureManager resolves references
                // and starts over the radius-8 neighbourhood, not the 3x3 write zone.
                if (arity >= 3 && this.structureGrid != null) {
                    return this.structureGrid.chunk(center, x, z);
                }
                return chunk(x, z).chunk();
            }
            case "hasChunk":
            case "hasChunkAt": {
                if (arity >= 1 && args[0] instanceof BlockPos pos) {
                    return withinWriteZone(pos);
                }
                int x = (Integer) args[0];
                int z = (Integer) args[1];
                if (this.structureGrid != null && arity >= 3) {
                    return this.structureGrid.has(center, x, z);
                }
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
            case "isInsideBuildHeight": {
                // `LevelHeightAccessor.isInsideBuildHeight` 就是 `!isOutsideBuildHeight`。
                // `LevelHeightAccessor.isInsideBuildHeight` is `!isOutsideBuildHeight`.
                int y = args[0] instanceof BlockPos pos ? pos.getY() : (Integer) args[0];
                return y >= this.minY && y <= this.minY + this.height - 1;
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
                // 诊断开关：把区域随机源包一层，记录 `nextBoolean` 的取值。
                // `level.getRandom()` 只被 `MossyCarpetBlock.placeAt` 一类调用读，
                // 打印它的序列就能和 Rust 侧逐次对比。
                // Diagnostic switch: wrap the region random source to log `nextBoolean`.
                // `level.getRandom()` is read by calls such as `MossyCarpetBlock.placeAt`,
                // so the sequence can be compared with the Rust side draw by draw.
                if (Boolean.getBoolean("bamboo.debug.region-random")) {
                    return new RandomSource() {
                        @Override public RandomSource fork() { return random.fork(); }
                        @Override public PositionalRandomFactory forkPositional() {
                            return random.forkPositional();
                        }
                        @Override public void setSeed(long seed) { random.setSeed(seed); }
                        @Override public int nextInt() { return random.nextInt(); }
                        @Override public int nextInt(int bound) { return random.nextInt(bound); }
                        @Override public long nextLong() { return random.nextLong(); }
                        @Override public boolean nextBoolean() {
                            boolean value = random.nextBoolean();
                            try (java.io.FileWriter writer = new java.io.FileWriter(
                                    System.getProperty("bamboo.debug.region-random.file",
                                            "region-random.log"), true)) {
                                writer.write((value ? "true" : "false") + "\n");
                            } catch (java.io.IOException ignored) {
                                // 诊断用，失败就当没开。
                                // Diagnostic only; ignore IO failures.
                            }
                            return value;
                        }
                        @Override public float nextFloat() { return random.nextFloat(); }
                        @Override public double nextDouble() { return random.nextDouble(); }
                        @Override public double nextGaussian() { return random.nextGaussian(); }
                    };
                }
                return this.random;
            case "getBiome":
                // LevelReader.getBiome: BiomeManager.getBiome(pos)。这个 BiomeManager 的
                // BiomeResolver 必须是「本代理」而不是 uncached resolver：官方
                // WorldGenRegion 构造时传的是 region 自己（字节码 149-165），于是
                // BiomeFilter 读的是 3×3 区块里 BIOMES 阶段填好的 quart 格。
                // LevelReader.getBiome: BiomeManager.getBiome(pos). The manager's
                // BiomeResolver must be this proxy (not the uncached resolver):
                // vanilla's WorldGenRegion constructor passes the region itself
                // (bytecode 149-165), so BiomeFilter reads the stored quart cells.
                return this.biomeManager.getBiome((BlockPos) args[0]);
            case "getNoiseBiome": {
                // LevelReader.getNoiseBiome 默认实现：先找 3×3 里的区块读 quart 格，
                // 读不到才回退 getUncachedNoiseBiome()。BiomeManager.getBiome 的
                // 八角插值选中的 quart 角会走到这里。
                // LevelReader.getNoiseBiome: find the chunk in the 3x3 and read its
                // quart cell, falling back to getUncachedNoiseBiome(). This is the
                // path BiomeManager.getBiome's octant interpolation ends up on.
                int qx = (Integer) args[0];
                int qy = (Integer) args[1];
                int qz = (Integer) args[2];
                int cx = QuartPos.toSection(qx);
                int cz = QuartPos.toSection(qz);
                if (Math.abs(cx - center.x()) <= 1 && Math.abs(cz - center.z()) <= 1) {
                    return chunk(cx, cz).chunk().getNoiseBiome(qx, qy, qz);
                }
                return uncachedBiomeResolver.getNoiseBiome(qx, qy, qz);
            }
            case "getUncachedNoiseBiome":
                return uncachedBiomeResolver.getNoiseBiome((Integer) args[0], (Integer) args[1],
                        (Integer) args[2]);
            case "getBiomeManager":
                return this.biomeManager;
            case "registryAccess":
                return this.registries;
            case "holderLookup":
                // `JigsawReplacementProcessor.processBlock` 用 BLOCK 查找解析
                // 拼图方块的 `final_state`。
                return this.registries.lookupOrThrow(
                        (net.minecraft.resources.ResourceKey) args[0]);
            case "getLevel":
                // `WorldGenRegion.getLevel()` 返回 ServerLevel；生成期实体要用它。
                return this.serverLevel;
            case "addFreshEntity":
                return Boolean.TRUE;
            case "addFreshEntityWithPassengers":
            case "getCurrentDifficultyAt":
                return null;
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
            case "getBrightness", "getRawBrightness":
                // 假世界没有光照引擎，地物阶段官方世界里方块光也只有 0，天空光是 15
                // （还没跑 light 阶段，整片区域都当露天）。
                // The fake world has no light engine: block light is 0 during the feature
                // stage and sky light is 15 (light has not run, everything is open sky).
                if (args[0] instanceof net.minecraft.world.level.LightLayer layer) {
                    return layer == net.minecraft.world.level.LightLayer.SKY ? 15 : 0;
                }
                return 15;
            case "canSeeSky":
                return true;
            case "getLightEngine":
                throw new UnsupportedOperationException("getLightEngine is not available here");
            case "getBlockEntity": {
                // WorldGenRegion.getBlockEntity：区块里还没有方块实体、但 setBlock 留了
                // DUMMY NBT 时，现造一个放回区块。蜂巢装饰器靠它决定取不取随机数。
                // WorldGenRegion.getBlockEntity: with no block entity yet but a DUMMY NBT
                // left by setBlock, create one and store it. The beehive decorator's
                // random draws depend on it.
                BlockPos pos = (BlockPos) args[0];
                // 2 参数重载 `getBlockEntity(pos, type)` 返回 `Optional<T>`
                // （`DesertPyramidStructure.placeSuspiciousSand` 用它设置战利品表）。
                boolean typed = arity >= 2;
                ChunkAccess chunk = chunkAt(pos).chunk();
                net.minecraft.world.level.block.entity.BlockEntity found = chunk.getBlockEntity(pos);
                if (found == null) {
                    CompoundTag tag = chunk.getBlockEntityNbt(pos);
                    BlockState state = chunk.getBlockState(pos);
                    if (tag != null && "DUMMY".equals(tag.getStringOr("id", ""))
                            && state.hasBlockEntity()) {
                        found = ((net.minecraft.world.level.block.EntityBlock) state.getBlock())
                                .newBlockEntity(pos, state);
                        if (found != null) {
                            chunk.setBlockEntity(found);
                        }
                    }
                }
                if (typed) {
                    if (found == null || !((net.minecraft.world.level.block.entity.BlockEntityType<?>)
                            args[1]).isValid(chunk.getBlockState(pos))) {
                        return java.util.Optional.empty();
                    }
                    return java.util.Optional.of(found);
                }
                return found;
            }
            // 假世界没有客户端，声音和等级事件都只有表现作用，对拍只看方块；
            // 但 `SculkSpreader` / `SculkBlock` 会无条件调它们，必须吞掉。
            // The fake world has no client, so sounds and level events only matter for
            // rendering; `SculkSpreader` / `SculkBlock` call them unconditionally.
            case "playSound":
                return null;
            case "levelEvent":
                return null;
            case "gameEvent":
                return null;
            // `LevelReader.findBlocksIn`：`SculkBlock.canPlaceGrowth` 用它数
            // `sculk_growth_inhibitors`。假世界是直接读 3×3 区块，用简单的扫描实现。
            // `LevelReader.findBlocksIn`: `SculkBlock.canPlaceGrowth` counts
            // `sculk_growth_inhibitors` with it; the fake world reads the 3x3 directly.
            case "findBlocksIn": {
                BlockPos from = (BlockPos) args[0];
                BlockPos to = (BlockPos) args[1];
                return ChunkgenBlockMatcher.of(this, from, to);
            }
            case "markPosForPostProcessing":
                chunkAt((BlockPos) args[0]).chunk().markPosForPostProcessing((BlockPos) args[0]);
                return null;
            case "updateNeighborsAt":
                // `LevelAccessor` 的默认实现是空操作，`WorldGenRegion` 没有覆写。
                return null;
            case "getEntities":
                // `WorldGenRegion.getEntities` 两个重载都返回 `Collections.emptyList()`；
                // `SculkVeinBlock.attemptPlaceSculk` → `Block.pushEntitiesUp` 会调它。
                // Both `WorldGenRegion.getEntities` overloads return an empty list;
                // `Block.pushEntitiesUp` (from sculk vein spreading) calls it.
                return java.util.Collections.emptyList();
            case "environmentAttributes":
                // 放置路径只有流体刻延迟读它，不影响写方块。
                return net.minecraft.world.attribute.EnvironmentAttributeReader.EMPTY;
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

    /** `getBlockState` 给同包的 `ChunkgenBlockMatcher` 用。 */
    BlockState getBlockStatePublic(BlockPos pos) {
        return getBlockState(pos);
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
        if (this.countWritesInto != null
                && SectionPos.blockToSectionCoord(pos.getX()) == this.countWritesInto.x()
                && SectionPos.blockToSectionCoord(pos.getZ()) == this.countWritesInto.z()) {
            this.countedWrites++;
            this.touchedInTarget.add(pos.asLong());
        }
        traceWrite(pos, state);
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

    /**
     * 诊断开关 `-Dbamboo.debug.trace-pos=x,y,z;x,y,z`：记录写到这些位置的每一次
     * `setBlock`（新状态 + 调用栈），写到 `bamboo.debug.trace-pos.file`。
     * Diagnostic switch: log every write to the listed positions (new state and
     * call stack) to `bamboo.debug.trace-pos.file`.
     */
    private static final java.util.Set<Long> TRACE_POS = parseTracePos();

    private static java.util.Set<Long> parseTracePos() {
        java.util.Set<Long> out = new java.util.HashSet<>();
        String spec = System.getProperty("bamboo.debug.trace-pos", "");
        for (String part : spec.split(";")) {
            String[] xyz = part.trim().split(",");
            if (xyz.length == 3) {
                out.add(BlockPos.asLong(Integer.parseInt(xyz[0].trim()),
                        Integer.parseInt(xyz[1].trim()), Integer.parseInt(xyz[2].trim())));
            }
        }
        return out;
    }

    private static void traceWrite(BlockPos pos, BlockState state) {
        if (TRACE_POS.isEmpty() || !TRACE_POS.contains(pos.asLong())) {
            return;
        }
        StringBuilder text = new StringBuilder();
        text.append("WRITE ").append(pos.toShortString()).append(' ').append(state).append('\n');
        for (StackTraceElement frame : new Throwable().getStackTrace()) {
            text.append("    at ").append(frame).append('\n');
        }
        try (java.io.FileWriter writer = new java.io.FileWriter(
                System.getProperty("bamboo.debug.trace-pos.file", "trace-pos.log"), true)) {
            writer.write(text.toString());
        } catch (java.io.IOException ignored) {
            // 诊断用，失败就当没开。
            // Diagnostic only; ignore IO failures.
        }
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
 * 带结构模式下的半径 9 区块网格：`StructureManager.startsForStructure` 需要按
 * `STRUCTURE_REFERENCES` / `STRUCTURE_STARTS` 读取中心 ±8 的起点，而 3×3 的
 * terrain 中心各自要引用中心 ±9 范围里的起点，所以整张网格是 19×19。
 *
 * <p>网格里的 ProtoChunk 与 3×3 写区的 9 个是**同一批对象**：地形先跑到
 * CARVERS，引用 / 起点是这些 ProtoChunk 上的字段，`applyBiomeDecoration` 通过
 * 4 参数 `getChunk` 读到的就是它们。
 */
final class StructureChunkGrid {
    static final int RADIUS = 9;
    private final ChunkPos origin;
    private final ProtoChunk[][] chunks;
    /** 网格半径；邻居溢出合并对拍要 5×5 各自的半径 8，用 10。 */
    private final int radius;

    StructureChunkGrid(ChunkPos origin, ProtoChunk[][] chunks) {
        this(origin, chunks, RADIUS);
    }

    StructureChunkGrid(ChunkPos origin, ProtoChunk[][] chunks, int radius) {
        this.origin = origin;
        this.chunks = chunks;
        this.radius = radius;
    }

    boolean has(ChunkPos center, int x, int z) {
        int dx = x - origin.x();
        int dz = z - origin.z();
        return dx >= -radius && dx <= radius && dz >= -radius && dz <= radius;
    }

    ProtoChunk chunk(ChunkPos center, int x, int z) {
        int dx = x - origin.x();
        int dz = z - origin.z();
        if (dx < -radius || dx > radius || dz < -radius || dz > radius) {
            throw new IllegalStateException("structure getChunk outside the grid: " + x + "," + z);
        }
        return chunks[dx + radius][dz + radius];
    }

    static int size() {
        return RADIUS * 2 + 1;
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
            // `Block.newBlockEntity` 读组件默认值；组件只在服务器资源重载时绑定，
            // 带结构模式要构造方块实体（试炼密室的宝库等），这里手动绑一次。
            // Bind data-component defaults once: structure placement builds block
            // entities (e.g. trial-chamber vaults) whose constructors read them.
            if (options.structures()) {
                BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(registries)
                        .forEach(net.minecraft.core.component.DataComponentInitializers
                                .PendingComponents::apply);
            }
            new Oracle(registries, resources, options, out).run();
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
            String featureAllowList, boolean realPath, String settings,
            Integer forcedX, Integer forcedZ, int scan, boolean structures, Long forcedSeed,
            Path chunksFile, String mergeOrders) {
        static final int ALL_STEPS = (1 << 11) - 1;

        static Options parse(String[] args) {
            int seeds = 3;
            int chunks = 12;
            long salt = 1;
            int through = 3;
            int stepsMask = ALL_STEPS;
            // 逗号分隔的 Feature 类型 ID 白名单；null 表示不过滤。`--ore-only` 等价于
            // `--feature-types minecraft:ore`，保留它是为了兼容旧的对拍脚本。
            // Comma-separated Feature type-ID allow list; null means no filter.
            // `--ore-only` is kept as an alias for `--feature-types minecraft:ore`.
            String featureAllowList = null;
            // 真实服务端路径：3×3 possibleBiomes、carverBiome 分支、region.getBiomeManager()。
            // Real-server path: 3x3 possibleBiomes, the carverBiome branch, region.getBiomeManager().
            boolean realPath = false;
            String settings = "overworld";
            // `--chunk X Z` 只出题这一个区块（精确复现）；`--scan N` 每个种子出 N 个
            // 覆盖近处/远处/负坐标的随机区块；两者合用则出以 (X,Z) 为中心的连续网格。
            // 都只改坐标来源。
            // `--chunk X Z` emits only this one chunk (exact reproduction); `--scan N` emits
            // N random chunks per seed spanning near/far/negative coordinates; combined they
            // emit a contiguous grid centered on (X,Z). All only change the coordinate source.
            Integer forcedX = null;
            Integer forcedZ = null;
            int scan = 0;
            // `--structures`：跑官方 createStructures / createReferences、真实
            // Beardifier 的 CARVERS、带结构的 applyBiomeDecoration。
            boolean structures = false;
            Long forcedSeed = null;
            Path chunksFile = null;
            // `--merge-neighbors ORDERS`：邻居溢出合并对拍，逗号分隔的中心先后
            // （center-first / center-last），每种先后各输出一条 merged 线。
            // `--merge-neighbors ORDERS`: neighbor-spill merge oracle, one merged
            // line per comma-separated run order (center-first / center-last).
            String mergeOrders = null;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--seeds" -> seeds = Integer.parseInt(args[++i]);
                    case "--chunks" -> chunks = Integer.parseInt(args[++i]);
                    case "--salt" -> salt = Long.parseLong(args[++i]);
                    case "--settings" -> settings = args[++i];
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
                    // `--types` 是早期诊断开关的名字，保留成别名。
                    // `--types` is the older name for the same diagnostic switch; keep it as an alias.
                    case "--types", "--feature-types" -> featureAllowList = args[++i];
                    case "--biomes-only" -> through = 0;
                    case "--real-path" -> realPath = true;
                    case "--chunk" -> {
                        forcedX = Integer.parseInt(args[++i]);
                        forcedZ = Integer.parseInt(args[++i]);
                    }
                    case "--scan" -> scan = Integer.parseInt(args[++i]);
                    case "--structures" -> structures = true;
                    case "--seed" -> forcedSeed = Long.parseLong(args[++i]);
                    case "--chunks-file" -> chunksFile = Path.of(args[++i]);
                    case "--merge-neighbors" -> mergeOrders = args[++i];
                    default -> throw new IllegalArgumentException("unknown option " + args[i]);
                }
            }
            return new Options(seeds, chunks, salt, through, stepsMask, featureAllowList, realPath, settings,
                    forcedX, forcedZ, scan, structures, forcedSeed, chunksFile, mergeOrders);
        }
    }

    private static final class Oracle {
        private final RegistryAccess registries;
        /** 带结构模式要按官方方式加载 `.nbt` 模板（不启动服务端、不读存档）。 */
        private final CloseableResourceManager resources;
        private final StructureTemplateManager templates;
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

        Oracle(RegistryAccess registries, CloseableResourceManager resources,
                Options options, Writer out) throws Exception {
            this.registries = registries;
            this.resources = resources;
            this.templates = templateManager(resources);
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

        private static StructureTemplateManager templateManager(CloseableResourceManager resources)
                throws Exception {
            Path temp = Files.createTempDirectory("bamboo-genworld-structures");
            LevelStorageSource.LevelStorageAccess storage =
                    LevelStorageSource.createDefault(temp).createAccess("struct");
            return new StructureTemplateManager(
                    resources, storage, DataFixers.getDataFixer(), BuiltInRegistries.BLOCK);
        }

        void run() throws Exception {
            // --settings 在 normal / large_biomes / amplified 之间切 NoiseGeneratorSettings，
            // 三种世界共用 minecraft:overworld 群系预设（WorldPresets.bootstrap）。
            // --settings switches NoiseGeneratorSettings between normal / large_biomes /
            // amplified; all three share the minecraft:overworld biome preset (WorldPresets.bootstrap).
            String settingsId = "minecraft:" + this.options.settings();
            NoiseGeneratorSettings generatorSettings =
                    this.settings.getValue(Identifier.withDefaultNamespace(this.options.settings()));
            if (generatorSettings == null) {
                throw new IllegalStateException("missing " + settingsId + " noise settings");
            }
            // `nether` 换群系预设；其余（overworld 的 normal / large_biomes / amplified）
            // 共用 minecraft:overworld 预设（WorldPresets.bootstrap）。
            // `nether` switches the biome preset; the overworld variants all share
            // minecraft:overworld.
            boolean nether = "minecraft:nether".equals(settingsId);
            String preset = nether ? "minecraft:nether" : "minecraft:overworld";
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            header.addProperty("settings", settingsId);
            header.addProperty("preset", preset);
            header.addProperty("minY", generatorSettings.noiseSettings().minY());
            header.addProperty("height", generatorSettings.noiseSettings().height());
            header.addProperty("seaLevel", generatorSettings.seaLevel());
            header.addProperty("through", this.options.through());
            header.addProperty("steps", this.options.stepsMask());
            header.addProperty("structures", this.options.structures());
            write(header);

            NoiseBasedChunkGenerator generator =
                    new NoiseBasedChunkGenerator(
                            MultiNoiseBiomeSource.createFromPreset(
                                    registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                            .getOrThrow(nether
                                                    ? MultiNoiseBiomeSourceParameterLists.NETHER
                                                    : MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                            this.settings.wrapAsHolder(generatorSettings));
            // featuresPerStep 是 memoized supplier，第一次 get() 会按注册表此刻的
            // BiomeGenerationSettings.features() 排全局顺序。必须在两个诊断过滤之前
            // 固定它，否则 --steps / --ore-only 会让全局 index 变成过滤后的下标，
            // setFeatureSeed 的参数就和导出的 features_per_step 不一致了。
            generator.validate();
            checkFeaturesPerStep(generator);
            applyStepsMask(generator);
            applyFeatureAllowList(generator);
            // 过滤只能改变「哪些 index 被跑」，不能改变全局顺序本身。
            checkFeaturesPerStep(generator);
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
                    if (this.options.realPath()) {
                        // 真实服务端路径：3×3 possibleBiomes、carverBiome、region 的 biomeManager，
                        // 并输出各阶段 prime/更新后的高度图。
                        // Real-server path: 3x3 possibleBiomes, carverBiome, region's biomeManager,
                        // plus every heightmap after the stage's prime/update.
                        writeRealPath(seed, pos, proto, generator, generatorSettings, state, resolver);
                        continue;
                    }
                    if (this.options.through() >= 4 && this.options.mergeOrders() != null) {
                        for (String order : this.options.mergeOrders().split(",")) {
                            writeMergedFeatures(seed, pos, order.trim(), generator,
                                    generatorSettings, state, resolver);
                        }
                        continue;
                    }
                    if (this.options.through() >= 4) {
                        if (this.options.structures()) {
                            writeStructuredFeatures(seed, pos, generator, generatorSettings, state,
                                    resolver);
                        } else {
                            writeFeatures(seed, pos, generator, generatorSettings, state, resolver);
                        }
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
         * 邻居溢出合并对拍：目标区块 `pos` 的最终方块要由 9 个中心（它自己和 8 个邻居）
         * 各自的 `applyBiomeDecoration` 写出来。这里造 5×5 个跑到 CARVERS 的
         * ProtoChunk（带结构时整张网格先跑官方起点 / 引用，5×5 各按自己的引用带
         * Beardifier），再按 `order` 依次以 3×3 里的 9 个区块为中心各跑一次
         * `applyBiomeDecoration`，都写进**同一批**共享 ProtoChunk —— 和官方
         * `WorldGenRegion` 直接写邻居 ProtoChunk 一样，后跑的中心看得到先跑的写。
         * 输出目标区块的最终方块，以及每个中心落进目标区块的 setBlock 次数（证明
         * 8 个邻居的 features 都真的跑过、真的溢出）。
         *
         * <p>`order`：`center-first` 先跑目标自己再按 (dz,dx) 跑 8 个邻居，
         * `center-last` 先跑 8 个邻居再跑目标。官方真实服务端里先后由调度决定。
         *
         * Neighbor-spill merge oracle: build a 5×5 of CARVERS ProtoChunks, run vanilla
         * `applyBiomeDecoration` centered on each of the 9 chunks around `pos` in
         * `order`, all writing into the same shared ProtoChunks (as vanilla's
         * `WorldGenRegion` does), then emit the target's blocks and per-center write
         * counts into the target.
         */
        private void writeMergedFeatures(long seed, ChunkPos pos, String order,
                NoiseBasedChunkGenerator generator, NoiseGeneratorSettings generatorSettings,
                RandomState randomState, BiomeResolver resolver) throws Exception {
            boolean structures = this.options.structures();
            // 5×5 下标 (dz + 2) * 5 + (dx + 2)。
            TerrainChunk[] area = new TerrainChunk[25];
            StructureChunkGrid structureGrid = null;
            net.minecraft.server.level.ServerLevel entityStub = null;
            WorldOptions worldOptions = new WorldOptions(seed, structures, false);
            if (structures) {
                // 5×5 每格的 Beardifier / 引用要读它 ±8 的起点，地形的 carverBiome 读
                // 它 ±8 的群系：网格半径 10。
                int radius = 10;
                int size = radius * 2 + 1;
                ProtoChunk[][] grid = new ProtoChunk[size][size];
                for (int dx = -radius; dx <= radius; dx++) {
                    for (int dz = -radius; dz <= radius; dz++) {
                        grid[dx + radius][dz + radius] =
                                newChunk(new ChunkPos(pos.x() + dx, pos.z() + dz), generatorSettings);
                    }
                }
                structureGrid = new StructureChunkGrid(pos, grid, radius);
                entityStub = installEntityStub(seed, generator);
                HolderLookup<StructureSet> setLookup = registries.lookupOrThrow(Registries.STRUCTURE_SET);
                ChunkGeneratorStructureState structureState =
                        ChunkGeneratorStructureState.createForNormal(
                                randomState, seed, new ChunkPos(0, 0), generator.getBiomeSource(),
                                setLookup);
                structureState.ensureStructuresGenerated();
                for (int dx = -radius; dx <= radius; dx++) {
                    for (int dz = -radius; dz <= radius; dz++) {
                        ProtoChunk chunk = structureGrid.chunk(pos, pos.x() + dx, pos.z() + dz);
                        StructureManager manager = new StructureManager(
                                structureLevel(registries, chunk, structureGrid, pos), worldOptions, null);
                        generator.createStructures(registries, structureState, manager, chunk,
                                templates, net.minecraft.resources.ResourceKey.create(
                                        net.minecraft.core.registries.Registries.DIMENSION,
                                        Identifier.withDefaultNamespace("overworld")));
                    }
                }
                WorldGenLevel referenceRegion = referenceRegion(pos, structureGrid, registries);
                for (int dz = -2; dz <= 2; dz++) {
                    for (int dx = -2; dx <= 2; dx++) {
                        ProtoChunk chunk = structureGrid.chunk(pos, pos.x() + dx, pos.z() + dz);
                        generator.createReferences(referenceRegion,
                                new StructureManager(referenceRegion, worldOptions, null), chunk);
                    }
                }
                for (int dz = -radius; dz <= radius; dz++) {
                    for (int dx = -radius; dx <= radius; dx++) {
                        ProtoChunk chunk = structureGrid.chunk(pos, pos.x() + dx, pos.z() + dz);
                        generator.createBiomes(randomState, Blender.empty(), null, chunk).join();
                        chunk.setPersistedStatus(ChunkStatus.BIOMES);
                        if (Math.abs(dx) <= 2 && Math.abs(dz) <= 2) {
                            area[(dz + 2) * 5 + dx + 2] = new TerrainChunk(chunk);
                        }
                    }
                }
                TerrainChunk[] centerView = view3x3(area, 0, 0);
                for (int dz = -2; dz <= 2; dz++) {
                    for (int dx = -2; dx <= 2; dx++) {
                        ChunkPos at = new ChunkPos(pos.x() + dx, pos.z() + dz);
                        ProtoChunk chunk = structureGrid.chunk(pos, at.x(), at.z());
                        StructureManager manager = new StructureManager(
                                featureLevelForStructures(pos, seed, generatorSettings, centerView,
                                        randomState, resolver, structureGrid, entityStub),
                                worldOptions, null);
                        buildStructuredTerrain(seed, at, chunk, generator, generatorSettings,
                                randomState, resolver, structureGrid,
                                Beardifier.forStructuresInChunk(manager, at));
                    }
                }
            } else {
                for (int dz = -2; dz <= 2; dz++) {
                    for (int dx = -2; dx <= 2; dx++) {
                        area[(dz + 2) * 5 + dx + 2] = generateTerrain(
                                seed, new ChunkPos(pos.x() + dx, pos.z() + dz), generator,
                                generatorSettings, randomState, resolver);
                    }
                }
            }

            if ("isolated".equals(order)) {
                writeIsolatedFeatures(seed, pos, area, generator, generatorSettings, randomState,
                        resolver, structureGrid, entityStub, worldOptions);
                return;
            }
            List<int[]> centers = new ArrayList<>();
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    if (dx != 0 || dz != 0) {
                        centers.add(new int[] {dx, dz});
                    }
                }
            }
            switch (order) {
                case "center-first" -> centers.add(0, new int[] {0, 0});
                case "center-last" -> centers.add(new int[] {0, 0});
                default -> throw new IllegalArgumentException("unknown merge order " + order);
            }
            long[] writes = new long[9];
            int runs = 0;
            for (int[] offset : centers) {
                int dx = offset[0];
                int dz = offset[1];
                ChunkPos center = new ChunkPos(pos.x() + dx, pos.z() + dz);
                TerrainChunk[] view = view3x3(area, dx, dz);
                final WorldGenLevel[] holder = new WorldGenLevel[1];
                FeatureLevelHandler handler = new FeatureLevelHandler(center, seed, generatorSettings,
                        wrapped(view), this.registries,
                        new BiomeManager((qx, qy, qz) -> holder[0].getNoiseBiome(qx, qy, qz),
                                BiomeManager.obfuscateSeed(seed)),
                        resolver, randomState, structureGrid, entityStub);
                handler.countWritesInto = pos;
                WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(
                        ChunkgenOracle.class.getClassLoader(),
                        new Class<?>[] {WorldGenLevel.class}, handler);
                holder[0] = level;
                generator.applyBiomeDecoration(level, view[4].chunk(),
                        new StructureManager(level, worldOptions, null));
                writes[(dz + 1) * 3 + dx + 1] = handler.countedWrites;
                runs++;
            }

            JsonObject line = new JsonObject();
            line.addProperty("type", "merged");
            line.addProperty("seed", seed);
            line.addProperty("x", pos.x());
            line.addProperty("z", pos.z());
            line.addProperty("order", order);
            line.addProperty("runs", runs);
            line.addProperty("structures", structures);
            JsonArray counts = new JsonArray();
            for (long count : writes) {
                counts.add(count);
            }
            line.add("writes", counts);
            addBlocks(line, area[12].chunk());
            write(line);
        }

        /**
         * `isolated`：9 个中心各自在**干净**的 3×3 上跑官方 `applyBiomeDecoration`（每跑完
         * 一个就把 5×5 还原成 CARVERS），记下每个中心写进目标区块的格子和最终状态：
         * 中心自己按「和干净地形不同」记，邻居按 setBlock 落点并上和干净地形不同的格子
         * （矿石走 `BulkSectionAccess` 不经过 setBlock）。合并交给 Rust 侧，按游戏的
         * 固定先后规则合（同格先写的赢）；这样比的是「官方每个中心的输出」加
         * 「游戏的合并路径」。
         *
         * `isolated`: run vanilla `applyBiomeDecoration` for each of the 9 centers on a
         * clean 3×3 (restoring the 5×5 to CARVERS after each run) and record every
         * cell each center writes into the target with its final state. The Rust side
         * merges them with the game's fixed-order first-writer rule.
         */
        private void writeIsolatedFeatures(long seed, ChunkPos pos, TerrainChunk[] area,
                NoiseBasedChunkGenerator generator, NoiseGeneratorSettings generatorSettings,
                RandomState randomState, BiomeResolver resolver, StructureChunkGrid structureGrid,
                net.minecraft.server.level.ServerLevel entityStub, WorldOptions worldOptions)
                throws Exception {
            AreaSnapshot[] saved = new AreaSnapshot[area.length];
            for (int i = 0; i < area.length; i++) {
                saved[i] = AreaSnapshot.take(area[i].chunk());
            }
            ProtoChunk target = area[12].chunk();
            int minY = target.getMinY();
            int height = target.getHeight();
            List<BlockState> palette = new ArrayList<>();
            java.util.Map<BlockState, Integer> ids = new java.util.HashMap<>();
            JsonArray sources = new JsonArray();
            int runs = 0;
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    for (int i = 0; i < area.length; i++) {
                        saved[i].restore(area[i].chunk());
                    }
                    ChunkPos center = new ChunkPos(pos.x() + dx, pos.z() + dz);
                    TerrainChunk[] view = view3x3(area, dx, dz);
                    final WorldGenLevel[] holder = new WorldGenLevel[1];
                    FeatureLevelHandler handler = new FeatureLevelHandler(center, seed,
                            generatorSettings, wrapped(view), this.registries,
                            new BiomeManager((qx, qy, qz) -> holder[0].getNoiseBiome(qx, qy, qz),
                                    BiomeManager.obfuscateSeed(seed)),
                            resolver, randomState, structureGrid, entityStub);
                    handler.countWritesInto = pos;
                    WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(
                            ChunkgenOracle.class.getClassLoader(),
                            new Class<?>[] {WorldGenLevel.class}, handler);
                    holder[0] = level;
                    generator.applyBiomeDecoration(level, view[4].chunk(),
                            new StructureManager(level, worldOptions, null));
                    runs++;
                    boolean self = dx == 0 && dz == 0;
                    JsonArray writes = new JsonArray();
                    AreaSnapshot clean = saved[12];
                    for (int y = 0; y < height; y++) {
                        for (int x = 0; x < 16; x++) {
                            for (int z = 0; z < 16; z++) {
                                BlockPos at = new BlockPos(pos.getMinBlockX() + x, minY + y,
                                        pos.getMinBlockZ() + z);
                                BlockState now = target.getBlockState(at);
                                boolean changed = now != clean.state(target, y, x, z);
                                boolean touched = !self && handler.touchedInTarget.contains(at.asLong());
                                if (!changed && !touched) {
                                    continue;
                                }
                                Integer id = ids.get(now);
                                if (id == null) {
                                    id = palette.size();
                                    palette.add(now);
                                    ids.put(now, id);
                                }
                                JsonArray write = new JsonArray();
                                write.add((y * 16 + x) * 16 + z);
                                write.add(id);
                                writes.add(write);
                            }
                        }
                    }
                    sources.add(writes);
                }
            }
            for (int i = 0; i < area.length; i++) {
                saved[i].restore(area[i].chunk());
            }
            JsonObject line = new JsonObject();
            line.addProperty("type", "isolated");
            line.addProperty("seed", seed);
            line.addProperty("x", pos.x());
            line.addProperty("z", pos.z());
            line.addProperty("order", "isolated");
            line.addProperty("runs", runs);
            JsonArray names = new JsonArray();
            for (BlockState state : palette) {
                names.add(state.toString());
            }
            line.add("writePalette", names);
            line.add("sources", sources);
            addBlocks(line, target);
            write(line);
        }

        /**
         * 一个 ProtoChunk 跑地物前的样子：段（方块 + 群系）、高度图、方块实体表。
         * 还原时把段换回拷贝，`isolated` 每个中心都从同一份干净地形开始。
         *
         * A ProtoChunk before features: sections, heightmaps and block-entity maps,
         * restored so every `isolated` center starts from the same clean terrain.
         */
        private record AreaSnapshot(net.minecraft.world.level.chunk.LevelChunkSection[] sections,
                java.util.Map<Heightmap.Types, long[]> heightmaps,
                java.util.Map<BlockPos, CompoundTag> pending,
                java.util.Map<BlockPos, BlockEntity> entities) {
            static AreaSnapshot take(ProtoChunk chunk) throws Exception {
                net.minecraft.world.level.chunk.LevelChunkSection[] live = sections(chunk);
                net.minecraft.world.level.chunk.LevelChunkSection[] copy =
                        new net.minecraft.world.level.chunk.LevelChunkSection[live.length];
                for (int i = 0; i < live.length; i++) {
                    copy[i] = live[i].copy();
                }
                java.util.Map<Heightmap.Types, long[]> maps = new java.util.EnumMap<>(Heightmap.Types.class);
                for (java.util.Map.Entry<Heightmap.Types, Heightmap> entry : chunk.getHeightmaps()) {
                    maps.put(entry.getKey(), entry.getValue().getRawData().clone());
                }
                @SuppressWarnings("unchecked")
                java.util.Map<BlockPos, CompoundTag> pending =
                        (java.util.Map<BlockPos, CompoundTag>) blockEntityMap(chunk, "pendingBlockEntities");
                @SuppressWarnings("unchecked")
                java.util.Map<BlockPos, BlockEntity> entities =
                        (java.util.Map<BlockPos, BlockEntity>) blockEntityMap(chunk, "blockEntities");
                return new AreaSnapshot(copy, maps, new java.util.HashMap<>(pending),
                        new java.util.HashMap<>(entities));
            }

            void restore(ProtoChunk chunk) throws Exception {
                net.minecraft.world.level.chunk.LevelChunkSection[] live = sections(chunk);
                for (int i = 0; i < live.length; i++) {
                    live[i] = this.sections[i].copy();
                }
                for (java.util.Map.Entry<Heightmap.Types, long[]> entry : this.heightmaps.entrySet()) {
                    chunk.setHeightmap(entry.getKey(), entry.getValue().clone());
                }
                @SuppressWarnings("unchecked")
                java.util.Map<BlockPos, CompoundTag> pendingNow =
                        (java.util.Map<BlockPos, CompoundTag>) blockEntityMap(chunk, "pendingBlockEntities");
                pendingNow.clear();
                pendingNow.putAll(this.pending);
                @SuppressWarnings("unchecked")
                java.util.Map<BlockPos, BlockEntity> entitiesNow =
                        (java.util.Map<BlockPos, BlockEntity>) blockEntityMap(chunk, "blockEntities");
                entitiesNow.clear();
                entitiesNow.putAll(this.entities);
            }

            BlockState state(ProtoChunk chunk, int y, int x, int z) {
                return this.sections[y >> 4].getBlockState(x, y & 15, z);
            }

            private static net.minecraft.world.level.chunk.LevelChunkSection[] sections(ProtoChunk chunk)
                    throws Exception {
                Field field = ChunkAccess.class.getDeclaredField("sections");
                field.setAccessible(true);
                return (net.minecraft.world.level.chunk.LevelChunkSection[]) field.get(chunk);
            }

            private static java.util.Map<?, ?> blockEntityMap(ProtoChunk chunk, String name)
                    throws Exception {
                Field field = ChunkAccess.class.getDeclaredField(name);
                field.setAccessible(true);
                return (java.util.Map<?, ?>) field.get(chunk);
            }
        }

        /** 5×5 里以 (dx, dz) 为中心的 3×3，按 (dz, dx) 排。 */
        private static TerrainChunk[] view3x3(TerrainChunk[] area, int dx, int dz) {
            TerrainChunk[] view = new TerrainChunk[9];
            for (int vz = -1; vz <= 1; vz++) {
                for (int vx = -1; vx <= 1; vx++) {
                    view[(vz + 1) * 3 + vx + 1] = area[(dz + vz + 2) * 5 + dx + vx + 2];
                }
            }
            return view;
        }

        /**
         * 带结构的 `features` 阶段：照 Director 的 ChunkStatus 顺序，先跑官方
         * `createStructures` / `createReferences`，再用真实 `Beardifier` 把 3×3 写区
         * 跑到 CARVERS，最后让官方 `applyBiomeDecoration` 在 `shouldGenerateStructures()`
         * 为真的 StructureManager 下同时放结构件和地物。
         *
         * <p>字节码依据：`ChunkGenerator.applyBiomeDecoration` 238–418 的
         * `startsForStructure` / `placeInChunk` 分支；`StructureManager.startsForStructure`
         * 按 STRUCTURE_REFERENCES / STRUCTURE_STARTS 取 17×17 邻域；
         * `NoiseBasedChunkGenerator.createNoiseChunk` 用
         * `Beardifier.forStructuresInChunk(manager, chunkPos)`。
         */
        private void writeStructuredFeatures(long seed, ChunkPos pos,
                NoiseBasedChunkGenerator generator, NoiseGeneratorSettings generatorSettings,
                RandomState randomState, BiomeResolver resolver) throws Exception {
            int size = StructureChunkGrid.size();
            ProtoChunk[][] grid = new ProtoChunk[size][size];
            for (int dx = -StructureChunkGrid.RADIUS; dx <= StructureChunkGrid.RADIUS; dx++) {
                for (int dz = -StructureChunkGrid.RADIUS; dz <= StructureChunkGrid.RADIUS; dz++) {
                    grid[dx + StructureChunkGrid.RADIUS][dz + StructureChunkGrid.RADIUS] =
                            newChunk(new ChunkPos(pos.x() + dx, pos.z() + dz), generatorSettings);
                }
            }
            StructureChunkGrid structureGrid = new StructureChunkGrid(pos, grid);
            net.minecraft.server.level.ServerLevel entityStub = installEntityStub(seed, generator);
            WorldOptions worldOptions = new WorldOptions(seed, true, false);
            HolderLookup<StructureSet> setLookup = registries.lookupOrThrow(Registries.STRUCTURE_SET);
            ChunkGeneratorStructureState structureState =
                    ChunkGeneratorStructureState.createForNormal(
                            randomState, seed, new ChunkPos(0, 0), generator.getBiomeSource(),
                            setLookup);
            structureState.ensureStructuresGenerated();
            // 1. STRUCTURE_STARTS：中心 ±9，够 3×3 写区各自的 createReferences 用。
            for (int dx = -StructureChunkGrid.RADIUS; dx <= StructureChunkGrid.RADIUS; dx++) {
                for (int dz = -StructureChunkGrid.RADIUS; dz <= StructureChunkGrid.RADIUS; dz++) {
                    ProtoChunk chunk = structureGrid.chunk(pos, pos.x() + dx, pos.z() + dz);
                    StructureManager manager = new StructureManager(
                            structureLevel(registries, chunk, structureGrid, pos), worldOptions, null);
                    generator.createStructures(registries, structureState, manager, chunk,
                            templates, net.minecraft.resources.ResourceKey.create(
                                    net.minecraft.core.registries.Registries.DIMENSION,
                                    Identifier.withDefaultNamespace("overworld")));
                }
            }
            // 2. STRUCTURE_REFERENCES：先给中心算，`applyBiomeDecoration` 和中心的
            // Beardifier 都读它。3×3 里的另外 8 个也要算，因为它们的 CARVERS 地形
            // 在游戏侧各自带自己的 Beardifier。
            WorldGenLevel referenceRegion = referenceRegion(pos, structureGrid, registries);
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    ProtoChunk chunk = structureGrid.chunk(pos, pos.x() + dx, pos.z() + dz);
                    generator.createReferences(referenceRegion,
                            new StructureManager(referenceRegion, worldOptions, null), chunk);
                }
            }
            // 3. BIOMES + TERRAIN：3×3 每格先填 quart 群系，再按自己的引用造
            // Beardifier 跑到 CARVERS；高度图用官方 `primeHeightmaps` 补齐。
            // carverBiome 会读中心 ±8 的群系调色板，整张网格都要填 quart 格，
            // 但只有 3×3 要跑到 CARVERS。
            TerrainChunk[] chunks = new TerrainChunk[9];
            for (int dz = -StructureChunkGrid.RADIUS; dz <= StructureChunkGrid.RADIUS; dz++) {
                for (int dx = -StructureChunkGrid.RADIUS; dx <= StructureChunkGrid.RADIUS; dx++) {
                    ProtoChunk chunk = structureGrid.chunk(pos, pos.x() + dx, pos.z() + dz);
                    generator.createBiomes(randomState, Blender.empty(), null, chunk).join();
                    chunk.setPersistedStatus(ChunkStatus.BIOMES);
                    if (Math.abs(dx) <= 1 && Math.abs(dz) <= 1) {
                        chunks[(dz + 1) * 3 + dx + 1] = new TerrainChunk(chunk);
                    }
                }
            }
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    ChunkPos at = new ChunkPos(pos.x() + dx, pos.z() + dz);
                    ProtoChunk chunk = structureGrid.chunk(pos, at.x(), at.z());
                    StructureManager manager = new StructureManager(
                            featureLevelForStructures(pos, seed, generatorSettings, chunks,
                                    randomState, resolver, structureGrid, entityStub),
                            worldOptions, null);
                    buildStructuredTerrain(seed, at, chunk, generator, generatorSettings,
                            randomState, resolver, structureGrid,
                            Beardifier.forStructuresInChunk(manager, at));
                }
            }
            // 4. FEATURES：和 writeFeatures 同一个 3×3 写区代理，只是这次
            // StructureManager 开结构，且 4 参数 getChunk 能看到整张起点网格。
            final WorldGenLevel[] holder = new WorldGenLevel[1];
            FeatureLevelHandler handler = new FeatureLevelHandler(pos, seed, generatorSettings,
                    wrapped(chunks), this.registries,
                    new BiomeManager((qx, qy, qz) -> holder[0].getNoiseBiome(qx, qy, qz),
                            BiomeManager.obfuscateSeed(seed)),
                    resolver, randomState, structureGrid, entityStub);
            WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(
                    ChunkgenOracle.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class}, handler);
            holder[0] = level;
            // `--steps` 是「只跑这些 step」的对拍开关：Java 侧原本只清空群系的
            // HolderSet，结构分支仍会在每个 step 放件；这里按 structure.step() 把
            // 不属于掩码的起点挡掉，与 Rust 的 `features` 循环对称。
            StructureManager structures = new StructureManager(level, worldOptions, null) {
                @Override
                public List<StructureStart> startsForStructure(int x, int z, Structure structure) {
                    int step = structure.step().ordinal();
                    if ((options.stepsMask() & (1 << step)) == 0) {
                        return List.of();
                    }
                    return super.startsForStructure(x, z, structure);
                }
            };
            generator.applyBiomeDecoration(level, chunks[4].chunk(), structures);

            JsonArray results = new JsonArray();
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    int index = (dz + 1) * 3 + dx + 1;
                    JsonObject entry = new JsonObject();
                    entry.addProperty("dx", dx);
                    entry.addProperty("dz", dz);
                    addBlocks(entry, chunks[index].chunk());
                    addBlockEntities(entry, chunks[index].chunk());
                    results.add(entry);
                }
            }
            JsonObject line = new JsonObject();
            line.addProperty("type", "features");
            line.addProperty("seed", seed);
            line.addProperty("x", pos.x());
            line.addProperty("z", pos.z());
            line.add("chunks", results);
            line.addProperty("steps", this.options.stepsMask());
            // 诊断：中心区块的 Beardifier 是否非空、引用了哪些结构。
            line.addProperty("beardifier",
                    !Beardifier.forStructuresInChunk(structures, pos).equals(Beardifier.EMPTY));
            line.add("centerRefs", centerRefNames(structureGrid.chunk(pos, pos.x(), pos.z())));
            write(line);
            // 覆盖统计：中心区块的起点 / 引用 / 每种结构的结构件数，供 Rust 侧
            // 和游戏侧 `game_features_with_mask` 的放置次数对账。
            writeStructureCoverage(seed, pos, structureGrid.chunk(pos, pos.x(), pos.z()));
        }

        /** 中心区块的引用结构名，用于诊断真 Beardifier 是否生效。 */
        private JsonArray centerRefNames(ProtoChunk center) {
            Registry<Structure> structureReg = registries.lookupOrThrow(Registries.STRUCTURE);
            List<String> names = new ArrayList<>();
            for (Structure structure : center.getAllReferences().keySet()) {
                names.add(structureReg.getKey(structure).toString());
            }
            names.sort(Comparator.naturalOrder());
            JsonArray out = new JsonArray();
            for (String name : names) {
                out.add(name);
            }
            return out;
        }

        /** 中心区块实际生成了哪些结构、各有多少结构件。 */
        private void writeStructureCoverage(long seed, ChunkPos pos, ProtoChunk center)
                throws Exception {
            Registry<Structure> structureReg = registries.lookupOrThrow(Registries.STRUCTURE);
            JsonArray starts = new JsonArray();
            List<String> names = new ArrayList<>();
            for (Structure structure : center.getAllStarts().keySet()) {
                names.add(structureReg.getKey(structure).toString());
            }
            names.sort(Comparator.naturalOrder());
            for (String name : names) {
                Structure key = structureReg.getValue(Identifier.parse(name));
                StructureStart start = center.getAllStarts().get(key);
                if (start == null || !start.isValid()) {
                    continue;
                }
                JsonArray row = new JsonArray();
                row.add(name);
                row.add(start.getPieces().size());
                starts.add(row);
            }
            JsonArray refs = new JsonArray();
            List<String> refNames = new ArrayList<>();
            for (Structure structure : center.getAllReferences().keySet()) {
                refNames.add(structureReg.getKey(structure).toString());
            }
            refNames.sort(Comparator.naturalOrder());
            for (String name : refNames) {
                Structure key = structureReg.getValue(Identifier.parse(name));
                var keys = center.getAllReferences().get(key);
                if (keys == null) {
                    continue;
                }
                List<Long> packed = new ArrayList<>();
                for (long value : keys) {
                    packed.add(value);
                }
                java.util.Collections.sort(packed);
                for (long value : packed) {
                    JsonArray row = new JsonArray();
                    row.add(name);
                    row.add(value);
                    refs.add(row);
                }
            }
            JsonObject line = new JsonObject();
            line.addProperty("type", "coverage");
            line.addProperty("seed", seed);
            line.addProperty("x", pos.x());
            line.addProperty("z", pos.z());
            line.add("starts", starts);
            line.add("references", refs);
            // 结构件清单（含包围盒）：先定位「布局错」还是「放置错」。
            JsonArray pieces = new JsonArray();
            for (String name : names) {
                Structure key = structureReg.getValue(Identifier.parse(name));
                StructureStart start = center.getAllStarts().get(key);
                if (start == null || !start.isValid()) {
                    continue;
                }
                for (var piece : start.getPieces()) {
                    JsonArray row = new JsonArray();
                    row.add(name);
                    row.add(BuiltInRegistries.STRUCTURE_PIECE.getKey(piece.getType()).toString());
                    row.add(piece.getBoundingBox().minX());
                    row.add(piece.getBoundingBox().minY());
                    row.add(piece.getBoundingBox().minZ());
                    row.add(piece.getBoundingBox().maxX());
                    row.add(piece.getBoundingBox().maxY());
                    row.add(piece.getBoundingBox().maxZ());
                    pieces.add(row);
                }
            }
            line.add("pieces", pieces);
            write(line);
        }

        /** createStructures 的 LevelAccessor：主区块是当前区块，getChunk 也能取邻域。 */
        private static net.minecraft.world.level.LevelAccessor structureLevel(
                RegistryAccess registries, ProtoChunk chunk, StructureChunkGrid grid,
                ChunkPos center) {
            return (net.minecraft.world.level.LevelAccessor) Proxy.newProxyInstance(
                    ChunkgenOracle.class.getClassLoader(),
                    new Class<?>[] {net.minecraft.world.level.LevelAccessor.class},
                    (proxy, method, args) -> {
                        switch (method.getName()) {
                            case "registryAccess": return registries;
                            case "getChunk": {
                                if (args != null && args.length >= 2
                                        && args[0] instanceof Integer) {
                                    return grid.chunk(center, (Integer) args[0], (Integer) args[1]);
                                }
                                return chunk;
                            }
                            case "hasChunk": {
                                if (args != null && args.length >= 2
                                        && args[0] instanceof Integer) {
                                    return grid.has(center, (Integer) args[0], (Integer) args[1]);
                                }
                                return Boolean.TRUE;
                            }
                            case "getSeed": return 0L;
                            case "getMinY": return chunk.getMinY();
                            case "getHeight": return chunk.getHeight();
                            case "getMaxY": return chunk.getMaxY();
                            case "getBlockState":
                                return Blocks.AIR.defaultBlockState();
                            case "getFluidState":
                                return net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
                            case "hashCode": return System.identityHashCode(proxy);
                            case "equals": return proxy == args[0];
                            case "toString": return "StructureLevel";
                            default:
                                if (method.getReturnType() == boolean.class) return false;
                                if (method.getReturnType() == int.class) return 0;
                                if (method.getReturnType() == long.class) return 0L;
                                return null;
                        }
                    });
        }

        /** createReferences 只读 `getChunk(x,z)`（半径 8）。 */
        private static WorldGenLevel referenceRegion(ChunkPos center, StructureChunkGrid grid,
                RegistryAccess registries) {
            return (WorldGenLevel) Proxy.newProxyInstance(
                    ChunkgenOracle.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class},
                    (proxy, method, args) -> {
                        switch (method.getName()) {
                            case "getChunk": {
                                int x, z;
                                if (args[0] instanceof BlockPos pos) {
                                    x = pos.getX() >> 4;
                                    z = pos.getZ() >> 4;
                                } else {
                                    x = (Integer) args[0];
                                    z = (Integer) args[1];
                                }
                                return grid.chunk(center, x, z);
                            }
                            case "hasChunk": return grid.has(center,
                                    (Integer) args[0], (Integer) args[1]);
                            case "registryAccess": return registries;
                            case "hashCode": return System.identityHashCode(proxy);
                            case "equals": return proxy == args[0];
                            case "toString": return "StructureReferenceRegion";
                            default:
                                if (method.getReturnType() == boolean.class) return false;
                                if (method.getReturnType() == int.class) return 0;
                                if (method.getReturnType() == long.class) return 0L;
                                return null;
                        }
                    });
        }

        /** Beardifier 要读中心区块的 References 和被引用的起点，用同一个写区代理。 */
        private WorldGenLevel featureLevelForStructures(ChunkPos center, long seed,
                NoiseGeneratorSettings settings, TerrainChunk[] chunks, RandomState randomState,
                BiomeResolver resolver, StructureChunkGrid grid,
                net.minecraft.server.level.ServerLevel entityStub) throws Exception {
            final WorldGenLevel[] holder = new WorldGenLevel[1];
            FeatureLevelHandler handler = new FeatureLevelHandler(center, seed, settings,
                    wrapped(chunks), this.registries,
                    new BiomeManager((qx, qy, qz) -> holder[0].getNoiseBiome(qx, qy, qz),
                            BiomeManager.obfuscateSeed(seed)),
                    resolver, randomState, grid, entityStub);
            WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(
                    ChunkgenOracle.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class}, handler);
            holder[0] = level;
            return level;
        }

        /** 真 Beardifier 的 CARVERS：和 buildRealTerrain 同样走 17×17 WorldGenRegion。 */
        private void buildStructuredTerrain(long seed, ChunkPos pos, ProtoChunk center,
                NoiseBasedChunkGenerator generator, NoiseGeneratorSettings gs,
                RandomState state, BiomeResolver resolver, StructureChunkGrid grid,
                Beardifier beardifier) throws Exception {
            ProtoChunk[][] neighbors = new ProtoChunk[17][17];
            for (int dx = -8; dx <= 8; dx++) {
                for (int dz = -8; dz <= 8; dz++) {
                    neighbors[dx + 8][dz + 8] = grid.chunk(pos, pos.x() + dx, pos.z() + dz);
                }
            }
            net.minecraft.server.level.WorldGenRegion region =
                    makeRegion(center, neighbors, pos, seed, resolver);
            Set<Holder<Biome>> possibleBiomes = collectPossibleBiomes(region, pos);
            DensityVolume volume = new DensityVolume(
                    16, gs.noiseSettings().height(), 16,
                    pos.getMinBlockX(), gs.noiseSettings().minY(), pos.getMinBlockZ());
            try (NoiseChunk noiseChunk = new NoiseChunk(
                    state, beardifier, gs, instancePicker(generator).get(),
                    Blender.empty(), volume)) {
                this.doFill.invoke(generator, noiseChunk, center);
                this.buildSurface.invoke(generator, center, noiseChunk, state,
                        region.getBiomeManager(), possibleBiomes, gs.materialRule().value());
                this.generateCarvers.invoke(generator, center, Blender.empty(), noiseChunk,
                        state, region.getBiomeManager(), region, gs.materialRule().value());
            }
            center.setPersistedStatus(ChunkStatus.TERRAIN);
            Heightmap.primeHeightmaps(center, ChunkStatus.FINAL_HEIGHTMAPS);
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
                BiomeResolver resolver) throws Exception {
            TerrainChunk[] chunks = new TerrainChunk[9];
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    chunks[(dz + 1) * 3 + dx + 1] = generateTerrain(
                            seed, new ChunkPos(pos.x() + dx, pos.z() + dz), generator,
                            generatorSettings, randomState, resolver);
                }
            }
            // WorldGenRegion 构造 149-165：BiomeManager 的 BiomeResolver 是 region 自己，
            // 不是 ServerLevel.uncachedBiomeResolver()。这里让代理实现 BiomeResolver，
            // 用同一份 obfuscateSeed(seed) 建 manager，BiomeFilter 才会读 3×3 里
            // BIOMES 阶段填好的 quart 格；3×3 之外才回退到 uncached resolver。
            // WorldGenRegion ctor 149-165: the manager's BiomeResolver is the region
            // itself, not ServerLevel.uncachedBiomeResolver(). The proxy implements
            // BiomeResolver so BiomeFilter reads the stored quart cells, falling back
            // to the uncached resolver outside the 3x3.
            final WorldGenLevel[] holder = new WorldGenLevel[1];
            FeatureLevelHandler handler = new FeatureLevelHandler(pos, seed, generatorSettings,
                    wrapped(chunks), this.registries,
                    new BiomeManager((qx, qy, qz) -> holder[0].getNoiseBiome(qx, qy, qz),
                            BiomeManager.obfuscateSeed(seed)),
                    // 无结构模式也要 `getLevel()`：沙漠水井 / 化石的模板管理器挂在它的服务端上。
                    // Structure-less runs need `getLevel()` too: the desert well and fossil
                    // reach the template manager through its server.
                    resolver, randomState, null, installEntityStub(seed, generator));
            WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(
                    ChunkgenOracle.class.getClassLoader(),
                    new Class<?>[] {WorldGenLevel.class}, handler);
            holder[0] = level;
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
                    addBlockEntities(entry, chunks[index].chunk());
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
                BiomeResolver resolver) throws Exception {
            // 和游戏一样走真实服务端路径（3×3 possibleBiomes、region 的 biomeManager）。
            // Same real-server path as the game (3x3 possibleBiomes, region's biomeManager).
            ProtoChunk proto = newChunk(pos, generatorSettings);
            generator.createBiomes(state, Blender.empty(), null, proto).join();
            proto.setPersistedStatus(ChunkStatus.BIOMES);
            buildRealTerrain(seed, pos, proto, generator, generatorSettings, state, resolver);
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
        /**
         * 核对 {@code featuresPerStep[6]} 仍是导出的 34 项、且 {@code ore_clay}
         * 在 index 27。诊断过滤只能改变哪些 index 被跑，不能改变全局顺序；
         * 换了 jar 版本或 validate() 的行为变了，这里会立刻报错而不是静默算错。
         */
        private void checkFeaturesPerStep(NoiseBasedChunkGenerator generator) throws Exception {
            // 这组数字是主世界群系集合的指纹；下界的 step 6 只有远古残骸那几个，
            // 尺寸和下标都不适用。
            // These numbers fingerprint the overworld biome set; the nether step 6 holds
            // only the ancient debris features, so neither size nor index applies.
            if (!"minecraft:overworld".equalsIgnoreCase(this.options.settings())) {
                return;
            }
            Field field = net.minecraft.world.level.chunk.ChunkGenerator.class
                    .getDeclaredField("featuresPerStep");
            field.setAccessible(true);
            @SuppressWarnings("unchecked")
            java.util.function.Supplier<List<net.minecraft.world.level.biome.FeatureSorter.StepFeatureData>>
                    supplier = (java.util.function.Supplier<
                            List<net.minecraft.world.level.biome.FeatureSorter.StepFeatureData>>)
                            field.get(generator);
            List<net.minecraft.world.level.levelgen.placement.PlacedFeature> features =
                    supplier.get().get(6).features();
            Registry<net.minecraft.world.level.levelgen.placement.PlacedFeature> placed =
                    this.registries.lookupOrThrow(Registries.PLACED_FEATURE);
            if (features.size() != 34
                    || !placed.getKey(features.get(27)).toString()
                            .equals("minecraft:ore_clay")) {
                throw new IllegalStateException(
                        "featuresPerStep[6] changed: size=" + features.size()
                                + " index27=" + placed.getKey(features.get(27)));
            }
        }

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
            // 白名单是逗号分隔的一组条目：每个可以是 Feature 类型 ID（精确匹配 codec
            // 的类型 ID），也可以是 placed feature 的注册名（精确匹配）。这样既能按类型
            // 放开一整类，也能只隔离出单个 placed feature 做逐方块对拍。
            // `--ore-only` 传单个 "minecraft:ore"，走同一条路径。
            // Comma-separated allow list: each entry is either a Feature type ID
            // (matched against the codec type ID) or a placed-feature registry name
            // (matched exactly), so a whole type or a single placed feature can be run.
            Set<String> selectors = new HashSet<>();
            for (String part : allowed.split(",")) {
                String trimmed = part.trim();
                if (!trimmed.isEmpty()) {
                    selectors.add(trimmed);
                }
            }
            Registry<net.minecraft.world.level.levelgen.placement.PlacedFeature> placed =
                    this.registries.lookupOrThrow(Registries.PLACED_FEATURE);
            Set<String> keep = new HashSet<>();
            for (java.util.Map.Entry<net.minecraft.resources.ResourceKey<
                    net.minecraft.world.level.levelgen.placement.PlacedFeature>,
                    net.minecraft.world.level.levelgen.placement.PlacedFeature> entry
                    : placed.entrySet()) {
                net.minecraft.world.level.levelgen.placement.PlacedFeature value = entry.getValue();
                // 先按 codec 的类型 ID 比，再按 placed feature 注册名比；不靠注册名前缀猜。
                // Match the codec type ID first, then the placed-feature registry name.
                String name = entry.getKey().identifier().toString();
                if (selectors.contains(featureTypeId(value.feature().value())) || selectors.contains(name)) {
                    keep.add(name);
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

        /**
         * 真实服务端路径：照 {@code ChunkStatusTasks.buildTerrain}，用周围 3×3 群系调色板
         * 并集、{@code carverBiome} 分支和 {@code region.getBiomeManager()}。造 17×17 个
         * ProtoChunk（3×3 跑 {@code createBiomes}），不启动服务端、不接受 EULA。
         *
         * Real-server path: mirrors {@code ChunkStatusTasks.buildTerrain} with the
         * 3x3 biome-palette union, the {@code carverBiome} branch and
         * {@code region.getBiomeManager()}. Builds 17x17 ProtoChunks (3x3 run
         * {@code createBiomes}); no server, no EULA.
         */
        private void writeRealPath(long seed, ChunkPos pos, ProtoChunk center,
                NoiseBasedChunkGenerator generator, NoiseGeneratorSettings gs,
                RandomState state, BiomeResolver resolver) throws Exception {
            buildRealTerrain(seed, pos, center, generator, gs, state, resolver);
            JsonObject line = new JsonObject();
            line.addProperty("type", "carvers");
            line.addProperty("seed", seed);
            line.addProperty("x", pos.x());
            line.addProperty("z", pos.z());
            addBlocks(line, center);
            addHeightmaps(line, center);
            write(line);
        }

        /**
         * 真实服务端路径把 center 跑到 TERRAIN 并 prime FINAL_HEIGHTMAPS；center 须已有群系。
         *
         * Real-server path: run center (biomes already filled) to TERRAIN and prime
         * FINAL_HEIGHTMAPS.
         */
        private void buildRealTerrain(long seed, ChunkPos pos, ProtoChunk center,
                NoiseBasedChunkGenerator generator, NoiseGeneratorSettings gs,
                RandomState state, BiomeResolver resolver) throws Exception {
            ProtoChunk[][] neighbors = new ProtoChunk[17][17];
            for (int dx = -8; dx <= 8; dx++) {
                for (int dz = -8; dz <= 8; dz++) {
                    ProtoChunk p;
                    if (dx == 0 && dz == 0) {
                        p = center;
                    } else {
                        p = newChunk(new ChunkPos(pos.x() + dx, pos.z() + dz), gs);
                        // 3×3 的 quart 群系格必须填好：collectPossibleBiomes 和
                        // region.getBiomeManager().getBiome 都会读它们。
                        // The 3x3 quart cells must be filled: collectPossibleBiomes and
                        // region.getBiomeManager().getBiome both read them.
                        if (Math.abs(dx) <= 1 && Math.abs(dz) <= 1) {
                            generator.createBiomes(state, Blender.empty(), null, p).join();
                        }
                        p.setPersistedStatus(ChunkStatus.BIOMES);
                    }
                    neighbors[dx + 8][dz + 8] = p;
                }
            }
            net.minecraft.server.level.WorldGenRegion region =
                    makeRegion(center, neighbors, pos, seed, resolver);
            Set<Holder<Biome>> possibleBiomes = collectPossibleBiomes(region, pos);
            DensityVolume volume = new DensityVolume(
                    16, gs.noiseSettings().height(), 16,
                    pos.getMinBlockX(), gs.noiseSettings().minY(), pos.getMinBlockZ());
            try (NoiseChunk noiseChunk = new NoiseChunk(
                    state, Beardifier.EMPTY, gs, instancePicker(generator).get(),
                    Blender.empty(), volume)) {
                this.doFill.invoke(generator, noiseChunk, center);
                this.buildSurface.invoke(generator, center, noiseChunk, state,
                        region.getBiomeManager(), possibleBiomes, gs.materialRule().value());
                this.generateCarvers.invoke(generator, center, Blender.empty(), noiseChunk,
                        state, region.getBiomeManager(), region, gs.materialRule().value());
            }
            // ChunkStatusTasks.buildTerrain 的 thenApply：升到 TERRAIN 后 prime FINAL_HEIGHTMAPS。
            // buildTerrain's thenApply: raise to TERRAIN, then prime FINAL_HEIGHTMAPS.
            center.setPersistedStatus(ChunkStatus.TERRAIN);
            Heightmap.primeHeightmaps(center, ChunkStatus.FINAL_HEIGHTMAPS);
        }

        /**
         * 用 Unsafe 造一个不跑构造器的 WorldGenRegion，只填 getChunk/getBiomeManager 需要的字段。
         *
         * Build a constructor-less WorldGenRegion via Unsafe, filling only the fields
         * getChunk/getBiomeManager need.
         */
        private net.minecraft.server.level.WorldGenRegion makeRegion(
                ProtoChunk center, ProtoChunk[][] neighbors, ChunkPos pos, long seed,
                BiomeResolver resolver) throws Exception {
            net.minecraft.server.level.WorldGenRegion region =
                    (net.minecraft.server.level.WorldGenRegion) allocate(
                            net.minecraft.server.level.WorldGenRegion.class);
            net.minecraft.util.StaticCache2D<ChunkAccess> cache =
                    net.minecraft.util.StaticCache2D.create(pos.x(), pos.z(), 8,
                            (x, z) -> neighbors[x - pos.x() + 8][z - pos.z() + 8]);
            // terrain 步的 directDependencies：[BIOMES, BIOMES, STRUCTURE_STARTS×7]
            // （ChunkPyramid: addRequirement(STRUCTURE_STARTS,8) + addRequirement(BIOMES,1)）。
            // terrain step's directDependencies, from ChunkPyramid.
            List<ChunkStatus> deps = new ArrayList<>();
            deps.add(ChunkStatus.BIOMES);
            deps.add(ChunkStatus.BIOMES);
            for (int i = 2; i < 9; i++) {
                deps.add(ChunkStatus.STRUCTURE_STARTS);
            }
            net.minecraft.world.level.chunk.status.ChunkDependencies direct =
                    new net.minecraft.world.level.chunk.status.ChunkDependencies(
                            com.google.common.collect.ImmutableList.copyOf(deps));
            net.minecraft.world.level.chunk.status.ChunkStep step =
                    new net.minecraft.world.level.chunk.status.ChunkStep(
                            ChunkStatus.TERRAIN, direct, direct, 1, null);
            setField(net.minecraft.server.level.WorldGenRegion.class, region, "cache", cache);
            setField(net.minecraft.server.level.WorldGenRegion.class, region, "center", center);
            setField(net.minecraft.server.level.WorldGenRegion.class, region, "generatingStep", step);
            setField(net.minecraft.server.level.WorldGenRegion.class, region,
                    "uncachedBiomeResolver", resolver);
            setField(net.minecraft.server.level.WorldGenRegion.class, region, "biomeManager",
                    new BiomeManager(region, BiomeManager.obfuscateSeed(seed)));
            return region;
        }

        /**
         * 照 ChunkStatusTasks.collectPossibleBiomes(region, 1)：周围 3×3 调色板并集。
         *
         * Mirrors ChunkStatusTasks.collectPossibleBiomes(region, 1): the 3x3 palette union.
         */
        private Set<Holder<Biome>> collectPossibleBiomes(
                net.minecraft.server.level.WorldGenRegion region, ChunkPos pos) {
            Set<Holder<Biome>> out = new HashSet<>();
            for (int z = pos.z() - 1; z <= pos.z() + 1; z++) {
                for (int x = pos.x() - 1; x <= pos.x() + 1; x++) {
                    region.getChunk(x, z).collectBiomesInPalette(out);
                }
            }
            return out;
        }

        private static void installInertMob(Object type, Class<?> inert) {
            setField(net.minecraft.world.entity.EntityType.class, type, "factory",
                    (net.minecraft.world.entity.EntityType.EntityFactory<
                            net.minecraft.world.entity.Entity>) (t, level) -> {
                        try {
                            return (net.minecraft.world.entity.Entity)
                                    UNSAFE.allocateInstance(inert);
                        } catch (InstantiationException error) {
                            throw new IllegalStateException(error);
                        }
                    });
        }

        private static final class InertWitch
                extends net.minecraft.world.entity.monster.Witch {
            private InertWitch() {
                super(null, null);
            }

            @Override
            public void setPersistenceRequired() {
            }

            @Override
            public void snapTo(double x, double y, double z, float yRot, float xRot) {
            }

            @Override
            public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                    net.minecraft.world.level.ServerLevelAccessor level,
                    net.minecraft.world.DifficultyInstance difficulty,
                    net.minecraft.world.entity.EntitySpawnReason reason,
                    net.minecraft.world.entity.SpawnGroupData data) {
                return data;
            }
        }

        private static final class InertCat extends net.minecraft.world.entity.animal.feline.Cat {
            private InertCat() {
                super(null, null);
            }

            @Override
            public void setPersistenceRequired() {
            }

            @Override
            public void snapTo(double x, double y, double z, float yRot, float xRot) {
            }

            @Override
            public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                    net.minecraft.world.level.ServerLevelAccessor level,
                    net.minecraft.world.DifficultyInstance difficulty,
                    net.minecraft.world.entity.EntitySpawnReason reason,
                    net.minecraft.world.entity.SpawnGroupData data) {
                return data;
            }
        }

        private static final class InertElderGuardian
                extends net.minecraft.world.entity.monster.ElderGuardian {
            private InertElderGuardian() {
                super(null, null);
            }

            @Override
            public void setPersistenceRequired() {
            }

            @Override
            public void snapTo(double x, double y, double z, float yRot, float xRot) {
            }

            @Override
            public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                    net.minecraft.world.level.ServerLevelAccessor level,
                    net.minecraft.world.DifficultyInstance difficulty,
                    net.minecraft.world.entity.EntitySpawnReason reason,
                    net.minecraft.world.entity.SpawnGroupData data) {
                return data;
            }
        }

        private static final class InertEvoker extends net.minecraft.world.entity.monster.illager.Evoker {
            private InertEvoker() {
                super(null, null);
            }

            @Override
            public void setPersistenceRequired() {
            }

            @Override
            public void snapTo(double x, double y, double z, float yRot, float xRot) {
            }

            @Override
            public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                    net.minecraft.world.level.ServerLevelAccessor level,
                    net.minecraft.world.DifficultyInstance difficulty,
                    net.minecraft.world.entity.EntitySpawnReason reason,
                    net.minecraft.world.entity.SpawnGroupData data) {
                return data;
            }
        }

        private static final class InertVindicator
                extends net.minecraft.world.entity.monster.illager.Vindicator {
            private InertVindicator() {
                super(null, null);
            }

            @Override
            public void setPersistenceRequired() {
            }

            @Override
            public void snapTo(double x, double y, double z, float yRot, float xRot) {
            }

            @Override
            public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                    net.minecraft.world.level.ServerLevelAccessor level,
                    net.minecraft.world.DifficultyInstance difficulty,
                    net.minecraft.world.entity.EntitySpawnReason reason,
                    net.minecraft.world.entity.SpawnGroupData data) {
                return data;
            }
        }

        private static final class InertAllay extends net.minecraft.world.entity.animal.allay.Allay {
            private InertAllay() {
                super(null, null);
            }

            @Override
            public void setPersistenceRequired() {
            }

            @Override
            public void snapTo(double x, double y, double z, float yRot, float xRot) {
            }

            @Override
            public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                    net.minecraft.world.level.ServerLevelAccessor level,
                    net.minecraft.world.DifficultyInstance difficulty,
                    net.minecraft.world.entity.EntitySpawnReason reason,
                    net.minecraft.world.entity.SpawnGroupData data) {
                return data;
            }
        }

        /** 海底遗迹的 `drowned` 数据标记会造一只溺尸；同样用不走构造器的惰性实例。 */
        private static final class InertDrowned
                extends net.minecraft.world.entity.monster.zombie.Drowned {
            private InertDrowned() {
                super(null, null);
            }

            @Override
            public void setPersistenceRequired() {
            }

            @Override
            public void snapTo(double x, double y, double z, float yRot, float xRot) {
            }

            @Override
            public net.minecraft.world.entity.SpawnGroupData finalizeSpawn(
                    net.minecraft.world.level.ServerLevelAccessor level,
                    net.minecraft.world.DifficultyInstance difficulty,
                    net.minecraft.world.entity.EntitySpawnReason reason,
                    net.minecraft.world.entity.SpawnGroupData data) {
                return data;
            }
        }

        /** 不走构造器的运输矿车：`createChest` 后续调用全部变成空操作。 */
        private static final class InertMinecartChest
                extends net.minecraft.world.entity.vehicle.minecart.MinecartChest {
            private InertMinecartChest() {
                super(null, null);
            }

            @Override
            public void setInitialPos(double x, double y, double z) {
            }

            @Override
            public void setLootTable(net.minecraft.resources.ResourceKey<
                    net.minecraft.world.level.storage.loot.LootTable> table, long seed) {
            }
        }

        /**
         * 带结构模式里生成期实体（矿车、村民、悦灵、守卫者……）不需要真的进世界，
         * 但官方 `StructurePiece.createChest` 会调
         * `EntityTypes.CHEST_MINECART.create(level.getLevel(), ...)` 并接着
         * `random.nextLong()` 定战利品种子。照 `StructurePlaceOracle` 的做法：
         * `getLevel()` 返回一个不跑构造器的 ServerLevel，实体工厂返回不跑构造器的
         * 惰性实体，`addFreshEntity` 只回 true。方块与随机数序列完全按官方走。
         *
         * Generation-time entities need not enter a fake world, but vanilla's
         * `StructurePiece.createChest` calls `EntityTypes.CHEST_MINECART.create` and
         * then draws `nextLong()` for the loot seed. Mirror `StructurePlaceOracle`:
         * `getLevel()` returns a constructor-less ServerLevel, the entity factory
         * returns an inert minecart, and `addFreshEntity` just returns true.
         */
        private net.minecraft.server.level.ServerLevel installEntityStub(
                long seed, NoiseBasedChunkGenerator generator) {
            try {
                Object type = net.minecraft.world.entity.EntityTypes.CHEST_MINECART;
                setField(net.minecraft.world.entity.EntityType.class, type, "factory",
                        (net.minecraft.world.entity.EntityType.EntityFactory<
                                net.minecraft.world.entity.Entity>) (t, level) ->
                                allocate(InertMinecartChest.class));
                Object server = allocate(
                        net.minecraft.server.dedicated.DedicatedServer.class);
                // `CappedProcessor.finalizeProcessing` 要 `level.getLevel().getSeed()`：
                // 和 `JigsawOracle.fakeServerLevelWithSeed` 一样接上 worldOptions.seed。
                Object worldOptions = allocate(net.minecraft.world.level.levelgen.WorldOptions.class);
                setFieldInHierarchy(worldOptions, "seed", seed);
                Object genSettings = allocate(
                        net.minecraft.world.level.levelgen.WorldGenSettings.class);
                setFieldInHierarchy(genSettings, "options", worldOptions);
                setFieldInHierarchy(server, "worldGenSettings", genSettings);
                net.minecraft.world.level.storage.WorldData data =
                        (net.minecraft.world.level.storage.WorldData) Proxy.newProxyInstance(
                                ChunkgenOracle.class.getClassLoader(),
                                new Class<?>[] {net.minecraft.world.level.storage.WorldData.class},
                                (self, method, args) -> {
                                    if (method.getName().equals("enabledFeatures")) {
                                        return net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS;
                                    }
                                    throw new UnsupportedOperationException(
                                            "WorldData." + method.getName());
                                });
                setField(net.minecraft.server.MinecraftServer.class, server, "worldData", data);
                // `TemplateFeature.place`（沙漠水井的 `overlay` 子项）和
                // `FossilFeature.place` 第一步都是
                // `level.getLevel().getServer().getStructureTemplateManager()`；
                // 不跑构造器的服务端这个字段是 null，会在放置时抛 NPE。
                // `TemplateFeature.place` (the desert well's `overlay` child) and
                // `FossilFeature.place` both start with
                // `level.getLevel().getServer().getStructureTemplateManager()`, which is
                // null on a constructor-less server.
                setField(net.minecraft.server.MinecraftServer.class, server,
                        "structureTemplateManager", this.templates);
                Object levelData = Proxy.newProxyInstance(
                        ChunkgenOracle.class.getClassLoader(),
                        new Class<?>[] {net.minecraft.world.level.storage.ServerLevelData.class},
                        (self, method, args) -> {
                            if (method.getName().equals("getDifficulty")) {
                                return net.minecraft.world.Difficulty.NORMAL;
                            }
                            throw new UnsupportedOperationException(
                                    "LevelData." + method.getName());
                        });
                Object fakeLevel = allocate(net.minecraft.server.level.ServerLevel.class);
                setField(net.minecraft.server.level.ServerLevel.class, fakeLevel, "server", server);
                setField(net.minecraft.world.level.Level.class, fakeLevel, "levelData", levelData);
                // `PaleMossDecorator.lambda$place$1` 走
                // `getLevel().getChunkSource().getGenerator()` 拿 `minecraft:pale_moss_patch`
                // 的递归 Feature.place；不跑构造器的 ServerLevel 的 chunkSource 是 null。
                // 按字节码把这条链补成
                // ServerLevel.chunkSource → ServerChunkCache.chunkMap →
                // ChunkMap.worldGenContext → WorldGenContext.generator，
                // generator 就是本次对拍用的那个实例（`getGenDepth()` / `getSeaLevel()` 要给对）。
                // `chunkSource` / `worldGenContext` 是 final，普通反射写不进，用 Unsafe；
                // `WorldGenContext` 是 record，Unsafe 不肯给它的字段算偏移，
                // 直接调公开构造器（构造器只做赋值，不碰别的状态）。
                //
                // `PaleMossDecorator.lambda$place$1` reaches the recursive
                // `minecraft:pale_moss_patch` Feature.place through
                // `getLevel().getChunkSource().getGenerator()`. A constructor-less
                // ServerLevel has a null chunkSource; rebuild that chain with the
                // very generator used for this comparison.
                Object chunkSource = allocate(net.minecraft.server.level.ServerChunkCache.class);
                Object chunkMap = allocate(net.minecraft.server.level.ChunkMap.class);
                Object worldGenContext =
                        net.minecraft.world.level.chunk.status.WorldGenContext.class
                                .getConstructor(
                                        net.minecraft.server.level.ServerLevel.class,
                                        net.minecraft.world.level.chunk.ChunkGenerator.class,
                                        StructureTemplateManager.class,
                                        net.minecraft.server.level.ThreadedLevelLightEngine.class,
                                        java.util.concurrent.Executor.class,
                                        net.minecraft.world.level.chunk.LevelChunk.UnsavedListener.class)
                                .newInstance(fakeLevel, generator, this.templates, null, null, null);
                putFinal(chunkMap, net.minecraft.server.level.ChunkMap.class,
                        "worldGenContext", worldGenContext);
                putFinal(chunkSource, net.minecraft.server.level.ServerChunkCache.class,
                        "chunkMap", chunkMap);
                putFinal(fakeLevel, net.minecraft.server.level.ServerLevel.class,
                        "chunkSource", chunkSource);
                // 结构件在 FEATURES 阶段生成的实体都不进假世界，但官方仍要构造它们
                // （女巫小屋、府邸、海底神殿、前哨、村庄……）。工厂换成惰性实例，
                // 后面的 `snapTo` / `finalizeSpawn` / `addFreshEntity` 全部空操作。
                installInertMob(net.minecraft.world.entity.EntityTypes.WITCH, InertWitch.class);
                installInertMob(net.minecraft.world.entity.EntityTypes.CAT, InertCat.class);
                installInertMob(net.minecraft.world.entity.EntityTypes.ELDER_GUARDIAN,
                        InertElderGuardian.class);
                installInertMob(net.minecraft.world.entity.EntityTypes.EVOKER, InertEvoker.class);
                installInertMob(net.minecraft.world.entity.EntityTypes.VINDICATOR,
                        InertVindicator.class);
                installInertMob(net.minecraft.world.entity.EntityTypes.ALLAY, InertAllay.class);
                installInertMob(net.minecraft.world.entity.EntityTypes.DROWNED,
                        InertDrowned.class);
                return (net.minecraft.server.level.ServerLevel) fakeLevel;
            } catch (Exception error) {
                throw new IllegalStateException("cannot stub generation-time entities", error);
            }
        }

        private static final sun.misc.Unsafe UNSAFE = loadUnsafe();

        private static sun.misc.Unsafe loadUnsafe() {
            try {
                Field field = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
                field.setAccessible(true);
                return (sun.misc.Unsafe) field.get(null);
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
        }

        /** 跳过构造器分配实例，和 BlockOracle 一样。 Skip the constructor, as BlockOracle does. */
        @SuppressWarnings("unchecked")
        private static <T> T allocate(Class<T> type) {
            try {
                return (T) UNSAFE.allocateInstance(type);
            } catch (InstantiationException error) {
                throw new IllegalStateException(error);
            }
        }

        private static void setField(Class<?> owner, Object target, String name, Object value) {
            try {
                Field field = owner.getDeclaredField(name);
                field.setAccessible(true);
                field.set(target, value);
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
        }

        /** 写一个可能声明在父类上的 final 字段（世界种子链）。 */
        /** 写一个类自己声明的 final 引用字段（跳过构造器的实例）。 */
        private static void putFinal(Object target, Class<?> owner, String name, Object value) {
            try {
                Field field = owner.getDeclaredField(name);
                field.setAccessible(true);
                UNSAFE.putObject(target, UNSAFE.objectFieldOffset(field), value);
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
        }

        private static void setFieldInHierarchy(Object target, String name, Object value) {
            for (Class<?> owner = target.getClass(); owner != null; owner = owner.getSuperclass()) {
                try {
                    Field field = owner.getDeclaredField(name);
                    field.setAccessible(true);
                    field.set(target, value);
                    return;
                } catch (NoSuchFieldException ignored) {
                    // 继续往父类找。 / Keep walking up the hierarchy.
                } catch (IllegalAccessException error) {
                    throw new IllegalStateException(error);
                }
            }
            throw new IllegalStateException("no field " + name);
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

        /**
         * 输出这个区块里所有「真的实例化过」的方块实体：`saveWithFullMetadata` 的
         * 紧凑 SNBT。只有 `pendingBlockEntities` 里的 DUMMY nbt 不算——那是
         * `WorldGenRegion.setBlock` 给带方块实体的方块留的占位符，没有实体数据。
         *
         * Emit every instantiated block entity of the chunk as compact SNBT from
         * `saveWithFullMetadata`. The DUMMY nbt left in `pendingBlockEntities` by
         * `WorldGenRegion.setBlock` is a placeholder, not an entity, so it is skipped.
         */
        private void addBlockEntities(JsonObject line, ChunkAccess chunk) {
            JsonArray entities = new JsonArray();
            List<BlockPos> positions = new ArrayList<>(chunk.getBlockEntitiesPos());
            positions.sort(Comparator.comparingInt((BlockPos p) -> p.getY())
                    .thenComparingInt(p -> p.getX()).thenComparingInt(p -> p.getZ()));
            for (BlockPos pos : positions) {
                CompoundTag tag = chunk.getBlockEntityNbtForSaving(pos, registries);
                if (tag == null || "DUMMY".equals(tag.getStringOr("id", ""))) {
                    continue;
                }
                JsonArray entry = new JsonArray();
                entry.add(pos.getX());
                entry.add(pos.getY());
                entry.add(pos.getZ());
                entry.add(tag.toString());
                entities.add(entry);
            }
            line.add("block_entities", entities);
            this.checks += positions.size();
        }

        /**
         * 输出 ProtoChunk 里现有的每一张高度图：逐列的 getFirstAvailable（世界高度）
         * 和打包的 raw long[]（存档/发包用的位流）。
         *
         * Emit every heightmap present on the ProtoChunk: per-column getFirstAvailable
         * (world height) and the packed raw long[] (the on-disk/wire bitstream).
         */
        private void addHeightmaps(JsonObject line, ChunkAccess chunk) {
            JsonObject columns = new JsonObject();
            JsonObject words = new JsonObject();
            for (java.util.Map.Entry<Heightmap.Types, Heightmap> entry : chunk.getHeightmaps()) {
                Heightmap.Types type = entry.getKey();
                Heightmap map = entry.getValue();
                JsonArray perColumn = new JsonArray();
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++) {
                        perColumn.add(map.getFirstAvailable(x, z));
                        this.checks++;
                    }
                }
                columns.add(type.getSerializationKey(), perColumn);
                JsonArray raw = new JsonArray();
                for (long word : map.getRawData()) {
                    raw.add(word);
                    this.checks++;
                }
                words.add(type.getSerializationKey(), raw);
            }
            line.add("heightmaps", columns);
            line.add("heightmap_words", words);
        }

        private long[] seeds() {
            if (this.options.chunksFile() != null) {
                try {
                    List<Long> parsed = new ArrayList<>();
                    for (String line : Files.readAllLines(this.options.chunksFile(),
                            StandardCharsets.UTF_8)) {
                        if (!line.isBlank()) {
                            parsed.add(Long.parseLong(line.trim().split("\\s+")[0]));
                        }
                    }
                    // 保留文件里的顺序，去重后按种子分组。
                    return parsed.stream().distinct().mapToLong(Long::longValue).toArray();
                } catch (java.io.IOException error) {
                    throw new IllegalStateException("cannot read chunks file", error);
                }
            }
            if (this.options.forcedSeed() != null) {
                return new long[] {this.options.forcedSeed()};
            }
            long[] seeds = new long[this.options.seeds()];
            Random random = new Random(this.options.salt());
            for (int i = 0; i < seeds.length; i++) {
                seeds[i] = i < FIXED_SEEDS.length ? FIXED_SEEDS[i] : random.nextLong();
            }
            return seeds;
        }

        private int[][] chunks(long seed) {
            if (this.options.chunksFile() != null) {
                try {
                    List<int[]> parsed = new ArrayList<>();
                    for (String line : Files.readAllLines(this.options.chunksFile(),
                            StandardCharsets.UTF_8)) {
                        if (line.isBlank()) {
                            continue;
                        }
                        String[] parts = line.trim().split("\\s+");
                        if (Long.parseLong(parts[0]) == seed) {
                            parsed.add(new int[] {
                                Integer.parseInt(parts[1]), Integer.parseInt(parts[2])
                            });
                        }
                    }
                    return parsed.toArray(new int[0][]);
                } catch (java.io.IOException error) {
                    throw new IllegalStateException("cannot read chunks file", error);
                }
            }
            // `--chunk X Z --scan N`：以 (X,Z) 为中心的一连续网格，密集覆盖含水层
            // 格子边界。稀有分支（如岩浆层上边界 y=-54）只在连续洞穴区域里出现，
            // 稀疏的随机区块扫不到，所以扫描要用相邻区块而不是完全随机的坐标。
            // `--chunk X Z --scan N`: a contiguous grid centered on (X,Z) that densely
            // covers aquifer cell boundaries. Rare branches (e.g. the lava-layer top at
            // y=-54) only occur inside connected cave regions, which sparse random chunks
            // miss; scanning therefore needs adjacent chunks rather than purely random ones.
            if (this.options.forcedX() != null && this.options.scan() > 0) {
                int side = 1;
                while (side * side < this.options.scan()) {
                    side += 2;
                }
                int r = side / 2;
                int[][] grid = new int[side * side][2];
                int k = 0;
                for (int dz = -r; dz <= r; dz++) {
                    for (int dx = -r; dx <= r; dx++) {
                        grid[k][0] = this.options.forcedX() + dx;
                        grid[k][1] = this.options.forcedZ() + dz;
                        k++;
                    }
                }
                return grid;
            }
            // `--chunk` 覆盖成单个区块，用 FIXED_SEEDS 里的种子逐个复现。
            // `--chunk` overrides to a single chunk, reproduced under each FIXED_SEEDS entry.
            if (this.options.forcedX() != null) {
                return new int[][] {{this.options.forcedX(), this.options.forcedZ()}};
            }
            boolean scan = this.options.scan() > 0;
            int count = scan ? this.options.scan() : this.options.chunks();
            int[][] result = new int[count][2];
            // 扫描用独立的播种，覆盖近处网格（容易撞上含水层格子边界）、
            // 中距离、远距离和负坐标；和默认坐标生成互不干扰。
            // Scanning uses its own seed and spans a near grid (likely to hit aquifer cell
            // boundaries), mid range, far range and negative coordinates, independent of
            // the default coordinate generator.
            long randomSeed = seed ^ 0x51A7_5EEDL ^ this.options.salt();
            Random random = new Random(scan ? randomSeed ^ 0x5CA4_5EEDL : randomSeed);
            for (int i = 0; i < count; i++) {
                if (!scan && i < FIXED_CHUNKS.length) {
                    result[i] = FIXED_CHUNKS[i].clone();
                } else {
                    int bound = switch (i % 4) {
                        case 0 -> 48;
                        case 1 -> 512;
                        case 2 -> 16384;
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

/**
 * `LevelReader.findBlocksIn` 在假世界里的实现：直接按区块扫描一个闭区间盒子。
 *
 * <p>官方 `BoxBlockMatcher` 走 `BlockScanUtils.findBlocks`，按 section 的 x、z、y 顺序
 * 扫描，section 内是 y、z、x 顺序；这里保留同样的顺序，因为 `atMostMatched` 会在数到
 * 第 n 个就停下，顺序影响命中判定。`SculkBlock.canPlaceGrowth` 只用 `atMostMatched(2)`
 * 数 `sculk_growth_inhibitors`，三个以内的顺序无关；仍照官方顺序写，避免以后出错。
 *
 * The fake world's `LevelReader.findBlocksIn`: scan a closed box directly. The official
 * `BoxBlockMatcher` goes through `BlockScanUtils.findBlocks`, which walks sections in
 * x, z, y order and blocks inside a section in y, z, x order; the same order is kept
 * because `atMostMatched` stops at the n-th hit. `SculkBlock.canPlaceGrowth` only counts
 * `sculk_growth_inhibitors` up to two, so order does not change its answer, but keeping
 * the official order avoids surprises later.
 */
final class ChunkgenBlockMatcher extends net.minecraft.world.level.blockscan.BlockMatcher {
    private final FeatureLevelHandler handler;
    private final BlockPos from;
    private final BlockPos to;

    private ChunkgenBlockMatcher(FeatureLevelHandler handler, BlockPos from, BlockPos to) {
        super(null);
        this.handler = handler;
        this.from = from;
        this.to = to;
    }

    static ChunkgenBlockMatcher of(FeatureLevelHandler handler, BlockPos from, BlockPos to) {
        return new ChunkgenBlockMatcher(handler, from, to);
    }

    @Override
    public boolean atLeastMatched(int n) {
        return matchCount(n + 1) >= n;
    }

    private int matchCount(int limit) {
        int count = 0;
        int minX = Math.min(this.from.getX(), this.to.getX());
        int minY = Math.min(this.from.getY(), this.to.getY());
        int minZ = Math.min(this.from.getZ(), this.to.getZ());
        int maxX = Math.max(this.from.getX(), this.to.getX());
        int maxY = Math.max(this.from.getY(), this.to.getY());
        int maxZ = Math.max(this.from.getZ(), this.to.getZ());
        for (int x = minX; x <= maxX; x++) {
            for (int z = minZ; z <= maxZ; z++) {
                for (int y = minY; y <= maxY; y++) {
                    BlockPos pos = new BlockPos(x, y, z);
                    if (this.statePredicate.test(this.handler.getBlockStatePublic(pos))) {
                        count++;
                        if (count >= limit) {
                            return count;
                        }
                    }
                }
            }
        }
        return count;
    }

    @Override
    protected boolean anyMatched(java.util.function.Predicate<BlockState> predicate) {
        return matchCount(1) >= 1;
    }

    @Override
    public void forEach(java.util.function.BiConsumer<BlockPos, BlockState> consumer) {
        throw new UnsupportedOperationException("forEach is not used by the chunkgen oracle");
    }

    @Override
    public boolean forEachUntil(net.minecraft.world.level.blockscan.BlockStateConsumer consumer) {
        throw new UnsupportedOperationException("forEachUntil is not used by the chunkgen oracle");
    }
}
