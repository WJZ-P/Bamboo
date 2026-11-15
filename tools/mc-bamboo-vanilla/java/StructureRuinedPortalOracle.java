import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.stream.Collectors;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.DoubleTag;
import net.minecraft.nbt.FloatTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.Tag;
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
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;

/**
 * 用官方 26.3 的 {@code RuinedPortalStructure.findGenerationPoint} 对拍废弃传送门的结构件布局
 * （含 {@code findSuitableY} 的整列方块搜索与 {@code Mirror.FRONT_BACK}）。
 *
 * <p>不启动服务端、不读存档、不接受 EULA。需要读官方注册表（结构与噪声设置）和一份真实
 * {@code StructureTemplateManager}，所以这里只做 {@code Bootstrap.bootStrap()} 加
 * {@code RegistryDataLoader}，和 {@code StructureGenerationOracle} 同一套。
 *
 * <p>{@code findGenerationPoint} 是 protected，改用公开的 {@code findValidGenerationPoint}
 * （= {@code findGenerationPoint} 再 {@code filter(isValidBiome)}）。它多一层起点群系过滤，
 * Rust 侧也照做：群系不合法时结构件数记为 0。
 *
 * <p>queries.txt 每行 "{@code structure_id seed cx cz}"。输出每个查询一段：
 * {@code Q id seed cx cz count}，随后若干 {@code P} 行。
 *
 * <p>Compare the ruined portal piece layout against vanilla 26.3, covering
 * {@code findSuitableY}'s full-column scan and {@code Mirror.FRONT_BACK}. No server, save or
 * EULA; official registries and a real {@code StructureTemplateManager} are loaded the same
 * way {@code StructureGenerationOracle} does. {@code findGenerationPoint} is protected, so the
 * public {@code findValidGenerationPoint} is used; the extra start-biome filter is mirrored on
 * the Rust side by reporting zero pieces when the biome is not allowed.
 *
 * <p>Usage: StructureRuinedPortalOracle &lt;queries.txt&gt;
 */
public final class StructureRuinedPortalOracle {
    // createTag 里由基类写的四个键，不进 extras。
    // Four base keys written by StructurePiece.createTag; excluded from extras.
    private static final java.util.Set<String> BASE_KEYS =
            java.util.Set.of("id", "BB", "O", "GD");

    private static final StructurePieceSerializationContext CTX =
            new StructurePieceSerializationContext(null, null, null);

    private static StructureTemplateManager templateManager;

