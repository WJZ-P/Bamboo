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
import java.util.Set;
import java.util.function.Supplier;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
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
import net.minecraft.world.level.levelgen.densityfunction.DensityBuffer;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.TerrainAdjustment;
import net.minecraft.world.level.levelgen.structure.pools.JigsawJunction;
import net.minecraft.world.level.levelgen.structure.pools.StructureTemplatePool;

/**
 * Beardifier 对拍：用官方 Beardifier 的 @VisibleForTesting 构造器，从一组覆盖全部
 * TerrainAdjustment 的代表性结构件包围盒 / jigsaw 连接点造真实 Beardifier，
 * 在 NoiseChunk 里连进 doFill / buildSurface / generateCarvers 出题。
 *
 * <p>这些盒是按真实结构（村庄 / 古城 / 试炼密室 / 前哨站等）的尺度与地形调整类型
 * 构造的输入，不是 {@code Structure.generate} 跑出来的结构件——jigsaw 的模板导出和
 * 结构件生成归其它里程碑。Beardifier 的输入本来就只是包围盒 + 调整类型 + 连接点，
 * 所以这足以把它逐位对拍清楚，等结构件接上后把 {@code forStructuresInChunk} 的
 * 产物填进同一套输入即可。
 *
 * Dumps each case's Rigid list, junctions, affectedBox, the full sampleValue grid
 * (raw float bits), the full sampleVolume buffer (raw float bits), the BEARD_KERNEL
 * (raw float bits), and NOISE / SURFACE / CARVERS blocks. The boxes are representative
 * structure footprints (all TerrainAdjustment kinds), not output of
 * {@code Structure.generate}, which needs the jigsaw/template work of other milestones;
 * the Beardifier input is only boxes + adjustment + junctions, so this fully validates it.
 * Bamboo rebuilds the Beardifier from the exported inputs and recomputes everything.
 */
public class BeardifierOracle {
    private static final int OUTPUT_BUFFER = 8 << 20;

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
        int through = 3;
        for (int i = 0; i < args.length; i++) {
            if (args[i].equals("--through")) {
                String value = args[++i];
                through = switch (value) {
                    case "noise" -> 1;
                    case "surface" -> 2;
                    case "carvers" -> 3;
                    default -> throw new IllegalArgumentException("unknown stage " + value);
                };
            } else {
                throw new IllegalArgumentException("unknown option " + args[i]);
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
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            new Oracle(registries, through, out).run();
        }
    }

    /**
     * 和 ChunkgenOracle 一样：按官方 WorldLoader 的前半段读数据包注册表。
     *
     * <p>Same as ChunkgenOracle: load datapack registries via the first half of vanilla WorldLoader.
     */
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
     * 一个对拍用例：真实 Beardifier 的输入就是这些 Rigid + Junction。
     *
     * <p>One oracle case: the Rigid + Junction list is exactly the input of a real Beardifier.
     */
    private static final class Case {
        final long seed;
        final int cx;
        final int cz;
        final List<Beardifier.Rigid> rigids = new ArrayList<>();
        final List<JigsawJunction> junctions = new ArrayList<>();

        Case(long seed, int cx, int cz) {
            this.seed = seed;
            this.cx = cx;
            this.cz = cz;
        }

        int bx() { return cx * 16; }
        int bz() { return cz * 16; }

        Case rigid(int x0, int y0, int z0, int x1, int y1, int z1,
                TerrainAdjustment adj, int gld) {
            BoundingBox box = new BoundingBox(
                    bx() + x0, y0, bz() + z0, bx() + x1, y1, bz() + z1);
            rigids.add(new Beardifier.Rigid(box, adj, gld));
            return this;
        }

        Case junction(int x, int gy, int z) {
            junctions.add(new JigsawJunction(
                    bx() + x, gy, bz() + z, 0, StructureTemplatePool.Projection.RIGID));
            return this;
        }
    }

