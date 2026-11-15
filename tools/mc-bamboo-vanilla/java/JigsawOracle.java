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
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.Mirror;
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
import net.minecraft.world.level.levelgen.structure.templatesystem.rule.blockentity.AppendLoot;
import net.minecraft.world.level.levelgen.structure.templatesystem.rule.blockentity.AppendStatic;
import net.minecraft.world.level.levelgen.structure.templatesystem.rule.blockentity.Clear;
import net.minecraft.world.level.levelgen.structure.templatesystem.rule.blockentity.Passthrough;
import net.minecraft.world.level.levelgen.structure.templatesystem.rule.blockentity.RuleBlockEntityModifier;
import net.minecraft.nbt.TagParser;
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
            // `Block.newBlockEntity` 会读组件的默认值，而组件只在服务器资源重载时
            // （`ReloadableServerResources`）才被绑定。这里手动跑一次，
            // 让方块实体能被构造出来（`rc` 模式需要）。
            // `Block.newBlockEntity` reads component defaults, which are bound only during a
            // server resource reload. Run it once here so block entities can be built.
            BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(registries)
                    .forEach(net.minecraft.core.component.DataComponentInitializers.PendingComponents::apply);
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
                    case "capped" -> runCapped(registries, Path.of(args[1]));
                    case "cappedfull" -> runCappedFull(registries, Path.of(args[1]));
                    case "placepiece" -> runPlacePiece(manager, registries, Path.of(args[1]));
                    case "waterpiece" -> runWaterPiece(manager, registries, Path.of(args[1]));
                    case "entities" -> runEntities(manager, Path.of(args[1]));
                    case "bemod" -> runBlockEntityModifier(registries, Path.of(args[1]));
                    case "rc" -> runRandomizableContainer(registries, Path.of(args[1]));
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
     * capped 模式：queries 每行 {@code seed|pieceX|pieceY|pieceZ|listSize|limit}。复刻
     * `CappedProcessor.finalizeProcessing` 的完整取数顺序：
     * {@code RandomSource.createThreadLocalInstance(seed).forkPositional().at(pos)}，
     * {@code maxToReplace = min(limit.sample(random), listSize)}，
     * {@code Util.toShuffledList(IntStream.range(0, listSize), random)}，然后取前
     * {@code maxToReplace} 个下标。输出 {@code maxToReplace} 与**截断后的下标序列**，这样
     * 「随机源派生 + 取样 + 洗牌 + 截断」四段都能逐项比。
     *
     * <p>Capped mode: one {@code seed|pieceX|pieceY|pieceZ|listSize|limit} per line,
     * replaying the whole draw order of `CappedProcessor.finalizeProcessing` and printing
     * the clamped `maxToReplace` plus the truncated index list, so all four stages can be
     * compared.
     */
    /**
     * capped 模式：queries 每行
     * {@code seed|pieceX|pieceY|pieceZ|listSize|limit|integrity}。构造真实的
     * `new CappedProcessor(new BlockRotProcessor(integrity), ConstantInt.of(limit))`，
     * 放进 `StructurePlaceSettings` 后调官方 `StructureTemplate.processBlockInfos`，
     * 输出**真正存活下来的方块**（位置 + 状态）。Rust 侧走
     * `structure::processor_apply::process_blocks`，两边都是产品代码。
     *
     * <p>Capped mode: builds a real
     * `CappedProcessor(BlockRotProcessor(integrity), ConstantInt.of(limit))`, runs vanilla
     * `processBlockInfos` and prints the surviving blocks. The Rust side runs
     * `process_blocks`, so both sides exercise product code.
     */
    /** `capped` 对拍用的 16 种羊毛注册名，两边按同一顺序取。 */
    private static final String[] WOOL_NAMES = {
        "minecraft:white_wool",
        "minecraft:orange_wool",
        "minecraft:magenta_wool",
        "minecraft:light_blue_wool",
        "minecraft:yellow_wool",
        "minecraft:lime_wool",
        "minecraft:pink_wool",
        "minecraft:gray_wool",
        "minecraft:light_gray_wool",
        "minecraft:cyan_wool",
        "minecraft:purple_wool",
        "minecraft:blue_wool",
        "minecraft:brown_wool",
        "minecraft:green_wool",
        "minecraft:red_wool",
        "minecraft:black_wool",
    };

    private static void runCapped(RegistryAccess registries, Path queries) throws Exception {
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            long seed = Long.parseLong(p[0]);
            BlockPos piece = new BlockPos(
                    Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3]));
            int listSize = Integer.parseInt(p[4]);
            int limit = Integer.parseInt(p[5]);
            float integrity = Float.parseFloat(p[6]);
            // 输入方块：每个位置一块**不同**颜色的羊毛。必须各不相同，否则
            // `BlockRotProcessor` 把方块换成空气后，「换掉了哪一个」在输出里看不出来，
            // 洗牌顺序就无法被验证。
            // Inputs: a **distinct** wool colour per position. They must differ, otherwise
            // `BlockRotProcessor` turning a block into air makes "which one was replaced"
            // invisible and the shuffle order becomes unverifiable.
            List<StructureTemplate.StructureBlockInfo> infos = new java.util.ArrayList<>();
            for (int i = 0; i < listSize; i++) {
                var wool = BuiltInRegistries.BLOCK.getValue(
                        Identifier.parse(WOOL_NAMES[i % WOOL_NAMES.length]));
                infos.add(new StructureTemplate.StructureBlockInfo(
                        new BlockPos(i % 4, i / 4, 0),
                        wool.defaultBlockState(),
                        null));
            }
            StructurePlaceSettings settings = new StructurePlaceSettings();
            // delegate 必须是 `RuleProcessor`：`BlockRotProcessor` 返回 `null`，而
            // `finalizeProcessing` 只在 `maybeAltered != null && !equals` 时计数，
            // `null` 会被跳过，delegate 效果不可观测。
            // The delegate must be a `RuleProcessor`: `BlockRotProcessor` returns `null`
            // and `finalizeProcessing` only counts `maybeAltered != null && !equals`, so a
            // `null` result is skipped and the delegate's effect becomes invisible.
            List<String> allRules = java.util.List.of(
                    "minecraft:white_wool", "minecraft:orange_wool",
                    "minecraft:magenta_wool", "minecraft:light_blue_wool",
                    "minecraft:yellow_wool", "minecraft:lime_wool",
                    "minecraft:pink_wool", "minecraft:gray_wool",
                    "minecraft:light_gray_wool", "minecraft:cyan_wool",
                    "minecraft:purple_wool", "minecraft:blue_wool",
                    "minecraft:brown_wool", "minecraft:green_wool",
                    "minecraft:red_wool", "minecraft:black_wool");
            // 输出列与 Rust 侧 `all_rules` 的第二列一一对应。
            // The output column matches the second column of the Rust `all_rules` table.
            List<String> allOutputs = java.util.List.of(
                    "minecraft:blue_wool", "minecraft:light_blue_wool",
                    "minecraft:lime_wool", "minecraft:yellow_wool",
                    "minecraft:purple_wool", "minecraft:pink_wool",
                    "minecraft:gray_wool", "minecraft:light_gray_wool",
                    "minecraft:cyan_wool", "minecraft:orange_wool",
                    "minecraft:magenta_wool", "minecraft:white_wool",
                    "minecraft:red_wool", "minecraft:brown_wool",
                    "minecraft:green_wool", "minecraft:cyan_wool");
            int enabled = Math.min(
                    Math.round(allRules.size() * integrity), allRules.size());
            java.util.List<net.minecraft.world.level.levelgen.structure.templatesystem
                    .ProcessorRule> rules = new java.util.ArrayList<>();
            for (int i = 0; i < enabled; i++) {
                String source = allRules.get(i);
                String output = allOutputs.get(i);
                rules.add(new net.minecraft.world.level.levelgen.structure.templatesystem
                        .ProcessorRule(
                                new net.minecraft.world.level.levelgen.structure.templatesystem
                                        .BlockMatchTest(
                                                BuiltInRegistries.BLOCK.getValue(
                                                        Identifier.parse(source))),
                                net.minecraft.world.level.levelgen.structure.templatesystem
                                        .AlwaysTrueTest.INSTANCE,
                                BuiltInRegistries.BLOCK.getValue(
                                        Identifier.parse(output)).defaultBlockState()));
            }
            settings.addProcessor(new net.minecraft.world.level.levelgen.structure.templatesystem
                    .CappedProcessor(
                            new net.minecraft.world.level.levelgen.structure.templatesystem
                                    .RuleProcessor(rules),
                            net.minecraft.util.valueproviders.ConstantInt.of(limit)));
            // `getLevel().getSeed()` 必须是真种子：`finalizeProcessing` 用它派生随机源。
            // `getLevel().getSeed()` must be the real seed; `finalizeProcessing` derives its
            // random source from it.
            FakeProcessorLevel level = new FakeProcessorLevel(Blocks.AIR.defaultBlockState(), seed);
            List<StructureTemplate.StructureBlockInfo> processed =
                    StructureTemplate.processBlockInfos(
                            level.proxy(), piece, piece, settings, infos);
            String key = String.join("|", p);
            out.append("C|").append(key).append('|').append(processed.size()).append('\n');
            for (StructureTemplate.StructureBlockInfo info : processed) {
                BlockPos bp = info.pos();
                out.append("S|").append(key).append('|')
                        .append(bp.getX()).append('|').append(bp.getY()).append('|')
                        .append(bp.getZ()).append('|')
                        .append(BuiltInRegistries.BLOCK.getKey(info.state().getBlock()))
                        .append('\n');
            }
        }
        System.out.print(out);
        System.out.flush();
    }

    /**
     * 造一个 `getSeed()` 返回 `seed` 的假 `ServerLevel`。`getSeed()` 的链路是
     * `server.worldGenSettings.options.seed`，三层都是 final 字段，所以用
     * `Unsafe.allocateInstance` 分配、再用反射写入。这样 `CappedProcessor.finalizeProcessing`
     * 里的 `level.getLevel().getSeed()` 就能拿到真实数值，从而能端到端对拍整个函数。
     *
     * <p>A fake `ServerLevel` whose `getSeed()` returns `seed`. The chain is
     * `server.worldGenSettings.options.seed`; all three are final, so allocate them with
     * `Unsafe.allocateInstance` and set the fields reflectively. That makes
     * `CappedProcessor.finalizeProcessing`'s `level.getLevel().getSeed()` return a real
     * value, so the whole method can be compared end to end.
     */
    private static net.minecraft.server.level.ServerLevel fakeServerLevelWithSeed(long seed) {
        try {
            Class<?> unsafeClass = Class.forName("sun.misc.Unsafe");
            var theUnsafe = unsafeClass.getDeclaredField("theUnsafe");
            theUnsafe.setAccessible(true);
            Object unsafe = theUnsafe.get(null);
            var allocate = unsafeClass.getMethod("allocateInstance", Class.class);

            Object worldOptions = allocate.invoke(
                    unsafe, net.minecraft.world.level.levelgen.WorldOptions.class);
            setFinalField(worldOptions, "seed", seed);

            Object genSettings = allocate.invoke(
                    unsafe, net.minecraft.world.level.levelgen.WorldGenSettings.class);
            setFinalField(genSettings, "options", worldOptions);

            Object server = allocate.invoke(
                    unsafe, net.minecraft.server.dedicated.DedicatedServer.class);
            setFinalField(server, "worldGenSettings", genSettings);

            Object level = allocate.invoke(
                    unsafe, net.minecraft.server.level.ServerLevel.class);
            setFinalField(level, "server", server);
            return (net.minecraft.server.level.ServerLevel) level;
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot build a seeded fake ServerLevel", error);
        }
    }

    /** 写一个可能声明在父类上的字段。 */
    private static void setFinalField(Object target, String name, Object value) throws
            ReflectiveOperationException {
        for (Class<?> c = target.getClass(); c != null; c = c.getSuperclass()) {
            try {
                var field = c.getDeclaredField(name);
                field.setAccessible(true);
                field.set(target, value);
                return;
            } catch (NoSuchFieldException ignored) {
                // 继续往父类找。
                // Keep walking up the hierarchy.
            }
        }
        throw new NoSuchFieldException(name);
    }

    /**
     * cappedfull 模式：queries 每行
     * {@code list|seed|pieceX|pieceY|pieceZ|worldState|n|x,y,z,state...}。
     * 用真实种子跑**完整**的 `StructureTemplate.processBlockInfos`（链里保留真实的
     * `CappedProcessor`，于是 `finalizeProcessing` 会真的执行限额替换），输出最终存活
     * 的方块。这样 `capped` 的「随机源 + 洗牌 + 取样」与 delegate 效果被端到端串起来验证。
     *
     * <p>Capped-full mode: runs the complete `processBlockInfos` with a real seed so
     * `finalizeProcessing` really performs the capped replacement, and prints the final
     * surviving blocks — the end-to-end check the piecewise `capped` mode cannot give.
     */
    private static void runCappedFull(RegistryAccess registries, Path queries) throws Exception {
        Registry<StructureProcessorList> reg = registries.lookupOrThrow(Registries.PROCESSOR_LIST);
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            String listId = p[0];
            long seed = Long.parseLong(p[1]);
            BlockPos piece = new BlockPos(
                    Integer.parseInt(p[2]), Integer.parseInt(p[3]), Integer.parseInt(p[4]));
            BlockState worldState = stateOf(p[5]);
            int count = Integer.parseInt(p[6]);
            List<String> inputProps = new java.util.ArrayList<>();
            List<StructureTemplate.StructureBlockInfo> infos = new java.util.ArrayList<>();
            int pos = 7;
            for (int i = 0; i < count; i++) {
                int x = Integer.parseInt(p[pos]);
                int y = Integer.parseInt(p[pos + 1]);
                int z = Integer.parseInt(p[pos + 2]);
                String stateText = p[pos + 3];
                for (String property : propertyNames(stateText)) {
                    if (!inputProps.contains(property)) {
                        inputProps.add(property);
                    }
                }
                pos += 4;
                infos.add(new StructureTemplate.StructureBlockInfo(
                        new BlockPos(x, y, z), stateOf(stateText), null));
            }
            StructureProcessorList list = reg.getValue(Identifier.parse(listId));
            if (list == null) {
                throw new IllegalStateException("no processor list " + listId);
            }
            StructurePlaceSettings settings = new StructurePlaceSettings();
            for (StructureProcessor processor : list.list()) {
                settings.addProcessor(processor);
            }
            // 这里 `levelSeed` 是真的：`fakeServerLevelWithSeed` 让
            // `getLevel().getSeed()` 返回 `seed`。
            // The level seed is real here: `getLevel().getSeed()` returns `seed`.
            FakeProcessorLevel level = new FakeProcessorLevel(worldState, seed);
            List<StructureTemplate.StructureBlockInfo> processed =
                    StructureTemplate.processBlockInfos(
                            level.proxy(), piece, piece, settings, infos);
            String key = String.join("|", java.util.Arrays.copyOfRange(p, 0, 7));
            out.append("A|").append(key).append('|').append(processed.size()).append('\n');
            for (StructureTemplate.StructureBlockInfo info : processed) {
                BlockPos bp = info.pos();
                out.append("S|").append(key).append('|')
                        .append(bp.getX()).append('|').append(bp.getY()).append('|')
                        .append(bp.getZ()).append('|')
                        .append(stateName(info.state(), inputProps)).append('\n');
            }
        }
        System.out.print(out);
        System.out.flush();
    }

    /**
     * entities 模式：queries 每行
     * {@code template|pieceX|pieceY|pieceZ|mirror|rotation|bb(6)}。
     * 复算官方 {@code StructureTemplate.placeEntities} 里除「创建实体」以外的全部步骤：
     * `transform` 后的 `blockPos` 与 `Vec3`、改写后的 `Pos`、删掉的 `UUID`、改写后的
     * `block_pos`，以及 `entity.rotate(rotation) + entity.mirror(mirror) - entity.getYRot()`。
     * Rust 侧的 `jigsaw::place::place_entities` 复算同样的东西，逐实体比较。
     *
     * <p>Entities mode: replays every step of `StructureTemplate.placeEntities` except
     * creating the entity, and prints the transformed `blockPos` / `Vec3`, the rewritten
     * `Pos`, the dropped `UUID`, the rewritten `block_pos` and the computed `yRot`.
     */
    private static void runEntities(
            StructureTemplateManager manager, Path queries) throws Exception {
        StringBuilder out = new StringBuilder();
        // 一个不初始化字段的具体 `Entity`（`Marker` 是最简单的具体子类），只为调
        // `rotate` / `mirror` 这两个官方方法：它们只读 `getYRot()`，`setYRot` 也只写那
        // 一个字段，所以字段全零的实例足够。
        // A concrete `Entity` with no initialisation (`Marker` is the simplest concrete
        // subclass), used only for the official `rotate` / `mirror` methods: they read
        // `getYRot()` and `setYRot` writes just that field, so an all-zero instance is fine.
        net.minecraft.world.entity.Entity blankEntity =
                (net.minecraft.world.entity.Entity) allocateBlank(
                        net.minecraft.world.entity.Marker.class);
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            Identifier id = Identifier.parse(p[0]);
            BlockPos piece = new BlockPos(
                    Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3]));
            Mirror mirror = Mirror.values()[Integer.parseInt(p[4])];
            Rotation rotation = Rotation.values()[Integer.parseInt(p[5])];
            BoundingBox bb = new BoundingBox(
                    Integer.parseInt(p[6]), Integer.parseInt(p[7]), Integer.parseInt(p[8]),
                    Integer.parseInt(p[9]), Integer.parseInt(p[10]), Integer.parseInt(p[11]));
            StructureTemplate template = manager.get(id).orElseThrow(
                    () -> new IllegalStateException("template not loaded: " + id));
            // `entityInfoList` 是私有的；用 `save` 拿同一份数据（`entities` 列表）。
            // `entityInfoList` is private; `save` exposes the same data.
            CompoundTag saved = template.save(new CompoundTag());
            String key = String.join("|", p);
            var entities = saved.getListOrEmpty("entities");
            out.append("Q|").append(key).append('|').append(entities.size()).append('\n');
            for (int i = 0; i < entities.size(); i++) {
                CompoundTag entry = entities.getCompoundOrEmpty(i);
                CompoundTag nbt = entry.getCompoundOrEmpty("nbt");
                BlockPos templateBlockPos = readBlockPos(entry.getListOrEmpty("blockPos"));
                ListTag posList = entry.getListOrEmpty("pos");
                net.minecraft.world.phys.Vec3 templatePos = new net.minecraft.world.phys.Vec3(
                        posList.getDoubleOr(0, 0.0),
                        posList.getDoubleOr(1, 0.0),
                        posList.getDoubleOr(2, 0.0));
                BlockPos blockPos = StructureTemplate
                        .transform(templateBlockPos, mirror, rotation, BlockPos.ZERO)
                        .offset(piece);
                if (!bb.isInside(blockPos)) {
                    continue;
                }
                net.minecraft.world.phys.Vec3 pos = StructureTemplate
                        .transform(templatePos, mirror, rotation, BlockPos.ZERO)
                        .add(piece.getX(), piece.getY(), piece.getZ());
                float rawYRot = nbt.getListOrEmpty("Rotation").getFloatOr(0, 0.0F);
                float xRot = nbt.getListOrEmpty("Rotation").getFloatOr(1, 0.0F);
                blankEntity.setYRot(rawYRot);
                float yRot = blankEntity.rotate(rotation);
                yRot += blankEntity.mirror(mirror) - blankEntity.getYRot();
                // 照官方顺序改写这一份副本：`Pos` 覆盖、删 `UUID`、`block_pos` 重存，
                // 然后才读回来报告。
                // Rewrite this copy in vanilla's order: overwrite `Pos`, drop `UUID`,
                // re-store `block_pos`; only then read back what gets reported.
                ListTag posTag = new ListTag();
                posTag.add(net.minecraft.nbt.DoubleTag.valueOf(pos.x));
                posTag.add(net.minecraft.nbt.DoubleTag.valueOf(pos.y));
                posTag.add(net.minecraft.nbt.DoubleTag.valueOf(pos.z));
                nbt.put("Pos", posTag);
                // 删之前先记下来：删完再判永远是否。
                // Record it before the removal; checking afterwards would always say no.
                boolean hadUuid = nbt.contains("UUID");
                nbt.remove("UUID");
                if (nbt.contains("block_pos")) {
                    nbt.store("block_pos", BlockPos.CODEC, blockPos);
                }
                BlockPos rewrittenBlockPos = nbt.contains("block_pos")
                        ? nbt.read("block_pos", BlockPos.CODEC).orElse(blockPos)
                        : null;
                out.append("E|").append(key)
                        .append('|').append(blockPos.getX())
                        .append(',').append(blockPos.getY())
                        .append(',').append(blockPos.getZ())
                        .append('|').append(Double.doubleToRawLongBits(pos.x))
                        .append('|').append(Double.doubleToRawLongBits(pos.y))
                        .append('|').append(Double.doubleToRawLongBits(pos.z))
                        .append('|').append(Float.floatToRawIntBits(yRot))
                        .append('|').append(Float.floatToRawIntBits(xRot))
                        .append('|').append(nbt.contains("UUID") ? 1 : 0)
                        .append('|').append(rewrittenBlockPos == null ? "-" : rewrittenBlockPos.getX()
                                + "," + rewrittenBlockPos.getY() + "," + rewrittenBlockPos.getZ())
                        .append('|').append(nbt.getStringOr("id", ""))
                        .append('\n');
            }
        }
        System.out.print(out);
        System.out.flush();
    }

    private static BlockPos readBlockPos(ListTag list) {
        return new BlockPos(
                list.getIntOr(0, 0), list.getIntOr(1, 0), list.getIntOr(2, 0));
    }

    /// `sun.misc.Unsafe.allocateInstance`：跳过构造器造一个字段全零的对象。
    ///
    /// `sun.misc.Unsafe.allocateInstance`: allocate an all-zero instance, skipping the
    /// constructor.
    private static Object allocateBlank(Class<?> type) throws ReflectiveOperationException {
        Class<?> unsafeClass = Class.forName("sun.misc.Unsafe");
        var theUnsafe = unsafeClass.getDeclaredField("theUnsafe");
        theUnsafe.setAccessible(true);
        Object unsafe = theUnsafe.get(null);
        return unsafeClass.getMethod("allocateInstance", Class.class).invoke(unsafe, type);
    }

    /**
     * placepiece 模式：queries 每行
     * {@code template|pieceX|pieceY|pieceZ|mirror|rotation|bb(6)|worldState}。
     * 用官方 {@code StructureTemplate.placeInWorld}（`IGNORE_WATERLOGGING`，无 processor）
     * 在记录型假世界上放置模板，输出每次 `setBlock` 的世界坐标与最终状态。Rust 侧的
     * `jigsaw::place::place_piece` 做同样的事，逐写入比较。
     *
     * <p>Place-piece mode: runs the official `StructureTemplate.placeInWorld` (no
     * processors, `IGNORE_WATERLOGGING`) over a recording fake level and prints every
     * `setBlock` with its final state, comparing the whole place path block by block.
     */
    private static void runPlacePiece(
            StructureTemplateManager manager, RegistryAccess registries, Path queries)
            throws Exception {
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|");
            Identifier id = Identifier.parse(p[0]);
            BlockPos piece = new BlockPos(
                    Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3]));
            Mirror mirror = Mirror.values()[Integer.parseInt(p[4])];
            Rotation rotation = Rotation.values()[Integer.parseInt(p[5])];
            BoundingBox bb = new BoundingBox(
                    Integer.parseInt(p[6]), Integer.parseInt(p[7]), Integer.parseInt(p[8]),
                    Integer.parseInt(p[9]), Integer.parseInt(p[10]), Integer.parseInt(p[11]));
            StructureTemplate template = manager.get(id).orElseThrow(
                    () -> new IllegalStateException("template not loaded: " + id));
            StructurePlaceSettings settings = new StructurePlaceSettings();
            settings.setRotation(rotation);
            settings.setMirror(mirror);
            settings.setBoundingBox(bb);
            settings.setLiquidSettings(LiquidSettings.IGNORE_WATERLOGGING);
            // `SinglePoolElement.getSettings` 也这么设：形状已知时官方跳过
            // `updateShapeAtEdge` 与逐方块 `updateFromNeighbourShapes`，这两段不在本对拍范围内。
            // Same as `SinglePoolElement.getSettings`: a known shape makes vanilla skip
            // `updateShapeAtEdge` and the per-block `updateFromNeighbourShapes`, neither of
            // which this oracle covers.
            settings.setKnownShape(true);
            RecordingLevel level = new RecordingLevel(registries);
            // `RandomSource.create(0)`：这一轮不比随机相关行为（没有 processor）。
            // `RandomSource.create(0)`: nothing random in this round (no processors).
            boolean placed = template.placeInWorld(
                    level.proxy(),
                    piece,
                    piece,
                    settings,
                    RandomSource.create(0L),
                    2);
            String key = String.join("|", java.util.Arrays.copyOfRange(p, 0, 12));
            out.append("Q|").append(key).append('|').append(placed ? 1 : 0).append('|')
                    .append(level.writes.size()).append('\n');
            for (var w : level.writes) {
                out.append("W|").append(key).append('|')
                        .append(w.pos().getX()).append('|')
                        .append(w.pos().getY()).append('|')
                        .append(w.pos().getZ()).append('|')
                        .append(BuiltInRegistries.BLOCK.getKey(w.state().getBlock())).append('\n');
            }
            level.collectSeeds();
            // `S|key|x|y|z|seed`：带 nbt 且方块实体是随机容器的位置。
            // 按 `(x, y, z)` 排序（`TreeMap<BlockPos>` 用的是 `Vec3i.compareTo`，即 `y, z, x`，
            // 与 Rust 侧不同，所以这里显式重排）。
            // `S|key|x|y|z|seed`: positions with nbt whose block entity is a randomizable
            // container, sorted by `(x, y, z)` to match the Rust side (`BlockPos.compareTo`
            // orders by `y, z, x`).
            java.util.List<BlockPos> seedOrder = new java.util.ArrayList<>(level.seeds.keySet());
            seedOrder.sort(java.util.Comparator.<BlockPos>comparingInt(BlockPos::getX)
                    .thenComparingInt(BlockPos::getY).thenComparingInt(BlockPos::getZ));
            for (BlockPos pos : seedOrder) {
                out.append("S|").append(key).append('|')
                        .append(pos.getX()).append('|')
                        .append(pos.getY()).append('|')
                        .append(pos.getZ()).append('|')
                        .append(level.seeds.get(pos)).append('\n');
            }
        }
        System.out.print(out);
        System.out.flush();
    }

    /**
     * waterpiece 模式：查询每行
     * {@code template|pieceX|pieceY|pieceZ|mirror|rotation|bb(6)|fluids}，
     * `fluids` 是 `-` 或 `kind:x,y,z,kind:x,y,z,...` 形式的水位表（`kind` 为
     * `water` / `flowing_water` / `lava`）。用 `APPLY_WATERLOGGING` 跑官方
     * `StructureTemplate.placeInWorld`，输出每次 `setBlock` 的坐标与**完整状态**
     * （含属性），Rust 侧的 `jigsaw::place::place_piece` 带流体模型复算。
     *
     * <p>Water-piece mode: like place-piece but with `APPLY_WATERLOGGING` and a fluid table,
     * printing every write with its full state (properties included).
     */
    private static void runWaterPiece(
            StructureTemplateManager manager, RegistryAccess registries, Path queries)
            throws Exception {
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\|", -1);
            Identifier id = Identifier.parse(p[0]);
            BlockPos piece = new BlockPos(
                    Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3]));
            Mirror mirror = Mirror.values()[Integer.parseInt(p[4])];
            Rotation rotation = Rotation.values()[Integer.parseInt(p[5])];
            BoundingBox bb = new BoundingBox(
                    Integer.parseInt(p[6]), Integer.parseInt(p[7]), Integer.parseInt(p[8]),
                    Integer.parseInt(p[9]), Integer.parseInt(p[10]), Integer.parseInt(p[11]));
            String fluids = p[12];
            StructureTemplate template = manager.get(id).orElseThrow(
                    () -> new IllegalStateException("template not loaded: " + id));
            StructurePlaceSettings settings = new StructurePlaceSettings();
            settings.setRotation(rotation);
            settings.setMirror(mirror);
            settings.setBoundingBox(bb);
            settings.setLiquidSettings(LiquidSettings.APPLY_WATERLOGGING);
            // `SinglePoolElement.getSettings` 里就有 `setKnownShape(true)`：形状已知时官方
            // 跳过 `updateShapeAtEdge` 与逐方块 `updateFromNeighbourShapes` 那两段。
            // 这一段在 `placepiece` 里也没覆盖，跳过保持两边口径一致。
            // `SinglePoolElement.getSettings` sets `setKnownShape(true)`, which makes vanilla
            // skip `updateShapeAtEdge` and the per-block `updateFromNeighbourShapes`; the
            // placepiece oracle skips them too, so this keeps both comparable.
            settings.setKnownShape(true);
            RecordingLevel level = new RecordingLevel(registries);
            if (!fluids.equals("-")) {
                for (String entry : fluids.split(";")) {
                    String[] parts = entry.split(":");
                    int kind = switch (parts[0]) {
                        case "water" -> 1;
                        case "flowing_water" -> 2;
                        case "lava" -> 3;
                        default -> throw new IllegalArgumentException("bad fluid " + parts[0]);
                    };
                    String[] xyz = parts[1].split(",");
                    level.setFluid(new BlockPos(
                            Integer.parseInt(xyz[0]),
                            Integer.parseInt(xyz[1]),
                            Integer.parseInt(xyz[2])), kind);
                }
            }
            boolean placed = template.placeInWorld(
                    level.proxy(),
                    piece,
                    piece,
                    settings,
                    RandomSource.create(0L),
                    2);
            String key = String.join("|", p);
            out.append("Q|").append(key).append('|').append(placed ? 1 : 0).append('|')
                    .append(level.writes.size()).append('|')
                    .append(level.fluidQueries).append('|')
                    .append(level.blockAndUpdateCalls).append('\n');
            for (var w : level.writes) {
                out.append("W|").append(key).append('|')
                        .append(w.pos().getX()).append('|')
                        .append(w.pos().getY()).append('|')
                        .append(w.pos().getZ()).append('|')
                        .append(stateText(w.state())).append('\n');
            }
        }
        System.out.print(out);
        System.out.flush();
    }

    /**
     * 方块状态的比较文本：`minecraft:oak_slab[waterlogged=true,type=bottom]`，属性按名字
     * 排序后输出，只保留 Rust 侧也能算的属性（全部属性）。
     *
     * <p>A comparable block-state text with every property, sorted by name.
     */
    private static String stateText(BlockState state) {
        var properties = new java.util.TreeMap<String, String>();
        for (var property : state.getProperties()) {
            properties.put(property.getName(), propertyValue(state, property));
        }
        String suffix = properties.isEmpty()
                ? ""
                : properties.entrySet().stream()
                        .map(entry -> entry.getKey() + "=" + entry.getValue())
                        .collect(java.util.stream.Collectors.joining(",", "[", "]"));
        return BuiltInRegistries.BLOCK.getKey(state.getBlock()) + suffix;
    }

    @SuppressWarnings("unchecked")
    private static <T extends Comparable<T>> String propertyValue(
            BlockState state, net.minecraft.world.level.block.state.properties.Property<T> property) {
        return property.getName(state.getValue(property));
    }

    /**
     * bemod 模式：把 26.3 数据里出现过的每一种 `block_entity_modifier`（`append_loot` /
     * `clear` / `passthrough` / `append_static`）用官方 `apply(random, existing)` 跑一遍，
     * 输出生成的 NBT。Rust 侧用 `RuleBlockEntityModifier::apply` 复算，逐键比。
     * 这一条专门覆盖 `append_loot` 的 `LootTable` + `LootTableSeed`（含 `nextLong` 取数）。
     *
     * <p>Block-entity-modifier mode: runs every modifier kind through the official
     * `apply(random, existing)` and prints the resulting NBT, covering `append_loot`'s
     * `LootTable` + `LootTableSeed` (including the `nextLong` draw).
     */
    private static void runBlockEntityModifier(RegistryAccess registries, Path queries)
            throws Exception {
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            // `kind|seed|lootTable|existingJson`，`existingJson` 为 `-` 表示没有 nbt。
            // `kind|seed|lootTable|existingJson`; `-` means no existing nbt.
            String[] p = line.trim().split("\\|", -1);
            String kind = p[0];
            long seed = Long.parseLong(p[1]);
            String lootTable = p[2];
            var ops = registries.createSerializationContext(NbtOps.INSTANCE);
            CompoundTag existing = p[3].equals("-")
                    ? null
                    : TagParser.parseCompoundFully(p[3]);
            RandomSource random = RandomSource.create(seed);
            RuleBlockEntityModifier modifier = switch (kind) {
                case "passthrough" -> Passthrough.INSTANCE;
                case "clear" -> Clear.CODEC
                        .codec()
                        .parse(ops, ops.emptyMap())
                        .getOrThrow(error -> new IllegalStateException("clear: " + error));
                case "append_loot" -> new AppendLoot(
                        net.minecraft.resources.ResourceKey.create(
                                Registries.LOOT_TABLE, Identifier.parse(lootTable)));
                case "append_static" -> new AppendStatic(
                        existing == null ? new CompoundTag() : existing.copy());
                // 与现有 nbt 的嵌套键不同，用来区分「递归合并」与「覆盖」。
                // Nested keys differ from the existing tag so merge and overwrite diverge.
                case "append_static_data" -> new AppendStatic(staticData());
                default -> throw new IllegalStateException("unknown modifier " + kind);
            };
            CompoundTag applied = modifier.apply(random, existing);
            out.append("M|").append(String.join("|", p)).append('|')
                    .append(applied == null ? "-" : GSON.toJson(canonical(applied)))
                    .append('\n');
            // 再取一次随机数，证明 `apply` 消耗的取数次数与 Rust 一致。
            // Draw once more, proving `apply` consumed the same number of draws as Rust.
            out.append("N|").append(String.join("|", p)).append('|')
                    .append(random.nextLong()).append('\n');
        }
        System.out.print(out);
        System.out.flush();
    }

    /** `append_static_data` 用的固定 nbt：`{Nested:{A:1}, Extra:7}`。 */
    private static CompoundTag staticData() {
        CompoundTag nested = new CompoundTag();
        nested.putInt("A", 1);
        CompoundTag data = new CompoundTag();
        data.put("Nested", nested);
        data.putInt("Extra", 7);
        return data;
    }

    /**
     * rc 模式：queries 每行一个方块 id。输出官方认为它是不是 `RandomizableContainer`
     * ——直接构造该方块的方块实体再 `instanceof`。这一条钉住
     * `is_randomizable_container` 的类型集合：它决定放置时要不要多取一次 `nextLong`，
     * 错了会让世界随机源序列错开。
     *
     * <p>Randomizable-container mode: one block id per line, building the block entity and
     * testing `instanceof RandomizableContainer`. That predicate decides whether placement
     * draws one extra `nextLong`.
     */
    private static void runRandomizableContainer(RegistryAccess registries, Path queries)
            throws Exception {
        Registry<Block> blocks = registries.lookupOrThrow(Registries.BLOCK);
        StringBuilder out = new StringBuilder();
        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            Identifier id = Identifier.parse(line.trim());
            Block block = blocks.getValue(id);
            if (block == null) {
                throw new IllegalStateException("no block " + id);
            }
            BlockState state = block.defaultBlockState();
            Object be = block instanceof net.minecraft.world.level.block.EntityBlock entityBlock
                    ? entityBlock.newBlockEntity(BlockPos.ZERO, state)
                    : null;
            boolean randomizable = be instanceof net.minecraft.world.RandomizableContainer;
            out.append("R|").append(id).append('|').append(randomizable ? 1 : 0).append('\n');
        }
        System.out.print(out);
        System.out.flush();
    }

    /** 一次 `setBlock`：(位置, 状态)。 */
    private record Write(BlockPos pos, BlockState state) {
    }

    /**
     * 记录型假世界：`setBlock` 只记账，`getBlockState` 返回空气。
     *
     * <p>Recording fake level: `setBlock` only records, `getBlockState` returns air.
     */
    private static final class RecordingLevel {
        private final List<Write> writes = new java.util.ArrayList<>();
        /**
         * 官方在 `blockInfo.nbt != null` 时先写一个 `BARRIER`——这是唯一能从
         * `setBlock` 序列里看出「这个位置带 nbt」的信号。
         *
         * <p>Vanilla writes a `BARRIER` first when `blockInfo.nbt != null`; that is the only
         * signal in the `setBlock` sequence telling us the position carries nbt.
         */
        private final java.util.Set<BlockPos> nbtPositions = new java.util.HashSet<>();
        /** 带 nbt 且有方块实体的位置 → 方块实体（`getBlockEntity` 要返回它）。 */
        private final java.util.Map<BlockPos, Object> entities = new java.util.HashMap<>();
        /** 最终种子（`placeInWorld` 之后从方块实体读回）。 */
        private final java.util.Map<BlockPos, Long> seeds = new java.util.TreeMap<>();
        private final RegistryAccess registries;
        /**
         * 每个位置原本的流体，`0` 表示没有（空气 / 非流体方块）。`1` 是水、`2` 是流动水、
         * `3` 是岩浆。watering 读的是**写方块之前**的流体，所以这份表是模板外世界的
         * 初始状态。
         *
         * <p>Each position's original fluid, `0` for none. `1` water, `2` flowing water,
         * `3` lava. Watering reads the fluid **before** the block is written, so this table
         * is the template's surroundings.
         */
        private final java.util.Map<BlockPos, Integer> fluids = new java.util.HashMap<>();
        /** `getBlockState` 看到的最新状态：每次 `setBlock` 更新，watering 第二段要读它。 */
        private final java.util.Map<BlockPos, BlockState> lastWritten = new java.util.HashMap<>();
        /** `getFluidState` 被问到的次数，用来确认 watering 真的走了。 */
        private int fluidQueries;
        /** `scheduleTick` 被调用的次数。 */
        private int scheduledTicks;
        /** `setBlockAndUpdate` 被调用的次数（`SimpleWaterloggedBlock.placeLiquid` 的写法）。 */
        private int blockAndUpdateCalls;

        RecordingLevel(RegistryAccess registries) {
            this.registries = registries;
        }

        void setFluid(BlockPos pos, int kind) {
            this.fluids.put(pos.immutable(), kind);
        }

        /**
         * `Level.getFluidState`：按表返回水位对应的流体状态。
         *
         * <p>`Level.getFluidState`: the fluid state for the position's table entry.
         */
        private net.minecraft.world.level.material.FluidState fluidAt(BlockPos pos) {
            this.fluidQueries++;
            return switch (this.fluids.getOrDefault(pos, 0)) {
                case 1 -> net.minecraft.world.level.material.Fluids.WATER.getSource(false);
                case 2 -> net.minecraft.world.level.material.Fluids.FLOWING_WATER
                        .defaultFluidState();
                case 3 -> net.minecraft.world.level.material.Fluids.LAVA.getSource(false);
                default -> Blocks.AIR.defaultBlockState().getFluidState();
            };
        }

        /**
         * 收集种子的最终值。只收「带 nbt 且方块实体是随机容器」的位置——这与官方
         * `if (blockInfo.nbt != null) { if (blockEntity instanceof RandomizableContainer) ... }`
         * 完全对应。
         *
         * <p>Collect the final seed values. Only positions that carried nbt and whose block
         * entity is a `RandomizableContainer`, exactly matching vanilla.
         */
        void collectSeeds() {
            for (var entry : this.entities.entrySet()) {
                if (!this.nbtPositions.contains(entry.getKey())) {
                    continue;
                }
                if (entry.getValue() instanceof net.minecraft.world.RandomizableContainer c) {
                    this.seeds.put(entry.getKey(), c.getLootTableSeed());
                }
            }
        }

        net.minecraft.world.level.ServerLevelAccessor proxy() {
            return (net.minecraft.world.level.ServerLevelAccessor)
                    java.lang.reflect.Proxy.newProxyInstance(
                            JigsawOracle.class.getClassLoader(),
                            new Class<?>[] {net.minecraft.world.level.ServerLevelAccessor.class},
                            (self, method, args) -> switch (method.getName()) {
                                case "setBlock" -> {
                                    BlockPos pos = (BlockPos) args[0];
                                    BlockState state = (BlockState) args[1];
                                    this.writes.add(new Write(pos.immutable(), state));
                                    this.lastWritten.put(pos.immutable(), state);
                                    // 写 `BARRIER` 说明这个位置带 nbt；随后那次真方块写入时
                                    // 建方块实体，好让官方的 `getBlockEntity` 返回非 null
                                    // （否则 `LootTableSeed` 的覆盖根本不会执行）。
                                    // Writing `BARRIER` means the position carries nbt; build
                                    // the block entity on the real write so vanilla's
                                    // `getBlockEntity` is non-null and the `LootTableSeed`
                                    // overwrite actually runs.
                                    if (state.is(Blocks.BARRIER)) {
                                        this.nbtPositions.add(pos.immutable());
                                    } else if (this.nbtPositions.contains(pos)
                                            && state.getBlock() instanceof net.minecraft.world.level.block.EntityBlock eb) {
                                        Object be = eb.newBlockEntity(pos.immutable(), state);
                                        if (be != null) {
                                            this.entities.put(pos.immutable(), be);
                                        }
                                    }
                                    yield true;
                                }
                                case "getBlockState" -> {
                                    // watering 的第二段会读**刚写进去**的方块（`getBlockState(pos)`
                                    // 之后 `instanceof LiquidBlockContainer`），所以要先查写入表。
                                    // The second watering pass reads the block just written, so
                                    // consult the write table first.
                                    BlockState written = this.lastWritten.get((BlockPos) args[0]);
                                    yield written == null
                                            ? Blocks.AIR.defaultBlockState()
                                            : written;
                                }
                                case "getFluidState" -> this.fluidAt((BlockPos) args[0]);
                                // `SimpleWaterloggedBlock.placeLiquid` 用 `isClientSide()` 决定
                                // 要不要真的写方块；假世界是服务端，返回 false。
                                // `placeLiquid` gates the write on `isClientSide()`; this fake
                                // level is the server side, so false.
                                case "isClientSide" -> false;
                                // `SimpleWaterloggedBlock.placeLiquid` 走
                                // `level.setBlockAndUpdate(pos, state)` = `setBlock(pos, state, 3)`。
                                // `setBlockAndUpdate` 是 `LevelAccessor` 的默认方法，代理必须自己实现。
                                // `placeLiquid` calls `level.setBlockAndUpdate(pos, state)`, which
                                // is `setBlock(pos, state, 3)`; a proxy must implement the default
                                // method itself.
                                case "setBlockAndUpdate" -> {
                                    this.blockAndUpdateCalls++;
                                    BlockPos pos = (BlockPos) args[0];
                                    BlockState state = (BlockState) args[1];
                                    this.writes.add(new Write(pos.immutable(), state));
                                    this.lastWritten.put(pos.immutable(), state);
                                    yield true;
                                }
                                // `placeLiquid` 的 `scheduleTick` 不改方块，记下即可。
                                // `placeLiquid` also schedules a tick; it changes no block.
                                case "scheduleTick" -> {
                                    this.scheduledTicks++;
                                    yield null;
                                }
                                case "isEmptyBlock" -> true;
                                case "getMinY" -> JigsawOracle.MIN_Y;
                                case "getHeight" ->
                                        method.getParameterCount() == 0 ? 384 : JigsawOracle.MIN_Y + 1;
                                case "getBlockEntity" -> this.entities.get((BlockPos) args[0]);
                                case "registryAccess" -> this.registries;
                                case "getRandom" -> RandomSource.create(0L);
                                case "toString" -> "RecordingLevel";
                                case "hashCode" -> System.identityHashCode(self);
                                case "equals" -> self == args[0];
                                // 其余副作用方法（`updateNeighborsAt` / `blockUpdated` /
                                // `scheduleTick` / `gameEvent` 等）在真实放置里不改变写入的
                                // 方块，本轮只比写入，所以一律当无操作返回 `null`。若某个方法
                                // 该返回非 void，代理会自动装箱默认值，不影响本对拍。
                                // Other side-effecting methods change no written block during
                                // placement, so they are no-ops here; a non-void one gets the
                                // proxy's default value, which is harmless for this oracle.
                                default -> null;
                            });
        }
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

        /** 假 `ServerLevel`：把种子接到 `fakeServerLevelWithSeed`。 */
        private net.minecraft.server.level.ServerLevel fakeServerLevel() {
            return fakeServerLevelWithSeed(this.levelSeed);
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
