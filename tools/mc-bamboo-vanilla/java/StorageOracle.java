import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.Tag;
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
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.storage.RegionFile;
import net.minecraft.world.level.chunk.storage.RegionFileVersion;
import net.minecraft.world.level.chunk.storage.RegionStorageInfo;
import net.minecraft.world.level.chunk.storage.SerializableChunkData;
import net.minecraft.world.level.levelgen.Heightmap;

/**
 * 用官方的区块 NBT 和 Anvil 类检查 Bamboo 的存档编解码层。
 *
 * <p>不启动服务端，不接受 EULA。题目从 stdin 读一行一个 JSON，输出一行一个 JSON。
 * 命令：
 * <ul>
 *   <li>{@code chunk-read}：给一段 Bamboo 写出的未压缩 NBT（base64），用
 *       {@code SerializableChunkData.parse} 读，再 {@code write()} 写回；两边都序列化成
 *       按键排序的规范 JSON，交给 Rust 侧比较。</li>
 *   <li>{@code chunk-write}：从官方的 {@link SerializableChunkData} 造一段区块 NBT，
 *       返回未压缩字节（base64）和它的规范 JSON。</li>
 *   <li>{@code region}：给一个目录和命令列表，用官方 {@code RegionFile} 读写区块，
 *       返回读到的字节（base64）。</li>
 * </ul>
 */
