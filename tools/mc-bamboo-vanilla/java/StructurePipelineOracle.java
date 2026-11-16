import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.SharedConstants;
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
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;
import java.lang.reflect.Method;
import java.lang.reflect.Field;
import java.util.Set;
import java.util.HashSet;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureSet;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;
import net.minecraft.util.StaticCache2D;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.core.registries.BuiltInRegistries;

/**
 * 官方 `ChunkGenerator.createStructures` / `createReferences` 的端到端对拍：在 JVM 里
 * 按 Director 的同一条路径造结构起点、引用和 `structures` 标签，输出逐区块的起点 /
 * 引用 / 结构件清单和中心区块的方块。不启动服务端、不读存档、不接受 EULA。
 *
 * <p>Usage: StructurePipelineOracle &lt;queries.txt&gt; [allowed-type,allowed-type,...]
 */
public final class StructurePipelineOracle {
    private static final Writer OUT = new OutputStreamWriter(
            new PrintStream(new FileOutputStream(FileDescriptor.out)), StandardCharsets.UTF_8);

    public static void main(String[] args) throws Exception {
        if (args.length < 1 || args.length > 2) {
            throw new IllegalArgumentException("usage: StructurePipelineOracle <queries.txt> [types]");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            run(registries, resources, Path.of(args[0]),
                    args.length == 2 ? args[1] : "");
        }
        OUT.flush();
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

    private static StructureTemplateManager templateManager(CloseableResourceManager resources)
            throws Exception {
        Path temp = Files.createTempDirectory("bamboo-structure-oracle");
        LevelStorageSource.LevelStorageAccess storage =
                LevelStorageSource.createDefault(temp).createAccess("struct");
        return new StructureTemplateManager(
                resources, storage, DataFixers.getDataFixer(), BuiltInRegistries.BLOCK);
    }

    private static void run(RegistryAccess registries, CloseableResourceManager resources,
            Path queries, String allowedText) throws Exception {
        HolderLookup.RegistryLookup<net.minecraft.world.level.levelgen.synth.NormalNoise> noiseLookup =
                registries.lookupOrThrow(Registries.NOISE);
        Registry<NoiseGeneratorSettings> settingsReg =
                registries.lookupOrThrow(Registries.NOISE_SETTINGS);
        NoiseGeneratorSettings settings = settingsReg.getValue(Identifier.withDefaultNamespace("overworld"));
        Registry<Structure> structureReg = registries.lookupOrThrow(Registries.STRUCTURE);
        HolderLookup<StructureSet> setLookup = registries.lookupOrThrow(Registries.STRUCTURE_SET);
        StructureTemplateManager templates = templateManager(resources);
        StructurePieceSerializationContext pieceContext =
                new StructurePieceSerializationContext(resources, registries, templates);

        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        BiomeSource biomeSource = generator.getBiomeSource();
        LevelHeightAccessor height = LevelHeightAccessor.create(
                settings.noiseSettings().minY(), settings.noiseSettings().height());

        JsonObject header = new JsonObject();
        header.addProperty("type", "header");
        header.addProperty("version", SharedConstants.getCurrentVersion().id());
        header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
        header.addProperty("allowed", allowedText);
        emit(header);

        for (String line : Files.readAllLines(queries, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] parts = line.trim().split("\\s+");
            long seed = Long.parseLong(parts[0]);
            int cx = Integer.parseInt(parts[1]);
            int cz = Integer.parseInt(parts[2]);
            ChunkPos center = new ChunkPos(cx, cz);
            RandomState state = RandomState.create(noiseLookup, seed, settings);
            ChunkGeneratorStructureState structureState =
                    ChunkGeneratorStructureState.createForNormal(
                            state, seed, new ChunkPos(0, 0), biomeSource, setLookup);
            structureState.ensureStructuresGenerated();

            // 17×17 邻域：每个区块跑官方 createStructures，再给中心跑 createReferences。
            ProtoChunk[][] grid = new ProtoChunk[17][17];
            WorldOptions options = new WorldOptions(seed, true, false);
            for (int dx = -8; dx <= 8; dx++) {
                for (int dz = -8; dz <= 8; dz++) {
                    ChunkPos pos = new ChunkPos(cx + dx, cz + dz);
                    ProtoChunk chunk = new ProtoChunk(pos, UpgradeData.EMPTY, height,
                            PalettedContainerFactory.create(registries), null);
                    StructureManager manager = new StructureManager(
                            nullLevel(registries, chunk), options, null);
                    generator.createStructures(registries, structureState, manager, chunk,
                            templates, net.minecraft.resources.ResourceKey.create(
                                    net.minecraft.core.registries.Registries.DIMENSION,
                                    Identifier.withDefaultNamespace("overworld")));
                    grid[dx + 8][dz + 8] = chunk;
                }
            }
            ProtoChunk centerChunk = grid[8][8];
            WorldGenLevel region = regionOf(center, centerChunk, grid);
            StructureManager centerManager = new StructureManager(region, options, null);
            generator.createReferences(region, centerManager, centerChunk);

            JsonObject out = new JsonObject();
            out.addProperty("type", "structures");
            out.addProperty("seed", seed);
            out.addProperty("x", cx);
            out.addProperty("z", cz);
            JsonArray starts = new JsonArray();
            List<String> names = new ArrayList<>();
            for (Map.Entry<Structure, StructureStart> entry
                    : centerChunk.getAllStarts().entrySet()) {
                names.add(structureReg.getKey(entry.getKey()).toString());
            }
            names.sort(Comparator.naturalOrder());
            for (String name : names) {
                Structure key = structureReg.getValue(Identifier.parse(name));
                StructureStart start = centerChunk.getAllStarts().get(key);
                if (start == null || !start.isValid()) {
                    continue;
                }
                JsonArray row = new JsonArray();
                row.add(name);
                row.add(start.getPieces().size());
                starts.add(row);
            }
            out.add("starts", starts);
            JsonArray refs = new JsonArray();
            List<String> refNames = new ArrayList<>();
            for (Structure key : centerChunk.getAllReferences().keySet()) {
                refNames.add(structureReg.getKey(key).toString());
            }
            refNames.sort(Comparator.naturalOrder());
            for (String name : refNames) {
                Structure key = structureReg.getValue(Identifier.parse(name));
                var keys = centerChunk.getAllReferences().get(key);
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
            out.add("references", refs);
            // 结构件清单（含包围盒）也输出，方便 Rust 侧逐项核对。
            JsonArray pieces = new JsonArray();
            for (String name : names) {
                StructureStart start = centerChunk.getAllStarts()
                        .get(structureReg.getValue(Identifier.parse(name)));
                if (start == null || !start.isValid()) {
                    continue;
                }
                for (var piece : start.getPieces()) {
                    JsonArray row = new JsonArray();
                    row.add(name);
                    row.add(BuiltInRegistries.STRUCTURE_PIECE
                            .getKey(piece.getType()).toString());
                    row.add(piece.getBoundingBox().minX());
                    row.add(piece.getBoundingBox().minY());
                    row.add(piece.getBoundingBox().minZ());
                    row.add(piece.getBoundingBox().maxX());
                    row.add(piece.getBoundingBox().maxY());
                    row.add(piece.getBoundingBox().maxZ());
                    pieces.add(row);
                }
            }
            out.add("pieces", pieces);
            // 官方 `createNoiseChunk`：从中心区块的 References 反查起点，造真实
            // Beardifier 注入 finalDensity，再跑到 CARVERS，输出中心区块逐方块。
            // Vanilla `createNoiseChunk`: resolve referenced starts, build the real
            // Beardifier, run to CARVERS and emit the center chunk's blocks.
            Beardifier beardifier = Beardifier.forStructuresInChunk(centerManager, center);
            // 3×3 的 quart 群系格要先填好（buildSurface / carvers 的 biomeManager 要读）。
            for (int dx = -1; dx <= 1; dx++) {
                for (int dz = -1; dz <= 1; dz++) {
                    ProtoChunk p = grid[dx + 8][dz + 8];
                    generator.createBiomes(state, Blender.empty(), null, p).join();
                    p.setPersistedStatus(ChunkStatus.BIOMES);
                }
            }
            ProtoChunk terrain = grid[8][8];
            emitTerrainBlocks(seed, cx, cz, terrain, generator, settings, state,
                    beardifier, grid, center, resizeResolver(state, biomeSource));
            emit(out);
        }
        JsonObject end = new JsonObject();
        end.addProperty("type", "end");
        emit(end);
    }

    private static void emit(JsonObject line) throws Exception {
        OUT.write(line.toString());
        OUT.write('\n');
    }

    /** 给 terrain 用的真 `WorldGenRegion`：17×17 缓存 + 生成步骤的写半径。 */
    private static net.minecraft.server.level.WorldGenRegion terrainRegion(
            ChunkPos center, ProtoChunk centerChunk, ProtoChunk[][] grid,
            long seed, RandomState state, BiomeSource source) throws Exception {
        StaticCache2D<ChunkAccess> cache = StaticCache2D.create(center.x(), center.z(), 8,
                (x, z) -> grid[x - center.x() + 8][z - center.z() + 8]);
        java.util.List<ChunkStatus> deps = new java.util.ArrayList<>();
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
        net.minecraft.server.level.WorldGenRegion region =
                (net.minecraft.server.level.WorldGenRegion) allocateRegion();
        setRegionField(region, "cache", cache);
        setRegionField(region, "center", centerChunk);
        setRegionField(region, "generatingStep", step);
        BiomeResolver resolver = source.createUncachedResolver(state);
        setRegionField(region, "uncachedBiomeResolver", resolver);
        setRegionField(region, "biomeManager",
                new BiomeManager(region, BiomeManager.obfuscateSeed(seed)));
        return region;
    }

    private static Object allocateRegion() {
        try {
            java.lang.reflect.Field f = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
            f.setAccessible(true);
            sun.misc.Unsafe u = (sun.misc.Unsafe) f.get(null);
            return u.allocateInstance(net.minecraft.server.level.WorldGenRegion.class);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
    }

    private static void setRegionField(Object target, String name, Object value) {
        try {
            java.lang.reflect.Field f =
                    net.minecraft.server.level.WorldGenRegion.class.getDeclaredField(name);
            f.setAccessible(true);
            f.set(target, value);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
    }

    private static BiomeSource biomeSourceOf(NoiseBasedChunkGenerator generator) {
        return generator.getBiomeSource();
    }

    private static BiomeResolver resizeResolver(RandomState state, BiomeSource source) {
        return source.createUncachedResolver(state);
    }

    /** 造一个 `NoiseChunk`（带真实 Beardifier）并跑到 CARVERS，输出中心区块方块。 */
    private static void emitTerrainBlocks(long seed, int cx, int cz, ProtoChunk center,
            NoiseBasedChunkGenerator generator, NoiseGeneratorSettings settings,
            RandomState state, Beardifier beardifier, ProtoChunk[][] grid, ChunkPos pos,
            BiomeResolver resolver) throws Exception {
        // 与 ChunkgenOracle.buildRealTerrain 一样造 17×17 邻域 + WorldGenRegion。
        net.minecraft.server.level.WorldGenRegion region =
                terrainRegion(pos, center, grid, seed, state, biomeSourceOf(generator));
        Set<Holder<Biome>> possible = new HashSet<>();
        for (int z = pos.z() - 1; z <= pos.z() + 1; z++) {
            for (int x = pos.x() - 1; x <= pos.x() + 1; x++) {
                region.getChunk(x, z).collectBiomesInPalette(possible);
            }
        }
        DensityVolume volume = new DensityVolume(16, settings.noiseSettings().height(), 16,
                pos.getMinBlockX(), settings.noiseSettings().minY(), pos.getMinBlockZ());
        Method doFill = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                "doFill", NoiseChunk.class, ChunkAccess.class);
        doFill.setAccessible(true);
        Method buildSurface = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                "buildSurface", ChunkAccess.class, NoiseChunk.class, RandomState.class,
                BiomeManager.class, Set.class, MaterialRule.class);
        buildSurface.setAccessible(true);
        Method generateCarvers = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                "generateCarvers", ChunkAccess.class, Blender.class, NoiseChunk.class,
                RandomState.class, BiomeManager.class,
                net.minecraft.server.level.WorldGenRegion.class, MaterialRule.class);
        generateCarvers.setAccessible(true);
        Field pickerField = NoiseBasedChunkGenerator.class.getDeclaredField("globalFluidPicker");
        pickerField.setAccessible(true);
        @SuppressWarnings("unchecked")
        java.util.function.Supplier<Aquifer.FluidPicker> picker =
                (java.util.function.Supplier<Aquifer.FluidPicker>) pickerField.get(generator);
        try (NoiseChunk noiseChunk = new NoiseChunk(state, beardifier, settings, picker.get(),
                Blender.empty(), volume)) {
            doFill.invoke(generator, noiseChunk, center);
            buildSurface.invoke(generator, center, noiseChunk, state, region.getBiomeManager(),
                    possible, settings.materialRule().value());
            generateCarvers.invoke(generator, center, Blender.empty(), noiseChunk, state,
                    region.getBiomeManager(), region, settings.materialRule().value());
        }
        center.setPersistedStatus(ChunkStatus.TERRAIN);
        Heightmap.primeHeightmaps(center, ChunkStatus.FINAL_HEIGHTMAPS);
        JsonObject line = new JsonObject();
        line.addProperty("type", "terrain");
        line.addProperty("seed", seed);
        line.addProperty("x", cx);
        line.addProperty("z", cz);
        addBlocks(line, center);
        emit(line);
    }

    /** palette + blocks，顺序 y、x、z；和 gen-world 的 CARVERS 行同格式。 */
    private static void addBlocks(JsonObject line, ChunkAccess chunk) {
        java.util.List<BlockState> palette = new java.util.ArrayList<>();
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

    /** 只实现 createStructures 需要的 `LevelAccessor` 表面：当前区块与注册表访问。 */
    private static net.minecraft.world.level.LevelAccessor nullLevel(
            RegistryAccess registries, ProtoChunk chunk) {
        return (net.minecraft.world.level.LevelAccessor) java.lang.reflect.Proxy.newProxyInstance(
                StructurePipelineOracle.class.getClassLoader(),
                new Class<?>[] {net.minecraft.world.level.LevelAccessor.class},
                (proxy, method, args) -> {
                    switch (method.getName()) {
                        case "registryAccess": return registries;
                        case "getChunk": return chunk;
                        case "hasChunk": return Boolean.TRUE;
                        case "getSeed": return 0L;
                        case "getMinY": return chunk.getMinY();
                        case "getHeight": return chunk.getHeight();
                        case "getMaxY": return chunk.getMaxY();
                        case "getBlockState":
                            return net.minecraft.world.level.block.Blocks.AIR.defaultBlockState();
                        case "getFluidState":
                            return net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
                        case "hashCode": return System.identityHashCode(proxy);
                        case "equals": return proxy == args[0];
                        case "toString": return "NullLevel";
                        default:
                            if (method.getReturnType() == boolean.class) return false;
                            if (method.getReturnType() == int.class) return 0;
                            if (method.getReturnType() == long.class) return 0L;
                            return null;
                    }
                });
    }

    /** 17×17 的 `WorldGenLevel` 代理：`createReferences` 只用 `getChunk(x,z)`。 */
    private static WorldGenLevel regionOf(ChunkPos center, ProtoChunk centerChunk,
            ProtoChunk[][] grid) {
        return (WorldGenLevel) java.lang.reflect.Proxy.newProxyInstance(
                StructurePipelineOracle.class.getClassLoader(),
                new Class<?>[] {WorldGenLevel.class},
                (proxy, method, args) -> {
                    switch (method.getName()) {
                        case "getChunk": {
                            int x, z;
                            if (args[0] instanceof net.minecraft.core.BlockPos pos) {
                                x = pos.getX() >> 4;
                                z = pos.getZ() >> 4;
                            } else {
                                x = (Integer) args[0];
                                z = (Integer) args[1];
                            }
                            int dx = x - center.x();
                            int dz = z - center.z();
                            if (dx < -8 || dx > 8 || dz < -8 || dz > 8) {
                                throw new IllegalStateException("outside 17x17: " + x + "," + z);
                            }
                            return grid[dx + 8][dz + 8];
                        }
                        case "hasChunk": return Boolean.TRUE;
                        case "getMinY": return centerChunk.getMinY();
                        case "getHeight": return centerChunk.getHeight();
                        case "getMaxY": return centerChunk.getMaxY();
                        case "getMinSectionY": return centerChunk.getMinSectionY();
                        case "getMaxSectionY": return centerChunk.getMaxSectionY();
                        case "hashCode": return System.identityHashCode(proxy);
                        case "equals": return proxy == args[0];
                        case "toString": return "StructureRegion";
                        default:
                            if (method.getReturnType() == boolean.class) return false;
                            if (method.getReturnType() == int.class) return 0;
                            if (method.getReturnType() == long.class) return 0L;
                            return null;
                    }
                });
    }

}