    private static StructureTemplateManager templateManager() {
        if (templateManager == null) {
            try {
                PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
                repository.reload();
                repository.setSelected(List.of("vanilla"));
                MultiPackResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Path temp = Files.createTempDirectory("bamboo-ruined-portal");
                LevelStorageSource.LevelStorageAccess storage =
                        LevelStorageSource.createDefault(temp).createAccess("portal");
                templateManager = new StructureTemplateManager(
                        resources, storage, DataFixers.getDataFixer(), BuiltInRegistries.BLOCK);
            } catch (Exception e) {
                throw new IllegalStateException("cannot build StructureTemplateManager", e);
            }
        }
        return templateManager;
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException(
                    "usage: StructureRuinedPortalOracle <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            run(registries, Path.of(args[0]));
        }
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

    private static void run(RegistryAccess registries, Path queriesPath) throws Exception {
        HolderLookup.RegistryLookup<net.minecraft.world.level.levelgen.synth.NormalNoise> noiseLookup =
                registries.lookupOrThrow(Registries.NOISE);
        Registry<NoiseGeneratorSettings> settingsReg = registries.lookupOrThrow(Registries.NOISE_SETTINGS);
        NoiseGeneratorSettings settings = settingsReg.getValue(Identifier.withDefaultNamespace("overworld"));
        if (settings == null) {
            throw new IllegalStateException("missing minecraft:overworld noise settings");
        }
        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        var biomeSource = generator.getBiomeSource();
        LevelHeightAccessor heightAccessor = LevelHeightAccessor.create(
                settings.noiseSettings().minY(), settings.noiseSettings().height());
        Registry<Structure> structureReg = registries.lookupOrThrow(Registries.STRUCTURE);

        List<String> lines = Files.readAllLines(queriesPath, StandardCharsets.UTF_8);
        Map<Long, RandomState> states = new HashMap<>();
        Map<Long, Climate.Sampler> samplers = new HashMap<>();
        for (String line : lines) {
            if (line.isBlank()) {
                continue;
            }
            long seed = Long.parseLong(line.trim().split("\\s+")[1]);
            if (!states.containsKey(seed)) {
                RandomState state = RandomState.create(noiseLookup, seed, settings);
                states.put(seed, state);
                samplers.put(seed, state.createClimateSampler(
                        SamplerContext.builder().enableCaches().build()));
            }
        }

        StringBuilder out = new StringBuilder();
        for (String line : lines) {
            if (line.isBlank()) {
                continue;
            }
            String[] p = line.trim().split("\\s+");
            String id = p[0];
            long seed = Long.parseLong(p[1]);
            int cx = Integer.parseInt(p[2]);
            int cz = Integer.parseInt(p[3]);
            Structure structure = structureReg.getValue(Identifier.parse(id));
            if (structure == null) {
                throw new IllegalStateException("missing structure " + id);
            }
            RandomState state = states.get(seed);
            Climate.Sampler sampler = samplers.get(seed);
            ChunkPos pos = new ChunkPos(cx, cz);
            Structure.GenerationContext ctx = new Structure.GenerationContext(
                    registries, generator, biomeSource, sampler, state, templateManager(), seed, pos,
                    heightAccessor, structure.biomes()::contains);
            Optional<Structure.GenerationStub> stub = structure.findValidGenerationPoint(ctx);
            List<String> pieces = new ArrayList<>();
            if (stub.isPresent()) {
                for (StructurePiece piece : stub.get().getPiecesBuilder().build().pieces()) {
                    pieces.add(dump(piece));
                }
            }
            out.append("Q ").append(id).append(' ').append(seed).append(' ').append(cx)
                    .append(' ').append(cz).append(' ').append(pieces.size()).append('\n');
            for (String piece : pieces) {
                out.append(piece).append('\n');
            }
        }
        System.out.print(out);
    }

    /** 把一个结构件按统一格式 dump 成一行。 */
    private static String dump(StructurePiece piece) {
        String id = BuiltInRegistries.STRUCTURE_PIECE.getKey(piece.getType()).toString();
        var bb = piece.getBoundingBox();
        Direction orientation = piece.getOrientation();
        int o = orientation == null ? -1 : orientation.get2DDataValue();
        int gd = piece.getGenDepth();
        int rot = piece.getRotation() == null ? -1 : piece.getRotation().ordinal();
        int mirror = piece.getMirror() == null ? -1 : piece.getMirror().ordinal();

        CompoundTag tag = piece.createTag(CTX);
        TreeMap<String, String> extras = new TreeMap<>();
        for (var entry : tag.entrySet()) {
            String key = entry.getKey();
            if (!BASE_KEYS.contains(key)) {
                extras.put(key, value(entry.getValue()));
            }
        }
        String extraToken = extras.isEmpty() ? "-"
                : extras.entrySet().stream()
                        .map(e -> e.getKey() + "=" + e.getValue())
                        .collect(Collectors.joining(","));

        return "P " + id + " " + bb.minX() + " " + bb.minY() + " " + bb.minZ() + " "
                + bb.maxX() + " " + bb.maxY() + " " + bb.maxZ() + " " + o + " " + gd + " "
                + rot + " " + mirror + " " + extraToken;
    }

    /**
     * createTag 里 extras 的值统一成便于 Rust 复现的标准串。
     *
     * <p>compound 写成 {@code {k=v;k=v}}（键排序、用分号分隔，避免和外层的逗号分隔冲突），
     * 浮点按 {@code Float.toString} / {@code Double.toString}，整数走 {@code longValue()}。
     *
     * <p>Canonical string for extras. Compounds are {@code {k=v;k=v}} with sorted keys and
     * semicolons (so they never collide with the comma-joined pair list); floats use
     * {@code Float.toString} / {@code Double.toString}, integers go through {@code longValue()}.
     */
    private static String value(Tag tag) {
        if (tag instanceof CompoundTag compound) {
            TreeMap<String, String> fields = new TreeMap<>();
            for (var entry : compound.entrySet()) {
                fields.put(entry.getKey(), value(entry.getValue()));
            }
            return fields.entrySet().stream()
                    .map(e -> e.getKey() + "=" + e.getValue())
                    .collect(Collectors.joining(";", "{", "}"));
        }
        if (tag instanceof IntArrayTag array) {
            return joinInts(array.getAsIntArray());
        }
        if (tag instanceof ListTag list) {
            StringBuilder sb = new StringBuilder();
            for (Tag element : list) {
                if (!sb.isEmpty()) {
                    sb.append('/');
                }
                sb.append(value(element));
            }
            return sb.toString();
        }
        if (tag instanceof FloatTag floatTag) {
            return Float.toString(floatTag.floatValue());
        }
        if (tag instanceof DoubleTag doubleTag) {
            return Double.toString(doubleTag.doubleValue());
        }
        Optional<Number> number = tag.asNumber();
        if (number.isPresent()) {
            return Long.toString(number.get().longValue());
        }
        Optional<String> string = tag.asString();
        if (string.isPresent()) {
            return string.get();
        }
        return "?";
    }

    private static String joinInts(int[] ints) {
        StringBuilder sb = new StringBuilder();
        for (int value : ints) {
            if (!sb.isEmpty()) {
                sb.append('_');
            }
            sb.append(value);
        }
        return sb.toString();
    }
}
