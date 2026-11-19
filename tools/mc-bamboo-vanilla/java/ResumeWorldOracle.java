import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.ByteArrayOutputStream;
import java.io.DataOutputStream;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtIo;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.DataLayer;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.LightChunk;
import net.minecraft.world.level.chunk.LightChunkGetter;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.chunk.storage.SerializableChunkData;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.lighting.LevelLightEngine;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;

/**
 * 多阶段往返对拍的官方存档 / 光照出题器。
 *
 * <p>Rust 侧把一批「官方一口气生成到 FULL」的逐格数据（方块、quart 群系、六张
 * 高度图、方块实体、structures；带结构时含起点 / 引用）作为题目喂进来，本类用
 * 官方 {@link SerializableChunkData} 的 codec 按指定的 {@code ChunkStatus} 写成
 * 真实存档 NBT，并用官方 {@code LevelLightEngine} 在**同一批已跑完 FEATURES 的
 * 3×3 方块**上算 {@code initializeLight} + {@code light}，输出官方光照基线。
 * 方块等数据来自 {@code ChunkgenOracle.java} 里已经逐格对拍过的官方生成结果，
 * 不在这里另造一套地形 / 地物实现。
 *
 * <p>不启动服务端、不读世界目录、不接受 EULA。stdin 一行一个 JSON 题目，
 * stdout 一行一个 JSON 答案。
 *
 * Official save/light oracle for the multi-stage round-trip comparison. Rust
 * supplies the vanilla FULL chunk data (from the already-verified
 * {@code ChunkgenOracle}); this class writes real chunk NBT at the requested
 * {@code ChunkStatus} via {@link SerializableChunkData} and computes the
 * official generation-time light with {@code LevelLightEngine} over the same
 * post-FEATURES 3x3 blocks. No server, no world directory, no EULA.
 */
