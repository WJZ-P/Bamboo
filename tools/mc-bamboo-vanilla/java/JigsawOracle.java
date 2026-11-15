import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Predicate;
import net.minecraft.nbt.ByteArrayTag;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.LongArrayTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.NumericTag;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.JigsawBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.Vec3i;
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
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.PoolElementStructurePiece;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.BooleanOp;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder;
import net.minecraft.world.level.levelgen.structure.pools.EmptyPoolElement;
import net.minecraft.world.level.levelgen.structure.pools.FeaturePoolElement;
import net.minecraft.world.level.levelgen.structure.pools.JigsawJunction;
import net.minecraft.world.level.levelgen.structure.pools.JigsawPlacement;
import net.minecraft.world.level.levelgen.structure.pools.LegacySinglePoolElement;
import net.minecraft.world.level.levelgen.structure.pools.ListPoolElement;
import net.minecraft.world.level.levelgen.structure.pools.SinglePoolElement;
import net.minecraft.world.level.levelgen.structure.pools.StructurePoolElement;
import net.minecraft.world.level.levelgen.structure.pools.StructureTemplatePool;
import net.minecraft.world.level.levelgen.structure.pools.alias.PoolAliasLookup;
import net.minecraft.world.level.levelgen.structure.structures.JigsawStructure;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessorList;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessorType;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.levelgen.structure.templatesystem.LiquidSettings;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.storage.LevelResource;
import net.minecraft.world.level.storage.LevelStorageSource;

/**
 * 官方 jigsaw 对拍。构造真实的 StructureTemplateManager（从 vanilla 数据包加载
 * data/minecraft/structure/**.nbt），把 Structure.GenerationContext 的
 * structureTemplateManager 从 null 换成它。
 *
 * <p>templates 模式：逐个 get(id) 加载模板，输出 size / save() 的 blocks 数 /
 * getJigsaws(ZERO, NONE) 的拼图方块数，验证模板确实被加载。
 *
 * <p>pieces 模式：queries 每行 {@code id|seed|cx|cz}。造官方
 * NoiseBasedChunkGenerator / RandomState / GenerationContext，调
 * JigsawStructure.findGenerationPoint，输出 stub 位置和每结构件的类型、模板名、
 * 位置、旋转、包围盒、groundLevelDelta、junction 数。
 *
 * <p>Compare jigsaw against vanilla with a real StructureTemplateManager. No
 * server, no save, no EULA.
 *
 * <p>Usage: JigsawOracle templates|pieces &lt;queries.txt&gt;
 */
public final class JigsawOracle {
    private static final Gson GSON = new Gson();

    private JigsawOracle() {
    }

    public static void main(String[] args) throws Exception {
        // `processors` 模式不需要 queries 文件。
        // The `processors` mode needs no queries file.
        boolean noQueries = args.length == 1 && args[0].equals("processors");
        if (args.length != 2 && !noQueries) {
            throw new IllegalArgumentException(
                    "usage: JigsawOracle templates|pieces|pools|save|processors <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            Path temp = Files.createTempDirectory("jigsaw-stm");
            LevelStorageSource storageSource = LevelStorageSource.createDefault(temp);
            try (LevelStorageSource.LevelStorageAccess storage = storageSource.createAccess("world")) {
                Files.createDirectories(storage.getLevelPath(LevelResource.GENERATED_DIR));
                StructureTemplateManager manager = new StructureTemplateManager(
                        resources, storage, DataFixers.getDataFixer(),
                        registries.lookupOrThrow(Registries.BLOCK));
                switch (args[0]) {
                    case "templates" -> runTemplates(manager, Path.of(args[1]));
                    case "jigsaws" -> runJigsaws(manager, Path.of(args[1]));
                    case "pieces" -> runPieces(manager, registries, Path.of(args[1]));
                    case "pools" -> runPools(manager, registries, Path.of(args[1]));
                    case "save" -> runSave(manager, registries, Path.of(args[1]));
                    case "processors" -> runProcessors(registries);
                    case "shapes" -> runShapes(Path.of(args[1]));
                    default -> throw new IllegalArgumentException("unknown mode " + args[0]);
                }
            }
        }
    }

    private static void runTemplates(StructureTemplateManager manager, Path queries) throws Exception {
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            Identifier id = Identifier.parse(line.trim());
            StructureTemplate template = manager.get(id).orElseThrow(
                    () -> new IllegalStateException("template not loaded: " + id));
            Vec3i size = template.getSize();
            CompoundTag saved = template.save(new CompoundTag());
            int blocks = saved.getListOrEmpty("blocks").size();
            int jigsaws = template.getJigsaws(BlockPos.ZERO, Rotation.NONE).size();
            out.append(id).append('|').append(size.getX()).append('|')
                    .append(size.getY()).append('|').append(size.getZ()).append('|')
                    .append(blocks).append('|').append(jigsaws).append('\n');
        }
        System.out.print(out);
    }

