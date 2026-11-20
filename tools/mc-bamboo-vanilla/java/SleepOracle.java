// 用官方 26.3 的 `BedRule` / `Level.isDarkOutside` / `EnvironmentAttributes` /
// `SleepStatus` / `ClockTimeMarker` / `GameRules` 类对拍 Bamboo 的睡眠判定和跳早数学。
// 只加载官方类，不启动服务端、不接受 EULA、不抄反编译代码。
//
// Compare Bamboo's sleep decisions and morning-skip maths with official 26.3
// `BedRule` / `Level.isDarkOutside` / `EnvironmentAttributes` / `SleepStatus` /
// `ClockTimeMarker` / `GameRules`. Official classes only, no server, no EULA,
// no decompiled code.

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
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
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
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.dedicated.DedicatedPlayerList;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.players.SleepStatus;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.util.random.WeightedList;
import net.minecraft.world.TickRateManager;
import net.minecraft.world.attribute.BedRule;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.attribute.EnvironmentAttributes;
import net.minecraft.world.attribute.WeatherAttributes;
import net.minecraft.world.clock.ClockManager;
import net.minecraft.world.clock.ClockTimeMarker;
import net.minecraft.world.clock.PackedClockStates;
import net.minecraft.world.clock.ServerClockManager;
import net.minecraft.world.clock.WorldClock;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.boss.enderdragon.EnderDragonPart;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.crafting.RecipeAccess;
import net.minecraft.world.level.ExplosionDamageCalculator;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.Level.ExplosionInteraction;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.entity.BlockEntity;
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