    /**
     * 一组覆盖五个 TerrainAdjustment、junction、空、边界外扩的用例。
     *
     * <p>Cases covering all five TerrainAdjustment kinds, junctions, empty and boundary boxes.
     */
    private static List<Case> cases() {
        long[] seeds = {0L, 1L, -1L, 42L, 1234567890123456789L, -8765432109876543210L};
        int[][] chunks = {
            {0, 0}, {1, 1}, {-1, -1}, {34, 34}, {-34, 50},
            {100, -100}, {1000, 1000}, {-1000, 1000}, {12, -7}, {250, 250},
        };
        List<Case> list = new ArrayList<>();
        Case c = new Case(seeds[0], chunks[0][0], chunks[0][1]);
        // case 0：EMPTY，没有结构影响密度。
        // case 0: EMPTY, no structure affects density.
        list.add(c);
        c = new Case(seeds[1], chunks[1][0], chunks[1][1]);
        c.rigid(2, 64, 2, 13, 72, 13, TerrainAdjustment.BEARD_THIN, 0);
        // case 1：一块村庄式的 beard_thin。
        // case 1: one village-like beard_thin piece.
        list.add(c);
        c = new Case(seeds[2], chunks[2][0], chunks[2][1]);
        c.rigid(0, -52, 0, 24, -40, 24, TerrainAdjustment.BURY, -1);
        // case 2：深埋古城式的 bury。
        // case 2: a deep ancient-city-like bury piece.
        list.add(c);
        c = new Case(seeds[3], chunks[3][0], chunks[3][1]);
        c.rigid(-8, 60, -8, 20, 78, 20, TerrainAdjustment.BEARD_BOX, 2);
        // case 3：beard_box 跨出区块 XZ。
        // case 3: beard_box extending beyond the chunk XZ bounds.
        list.add(c);
        c = new Case(seeds[4], chunks[4][0], chunks[4][1]);
        c.rigid(4, -40, 4, 19, -20, 19, TerrainAdjustment.ENCAPSULATE, 0);
        // case 4：试炼密室式的 encapsulate。
        // case 4: a trial-chambers-like encapsulate piece.
        list.add(c);
        c = new Case(seeds[5], chunks[5][0], chunks[5][1]);
        c.rigid(1, 60, 1, 8, 68, 8, TerrainAdjustment.NONE, 0);
        c.rigid(8, 62, 8, 15, 70, 15, TerrainAdjustment.BEARD_THIN, 0);
        // case 5：一块 NONE(贡献 0) 加一块 beard_thin。
        // case 5: a NONE piece (zero contribution) plus a beard_thin piece.
        list.add(c);
        c = new Case(seeds[0], chunks[6][0], chunks[6][1]);
        c.rigid(0, 62, 0, 10, 70, 10, TerrainAdjustment.BEARD_THIN, 1);
        c.rigid(6, -50, 6, 15, -42, 15, TerrainAdjustment.BURY, -2);
        c.rigid(2, 66, 2, 14, 80, 14, TerrainAdjustment.BEARD_BOX, 3);
        c.rigid(4, 300, 4, 12, 318, 12, TerrainAdjustment.ENCAPSULATE, 0);
        c.junction(8, 70, 8).junction(3, 64, 12).junction(12, 66, 3);
        // case 6：四个 adjustment 加三个 junction 同时。
        // case 6: four adjustments plus three junctions at once.
        list.add(c);
        c = new Case(seeds[1], chunks[7][0], chunks[7][1]);
        c.junction(2, 64, 2).junction(13, 66, 5).junction(7, 68, 12).junction(0, 70, 0);
        // case 7：只有 junction。
        // case 7: junctions only.
        list.add(c);
        c = new Case(seeds[2], chunks[8][0], chunks[8][1]);
        c.rigid(3, 60, 3, 14, 74, 14, TerrainAdjustment.BEARD_THIN, -3);
        c.junction(8, 62, 8).junction(11, 64, 11);
        // case 8：负 groundLevelDelta 加 junction。
        // case 8: a negative groundLevelDelta plus junctions.
        list.add(c);
        c = new Case(seeds[3], chunks[9][0], chunks[9][1]);
        c.rigid(0, -60, 0, 15, -48, 15, TerrainAdjustment.BEARD_BOX, 0);
        c.rigid(5, 66, 5, 18, 76, 18, TerrainAdjustment.ENCAPSULATE, 1);
        c.junction(9, 70, 9);
        // case 9：贴底加贴顶加 junction。
        // case 9: bottom-hugging plus top-hugging boxes plus a junction.
        list.add(c);
        c = new Case(seeds[4], chunks[0][0], chunks[0][1]);
        c.rigid(-10, 64, 2, 6, 70, 14, TerrainAdjustment.BEARD_THIN, 0);
        c.rigid(9, 64, -6, 26, 70, 8, TerrainAdjustment.BEARD_THIN, 0);
        // case 10：两块相邻 village beard_thin，XZ 都外扩。
        // case 10: two adjacent village beard_thin pieces, both extending past XZ.
        list.add(c);
        c = new Case(seeds[5], chunks[1][0], chunks[1][1]);
        c.rigid(2, -58, 2, 13, -44, 13, TerrainAdjustment.BURY, 0);
        c.rigid(4, 62, 4, 12, 70, 12, TerrainAdjustment.BEARD_THIN, 2);
        c.junction(8, 64, 8).junction(8, -50, 8);
        // case 11：深 bury 加地表 beard_thin 加两个 junction。
        // case 11: a deep bury plus a surface beard_thin plus two junctions.
        list.add(c);
        return list;
    }