    private static void runPieces(
            StructureTemplateManager manager, RegistryAccess registries, Path queries)
            throws Exception {
        HolderLookup.RegistryLookup<NormalNoise> noiseLookup =
                registries.lookupOrThrow(Registries.NOISE);
        Registry<NoiseGeneratorSettings> settingsReg =
                registries.lookupOrThrow(Registries.NOISE_SETTINGS);
        NoiseGeneratorSettings settings =
                settingsReg.getValue(Identifier.withDefaultNamespace("overworld"));
        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(
                                        Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        BiomeSource biomeSource = generator.getBiomeSource();
        LevelHeightAccessor heightAccessor = LevelHeightAccessor.create(
                settings.noiseSettings().minY(), settings.noiseSettings().height());
        Registry<Structure> structureReg = registries.lookupOrThrow(Registries.STRUCTURE);

        Map<Long, RandomState> states = new HashMap<>();
        Map<Long, Climate.Sampler> samplers = new HashMap<>();
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            String id = p[0];
            long seed = Long.parseLong(p[1]);
            int cx = Integer.parseInt(p[2]);
            int cz = Integer.parseInt(p[3]);
            RandomState state = states.get(seed);
            Climate.Sampler sampler = samplers.get(seed);
            if (state == null || sampler == null) {
                state = RandomState.create(noiseLookup, seed, settings);
                sampler = state.createClimateSampler(
                        SamplerContext.builder().enableCaches().build());
                states.put(seed, state);
                samplers.put(seed, sampler);
            }
            Structure structure = structureReg.getValue(Identifier.parse(id));
            if (!(structure instanceof JigsawStructure jigsaw)) {
                throw new IllegalStateException(id + " is not a jigsaw structure");
            }
            Predicate<Holder<Biome>> validBiome = structure.biomes()::contains;
            Structure.GenerationContext context = new Structure.GenerationContext(
                    registries, generator, biomeSource, sampler, state, manager, seed,
                    new ChunkPos(cx, cz), heightAccessor, validBiome);
            String key = id + "|" + seed + "|" + cx + "|" + cz;
            Optional<Structure.GenerationStub> stub = jigsaw.findGenerationPoint(context);
            boolean valid = stub.isPresent() && context.isValidBiome(stub.get());
            if (stub.isEmpty()) {
                out.append("Q|").append(key).append("|0|0|0|0|0\n");
                System.out.print(out);
                System.out.flush();
                out.setLength(0);
                continue;
            }
            StructurePiecesBuilder builder = stub.get().getPiecesBuilder();
            List<StructurePiece> pieces = builder.build().pieces();
            BlockPos position = stub.get().position();
            // Q 行末段是 `isValidBiome(stub)`，即 `findValidGenerationPoint` 的最后一道门。
            // The Q line ends with `isValidBiome(stub)`, the last gate of
            // `findValidGenerationPoint`.
            out.append("Q|").append(key).append("|1|").append(position.getX()).append('|')
                    .append(position.getY()).append('|').append(position.getZ()).append('|')
                    .append(pieces.size()).append('|').append(valid ? 1 : 0).append('\n');
            for (int i = 0; i < pieces.size(); i++) {
                StructurePiece piece = pieces.get(i);
                String description = piece instanceof PoolElementStructurePiece pool
                        ? describe(pool) : "other|-|0|0|0|0|0|0|0|0|0|0|0|0";
                out.append("P|").append(key).append('|').append(i).append('|')
                        .append(description).append('\n');
            }
            System.out.print(out);
            System.out.flush();
            out.setLength(0);
        }
    }

