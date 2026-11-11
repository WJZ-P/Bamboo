import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap;
import it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.util.debug.ServerDebugSubscribers;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ChunkMap;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.ProblemReporter;
import net.minecraft.util.RandomSource;
import net.minecraft.world.Difficulty;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.item.FallingBlockEntity;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.entity.EntityProcessor;
import net.minecraft.world.entity.item.PrimedTnt;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.ticks.LevelTicks;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.LevelSettings.DifficultySettings;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.storage.PrimaryLevelData;
import com.mojang.serialization.Lifecycle;
import net.minecraft.world.level.storage.WorldData;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.ChestBlockEntity;
import net.minecraft.world.level.block.entity.HopperBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.storage.SerializableChunkData;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.level.redstone.CollectingNeighborUpdater;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.TagValueInput;
import net.minecraft.world.level.storage.TagValueOutput;
import net.minecraft.world.level.storage.ValueInput;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.ticks.LevelChunkTicks;
import net.minecraft.world.ticks.SavedTick;
import net.minecraft.world.ticks.TickPriority;
import sun.misc.Unsafe;

/**
 * 用官方 26.3 的类检查区块里的上层存档 NBT：方块实体逐类型、计划刻、三种实体。
 *
 * <p>不启动服务端，不接受 EULA。题目从 stdin 一行一个 JSON，输出一行一个 JSON；
 * 官方的 CompoundTag 用 HashMap，键序是哈希序，所以输出按键排序的规范 JSON。
 * 实体的读取需要 Level；复用 EntityOracle 的 FakeLevel 形状（Unsafe 分配 +
 * 反射填字段），不跑任何服务端生命周期。
 */
public final class ChunkContentOracle {
    private static final int DATA_VERSION = 5023;
    private static final Unsafe UNSAFE = loadUnsafe();

