// 自然刷怪的加权选择和组大小对拍：不启动服务端、不读存档、不接受 EULA。
//
// 对每个群系的每个 `MobCategory`，用同一个种子的官方 `RandomSource` 跑若干次：
// `WeightedList.getRandom` 挑出一项（`nextInt(总权重)` 后累减权重），再用这一项的
// `IntProvider.sample` 取组大小（官方刷怪表里只有常量和均匀分布）。另外把 0..n 的列表
// 交给官方 `Util.shuffle`，和 Bamboo 的打乱逐项比较。
//
// 一行 JSON：`{"type":"pick","biome":"minecraft:plains","category":"creature","picks":[[type,count],...],"seed":N}`
// 或 `{"type":"shuffle","size":8,"order":[...],"seed":N}`。
//
// Natural-spawn weighted selection and group size, without a server, a save or the
// EULA. For every biome and `MobCategory`, run vanilla `WeightedList.getRandom`
// (nextInt(total weight) then walk the cumulative weights) and `IntProvider.sample`
// with a seeded `RandomSource`. Also shuffle 0..n with vanilla `Util.shuffle`.
// One JSON line per case.

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
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.level.ChunkMap;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.util.Util;
import net.minecraft.util.random.WeightedList;
import net.minecraft.world.attribute.EnvironmentAttributes;
import net.minecraft.world.entity.MobCategory;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.NaturalSpawner;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureSpawnOverride;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.MobSpawnSettings;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.phys.Vec3;
import sun.misc.Unsafe;

public final class NaturalSpawnOracle {
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final int PICKS_PER_CASE = 8;
    private static final int SHUFFLE_SIZE = 16;
    private static final long[] SEEDS = {0L, 1L, 2L, 7L, 42L, -1L, 12345L};

