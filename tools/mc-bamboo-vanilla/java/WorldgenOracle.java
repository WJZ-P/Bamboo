import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Random;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.biome.TheEndBiomeSource;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseRouter;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.densityfunction.DensityBuffer;
import net.minecraft.world.level.levelgen.densityfunction.DensityFunction;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.synth.Noise;
import net.minecraft.world.level.levelgen.synth.NormalNoise;

/**
 * 用官方代码给 Bamboo 的世界生成数学出题和给答案。
 *
 * <p>不启动服务端，也不生成任何区块：只按官方的加载流程把数据包注册表读进来，
 * 再对一批种子和一批坐标问官方要数值。
 *
 * <p>每个种子下问三类东西：
 *
 * <ul>
 *   <li>`worldgen/noise` 里每一条噪声：`Noise.get(x, y, z)`（单点那条路径）和
 *       `Noise.addToVolume` 在 1×1×1 体积上的结果（成批那条路径）。两条路径的浮点运算
 *       顺序不同，最后一位会差，所以都要比。</li>
 *   <li>`worldgen/density_function` 里每一条，加上七套维度设置的 `NoiseRouter` 的八个输出：
 *       `RandomState.sampleBlockValueUncached`（单点）和 1×1×1 的 `sampleVolume`（成批）。</li>
 *   <li>群系：主世界和下界的多噪声群系源，加上末地的群系源，按 quart 坐标问
 *       `BiomeResolver.getNoiseBiome`。</li>
 * </ul>
 *
 * <p>浮点一律按 `Float.floatToRawIntBits` 输出，比较是逐位的。
 *
 * <p>输出一行一个 JSON：header、若干 noise / function / router / biome，最后 end。
 */
public final class WorldgenOracle {
    private static final int OUTPUT_BUFFER = 4 << 20;
    /** 七套官方维度设置，覆盖两种随机算法和全部注册表密度函数。 */
    private static final List<String> SETTINGS = List.of(
            "overworld", "large_biomes", "amplified", "nether", "end", "caves", "floating_islands");
    /** 噪声和注册表密度函数只跟（种子，随机算法）有关，用这两套设置各来一遍。 */
    private static final List<String> RANDOM_STATES = List.of("overworld", "nether");
    /** 噪声的两个坐标系数，取能精确表示的值，两条路径用同样的数。 */
    private static final double NOISE_XZ_SCALE = 0.3125;
    private static final double NOISE_Y_SCALE = 1.25;
    /** 一定要试的几个种子：0、负数、很大的正数和很大的负数。 */
    private static final long[] FIXED_SEEDS = {
        0L, -1L, 1L, 1234567890123456789L, -8765432109876543210L, 0x7FFF_FFFF_FFFF_FFFFL
    };