    /**
     * 照 forStructuresInChunk 的尾部：所有 piece / junction 包围盒并集再 inflatedBy(24)。
     *
     * <p>Like the tail of forStructuresInChunk: union of all piece/junction boxes, inflatedBy(24).
     */
    private static BoundingBox affectedBoxOf(Case c) {
        BoundingBox any = null;
        for (Beardifier.Rigid r : c.rigids) {
            any = any == null ? r.box() : BoundingBox.encapsulating(any, r.box());
        }
        for (JigsawJunction j : c.junctions) {
            BoundingBox point = new BoundingBox(
                    new BlockPos(j.getSourceX(), j.getSourceGroundY(), j.getSourceZ()));
            any = any == null ? point : BoundingBox.encapsulating(any, point);
        }
        return any == null ? null : any.inflatedBy(24);
    }

    private static Beardifier buildBeardifier(Case c) {
        return new Beardifier(List.copyOf(c.rigids), List.copyOf(c.junctions), affectedBoxOf(c));
    }

    private static final class Oracle {
        private final RegistryAccess registries;
        private final int through;
        private final Writer out;
        private final Registry<net.minecraft.world.level.levelgen.synth.NormalNoise> noises;
        private final HolderLookup.RegistryLookup<net.minecraft.world.level.levelgen.synth.NormalNoise> noiseLookup;
        private final Registry<NoiseGeneratorSettings> settings;
        private final Method doFill;
        private final Method buildSurface;
        private final Method generateCarvers;
        private final Field globalFluidPicker;
        private final Set<Holder<Biome>> possibleBiomes;
        private long checks;

        Oracle(RegistryAccess registries, int through, Writer out) throws Exception {
            this.registries = registries;
            this.through = through;
            this.out = out;
            this.noises = registries.lookupOrThrow(Registries.NOISE);
            this.noiseLookup = registries.lookupOrThrow(Registries.NOISE);
            this.settings = registries.lookupOrThrow(Registries.NOISE_SETTINGS);
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
            this.possibleBiomes = new HashSet<>();
            for (Biome biome : registries.lookupOrThrow(Registries.BIOME)) {
                this.possibleBiomes.add(registries.lookupOrThrow(Registries.BIOME).wrapAsHolder(biome));
            }
        }