    /**
     * pools 模式：queries 每行 {@code pool|seed|cx|cz}。把任意 template pool 当起始 pool
     * 直接调官方 {@code JigsawPlacement.addPieces}（maxDepth 4、不 project、expansion hack 开），
     * 输出 stub 位置和全部结构件。这样 feature / list / empty 三种元素当中心件、以及每个
     * pool 的拼接路径都被逐件比到，不依赖哪个结构碰巧用到它们。
     *
     * <p>Pool mode: one {@code pool|seed|cx|cz} per line. Runs vanilla
     * {@code JigsawPlacement.addPieces} with that pool as the start pool (maxDepth 4, no
     * projection, expansion hack on) and prints the stub plus every piece, so feature,
     * list and empty elements as the center piece and every pool's attachment path get
     * compared per piece instead of relying on which structure happens to use them.
     */
    private static void runPools(
            StructureTemplateManager manager, RegistryAccess registries, Path queries)
            throws Exception {
        HolderLookup.RegistryLookup<NormalNoise> noiseLookup =
                registries.lookupOrThrow(Registries.NOISE);
        Registry<NoiseGeneratorSettings> settingsReg =
                registries.lookupOrThrow(Registries.NOISE_SETTINGS);
        NoiseGeneratorSettings settings =
                settingsReg.getValue(Identifier.withDefaultNamespace("overworld"));
        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(
                                        Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        BiomeSource biomeSource = generator.getBiomeSource();
        LevelHeightAccessor heightAccessor = LevelHeightAccessor.create(
                settings.noiseSettings().minY(), settings.noiseSettings().height());
        Registry<StructureTemplatePool> poolReg = registries.lookupOrThrow(Registries.TEMPLATE_POOL);

        Map<Long, RandomState> states = new HashMap<>();
        Map<Long, Climate.Sampler> samplers = new HashMap<>();
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            String poolName = p[0];
            long seed = Long.parseLong(p[1]);
            int cx = Integer.parseInt(p[2]);
            int cz = Integer.parseInt(p[3]);
            RandomState state = states.get(seed);
            Climate.Sampler sampler = samplers.get(seed);
            if (state == null || sampler == null) {
                state = RandomState.create(noiseLookup, seed, settings);
                sampler = state.createClimateSampler(
                        SamplerContext.builder().enableCaches().build());
                states.put(seed, state);
                samplers.put(seed, sampler);
            }
            Holder<StructureTemplatePool> startPool = poolReg
                    .get(Identifier.parse(poolName))
                    .orElseThrow(() -> new IllegalStateException("no pool " + poolName));
            Structure.GenerationContext context = new Structure.GenerationContext(
                    registries, generator, biomeSource, sampler, state, manager, seed,
                    new ChunkPos(cx, cz), heightAccessor, biome -> true);
            String key = poolName + "|" + seed + "|" + cx + "|" + cz;
            BlockPos startPos = new BlockPos(cx * 16, 64, cz * 16);
            Optional<Structure.GenerationStub> stub = JigsawPlacement.addPieces(
                    context,
                    startPool,
                    Optional.empty(),
                    4,
                    startPos,
                    true,
                    Optional.empty(),
                    new JigsawStructure.MaxDistance(80),
                    PoolAliasLookup.EMPTY,
                    JigsawStructure.DEFAULT_DIMENSION_PADDING,
                    JigsawStructure.DEFAULT_LIQUID_SETTINGS);
            if (stub.isEmpty()) {
                out.append("Q|").append(key).append("|0|0|0|0|0\n");
                System.out.print(out);
                System.out.flush();
                out.setLength(0);
                continue;
            }
            StructurePiecesBuilder builder = stub.get().getPiecesBuilder();
            List<StructurePiece> pieces = builder.build().pieces();
            BlockPos position = stub.get().position();
            // 这里的 `validBiome` 恒真，所以 `isValidBiome` 也恒真，直接写 1。
            // `validBiome` is always true here, so `isValidBiome` is too; write 1.
            out.append("Q|").append(key).append("|1|").append(position.getX()).append('|')
                    .append(position.getY()).append('|').append(position.getZ()).append('|')
                    .append(pieces.size()).append("|1\n");
            for (int i = 0; i < pieces.size(); i++) {
                StructurePiece piece = pieces.get(i);
                String description = piece instanceof PoolElementStructurePiece pool
                        ? describe(pool) : "other|-|0|0|0|0|0|0|0|0|0|0|0|0|0|-|0";
                out.append("P|").append(key).append('|').append(i).append('|')
                        .append(description).append('\n');
            }
            System.out.print(out);
            System.out.flush();
            out.setLength(0);
        }
    }

