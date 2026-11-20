// 用官方 26.3 的时钟 / 时间线 / 环境属性类逐刻对拍时钟状态和 `getSkyDarken()`。
// 只在 JVM 里调官方类，不启动服务端、不接受 EULA、不抄反编译代码。
//
// Compare clock state and `getSkyDarken()` tick by tick with official 26.3
// clock / timeline / environment-attribute classes. This only calls official
// classes in a JVM: no server, no EULA, no decompiled code.

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.particles.ExplosionParticleInfo;
import net.minecraft.core.particles.ParticleOptions;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.dedicated.DedicatedPlayerList;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.util.random.WeightedList;
import net.minecraft.world.TickRateManager;
import net.minecraft.world.attribute.EnvironmentAttribute;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.attribute.EnvironmentAttributes;
import net.minecraft.world.attribute.WeatherAttributes;
import net.minecraft.world.clock.ClockInstance;
import net.minecraft.world.clock.ClockManager;
import net.minecraft.world.clock.ClockState;
import net.minecraft.world.clock.PackedClockStates;
import net.minecraft.world.clock.ServerClockManager;
import net.minecraft.world.clock.WorldClock;
import net.minecraft.world.clock.WorldClocks;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.boss.enderdragon.EnderDragonPart;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.crafting.RecipeAccess;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.ExplosionDamageCalculator;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.Level.ExplosionInteraction;
import net.minecraft.world.level.ScheduledTickAccess;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkSource;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.saveddata.maps.MapId;
import net.minecraft.world.level.saveddata.maps.MapItemSavedData;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.scores.Scoreboard;
import net.minecraft.world.ticks.LevelTickAccess;
import net.minecraft.world.ticks.LevelTicks;
import net.minecraft.world.ticks.ScheduledTick;
import net.minecraft.world.ticks.TickPriority;
import sun.misc.Unsafe;