public final class StorageOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final int OUTPUT_BUFFER = 4 << 20;

    private StorageOracle() {
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
        // 官方日志默认写 stdout，要抢在日志框架初始化之前换掉，否则日志会混进 JSON。
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            PalettedContainerFactory containers = PalettedContainerFactory.create(registries);
            LevelHeightAccessor height = LevelHeightAccessor.create(MIN_Y, HEIGHT);
            new Oracle(containers, height, registries, out).run();
        }
    }

    /** 和 LightOracle 一样，只加载世界层注册表，不构造 ServerLevel。 */
    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(CloseableResourceManager resources) {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
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

    private static final class Oracle {
        private final PalettedContainerFactory containers;
        private final LevelHeightAccessor height;
        private final RegistryAccess registries;
        private final Writer out;
        private final java.io.BufferedReader in;

        Oracle(PalettedContainerFactory containers, LevelHeightAccessor height, RegistryAccess registries, Writer out) {
            this.containers = containers;
            this.height = height;
            this.registries = registries;
            this.out = out;
            this.in = new java.io.BufferedReader(new java.io.InputStreamReader(System.in, StandardCharsets.UTF_8));
        }

        void run() throws Exception {
            String line;
            while ((line = in.readLine()) != null) {
                if (line.isBlank()) {
                    continue;
                }
                JsonObject request = com.google.gson.JsonParser.parseString(line).getAsJsonObject();
                JsonObject response = handle(request);
                out.write(response.toString());
                out.write('\n');
                out.flush();
            }
        }

        private JsonObject handle(JsonObject request) throws Exception {
            String command = request.get("command").getAsString();
            JsonObject response = new JsonObject();
            response.addProperty("command", command);
            switch (command) {
                case "chunk-read" -> {
                    byte[] bytes = Base64.getDecoder().decode(request.get("nbt").getAsString());
                    CompoundTag root = NbtIo.read(new DataInputStream(new ByteArrayInputStream(bytes)));
                    SerializableChunkData parsed = SerializableChunkData.parse(height, containers, root);
                    if (parsed == null) {
                        response.addProperty("error", "official parse returned null");
                        break;
                    }
                    CompoundTag written = parsed.write();
                    response.add("original", canonical(root));
                    response.add("written", canonical(written));
                    response.addProperty("writtenNbt", encode(written));
                    response.addProperty("status", parsed.chunkStatus().getName());
                    response.addProperty("sections", parsed.sectionData().size());
                }
                case "chunk-write" -> {
                    CompoundTag generated = makeChunk(request);
                    response.addProperty("nbt", encode(generated));
                    response.add("canonical", canonical(generated));
                }
                case "region-write" -> {
                    Path dir = Path.of(request.get("dir").getAsString());
                    Files.createDirectories(dir);
                    int x = request.get("x").getAsInt();
                    int z = request.get("z").getAsInt();
                    int id = request.get("compression").getAsInt();
                    RegionFileVersion version = RegionFileVersion.fromId(id);
                    Path path = dir.resolve("r." + (x >> 5) + "." + (z >> 5) + ".mca");
                    RegionStorageInfo info = new RegionStorageInfo("oracle", Level.OVERWORLD, "region");
                    try (RegionFile region = new RegionFile(info, path, dir, version, false)) {
                        for (var entry : request.getAsJsonArray("chunks")) {
                            JsonObject chunk = entry.getAsJsonObject();
                            ChunkPos pos = new ChunkPos(chunk.get("x").getAsInt(), chunk.get("z").getAsInt());
                            byte[] data = Base64.getDecoder().decode(chunk.get("data").getAsString());
                            try (DataOutputStream stream = region.getChunkDataOutputStream(pos)) {
                                stream.write(data);
                            }
                        }
                        region.flush();
                    }
                    response.addProperty("path", path.toString());
                }
                case "region-read" -> {
                    Path dir = Path.of(request.get("dir").getAsString());
                    int x = request.get("x").getAsInt();
                    int z = request.get("z").getAsInt();
                    Path path = dir.resolve("r." + (x >> 5) + "." + (z >> 5) + ".mca");
                    RegionStorageInfo info = new RegionStorageInfo("oracle", Level.OVERWORLD, "region");
                    JsonArray chunks = new JsonArray();
                    try (RegionFile region = new RegionFile(info, path, dir, false)) {
                        for (var entry : request.getAsJsonArray("positions")) {
                            JsonObject wanted = entry.getAsJsonObject();
                            ChunkPos pos = new ChunkPos(wanted.get("x").getAsInt(), wanted.get("z").getAsInt());
                            JsonObject found = new JsonObject();
                            found.addProperty("x", pos.x());
                            found.addProperty("z", pos.z());
                            found.addProperty("exists", region.hasChunk(pos));
                            try (DataInputStream stream = region.getChunkDataInputStream(pos)) {
                                if (stream != null) {
                                    found.addProperty("data", Base64.getEncoder().encodeToString(stream.readAllBytes()));
                                }
                            }
                            chunks.add(found);
                        }
                    }
                    response.add("chunks", chunks);
                }
                case "nbt-io" -> {
                    // NbtIo.writeCompressed 写出的 gzip 文件，Bamboo 要能读。
                    CompoundTag tag = new CompoundTag();
                    tag.putInt("DataVersion", SharedConstants.getCurrentVersion().dataVersion().version());
                    tag.putString("LevelName", "竹 bamboo");
                    tag.putLongArray("words", new long[] {-1, 0, Long.MAX_VALUE});
                    ByteArrayOutputStream buffer = new ByteArrayOutputStream();
                    NbtIo.writeCompressed(tag, buffer);
                    response.addProperty("gzip", Base64.getEncoder().encodeToString(buffer.toByteArray()));
                    response.add("canonical", canonical(tag));
                }
                default -> response.addProperty("error", "unknown command " + command);
            }
            return response;
        }

        /** 从 JSON 描述造一个官方的 SerializableChunkData，再 write()。 */
        private CompoundTag makeChunk(JsonObject request) throws IOException {
            int x = request.get("x").getAsInt();
            int z = request.get("z").getAsInt();
            ChunkPos pos = new ChunkPos(x, z);
            Registry<Biome> biomes = registries.lookupOrThrow(Registries.BIOME);
            var plains = biomes.getOrThrow(net.minecraft.world.level.biome.Biomes.PLAINS);

            List<SerializableChunkData.SectionData> sections = new ArrayList<>();
            int minLightSection = height.getMinSectionY() - 1;
            int maxLightSection = height.getMaxSectionY() + 1;
            for (int y = minLightSection; y <= maxLightSection; y++) {
                int sectionIndex = height.getSectionIndexFromSectionY(y);
                LevelChunkSection section = null;
                if (sectionIndex >= 0 && sectionIndex < height.getSectionsCount()) {
                    section = new LevelChunkSection(containers);
                    JsonObject spec = sectionSpec(request, y);
                    for (var entry : spec.getAsJsonArray("blocks")) {
                        JsonObject block = entry.getAsJsonObject();
                        // 题目里的状态用注册名 + 属性描述，交给官方的 BlockState.CODEC 解析。
                        var parsed = BlockState.CODEC.parse(net.minecraft.nbt.NbtOps.INSTANCE,
                                tagFrom(block.getAsJsonObject("state")));
                        BlockState state = parsed.getOrThrow(msg -> new IOException("bad state: " + msg));
                        int bx = block.get("x").getAsInt();
                        int by = block.get("y").getAsInt();
                        int bz = block.get("z").getAsInt();
                        for (int cx = 0; cx < 16; cx++) {
                            section.setBlockState(cx, by, bz, state);
                        }
                        if (bx >= 0) {
                            section.setBlockState(bx, by, bz, state);
                        }
                    }
                    // 让方块调色板不只是单值。
                    section.setBlockState(0, 0, 0, net.minecraft.world.level.block.Blocks.GRASS_BLOCK.defaultBlockState());
                }
                byte[] sky = null;
                byte[] blockLight = null;
                JsonObject spec = sectionSpec(request, y);
                if (spec.has("sky")) {
                    sky = decodeBytes(spec.get("sky").getAsString());
                }
                if (spec.has("block")) {
                    blockLight = decodeBytes(spec.get("block").getAsString());
                }
                sections.add(new SerializableChunkData.SectionData(
                        y,
                        section,
                        blockLight == null ? null : new net.minecraft.world.level.chunk.DataLayer(blockLight),
                        sky == null ? null : new net.minecraft.world.level.chunk.DataLayer(sky)));
            }

            Map<Heightmap.Types, long[]> heightmaps = new LinkedHashMap<>();
            for (Heightmap.Types type : Heightmap.Types.values()) {
                if (type.sendToClient()) {
                    long[] words = new long[37];
                    for (int i = 0; i < words.length; i++) {
                        words[i] = i % 4;
                    }
                    heightmaps.put(type, words);
                }
            }
            CompoundTag structures = new CompoundTag();
            structures.put("starts", new CompoundTag());
            structures.put("References", new CompoundTag());
            CompoundTag blank = new CompoundTag();
            blank.putString("id", "minecraft:chest");
            blank.putInt("x", x * 16 + 3);
            blank.putInt("y", -60);
            blank.putInt("z", z * 16 + 5);
            blank.put("Items", new net.minecraft.nbt.ListTag());

            SerializableChunkData data = new SerializableChunkData(
                    containers,
                    pos,
                    height.getMinSectionY(),
                    1234L,
                    99L,
                    net.minecraft.world.level.chunk.status.ChunkStatus.FULL,
                    null,
                    null,
                    net.minecraft.world.level.chunk.UpgradeData.EMPTY,
                    heightmaps,
                    new net.minecraft.world.level.chunk.ChunkAccess.PackedTicks(List.of(), List.of()),
                    new it.unimi.dsi.fastutil.shorts.ShortList[height.getSectionsCount()],
                    request.get("lightOn").getAsBoolean(),
                    sections,
                    List.of(),
                    request.get("blockEntities").getAsBoolean() ? List.of(blank) : List.of(),
                    structures);
            return data.write();
        }

        private JsonObject sectionSpec(JsonObject request, int y) {
            for (var entry : request.getAsJsonArray("sections")) {
                JsonObject spec = entry.getAsJsonObject();
                if (spec.get("y").getAsInt() == y) {
                    return spec;
                }
            }
            JsonObject empty = new JsonObject();
            empty.addProperty("y", y);
            empty.add("blocks", new JsonArray());
            return empty;
        }

        private static byte[] decodeBytes(String base64) {
            return Base64.getDecoder().decode(base64);
        }

        private static CompoundTag tagFrom(JsonObject object) {
            CompoundTag tag = new CompoundTag();
            for (var entry : object.entrySet()) {
                tag.putString(entry.getKey(), entry.getValue().getAsString());
            }
            return tag;
        }

        private static String encode(CompoundTag tag) throws IOException {
            ByteArrayOutputStream buffer = new ByteArrayOutputStream();
            NbtIo.write(tag, new DataOutputStream(buffer));
            return Base64.getEncoder().encodeToString(buffer.toByteArray());
        }

        /** CompoundTag 用 HashMap，键序不确定：转成按键排序的规范 JSON 再比。 */
        private static JsonObject canonical(Tag tag) {
            JsonObject out = new JsonObject();
            out.addProperty("type", tag.getId());
            switch (tag.getId()) {
                case 1, 2, 3, 4, 5, 6 -> out.addProperty("value", String.valueOf(((net.minecraft.nbt.NumericTag) tag).box()));
                case 8 -> out.addProperty("value", ((net.minecraft.nbt.StringTag) tag).value());
                case 7 -> {
                    JsonArray values = new JsonArray();
                    for (byte value : ((net.minecraft.nbt.ByteArrayTag) tag).getAsByteArray()) {
                        values.add((int) value);
                    }
                    out.add("values", values);
                }
                case 11 -> {
                    JsonArray values = new JsonArray();
                    for (int value : ((net.minecraft.nbt.IntArrayTag) tag).getAsIntArray()) {
                        values.add(value);
                    }
                    out.add("values", values);
                }
                case 12 -> {
                    JsonArray values = new JsonArray();
                    for (long value : ((net.minecraft.nbt.LongArrayTag) tag).getAsLongArray()) {
                        values.add(value);
                    }
                    out.add("values", values);
                }
                case 9 -> {
                    JsonArray values = new JsonArray();
                    for (Tag value : (net.minecraft.nbt.ListTag) tag) {
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
    }
}