    private WorldgenOracle() {
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
            new Oracle(registries, options, out).run();
        }
    }

    /** 和 WorldLoader.load 的前半段相同。 */
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

    private record Options(int seeds, long salt, int points, int biomePoints) {
        static Options parse(String[] args) {
            int seeds = 3;
            long salt = 1;
            int points = 6;
            int biomePoints = 32;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--seeds" -> seeds = Integer.parseInt(args[++i]);
                    case "--salt" -> salt = Long.parseLong(args[++i]);
                    case "--points" -> points = Integer.parseInt(args[++i]);
                    case "--biome-points" -> biomePoints = Integer.parseInt(args[++i]);
                    default -> throw new IllegalArgumentException("unknown option " + args[i]);
                }
            }
            return new Options(seeds, salt, points, biomePoints);
        }
    }

    private static final class Oracle {
        private final RegistryAccess registries;
        private final Registry<NormalNoise> noises;
        private final Registry<DensityFunction> functions;
        private final Registry<NoiseGeneratorSettings> settings;
        private final Options options;
        private final Writer out;
        private final DensityBuffer single = DensityBuffer.createUnpooled(1);
        private int checks;

        Oracle(RegistryAccess registries, Options options, Writer out) {
            this.registries = registries;
            this.noises = registries.lookupOrThrow(Registries.NOISE);
            this.functions = registries.lookupOrThrow(Registries.DENSITY_FUNCTION);
            this.settings = registries.lookupOrThrow(Registries.NOISE_SETTINGS);
            this.options = options;
            this.out = out;
        }

        void run() throws Exception {
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            write(header);

            for (long seed : seeds()) {
                for (String name : RANDOM_STATES) {
                    RandomState state = randomState(name, seed);
                    writeNoises(state, name, seed);
                    writeFunctions(state, name, seed);
                }
                for (String name : SETTINGS) {
                    writeRouter(randomState(name, seed), name, seed);
                }
                writeBiomes(seed);
            }

            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            end.addProperty("checks", this.checks);
            write(end);
            this.out.flush();
        }

        private long[] seeds() {
            long[] seeds = new long[this.options.seeds()];
            Random random = new Random(this.options.salt());
            for (int i = 0; i < seeds.length; i++) {
                seeds[i] = i < FIXED_SEEDS.length ? FIXED_SEEDS[i] : random.nextLong();
            }
            return seeds;
        }

        private RandomState randomState(String name, long seed) {
            NoiseGeneratorSettings generator = this.settings.getValue(Identifier.withDefaultNamespace(name));
            if (generator == null) {
                throw new IllegalStateException("unknown noise settings " + name);
            }
            return RandomState.create(this.noises, seed, generator);
        }

        /** 坐标：大部分在普通范围里，再掺几个很远的，逼出 GradientNoise.wrap 的折返。 */
        private int[][] points(long seed, int count, boolean quart) {
            Random random = new Random(seed ^ 0x5A5A_5A5AL ^ (quart ? 1 : 0));
            int[][] points = new int[count][3];
            for (int i = 0; i < count; i++) {
                int bound = switch (i % 4) {
                    case 0 -> 512;
                    case 1 -> 100_000;
                    case 2 -> 20_000_000;
                    default -> 4_000_000;
                };
                int x = random.nextInt(-bound, bound);
                int z = random.nextInt(-bound, bound);
                int y = switch (i % 3) {
                    case 0 -> random.nextInt(-64, 320);
                    case 1 -> random.nextInt(-2048, 2048);
                    default -> random.nextInt(-100_000, 100_000);
                };
                points[i][0] = quart ? x >> 2 : x;
                points[i][1] = quart ? y >> 2 : y;
                points[i][2] = quart ? z >> 2 : z;
            }
            return points;
        }

        private void writeNoises(RandomState state, String settingsName, long seed) throws Exception {
            for (Identifier name : this.noises.keySet().stream().sorted().toList()) {
                ResourceKey<NormalNoise> key = ResourceKey.create(Registries.NOISE, name);
                Noise noise = state.getOrCreateNoise(key);
                JsonArray samples = new JsonArray();
                for (int[] point : points(seed ^ name.hashCode(), this.options.points(), false)) {
                    float direct = noise.get(
                            point[0] * NOISE_XZ_SCALE, point[1] * NOISE_Y_SCALE, point[2] * NOISE_XZ_SCALE);
                    this.single.fill(0.0F);
                    noise.addToVolume(
                            this.single,
                            new DensityVolume(1, 1, 1, point[0], point[1], point[2], 1, 1, 1),
                            NOISE_XZ_SCALE,
                            NOISE_Y_SCALE,
                            1.0F);
                    samples.add(sample(point, direct, this.single.get(0)));
                    this.checks += 2;
                }
                JsonObject line = new JsonObject();
                line.addProperty("type", "noise");
                line.addProperty("settings", "minecraft:" + settingsName);
                line.addProperty("seed", seed);
                line.addProperty("id", name.toString());
                line.add("samples", samples);
                write(line);
            }
        }

        private void writeFunctions(RandomState state, String settingsName, long seed) throws Exception {
            for (Identifier name : this.functions.keySet().stream().sorted().toList()) {
                DensityFunction function = this.functions.getValue(name);
                JsonObject line = new JsonObject();
                line.addProperty("type", "function");
                line.addProperty("settings", "minecraft:" + settingsName);
                line.addProperty("seed", seed);
                line.addProperty("id", name.toString());
                line.add("samples", functionSamples(state, function, seed ^ name.hashCode()));
                write(line);
            }
        }

        private void writeRouter(RandomState state, String settingsName, long seed) throws Exception {
            NoiseGeneratorSettings generator = this.settings.getValue(Identifier.withDefaultNamespace(settingsName));
            NoiseRouter router = generator.noiseRouter();
            List<String> names = List.of(
                    "temperature", "vegetation", "continents", "erosion",
                    "depth", "ridges", "chunk_surface_level", "final_density");
            List<DensityFunction> outputs = List.of(
                    router.temperature(), router.vegetation(), router.continents(), router.erosion(),
                    router.depth(), router.ridges(), router.chunkSurfaceLevel(), router.finalDensity());
            for (int i = 0; i < names.size(); i++) {
                JsonObject line = new JsonObject();
                line.addProperty("type", "router");
                line.addProperty("settings", "minecraft:" + settingsName);
                line.addProperty("seed", seed);
                line.addProperty("output", names.get(i));
                line.add("samples", functionSamples(state, outputs.get(i), seed ^ names.get(i).hashCode()));
                write(line);
            }
        }

        /** 一个密度函数在一批坐标上的两条路径。 */
        private JsonArray functionSamples(RandomState state, DensityFunction function, long pointSeed) {
            JsonArray samples = new JsonArray();
            for (int[] point : points(pointSeed, this.options.points(), false)) {
                float direct = state.sampleBlockValueUncached(function, point[0], point[1], point[2]);
                this.single.fill(0.0F);
                state.getSampler(function)
                        .sampleVolume(
                                SamplerContext.EMPTY_UNCACHED,
                                this.single,
                                new DensityVolume(1, 1, 1, point[0], point[1], point[2], 1, 1, 1));
                samples.add(sample(point, direct, this.single.get(0)));
                this.checks += 2;
            }
            return samples;
        }

        /**
         * 群系：主世界和下界走多噪声群系源，末地走 TheEndBiomeSource。
         *
         * <p>官方的 R 树查找带着「上一次的结果」当初始候选，平局时上一次赢，所以这里的顺序有意义：
         * Bamboo 要按同样的顺序重放。一个预设的树在整个进程里是同一棵，状态跨种子延续，两边都一样。
         */
        private void writeBiomes(long seed) throws Exception {
            HolderLookup.RegistryLookup<Biome> biomes = this.registries.lookupOrThrow(Registries.BIOME);
            Registry<MultiNoiseBiomeSourceParameterList> presets =
                    this.registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST);

            writeBiomeBlock(
                    "overworld",
                    "minecraft:overworld",
                    seed,
                    MultiNoiseBiomeSource.createFromPreset(presets.getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)));
            writeBiomeBlock(
                    "nether",
                    "minecraft:nether",
                    seed,
                    MultiNoiseBiomeSource.createFromPreset(presets.getOrThrow(MultiNoiseBiomeSourceParameterLists.NETHER)));
            writeBiomeBlock("end", "the_end", seed, TheEndBiomeSource.create(biomes));
        }

        private void writeBiomeBlock(
                String settingsName, String preset, long seed, net.minecraft.world.level.biome.BiomeSource source)
                throws Exception {
            RandomState state = randomState(settingsName, seed);
            Climate.Sampler sampler = state.createClimateSampler(SamplerContext.EMPTY_UNCACHED);
            BiomeResolver resolver = source.createResolver(sampler);
            JsonArray samples = new JsonArray();
            for (int[] point : points(seed ^ preset.hashCode(), this.options.biomePoints(), true)) {
                Holder<Biome> biome = resolver.getNoiseBiome(point[0], point[1], point[2]);
                JsonArray entry = new JsonArray();
                entry.add(point[0]);
                entry.add(point[1]);
                entry.add(point[2]);
                entry.add(biome.unwrapKey().orElseThrow().identifier().toString());
                samples.add(entry);
                this.checks++;
            }
            JsonObject line = new JsonObject();
            line.addProperty("type", "biome");
            line.addProperty("settings", "minecraft:" + settingsName);
            line.addProperty("preset", preset);
            line.addProperty("seed", seed);
            line.add("samples", samples);
            write(line);
        }

        private static JsonArray sample(int[] point, float direct, float volume) {
            JsonArray entry = new JsonArray();
            entry.add(point[0]);
            entry.add(point[1]);
            entry.add(point[2]);
            entry.add(Float.floatToRawIntBits(direct));
            entry.add(Float.floatToRawIntBits(volume));
            return entry;
        }

        private void write(JsonObject line) throws Exception {
            this.out.write(line.toString());
            this.out.write('\n');
        }
    }
}