    /**
     * save 模式：queries 每行 {@code id|seed|cx|cz}。用真实结构件跑官方
     * {@code PoolElementStructurePiece.createTag}，输出每个 Children 条目的规范 NBT，
     * 这样 pool_element / rotation / junctions / liquid_settings / PosX 等子类字段都被
     * 逐键比到，不只是基类四字段。
     *
     * <p>Save mode: one {@code id|seed|cx|cz} per line. Runs vanilla
     * {@code PoolElementStructurePiece.createTag} on real pieces and prints each Children
     * entry as canonical NBT, comparing every subclass field (pool_element, rotation,
     * junctions, liquid_settings, PosX, ...) instead of just the four base fields.
     */
    private static void runSave(
            StructureTemplateManager manager, RegistryAccess registries, Path queries)
            throws Exception {
        HolderLookup.RegistryLookup<NormalNoise> noiseLookup =
                registries.lookupOrThrow(Registries.NOISE);
        Registry<NoiseGeneratorSettings> settingsReg =
                registries.lookupOrThrow(Registries.NOISE_SETTINGS);
        NoiseGeneratorSettings settings =
                settingsReg.getValue(Identifier.withDefaultNamespace("overworld"));
        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(
                                        Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        BiomeSource biomeSource = generator.getBiomeSource();
        LevelHeightAccessor heightAccessor = LevelHeightAccessor.create(
                settings.noiseSettings().minY(), settings.noiseSettings().height());
        Registry<Structure> structureReg = registries.lookupOrThrow(Registries.STRUCTURE);
        StructurePieceSerializationContext ctx =
                new StructurePieceSerializationContext(null, registries, manager);

        Map<Long, RandomState> states = new HashMap<>();
        Map<Long, Climate.Sampler> samplers = new HashMap<>();
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            String id = p[0];
            long seed = Long.parseLong(p[1]);
            int cx = Integer.parseInt(p[2]);
            int cz = Integer.parseInt(p[3]);
            RandomState state = states.get(seed);
            Climate.Sampler sampler = samplers.get(seed);
            if (state == null || sampler == null) {
                state = RandomState.create(noiseLookup, seed, settings);
                sampler = state.createClimateSampler(
                        SamplerContext.builder().enableCaches().build());
                states.put(seed, state);
                samplers.put(seed, sampler);
            }
            Structure structure = structureReg.getValue(Identifier.parse(id));
            if (!(structure instanceof JigsawStructure jigsaw)) {
                throw new IllegalStateException(id + " is not a jigsaw structure");
            }
            Predicate<Holder<Biome>> validBiome = structure.biomes()::contains;
            Structure.GenerationContext context = new Structure.GenerationContext(
                    registries, generator, biomeSource, sampler, state, manager, seed,
                    new ChunkPos(cx, cz), heightAccessor, validBiome);
            String key = id + "|" + seed + "|" + cx + "|" + cz;
            Optional<Structure.GenerationStub> stub = jigsaw.findGenerationPoint(context);
            if (stub.isEmpty()) {
                out.append("Q|").append(key).append("|0|0\n");
                System.out.print(out);
                System.out.flush();
                out.setLength(0);
                continue;
            }
            List<StructurePiece> pieces = stub.get().getPiecesBuilder().build().pieces();
            out.append("Q|").append(key).append("|1|").append(pieces.size()).append('\n');
            for (int i = 0; i < pieces.size(); i++) {
                CompoundTag tag = pieces.get(i).createTag(ctx);
                out.append("T|").append(key).append('|').append(i).append('|')
                        .append(GSON.toJson(canonical(tag))).append('\n');
            }
            System.out.print(out);
            System.out.flush();
            out.setLength(0);
        }
    }