        void run() throws Exception {
            NoiseGeneratorSettings generatorSettings =
                    this.settings.getValue(Identifier.withDefaultNamespace("overworld"));
            if (generatorSettings == null) {
                throw new IllegalStateException("missing minecraft:overworld noise settings");
            }
            NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                    MultiNoiseBiomeSource.createFromPreset(
                            registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                    .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                    this.settings.wrapAsHolder(generatorSettings));
            MaterialRule materialRule = generatorSettings.materialRule().value();
            int minY = generatorSettings.noiseSettings().minY();
            int height = generatorSettings.noiseSettings().height();
            int seaLevel = generatorSettings.seaLevel();
            List<Case> cases = cases();

            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            header.addProperty("minY", minY);
            header.addProperty("height", height);
            header.addProperty("seaLevel", seaLevel);
            header.addProperty("count", cases.size());
            write(header);

            // BEARD_KERNEL 的原始位模式，Rust 侧逐位核对它的构造。
            // Raw BEARD_KERNEL bits, for Rust to verify its construction bit-for-bit.
            Field kernelField = Beardifier.class.getDeclaredField("BEARD_KERNEL");
            kernelField.setAccessible(true);
            float[] kernel = (float[]) kernelField.get(null);
            JsonArray kernelBits = new JsonArray();
            for (float v : kernel) {
                kernelBits.add(Float.floatToRawIntBits(v) & 0xFFFFFFFFL);
                this.checks++;
            }
            JsonObject kernelLine = new JsonObject();
            kernelLine.addProperty("type", "kernel");
            kernelLine.add("bits", kernelBits);
            write(kernelLine);

            for (int id = 0; id < cases.size(); id++) {
                Case c = cases.get(id);
                long seed = c.seed;
                ChunkPos pos = new ChunkPos(c.cx, c.cz);
                RandomState state = RandomState.create(this.noiseLookup, seed, generatorSettings);
                Supplier<Aquifer.FluidPicker> picker = instancePicker(generator);
                BiomeResolver resolver =
                        ((BiomeSource) generator.getBiomeSource()).createUncachedResolver(state);
                BiomeManager biomeManager =
                        new BiomeManager(resolver, BiomeManager.obfuscateSeed(seed));
                ProtoChunk proto = newChunk(pos, generatorSettings);
                generator.createBiomes(state, Blender.empty(), null, proto).join();
                proto.setPersistedStatus(ChunkStatus.BIOMES);

                DensityVolume volume = new DensityVolume(
                        16, height, 16, pos.getMinBlockX(), minY, pos.getMinBlockZ());
                Beardifier beardifier = buildBeardifier(c);
                writeCase(id, c, beardifier, volume, minY, height);

                try (NoiseChunk noiseChunk = new NoiseChunk(
                        state, beardifier, generatorSettings, picker.get(), Blender.empty(), volume)) {
                    this.doFill.invoke(generator, noiseChunk, proto);
                    writeBlocks("noise", id, proto);
                    if (this.through >= 2) {
                        this.buildSurface.invoke(generator, proto, noiseChunk, state, biomeManager,
                                this.possibleBiomes, materialRule);
                        writeBlocks("surface", id, proto);
                    }
                    if (this.through >= 3) {
                        this.generateCarvers.invoke(generator, proto, Blender.empty(), noiseChunk,
                                state, biomeManager, null, materialRule);
                        writeBlocks("carvers", id, proto);
                    }
                }
            }

            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            end.addProperty("checks", this.checks);
            write(end);
            this.out.flush();
        }