public final class SleepOracle {
    private static final Map<String, Field> FIELDS = new HashMap<>();
    private static final Unsafe UNSAFE = loadUnsafe();

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
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), 1 << 20)) {
            out.write(query(resources).toString());
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
        Holder.Reference<WorldClock> overworldClock = world.lookupOrThrow(Registries.WORLD_CLOCK)
                .getOrThrow(net.minecraft.world.clock.WorldClocks.OVERWORLD);
        Holder<DimensionType> overworld = world.lookupOrThrow(Registries.DIMENSION_TYPE)
                .getOrThrow(BuiltinDimensionTypes.OVERWORLD);
        Holder<Biome> plains = world.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);

        JsonObject result = new JsonObject();
        result.add("darkness", darkness(world, overworldClock, overworld, plains));
        result.add("sleep_status", sleepStatus());
        result.add("wake_up", wakeUp(overworldClock));
        return result;
    }

    /** 真的构造官方 `Level`，按官方时钟 / 天气算 `updateSkyBrightness()`，
     *  再调 `BedRule.canSleep(level)`（内部走 `Level.isDarkOutside`）。 */
    private static JsonArray darkness(RegistryAccess.Frozen world,
            Holder.Reference<WorldClock> overworldClock, Holder<DimensionType> overworld,
            Holder<Biome> plains) throws Exception {
        JsonArray out = new JsonArray();
        // 主世界 `BED_RULE` 的 `can_sleep = when_dark`；`BedRule.canSleep(level)`
        // 内部调 `Rule.test(level)` → `Level.isDarkOutside()`。
        BedRule bedRule = BedRule.CAN_SLEEP_WHEN_DARK;
        for (int tick : new int[] {0, 133, 1000, 6000, 11867, 12768, 13670, 18000, 22330, 23999}) {
            for (int weather = 0; weather < 3; weather++) {
                FakeLevel level = FakeLevel.create(world, overworldClock, overworld, plains,
                        tick, weather >= 1, weather == 2);
                level.updateSkyBrightness();
                JsonObject entry = new JsonObject();
                entry.addProperty("clock_ticks", tick);
                entry.addProperty("rain", weather >= 1);
                entry.addProperty("thunder", weather == 2);
                entry.addProperty("sky_darken", level.getSkyDarken());
                entry.addProperty("sky_light_level", level.attributes.getDimensionValue(EnvironmentAttributes.SKY_LIGHT_LEVEL));
                // 官方方法链：`BedRule.canSleep` → `BedRule.Rule.WHEN_DARK.test` →
                // `Level.isDarkOutside` → `Level.isBrightOutside` → `skyDarken`。
                entry.addProperty("can_sleep", bedRule.canSleep(level));
                entry.addProperty("dark_outside", level.isDarkOutside());
                out.add(entry);
            }
        }
        return out;
    }

    private static JsonArray sleepStatus() throws Exception {
        JsonArray out = new JsonArray();
        GameRules rules = new GameRules(FeatureFlags.DEFAULT_FLAGS);
        Field active = SleepStatus.class.getDeclaredField("activePlayers");
        Field sleeping = SleepStatus.class.getDeclaredField("sleepingPlayers");
        active.setAccessible(true);
        sleeping.setAccessible(true);
        for (int percentage : new int[] {0, 1, 25, 50, 99, 100, 101, 200}) {
            rules.set(GameRules.PLAYERS_SLEEPING_PERCENTAGE, percentage, null);
            for (int total : new int[] {0, 1, 2, 3, 4, 7, 20}) {
                for (int asleep : new int[] {0, 1, 2, 3, 4, 7, 20}) {
                    SleepStatus status = new SleepStatus();
                    active.setInt(status, total);
                    sleeping.setInt(status, asleep);
                    JsonObject entry = new JsonObject();
                    entry.addProperty("percentage", percentage);
                    entry.addProperty("active", total);
                    entry.addProperty("sleeping", asleep);
                    entry.addProperty("needed", status.sleepersNeeded(percentage));
                    entry.addProperty("enough", status.areEnoughSleeping(percentage));
                    entry.addProperty("can_sleep_through_night", percentage <= 100);
                    out.add(entry);
                }
            }
        }
        return out;
    }

    private static JsonArray wakeUp(Holder.Reference<WorldClock> overworld) {
        JsonArray out = new JsonArray();
        ClockTimeMarker marker = new ClockTimeMarker(overworld, 0, Optional.of(24_000), false);
        for (long ticks : new long[] {0, 1, 133, 6_000, 13_670, 18_000, 23_999, 24_000, 24_001, 42_000, -1, -24_000}) {
            JsonObject entry = new JsonObject();
            entry.addProperty("total_ticks", ticks);
            entry.addProperty("wake_up_ticks", marker.resolveTimeToMoveTo(ticks));
            entry.addProperty("occurs_at", marker.occursAt(ticks));
            out.add(entry);
        }
        return out;
    }

    private static Unsafe loadUnsafe() {
        try {
            Field f = Unsafe.class.getDeclaredField("theUnsafe");
            f.setAccessible(true);
            return (Unsafe) f.get(null);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static Field field(Class<?> owner, String name) {
        return FIELDS.computeIfAbsent(owner.getName() + '#' + name, key -> {
            try {
                Field f = owner.getDeclaredField(name);
                f.setAccessible(true);
                return f;
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

    /** 只有环境属性、天气和时钟字段的 `Level`；`updateSkyBrightness` /
     *  `isDarkOutside` / `getSkyDarken` 都是官方自己的实现。 */
    public static final class FakeLevel extends Level {
        public RegistryAccess.Frozen access;
        public Holder<DimensionType> dimensionType;
        public ServerClockManager clocks;
        public EnvironmentAttributeSystem attributes;
        public BiomeManager biomeManager;

        public FakeLevel() { super(null, null, null, null, false, false, 0L, 0); }

        static FakeLevel create(RegistryAccess.Frozen access, Holder.Reference<WorldClock> overworldClock,
                Holder<DimensionType> overworld, Holder<Biome> plains, long tick, boolean rain,
                boolean thunder) throws Exception {
            FakeLevel level = allocate(FakeLevel.class);
            level.access = access;
            level.dimensionType = overworld;
            level.biomeManager = new BiomeManager((x, y, z) -> plains, 0L);
            set(Level.class, level, "dimensionTypeRegistration", overworld);
            set(Level.class, level, "registryAccess", access);
            set(Level.class, level, "dimension", Level.OVERWORLD);
            set(Level.class, level, "random", RandomSource.create(0L));

            GameRules rules = new GameRules(FeatureFlags.DEFAULT_FLAGS);
            ServerClockManager clocks = allocate(ServerClockManager.class);
            set(ServerClockManager.class, clocks, "packedClockStates", PackedClockStates.EMPTY);
            set(ServerClockManager.class, clocks, "clocks", new HashMap<>());
            level.clocks = clocks;
            clocks.init(FakeServer.create(access, rules, clocks));
            // 用官方 `loadFrom` 把时钟钉在题目的刻上（不改流速 / 暂停）。
            net.minecraft.world.clock.ServerClockManager.ServerClockInstance instance =
                    (net.minecraft.world.clock.ServerClockManager.ServerClockInstance)
                            clocks.getInstance(overworldClock);
            instance.loadFrom(new net.minecraft.world.clock.ClockState(tick, 0.0F, 1.0F, false));

            EnvironmentAttributeSystem.Builder builder = EnvironmentAttributeSystem.builder()
                    .addConstantLayer(overworld.value().attributes());
            for (Holder<net.minecraft.world.timeline.Timeline> timeline : overworld.value().timelines()) {
                builder.addTimelineLayer(timeline, clocks);
            }
            float r = rain ? 1.0F : 0.0F;
            float t = thunder ? 1.0F : 0.0F;
            set(Level.class, level, "rainLevel", r);
            set(Level.class, level, "oRainLevel", r);
            set(Level.class, level, "thunderLevel", t);
            set(Level.class, level, "oThunderLevel", t);
            if (level.canHaveWeather()) {
                WeatherAttributes.addBuiltinLayers(builder, WeatherAttributes.WeatherAccess.from(level));
            }
            level.attributes = builder.build();
            return level;
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