    /**
     * processors 模式：不带 queries 文件，把注册表里全部 `processor_list` 用官方
     * `StructureProcessorType.LIST_CODEC` 编码成 NBT 输出。这样 40 个列表里的每一种
     * processor（`block_rot` / `rule` / `protected_blocks` / `capped`）、五种 `RuleTest`、
     * `axis_aligned_linear_pos` 和 `append_loot` 的编码都被逐键比到，不依赖某次生成是否
     * 碰到它们。
     *
     * <p>Processor mode: no queries file. Encodes every registered `processor_list` with
     * vanilla `StructureProcessorType.LIST_CODEC`, so every processor, `RuleTest`,
     * `PosRuleTest` and block-entity modifier in the 40 lists is compared key by key.
     */
    private static void runProcessors(RegistryAccess registries) throws Exception {
        Registry<StructureProcessorList> reg = registries.lookupOrThrow(Registries.PROCESSOR_LIST);
        var ops = registries.createSerializationContext(NbtOps.INSTANCE);
        StringBuilder out = new StringBuilder();
        for (var entry : reg.entrySet()) {
            Identifier id = entry.getKey().identifier();
            StructureProcessorList list = entry.getValue();
            // `LIST_CODEC` 编的是 `Holder`，走引用时只写注册名，所以另外用 `DIRECT_CODEC`
            // 编内联内容：那个才真正展开每个 processor。
            // `LIST_CODEC` encodes a `Holder` and yields only the registry name; also
            // encode the inline contents with `DIRECT_CODEC`, which expands each processor.
            Tag reference = StructureProcessorType.LIST_CODEC
                    .encodeStart(ops, reg.wrapAsHolder(list))
                    .getOrThrow(error -> new IllegalStateException(id + ": " + error));
            Tag inline = StructureProcessorType.DIRECT_CODEC
                    .encodeStart(ops, list)
                    .getOrThrow(error -> new IllegalStateException(id + ": " + error));
            out.append("P|").append(id).append('|')
                    .append(GSON.toJson(canonical(reference))).append('|')
                    .append(GSON.toJson(canonical(inline))).append('\n');
        }
        System.out.print(out);
        System.out.flush();
    }

    /**
     * `CompoundTag` 的键序不确定：转成按键排序的规范 JSON（同 StructureSaveOracle）。
     * 浮点用位模式（`#xxxxxxxx`），避免 Java 与 Rust 的默认格式化差异（`1.0E-4` vs
     * `0.0001`）造成假失败。
     *
     * <p>Key-sorted canonical JSON like StructureSaveOracle. Floats use their bit pattern
     * so Java and Rust default formatting (`1.0E-4` vs `0.0001`) cannot cause false diffs.
     */
    private static JsonObject canonical(Tag tag) {
        JsonObject out = new JsonObject();
        out.addProperty("type", tag.getId());
        switch (tag.getId()) {
            case 1, 2, 3, 4 -> out.addProperty("value", String.valueOf(((NumericTag) tag).box()));
            case 5 -> out.addProperty("value", String.format(
                    "#%08x", Float.floatToRawIntBits(((NumericTag) tag).floatValue())));
            case 6 -> out.addProperty("value", String.format(
                    "#%016x", Double.doubleToRawLongBits(((NumericTag) tag).doubleValue())));
            case 8 -> out.addProperty("value", ((StringTag) tag).value());
            case 7 -> {
                JsonArray values = new JsonArray();
                for (byte value : ((ByteArrayTag) tag).getAsByteArray()) {
                    values.add((int) value);
                }
                out.add("values", values);
            }
            case 11 -> {
                JsonArray values = new JsonArray();
                for (int value : ((IntArrayTag) tag).getAsIntArray()) {
                    values.add(value);
                }
                out.add("values", values);
            }
            case 12 -> {
                JsonArray values = new JsonArray();
                for (long value : ((LongArrayTag) tag).getAsLongArray()) {
                    values.add(value);
                }
                out.add("values", values);
            }
            case 9 -> {
                JsonArray values = new JsonArray();
                for (Tag value : (ListTag) tag) {
                    values.add(canonical(value));
                }
                out.add("values", values);
            }
            case 10 -> {
                JsonObject values = new JsonObject();
                List<String> keys = new ArrayList<>(((CompoundTag) tag).keySet());
                keys.sort(String::compareTo);
                for (String key : keys) {
                    values.add(key, canonical(((CompoundTag) tag).get(key)));
                }
                out.add("values", values);
            }
            default -> throw new IllegalArgumentException("tag type " + tag.getId());
        }
        return out;
    }

