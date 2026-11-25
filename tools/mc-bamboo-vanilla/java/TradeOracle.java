import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.serialization.JsonOps;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.world.RandomSequences;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntitySpawnRequest;
import net.minecraft.world.entity.npc.villager.Villager;
import net.minecraft.world.entity.npc.wanderingtrader.WanderingTrader;
import net.minecraft.world.entity.npc.villager.VillagerData;
import net.minecraft.world.entity.npc.villager.VillagerProfession;
import net.minecraft.world.entity.npc.villager.VillagerType;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.trading.MerchantOffer;
import net.minecraft.world.item.trading.MerchantOffers;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import sun.misc.Unsafe;

/** 用官方 `Villager.updateTrades` 给 Bamboo 的交易生成出题。 */
public final class TradeOracle {
    private static final PrintStream OUT =
            new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8);
    private static final Unsafe UNSAFE = loadUnsafe();
    private static final String[] PROFESSIONS = {
        "armorer", "butcher", "cartographer", "cleric", "farmer", "fisherman", "fletcher",
        "leatherworker", "librarian", "mason", "shepherd", "toolsmith", "weaponsmith"
    };
    private static final String[] TYPES = {
        "desert", "jungle", "plains", "savanna", "snow", "swamp", "taiga"
    };
    private static final long[] SEEDS = {0L, 1L, 123456789L, -987654321L};

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Throwable error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        System.setOut(log);
        String only = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--only" -> only = args[++i];
                default -> throw new IllegalArgumentException("usage: TradeOracle [--only PREFIX]");
            }
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            World world = World.load(resources);
            int cases = 0;
            for (String profession : PROFESSIONS) {
                for (int level = 1; level <= 5; level++) {
                    for (long seed : SEEDS) {
                        for (String type : TYPES) {
                            String name = profession + "/" + level;
                            if (only != null && !name.startsWith(only)) continue;
                            JsonObject line = world.caseOf(profession, level, type, seed);
                            cases++;
                            OUT.println(line);
                        }
                    }
                }
            }
            // 流浪商人没有职业 / 等级 / 品种：三张交易集一次求值，四个世界种子各一题。
            // Wandering traders have no profession/level/type: one evaluation of
            // the three trade sets per world seed.
            for (long seed : SEEDS) {
                String name = "wandering_trader";
                if (only != null && !name.startsWith(only)) continue;
                JsonObject line = world.wanderingCaseOf(seed);
                cases++;
                OUT.println(line);
            }
            JsonObject summary = new JsonObject();
            summary.addProperty("kind", "summary");
            summary.addProperty("cases", cases);
            OUT.println(summary);
        }
    }

    /** 一份交易题：职业、等级、品种、世界种子，和官方 `MerchantOffers.CODEC` 的 JSON。 */
    private static JsonObject caseLine(String profession, int level, String type, long seed,
            MerchantOffers offers, RegistryOps<JsonElement> ops) {
        JsonObject line = new JsonObject();
        line.addProperty("kind", "trade");
        line.addProperty("profession", profession);
        line.addProperty("level", level);
        line.addProperty("type", type);
        line.addProperty("seed", Long.toString(seed));
        JsonElement encoded = MerchantOffers.CODEC
                .encodeStart(ops, offers)
                .getOrThrow(error -> new IllegalStateException("cannot encode offers: " + error));
        line.add("offers", encoded);
        return line;
    }

    private static final class World {
        final RegistryAccess registries;
        final Holder<Biome> plains;
        final FakeServer server;
        final FakeLevel level;
        final RegistryOps<JsonElement> ops;

        World(RegistryAccess registries, Holder<DimensionType> overworld, Holder<Biome> plains,
                ReloadableServerRegistries.Holder full, FakeServer server, FakeLevel level) {
            this.registries = registries;
            this.plains = plains;
            this.server = server;
            this.level = level;
            this.ops = registries.createSerializationContext(JsonOps.INSTANCE);
            level.server = server;
            level.plains = plains;
            set(Level.class, level, "dimensionTypeRegistration", overworld);
            set(Level.class, level, "dimension", Level.OVERWORLD);
            set(Level.class, level, "registryAccess", registries);
            set(Level.class, level, "damageSources", new net.minecraft.world.damagesource.DamageSources(registries));
            set(ServerLevel.class, level, "server", server);
            set(Level.class, level, "levelData", new FakeLevelData());
            set(ServerLevel.class, level, "environmentAttributes",
                    net.minecraft.world.attribute.EnvironmentAttributeSystem.builder()
                            .addStaticLayers(level).build());
            set(ServerLevel.class, level, "structureManager",
                    new net.minecraft.world.level.StructureManager(level,
                            new net.minecraft.world.level.levelgen.WorldOptions(0L, true, false), null));
            server.registries = full;
        }

        static World load(CloseableResourceManager resources) throws Exception {
            LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
            RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
            RegistryAccess.Frozen worldGen = RegistryDataLoader
                    .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool()).join();
            var layers = initial.replaceFrom(RegistryLayer.WORLD, worldGen);
            ReloadableServerResources managers = ReloadableServerResources.loadResources(
                            resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                            Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                            ForkJoinPool.commonPool(), Runnable::run).get();
            managers.updateComponentsAndStaticRegistryTags();
            RegistryAccess registries = (RegistryAccess) managers.fullRegistries().lookup();
            Holder<DimensionType> overworld =
                    registries.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            Holder<Biome> plains = registries.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
            return new World(registries, overworld, plains, managers.fullRegistries(),
                    allocate(FakeServer.class), allocate(FakeLevel.class));
        }

        JsonObject caseOf(String profession, int level, String type, long seed) throws Exception {
            server.sequences = new RandomSequences();
            server.worldSeed = seed;
            set(Level.class, this.level, "random", RandomSource.create(seed));
            Villager mob = (Villager) net.minecraft.world.entity.EntityTypes.VILLAGER.create(
                    this.level, EntitySpawnReason.COMMAND);
            if (mob == null) throw new IllegalStateException("cannot create villager");
            mob.setPos(1.5, 64.0, 2.5);
            var types = registries.lookupOrThrow(Registries.VILLAGER_TYPE);
            Holder<VillagerType> variant = types
                    .get(Identifier.withDefaultNamespace(type))
                    .orElseThrow(() -> new IllegalStateException("type " + type + " of " + types.keySet()));
            var jobs = registries.lookupOrThrow(Registries.VILLAGER_PROFESSION);
            Holder<VillagerProfession> job = jobs
                    .get(Identifier.withDefaultNamespace(profession))
                    .orElseThrow(() -> new IllegalStateException("profession " + profession + " of " + jobs.keySet()));
            mob.setVillagerData(new VillagerData(variant, job, level));
            MerchantOffers offers = mob.getOffers();
            return caseLine(profession, level, type, seed, offers, ops);
        }

        JsonObject wanderingCaseOf(long seed) throws Exception {
            server.sequences = new RandomSequences();
            server.worldSeed = seed;
            set(Level.class, this.level, "random", RandomSource.create(seed));
            WanderingTrader mob = (WanderingTrader) net.minecraft.world.entity.EntityTypes.WANDERING_TRADER.create(
                    this.level, EntitySpawnReason.COMMAND);
            if (mob == null) throw new IllegalStateException("cannot create wandering trader");
            mob.setPos(1.5, 64.0, 2.5);
            MerchantOffers offers = mob.getOffers();
            JsonObject line = new JsonObject();
            line.addProperty("kind", "wandering_trade");
            line.addProperty("seed", Long.toString(seed));
            JsonElement encoded = MerchantOffers.CODEC
                    .encodeStart(ops, offers)
                    .getOrThrow(error -> new IllegalStateException("cannot encode offers: " + error));
            line.add("offers", encoded);
            return line;
        }

    }

    private static final class FakeLevelData implements net.minecraft.world.level.storage.WritableLevelData {
        @Override public RespawnData getRespawnData() { return RespawnData.DEFAULT; }
        @Override public long getGameTime() { return 0L; }
        @Override public boolean isHardcore() { return false; }
        @Override public net.minecraft.world.Difficulty getDifficulty() { return net.minecraft.world.Difficulty.NORMAL; }
        @Override public boolean isDifficultyLocked() { return false; }
        @Override public void setSpawn(RespawnData spawn) {}
    }

    /** 不查区块就返回 INVALID 的 StructureStart，让 claimStructureAt 安静跳过。 */
    private static final class NoStructures extends net.minecraft.world.level.StructureManager {
        NoStructures(net.minecraft.world.level.LevelAccessor level) {
            super(level, new net.minecraft.world.level.levelgen.WorldOptions(0L, true, false), null);
        }
        @Override public net.minecraft.world.level.levelgen.structure.StructureStart getStructureAt(
                BlockPos pos, net.minecraft.core.HolderSet<net.minecraft.world.level.levelgen.structure.Structure> structures) {
            return net.minecraft.world.level.levelgen.structure.StructureStart.INVALID_START;
        }
    }

    private static final class FakeServer extends DedicatedServer {
        ReloadableServerRegistries.Holder registries;
        RandomSequences sequences = new RandomSequences();
        long worldSeed;

        private FakeServer() { super(null, null, null, null, null, null, null, null, null, null); }

        @Override public RandomSource getRandomSequence(Identifier key) {
            return sequences.get(key, worldSeed);
        }
        @Override public ReloadableServerRegistries.Holder reloadableRegistries() { return registries; }
    }

    /** 和 LootOracle 的假世界一样：全是空气、平原、看得到天。 */
    private static final class FakeLevel extends ServerLevel {
        FakeServer server;
        Holder<Biome> plains;
        int nextEntityId = 1;

        private FakeLevel() { super(null, null, null, null, null, null, false, 0L, null, false); }

        @Override public net.minecraft.server.MinecraftServer getServer() { return server; }
        @Override public FeatureFlagSet enabledFeatures() { return FeatureFlags.DEFAULT_FLAGS; }
        @Override public BlockState getBlockState(BlockPos pos) { return Blocks.AIR.defaultBlockState(); }
        @Override public boolean isLoaded(BlockPos pos) { return true; }
        @Override public Holder<Biome> getBiome(BlockPos pos) { return plains; }
        @Override public boolean canSeeSky(BlockPos pos) { return true; }
        @Override public int getMaxLocalRawBrightness(BlockPos pos) { return 15; }
        @Override public int getMaxLocalRawBrightness(BlockPos pos, int skyDarkening) { return 15; }
        @Override public boolean isRaining() { return false; }
        @Override public boolean isThundering() { return false; }
        @Override public int getSeaLevel() { return 63; }
        @Override public int getNextEntityId() { return nextEntityId++; }
        @Override public net.minecraft.core.BlockPos findNearestMapStructure(
                net.minecraft.core.HolderSet<net.minecraft.world.level.levelgen.structure.Structure> destination,
                BlockPos pos, int radius, boolean skipKnown) {
            // 假世界里没有结构，官方的地图函数因此原样退回输入物品堆。
            return null;
        }
        @Override public net.minecraft.world.level.StructureManager structureManager() {
            return new NoStructures(this);
        }
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

    private static <T> T allocate(Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static void set(Class<?> owner, Object target, String name, Object value) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            field.set(target, value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(owner + "." + name, error);
        }
    }
}