    private NaturalSpawnOracle() {
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
             Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            Registry<Biome> biomes = registries.lookupOrThrow(Registries.BIOME);
            for (Identifier name : biomes.keySet().stream().sorted().toList()) {
                Biome biome = biomes.getValue(name);
                // 26.3 的刷怪设置是群系的环境属性 `NATURAL_MOB_SPAWNS`，按 overlay 叠在空表上。
                MobSpawnSettings settings = biome.getAttributes()
                        .applyModifier(EnvironmentAttributes.NATURAL_MOB_SPAWNS, MobSpawnSettings.EMPTY);
                for (MobCategory category : MobCategory.values()) {
                    WeightedList<MobSpawnSettings.SpawnerData> spawns = settings.getMobsToSpawn(category);
                    if (spawns.isEmpty()) {
                        continue;
                    }
                    for (long seed : SEEDS) {
                        out.write(pickCase(name, category, spawns, seed).toString());
                        out.write('\n');
                    }
                }
            }
            for (long seed : SEEDS) {
                out.write(shuffleCase(seed).toString());
                out.write('\n');
            }
            writeStructureOverrideCases(out, registries);
            writePositionCases(out);
            out.flush();
        }
    }

    // 跳过构造，只注入官方随机起点方法读取的高度和随机源，不启动世界线程。
    // Skip constructors and inject only the height and RNG read by the official method.
    private static void writePositionCases(Writer out) throws Exception {
        Field singleton = Unsafe.class.getDeclaredField("theUnsafe");
        singleton.setAccessible(true);
        Unsafe unsafe = (Unsafe) singleton.get(null);
        PositionLevel level = (PositionLevel) unsafe.allocateInstance(PositionLevel.class);
        PositionChunk chunk = (PositionChunk) unsafe.allocateInstance(PositionChunk.class);
        Field randomField = Level.class.getDeclaredField("random");
        randomField.setAccessible(true);
        Method origin = NaturalSpawner.class.getDeclaredMethod("getRandomPosWithin", Level.class, LevelChunk.class);
        origin.setAccessible(true);
        Method distance = ChunkMap.class.getDeclaredMethod("euclideanDistanceSquared", ChunkPos.class, Vec3.class);
        distance.setAccessible(true);
        chunk.pos = new ChunkPos(-3, 2);
        for (boolean empty : new boolean[]{true, false}) {
            chunk.empty = empty;
            for (long seed : SEEDS) {
                RandomSource random = RandomSource.create(seed);
                randomField.set(level, random);
                JsonArray positions = new JsonArray();
                for (int i = 0; i < PICKS_PER_CASE; i++) {
                    BlockPos pos = (BlockPos) origin.invoke(null, level, chunk);
                    JsonArray point = new JsonArray();
                    point.add(pos.getX());
                    point.add(pos.getY());
                    point.add(pos.getZ());
                    positions.add(point);
                }
                JsonObject row = new JsonObject();
                row.addProperty("type", "origin");
                row.addProperty("seed", seed);
                row.addProperty("empty", empty);
                row.add("positions", positions);
                row.addProperty("after", seedAfter(random));
                out.write(row + "\n");
            }
        }
        for (double dx : new double[]{24, 32, 90.5, 127.999, 128, 128.001}) {
            for (double dz : new double[]{0, 90.5}) {
                Vec3 player = new Vec3(chunk.pos.getMiddleBlockX() + dx, 999, chunk.pos.getMiddleBlockZ() + dz);
                JsonObject row = new JsonObject();
                row.addProperty("type", "chunk_distance");
                row.addProperty("x", player.x);
                row.addProperty("z", player.z);
                row.addProperty("close", (double) distance.invoke(null, chunk.pos, player) < 16384.0);
                out.write(row + "\n");
            }
        }
    }

    private static final class PositionLevel extends ServerLevel {
        private PositionLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        @Override
        public int getMinY() {
            return -64;
        }
    }

    private static final class PositionChunk extends LevelChunk {
        private ChunkPos pos;
        private boolean empty;

        private PositionChunk() {
            super(null, null);
        }

        @Override
        public ChunkPos getPos() {
            return pos;
        }

        @Override
        public int getHeight(Heightmap.Types type, int x, int z) {
            return empty ? -65 : 63 + (x & 15) - (z & 15);
        }
    }

    /**
     * 官方 `Structure.spawnOverrides()` 的每张加权表：结构名、类别、盒类型和加权
     * 列表的取数结果。空表也要输出，Bamboo 侧用它挡住群系表。
     *
     * Every `Structure.spawnOverrides()` weighted list: structure, category, box type
     * and the draw results. Empty lists are included because they suppress the biome table.
     */
    private static void writeStructureOverrideCases(Writer out, RegistryAccess.Frozen registries)
            throws Exception {
        Registry<Structure> structures = registries.lookupOrThrow(Registries.STRUCTURE);
        for (Identifier name : structures.keySet().stream().sorted().toList()) {
            Structure structure = structures.getValue(name);
            for (Map.Entry<MobCategory, StructureSpawnOverride> entry
                    : structure.spawnOverrides().entrySet()) {
                MobCategory category = entry.getKey();
                StructureSpawnOverride override = entry.getValue();
                String bounding = override.boundingBox().getSerializedName();
                WeightedList<MobSpawnSettings.SpawnerData> spawns = override.spawns();
                for (long seed : SEEDS) {
                    RandomSource random = RandomSource.create(seed);
                    JsonArray picks = new JsonArray();
                    for (int i = 0; i < PICKS_PER_CASE; i++) {
                        Optional<MobSpawnSettings.SpawnerData> picked = spawns.getRandom(random);
                        if (picked.isEmpty()) {
                            picks.add("empty");
                            continue;
                        }
                        MobSpawnSettings.SpawnerData data = picked.get();
                        int count = data.count().sample(random);
                        JsonArray pick = new JsonArray();
                        pick.add(net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE
                                .getKey(data.type()).toString());
                        pick.add(count);
                        picks.add(pick);
                    }
                    JsonObject row = new JsonObject();
                    row.addProperty("type", "structureOverride");
                    row.addProperty("structure", name.toString());
                    row.addProperty("category", category.getName());
                    row.addProperty("bounding", bounding);
                    row.add("picks", picks);
                    row.addProperty("seed", seed);
                    row.addProperty("after", seedAfter(random));
                    out.write(row + "\n");
                }
            }
        }
    }

    private static JsonObject pickCase(Identifier biome, MobCategory category,
            WeightedList<MobSpawnSettings.SpawnerData> spawns, long seed) {
        RandomSource random = RandomSource.create(seed);
        JsonArray picks = new JsonArray();
        for (int i = 0; i < PICKS_PER_CASE; i++) {
            Optional<MobSpawnSettings.SpawnerData> picked = spawns.getRandom(random);
            if (picked.isEmpty()) {
                picks.add("empty");
                continue;
            }
            MobSpawnSettings.SpawnerData data = picked.get();
            int count = data.count().sample(random);
            JsonArray pick = new JsonArray();
            pick.add(net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(data.type()).toString());
            pick.add(count);
            picks.add(pick);
        }
        JsonObject line = new JsonObject();
        line.addProperty("type", "pick");
        line.addProperty("biome", biome.toString());
        line.addProperty("category", category.getName());
        line.add("picks", picks);
        line.addProperty("seed", seed);
        line.addProperty("after", seedAfter(random));
        return line;
    }

    private static JsonObject shuffleCase(long seed) {
        RandomSource random = RandomSource.create(seed);
        List<Integer> order = new ArrayList<>();
        for (int i = 0; i < SHUFFLE_SIZE; i++) {
            order.add(i);
        }
        Util.shuffle(order, random);
        JsonArray array = new JsonArray();
        for (int value : order) {
            array.add(value);
        }
        JsonObject line = new JsonObject();
        line.addProperty("type", "shuffle");
        line.addProperty("size", SHUFFLE_SIZE);
        line.add("order", array);
        line.addProperty("seed", seed);
        return line;
    }

    /** 官方 `RandomSource` 的内部状态不能直接读，改用「再取一个 nextInt」当指纹。 */
    private static int seedAfter(RandomSource random) {
        return random.nextInt(1000);
    }

    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
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
}
