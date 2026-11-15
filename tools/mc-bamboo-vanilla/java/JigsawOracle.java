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
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceType;
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
import net.minecraft.util.RandomSource;
import net.minecraft.util.Util;
import net.minecraft.util.valueproviders.ConstantInt;
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
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructurePlaceSettings;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessor;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.levelgen.structure.templatesystem.CappedProcessor;
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
    /** 主世界最小 Y（`noise_settings.minecraft:overworld` 的 `min_y`）。 */
    private static final int MIN_Y = -64;
    /** `PiecesContainer.RENAMES`：老的 jigsaw 结构件 id 统一成 `minecraft:jigsaw`。 */
    private static final Map<Identifier, Identifier> LOAD_RENAMES = Map.of(
            Identifier.withDefaultNamespace("nvi"), Identifier.withDefaultNamespace("jigsaw"),
            Identifier.withDefaultNamespace("pcp"), Identifier.withDefaultNamespace("jigsaw"),
            Identifier.withDefaultNamespace("bastionremnant"),
            Identifier.withDefaultNamespace("jigsaw"),
            Identifier.withDefaultNamespace("runtime"), Identifier.withDefaultNamespace("jigsaw"));

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
                    case "load" -> runLoad(manager, registries, Path.of(args[1]));
                    case "apply" -> runApply(registries, Path.of(args[1]));
                    case "capped" -> runCapped(Path.of(args[1]));
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
     * load 模式：queries 每行 {@code id|seed|cx|cz}。先用官方生成结构件，再对每个结构件
     * {@code createTag} → 用官方 {@code StructurePieceType.load} 读回，输出读回后的全部
     * 字段（位置、旋转、包围盒、groundLevelDelta、junction 数、液体设置、元素指纹）。
     * 这样「存档写 → 官方读」这条链路和 Rust 的 `Piece::from_save_tag` 逐项比到，
     * 包围盒重算也在里面。
     *
     * <p>Load mode: one {@code id|seed|cx|cz} per line. Generates the pieces, runs
     * {@code createTag} and then vanilla {@code StructurePieceType.load} to read each one
     * back, printing the fields that survived. This compares the whole write-then-read
     * chain (including the recomputed bounding box) against `Piece::from_save_tag`.
     */
    private static void runLoad(
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
            int reloaded = 0;
            for (int i = 0; i < pieces.size(); i++) {
                CompoundTag tag = pieces.get(i).createTag(ctx);
                StructurePiece loaded;
                try {
                    // 照 `PiecesContainer.load` 的三步：`RENAMES` → 注册表查类型 → `load`。
                    // Mirror `PiecesContainer.load`: `RENAMES`, registry lookup, `load`.
                    Identifier oldKey = Identifier.parse(
                            tag.getStringOr("id", "").toLowerCase(java.util.Locale.ROOT));
                    Identifier pieceId = LOAD_RENAMES.getOrDefault(oldKey, oldKey);
                    StructurePieceType pieceType =
                            BuiltInRegistries.STRUCTURE_PIECE.getValue(pieceId);
                    if (pieceType == null) {
                        out.append("R|").append(key).append('|').append(i)
                                .append("|unknown\n");
                        continue;
                    }
                    loaded = pieceType.load(ctx, tag);
                } catch (RuntimeException error) {
                    // 官方 `PiecesContainer.load` 对坏标签是 catch 后跳过。
                    // Vanilla `PiecesContainer.load` catches bad tags and skips the piece.
                    out.append("R|").append(key).append('|').append(i).append("|skip\n");
                    continue;
                }
                if (!(loaded instanceof PoolElementStructurePiece pool)) {
                    out.append("R|").append(key).append('|').append(i).append("|other\n");
                    continue;
                }
                reloaded++;
                BlockPos pos = pool.getPosition();
                BoundingBox box = pool.getBoundingBox();
                out.append("R|").append(key).append('|').append(i).append('|')
                        .append(pos.getX()).append('|').append(pos.getY()).append('|')
                        .append(pos.getZ()).append('|')
                        .append(pool.getRotation().ordinal()).append('|')
                        .append(box.minX()).append('|').append(box.minY()).append('|')
                        .append(box.minZ()).append('|')
                        .append(box.maxX()).append('|').append(box.maxY()).append('|')
                        .append(box.maxZ()).append('|')
                        .append(pool.getGroundLevelDelta()).append('|')
                        .append(pool.getJunctions().size()).append('|')
                        .append(liquidName(pool)).append('\n');
            }
            out.append("A|").append(key).append('|').append(reloaded).append('\n');
            System.out.print(out);
            System.out.flush();
            out.setLength(0);
        }
    }

    /**
     * 元素的可比指纹：官方 `StructurePoolElement.CODEC` 编出的规范 NBT。元素里带
     * `Holder`（`feature` / `processors`），必须用带注册表的序列化上下文。
     *
     * <p>Element digest: canonical NBT from the official `StructurePoolElement.CODEC`. The
     * element holds `Holder`s (`feature` / `processors`), so it needs a registry-aware
     * serialization context.
     */
    private static String elementDigest(
            RegistryAccess registries, StructurePoolElement element) {
        var ops = registries.createSerializationContext(NbtOps.INSTANCE);
        return canonical(StructurePoolElement.CODEC
                        .encodeStart(ops, element)
                        .getOrThrow(error -> new IllegalStateException("encode: " + error)))
                .toString();
    }

    /**
     * apply 模式：queries 每行
     * {@code list|pieceX|pieceY|pieceZ|refX|refY|refZ|worldState|n|x,y,z,state;...}。
     * 用官方 {@code StructureTemplate.processBlockInfos} 在假世界上跑一遍给定的
     * processor 列表，输出保留下来的方块（位置 + 状态）与丢弃数。假世界只有两种状态：
     * {@code worldState}（所有位置都是它）与空；这足以覆盖 `location_predicate` /
     * `ProtectedBlockProcessor` 的两支。
     *
     * <p>Apply mode: one line per query, running the official
     * {@code StructureTemplate.processBlockInfos} over a fake level that returns
     * `worldState` everywhere, and printing the surviving blocks plus the drop count.
     */
    private static void runApply(RegistryAccess registries, Path queries) throws Exception {
        Registry<StructureProcessorList> reg = registries.lookupOrThrow(Registries.PROCESSOR_LIST);
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            String listId = p[0];
            BlockPos piecePos = new BlockPos(
                    Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3]));
            BlockPos refPos = new BlockPos(
                    Integer.parseInt(p[4]), Integer.parseInt(p[5]), Integer.parseInt(p[6]));
            BlockState worldState = stateOf(p[7]);
            int count = Integer.parseInt(p[8]);
            // 第 10 段（下标 9）是 `chunkBB`：`none` 或 6 个整数。
            // Field 10 (index 9) is the `chunkBB`: `none` or six ints.
            BoundingBox chunkBb = null;
            int at = 9;
            if (!p[at].equals("none")) {
                chunkBb = new BoundingBox(
                        Integer.parseInt(p[at]), Integer.parseInt(p[at + 1]),
                        Integer.parseInt(p[at + 2]), Integer.parseInt(p[at + 3]),
                        Integer.parseInt(p[at + 4]), Integer.parseInt(p[at + 5]));
                at += 6;
            } else {
                at += 1;
            }
            List<StructureTemplate.StructureBlockInfo> infos = new java.util.ArrayList<>();
            List<String> inputProps = new java.util.ArrayList<>();
            int pos = at;
            for (int i = 0; i < count; i++) {
                // 每块是 `x|y|z|state` 四段。
                // Each block is four fields: `x|y|z|state`.
                int x = Integer.parseInt(p[pos]);
                int y = Integer.parseInt(p[pos + 1]);
                int z = Integer.parseInt(p[pos + 2]);
                String stateText = p[pos + 3];
                for (String property : propertyNames(stateText)) {
                    if (!inputProps.contains(property)) {
                        inputProps.add(property);
                    }
                }
                BlockState state = stateOf(stateText);
                pos += 4;
                infos.add(new StructureTemplate.StructureBlockInfo(
                        new BlockPos(x, y, z), state, null));
            }
            StructureProcessorList list = reg.getValue(Identifier.parse(listId));
            if (list == null) {
                throw new IllegalStateException("no processor list " + listId);
            }
            StructurePlaceSettings settings = new StructurePlaceSettings();
            settings.setBoundingBox(chunkBb);
            // 整条链都进 settings：`CappedProcessor.processBlock` 是空实现，但它的
            // `evaluatesEntirePieceState()` 会关掉 `processOnlyInCurrentChunk`，这正是要覆盖的。
            // The whole chain goes in: `CappedProcessor.processBlock` is a no-op, but its
            // `evaluatesEntirePieceState()` disables `processOnlyInCurrentChunk`.
            for (StructureProcessor processor : list.list()) {
                settings.addProcessor(
                        processor instanceof CappedProcessor
                                ? cappedWithoutFinalizing(processor)
                                : processor);
            }
            List<StructureProcessor> capped = new java.util.ArrayList<>();
            for (StructureProcessor processor : list.list()) {
                if (processor instanceof CappedProcessor) {
                    capped.add(processor);
                }
            }
            FakeProcessorLevel level = new FakeProcessorLevel(worldState, 0L);
            List<StructureTemplate.StructureBlockInfo> processed =
                    StructureTemplate.processBlockInfos(
                            level.proxy(), piecePos, refPos, settings, infos);
            // key 只是前 9 段（`list|piece(3)|ref(3)|worldState|count`），方块列表不进去。
            // The key is only the first nine fields; the block list stays out of it.
            String key = String.join("|", java.util.Arrays.copyOfRange(
                    p, 0, chunkBb == null ? 10 : 15));
            out.append("A|").append(key).append('|')
                    .append(processed.size()).append('\n');
            for (StructureTemplate.StructureBlockInfo info : processed) {
                BlockPos bp = info.pos();
                out.append("S|").append(key).append('|')
                        .append(bp.getX()).append('|').append(bp.getY()).append('|')
                        .append(bp.getZ()).append('|')
                        .append(stateName(info.state(), inputProps)).append('\n');
            }
            // `CappedProcessor` 的 `finalizeProcessing` 要 `level.getLevel().getSeed()`，
            // 假世界给不出真实 `ServerLevel`，所以单独输出「它的 delegate 作用在同样的
            // 方块上」的结果：Rust 侧只需比较 delegate 的逐块效果与 capped 的选取下标数。
            // `CappedProcessor.finalizeProcessing` needs `level.getLevel().getSeed()`,
            // which a fake level cannot provide, so emit the delegate's effect on the same
            // blocks instead: Rust only needs that plus the capped selection count.
            for (StructureProcessor processor : capped) {
                // `CappedProcessor.delegate` 是私有字段，反射读。
                // `CappedProcessor.delegate` is private; read it reflectively.
                StructurePlaceSettings delegateSettings = new StructurePlaceSettings();
                delegateSettings.setBoundingBox(chunkBb);
                delegateSettings.addProcessor(cappedDelegate(processor));
                List<StructureTemplate.StructureBlockInfo> viaDelegate =
                        StructureTemplate.processBlockInfos(
                                level.proxy(), piecePos, refPos, delegateSettings, infos);
                out.append("C|").append(key).append('|')
                        .append(viaDelegate.size()).append('\n');
                for (StructureTemplate.StructureBlockInfo info : viaDelegate) {
                    BlockPos bp = info.pos();
                    out.append("D|").append(key).append('|')
                            .append(bp.getX()).append('|').append(bp.getY()).append('|')
                            .append(bp.getZ()).append('|')
                            .append(stateName(info.state(), inputProps)).append('\n');
                }
            }
        }
        System.out.print(out);
        System.out.flush();
    }

    /**
     * 造一个 `limit` 为 0 的等价 `CappedProcessor`：`processBlock` 本来就是空实现，
     * `finalizeProcessing` 在 `limit == 0` 时直接返回，于是不需要真实的
     * `level.getLevel().getSeed()`；而 `evaluatesEntirePieceState()` 仍然是 true，
     * `processOnlyInCurrentChunk` 照样被关掉。这是覆盖那条分支又不伪造 `ServerLevel` 的办法。
     *
     * <p>A `CappedProcessor` clone whose limit is 0: `processBlock` is a no-op and
     * `finalizeProcessing` returns early, so no real `ServerLevel` seed is needed, while
     * `evaluatesEntirePieceState()` stays true and `processOnlyInCurrentChunk` stays off.
     */
    private static StructureProcessor cappedWithoutFinalizing(StructureProcessor capped) {
        try {
            var field = CappedProcessor.class.getDeclaredField("limit");
            field.setAccessible(true);
            Object limit = field.get(capped);
            if (limit instanceof ConstantInt) {
                // 原本就是常数：造一个 0 的替换。
                // Already a constant: replace it with zero.
                return new CappedProcessor(cappedDelegate(capped), ConstantInt.of(0));
            }
            return capped;
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot read CappedProcessor.limit", error);
        }
    }

    /** 反射读 `CappedProcessor.delegate`（没有公开访问器）。 */
    private static StructureProcessor cappedDelegate(StructureProcessor capped) {
        try {
            java.lang.reflect.Field field =
                    net.minecraft.world.level.levelgen.structure.templatesystem.CappedProcessor.class
                            .getDeclaredField("delegate");
            field.setAccessible(true);
            return (StructureProcessor) field.get(capped);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot read CappedProcessor.delegate", error);
        }
    }

    /**
     * 输出方块状态，但**只保留**输入的 `output_state` 里写出来的属性；如果输出状态与
     * 某个输入状态同名，就沿用那个输入的属性集合。这样 Rust 侧不必带全套默认属性。
     *
     * <p>Emit a block state keeping only the properties the input wrote; when the output
     * state shares a name with an input state, that input's property set is reused, so the
     * Rust side need not carry every default property.
     */
    private static String stateName(BlockState state, List<String> propertyNames) {
        StringBuilder sb = new StringBuilder();
        sb.append(BuiltInRegistries.BLOCK.getKey(state.getBlock()));
        List<String> shown = new java.util.ArrayList<>();
        for (var property : state.getProperties()) {
            if (propertyNames.contains(property.getName())) {
                shown.add(property.getName() + "=" + nameOf(state, property));
            }
        }
        if (!shown.isEmpty()) {
            shown.sort(String::compareTo);
            sb.append('[').append(String.join(",", shown)).append(']');
        }
        return sb.toString();
    }

    /** 从 `minecraft:oak_stairs[facing=north]` 取出属性名集合。 */
    private static List<String> propertyNames(String text) {
        List<String> names = new java.util.ArrayList<>();
        int bracket = text.indexOf('[');
        if (bracket < 0) {
            return names;
        }
        for (String pair : text.substring(bracket + 1, text.length() - 1).split(",")) {
            names.add(pair.split("=", 2)[0]);
        }
        return names;
    }

    /** `Block{minecraft:stone}` 或 `Block{minecraft:oak_stairs}[facing=north,...]`。 */
    private static String stateName(BlockState state) {
        StringBuilder sb = new StringBuilder();
        sb.append(BuiltInRegistries.BLOCK.getKey(state.getBlock()));
        if (!state.getProperties().isEmpty()) {
            sb.append('[');
            boolean first = true;
            for (var property : state.getProperties()) {
                if (!first) {
                    sb.append(',');
                }
                first = false;
                sb.append(property.getName()).append('=').append(nameOf(state, property));
            }
            sb.append(']');
        }
        return sb.toString();
    }

    private static <T extends Comparable<T>> String nameOf(
            BlockState state, net.minecraft.world.level.block.state.properties.Property<T> p) {
        return p.getName(state.getValue(p));
    }

    /** 从 `minecraft:stone` / `minecraft:oak_stairs[facing=north]` 造方块状态。 */
    private static BlockState stateOf(String text) {
        int bracket = text.indexOf('[');
        String name = bracket < 0 ? text : text.substring(0, bracket);
        Block block = BuiltInRegistries.BLOCK.getValue(Identifier.parse(name));
        if (block == null) {
            throw new IllegalStateException("no block " + name);
        }
        BlockState state = block.defaultBlockState();
        if (bracket >= 0) {
            String body = text.substring(bracket + 1, text.length() - 1);
            for (String pair : body.split(",")) {
                String[] kv = pair.split("=", 2);
                state = withProperty(state, kv[0], kv[1]);
            }
        }
        return state;
    }

    private static <T extends Comparable<T>> BlockState withProperty(
            BlockState state, String name, String value) {
        var property = state.getBlock().getStateDefinition().getProperty(name);
        if (property == null) {
            throw new IllegalStateException("no property " + name);
        }
        return setProperty(state, property, value);
    }

    private static <T extends Comparable<T>> BlockState setProperty(
            BlockState state,
            net.minecraft.world.level.block.state.properties.Property<T> property,
            String value) {
        return state.setValue(property, property.getValue(value).orElseThrow());
    }

    /**
     * 假世界：所有位置的方块都是同一个 `worldState`。`getBlockState` 与 `is(HolderSet)`
     * 是 processor 唯一会用到的东西。
     *
     * <p>Fake level: every position holds the same `worldState`; `getBlockState` and
     * `is(HolderSet)` are all a processor needs.
     */
    /**
     * capped 模式：queries 每行 {@code seed|pieceX|pieceY|pieceZ|size}。复刻
     * `CappedProcessor.finalizeProcessing` 的随机源与洗牌：
     * {@code RandomSource.createThreadLocalInstance(seed).forkPositional().at(pos)}，
     * 再 {@code Util.toShuffledList(IntStream.range(0, size), random)}，输出洗牌后的下标序列
     * 与随后 {@code limit.sample(random)} 的取值。这样「随机源派生 + 洗牌顺序 + 取样」
     * 三段都能和 Rust 逐项比，不需要伪造真实 {@code ServerLevel}。
     *
     * <p>Capped mode: one {@code seed|pieceX|pieceY|pieceZ|size} per line, replaying the
     * random source and shuffle of `CappedProcessor.finalizeProcessing` and printing the
     * shuffled indices plus the sampled limit, so all three stages can be compared.
     */
    private static void runCapped(Path queries) throws Exception {
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            long seed = Long.parseLong(p[0]);
            BlockPos piece = new BlockPos(
                    Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3]));
            int size = Integer.parseInt(p[4]);
            var random = RandomSource.createThreadLocalInstance(seed).forkPositional().at(piece);
            // 官方顺序：先 `limit.sample(random)`，再 `toShuffledList`。
            // Vanilla order: `limit.sample(random)` first, then `toShuffledList`.
            int sampled = net.minecraft.util.valueproviders.ConstantInt.of(6).sample(random);
            var indices = net.minecraft.util.Util.toShuffledList(
                    java.util.stream.IntStream.range(0, size), random);
            out.append("C|").append(String.join("|", p)).append('|')
                    .append(sampled).append('|');
            for (int i = 0; i < indices.size(); i++) {
                if (i > 0) {
                    out.append(',');
                }
                out.append(indices.getInt(i));
            }
            out.append('\n');
            // 再补一条 `nextInt` 序列，单独验随机源本身（不经过洗牌）。
            // Also emit a raw `nextInt` sequence to check the source itself.
            var src = RandomSource.createThreadLocalInstance(seed).forkPositional().at(piece);
            out.append("R|").append(String.join("|", p)).append('|');
            for (int i = 0; i < 8; i++) {
                if (i > 0) {
                    out.append(',');
                }
                out.append(src.nextInt(1000));
            }
            out.append('\n');
        }
        System.out.print(out);
        System.out.flush();
    }

    /**
     * 假世界：所有位置的方块都是同一个 `worldState`。processor 只用
     * `getBlockState` 与 `state.is(tag)`，所以用代理挡住其余方法。
     *
     * <p>Fake level: every position holds the same `worldState`. Processors only touch
     * `getBlockState` and `state.is(tag)`, so a proxy covers the rest.
     */
    private static final class FakeProcessorLevel {
        private final BlockState worldState;
        /** `CappedProcessor.finalizeProcessing` 只用 `level.getLevel().getSeed()`。 */
        private final long levelSeed;

        FakeProcessorLevel(BlockState worldState, long levelSeed) {
            this.worldState = worldState;
            this.levelSeed = levelSeed;
        }

        net.minecraft.world.level.ServerLevelAccessor proxy() {
            return (net.minecraft.world.level.ServerLevelAccessor)
                    java.lang.reflect.Proxy.newProxyInstance(
                            JigsawOracle.class.getClassLoader(),
                            new Class<?>[] {net.minecraft.world.level.ServerLevelAccessor.class},
                            (self, method, args) -> switch (method.getName()) {
                                case "getBlockState" -> this.worldState;
                                case "getFluidState" -> this.worldState.getFluidState();
                                case "isEmptyBlock" -> this.worldState.isAir();
                                case "getLevel" -> this.fakeServerLevel();
                                case "getMinY" -> JigsawOracle.MIN_Y;
                                case "getHeight" ->
                                        method.getParameterCount() == 0 ? 384 : JigsawOracle.MIN_Y + 1;
                                case "getBlockEntity" -> null;
                                case "toString" -> "FakeProcessorLevel";
                                case "hashCode" -> System.identityHashCode(self);
                                case "equals" -> self == args[0];
                                default -> throw new UnsupportedOperationException(
                                        "ServerLevelAccessor." + method.getName() + "/"
                                                + method.getParameterCount());
                            });
        }

        /**
         * 假 `ServerLevel`：`CappedProcessor.finalizeProcessing` 只调用 `getSeed()`。
         * `ServerLevel` 是类不是接口，代理造不出来，所以用 `Unsafe.allocateInstance`
         * 分配一个不跑构造器的实例，再用反射把 `Level` 里的种子字段设成 `levelSeed`。
         *
         * <p>Fake `ServerLevel`: `finalizeProcessing` only calls `getSeed()`. `ServerLevel`
         * is a class, so a proxy cannot stand in; allocate it without running the
         * constructor and set the level seed field reflectively.
         */
        private net.minecraft.server.level.ServerLevel fakeServerLevel() {
            try {
                var unsafeField = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
                unsafeField.setAccessible(true);
                Object unsafe = unsafeField.get(null);
                var allocate = Class.forName("sun.misc.Unsafe")
                        .getMethod("allocateInstance", Class.class);
                net.minecraft.server.level.ServerLevel level =
                        (net.minecraft.server.level.ServerLevel) allocate.invoke(
                                unsafe, net.minecraft.server.level.ServerLevel.class);
                // `Level` 里保存种子的字段在 26.3 叫 `seed`（`ServerLevelData` 之后会覆盖它，
                // 但 `getSeed()` 直接读这个字段）。
                // The field holding the seed is `seed` in 26.3; `getSeed()` reads it directly.
                java.lang.reflect.Field seedField = null;
                for (Class<?> c = level.getClass(); c != null; c = c.getSuperclass()) {
                    try {
                        seedField = c.getDeclaredField("seed");
                        break;
                    } catch (NoSuchFieldException ignored) {
                        // 继续往父类找。
                        // Keep walking up the hierarchy.
                    }
                }
                if (seedField == null) {
                    throw new IllegalStateException("no seed field on ServerLevel");
                }
                seedField.setAccessible(true);
                seedField.setLong(level, this.levelSeed);
                return level;
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException("cannot build a fake ServerLevel", error);
            }
        }
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
