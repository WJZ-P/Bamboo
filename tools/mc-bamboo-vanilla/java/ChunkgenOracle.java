import com.google.gson.JsonArray;
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
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.QuartPos;
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
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;

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

    private record Options(int seeds, int chunks, long salt, int through) {
        static Options parse(String[] args) {
            int seeds = 3;
            int chunks = 12;
            long salt = 1;
            int through = 3;
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
                            default -> throw new IllegalArgumentException("unknown stage " + value);
                        };
                    }
                    case "--biomes-only" -> through = 0;
                    default -> throw new IllegalArgumentException("unknown option " + args[i]);
                }
            }
            return new Options(seeds, chunks, salt, through);
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
            List<BlockState> palette = new ArrayList<>();
            java.util.Map<BlockState, Integer> ids = new java.util.HashMap<>();
            JsonArray blocks = new JsonArray();
            int minY = chunk.getMinY();
            for (int y = 0; y < chunk.getHeight(); y++) {
                for (int x = 0; x < 16; x++) {
                    for (int z = 0; z < 16; z++) {
                        BlockState state = chunk.getSection(chunk.getSectionIndex(minY + y)).getBlockState(x, y & 15, z);
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
            JsonObject line = new JsonObject();
            line.addProperty("type", stage);
            line.addProperty("seed", seed);
            line.addProperty("x", pos.getMinBlockX() >> 4);
            line.addProperty("z", pos.getMinBlockZ() >> 4);
            line.add("palette", names);
            line.add("blocks", blocks);
            write(line);
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