        private void writeCase(int id, Case c, Beardifier beardifier, DensityVolume volume,
                int minY, int height) throws Exception {
            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("seed", c.seed);
            line.addProperty("x", c.cx);
            line.addProperty("z", c.cz);
            JsonArray rigids = new JsonArray();
            for (Beardifier.Rigid r : c.rigids) {
                JsonObject o = new JsonObject();
                JsonArray b = new JsonArray();
                b.add(r.box().minX());
                b.add(r.box().minY());
                b.add(r.box().minZ());
                b.add(r.box().maxX());
                b.add(r.box().maxY());
                b.add(r.box().maxZ());
                o.add("box", b);
                o.addProperty("adj", r.terrainAdjustment().getSerializedName());
                o.addProperty("gld", r.groundLevelDelta());
                rigids.add(o);
            }
            line.add("rigids", rigids);
            JsonArray junctions = new JsonArray();
            for (JigsawJunction j : c.junctions) {
                JsonArray a = new JsonArray();
                a.add(j.getSourceX());
                a.add(j.getSourceGroundY());
                a.add(j.getSourceZ());
                junctions.add(a);
            }
            line.add("junctions", junctions);
            BoundingBox affected = affectedBoxOf(c);
            if (affected == null) {
                line.add("affected", null);
            } else {
                JsonArray a = new JsonArray();
                a.add(affected.minX());
                a.add(affected.minY());
                a.add(affected.minZ());
                a.add(affected.maxX());
                a.add(affected.maxY());
                a.add(affected.maxZ());
                line.add("affected", a);
            }
            // sampleValue 逐点（体积顺序 z,x,y），原始位模式。
            // sampleValue point by point (volume order z,x,y), raw bit patterns.
            JsonArray density = new JsonArray();
            int bx0 = c.bx();
            int bz0 = c.bz();
            for (int z = 0; z < 16; z++) {
                for (int x = 0; x < 16; x++) {
                    for (int y = 0; y < height; y++) {
                        float v = beardifier.sampleValue(SamplerContext.EMPTY_UNCACHED,
                                bx0 + x, minY + y, bz0 + z);
                        density.add(Float.floatToRawIntBits(v) & 0xFFFFFFFFL);
                        this.checks++;
                    }
                }
            }
            line.add("density", density);
            // sampleVolume 成批，原始位模式。
            // sampleVolume in bulk, raw bit patterns.
            DensityBuffer buffer = DensityBuffer.createUnpooled(volume.size());
            beardifier.sampleVolume(SamplerContext.EMPTY_UNCACHED, buffer, volume);
            JsonArray vol = new JsonArray();
            for (int i = 0; i < buffer.size(); i++) {
                vol.add(Float.floatToRawIntBits(buffer.get(i)) & 0xFFFFFFFFL);
                this.checks++;
            }
            line.add("volume", vol);
            write(line);
        }

        private Supplier<Aquifer.FluidPicker> instancePicker(
                NoiseBasedChunkGenerator generator) throws Exception {
            @SuppressWarnings("unchecked")
            Supplier<Aquifer.FluidPicker> picker =
                    (Supplier<Aquifer.FluidPicker>) this.globalFluidPicker.get(generator);
            return picker;
        }

        private ProtoChunk newChunk(ChunkPos pos, NoiseGeneratorSettings generatorSettings) {
            LevelHeightAccessor height = LevelHeightAccessor.create(
                    generatorSettings.noiseSettings().minY(), generatorSettings.noiseSettings().height());
            return new ProtoChunk(
                    pos, UpgradeData.EMPTY, height, PalettedContainerFactory.create(this.registries), null);
        }

        private void writeBlocks(String stage, int id, ChunkAccess chunk) throws Exception {
            JsonObject line = new JsonObject();
            line.addProperty("type", "blocks");
            line.addProperty("id", id);
            line.addProperty("stage", stage);
            List<BlockState> palette = new ArrayList<>();
            java.util.Map<BlockState, Integer> ids = new java.util.HashMap<>();
            JsonArray blocks = new JsonArray();
            int minY = chunk.getMinY();
            for (int y = 0; y < chunk.getHeight(); y++) {
                for (int x = 0; x < 16; x++) {
                    for (int z = 0; z < 16; z++) {
                        BlockState state = chunk.getSection(chunk.getSectionIndex(minY + y))
                                .getBlockState(x, y & 15, z);
                        Integer index = ids.get(state);
                        if (index == null) {
                            index = palette.size();
                            palette.add(state);
                            ids.put(state, index);
                        }
                        blocks.add(index);
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
            write(line);
        }

        private void write(JsonObject line) throws Exception {
            this.out.write(line.toString());
            this.out.write('\n');
        }
    }
}
