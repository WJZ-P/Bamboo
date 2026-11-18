// 整条 `/execute` 命令的官方对拍。
// 在 JVM 内搭一个最小的 `ServerLevel`，用官方 `Commands.performPrefixedCommand`
// 跑固定脚本。每条命令都从干净场景开始；输出方块表、计分板、是否跑到 `run`
// 和反馈文本的哈希，Rust 侧在同样的场景里重放比较。
//
// Oracle for whole `/execute` commands. A minimal in-JVM `ServerLevel` and
// official `Commands.performPrefixedCommand`; each case starts from a clean
// scene. Emits blocks, score, whether `run` happened, and a feedback hash.

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
import java.util.List;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.commands.CommandSource;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.util.AbortableIterationConsumer;
import net.minecraft.util.RandomSource;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.ServerScoreboard;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.Difficulty;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.animal.cow.Cow;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.phys.Vec2;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.scores.Objective;
import net.minecraft.world.scores.ScoreHolder;
import net.minecraft.world.scores.criteria.ObjectiveCriteria;
import sun.misc.Unsafe;

public final class ExecuteOracle {
    private static final Unsafe UNSAFE = loadUnsafe();
    private static final BlockPos TEST_POS = new BlockPos(4, -60, 6);
    private static final String[] SCENARIOS = {
        "execute positioned 4 -60 6 run setblock 4 -60 6 stone",
        "execute if block 4 -60 6 stone run say yes",
        "execute unless block 4 -60 6 stone run setblock 4 -60 6 stone",
        "execute as @s at @s run setblock 4 -60 6 stone",
        "execute store success score #total result run setblock 4 -60 6 stone",
    };

