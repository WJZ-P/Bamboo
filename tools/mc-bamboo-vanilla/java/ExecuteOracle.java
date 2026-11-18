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
    /** 32 条整命令场景；`store bossbar`、`in`、`if score` 和 NBT 目标需要更完整的
     *  WorldData / 谓词管理器 / NBT 数据源，见 README 的已知差异。 */
    private static final String[] SCENARIOS = {
        // 基础 run / 条件反馈
        "execute positioned 4 -60 6 run setblock 4 -60 6 stone",
        "execute if block 4 -60 6 stone run setblock 4 -60 6 stone",
        "execute unless block 4 -60 6 stone run setblock 4 -60 6 stone",
        "execute if block 2 -60 2 stone run setblock 4 -60 6 stone",
        "execute unless block 2 -60 2 stone run setblock 4 -60 6 stone",
        "execute if entity @e[type=cow] run setblock 4 -60 6 stone",
        "execute unless entity @e[type=cow] run setblock 4 -60 6 stone",
        "execute unless entity @e[type=slime] run setblock 4 -60 6 stone",
        // as / at 分叉与顺序
        "execute as @e[type=cow] at @s run setblock ~ ~ ~ stone",
        "execute at @e[type=cow] run setblock ~ ~ ~ stone",
        "execute as @e[type=cow,sort=nearest,limit=2] at @s run setblock ~ ~ ~ stone",
        "execute as @e[type=cow,sort=nearest,limit=1] at @s run setblock 4 -60 6 stone",
        "execute as @e[type=cow,sort=furthest,limit=1] at @s run setblock 4 -60 6 stone",
        "execute as @s at @s run setblock 4 -60 6 stone",
        // positioned / rotated / facing / align / anchored / in
        "execute positioned 2 -60 2 run setblock ~ ~ ~ stone",
        "execute positioned as @e[type=cow,limit=1,sort=nearest] run setblock 4 -60 6 stone",
        "execute rotated 90 0 run setblock 4 -60 6 stone",
        "execute rotated as @s run setblock 4 -60 6 stone",
        "execute facing 4 -60 6 run setblock 4 -60 6 stone",
        "execute facing entity @e[type=cow,limit=1,sort=nearest] eyes run setblock 4 -60 6 stone",
        "execute facing entity @e[type=cow,limit=1,sort=nearest] feet run setblock 4 -60 6 stone",
        "execute align xyz run setblock 4 -60 6 stone",
        "execute anchored eyes run setblock 4 -60 6 stone",
        "execute anchored feet run setblock 4 -60 6 stone",
        // blocks all / masked
        "execute if blocks 2 -60 2 2 -60 2 6 -60 6 all run setblock 4 -60 6 stone",
        "execute unless blocks 2 -60 2 2 -60 2 6 -60 6 masked run setblock 4 -60 6 stone",
        // score 条件
        // store 写回次数与值
        "execute store result score #total result run setblock 4 -60 6 stone",
        "execute store success score #total result run setblock 4 -60 6 stone",
        "execute as @e[type=cow] at @s store success score #total result run setblock ~ ~ ~ stone",
        "execute as @e[type=cow] at @s store result score #total result run setblock ~ ~ ~ stone",
        // 条件失败时的返回值
        "execute if score #total result matches 0 run setblock 4 -60 6 stone",
        "execute unless score #total result matches 0 run setblock 4 -60 6 stone",
        "execute if score #total result = #total result run setblock 4 -60 6 stone",
        "execute if block 4 -60 6 stone store success score #total result run setblock 4 -60 6 stone",
        "execute unless block 4 -60 6 stone store result score #total result run setblock 4 -60 6 stone",
        // /data 整条命令与 /execute 的 NBT 数据源
        "data merge storage minecraft:test {Value:7}",
        "data get storage minecraft:test Value",
        "data remove storage minecraft:test Value",
        "data modify storage minecraft:test Other set value 3",
        "execute if data storage minecraft:test Value run setblock 4 -60 6 stone",
        "execute unless data storage minecraft:test Missing run setblock 4 -60 6 stone",
        "execute store result storage minecraft:test Value int 1 run setblock 4 -60 6 stone",
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
        int ran = 0;
        try {
            official.commands.performCommand(
                    official.commands.getDispatcher().parse(line, source), line);
        } catch (Throwable error) {
            ran = 0;
        }
        if (feedback.isEmpty()) {
            ran = 0;
        }
        ran = feedback.stream().anyMatch(text -> text.contains("commands.setblock.success")) ? 1 : 0;
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

    /** 递归展开 `Component`：翻译键 + 参数，字面文本，子组件；不查语言表。 */
    private static String decompose(Component component) {
        StringBuilder out = new StringBuilder();
        decomposeInto(component, out);
        return out.toString();
    }

    private static void decomposeInto(Component component, StringBuilder out) {
        var contents = component.getContents();
        if (contents instanceof net.minecraft.network.chat.contents.TranslatableContents translated) {
            String key = translated.getKey();
            out.append("T[").append(key).append(']');
            for (Object arg : translated.getArgs()) {
                out.append('{');
                if (arg instanceof Component nested) {
                    decomposeInto(nested, out);
                } else if (arg instanceof net.minecraft.network.chat.contents.PlainTextContents plain) {
                    out.append("L[").append(plain.text()).append(']');
                } else if (key.equals("arguments.nbtpath.nothing_found")) {
                    out.append("L[").append(String.valueOf(arg)).append(']');
                } else {
                    out.append("S[").append(String.valueOf(arg)).append(']');
                }
                out.append('}');
            }
        } else if (contents instanceof net.minecraft.network.chat.contents.PlainTextContents literal) {
            out.append("L[").append(literal.text()).append(']');
        } else {
            out.append("C[").append(contents.getClass().getSimpleName()).append(']');
        }
        for (Component sibling : component.getSiblings()) {
            out.append('+');
            decomposeInto(sibling, out);
        }
    }

    private static int hash(List<String> lines) {
        // 输入是逐条反馈的「翻译键 + 参数 + 子组件」展开，Rust 侧按同样格式展开。
        int hash = 0x811c9dc5;
        for (String line : lines) {
            for (byte b : line.getBytes(StandardCharsets.UTF_8)) {
                hash ^= b & 0xff;
                hash *= 0x01000193;
            }
            hash ^= '\n';
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
            // 官方 `SetBlockCommand` 在非 strict 模式还会更新邻居；这里只保留方块写入，
            // 不跑邻居更新（假世界没有邻域方块表）。
            set(ServerLevel.class, level, "server", official.server);
            set(ServerLevel.class, level, "players", new ArrayList<ServerPlayer>());
            board = new ServerScoreboard(official.server);
            official.server.sharedScoreboard = board;
            official.server.currentLevel = level;
            var saved = UNSAFE.allocateInstance(net.minecraft.world.level.storage.SavedDataStorage.class);
            set(net.minecraft.world.level.storage.SavedDataStorage.class, saved, "cache",
                    new java.util.HashMap<>());
            official.server.commandStorage =
                    new net.minecraft.world.level.storage.CommandStorage(
                            (net.minecraft.world.level.storage.SavedDataStorage) saved);
            board.addObjective("result", ObjectiveCriteria.DUMMY, Component.literal("result"),
                    ObjectiveCriteria.RenderType.INTEGER, false, null);
            board.getOrCreatePlayerScore(ScoreHolder.forNameOnly("#total"),
                    board.getObjective("result")).set(0);
            fake.board = board;
            var chunkMap = allocate(net.minecraft.server.level.ChunkMap.class);
            set(net.minecraft.server.level.ChunkMap.class, chunkMap, "entityMap",
                    new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>());
            fake.chunkSource = allocate(net.minecraft.server.level.ServerChunkCache.class);
            set(net.minecraft.server.level.ServerChunkCache.class, fake.chunkSource, "chunkMap", chunkMap);
            set(ServerLevel.class, level, "chunkSource", fake.chunkSource);

            ServerPlayer player = allocate(ServerPlayer.class);
            set(Entity.class, player, "id", 1);
            set(Entity.class, player, "level", level);
            set(Entity.class, player, "position", new Vec3(8.5, -60.0, 8.5));
            player.setUUID(java.util.UUID.fromString("00000000-0000-0000-0000-000000000001"));
            level.players().add(player);
            tracked.add(player);
            for (int i = 0; i < 2; i++) {
                Cow cow = new Cow(EntityTypes.COW, level);
                cow.setId((1 << 24) + i);
                cow.setPos(i == 0 ? 2.5 : 6.5, -60.0, i == 0 ? 2.5 : 6.5);
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
            lines.add(decompose(message));
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
        ServerLevel currentLevel;
        net.minecraft.world.level.storage.CommandStorage commandStorage;
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
            return List.of(currentLevel);
        }

        @Override
        public net.minecraft.core.RegistryAccess.Frozen registryAccess() {
            return (net.minecraft.core.RegistryAccess.Frozen) managers.fullRegistries().lookup();
        }

        @Override
        public net.minecraft.world.level.storage.CommandStorage getCommandStorage() {
            return commandStorage;
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
        net.minecraft.server.level.ServerChunkCache chunkSource;
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
