// POI 对拍：不启动服务端、不接受 EULA、不读写真实世界目录。
//
// 用官方 `PoiManager`（构造在临时目录里）跑一组方块放置 / 拆除序列：
// 每次变化走 `PoiTypes.forState(old)` / `forState(new)`，两者相等就返回，否则
// `remove` + `add`，和 `ServerLevel.updatePOIOnBlockStateChange` 同规则。
// 每一步之后把查询族的结果逐项写成一行 JSON；最后把 `SectionStorage.writeChunk`
// 的根标签、`sectionsToVillage` 的一片网格、以及读盘往返的结果各写一行 JSON。
//
// 输入：stdin 一行 JSON `{"ops":[...],"queries":[...],"sections":[...],"chunk":{"x":..,"z":..}}`。
// op = `{"at":[x,y,z],"old":<状态ID>,"new":<状态ID>}` 或 `{"take":[...],"radius":n}`
//      / `{"release":[x,y,z]}`。
// 输出：一行一个 JSON，Rust 侧逐行比较。

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.datafixers.DataFixer;
import com.mojang.serialization.Dynamic;
import com.mojang.serialization.DynamicOps;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.server.RegistryLayer;
import net.minecraft.core.SectionPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.entity.ai.village.poi.PoiManager;
import net.minecraft.world.entity.ai.village.poi.PoiRecord;
import net.minecraft.world.entity.ai.village.poi.PoiTypes;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.storage.ChunkIOErrorReporter;
import net.minecraft.world.level.chunk.storage.RegionStorageInfo;
import net.minecraft.world.level.chunk.storage.SectionStorage;

public final class PoiOracle {
    private static final Gson GSON = new GsonBuilder().create();
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;