    private ChunkContentOracle() {
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
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), 1 << 20);
                BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8))) {
            Official official = Official.load(resources);
            Oracle oracle = new Oracle(official, out);
            String line;
            while ((line = in.readLine()) != null) {
                if (line.isBlank()) {
                    continue;
                }
                JsonObject request = JsonParser.parseString(line).getAsJsonObject();
                JsonObject response = oracle.handle(request);
                out.write(response.toString());
                out.write('\n');
                out.flush();
            }
        }
    }

    private static final class Official {
        final RegistryAccess registries;
        final Holder<DimensionType> overworld;
        final Holder<Biome> plains;
        final DamageSources damageSources;
        final FakeServer server;
        final GameRules gameRules = new GameRules(FeatureFlags.DEFAULT_FLAGS);
        final WorldBorder border = new WorldBorder();
        final LevelData levelData = new FixedLevelData();
        EnvironmentAttributeSystem attributes;

        private Official(ReloadableServerResources managers) {
            this.registries = (RegistryAccess) managers.fullRegistries().lookup();
            this.overworld = registries.lookupOrThrow(Registries.DIMENSION_TYPE)
                    .getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            this.plains = registries.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
            this.damageSources = new DamageSources(registries);
            this.server = FakeServer.create(managers, gameRules);
        }

        static Official load(CloseableResourceManager resources) throws Exception {
            LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
            RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
            RegistryAccess.Frozen world = RegistryDataLoader
                    .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                    .join();
            LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
            ReloadableServerResources managers = ReloadableServerResources
                    .loadResources(
                            resources,
                            layers,
                            staticTags,
                            FeatureFlags.DEFAULT_FLAGS,
                            Commands.CommandSelection.DEDICATED,
                            LevelBasedPermissionSet.GAMEMASTER,
                            ForkJoinPool.commonPool(),
                            Runnable::run)
                    .get();
            managers.updateComponentsAndStaticRegistryTags();
            managers.getRecipeManager().finalizeRecipeLoading(FeatureFlags.DEFAULT_FLAGS);
            Official official = new Official(managers);
            FakeLevel sample = FakeLevel.create(official, 0L);
            official.attributes = EnvironmentAttributeSystem.builder().addStaticLayers(sample).build();
            set(ServerLevel.class, sample, "environmentAttributes", official.attributes);
            return official;
        }
    }

    private static final class FakeServer extends DedicatedServer {
        private ReloadableServerResources managers;
        private GameRules rules;
        private ServerDebugSubscribers debugSubscribers;

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        static FakeServer create(ReloadableServerResources managers, GameRules rules) {
            FakeServer server = allocate(FakeServer.class);
            server.managers = managers;
            server.rules = rules;
            server.debugSubscribers = new ServerDebugSubscribers(server);
            return server;
        }

        @Override
        public ServerDebugSubscribers debugSubscribers() {
            return debugSubscribers;
        }

        @Override
        public net.minecraft.world.item.crafting.RecipeManager getRecipeManager() {
            return managers.getRecipeManager();
        }

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return new NoLoot(managers.fullRegistries().lookup());
        }

        @Override
        public GameRules getGameRules() {
            return rules;
        }

        @Override
        public WorldData getWorldData() {
            LevelSettings settings = new LevelSettings(
                    "oracle", GameType.SURVIVAL, DifficultySettings.DEFAULT, true,
                    WorldDataConfiguration.DEFAULT);
            return new PrimaryLevelData(
                    settings, PrimaryLevelData.SpecialWorldProperty.NONE, Lifecycle.stable());
        }
    }

    private static final class NoLoot extends ReloadableServerRegistries.Holder {
        NoLoot(HolderLookup.Provider registries) {
            super(registries);
        }

        @Override
        public LootTable getLootTable(net.minecraft.resources.ResourceKey<LootTable> key) {
            return LootTable.EMPTY;
        }
    }

    private static final class FixedLevelData implements LevelData {
        @Override
        public LevelData.RespawnData getRespawnData() {
            return LevelData.RespawnData.DEFAULT;
        }

        @Override
        public long getGameTime() {
            return 0L;
        }

        @Override
        public boolean isHardcore() {
            return false;
        }

        @Override
        public Difficulty getDifficulty() {
            return Difficulty.NORMAL;
        }

        @Override
        public boolean isDifficultyLocked() {
            return false;
        }
    }

    private static final class FakeLevel extends ServerLevel {
        private Official official;
        private Long2ObjectOpenHashMap<BlockState> blocks;
        private List<Entity> fresh = new ArrayList<>();
        private LevelTicks<Block> blockTicks;
        private LevelTicks<Fluid> fluidTicks;
        private ServerChunkCache chunkSource;
        private int nextId = 1;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        static FakeLevel create(Official official, long seed) {
            FakeLevel level = allocate(FakeLevel.class);
            level.official = official;
            level.fresh = new ArrayList<>();
            level.blocks = new Long2ObjectOpenHashMap<>();
            level.blockTicks = new LevelTicks<>(key -> true);
            level.fluidTicks = new LevelTicks<>(key -> true);
            level.chunkSource = chunkSource();
            set(Level.class, level, "neighborUpdater", new CollectingNeighborUpdater(level, 1_000_000));
            set(Level.class, level, "random", RandomSource.create(seed));
            set(Level.class, level, "soundSeedGenerator", RandomSource.createThreadSafe());
            set(Level.class, level, "dimensionTypeRegistration", official.overworld);
            set(Level.class, level, "dimension", Level.OVERWORLD);
            set(Level.class, level, "registryAccess", official.registries);
            set(
                    Level.class,
                    level,
                    "palettedContainerFactory",
                    PalettedContainerFactory.create(official.registries));
            set(Level.class, level, "damageSources", official.damageSources);
            set(Level.class, level, "thread", Thread.currentThread());
            set(ServerLevel.class, level, "players", new ArrayList<ServerPlayer>());
            set(ServerLevel.class, level, "server", official.server);
            set(ServerLevel.class, level, "chunkSource", level.chunkSource);
            if (official.attributes != null) {
                set(ServerLevel.class, level, "environmentAttributes", official.attributes);
            }
            return level;
        }

        private static ServerChunkCache chunkSource() {
            ServerChunkCache source = allocate(ServerChunkCache.class);
            ChunkMap map = allocate(ChunkMap.class);
            set(ChunkMap.class, map, "entityMap", new Int2ObjectOpenHashMap<>());
            set(ServerChunkCache.class, source, "chunkMap", map);
            return source;
        }

        @Override
        public int getNextEntityId() {
            return nextId++;
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            BlockState state = blocks.get(pos.asLong());
            return state != null ? state : Blocks.AIR.defaultBlockState();
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return getBlockState(pos).getFluidState();
        }

        @Override
        public LevelTicks<Block> getBlockTicks() {
            return blockTicks;
        }

        @Override
        public LevelTicks<Fluid> getFluidTicks() {
            return fluidTicks;
        }
    }

    private static final class Oracle {
        private final Official official;
        private final Writer out;

        Oracle(Official official, Writer out) {
            this.official = official;
            this.out = out;
        }

        JsonObject handle(JsonObject request) {
            String command = request.get("command").getAsString();
            JsonObject response = new JsonObject();
            response.addProperty("command", command);
            try {
                switch (command) {
                    case "header" -> {
                        response.addProperty("version", SharedConstants.getCurrentVersion().id());
                        response.addProperty(
                                "protocol", SharedConstants.getCurrentVersion().protocolVersion());
                        response.addProperty("dataVersion", SharedConstants.getCurrentVersion().dataVersion().version());
                    }
                    case "block-entities" -> response.add("cases", blockEntities());
                    case "block-entities-load" -> response.add("cases", blockEntitiesLoad(request));
                    case "ticks" -> response.add("cases", ticks());
                    case "entities" -> response.add("cases", entities(request));
                    case "chunk-read" -> {
                        byte[] bytes = Base64.getDecoder().decode(request.get("nbt").getAsString());
                        CompoundTag root = NbtIo.read(new java.io.DataInputStream(new java.io.ByteArrayInputStream(bytes)));
                        SerializableChunkData parsed = SerializableChunkData.parse(
                                LevelHeightAccessor.create(-64, 384),
                                PalettedContainerFactory.create(official.registries),
                                root);
                        CompoundTag written = parsed.write();
                        response.add("original", canonical(root));
                        response.add("written", canonical(written));
                        response.add("blockEntities", blockEntityKeys(root));
                    }
                    default -> response.addProperty("error", "unknown command " + command);
                }
            } catch (Exception error) {
                response.addProperty("error", String.valueOf(error));
            }
            return response;
        }

        /** 每种方块实体：写 → loadStatic 读 → 再写，按键比较规范 JSON。 */
        private JsonArray blockEntities() {
            JsonArray cases = new JsonArray();
            BlockPos pos = new BlockPos(3, 64, 5);
            List<NamedEntity> entities = List.of(
                    new NamedEntity("chest", new ChestBlockEntity(pos, Blocks.CHEST.defaultBlockState()), Blocks.CHEST.defaultBlockState()),
                    new NamedEntity("barrel", new net.minecraft.world.level.block.entity.BarrelBlockEntity(pos, Blocks.BARREL.defaultBlockState()), Blocks.BARREL.defaultBlockState()),
                    new NamedEntity("dispenser", new net.minecraft.world.level.block.entity.DispenserBlockEntity(pos, Blocks.DISPENSER.defaultBlockState()), Blocks.DISPENSER.defaultBlockState()),
                    new NamedEntity("hopper", new HopperBlockEntity(pos, Blocks.HOPPER.defaultBlockState()), Blocks.HOPPER.defaultBlockState()),
                    new NamedEntity("furnace", new net.minecraft.world.level.block.entity.FurnaceBlockEntity(pos, Blocks.FURNACE.defaultBlockState()), Blocks.FURNACE.defaultBlockState()),
                    new NamedEntity("brewing_stand", new net.minecraft.world.level.block.entity.BrewingStandBlockEntity(pos, Blocks.BREWING_STAND.defaultBlockState()), Blocks.BREWING_STAND.defaultBlockState()),
                    new NamedEntity("campfire", new net.minecraft.world.level.block.entity.CampfireBlockEntity(pos, Blocks.CAMPFIRE.defaultBlockState()), Blocks.CAMPFIRE.defaultBlockState()),
                    new NamedEntity("shulker_box", new net.minecraft.world.level.block.entity.ShulkerBoxBlockEntity(pos, Blocks.SHULKER_BOX.defaultBlockState()), Blocks.SHULKER_BOX.defaultBlockState()),
                    new NamedEntity("crafter", new net.minecraft.world.level.block.entity.CrafterBlockEntity(pos, Blocks.CRAFTER.defaultBlockState()), Blocks.CRAFTER.defaultBlockState()),
                    new NamedEntity("comparator", new net.minecraft.world.level.block.entity.ComparatorBlockEntity(pos, Blocks.COMPARATOR.defaultBlockState()), Blocks.COMPARATOR.defaultBlockState()),
                    new NamedEntity("lectern", new net.minecraft.world.level.block.entity.LecternBlockEntity(pos, Blocks.LECTERN.defaultBlockState()), Blocks.LECTERN.defaultBlockState()),
                    new NamedEntity("command_block", new net.minecraft.world.level.block.entity.CommandBlockEntity(pos, Blocks.COMMAND_BLOCK.defaultBlockState()), Blocks.COMMAND_BLOCK.defaultBlockState()),
                    new NamedEntity("sign", new net.minecraft.world.level.block.entity.SignBlockEntity(pos, Blocks.OAK_SIGN.defaultBlockState()), Blocks.OAK_SIGN.defaultBlockState()));
            for (NamedEntity named : entities) {
                JsonObject one = new JsonObject();
                one.addProperty("name", named.name);
                try {
                    TagValueOutput first = TagValueOutput.createWithContext(
                            ProblemReporter.DISCARDING, official.registries);
                    named.entity.saveWithFullMetadata(first);
                    CompoundTag firstTag = first.buildResult();
                    firstTag.putInt("x", pos.getX());
                    firstTag.putInt("y", pos.getY());
                    firstTag.putInt("z", pos.getZ());
                    firstTag.putBoolean("keepPacked", false);
                    BlockEntity loaded = BlockEntity.loadStatic(pos, named.state, firstTag, official.registries);
                    if (loaded == null) {
                        one.addProperty("error", "loadStatic returned null");
                    } else {
                        TagValueOutput second = TagValueOutput.createWithContext(
                                ProblemReporter.DISCARDING, official.registries);
                        loaded.saveWithFullMetadata(second);
                        CompoundTag secondTag = second.buildResult();
                        secondTag.putInt("x", pos.getX());
                        secondTag.putInt("y", pos.getY());
                        secondTag.putInt("z", pos.getZ());
                        secondTag.putBoolean("keepPacked", false);
                        one.add("original", canonical(firstTag));
                        one.add("written", canonical(secondTag));
                    }
                } catch (Exception error) {
                    one.addProperty("error", String.valueOf(error));
                }
                cases.add(one);
            }
            return cases;
        }

        /** 官方 loadStatic 读边界状态的 NBT，再 saveWithFullMetadata 写回来。 */
        private JsonArray blockEntitiesLoad(JsonObject request) {
            JsonArray cases = new JsonArray();
            BlockPos pos = new BlockPos(3, 64, 5);
            for (var element : request.getAsJsonArray("entities")) {
                JsonObject entry = element.getAsJsonObject();
                JsonObject one = new JsonObject();
                one.addProperty("name", entry.get("name").getAsString());
                try {
                    CompoundTag tag = (CompoundTag) tagFrom(entry.getAsJsonObject("tag"));
                    BlockState state = BlockState.CODEC
                            .parse(NbtOps.INSTANCE, StringTag.valueOf(entry.get("state").getAsString()))
                            .getOrThrow();
                    BlockEntity loaded = BlockEntity.loadStatic(pos, state, tag, official.registries);
                    if (loaded == null) {
                        one.addProperty("error", "loadStatic returned null");
                    } else {
                        TagValueOutput out = TagValueOutput.createWithContext(
                                ProblemReporter.DISCARDING, official.registries);
                        loaded.saveWithFullMetadata(out);
                        CompoundTag written = out.buildResult();
                        written.putBoolean("keepPacked", false);
                        one.add("written", canonical(written));
                    }
                } catch (Exception error) {
                    one.addProperty("error", String.valueOf(error));
                }
                cases.add(one);
            }
            return cases;
        }

        /** SavedTick.codec + LevelChunkTicks.pack/unpack。 */
        private JsonArray ticks() {
            JsonArray cases = new JsonArray();
            BlockPos pos = new BlockPos(3, 64, 5);
            cases.add(oneTickList(
                    "blocks",
                    SavedTick.codec(BuiltInRegistries.BLOCK.byNameCodec()),
                    List.of(
                            new SavedTick<>(Blocks.STONE, pos, 7, TickPriority.HIGH),
                            new SavedTick<>(Blocks.SAND, pos.above(), 0, TickPriority.NORMAL),
                            new SavedTick<>(Blocks.STONE, pos.below(), 1000, TickPriority.EXTREMELY_LOW)),
                    true));
            cases.add(oneTickList(
                    "fluids",
                    SavedTick.codec(BuiltInRegistries.FLUID.byNameCodec()),
                    List.of(
                            new SavedTick<>(Fluids.WATER, pos, -3, TickPriority.VERY_HIGH),
                            new SavedTick<>(Fluids.FLOWING_LAVA, pos, 12, TickPriority.LOW)),
                    false));
            return cases;
        }

        private <T> JsonObject oneTickList(
                String name,
                com.mojang.serialization.Codec<SavedTick<T>> codec,
                List<SavedTick<T>> ticks,
                boolean repack) {
            JsonObject one = new JsonObject();
            one.addProperty("name", name);
            try {
                CompoundTag root = new CompoundTag();
                ListTag list = new ListTag();
                for (SavedTick<T> tick : ticks) {
                    list.add(codec.encodeStart(NbtOps.INSTANCE, tick).getOrThrow());
                }
                root.put("Ticks", list);
                one.add("canonical", canonical(root));
                java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
                NbtIo.write(root, new java.io.DataOutputStream(bytes));
                one.addProperty("encoded", Base64.getEncoder().encodeToString(bytes.toByteArray()));
                if (repack) {
                    @SuppressWarnings("unchecked")
                    List<SavedTick<Block>> blocks = (List<SavedTick<Block>>) (List<?>) ticks;
                    LevelChunkTicks<Block> queue = new LevelChunkTicks<>(blocks);
                    queue.unpack(1234L);
                    List<SavedTick<Block>> packed = queue.pack(1234L);
                    ListTag repacked = new ListTag();
                    for (SavedTick<?> tick : packed) {
                        @SuppressWarnings("unchecked")
                        SavedTick<T> typed = (SavedTick<T>) tick;
                        repacked.add(codec.encodeStart(NbtOps.INSTANCE, typed).getOrThrow());
                    }
                    one.add("repacked", canonical(repacked));
                } else {
                    one.add("repacked", canonical(list));
                }
            } catch (Exception error) {
                one.addProperty("error", String.valueOf(error));
            }
            return one;
        }

        /** 官方真的 load 三种实体，再存回去按键比较。 */
        private JsonArray entities(JsonObject request) {
            JsonArray cases = new JsonArray();
            JsonArray entries = request.getAsJsonArray("entities");
            FakeLevel level = FakeLevel.create(official, 1L);
            for (int index = 0; index < entries.size(); index++) {
                JsonObject one = new JsonObject();
                one.addProperty("index", index);
                try {
                    Tag raw = tagFrom(entries.get(index).getAsJsonObject());
                    if (!(raw instanceof CompoundTag tag)) {
                        throw new IllegalArgumentException("entity entry is not a compound tag");
                    }
                    ValueInput input = TagValueInput.create(ProblemReporter.DISCARDING, official.registries, tag);
                    Entity loaded = EntityType.loadEntityRecursive(
                            input, level, EntitySpawnReason.LOAD, EntityProcessor.NOP);
                    if (loaded == null) {
                        one.addProperty("error", "official could not load this entity");
                    } else {
                        TagValueOutput output = TagValueOutput.createWithContext(
                                ProblemReporter.DISCARDING, official.registries);
                        if (!loaded.save(output)) {
                            one.addProperty("error", "official refused to save this entity");
                        } else {
                            CompoundTag written = output.buildResult();
                            one.add("original", canonical(tag));
                            one.add("written", canonical(written));
                        }
                    }
                } catch (Exception error) {
                    one.addProperty("error", String.valueOf(error));
                }
                cases.add(one);
            }
            return cases;
        }

        private JsonArray blockEntityKeys(CompoundTag root) {
            JsonArray out = new JsonArray();
            Optional<ListTag> list = root.getList("block_entities");
            if (list.isEmpty()) {
                return out;
            }
            for (Tag value : list.get()) {
                if (value instanceof CompoundTag tag) {
                    JsonArray keys = new JsonArray();
                    List<String> names = new ArrayList<>(tag.keySet());
                    names.sort(String::compareTo);
                    for (String key : names) {
                        keys.add(key);
                    }
                    out.add(keys);
                }
            }
            return out;
        }
    }

    private record NamedEntity(String name, BlockEntity entity, BlockState state) {
    }

    /** 把 JSON 描述的简单 NBT（Rust 侧的 canonical 形式）还原成官方 Tag。 */
    private static Tag tagFrom(JsonObject object) {
        int id = object.get("type").getAsInt();
        switch (id) {
            case 1:
                return net.minecraft.nbt.ByteTag.valueOf((byte) object.get("value").getAsInt());
            case 2:
                return net.minecraft.nbt.ShortTag.valueOf((short) object.get("value").getAsInt());
            case 3:
                return net.minecraft.nbt.IntTag.valueOf(object.get("value").getAsInt());
            case 4:
                return net.minecraft.nbt.LongTag.valueOf(object.get("value").getAsLong());
            case 5:
                return net.minecraft.nbt.FloatTag.valueOf(object.get("value").getAsFloat());
            case 6:
                return net.minecraft.nbt.DoubleTag.valueOf(object.get("value").getAsDouble());
            case 8:
                return StringTag.valueOf(object.get("value").getAsString());
            case 7: {
                byte[] values = new byte[object.getAsJsonArray("values").size()];
                for (int i = 0; i < values.length; i++) {
                    values[i] = (byte) object.getAsJsonArray("values").get(i).getAsInt();
                }
                return new net.minecraft.nbt.ByteArrayTag(values);
            }
            case 11: {
                int[] values = new int[object.getAsJsonArray("values").size()];
                for (int i = 0; i < values.length; i++) {
                    values[i] = object.getAsJsonArray("values").get(i).getAsInt();
                }
                return new net.minecraft.nbt.IntArrayTag(values);
            }
            case 12: {
                long[] values = new long[object.getAsJsonArray("values").size()];
                for (int i = 0; i < values.length; i++) {
                    values[i] = object.getAsJsonArray("values").get(i).getAsLong();
                }
                return new net.minecraft.nbt.LongArrayTag(values);
            }
            case 9: {
                ListTag list = new ListTag();
                for (var value : object.getAsJsonArray("values")) {
                    list.add(tagFrom(value.getAsJsonObject()));
                }
                return list;
            }
            case 10: {
                CompoundTag tag = new CompoundTag();
                for (var entry : object.getAsJsonObject("values").entrySet()) {
                    tag.put(entry.getKey(), tagFrom(entry.getValue().getAsJsonObject()));
                }
                return tag;
            }
            default:
                throw new IllegalArgumentException("tag type " + id);
        }
    }

    /** CompoundTag 用 HashMap，键序不确定：转成按键排序的规范 JSON 再比。 */
    private static JsonObject canonical(Tag tag) {
        JsonObject out = new JsonObject();
        out.addProperty("type", tag.getId());
        switch (tag.getId()) {
            case 1, 2, 3, 4, 5, 6 -> out.addProperty("value", String.valueOf(((net.minecraft.nbt.NumericTag) tag).box()));
            case 8 -> out.addProperty("value", ((StringTag) tag).value());
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

    private static Unsafe loadUnsafe() {
        try {
            Field field = Unsafe.class.getDeclaredField("theUnsafe");
            field.setAccessible(true);
            return (Unsafe) field.get(null);
        } catch (Exception error) {
            throw new IllegalStateException(error);
        }
    }

    private static <T> T allocate(Class<T> type) {
        try {
            @SuppressWarnings("unchecked")
            T value = (T) UNSAFE.allocateInstance(type);
            return value;
        } catch (Exception error) {
            throw new IllegalStateException(error);
        }
    }

    private static void set(Class<?> owner, Object target, String name, Object value) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            field.set(target, value);
        } catch (Exception error) {
            throw new IllegalStateException(name, error);
        }
    }
}
