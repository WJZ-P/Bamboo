// 官方 `MinecraftServer.setInitialSpawn` 的出生点对拍，不启动服务端、不读存档、
// 不接受 EULA。对每个种子用官方 `NoiseBasedChunkGenerator.getOrigin` 算出生区块，
// 再在 11×11 区块螺旋里把每个区块生成到 `terrain`（CARVERS）并 prime 高度图，
// 用官方 `Heightmap` / `BlockState.getFluidState` / `Block.isFaceFull` 复刻
// `PlayerSpawnFinder.getSpawnPosInChunk`，输出最终出生点。
//
// 只加载官方类、在 JVM 内造 ProtoChunk，不构造 ServerLevel；`getCollisionShape`
// 用 EmptyBlockGetter（CARVERS 方块都是完整方块/流体/空气，形状不依赖 level）。
// 和 gen-world 一样只比到 CARVERS，地物/结构/光照两边都排除。
//
// Spawn oracle for vanilla `MinecraftServer.setInitialSpawn`: no server, no save,
// no EULA. For each seed compute the spawn chunk with official
// `NoiseBasedChunkGenerator.getOrigin`, then in the 11x11 chunk spiral generate
// each chunk to `terrain` (CARVERS), prime heightmaps, and replicate
// `PlayerSpawnFinder.getSpawnPosInChunk` using official `Heightmap`,
// `BlockState.getFluidState` and `Block.isFaceFull`. Emit the final spawn.
// Only official classes in-JVM ProtoChunks, no ServerLevel; `getCollisionShape`
// uses EmptyBlockGetter (CARVERS blocks are full cubes / fluids / air, so their
// shapes do not depend on the level). Like gen-world, stops at CARVERS.

import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.function.Supplier;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
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
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.block.Block;
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
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;
import net.minecraft.world.level.levelgen.synth.NormalNoise;

public final class SpawnOracle {
    private static final int OUTPUT_BUFFER = 4 << 20;
    private static final long[] FIXED_SEEDS = {0L, -1L, 1L, 12345L, -999L, 999L, 1234567890123456789L,
            -8765432109876543210L};

    private static RegistryAccess.Frozen registries;
    private static HolderLookup.RegistryLookup<NormalNoise> noiseLookup;
    private static NoiseBasedChunkGenerator generator;
    private static NoiseGeneratorSettings settings;
    private static Method doFill;
    private static Method buildSurface;
    private static Method generateCarvers;
    private static Field globalFluidPicker;
    private static Set<Holder<Biome>> possibleBiomes;
    private static MaterialRule materialRule;
    private static String activeSettingsId;

