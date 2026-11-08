import it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap;
import it.unimi.dsi.fastutil.longs.Long2ObjectMap;
import it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.ForkJoinPool;
import java.util.function.Consumer;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
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
import net.minecraft.util.AbortableIterationConsumer;
import net.minecraft.util.RandomSource;
import net.minecraft.world.Difficulty;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.boss.enderdragon.EnderDragonPart;
import net.minecraft.world.entity.item.FallingBlockEntity;
import net.minecraft.world.entity.item.PrimedTnt;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.SpeleothemBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.SpeleothemThickness;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.util.debug.ServerDebugSubscribers;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.redstone.CollectingNeighborUpdater;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.ticks.LevelTicks;
import sun.misc.Unsafe;

/**
 * 官方下落方块和点燃 TNT 的物理。每个场景在假世界里跑，每 tick 打一行 JSON。
 *
 * Official falling-block and primed-TNT physics. Each scene runs in a fake world and prints one JSON line per tick.
 */
public final class EntityOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
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
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8))) {
            Official official = Official.load(resources);
            out.write("{\"type\":\"header\",\"version\":\"");
            out.write(SharedConstants.getCurrentVersion().id());
            out.write("\",\"protocol\":");
            out.write(Integer.toString(SharedConstants.getCurrentVersion().protocolVersion()));
            out.write("}\n");
            int cases = 0;
            cases += sandOnStone(official, out);
            cases += sandOnTorch(official, out);
            cases += concreteIntoWater(official, out);
            cases += dripstoneTip(official, out);
            cases += anvilCrack(official, out);
            cases += tntInAir(official, out);
            cases += tntBreaksStone(official, out);
            cases += tntChain(official, out);
            out.write("{\"type\":\"end\",\"cases\":" + cases + "}\n");
            out.flush();
        }
    }

    private static int sandOnStone(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 70, 0), Blocks.SAND.defaultBlockState());
        play(official, out, "sand_on_stone", "falling", Blocks.SAND.defaultBlockState(), 0, 70, 0, blocks, 1L, 1L, null, null, 40);
        return 1;
    }

    private static int sandOnTorch(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 65, 0), Blocks.TORCH.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 69, 0), Blocks.SAND.defaultBlockState());
        play(official, out, "sand_on_torch", "falling", Blocks.SAND.defaultBlockState(), 0, 69, 0, blocks, 1L, 1L, null, null, 40);
        return 1;
    }

    private static int concreteIntoWater(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.WATER.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 68, 0), Blocks.CONCRETE_POWDER.white().defaultBlockState());
        play(
                official,
                out,
                "concrete_into_water",
                "falling",
                Blocks.CONCRETE_POWDER.white().defaultBlockState(),
                0,
                68,
                0,
                blocks,
                1L,
                1L,
                null,
                null,
                40);
        return 1;
    }

    private static int dripstoneTip(Official official, Writer out) throws Exception {
        BlockState tip = Blocks.POINTED_DRIPSTONE
                .defaultBlockState()
                .setValue(SpeleothemBlock.TIP_DIRECTION, Direction.DOWN)
                .setValue(SpeleothemBlock.THICKNESS, SpeleothemThickness.TIP);
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 70, 0), tip);
        play(official, out, "dripstone_tip", "falling", tip, 0, 70, 0, blocks, 1L, 1L, null, null, 40);
        return 1;
    }

    private static int anvilCrack(Official official, Writer out) throws Exception {
        long seed = crackingSeed(official);
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 80, 0), Blocks.ANVIL.defaultBlockState());
        play(
                official,
                out,
                "anvil_crack",
                "falling",
                Blocks.ANVIL.defaultBlockState(),
                0,
                80,
                0,
                blocks,
                1L,
                seed,
                new float[] {2.0F, 40.0F},
                null,
                80);
        return 1;
    }

    /** 找一颗会把铁砧摔裂的实体随机种子。找不到就用 1，两边仍然要一致。 */
    private static long crackingSeed(Official official) {
        for (long seed = 1; seed <= 64; seed++) {
            Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
            blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
            blocks.put(BlockPos.asLong(0, 80, 0), Blocks.ANVIL.defaultBlockState());
            try {
                Scene scene = Scene.start(
                        official, "falling", Blocks.ANVIL.defaultBlockState(), 0, 80, 0, blocks, 1L, seed, new float[] {2.0F, 40.0F}, null);
                for (int tick = 0; tick < 80 && !scene.active.isEmpty(); tick++) {
                    scene.tick();
                }
                BlockState landed = scene.level.getBlockState(new BlockPos(0, 65, 0));
                if (landed.is(Blocks.CHIPPED_ANVIL)) {
                    return seed;
                }
            } catch (RuntimeException ignored) {
                return 1L;
            }
        }
        return 1L;
    }

    private static int tntInAir(Official official, Writer out) throws Exception {
        play(official, out, "tnt_in_air", "tnt", Blocks.TNT.defaultBlockState(), 0, 80, 0, new Long2ObjectOpenHashMap<>(), 1L, 1L, null, null, 30);
        return 1;
    }

    private static int tntBreaksStone(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        for (int x = -2; x <= 2; x++) {
            for (int z = -2; z <= 2; z++) {
                blocks.put(BlockPos.asLong(x, 64, z), Blocks.STONE.defaultBlockState());
            }
        }
        play(official, out, "tnt_breaks_stone", "tnt", Blocks.TNT.defaultBlockState(), 0, 65, 0, blocks, 3L, 1L, null, 4, 8);
        return 1;
    }

    private static int tntChain(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(1, 65, 0), Blocks.TNT.defaultBlockState());
        blocks.put(BlockPos.asLong(-1, 65, 0), Blocks.TNT.defaultBlockState());
        play(official, out, "tnt_chain", "tnt", Blocks.TNT.defaultBlockState(), 0, 65, 0, blocks, 5L, 1L, null, 4, 8);
        return 1;
    }

    private static void play(
            Official official,
            Writer out,
            String name,
            String kind,
            BlockState state,
            int x,
            int y,
            int z,
            Long2ObjectMap<BlockState> blocks,
            long levelSeed,
            long entitySeed,
            float[] hurts,
            Integer fuse,
            int ticks)
            throws Exception {
        Scene scene = Scene.start(official, kind, state, x, y, z, blocks, levelSeed, entitySeed, hurts, fuse);
        StringBuilder steps = new StringBuilder();
        steps.append('[');
        scene.writeStep(steps);
        for (int tick = 0; tick < ticks; tick++) {
            steps.append(',');
            scene.tick();
            scene.writeStep(steps);
        }
        steps.append(']');
        out.write("{\"type\":\"case\",\"name\":\"");
        out.write(name);
        out.write("\",\"kind\":\"");
        out.write(kind);
        out.write("\",\"state\":");
        out.write(Integer.toString(Block.getId(state)));
        out.write(",\"x\":");
        out.write(Integer.toString(x));
        out.write(",\"y\":");
        out.write(Integer.toString(y));
        out.write(",\"z\":");
        out.write(Integer.toString(z));
        out.write(",\"levelSeed\":");
        out.write(Long.toString(levelSeed));
        out.write(",\"entitySeed\":");
        out.write(Long.toString(entitySeed));
        out.write(",\"hurts\":");
        if (hurts == null) {
            out.write("null");
        } else {
            out.write("[" + hurts[0] + "," + hurts[1] + "]");
        }
        out.write(",\"fuse\":");
        out.write(fuse == null ? "null" : Integer.toString(fuse));
        out.write(",\"blocks\":");
        writeBlocks(out, blocks);
        out.write(",\"steps\":");
        out.write(steps.toString());
        out.write("}\n");
    }

    private static void writeBlocks(Writer out, Long2ObjectMap<BlockState> blocks) throws Exception {
        long[] keys = blocks.keySet().toLongArray();
        Arrays.sort(keys);
        out.write("[");
        boolean first = true;
        for (long key : keys) {
            BlockState state = blocks.get(key);
            if (state.isAir()) {
                continue;
            }
            if (!first) {
                out.write(',');
            }
            first = false;
            BlockPos pos = BlockPos.of(key);
            out.write("[" + pos.getX() + "," + pos.getY() + "," + pos.getZ() + "," + Block.getId(state) + "]");
        }
        out.write("]");
    }

    private static final class Scene {
        final FakeLevel level;
        final List<Entity> active = new ArrayList<>();
        /**
         * tick 结束时的实体，包括这一拍刚丢掉的。
         *
         * Entities at the end of the tick, including ones discarded during it.
         */
        final List<Entity> snapshot = new ArrayList<>();
        boolean ticked;

        private Scene(FakeLevel level) {
            this.level = level;
        }

        static Scene start(
                Official official,
                String kind,
                BlockState state,
                int x,
                int y,
                int z,
                Long2ObjectMap<BlockState> blocks,
                long levelSeed,
                long entitySeed,
                float[] hurts,
                Integer fuse) {
            FakeLevel level = FakeLevel.create(official, levelSeed);
            level.blocks.putAll(blocks);
            BlockPos pos = new BlockPos(x, y, z);
            Entity entity;
            if ("falling".equals(kind)) {
                entity = FallingBlockEntity.fall(level, pos, state);
            } else {
                entity = new PrimedTnt(level, x + 0.5, y, z + 0.5, null);
                level.addFreshEntity(entity);
            }
            setRandom(entity, entitySeed);
            if (hurts != null && entity instanceof FallingBlockEntity falling) {
                falling.setHurtsEntities(hurts[0], (int) hurts[1]);
            }
            if (fuse != null && entity instanceof PrimedTnt tnt) {
                tnt.setFuse(fuse);
            }
            Scene scene = new Scene(level);
            scene.active.addAll(level.fresh);
            level.fresh.clear();
            return scene;
        }

        void tick() {
            List<Entity> ticking = new ArrayList<>(active);
            level.fresh.clear();
            snapshot.clear();
            for (Entity entity : ticking) {
                if (!entity.isRemoved()) {
                    entity.commonTick();
                    entity.tick();
                }
                // 丢掉的实体这一拍还要留下位置，下一拍才从名单里消失。
                // A discarded entity still reports its position for this tick.
                snapshot.add(entity);
            }
            // 掉落物实体不在这一步的范围内。官方会生成，我们只核对下落方块和点燃的 TNT。
            // Item entities are out of scope. Vanilla spawns them; only falling blocks and primed TNT are compared.
            for (Entity spawned : level.fresh) {
                if (spawned instanceof FallingBlockEntity || spawned instanceof PrimedTnt) {
                    snapshot.add(spawned);
                    active.add(spawned);
                }
            }
            active.removeIf(Entity::isRemoved);
            ticked = true;
        }

        void writeStep(StringBuilder out) {
            List<Entity> snapped = ticked ? snapshot : active;
            out.append("{\"entities\":[");
            for (int i = 0; i < snapped.size(); i++) {
                if (i > 0) {
                    out.append(',');
                }
                writeEntity(out, snapped.get(i));
            }
            out.append("],\"blocks\":");
            out.append(blockArray(level.blocks));
            out.append('}');
        }
    }

    private static void writeEntity(StringBuilder out, Entity entity) {
        Vec3 pos = entity.position();
        Vec3 delta = entity.getDeltaMovement();
        String kind = entity instanceof PrimedTnt ? "tnt" : "falling";
        int block = entity instanceof FallingBlockEntity falling ? Block.getId(falling.getBlockState()) : -1;
        int fuse = entity instanceof PrimedTnt tnt ? tnt.getFuse() : -1;
        out.append("{\"kind\":\"").append(kind).append('"');
        out.append(",\"removed\":").append(entity.isRemoved());
        out.append(",\"onGround\":").append(entity.onGround());
        out.append(",\"x\":").append(Double.doubleToRawLongBits(pos.x));
        out.append(",\"y\":").append(Double.doubleToRawLongBits(pos.y));
        out.append(",\"z\":").append(Double.doubleToRawLongBits(pos.z));
        out.append(",\"dx\":").append(Double.doubleToRawLongBits(delta.x));
        out.append(",\"dy\":").append(Double.doubleToRawLongBits(delta.y));
        out.append(",\"dz\":").append(Double.doubleToRawLongBits(delta.z));
        out.append(",\"block\":").append(block);
        out.append(",\"fuse\":").append(fuse);
        out.append('}');
    }

    private static String blockArray(Long2ObjectMap<BlockState> blocks) {
        long[] keys = blocks.keySet().toLongArray();
        Arrays.sort(keys);
        StringBuilder out = new StringBuilder();
        out.append('[');
        boolean first = true;
        for (long key : keys) {
            BlockState state = blocks.get(key);
            if (state.isAir()) {
                continue;
            }
            if (!first) {
                out.append(',');
            }
            first = false;
            BlockPos pos = BlockPos.of(key);
            out.append('[').append(pos.getX()).append(',').append(pos.getY()).append(',').append(pos.getZ());
            out.append(',').append(Block.getId(state)).append(']');
        }
        out.append(']');
        return out.toString();
    }

    /**
     * 把实体自己的随机源换成固定种子。这个字段是 final，普通反射写不进去。
     *
     * Replace the entity's own random source with a fixed seed. The field is final, so plain reflection does not stick.
     */
    private static void setRandom(Entity entity, long seed) {
        try {
            Field field = Entity.class.getDeclaredField("random");
            field.setAccessible(true);
            UNSAFE.putObject(entity, UNSAFE.objectFieldOffset(field), RandomSource.create(seed));
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
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

    @SuppressWarnings("unchecked")
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
            throw new IllegalStateException(error);
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
            this.overworld = registries.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            this.plains = registries.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
            this.damageSources = new DamageSources(registries);
            this.server = FakeServer.create(managers, gameRules);
        }

        static Official load(CloseableResourceManager resources) throws Exception {
            var initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
            RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
            RegistryAccess.Frozen world = RegistryDataLoader
                    .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                    .join();
            var layers = initial.replaceFrom(RegistryLayer.WORLD, world);
            ReloadableServerResources managers = ReloadableServerResources
                    .loadResources(
                            resources,
                            layers,
                            staticTags,
                            FeatureFlags.DEFAULT_FLAGS,
                            net.minecraft.commands.Commands.CommandSelection.DEDICATED,
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
    }

    /** 落地时 onPlace 会排计划刻，只需要游戏时间，别的都用不到。 */
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

    /** 空的战利品表，这样爆炸破坏方块时不从世界随机源取掉落用的数。 */
    private static final class NoLoot extends ReloadableServerRegistries.Holder {
        NoLoot(HolderLookup.Provider registries) {
            super(registries);
        }

        @Override
        public LootTable getLootTable(ResourceKey<LootTable> id) {
            return LootTable.EMPTY;
        }
    }

    private static final class FakeLevel extends ServerLevel {
        private Official official;
        private Long2ObjectOpenHashMap<BlockState> blocks;
        private List<Entity> fresh;
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
            level.nextId = 1;
            level.blocks = new Long2ObjectOpenHashMap<>();
            level.blockTicks = new LevelTicks<>(key -> true);
            level.fluidTicks = new LevelTicks<>(key -> true);
            level.chunkSource = chunkSource();
            set(Level.class, level, "neighborUpdater", new CollectingNeighborUpdater(level, 1_000_000));
            set(Level.class, level, "random", RandomSource.create(seed));
            set(Level.class, level, "dimensionTypeRegistration", official.overworld);
            set(Level.class, level, "dimension", Level.OVERWORLD);
            set(Level.class, level, "registryAccess", official.registries);
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
        public BlockState getBlockState(BlockPos pos) {
            if (!isInsideBuildHeight(pos)) {
                return Blocks.VOID_AIR.defaultBlockState();
            }
            BlockState state = blocks.get(pos.asLong());
            return state != null ? state : Blocks.AIR.defaultBlockState();
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return isInsideBuildHeight(pos) ? getBlockState(pos).getFluidState() : net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int limit) {
            if (!isInsideBuildHeight(pos)) {
                return false;
            }
            BlockState old = getBlockState(pos);
            if (old == state) {
                return false;
            }
            blocks.put(pos.asLong(), state);
            boolean movedByPiston = (flags & 64) != 0;
            if (!old.is(state.getBlock()) && ((flags & 1) != 0 || movedByPiston)) {
                old.affectNeighborsAfterRemoval(this, pos, movedByPiston);
            }
            if (!getBlockState(pos).is(state.getBlock())) {
                return false;
            }
            if ((flags & 512) == 0) {
                state.onPlace(this, pos, old, movedByPiston);
            }
            if (getBlockState(pos) == state && (flags & 1) != 0) {
                updateNeighborsAt(pos, old.getBlock());
                if (state.hasAnalogOutputSignal()) {
                    updateNeighbourForOutputSignal(pos, state.getBlock());
                }
            }
            if ((flags & 16) == 0 && limit > 0) {
                int neighbourFlags = flags & -34;
                old.updateIndirectNeighbourShapes(this, pos, neighbourFlags, limit - 1);
                state.updateNeighbourShapes(this, pos, neighbourFlags, limit - 1);
                state.updateIndirectNeighbourShapes(this, pos, neighbourFlags, limit - 1);
            }
            return true;
        }

        @Override
        public boolean addFreshEntity(Entity entity) {
            entity.setId(getNextEntityId());
            fresh.add(entity);
            return true;
        }

        @Override
        public int getNextEntityId() {
            return nextId++;
        }

        @Override
        public int getMinY() {
            return MIN_Y;
        }

        @Override
        public int getHeight() {
            return HEIGHT;
        }

        @Override
        public int getSeaLevel() {
            return 63;
        }

        @Override
        public LevelTicks<Block> getBlockTicks() {
            return blockTicks;
        }

        @Override
        public LevelTicks<Fluid> getFluidTicks() {
            return fluidTicks;
        }

        @Override
        public ServerChunkCache getChunkSource() {
            return chunkSource;
        }

        @Override
        public BlockGetter getChunkForCollisions(int chunkX, int chunkZ) {
            return this;
        }

        @Override
        public ChunkAccess getChunk(int chunkX, int chunkZ, ChunkStatus status, boolean loadOrGenerate) {
            return null;
        }

        @Override
        public boolean hasChunk(int chunkX, int chunkZ) {
            return true;
        }

        @Override
        public boolean isLoaded(BlockPos pos) {
            return isInsideBuildHeight(pos);
        }

        @Override
        public GameRules getGameRules() {
            return official.gameRules;
        }

        @Override
        public WorldBorder getWorldBorder() {
            return official.border;
        }

        @Override
        public LevelData getLevelData() {
            return official.levelData;
        }

        @Override
        public DedicatedServer getServer() {
            return official.server;
        }

        @Override
        public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public EnvironmentAttributeSystem environmentAttributes() {
            return official.attributes;
        }

        @Override
        public Holder<Biome> getNoiseBiome(int quartX, int quartY, int quartZ) {
            return official.plains;
        }

        @Override
        public Holder<Biome> getUncachedNoiseBiome(int quartX, int quartY, int quartZ) {
            return official.plains;
        }

        @Override
        public Difficulty getDifficulty() {
            return Difficulty.NORMAL;
        }

        @Override
        public void gameEvent(Holder<GameEvent> gameEvent, Vec3 position, GameEvent.Context context) {}

        @Override
        public void sendBlockUpdated(BlockPos pos, BlockState old, BlockState current, int flags) {}

        @Override
        public void levelEvent(Entity source, int type, BlockPos pos, int data) {}

        @Override
        public void playSeededSound(
                Entity except, double x, double y, double z, Holder<net.minecraft.sounds.SoundEvent> sound,
                net.minecraft.sounds.SoundSource source, float volume, float pitch, long seed) {}

        @Override
        public void playSeededSound(
                Entity except, Entity sourceEntity, Holder<net.minecraft.sounds.SoundEvent> sound,
                net.minecraft.sounds.SoundSource source, float volume, float pitch, long seed) {}

        @Override
        public Collection<EnderDragonPart> dragonParts() {
            return List.of();
        }

        @Override
        protected LevelEntityGetter<Entity> getEntities() {
            return EmptyEntities.INSTANCE;
        }

        @Override
        public void updatePOIOnBlockStateChange(BlockPos pos, BlockState oldState, BlockState newState) {}

        @Override
        public StructureManager structureManager() {
            throw new UnsupportedOperationException("structure");
        }
    }

    private static final class EmptyEntities implements LevelEntityGetter<Entity> {
        static final EmptyEntities INSTANCE = new EmptyEntities();

        @Override
        public Entity get(int id) {
            return null;
        }

        @Override
        public Entity get(UUID id) {
            return null;
        }

        @Override
        public Iterable<Entity> getAll() {
            return List.of();
        }

        @Override
        public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AbortableIterationConsumer<U> consumer) {}

        @Override
        public void get(AABB box, Consumer<Entity> output) {}

        @Override
        public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AABB box, AbortableIterationConsumer<U> consumer) {}
    }

    private EntityOracle() {}
}