public final class ClockOracle {
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final Map<String, Field> FIELDS = new HashMap<>();
    private static final Unsafe UNSAFE = loadUnsafe();

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(log);
        } catch (Exception error) {
            error.printStackTrace(log);
            // 官方后台线程不是守护线程，不主动退出进程就不会结束。
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
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            JsonObject result = null;
            result = query(resources);
            out.write(result.toString());
            out.write('\n');
        }
    }

    private static JsonObject query(CloseableResourceManager resources) throws Exception {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                .join();
        LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
        RegistryAccess.Frozen access = layers.getAccessForLoading(RegistryLayer.DIMENSIONS);
        Holder.Reference<WorldClock> overworldClock = access.lookupOrThrow(Registries.WORLD_CLOCK).getOrThrow(WorldClocks.OVERWORLD);
        Holder.Reference<WorldClock> endClock = access.lookupOrThrow(Registries.WORLD_CLOCK).getOrThrow(WorldClocks.THE_END);
        Holder<DimensionType> overworld = access.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(BuiltinDimensionTypes.OVERWORLD);
        Holder<Biome> plains = access.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);

        JsonObject result = new JsonObject();
        JsonArray scenes = new JsonArray();
        // 晴天 / 雨 / 雷暴 / 渐变中的雨、时间规则关闭、/time set 跳变、非整数流速、暂停，
        // 以及从夜里起跑的一整个昼夜循环。每个场景都逐刻比两个时钟的状态和 skyDarken。
        //
        // Clear / rain / thunder / rising rain, `advance_time` off, `/time set` jumps,
        // a fractional rate, paused clocks and a full cycle starting at night.
        // Each scene compares both clock states and skyDarken at every tick.
        scenes.add(runScene("full_day", access, overworld, plains, overworldClock, endClock,
                24_100, 0, 0, 0, 0, true, new int[0], 1.0f, false));
        scenes.add(runScene("full_day_from_night", access, overworld, plains, overworldClock, endClock,
                24_100, 0, 0, 0, 0, true, new int[0], 1.0f, false, false, 18_000L));
        scenes.add(runScene("rain", access, overworld, plains, overworldClock, endClock,
                1_200, 1.0f, 1.0f, 1.0f, 0.0f, true, new int[] {100, 13_000}, 1.0f, false));
        scenes.add(runScene("thunder", access, overworld, plains, overworldClock, endClock,
                1_200, 1.0f, 1.0f, 1.0f, 1.0f, true, new int[] {100, 13_000}, 1.0f, false));
        scenes.add(runScene("night_thunder", access, overworld, plains, overworldClock, endClock,
                1_200, 1.0f, 1.0f, 1.0f, 1.0f, true, new int[] {18_000}, 1.0f, false));
        scenes.add(runScene("rising_rain", access, overworld, plains, overworldClock, endClock,
                160, 0.0f, 0.0f, 0.0f, 0.0f, true, new int[0], 1.0f, true));
        scenes.add(runScene("time_rule_off", access, overworld, plains, overworldClock, endClock,
                1_200, 0.0f, 0.0f, 0.0f, 0.0f, false, new int[0], 1.0f, false));
        scenes.add(runScene("jumps", access, overworld, plains, overworldClock, endClock,
                400, 0.0f, 0.0f, 0.0f, 0.0f, true,
                new int[] {0, 1, 132, 133, 11_866, 11_867, 12_768, 13_000, 13_669, 13_670,
                        18_000, 22_329, 22_330, 23_999, 24_000, 12_345}, 1.0f, false));
        scenes.add(runScene("fractional_rate", access, overworld, plains, overworldClock, endClock,
                1_200, 0.0f, 0.0f, 0.0f, 0.0f, true, new int[0], 0.25f, false));
        scenes.add(runScene("paused", access, overworld, plains, overworldClock, endClock,
                1_200, 1.0f, 1.0f, 1.0f, 0.0f, true, new int[0], 2.5f, false, true));
        result.add("scenes", scenes);
        return result;
    }

    private static JsonObject runScene(String name, RegistryAccess.Frozen access,
            Holder<DimensionType> overworld, Holder<Biome> plains,
            Holder.Reference<WorldClock> overworldClock, Holder.Reference<WorldClock> endClock,
            int ticks, float oRain, float rain, float oThunder, float thunder,
            boolean advanceTime, int[] jumps, float rate, boolean rising) throws Exception {
        return runScene(name, access, overworld, plains, overworldClock, endClock,
                ticks, oRain, rain, oThunder, thunder, advanceTime, jumps, rate, rising,
                false, 0L);
    }

    private static JsonObject runScene(String name, RegistryAccess.Frozen access,
            Holder<DimensionType> overworld, Holder<Biome> plains,
            Holder.Reference<WorldClock> overworldClock, Holder.Reference<WorldClock> endClock,
            int ticks, float oRain, float rain, float oThunder, float thunder,
            boolean advanceTime, int[] jumps, float rate, boolean rising,
            boolean paused) throws Exception {
        return runScene(name, access, overworld, plains, overworldClock, endClock,
                ticks, oRain, rain, oThunder, thunder, advanceTime, jumps, rate, rising,
                paused, 0L);
    }

    private static JsonObject runScene(String name, RegistryAccess.Frozen access,
            Holder<DimensionType> overworld, Holder<Biome> plains,
            Holder.Reference<WorldClock> overworldClock, Holder.Reference<WorldClock> endClock,
            int ticks, float oRain, float rain, float oThunder, float thunder,
            boolean advanceTime, int[] jumps, float rate, boolean rising, boolean paused,
            long startTicks) throws Exception {
        GameRules rules = new GameRules(FeatureFlags.DEFAULT_FLAGS);
        rules.set(GameRules.ADVANCE_TIME, advanceTime, null);
        FakeLevel level = allocate(FakeLevel.class);
        level.access = access;
        level.dimensionType = overworld;
        level.rules = rules;
        level.biomeManager = new BiomeManager((x, y, z) -> plains, 0L);
        set(Level.class, level, "dimensionTypeRegistration", overworld);
        set(Level.class, level, "registryAccess", access);
        set(Level.class, level, "dimension", Level.OVERWORLD);
        set(Level.class, level, "random", RandomSource.create(0L));
        set(Level.class, level, "rainLevel", rain);
        set(Level.class, level, "oRainLevel", oRain);
        set(Level.class, level, "thunderLevel", thunder);
        set(Level.class, level, "oThunderLevel", oThunder);

        ServerClockManager clocks = allocate(ServerClockManager.class);
        set(ServerClockManager.class, clocks, "packedClockStates", PackedClockStates.EMPTY);
        set(ServerClockManager.class, clocks, "clocks", new HashMap<>());
        FakeServer server = FakeServer.create(access, rules, clocks);
        clocks.init(server);
        // `setRate` / `setPaused` 的 `modifyClock` 要真的 `ServerLevel` 才能发包；
        // 用官方 `loadFrom` 写同样的初始状态。
        // `setRate` / `setPaused` go through `modifyClock`, which needs a real
        // `ServerLevel` to build a packet. Use official `loadFrom` instead.
        if (rate != 1.0f || paused || startTicks != 0L) {
            for (Holder.Reference<WorldClock> clock : List.of(overworldClock, endClock)) {
                net.minecraft.world.clock.ServerClockManager.ServerClockInstance instance =
                        (net.minecraft.world.clock.ServerClockManager.ServerClockInstance) clocks.getInstance(clock);
                instance.loadFrom(new ClockState(startTicks, 0.0f, rate, paused));
            }
        }
        level.clocks = clocks;
        EnvironmentAttributeSystem.Builder builder = EnvironmentAttributeSystem.builder()
                .addConstantLayer(overworld.value().attributes());
        // 官方 `addDynamicLayers` 的时间线层：只加主世界维度挂着的那些时间线。
        for (Holder<net.minecraft.world.timeline.Timeline> timeline : overworld.value().timelines()) {
            builder.addTimelineLayer(timeline, clocks);
        }
        if (level.canHaveWeather()) {
            WeatherAttributes.addBuiltinLayers(builder, WeatherAttributes.WeatherAccess.from(level));
        }
        level.attributes = builder.build();
        // `ServerLevel` 构造函数末尾调一次；这里等价于开局那一帧。
        level.updateSkyBrightness();

        JsonObject scene = new JsonObject();
        scene.addProperty("name", name);
        scene.addProperty("advance_time", advanceTime);
        scene.addProperty("rate", rate);
        scene.addProperty("paused", paused);
        scene.addProperty("start_ticks", startTicks);
        JsonArray jumpArray = new JsonArray();
        for (int jump : jumps) {
            jumpArray.add(jump);
        }
        scene.add("jumps", jumpArray);
        scene.addProperty("o_rain", oRain);
        scene.addProperty("rain", rain);
        scene.addProperty("o_thunder", oThunder);
        scene.addProperty("thunder", thunder);
        scene.addProperty("initial_sky_darken", level.getSkyDarken());
        JsonArray steps = new JsonArray();
        for (int tick = 0; tick <= ticks; tick++) {
            // 场景自己的时间跳变，等价于 `/time set` 在某刻执行。
            for (int jump : jumps) {
                if (jump == tick) {
                    // `ServerClockManager.setTotalTicks` 里除了改状态就是发包；假服务端没有
                    // 玩家也没有 `ServerLevel`，这里用官方的 loadFrom 写同一份 ClockState。
                    // Besides the state change, `setTotalTicks` only builds a packet.
                    // The fake server has neither players nor a `ServerLevel`, so use
                    // official `loadFrom` with the equivalent `ClockState`.
                    ClockInstance instance = clocks.getInstance(overworldClock);
                    net.minecraft.world.clock.ServerClockManager.ServerClockInstance serverInstance =
                            (net.minecraft.world.clock.ServerClockManager.ServerClockInstance) instance;
                    serverInstance.loadFrom(new ClockState(jump, 0.0f, instance.rate(), instance.isPaused()));
                }
            }
            // 题目给的是稳定的天气；`rising` 场景模拟 `advanceWeatherCycle` 每刻 0.01 的渐变。
            if (rising && tick > 0) {
                float nextRain = Math.min(1.0f, rain + 0.01f * tick);
                set(Level.class, level, "oRainLevel", Math.min(1.0f, rain + 0.01f * (tick - 1)));
                set(Level.class, level, "rainLevel", nextRain);
                set(Level.class, level, "oThunderLevel", 0.0f);
                set(Level.class, level, "thunderLevel", 0.0f);
                JsonObject w = new JsonObject();
                w.addProperty("o_rain", (float) field(Level.class, "oRainLevel").get(level));
                w.addProperty("rain", nextRain);
                w.addProperty("o_thunder", 0.0f);
                w.addProperty("thunder", 0.0f);
                // 天气不写进每一步，Rust 侧按同一公式算出这四个数；这里只留断言点。
                level.attributes.invalidateTickCache();
                level.updateSkyBrightness();
            } else if (tick > 0) {
                level.attributes.invalidateTickCache();
                level.updateSkyBrightness();
            }
            JsonObject step = new JsonObject();
            step.addProperty("tick", tick);
            step.add("overworld", clockState(clocks.getInstance(overworldClock)));
            step.add("the_end", clockState(clocks.getInstance(endClock)));
            step.addProperty("sky_darken", level.getSkyDarken());
            step.addProperty("sky_light_level", level.attributes.getDimensionValue(EnvironmentAttributes.SKY_LIGHT_LEVEL));
            if (rising && tick > 0) {
                step.addProperty("o_rain", (float) field(Level.class, "oRainLevel").get(level));
                step.addProperty("rain", (float) field(Level.class, "rainLevel").get(level));
            }
            steps.add(step);
            // 官方 `tickChildren` 的时钟阶段在 `ServerLevel.tick` 之后（下一刻开头）。
            clocks.tick();
        }
        scene.add("steps", steps);
        return scene;
    }

    private static JsonObject clockState(ClockInstance clock) {
        JsonObject state = new JsonObject();
        state.addProperty("total_ticks", clock.totalTicks());
        state.addProperty("partial_tick", clock.partialTick());
        state.addProperty("rate", clock.rate());
        state.addProperty("paused", clock.isPaused());
        return state;
    }

    private static Unsafe loadUnsafe() {
        try {
            Field field = Unsafe.class.getDeclaredField("theUnsafe");
            field.setAccessible(true);
            return (Unsafe) field.get(null);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static Field field(Class<?> owner, String name) {
        return FIELDS.computeIfAbsent(owner.getName() + '#' + name, key -> {
            try {
                Field field = owner.getDeclaredField(name);
                field.setAccessible(true);
                return field;
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
        });
    }

    private static void set(Class<?> owner, Object target, String name, Object value) {
        try {
            field(owner, name).set(target, value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
            throw new IllegalStateException(error);
        }
    }

    /** 一个只有时间 / 天气 / 维度相关字段的 `Level`；环境属性系统只读它。 */
    public static final class FakeLevel extends Level {
        public RegistryAccess.Frozen access;
        public Holder<DimensionType> dimensionType;
        public GameRules rules;
        public ServerClockManager clocks;
        public EnvironmentAttributeSystem attributes;
        public BiomeManager biomeManager;

        public FakeLevel() {
            super(null, null, null, null, false, false, 0L, 0);
        }

        @Override public ClockManager clockManager() { return clocks; }
        @Override public EnvironmentAttributeSystem environmentAttributes() { return attributes; }
        @Override public RegistryAccess registryAccess() { return access; }
        @Override public Holder<DimensionType> dimensionTypeRegistration() { return dimensionType; }
        @Override public BiomeManager getBiomeManager() { return biomeManager; }
        @Override public LevelData getLevelData() { return null; }
        @Override public ChunkSource getChunkSource() { return null; }
        @Override public WorldBorder getWorldBorder() { return new WorldBorder(); }
        @Override public Scoreboard getScoreboard() { return new Scoreboard(); }
        @Override public RandomSource getRandom() { return RandomSource.create(0); }
        @Override public boolean isClientSide() { return false; }
        @Override public String toString() { return "FakeLevel"; }
        @Override public void sendBlockUpdated(BlockPos p, BlockState a, BlockState b, int f) {}
        @Override public void playSeededSound(Entity e, double x, double y, double z, Holder<SoundEvent> s, SoundSource src, float v, float p, long seed) {}
        @Override public void playSeededSound(Entity a, Entity b, Holder<SoundEvent> s, SoundSource src, float v, float p, long seed) {}
        @Override public void explode(Entity e, DamageSource d, ExplosionDamageCalculator c, double x, double y, double z, float r, boolean f, ExplosionInteraction i, ParticleOptions a, ParticleOptions b, WeightedList<ExplosionParticleInfo> l, Holder<SoundEvent> s) {}
        @Override public String gatherChunkSourceStats() { return ""; }
        @Override public void setRespawnData(LevelData.RespawnData d) {}
        @Override public LevelData.RespawnData getRespawnData() { return null; }
        @Override public Entity getEntity(int id) { return null; }
        @Override public java.util.Collection<EnderDragonPart> dragonParts() { return List.of(); }
        @Override public TickRateManager tickRateManager() { return null; }
        @Override public MapItemSavedData getMapData(MapId id) { return null; }
        @Override public void destroyBlockProgress(int id, BlockPos p, int v) {}
        @Override public RecipeAccess recipeAccess() { return null; }
        @Override protected LevelEntityGetter<Entity> getEntities() { return null; }
        @Override public ChunkAccess getChunk(int x, int z, ChunkStatus status, boolean load) { return null; }
        @Override public boolean hasChunk(int x, int z) { return false; }
        @Override public BlockEntity getBlockEntity(BlockPos p) { return null; }
        @Override public BlockState getBlockState(BlockPos p) { return null; }
        @Override public FluidState getFluidState(BlockPos p) { return null; }
        @Override public int getHeight() { return 384; }
        @Override public int getMinY() { return -64; }
        @Override public void gameEvent(Holder<GameEvent> event, Vec3 pos, GameEvent.Context ctx) {}
        @Override public void levelEvent(Entity e, int type, BlockPos pos, int data) {}
        @Override public FeatureFlagSet enabledFeatures() { return FeatureFlags.DEFAULT_FLAGS; }
        @Override public int getSeaLevel() { return 63; }
        @Override public Holder<Biome> getUncachedNoiseBiome(int x, int y, int z) { return biomeManager.getNoiseBiomeAtQuart(x, y, z); }
        @Override public Holder<Biome> getNoiseBiome(int x, int y, int z) { return getUncachedNoiseBiome(x, y, z); }
        @Override public List<? extends Player> players() { return List.of(); }
        @Override public LevelTickAccess<Block> getBlockTicks() { return new LevelTicks<>(p -> true); }
        @Override public LevelTickAccess<Fluid> getFluidTicks() { return new LevelTicks<>(p -> true); }
        @Override public <T> ScheduledTick<T> createTick(BlockPos p, T t, int d, TickPriority pr) { return null; }
        @Override public <T> ScheduledTick<T> createTick(BlockPos p, T t, int d) { return null; }
        @Override public long nextSubTickCount() { return 0; }
        @Override public void playSound(Entity e, BlockPos p, SoundEvent s, SoundSource src, float v, float pitch) {}
        @Override public void addParticle(ParticleOptions o, double x, double y, double z, double dx, double dy, double dz) {}
    }

    /** `ServerClockManager.init` 只用到注册表、全局规则和玩家列表；不跑真正的服务端。 */
    public static final class FakeServer extends DedicatedServer {
        RegistryAccess.Frozen access;
        GameRules rules;
        ServerClockManager clocks;

        FakeServer() { super(null, null, null, null, null, null, null, null, null, null); }

        static FakeServer create(RegistryAccess.Frozen access, GameRules rules, ServerClockManager clocks) {
            FakeServer server = allocate(FakeServer.class);
            server.access = access;
            server.rules = rules;
            server.clocks = clocks;
            return server;
        }

        @Override public RegistryAccess.Frozen registryAccess() { return access; }
        @Override public GameRules getGlobalGameRules() { return rules; }
        @Override public ServerClockManager clockManager() { return clocks; }
        @Override public DedicatedPlayerList getPlayerList() { return null; }
        @Override public Iterable<ServerLevel> getAllLevels() { return List.of(); }
    }
}