    /**
     * jigsaws 模式：queries 每行 {@code id|rotation|x|y|z}。对每个旋转和位置调
     * {@code StructureTemplate.getJigsaws(position, rotation)}，输出每个拼图方块的
     * 世界位置、front、top、name、target、pool、joint、placement/selection 优先级。
     * 这样 getRandomPalette 的按位置选择、旋转后的 front/top 都被直接对拍。
     *
     * <p>Jigsaw-block mode: one {@code id|rotation|x|y|z} per line. Prints every jigsaw
     * block's world position, front, top, name, target, pool, joint and priorities after
     * {@code StructureTemplate.getJigsaws(position, rotation)}, which covers both the
     * position-based palette choice and the rotated front/top.
     */
    private static void runJigsaws(StructureTemplateManager manager, Path queries) throws Exception {
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            Identifier id = Identifier.parse(p[0]);
            Rotation rotation = Rotation.BY_ID.apply(Integer.parseInt(p[1]));
            BlockPos pos = new BlockPos(
                    Integer.parseInt(p[2]), Integer.parseInt(p[3]), Integer.parseInt(p[4]));
            StructureTemplate template = manager.get(id).orElseThrow();
            List<StructureTemplate.JigsawBlockInfo> jigsaws = template.getJigsaws(pos, rotation);
            out.append("J|").append(line.trim()).append('|').append(jigsaws.size()).append('\n');
            for (StructureTemplate.JigsawBlockInfo jigsaw : jigsaws) {
                BlockState state = jigsaw.state();
                out.append("K|").append(line.trim()).append('|')
                        .append(jigsaw.pos().getX()).append('|')
                        .append(jigsaw.pos().getY()).append('|')
                        .append(jigsaw.pos().getZ()).append('|')
                        .append(JigsawBlock.getFrontFacing(state).getSerializedName()).append('|')
                        .append(JigsawBlock.getTopFacing(state).getSerializedName()).append('|')
                        .append(jigsaw.name() == null ? "-" : jigsaw.name().toString()).append('|')
                        .append(jigsaw.target()).append('|')
                        .append(jigsaw.pool().identifier()).append('|')
                        .append(jigsaw.jointType().getSerializedName()).append('|')
                        .append(jigsaw.placementPriority()).append('|')
                        .append(jigsaw.selectionPriority()).append('\n');
            }
            System.out.print(out);
            out.setLength(0);
        }
    }

    /**
     * 类型|模板名|pos|rotation|boundingBox(6)|groundLevelDelta|genDepth|liquid|junctions|junction 详情。
     *
     * <p>Type|template|pos|rotation|boundingBox(6)|groundLevelDelta|genDepth|liquid|junctions|details.
     */
    private static String describe(PoolElementStructurePiece piece) {
        StructurePoolElement element = piece.getElement();
        String type;
        String location;
        if (element == EmptyPoolElement.INSTANCE) {
            type = "empty";
            location = "-";
        } else if (element instanceof LegacySinglePoolElement) {
            type = "legacy";
            location = ((SinglePoolElement) element).getTemplateLocation().toString();
        } else if (element instanceof SinglePoolElement single) {
            type = "single";
            location = single.getTemplateLocation().toString();
        } else if (element instanceof ListPoolElement) {
            type = "list";
            location = "-";
        } else if (element instanceof FeaturePoolElement) {
            type = "feature";
            location = "-";
        } else {
            type = "other";
            location = "-";
        }
        BlockPos pos = piece.getPosition();
        BoundingBox box = piece.getBoundingBox();
        StringBuilder line = new StringBuilder();
        line.append(type).append('|').append(location).append('|')
                .append(pos.getX()).append('|').append(pos.getY()).append('|')
                .append(pos.getZ()).append('|')
                .append(piece.getRotation().ordinal()).append('|')
                .append(box.minX()).append('|').append(box.minY()).append('|')
                .append(box.minZ()).append('|')
                .append(box.maxX()).append('|').append(box.maxY()).append('|')
                .append(box.maxZ()).append('|')
                .append(piece.getGroundLevelDelta()).append('|')
                .append(piece.getGenDepth()).append('|')
                .append(liquidName(piece)).append('|')
                .append(piece.getJunctions().size());
        // junction 详情：sourceX|sourceGroundY|sourceZ|deltaY|destProjection。
        // Junction detail: sourceX|sourceGroundY|sourceZ|deltaY|destProjection.
        for (JigsawJunction junction : piece.getJunctions()) {
            line.append('|')
                    .append(junction.getSourceX()).append('|')
                    .append(junction.getSourceGroundY()).append('|')
                    .append(junction.getSourceZ()).append('|')
                    .append(junction.getDeltaY()).append('|')
                    .append(junction.getDestProjection().getName());
        }
        return line.toString();
    }

    /**
     * `PoolElementStructurePiece.liquidSettings` 没有公开访问器，用反射读；读不到记 `?`。
     *
     * <p>`PoolElementStructurePiece.liquidSettings` has no accessor; read it via
     * reflection, falling back to `?`.
     */
    private static String liquidName(PoolElementStructurePiece piece) {
        try {
            java.lang.reflect.Field field =
                    PoolElementStructurePiece.class.getDeclaredField("liquidSettings");
            field.setAccessible(true);
            Object value = field.get(piece);
            return value instanceof LiquidSettings settings ? settings.getSerializedName() : "?";
        } catch (ReflectiveOperationException error) {
            return "?";
        }
    }

    /** 独立小场景：模拟 addPieces 里 free 的 join / joinIsNotEmpty / joinUnoptimized 序列。
     *
     * <p>Standalone scenes mimicking the free-space join sequence in addPieces.
     */
    private static void runShapes(Path queries) throws Exception {
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] f = line.trim().split("\\|");
            String scene = f[0];
            String kind = f[1];
            double[] big = new double[6];
            int[] initial = new int[6];
            for (int i = 0; i < 6; i++) {
                big[i] = Integer.parseInt(f[2 + i]);
                initial[i] = Integer.parseInt(f[8 + i]);
            }
            int count = Integer.parseInt(f[14]);
            AABB bigAabb = new AABB(big[0], big[1], big[2], big[3], big[4], big[5]);
            BoundingBox initialBox = new BoundingBox(
                    initial[0], initial[1], initial[2],
                    initial[3], initial[4], initial[5]);
            VoxelShape free;
            if (kind.equals("context")) {
                free = Shapes.join(Shapes.create(bigAabb),
                        Shapes.create(AABB.of(initialBox)), BooleanOp.ONLY_FIRST);
            } else {
                free = Shapes.create(AABB.of(initialBox));
            }
            StringBuilder results = new StringBuilder();
            int pos = 15;
            for (int k = 0; k < count; k++) {
                int[] t = new int[6];
                for (int i = 0; i < 6; i++) {
                    t[i] = Integer.parseInt(f[pos + i]);
                }
                pos += 6;
                BoundingBox target = new BoundingBox(t[0], t[1], t[2], t[3], t[4], t[5]);
                AABB deflated = AABB.of(target).deflate(0.25);
                boolean blocked = Shapes.joinIsNotEmpty(
                        free, Shapes.create(deflated), BooleanOp.ONLY_SECOND);
                results.append(blocked ? '1' : '0');
                if (!blocked) {
                    free = Shapes.joinUnoptimized(
                            free, Shapes.create(AABB.of(target)), BooleanOp.ONLY_FIRST);
                }
            }
            out.append(scene).append('|').append(results).append('\n');
        }
        // D 行：直接输出 AABB.of(bb).deflate(0.25) 的乘 4 边界，验证 deflate 数值。
        int[][] deflateCases = {
            {0, 0, 0, 0, 0, 0}, {5, 5, 5, 9, 9, 9},
            {-3, -2, -1, 4, 6, 8}, {10, 0, 0, 10, 0, 0},
        };
        for (int[] b : deflateCases) {
            AABB d = AABB.of(new BoundingBox(
                    b[0], b[1], b[2], b[3], b[4], b[5])).deflate(0.25);
            out.append("D|").append((long) (d.minX * 4)).append('|')
                    .append((long) (d.minY * 4)).append('|')
                    .append((long) (d.minZ * 4)).append('|')
                    .append((long) (d.maxX * 4)).append('|')
                    .append((long) (d.maxY * 4)).append('|')
                    .append((long) (d.maxZ * 4)).append('\n');
        }
        System.out.print(out);
    }

    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(
            CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }
}