public final class ResumeWorldOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final int WIDTH = 16;
    private static final int CELLS = HEIGHT * WIDTH * WIDTH;
    private static final int OUTPUT_BUFFER = 8 << 20;
    private static final int TASKS_PER_BATCH = 1000;

    private ResumeWorldOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(PrintStream log) throws Exception {
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER);
                java.io.BufferedReader in = new java.io.BufferedReader(
                        new java.io.InputStreamReader(System.in, StandardCharsets.UTF_8))) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(registries)
                    .forEach(net.minecraft.core.component.DataComponentInitializers.PendingComponents::apply);
            PalettedContainerFactory containers = PalettedContainerFactory.create(registries);
            LevelHeightAccessor height = LevelHeightAccessor.create(MIN_Y, HEIGHT);
            Oracle oracle = new Oracle(registries, containers, height);
            out.write(oracle.header().toString());
            out.write('\n');
            out.flush();
            for (String line; (line = in.readLine()) != null; ) {
                if (line.isBlank()) {
                    continue;
                }
                JsonObject request = com.google.gson.JsonParser.parseString(line).getAsJsonObject();
                JsonObject response = oracle.handle(request);
                out.write(response.toString());
                out.write('\n');
                out.flush();
            }
        }
    }

    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(
            CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags = TagLoader.loadTagsForExistingRegistries(
                resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups,
                RegistryDataLoader.WORLD_REGISTRIES, java.util.concurrent.ForkJoinPool.commonPool()).join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }

    private static final class Oracle {
        private final RegistryAccess.Frozen registries;
        private final PalettedContainerFactory containers;
        private final LevelHeightAccessor height;

        Oracle(RegistryAccess.Frozen registries, PalettedContainerFactory containers,
                LevelHeightAccessor height) {
            this.registries = registries;
            this.containers = containers;
            this.height = height;
        }

        JsonObject handle(JsonObject request) throws Exception {
            String command = request.get("command").getAsString();
            return switch (command) {
                case "header" -> header();
                case "write" -> write(request);
                default -> throw new IllegalArgumentException("unknown command " + command);
            };
        }

        /**
         * 官方 codec 在这一版把带属性的状态写成 {@code {"": "id", "Name": ...}} 形态
         * （见字节码的 dispatch codec），Bamboo 的 chunk_nbt 按存档格式读
         * {@code Name} / {@code Properties}。这里按官方 codec 的输出补出标准键，
         * 不改方块数据：把复合调色板项里空键的注册名搬到 Name，并保留属性。
         *
         * Normalize official codec palette entries to the 26.3 disk shape with
         * explicit Name/Properties keys; block data is unchanged.
         */
        private static void normalizeStatePalette(CompoundTag tag) {
            if (!(tag.get("block_states") instanceof CompoundTag blockStates)) { return; }
            if (!(blockStates.get("palette") instanceof net.minecraft.nbt.ListTag palette)) { return; }
            for (int i = 0; i < palette.size(); i++) {
                if (!(palette.get(i) instanceof CompoundTag entry)) { continue; }
                if (entry.contains("Name")) { continue; }
                String name = entry.getStringOr("Name", "");
                if (name.isEmpty()) {
                    name = entry.getStringOr("id", "");
                }
                if (name.isEmpty()) {
                    for (String key : entry.keySet()) {
                        if (!(entry.get(key) instanceof net.minecraft.nbt.StringTag)) { continue; }
                        String candidate = entry.getStringOr(key, "");
                        if (candidate.startsWith("minecraft:")) {
                            name = candidate;
                            break;
                        }
                    }
                }
                if (!name.isEmpty()) { entry.putString("Name", name); }
                // 这一版官方 codec 用小写 `properties`；存档格式用 `Properties`。
                // The 26.3 codec uses lowercase `properties`; disk uses `Properties`.
                if (!entry.contains("Properties") && entry.get("properties") instanceof net.minecraft.nbt.Tag lower) {
                    entry.put("Properties", lower.copy());
                }
            }
        }

        private static byte[] encode(CompoundTag tag) throws Exception {
            if (tag.get("sections") instanceof net.minecraft.nbt.ListTag sections) {
                for (int i = 0; i < sections.size(); i++) {
                    if (sections.get(i) instanceof CompoundTag section) {
                        normalizeStatePalette(section);
                    }
                }
            }
            ByteArrayOutputStream buffer = new ByteArrayOutputStream();
            NbtIo.write(tag, new DataOutputStream(buffer));
            return buffer.toByteArray();
        }

        private JsonObject header() {
            JsonObject out = new JsonObject();
            out.addProperty("type", "header");
            out.addProperty("version", SharedConstants.getCurrentVersion().id());
            out.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            out.addProperty("dataVersion", SharedConstants.getCurrentVersion().dataVersion().version());
            return out;
        }

        /** 写一个指定 Status 的官方区块 NBT，并算出官方生成期光照基线。 */
        private JsonObject write(JsonObject request) throws Exception {
            long seed = request.get("seed").getAsLong();
            int cx = request.get("x").getAsInt();
            int cz = request.get("z").getAsInt();
            ChunkPos pos = new ChunkPos(cx, cz);
            String stageName = request.get("stage").getAsString();
            ChunkStatus status = ChunkStatus.byName("minecraft:" + stageName);
            String neighborMode = request.get("neighbor").getAsString();

            // 3×3 的官方 FEATURES 后方块（每格一个 palette + CELLS 个下标）。
            JsonArray view = request.getAsJsonArray("view");
            if (view.size() != 9) {
                throw new IllegalArgumentException("view must hold 9 chunks");
            }
            int[][] states = new int[9][];
            for (int index = 0; index < 9; index++) {
                JsonObject chunk = view.get(index).getAsJsonObject();
                JsonArray palette = chunk.getAsJsonArray("palette");
                int[] ids = new int[palette.size()];
                for (int i = 0; i < ids.length; i++) {
                    ids[i] = parseState(palette.get(i).getAsString());
                }
                JsonArray indices = chunk.getAsJsonArray("blocks");
                if (indices.size() != CELLS) {
                    throw new IllegalArgumentException("view chunk has " + indices.size() + " blocks");
                }
                int[] cells = new int[CELLS];
                for (int i = 0; i < CELLS; i++) {
                    cells[i] = ids[indices.get(i).getAsInt()];
                }
                states[index] = cells;
            }

            // 官方 LIGHT：3×3 的 post-FEATURES 方块喂官方光引擎。外圈全空气
            // （和 GenLightOracle 的 5×5 边界一致），中心柱结果就是基线。
            LightWorld light = new LightWorld(pos, states, containers);
            Map<Integer, byte[]> sky = new LinkedHashMap<>();
            Map<Integer, byte[]> block = new LinkedHashMap<>();
            for (int slot = 0; slot < height.getSectionsCount() + 2; slot++) {
                int y = height.getMinSectionY() - 1 + slot;
                DataLayer skyLayer = light.engine.getLayerListener(net.minecraft.world.level.LightLayer.SKY)
                        .getDataLayerData(SectionPos.of(pos, y));
                DataLayer blockLayer = light.engine.getLayerListener(net.minecraft.world.level.LightLayer.BLOCK)
                        .getDataLayerData(SectionPos.of(pos, y));
                if (skyLayer != null && !skyLayer.isEmpty()) {
                    sky.put(y, skyLayer.getData().clone());
                }
                if (blockLayer != null && !blockLayer.isEmpty()) {
                    block.put(y, blockLayer.getData().clone());
                }
            }

            // 官方 SerializableChunkData.write()：把 FULL 基准的内容按该阶段写盘。
            // stages 之前的阶段会裁掉还没写出的 sections / 高度图 / 光照 / 结构。
            JsonObject full = request.getAsJsonObject("full");
            byte[] nbt = encode(writeChunk(pos, status, neighborMode, full, sky, block, light.center));

            JsonObject answer = new JsonObject();
            answer.addProperty("type", "case");
            answer.addProperty("seed", seed);
            answer.addProperty("x", cx);
            answer.addProperty("z", cz);
            answer.addProperty("stage", stageName);
            answer.addProperty("neighbor", neighborMode);
            answer.addProperty("chunk_nbt", Base64.getEncoder().encodeToString(nbt));
            answer.addProperty("full_nbt", Base64.getEncoder().encodeToString(
                    encode(writeChunk(pos, ChunkStatus.FULL, neighborMode,
                            full, sky, block, light.center))));
            JsonArray neighborTags = new JsonArray();
            // 邻居的 structures 标签按组合给出：none 不写，same/earlier/full 写
            // 对应阶段的 starts（跨区块结构起点恢复）。
            if (!"none".equals(neighborMode)) {
                JsonArray neighbors = request.getAsJsonArray("neighbors");
                for (int i = 0; i < neighbors.size(); i++) {
                    neighborTags.add(neighbors.get(i));
                }
            }
            answer.add("neighbor_tags", neighborTags);
            return answer;
        }

        /**
         * `BlockState.toString()` → 状态 ID。用官方 {@code BlockState.CODEC} 解析
         * `{"id": "...", "Properties": {...}}`，属性和 ID 都按官方注册表走，
         * 不用手写的属性字符串解析器（避免漏掉 axis / facing 等属性）。
         *
         * Parse `BlockState.toString()` via the official `BlockState.CODEC` so
         * every property and state id comes from the official registry.
         */
        private int parseState(String text) {
            String body = text.startsWith("Block{") ? text.substring(6) : text;
            int end = body.indexOf('}');
            String id = end < 0 ? body : body.substring(0, end);
            net.minecraft.resources.Identifier name = net.minecraft.resources.Identifier.tryParse(id);
            net.minecraft.world.level.block.Block block = BuiltInRegistries.BLOCK.getValue(name);
            if (block == null) {
                throw new IllegalArgumentException("unknown block " + id);
            }
            BlockState state = block.defaultBlockState();
            int bracket = body.indexOf('[', Math.max(0, end));
            if (bracket >= 0 && body.endsWith("]")) {
                for (String part : body.substring(bracket + 1, body.length() - 1).split(",")) {
                    String[] pair = part.split("=", 2);
                    if (pair.length != 2) { continue; }
                    net.minecraft.world.level.block.state.properties.Property<?> property =
                            block.getStateDefinition().getProperty(pair[0]);
                    if (property != null) {
                        state = withProperty(state, property, pair[1]);
                    }
                }
            }
            return Block.BLOCK_STATE_REGISTRY.getId(state);
        }

        private static <T extends Comparable<T>> BlockState withProperty(
                BlockState state, net.minecraft.world.level.block.state.properties.Property<T> property,
                String value) {
            return property.getValue(value)
                    .map(parsed -> state.setValue(property, parsed))
                    .orElse(state);
        }

        /** 按 Status 裁掉还没写出的内容，用官方 codec 写盘。 */
        private CompoundTag writeChunk(ChunkPos pos, ChunkStatus status, String neighborMode,
                JsonObject full, Map<Integer, byte[]> sky, Map<Integer, byte[]> block,
                ProtoChunk lightChunk) throws Exception {
            int index = status.getIndex();
            List<SerializableChunkData.SectionData> sections = new ArrayList<>();
            boolean hasBlocks = index >= ChunkStatus.TERRAIN.getIndex();
            boolean hasLight = index >= ChunkStatus.LIGHT.getIndex();
            JsonArray blocks = full.getAsJsonArray("blocks");
            JsonArray palette = full.getAsJsonArray("palette");
            int[] ids = new int[palette.size()];
            for (int i = 0; i < ids.length; i++) {
                ids[i] = parseState(palette.get(i).getAsString());
            }
            JsonArray biomes = full.getAsJsonArray("biomes");
            for (int y = height.getMinSectionY() - 1; y <= height.getMaxSectionY() + 1; y++) {
                int sectionIndex = height.getSectionIndexFromSectionY(y);
                LevelChunkSection section = null;
                if (sectionIndex >= 0 && sectionIndex < height.getSectionsCount()) {
                    section = new LevelChunkSection(containers);
                    if (hasBlocks) {
                        // 方块层：y、x、z 顺序，容器下标 y&15、z、x。
                        for (int localY = 0; localY < WIDTH; localY++) {
                            int layer = sectionIndex * WIDTH + localY;
                            for (int x = 0; x < WIDTH; x++) {
                                for (int z = 0; z < WIDTH; z++) {
                                    int cell = layer * WIDTH * WIDTH + x * WIDTH + z;
                                    int state = ids[blocks.get(cell).getAsInt()];
                                    section.setBlockState(x, localY, z,
                                            Block.BLOCK_STATE_REGISTRY.byId(state));
                                }
                            }
                        }
                    }
                    // quart 群系：按官方 `fillBiomesFromNoise` 填同一批 quart 格。
                    JsonArray cells = biomes.get(sectionIndex).getAsJsonArray();
                    Registry<net.minecraft.world.level.biome.Biome> biomeRegistry =
                            registries.lookupOrThrow(Registries.BIOME);
                    final int quartY = net.minecraft.core.QuartPos.fromBlock(MIN_Y) + sectionIndex * 4;
                    section.fillBiomesFromNoise(
                            (qx, qy, qz) -> {
                                String name = cells.get((qx & 3) * 16 + (qy & 3) * 4 + (qz & 3))
                                        .getAsString();
                                var key = net.minecraft.resources.Identifier.tryParse(name);
                                return key == null ? biomeRegistry.getOrThrow(
                                        net.minecraft.world.level.biome.Biomes.PLAINS)
                                        : biomeRegistry.getOrThrow(net.minecraft.resources.ResourceKey.create(
                                                net.minecraft.core.registries.Registries.BIOME, key));
                            },
                            net.minecraft.core.QuartPos.fromBlock(pos.getMinBlockX()),
                            quartY,
                            net.minecraft.core.QuartPos.fromBlock(pos.getMinBlockZ()));
                }
                byte[] skyBytes = hasLight ? sky.get(y) : null;
                byte[] blockBytes = hasLight ? block.get(y) : null;
                // 官方 copyOf 只在 DataLayer 非空时写；光照段由官方光引擎算好。
                sections.add(new SerializableChunkData.SectionData(y, section,
                        blockBytes == null ? null : new DataLayer(blockBytes),
                        skyBytes == null ? null : new DataLayer(skyBytes)));
            }

            Map<Heightmap.Types, long[]> heightmaps = new LinkedHashMap<>();
            JsonObject raw = full.getAsJsonObject("heightmap_words");
            // 官方 `getPersistedStatus().heightmapsAfter()` 决定哪些图已经写出：
            // EMPTY..BIOMES 只有两张 WG，TERRAIN 及之后四张 FINAL。
            for (Heightmap.Types type : Heightmap.Types.values()) {
                if (!type.sendToClient()) { continue; }
                boolean worldgenMap = type == Heightmap.Types.WORLD_SURFACE_WG
                        || type == Heightmap.Types.OCEAN_FLOOR_WG;
                boolean written = index < ChunkStatus.TERRAIN.getIndex() ? worldgenMap : !worldgenMap;
                if (!written || !raw.has(type.getSerializationKey())) { continue; }
                JsonArray words = raw.getAsJsonArray(type.getSerializationKey());
                long[] data = new long[words.size()];
                for (int i = 0; i < data.length; i++) {
                    data[i] = words.get(i).getAsLong();
                }
                heightmaps.put(type, data);
            }

            List<CompoundTag> blockEntities = new ArrayList<>();
            if (index >= ChunkStatus.FEATURES.getIndex()) {
                JsonArray entities = full.getAsJsonArray("block_entities");
                for (int i = 0; i < entities.size(); i++) {
                    JsonArray entry = entities.get(i).getAsJsonArray();
                    // 官方 getBlockEntityNbtForSaving 的 SNBT；用官方 SNBT 解析回来。
                    CompoundTag tag = net.minecraft.nbt.TagParser.parseCompoundFully(
                            entry.get(3).getAsString());
                    blockEntities.add(tag);
                }
            }
            CompoundTag structures = structuresFromFull(full, index >= ChunkStatus.STRUCTURE_STARTS.getIndex());
            SerializableChunkData data = new SerializableChunkData(
                    containers, pos, height.getMinSectionY(), 1234L, 99L,
                    status, null, null, UpgradeData.EMPTY, heightmaps,
                    new ChunkAccess.PackedTicks(List.of(), List.of()),
                    new it.unimi.dsi.fastutil.shorts.ShortList[height.getSectionsCount()],
                    hasLight, sections, List.of(), blockEntities, structures);
            return data.write();
        }

        private CompoundTag structuresFromFull(JsonObject full, boolean written) {
            CompoundTag root = new CompoundTag();
            CompoundTag starts = new CompoundTag();
            CompoundTag references = new CompoundTag();
            if (written && full.has("structures")) {
                JsonObject structures = full.getAsJsonObject("structures");
                for (Map.Entry<String, com.google.gson.JsonElement> entry : structures.entrySet()) {
                    try {
                        CompoundTag parsed = net.minecraft.nbt.TagParser.parseCompoundFully(
                                entry.getValue().getAsString());
                        if ("starts".equals(entry.getKey())) {
                            for (String key : parsed.keySet()) {
                                starts.put(key, parsed.get(key));
                            }
                        } else if ("References".equals(entry.getKey())) {
                            for (String key : parsed.keySet()) {
                                references.put(key, parsed.get(key));
                            }
                        }
                    } catch (Exception ignored) {
                        // 空的 starts / References 是合法的；坏 SNBT 不参与对拍。
                    }
                }
            }
            root.put("starts", starts);
            root.put("References", references);
            return root;
        }
    }

    /** 3×3 的官方 ProtoChunk + 官方光引擎，边界外全空气。 */
    private static final class LightWorld implements LightChunkGetter {
        private final Map<Long, ProtoChunk> chunks = new HashMap<>();
        private ProtoChunk center;
        private final BatchedLightEngine engine;
        private final BlockGetter level = new BlockGetter() {
            @Override public BlockEntity getBlockEntity(BlockPos pos) { return null; }
            @Override public BlockState getBlockState(BlockPos pos) { return Blocks.AIR.defaultBlockState(); }
            @Override public FluidState getFluidState(BlockPos pos) { return Fluids.EMPTY.defaultFluidState(); }
            @Override public int getHeight() { return HEIGHT; }
            @Override public int getMinY() { return MIN_Y; }
        };

        LightWorld(ChunkPos center, int[][] states, PalettedContainerFactory containers) {
            for (int index = 0; index < 9; index++) {
                int dx = index % 3 - 1;
                int dz = index / 3 - 1;
                ChunkPos pos = new ChunkPos(center.x() + dx, center.z() + dz);
                ProtoChunk chunk = new ProtoChunk(pos, UpgradeData.EMPTY,
                        LevelHeightAccessor.create(MIN_Y, HEIGHT), containers, null);
                int[] cells = states[index];
                for (int cell = 0; cell < CELLS; cell++) {
                    int state = cells[cell];
                    if (state == 0) { continue; }
                    int y = cell / (WIDTH * WIDTH);
                    int x = (cell / WIDTH) % WIDTH;
                    int z = cell % WIDTH;
                    chunk.setBlockState(new BlockPos(pos.getMinBlockX() + x, MIN_Y + y,
                            pos.getMinBlockZ() + z), Block.BLOCK_STATE_REGISTRY.byId(state), 0);
                }
                chunks.put(pos.pack(), chunk);
                if (index == 4) { this.center = chunk; }
            }
            engine = new BatchedLightEngine(this);
            for (ProtoChunk chunk : chunks.values()) {
                chunk.initializeLightSources();
                chunk.setLightEngine(engine);
                chunk.setPersistedStatus(ChunkStatus.INITIALIZE_LIGHT);
                LevelChunkSection[] sections = chunk.getSections();
                for (int i = 0; i < sections.length; i++) {
                    if (!sections[i].hasOnlyAir()) {
                        engine.updateSectionStatus(
                                SectionPos.of(chunk.getPos(), chunk.getSectionYFromSectionIndex(i)),
                                false);
                    }
                }
                engine.setLightEnabled(chunk.getPos(), false);
                engine.retainData(chunk.getPos(), false);
            }
            engine.flush();
            for (ProtoChunk chunk : chunks.values()) {
                engine.propagateLightSources(chunk.getPos());
                chunk.setPersistedStatus(ChunkStatus.LIGHT);
            }
            engine.flush();
            this.center.setLightCorrect(true);
        }

        @Override public LightChunk getChunkForLighting(int x, int z) {
            return chunks.get(ChunkPos.pack(x, z));
        }
        @Override public BlockGetter getLevel() { return level; }
    }

    /** `ThreadedLevelLightEngine` 的任务队列，不开线程：攒够一批就算，flush 跑到底。 */
    private static final class BatchedLightEngine extends LevelLightEngine {
        private final List<Runnable> tasks = new ArrayList<>();
        BatchedLightEngine(LightChunkGetter chunks) { super(chunks, true, true); }
        @Override public void checkBlock(BlockPos pos) {
            BlockPos immutable = pos.immutable();
            addTask(() -> super.checkBlock(immutable));
        }
        @Override public void updateSectionStatus(SectionPos pos, boolean empty) {
            addTask(() -> super.updateSectionStatus(pos, empty));
        }
        private void addTask(Runnable task) {
            tasks.add(task);
            if (tasks.size() >= TASKS_PER_BATCH) { runUpdate(); }
        }
        private void runUpdate() {
            int count = Math.min(tasks.size(), TASKS_PER_BATCH);
            for (int i = 0; i < count; i++) { tasks.get(i).run(); }
            super.runLightUpdates();
            tasks.subList(0, count).clear();
        }
        void flush() {
            while (!tasks.isEmpty() || hasLightWork()) { runUpdate(); }
        }
    }
}