    private ExecuteOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        if (args.length != 0) {
            throw new IllegalArgumentException("unexpected arguments");
        }
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(
                        new OutputStreamWriter(stdout, StandardCharsets.UTF_8), 1 << 20)) {
            Official official = Official.load(resources);
            out.write("{\"type\":\"header\",\"version\":\"");
            out.write(SharedConstants.getCurrentVersion().id());
            out.write("\"}\n");
            for (String line : SCENARIOS) {
                emit(out, official, line);
            }
            out.write("{\"type\":\"end\"}\n");
            out.flush();
        }
    }

    private static void emit(Writer out, Official official, String line) throws Exception {
        Scene scene = new Scene(official);
        List<String> feedback = new ArrayList<>();
        CommandSourceStack source = scene.source.withSource(new Recording(feedback));
        int ran = 1;
        try {
            official.commands.performCommand(
                    official.commands.getDispatcher().parse(line, source), line);
        } catch (Throwable error) {
            ran = 0;
        }
        if (feedback.isEmpty()) {
            ran = 0;
        }
        var info = scene.board.getPlayerScoreInfo(ScoreHolder.forNameOnly("#total"),
                scene.board.getObjective("result"));
        int score = info == null ? 0 : info.value();
        out.write("{\"type\":\"case\",\"input\":" + quote(line)
                + ",\"stone_4_6\":" + stone(scene, TEST_POS)
                + ",\"stone_cow_a\":" + stone(scene, new BlockPos(2, -60, 2))
                + ",\"stone_cow_b\":" + stone(scene, new BlockPos(6, -60, 6))
                + ",\"score\":" + score
                + ",\"ran\":" + ran
                + ",\"feedback\":" + hash(feedback)
                + "}\n");
    }

    private static int stone(Scene scene, BlockPos pos) {
        BlockState state = scene.blocks.get(pos.asLong());
        return state != null && state.is(Blocks.STONE) ? 1 : 0;
    }

    private static int hash(List<String> lines) {
        // 和 Rust 侧 `feedback_oracle_hash` 一样只比「有没有成功反馈」：
        // 官方是翻译后的英文，Bamboo 是翻译键，字面量并不相同。
        String text = lines.isEmpty() ? "conditions.not_met" : "success";
        int hash = 0x811c9dc5;
        for (byte b : text.getBytes(StandardCharsets.UTF_8)) {
            hash ^= b & 0xff;
            hash *= 0x01000193;
        }
        return hash;
    }

    /** 一个玩家、两头牛、一个空的方块表和一个计分板目标。 */
    private static final class Scene {
        final Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        final List<Entity> tracked = new ArrayList<>();
        final ServerScoreboard board;
        final CommandSourceStack source;
        final ServerLevel level;

        Scene(Official official) throws Exception {
            level = allocate(FakeLevel.class);
            FakeLevel fake = (FakeLevel) level;
            fake.blocks = blocks;
            fake.tracked = tracked;
            fake.nextId = 1;
            set(Level.class, level, "dimensionTypeRegistration", official.overworld);
            set(Level.class, level, "dimension", Level.OVERWORLD);
            set(Level.class, level, "registryAccess", official.registries);
            set(Level.class, level, "palettedContainerFactory",
                    PalettedContainerFactory.create(official.registries));
            set(Level.class, level, "thread", Thread.currentThread());
            set(Level.class, level, "random", net.minecraft.util.RandomSource.create(1L));
            set(Level.class, level, "soundSeedGenerator", net.minecraft.util.RandomSource.create(1L));
            set(Level.class, level, "neighborUpdater",
                    new net.minecraft.world.level.redstone.CollectingNeighborUpdater(level, 1_000_000));
            set(ServerLevel.class, level, "server", official.server);
            set(ServerLevel.class, level, "players", new ArrayList<ServerPlayer>());
            board = new ServerScoreboard(official.server);
            official.server.sharedScoreboard = board;
            fake.board = board;
            board.addObjective("result", ObjectiveCriteria.DUMMY, Component.literal("result"),
                    ObjectiveCriteria.RenderType.INTEGER, false, null);

            ServerPlayer player = allocate(ServerPlayer.class);
            set(Entity.class, player, "id", 1);
            set(Entity.class, player, "level", level);
            set(Entity.class, player, "position", new Vec3(8.5, -60.0, 8.5));
            player.setUUID(java.util.UUID.fromString("00000000-0000-0000-0000-000000000001"));
            level.players().add(player);
            tracked.add(player);
            for (int i = 0; i < 2; i++) {
                Cow cow = allocate(Cow.class);
                set(Entity.class, cow, "id", (1 << 24) + i);
                set(Entity.class, cow, "level", level);
                set(Entity.class, cow, "position",
                        i == 0 ? new Vec3(2.5, -60.0, 2.5) : new Vec3(6.5, -60.0, 6.5));
                tracked.add(cow);
            }
            source = new CommandSourceStack(
                    CommandSource.NULL,
                    new Vec3(8.5, -60.0, 8.5),
                    Vec2.ZERO,
                    level,
                    LevelBasedPermissionSet.GAMEMASTER,
                    official.server,
                    player);
        }
    }

    private static final class Recording implements CommandSource {
        private final List<String> lines;

        Recording(List<String> lines) {
            this.lines = lines;
        }

        @Override
        public void sendSystemMessage(Component message) {
            lines.add(message.getString());
        }

        @Override
        public boolean acceptsSuccess() {
            return true;
        }

        @Override
        public boolean acceptsFailure() {
            return true;
        }

        @Override
        public boolean shouldInformAdmins() {
            return false;
        }
    }

    private static final class Official {
        final net.minecraft.core.RegistryAccess registries;
        final Holder<DimensionType> overworld;
        final FakeServer server;
        final Commands commands;
        final GameRules rules = new GameRules(FeatureFlags.DEFAULT_FLAGS);

        private Official(ReloadableServerResources managers) {
            this.registries = (net.minecraft.core.RegistryAccess) managers.fullRegistries().lookup();
            this.overworld = registries.lookupOrThrow(Registries.DIMENSION_TYPE)
                    .getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            this.server = allocate(FakeServer.class);
            set(FakeServer.class, server, "managers", managers);
            set(FakeServer.class, server, "rules", rules);
            this.commands = managers.getCommands();
        }

        static Official load(CloseableResourceManager resources) throws Exception {
            LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
            HolderLookup.Provider context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(
                    (net.minecraft.core.RegistryAccess.Frozen) context, staticTags);
            net.minecraft.core.RegistryAccess.Frozen world = RegistryDataLoader
                    .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                    .join();
            LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
            ReloadableServerResources managers = ReloadableServerResources.loadResources(
                            resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                            Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                            ForkJoinPool.commonPool(), Runnable::run)
                    .get();
            managers.updateComponentsAndStaticRegistryTags();
            managers.getRecipeManager().finalizeRecipeLoading(FeatureFlags.DEFAULT_FLAGS);
            return new Official(managers);
        }
    }

    private static final class FakeServer extends DedicatedServer {
        ReloadableServerResources managers;
        GameRules rules;
        ServerScoreboard sharedScoreboard;
        net.minecraft.world.level.storage.WorldData worldData;

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        @Override
        public GameRules getGameRules() {
            return rules;
        }

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return managers.fullRegistries();
        }

        @Override
        public ServerScoreboard getScoreboard() {
            return sharedScoreboard;
        }

        @Override
        public Iterable<ServerLevel> getAllLevels() {
            return List.of();
        }

        @Override
        public net.minecraft.core.RegistryAccess.Frozen registryAccess() {
            return (net.minecraft.core.RegistryAccess.Frozen) managers.fullRegistries().lookup();
        }

        @Override
        public net.minecraft.world.level.storage.WorldData getWorldData() {
            return null;
        }

        @Override
        public void sendSystemMessage(Component message) {
        }
    }

    private static final class FakeLevel extends ServerLevel {
        Long2ObjectOpenHashMap<BlockState> blocks;
        List<Entity> tracked;
        ServerScoreboard board;
        final it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<
                net.minecraft.world.level.chunk.ChunkAccess> chunkCache =
                new it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<>();
        int nextId = 1;
        final LevelData fixedLevelData = new FixedLevelData();

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return blocks.getOrDefault(pos.asLong(), Blocks.AIR.defaultBlockState());
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int recursion) {
            blocks.put(pos.asLong(), state);
            return true;
        }

        @Override
        protected LevelEntityGetter<Entity> getEntities() {
            return new Tracked(tracked);
        }

        @Override
        public <T extends Entity> void getEntities(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, T> test,
                java.util.function.Predicate<? super T> filter,
                java.util.List<? super T> out, int limit) {
            for (Entity entity : tracked) {
                T cast = test.tryCast(entity);
                if (cast != null && filter.test(cast) && out.size() < limit) {
                    out.add(cast);
                }
            }
        }

        @Override
        public boolean hasChunk(int x, int z) {
            return true;
        }

        @Override
        public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public net.minecraft.world.level.chunk.LevelChunk getChunk(int x, int z) {
            return (net.minecraft.world.level.chunk.LevelChunk) getChunk(x, z,
                    net.minecraft.world.level.chunk.status.ChunkStatus.FULL, false);
        }

        @Override
        public net.minecraft.world.level.chunk.ChunkAccess getChunk(int x, int z,
                net.minecraft.world.level.chunk.status.ChunkStatus status, boolean load) {
            long key = net.minecraft.world.level.ChunkPos.pack(x, z);
            net.minecraft.world.level.chunk.ChunkAccess cached = chunkCache.get(key);
            if (cached != null) {
                return cached;
            }
            net.minecraft.world.level.chunk.LevelChunk chunk =
                    new net.minecraft.world.level.chunk.LevelChunk(
                            this, new net.minecraft.world.level.ChunkPos(x, z));
            chunkCache.put(key, chunk);
            return chunk;
        }

        @Override
        public LevelData getLevelData() {
            return fixedLevelData;
        }

        @Override
        public ServerScoreboard getScoreboard() {
            return board;
        }
    }

    private static final class Tracked implements LevelEntityGetter<Entity> {
        private final List<Entity> entities;

        Tracked(List<Entity> entities) {
            this.entities = entities;
        }

        @Override
        public Entity get(int id) {
            for (Entity entity : entities) {
                if (entity.getId() == id) {
                    return entity;
                }
            }
            return null;
        }

        @Override
        public Entity get(java.util.UUID id) {
            for (Entity entity : entities) {
                if (entity.getUUID().equals(id)) {
                    return entity;
                }
            }
            return null;
        }

        @Override
        public <U extends Entity> void get(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, U> test,
                AbortableIterationConsumer<U> consumer) {
            for (Entity entity : entities) {
                U cast = test.tryCast(entity);
                if (cast != null && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }

        @Override
        public void get(net.minecraft.world.phys.AABB box, java.util.function.Consumer<Entity> output) {
            for (Entity entity : entities) {
                if (entity.getBoundingBox().intersects(box)) {
                    output.accept(entity);
                }
            }
        }

        @Override
        public <U extends Entity> void get(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, U> test,
                net.minecraft.world.phys.AABB box,
                AbortableIterationConsumer<U> consumer) {
            for (Entity entity : entities) {
                U cast = test.tryCast(entity);
                if (cast != null && entity.getBoundingBox().intersects(box)
                        && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }

        @Override
        public Iterable<Entity> getAll() {
            return entities;
        }
    }

    private static final class FixedLevelData implements LevelData {
        @Override
        public LevelData.RespawnData getRespawnData() {
            return LevelData.RespawnData.of(Level.OVERWORLD, new BlockPos(8, 0, 8), 0.0F, 0.0F);
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

    private static void set(Class<?> owner, Object target, String name, Object value) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            field.set(target, value);
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

    private static Unsafe loadUnsafe() {
        try {
            Field field = Unsafe.class.getDeclaredField("theUnsafe");
            field.setAccessible(true);
            return (Unsafe) field.get(null);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static String quote(String text) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                default -> out.append(c);
            }
        }
        return out.append('"').toString();
    }
}