    private PoiOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run();
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run() throws Exception {
        // 官方日志默认写 stdout，先换掉，避免混进 JSON。
        System.setOut(System.err);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        JsonObject request = JsonParser.parseString(in.readLine()).getAsJsonObject();
        Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), 1 << 20);
        Path dir = Files.createTempDirectory("bamboo-poi-oracle");
        // 静态注册表（POI 类型）的标签要真的绑定，`isVillageCenter` 才认
        // `#minecraft:village`；这里照 WorldLoader.load 前半段加载一遍标签。
        LayeredRegistryAccess<RegistryLayer> layers = RegistryLayer.createRegistryAccess();
        RegistryAccess.Frozen registries;
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
            staticTags.forEach(Registry.PendingTags::apply);
            RegistryAccess.Frozen world = RegistryDataLoader
                    .load(resources, TagLoader.buildUpdatedLookups(
                            layers.getAccessForLoading(RegistryLayer.WORLD), staticTags),
                            RegistryDataLoader.WORLD_REGISTRIES,
                            java.util.concurrent.ForkJoinPool.commonPool()).join();
            registries = layers.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
        }
        DataFixer fixer = DataFixers.getDataFixer();
        LevelHeightAccessor height = LevelHeightAccessor.create(MIN_Y, HEIGHT);
        ChunkIOErrorReporter reporter = new ChunkIOErrorReporter() {
            public void reportChunkLoadFailure(Throwable error, RegionStorageInfo info, ChunkPos pos) {
            }

            public void reportChunkSaveFailure(Throwable error, RegionStorageInfo info, ChunkPos pos) {
            }
        };
        try (PoiManager poi = new PoiManager(
                new RegionStorageInfo("oracle", Level.OVERWORLD, "poi"), dir, fixer, false,
                registries, reporter, height)) {
            emitScenario(out, poi, request);
        }
        out.flush();
    }

    /** 和 WorldLoader.load 前半段相同：静态注册表标签 + 世界层注册表。 */
    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(CloseableResourceManager resources) {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool()).join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }

    private static void emitScenario(Writer out, PoiManager poi, JsonObject request) throws Exception {
        JsonArray ops = request.getAsJsonArray("ops");
        JsonArray queries = request.getAsJsonArray("queries");
        JsonArray sections = request.getAsJsonArray("sections");
        JsonObject chunk = request.getAsJsonObject("chunk");
        JsonArray takeResults = new JsonArray();
        for (int step = 0; step <= ops.size(); step++) {
            if (step > 0) {
                applyOp(poi, ops.get(step - 1).getAsJsonObject(), takeResults);
            }
            for (int q = 0; q < queries.size(); q++) {
                JsonObject query = queries.get(q).getAsJsonObject();
                JsonObject answer = answerQuery(poi, query);
                answer.addProperty("type", "query");
                answer.addProperty("step", step);
                answer.addProperty("query", q);
                out.write(GSON.toJson(answer));
                out.write('\n');
            }
        }
        JsonObject takes = new JsonObject();
        takes.addProperty("type", "takes");
        takes.add("values", takeResults);
        out.write(GSON.toJson(takes));
        out.write('\n');

        // `SectionStorage.writeChunk(pos, NbtOps.INSTANCE)` 产出的根标签。
        ChunkPos at = new ChunkPos(chunk.get("x").getAsInt(), chunk.get("z").getAsInt());
        CompoundTag root = writeChunk(poi, at);
        JsonObject saved = new JsonObject();
        saved.addProperty("type", "save");
        saved.add("nbt", canonical(root));
        out.write(GSON.toJson(saved));
        out.write('\n');

        // 读回：用官方 `PoiSection$Packed.CODEC` 逐段解析再编码，按内容比对
        // （`Sections` 的键序 / `Records` 的顺序都要和 Bamboo 的解码一致）。
        JsonObject roundtrip = new JsonObject();
        roundtrip.addProperty("type", "roundtrip");
        CompoundTag savedSections = root.getCompound("Sections").orElse(new CompoundTag());
        JsonObject decoded = new JsonObject();
        List<String> keys = new ArrayList<>(savedSections.keySet());
        java.util.Collections.sort(keys);
        for (String key : keys) {
            Tag raw = savedSections.get(key);
            Dynamic<Tag> dynamic = new Dynamic<>(registryOps(poi), raw);
            var result = net.minecraft.world.entity.ai.village.poi.PoiSection.Packed.CODEC
                    .parse(dynamic).result();
            if (result.isPresent()) {
                var encoded = net.minecraft.world.entity.ai.village.poi.PoiSection.Packed.CODEC
                        .encodeStart(registryOps(poi), result.get()).result();
                encoded.ifPresent(tag -> decoded.add(key, canonical(tag)));
            }
        }
        roundtrip.add("sections", decoded);
        out.write(GSON.toJson(roundtrip));
        out.write('\n');

        // `sectionsToVillage` 逐段比：官方的 `DistanceTracker` 不动点。
        for (int i = 0; i < sections.size(); i++) {
            JsonArray pos = sections.get(i).getAsJsonArray();
            SectionPos section = SectionPos.of(pos.get(0).getAsInt(), pos.get(1).getAsInt(), pos.get(2).getAsInt());
            JsonObject distance = new JsonObject();
            distance.addProperty("type", "village");
            distance.addProperty("x", section.x());
            distance.addProperty("y", section.y());
            distance.addProperty("z", section.z());
            distance.addProperty("level", poi.sectionsToVillage(section));
            out.write(GSON.toJson(distance));
            out.write('\n');
        }
    }

    private static void applyOp(PoiManager poi, JsonObject op, JsonArray takeResults) {
        if (op.has("at")) {
            JsonArray at = op.getAsJsonArray("at");
            BlockPos pos = blockPos(at);
            Holder<?> before = PoiTypes.forState(state(op.get("old").getAsInt())).orElse(null);
            Holder<?> after = PoiTypes.forState(state(op.get("new").getAsInt())).orElse(null);
            if (before != after) {
                poi.remove(pos);
                if (after != null) {
                    @SuppressWarnings("unchecked")
                    Holder<net.minecraft.world.entity.ai.village.poi.PoiType> typed =
                            (Holder<net.minecraft.world.entity.ai.village.poi.PoiType>) after;
                    poi.add(pos, typed);
                }
            }
        } else if (op.has("take")) {
            BlockPos center = blockPos(op.getAsJsonArray("take"));
            int radius = op.get("radius").getAsInt();
            // 只在一个 POI 类型里挑：`byType` 是 `HashMap<Holder.Reference>`，
            // 键的 identity hash 每次 JVM 都不同，多类型段的键序本身不可复现。
            // 单类型谓词让 `getRecords` 只走一个 HashSet，顺序可逐项对拍。
            Identifier wanted = op.has("poi")
                    ? Identifier.parse(op.get("poi").getAsString())
                    : Identifier.parse("minecraft:home");
            Optional<BlockPos> taken =
                    poi.take(holder -> holder.is(wanted), (holder, pos) -> true, center, radius);
            takeResults.add(taken.map(PoiOracle::posArray).orElse(null));
        } else if (op.has("release")) {
            poi.release(blockPos(op.getAsJsonArray("release")));
        } else {
            throw new IllegalArgumentException("unknown op " + op);
        }
    }

    private static JsonObject answerQuery(PoiManager poi, JsonObject query) {
        String kind = query.get("kind").getAsString();
        String type = query.get("poi").getAsString();
        Identifier typeId = Identifier.parse(type);
        BlockPos center = blockPos(query.getAsJsonArray("center"));
        int radius = query.get("radius").getAsInt();
        PoiManager.Occupancy occupancy = occupancy(query.get("occupancy").getAsString());
        java.util.function.Predicate<Holder<net.minecraft.world.entity.ai.village.poi.PoiType>> predicate =
                holder -> holder.is(typeId);
        JsonObject answer = new JsonObject();
        switch (kind) {
            case "exists" -> {
                BlockPos at = blockPos(query.getAsJsonArray("at"));
                answer.addProperty("value", poi.exists(at, predicate));
            }
            case "getType" -> {
                BlockPos at = blockPos(query.getAsJsonArray("at"));
                answer.add("value", poi.getType(at).map(h -> h.unwrapKey().get().identifier().toString())
                        .map(GSON::toJsonTree).orElse(null));
            }
            case "freeTickets" -> {
                BlockPos at = blockPos(query.getAsJsonArray("at"));
                answer.addProperty("value", freeTickets(poi, at));
            }
            case "countInRange" -> answer.addProperty("value", poi.getCountInRange(predicate, center, radius, occupancy));
            case "getInSquare" -> answer.add("value", positions(poi.getInSquare(predicate, center, radius, occupancy)));
            case "getInRange" -> answer.add("value", positions(poi.getInRange(predicate, center, radius, occupancy)));
            case "findAllClosestFirstWithType" -> {
                JsonArray pairs = new JsonArray();
                poi.findAllClosestFirstWithType(predicate, pos -> true, center, radius, occupancy)
                        .forEach(pair -> {
                            JsonArray entry = new JsonArray();
                            entry.add(pair.getFirst().unwrapKey().get().identifier().toString());
                            JsonArray at = new JsonArray();
                            at.add(pair.getSecond().getX());
                            at.add(pair.getSecond().getY());
                            at.add(pair.getSecond().getZ());
                            entry.add(at);
                            pairs.add(entry);
                        });
                answer.add("value", pairs);
            }
            case "findClosest" -> {
                answer.add("value", poi.findClosest(predicate, pos -> true, center, radius, occupancy)
                        .map(PoiOracle::posArray).orElse(null));
            }
            case "findClosestWithType" -> {
                answer.add("value", poi.findClosestWithType(predicate, center, radius, occupancy)
                        .map(pair -> {
                            JsonArray entry = new JsonArray();
                            entry.add(pair.getFirst().unwrapKey().get().identifier().toString());
                            entry.add(posArray(pair.getSecond()));
                            return entry;
                        }).orElse(null));
            }
            case "take" -> {
                answer.add("value", poi.take(predicate, (holder, pos) -> true, center, radius)
                        .map(PoiOracle::posArray).orElse(null));
            }
            default -> throw new IllegalArgumentException("unknown query " + kind);
        }
        return answer;
    }

    private static int freeTickets(PoiManager poi, BlockPos at) {
        return poi.getInRange(h -> true, at, 1, PoiManager.Occupancy.ANY)
                .filter(record -> record.getPos().equals(at))
                .mapToInt(PoiRecord::getFreeTickets)
                .findFirst()
                .orElse(0);
    }

    private static JsonArray positions(java.util.stream.Stream<PoiRecord> stream) {
        JsonArray out = new JsonArray();
        stream.forEach(record -> {
            JsonArray entry = new JsonArray();
            entry.add(record.getPoiType().unwrapKey().get().identifier().toString());
            entry.add(posArray(record.getPos()));
            entry.add(record.getFreeTickets());
            out.add(entry);
        });
        return out;
    }

    private static JsonArray posArray(BlockPos pos) {
        JsonArray out = new JsonArray();
        out.add(pos.getX());
        out.add(pos.getY());
        out.add(pos.getZ());
        return out;
    }

    private static BlockPos blockPos(JsonArray at) {
        return new BlockPos(at.get(0).getAsInt(), at.get(1).getAsInt(), at.get(2).getAsInt());
    }

    private static BlockState state(int id) {
        return Block.BLOCK_STATE_REGISTRY.byId(id);
    }

    private static PoiManager.Occupancy occupancy(String name) {
        return switch (name) {
            case "HAS_SPACE" -> PoiManager.Occupancy.HAS_SPACE;
            case "IS_OCCUPIED" -> PoiManager.Occupancy.IS_OCCUPIED;
            default -> PoiManager.Occupancy.ANY;
        };
    }

    private static DynamicOps<Tag> registryOps(PoiManager poi) throws Exception {
        java.lang.reflect.Field registry = SectionStorage.class.getDeclaredField("registryAccess");
        registry.setAccessible(true);
        return ((RegistryAccess) registry.get(poi)).createSerializationContext(NbtOps.INSTANCE);
    }

    /** 调用私有 `SectionStorage.writeChunk(ChunkPos, DynamicOps<Tag>)`，拿到官方根标签。 */
    @SuppressWarnings("unchecked")
    private static CompoundTag writeChunk(PoiManager poi, ChunkPos at) throws Exception {
        // `writeChunk(ChunkPos)` 先把 registryAccess 包成 RegistryOps 再调下面这个
        // 两参重载；直接传 NbtOps 会让 `RegistryFixedCodec` 找不到 POI 注册表。
        java.lang.reflect.Field registry = SectionStorage.class.getDeclaredField("registryAccess");
        registry.setAccessible(true);
        RegistryAccess access = (RegistryAccess) registry.get(poi);
        DynamicOps<Tag> ops = access.createSerializationContext(NbtOps.INSTANCE);
        Method method = SectionStorage.class.getDeclaredMethod(
                "writeChunk", ChunkPos.class, DynamicOps.class);
        method.setAccessible(true);
        Dynamic<Tag> dynamic = (Dynamic<Tag>) method.invoke(poi, at, ops);
        return (CompoundTag) dynamic.getValue();
    }

    /** 复合标签按键排序后转 JSON；数组和记录顺序保留（顺序本身要对拍）。 */
    private static com.google.gson.JsonElement canonical(Tag tag) {
        if (tag instanceof CompoundTag compound) {
            Map<String, com.google.gson.JsonElement> sorted = new TreeMap<>();
            for (String key : compound.keySet()) {
                sorted.put(key, canonical(compound.get(key)));
            }
            JsonObject object = new JsonObject();
            sorted.forEach(object::add);
            return object;
        }
        if (tag instanceof net.minecraft.nbt.ListTag list) {
            JsonArray array = new JsonArray();
            for (Tag item : list) {
                array.add(canonical(item));
            }
            return array;
        }
        if (tag instanceof net.minecraft.nbt.IntArrayTag array) {
            JsonArray out = new JsonArray();
            for (int value : array.getAsIntArray()) {
                out.add(value);
            }
            return out;
        }
        if (tag instanceof net.minecraft.nbt.StringTag string) {
            return GSON.toJsonTree(string.value());
        }
        if (tag instanceof net.minecraft.nbt.ByteTag number) {
            return GSON.toJsonTree(number.value());
        }
        if (tag instanceof net.minecraft.nbt.IntTag number) {
            return GSON.toJsonTree(number.value());
        }
        return GSON.toJsonTree(tag.toString());
    }
}