    private SpawnOracle() {
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
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        int seeds = 8;
        long salt = 1;
        String settingsId = "overworld";
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--seeds" -> seeds = Integer.parseInt(args[++i]);
                case "--salt" -> salt = Long.parseLong(args[++i]);
                case "--settings" -> settingsId = args[++i];
                default -> throw new IllegalArgumentException("unknown option " + args[i]);
            }
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                     new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
             Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            RegistryAccess.Frozen regs = loadRegistries(resources).compositeAccess();
            setup(regs, settingsId);
            for (long seed : seedList(seeds, salt)) {
                JsonObject line = spawnForSeed(seed);
                out.write(line.toString());
                out.write('\n');
            }
            out.flush();
        }
    }

    /** 和 WorldgenOracle / ChunkgenOracle 一样：按官方 WorldLoader 的前半段读数据包注册表。 */
    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(
            CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool()).join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }

    private static void setup(RegistryAccess.Frozen regs, String settingsId) throws Exception {
        registries = regs;
        activeSettingsId = settingsId;
        Registry<NoiseGeneratorSettings> settingsReg = regs.lookupOrThrow(Registries.NOISE_SETTINGS);
        noiseLookup = regs.lookupOrThrow(Registries.NOISE);
        settings = settingsReg.getValue(Identifier.withDefaultNamespace(settingsId));
        if (settings == null) {
            throw new IllegalStateException("missing minecraft:" + settingsId + " noise settings");
        }
        generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        regs.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        doFill = NoiseBasedChunkGenerator.class.getDeclaredMethod("doFill", NoiseChunk.class, ChunkAccess.class);
        doFill.setAccessible(true);
        buildSurface = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                "buildSurface", ChunkAccess.class, NoiseChunk.class, RandomState.class,
                BiomeManager.class, Set.class, MaterialRule.class);
        buildSurface.setAccessible(true);
        generateCarvers = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                "generateCarvers", ChunkAccess.class, Blender.class, NoiseChunk.class,
                RandomState.class, BiomeManager.class,
                net.minecraft.server.level.WorldGenRegion.class, MaterialRule.class);
        generateCarvers.setAccessible(true);
        globalFluidPicker = NoiseBasedChunkGenerator.class.getDeclaredField("globalFluidPicker");
        globalFluidPicker.setAccessible(true);
        possibleBiomes = new HashSet<>();
        for (Biome biome : regs.lookupOrThrow(Registries.BIOME)) {
            possibleBiomes.add(regs.lookupOrThrow(Registries.BIOME).wrapAsHolder(biome));
        }
        materialRule = settings.materialRule().value();
    }

    private static JsonObject spawnForSeed(long seed) throws Exception {
        RandomState state = RandomState.create(noiseLookup, seed, settings);
        BiomeResolver resolver = ((BiomeSource) generator.getBiomeSource()).createUncachedResolver(state);
        BiomeManager biomeManager = new BiomeManager(resolver, BiomeManager.obfuscateSeed(seed));

        // `MinecraftServer.setInitialSpawn`：`getOrigin` 给出生区块，暂定点 `(chunk*16+8, 64, +8)`。
        // `setInitialSpawn`: `getOrigin` gives the spawn chunk; provisional is `(chunk*16+8, 64, +8)`.
        ChunkPos origin = generator.getOrigin(state);
        BlockPos provisional = new BlockPos(origin.x() * 16 + 8, 64, origin.z() * 16 + 8);
        BlockPos spawn = null;

        int xChunkOffset = 0;
        int zChunkOffset = 0;
        int dXChunk = 0;
        int dZChunk = -1;
        for (int i = 0; i < MthSquare(11); i++) {
            if (xChunkOffset >= -5 && xChunkOffset <= 5 && zChunkOffset >= -5 && zChunkOffset <= 5) {
                ChunkPos chunkPos = new ChunkPos(origin.x() + xChunkOffset, origin.z() + zChunkOffset);
                ProtoChunk proto = generateTerrain(seed, chunkPos, state, biomeManager);
                BlockPos tested = getSpawnPosInChunk(proto, chunkPos);
                if (tested != null) {
                    spawn = tested;
                    break;
                }
            }
            if (xChunkOffset == zChunkOffset
                    || (xChunkOffset < 0 && xChunkOffset == -zChunkOffset)
                    || (xChunkOffset > 0 && xChunkOffset == 1 - zChunkOffset)) {
                int olddx = dXChunk;
                dXChunk = -dZChunk;
                dZChunk = olddx;
            }
            xChunkOffset += dXChunk;
            zChunkOffset += dZChunk;
        }
        if (spawn == null) {
            spawn = provisional;
        }

        JsonObject line = new JsonObject();
        line.addProperty("type", "spawn");
        line.addProperty("seed", seed);
        line.addProperty("settings", activeSettingsId);
        long[] o = {origin.x(), origin.z()};
        line.add("origin", toArray(o));
        long[] s = {spawn.getX(), spawn.getY(), spawn.getZ()};
        line.add("spawn", toArray(s));
        return line;
    }

    private static com.google.gson.JsonArray toArray(long[] values) {
        com.google.gson.JsonArray array = new com.google.gson.JsonArray();
        for (long v : values) {
            array.add(v);
        }
        return array;
    }

    private static int MthSquare(int x) {
        return x * x;
    }

    /** 把中心区块生成到 CARVERS 并 prime 高度图，和 `ChunkgenOracle.generateTerrain` 一致。 */
    private static ProtoChunk generateTerrain(long seed, ChunkPos pos, RandomState state,
            BiomeManager biomeManager) throws Exception {
        ProtoChunk proto = newChunk(pos);
        generator.createBiomes(state, Blender.empty(), null, proto).join();
        proto.setPersistedStatus(ChunkStatus.BIOMES);
        DensityVolume volume = new DensityVolume(
                16, settings.noiseSettings().height(), 16,
                pos.getMinBlockX(), settings.noiseSettings().minY(), pos.getMinBlockZ());
        @SuppressWarnings("unchecked")
        Supplier<Aquifer.FluidPicker> picker =
                (Supplier<Aquifer.FluidPicker>) globalFluidPicker.get(generator);
        try (NoiseChunk noiseChunk = new NoiseChunk(
                state, Beardifier.EMPTY, settings, picker.get(), Blender.empty(), volume)) {
            doFill.invoke(generator, noiseChunk, proto);
            buildSurface.invoke(generator, proto, noiseChunk, state, biomeManager, possibleBiomes, materialRule);
            generateCarvers.invoke(generator, proto, Blender.empty(), noiseChunk, state,
                    biomeManager, null, materialRule);
        }
        proto.setPersistedStatus(ChunkStatus.TERRAIN);
        Heightmap.primeHeightmaps(proto, ChunkStatus.FINAL_HEIGHTMAPS);
        return proto;
    }

    private static ProtoChunk newChunk(ChunkPos pos) {
        LevelHeightAccessor height = LevelHeightAccessor.create(
                settings.noiseSettings().minY(), settings.noiseSettings().height());
        return new ProtoChunk(pos, UpgradeData.EMPTY, height,
                PalettedContainerFactory.create(registries), null);
    }

    /** `PlayerSpawnFinder.getSpawnPosInChunk`：x 外、z 内递增找第一个能站的列。 */
    private static BlockPos getSpawnPosInChunk(ProtoChunk chunk, ChunkPos pos) {
        for (int x = pos.getMinBlockX(); x <= pos.getMaxBlockX(); x++) {
            for (int z = pos.getMinBlockZ(); z <= pos.getMaxBlockZ(); z++) {
                BlockPos valid = getLevelRespawnPos(chunk, x, z);
                if (valid != null) {
                    return valid;
                }
            }
        }
        return null;
    }

    /** `PlayerSpawnFinder.getLevelRespawnPos`，用官方高度图和碰撞形状判定。 */
    private static BlockPos getLevelRespawnPos(ChunkAccess chunk, int x, int z) {
        int minY = chunk.getMinY();
        int topY = chunk.getHeight(Heightmap.Types.MOTION_BLOCKING, x & 15, z & 15);
        if (topY < minY) {
            return null;
        }
        int surface = chunk.getHeight(Heightmap.Types.WORLD_SURFACE, x & 15, z & 15);
        if (surface <= topY
                && surface > chunk.getHeight(Heightmap.Types.OCEAN_FLOOR, x & 15, z & 15)) {
            return null;
        }
        BlockPos.MutableBlockPos pos = new BlockPos.MutableBlockPos();
        for (int y = topY + 1; y >= minY; y--) {
            pos.set(x, y, z);
            BlockState state = chunk.getBlockState(pos);
            if (!state.getFluidState().isEmpty()) {
                break;
            }
            if (Block.isFaceFull(state.getCollisionShape(EmptyBlockGetter.INSTANCE, pos), Direction.UP)) {
                return pos.above().immutable();
            }
        }
        return null;
    }

    private static long[] seedList(int count, long salt) {
        long[] seeds = new long[count];
        Random random = new Random(salt);
        for (int i = 0; i < count; i++) {
            seeds[i] = i < FIXED_SEEDS.length ? FIXED_SEEDS[i] : random.nextLong();
        }
        return seeds;
    }
}
