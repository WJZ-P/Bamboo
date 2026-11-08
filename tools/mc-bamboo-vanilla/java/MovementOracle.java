import com.mojang.authlib.GameProfile;
import it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap;
import it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap;
import net.minecraft.world.level.chunk.ChunkAccess;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Random;
import java.util.UUID;
import java.util.concurrent.ForkJoinPool;
import java.util.function.Consumer;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket;
import net.minecraft.network.protocol.game.ServerboundClientTickEndPacket;
import net.minecraft.network.protocol.game.ServerboundMovePlayerPacket;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.PlayerAdvancements;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ChunkMap;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.level.ServerPlayerGameMode;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.dedicated.DedicatedPlayerList;
import net.minecraft.server.players.PlayerList;
import net.minecraft.stats.ServerStatsCounter;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.AbortableIterationConsumer;
import net.minecraft.util.RandomSource;
import net.minecraft.world.Difficulty;
import net.minecraft.world.TickRateManager;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.boss.enderdragon.EnderDragonPart;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.server.ServerScoreboard;
import net.minecraft.world.ticks.LevelTicks;
import net.minecraft.network.PacketProcessor;
import sun.misc.Unsafe;

/**
 * 官方 `ServerGamePacketListenerImpl.handleMovePlayer`。每个样本在假世界里跑，打一行 JSON。
 *
 * 样本之间不跑 `LivingEntity.travel`。速度检查用的动量只来自 `Entity.move` 和跳跃。
 *
 * Official `ServerGamePacketListenerImpl.handleMovePlayer`. Each sample runs in a fake world and
 * prints one JSON line.
 *
 * Samples do not run `LivingEntity.travel`. The speed check's momentum comes only from
 * `Entity.move` and jumping.
 */
public final class MovementOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final Unsafe UNSAFE = loadUnsafe();
    private static ServerStatsCounter stats;
    private static PlayerAdvancements advancements;
    private static Session session;

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            int cases = 40;
            long seed = 1L;
            for (int i = 0; i < args.length; i++) {
                if ("--cases".equals(args[i]) && i + 1 < args.length) {
                    cases = Integer.parseInt(args[++i]);
                } else if ("--seed".equals(args[i]) && i + 1 < args.length) {
                    seed = Long.parseLong(args[++i]);
                } else {
                    throw new IllegalArgumentException(args[i]);
                }
            }
            run(log, cases, seed);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(PrintStream log, int cases, long seed) throws Exception {
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
            stats = new ServerStatsCounter(official.server, Path.of("move-oracle-stats.json"));
            advancements = allocate(PlayerAdvancements.class);
            official.server.setPlayerList(allocate(StubList.class));
            put(MinecraftServer.class, official.server, "packetProcessor", new PacketProcessor(Thread.currentThread()));
            out.write("{\"type\":\"header\",\"version\":\"");
            out.write(SharedConstants.getCurrentVersion().id());
            out.write("\",\"protocol\":");
            out.write(Integer.toString(SharedConstants.getCurrentVersion().protocolVersion()));
            out.write("}\n");
            int count = 0;
            count += fixed(official, out);
            count += random(official, out, cases, seed);
            out.write("{\"type\":\"end\",\"cases\":" + count + "}\n");
            out.flush();
        }
    }

    private static int fixed(Official official, Writer out) throws Exception {
        Vec3 air = new Vec3(0.5, 80.0, 0.5);
        List<BlockState> floor = List.of(Blocks.STONE.defaultBlockState());
        play(official, out, "walk", GameType.SURVIVAL, false, false, false, true, air, true, List.of(), List.of(move(air.add(0.2, 0.0, 0.0), true)));
        play(official, out, "ten", GameType.SURVIVAL, false, false, false, true, air, true, List.of(), List.of(move(air.add(10.0, 0.0, 0.0), true)));
        play(official, out, "hundred", GameType.SURVIVAL, false, false, false, true, air, true, List.of(), List.of(move(air.add(100.0, 0.0, 0.0), true)));
        BlockPos wall = BlockPos.containing(air.add(1.0, 0.0, 0.0));
        play(
                official,
                out,
                "into_stone",
                GameType.SURVIVAL,
                false,
                false,
                false,
                true,
                air,
                true,
                List.of(new Placed(wall, Blocks.STONE.defaultBlockState())),
                List.of(move(new Vec3(wall.getX() + 0.2, air.y, air.z), true)));
        play(
                official,
                out,
                "into_stone_creative",
                GameType.CREATIVE,
                false,
                false,
                false,
                true,
                air,
                true,
                List.of(new Placed(wall, Blocks.STONE.defaultBlockState())),
                List.of(move(new Vec3(wall.getX() + 0.2, air.y, air.z), true)));
        play(
                official,
                out,
                "spectator_through",
                GameType.SPECTATOR,
                false,
                false,
                false,
                true,
                air,
                true,
                List.of(new Placed(wall, Blocks.STONE.defaultBlockState())),
                List.of(move(new Vec3(wall.getX() + 0.2, air.y, air.z), true)));
        play(
                official,
                out,
                "double_position",
                GameType.SURVIVAL,
                false,
                false,
                false,
                true,
                air,
                true,
                List.of(),
                List.of(move(air.add(0.2, 0.0, 0.0), true), move(air.add(0.4, 0.0, 0.0), true)));
        play(
                official,
                out,
                "tick_end",
                GameType.SURVIVAL,
                false,
                false,
                false,
                true,
                air,
                true,
                List.of(),
                List.of(move(air.add(0.2, 0.0, 0.0), true), tickEnd(), serverTick(), move(air.add(0.4, 0.0, 0.0), true)));
        play(official, out, "not_loaded", GameType.SURVIVAL, false, false, false, false, air, true, List.of(), List.of(move(air.add(0.2, 0.0, 0.0), true)));
        List<Step> flying = new ArrayList<>();
        flying.add(move(air, true));
        for (int i = 0; i < 81; i++) {
            flying.add(serverTick());
        }
        play(official, out, "fly_survival", GameType.SURVIVAL, false, false, false, true, air, true, List.of(), flying);
        play(official, out, "fly_creative", GameType.CREATIVE, false, false, false, true, air, true, List.of(), flying);
        BlockPos slab = new BlockPos(1, 80, 0);
        play(
                official,
                out,
                "step_slab",
                GameType.SURVIVAL,
                false,
                false,
                false,
                true,
                new Vec3(0.6, 80.0, 0.5),
                true,
                List.of(new Placed(slab, Blocks.STONE_SLAB.defaultBlockState())),
                List.of(move(new Vec3(1.1, 80.5, 0.5), true)));
        play(
                official,
                out,
                "fall",
                GameType.SURVIVAL,
                false,
                false,
                false,
                true,
                air,
                false,
                List.of(floor.get(0) == null ? Blocks.AIR.defaultBlockState() : Blocks.AIR.defaultBlockState()).isEmpty()
                        ? List.of()
                        : List.of(),
                List.of(move(air.add(0.0, -2.0, 0.0), false)));
        return 13;
    }

    private static int random(Official official, Writer out, int cases, long seed) throws Exception {
        Random random = new Random(seed);
        GameType[] modes = {GameType.SURVIVAL, GameType.CREATIVE, GameType.SPECTATOR, GameType.ADVENTURE};
        BlockState[] choices = {
            Blocks.AIR.defaultBlockState(),
            Blocks.STONE.defaultBlockState(),
            Blocks.STONE_SLAB.defaultBlockState(),
            Blocks.OAK_FENCE.defaultBlockState()
        };
        for (int index = 0; index < cases; index++) {
            GameType mode = modes[random.nextInt(modes.length)];
            double x = 0.5 + (random.nextDouble() - 0.5) * 4.0;
            double y = 70.0 + random.nextInt(20);
            double z = 0.5 + (random.nextDouble() - 0.5) * 4.0;
            Vec3 start = new Vec3(x, y, z);
            List<Placed> blocks = new ArrayList<>();
            int count = random.nextInt(6);
            for (int block = 0; block < count; block++) {
                BlockPos pos = BlockPos.containing(start).offset(random.nextInt(5) - 2, random.nextInt(4) - 1, random.nextInt(5) - 2);
                blocks.add(new Placed(pos, choices[random.nextInt(choices.length)]));
            }
            List<Step> packets = new ArrayList<>();
            int moves = 1 + random.nextInt(3);
            Vec3 cursor = start;
            for (int move = 0; move < moves; move++) {
                double scale = random.nextInt(12) == 0 ? 40.0 : 2.0;
                cursor = cursor.add((random.nextDouble() - 0.5) * scale, (random.nextDouble() - 0.5) * scale, (random.nextDouble() - 0.5) * scale);
                packets.add(move(cursor, random.nextBoolean()));
                if (random.nextBoolean()) {
                    packets.add(tickEnd());
                }
                if (random.nextInt(4) == 0) {
                    packets.add(serverTick());
                }
            }
            play(
                    official,
                    out,
                    "random-" + seed + "-" + index,
                    mode,
                    random.nextBoolean(),
                    random.nextBoolean(),
                    false,
                    true,
                    start,
                    random.nextBoolean(),
                    blocks,
                    packets);
        }
        return cases;
    }

    private static void play(
            Official official,
            Writer out,
            String name,
            GameType mode,
            boolean shift,
            boolean sprint,
            boolean flying,
            boolean loaded,
            Vec3 start,
            boolean onGround,
            List<Placed> blocks,
            List<Step> packets)
            throws Exception {
        session = new Session();
        FakeLevel level = FakeLevel.create(official);
        for (Placed placed : blocks) {
            level.blocks.put(placed.pos.asLong(), placed.state);
        }
        GameProfile profile = new GameProfile(new UUID(1L, 2L), "Oracle");
        ServerPlayer player = new ServerPlayer(official.server, level, profile, ClientInformation.createDefault());
        setMode(player, mode);
        player.setPos(start);
        player.setYRot(0.0F);
        player.setXRot(0.0F);
        player.setOnGround(onGround);
        player.setShiftKeyDown(shift);
        player.setSprinting(sprint);
        player.getAbilities().flying = flying && (mode == GameType.CREATIVE || mode == GameType.SPECTATOR);
        updatePose(player);
        player.noPhysics = player.isSpectator();
        RecordingListener listener = allocate(RecordingListener.class);
        put(ServerGamePacketListenerImpl.class, listener, "player", player);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class, listener, "server", official.server);
        player.connection = listener;
        setInt(listener, "tickCount", 1);
        setInt(listener, "clientLoadedTimeoutTimer", loaded ? 0 : 60);
        listener.resetPosition();
        for (Step step : packets) {
            if (session.kicked) {
                break;
            }
            switch (step.kind) {
                case "move" -> listener.handleMovePlayer(step.packet);
                case "tickEnd" -> listener.handleClientTickEnd(ServerboundClientTickEndPacket.INSTANCE);
                case "serverTick" -> serverTick(listener, player);
                default -> throw new IllegalStateException(step.kind);
            }
        }
        String outcome = session.kicked ? "kick" : session.teleports > 0 ? "rubber" : "accept";
        StringBuilder line = new StringBuilder(512);
        line.append("{\"type\":\"case\",\"name\":").append(quote(name));
        line.append(",\"outcome\":").append(quote(outcome));
        line.append(",\"kicked\":").append(session.kicked);
        line.append(",\"teleports\":").append(session.teleports);
        line.append(",\"xBits\":").append(Double.doubleToRawLongBits(player.getX()));
        line.append(",\"yBits\":").append(Double.doubleToRawLongBits(player.getY()));
        line.append(",\"zBits\":").append(Double.doubleToRawLongBits(player.getZ()));
        line.append(",\"fallBits\":").append(Double.doubleToRawLongBits(player.fallDistance));
        line.append(",\"mode\":").append(quote(mode.getName()));
        line.append(",\"shift\":").append(shift);
        line.append(",\"sprint\":").append(sprint);
        line.append(",\"flying\":").append(player.getAbilities().flying);
        line.append(",\"loaded\":").append(loaded);
        line.append(",\"onGround\":").append(onGround);
        line.append(",\"sx\":").append(Double.doubleToRawLongBits(start.x));
        line.append(",\"sy\":").append(Double.doubleToRawLongBits(start.y));
        line.append(",\"sz\":").append(Double.doubleToRawLongBits(start.z));
        line.append(",\"blocks\":[");
        for (int i = 0; i < blocks.size(); i++) {
            if (i > 0) {
                line.append(',');
            }
            Placed placed = blocks.get(i);
            line.append('[')
                    .append(placed.pos.getX())
                    .append(',')
                    .append(placed.pos.getY())
                    .append(',')
                    .append(placed.pos.getZ())
                    .append(',')
                    .append(Block.getId(placed.state))
                    .append(']');
        }
        line.append("],\"packets\":[");
        for (int i = 0; i < packets.size(); i++) {
            if (i > 0) {
                line.append(',');
            }
            line.append(packets.get(i).json());
        }
        line.append("]}\n");
        out.write(line.toString());
    }

    private static void updatePose(ServerPlayer player) throws Exception {
        Method method = net.minecraft.world.entity.player.Player.class.getDeclaredMethod("updatePlayerPose");
        method.setAccessible(true);
        method.invoke(player);
    }

    private static void serverTick(RecordingListener listener, ServerPlayer player) throws Exception {
        updatePose(player);
        listener.resetPosition();
        player.noPhysics = player.isSpectator();
        int tick = getInt(listener, "tickCount") + 1;
        setInt(listener, "tickCount", tick);
        setInt(listener, "knownMovePacketCount", getInt(listener, "receivedMovePacketCount"));
        boolean floating = getBoolean(listener, "clientIsFloating");
        if (floating && !player.isSleeping() && !player.isPassenger() && !player.isDeadOrDying()) {
            int above = getInt(listener, "aboveGroundTickCount") + 1;
            setInt(listener, "aboveGroundTickCount", above);
            Method maximum = ServerGamePacketListenerImpl.class.getDeclaredMethod("getMaximumFlyingTicks", Entity.class);
            maximum.setAccessible(true);
            int limit = (Integer) maximum.invoke(listener, player);
            if (above > limit) {
                listener.disconnect(net.minecraft.network.chat.Component.translatable("multiplayer.disconnect.flying"));
            }
        } else {
            setBoolean(listener, "clientIsFloating", false);
            setInt(listener, "aboveGroundTickCount", 0);
        }
    }

    private static void setMode(ServerPlayer player, GameType mode) throws Exception {
        Method method = ServerPlayerGameMode.class.getDeclaredMethod("setGameModeForPlayer", GameType.class, GameType.class);
        method.setAccessible(true);
        method.invoke(player.gameMode, mode, null);
    }

    private static Step move(Vec3 target, boolean onGround) {
        return new Step("move", new ServerboundMovePlayerPacket.Pos(target, onGround, false), target, onGround);
    }

    private static Step tickEnd() {
        return new Step("tickEnd", null, null, false);
    }

    private static Step serverTick() {
        return new Step("serverTick", null, null, false);
    }

    private static String quote(String text) {
        return "\"" + text.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private record Placed(BlockPos pos, BlockState state) {}

    private record Step(String kind, ServerboundMovePlayerPacket packet, Vec3 target, boolean onGround) {
        String json() {
            if (!"move".equals(kind)) {
                return "{\"kind\":" + quote(kind) + "}";
            }
            return "{\"kind\":\"move\",\"x\":"
                    + Double.doubleToRawLongBits(target.x)
                    + ",\"y\":"
                    + Double.doubleToRawLongBits(target.y)
                    + ",\"z\":"
                    + Double.doubleToRawLongBits(target.z)
                    + ",\"onGround\":"
                    + onGround
                    + ",\"horizontal\":false}";
        }
    }

    private static final class Session {
        int teleports;
        boolean kicked;
    }

    private static final class RecordingListener extends ServerGamePacketListenerImpl {
        private RecordingListener() {
            super(null, null, null, null);
        }

        @Override
        public void send(Packet<?> packet) {
            if (packet instanceof ClientboundPlayerPositionPacket) {
                session.teleports++;
            }
        }

        @Override
        public void send(Packet<?> packet, io.netty.channel.ChannelFutureListener listener) {
            send(packet);
        }

        @Override
        public void disconnect(net.minecraft.network.chat.Component reason) {
            session.kicked = true;
        }

        @Override
        protected boolean isSingleplayerOwner() {
            return false;
        }
    }

    private static final class StubList extends DedicatedPlayerList {
        private StubList() {
            super(null, null, null);
        }

        @Override
        public ServerStatsCounter getPlayerStats(Player player) {
            return stats;
        }

        @Override
        public PlayerAdvancements getPlayerAdvancements(ServerPlayer player) {
            return advancements;
        }
    }

    private static final class StillChunks extends ServerChunkCache {
        private StillChunks() {
            super(null, null, null, null, null, null, 0, 0, false, null);
        }

        @Override
        public void move(ServerPlayer player) {}
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

    private static void put(Class<?> owner, Object target, String name, Object value) {
        try {
            Field field = owner.getDeclaredField(name);
            UNSAFE.putObject(target, UNSAFE.objectFieldOffset(field), value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static Field find(Object target, String name) {
        Class<?> type = target.getClass();
        while (type != null) {
            try {
                Field field = type.getDeclaredField(name);
                field.setAccessible(true);
                return field;
            } catch (NoSuchFieldException ignored) {
                type = type.getSuperclass();
            }
        }
        throw new IllegalStateException(name);
    }

    private static int getInt(Object target, String name) {
        try {
            return find(target, name).getInt(target);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    private static void setInt(Object target, String name, int value) {
        try {
            find(target, name).setInt(target, value);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    private static boolean getBoolean(Object target, String name) {
        try {
            return find(target, name).getBoolean(target);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    private static void setBoolean(Object target, String name, boolean value) {
        try {
            find(target, name).setBoolean(target, value);
        } catch (IllegalAccessException error) {
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
        final net.minecraft.world.level.border.WorldBorder border = new net.minecraft.world.level.border.WorldBorder();
        final LevelData levelData = new FixedLevelData();

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
            RegistryAccess.Frozen world = RegistryDataLoader.load(
                            resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                    .join();
            var layers = initial.replaceFrom(RegistryLayer.WORLD, world);
            ReloadableServerResources managers = ReloadableServerResources.loadResources(
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
            return new Official(managers);
        }
    }

    private static final class FakeServer extends DedicatedServer {
        private ReloadableServerResources managers;
        private GameRules rules;

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        static FakeServer create(ReloadableServerResources managers, GameRules rules) {
            FakeServer server = allocate(FakeServer.class);
            server.managers = managers;
            server.rules = rules;
            return server;
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
        public boolean allowFlight() {
            return false;
        }

        @Override
        public GameType getDefaultGameType() {
            return GameType.SURVIVAL;
        }

        @Override
        public GameType getForcedGameType() {
            return null;
        }

        @Override
        public boolean forceGameMode() {
            return false;
        }

        @Override
        public net.minecraft.util.debug.ServerDebugSubscribers debugSubscribers() {
            return new net.minecraft.util.debug.ServerDebugSubscribers(this);
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
        private LevelTicks<Block> blockTicks;
        private LevelTicks<net.minecraft.world.level.material.Fluid> fluidTicks;
        private ServerChunkCache chunkSource;
        private Long2ObjectOpenHashMap<ChunkAccess> chunkCache;
        private ServerScoreboard scoreboard;
        private TickRateManager tickRate;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        static FakeLevel create(Official official) {
            FakeLevel level = allocate(FakeLevel.class);
            level.official = official;
            level.blocks = new Long2ObjectOpenHashMap<>();
            level.blockTicks = new LevelTicks<>(key -> true);
            level.fluidTicks = new LevelTicks<>(key -> true);
            level.chunkSource = allocate(StillChunks.class);
            ChunkMap map = allocate(ChunkMap.class);
            setField(ChunkMap.class, map, "entityMap", new Int2ObjectOpenHashMap<>());
            setField(ServerChunkCache.class, level.chunkSource, "chunkMap", map);
            setField(Level.class, level, "neighborUpdater", new net.minecraft.world.level.redstone.CollectingNeighborUpdater(level, 1_000_000));
            setField(Level.class, level, "random", RandomSource.create(1L));
            setField(Level.class, level, "soundSeedGenerator", RandomSource.createThreadSafe());
            setField(Level.class, level, "dimensionTypeRegistration", official.overworld);
            setField(Level.class, level, "dimension", Level.OVERWORLD);
            setField(Level.class, level, "registryAccess", official.registries);
            setField(Level.class, level, "palettedContainerFactory", PalettedContainerFactory.create(official.registries));
            setField(Level.class, level, "damageSources", official.damageSources);
            setField(Level.class, level, "thread", Thread.currentThread());
            setField(ServerLevel.class, level, "players", new ArrayList<ServerPlayer>());
            setField(ServerLevel.class, level, "server", official.server);
            setField(ServerLevel.class, level, "chunkSource", level.chunkSource);
            level.blocks = new Long2ObjectOpenHashMap<>();
            level.chunkCache = new Long2ObjectOpenHashMap<>();
            level.scoreboard = new ServerScoreboard(official.server);
            level.tickRate = new TickRateManager();
            return level;
        }

        private static void setField(Class<?> owner, Object target, String name, Object value) {
            put(owner, target, name, value);
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
        public net.minecraft.world.level.material.FluidState getFluidState(BlockPos pos) {
            return isInsideBuildHeight(pos)
                    ? getBlockState(pos).getFluidState()
                    : net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
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
            long key = ChunkPos.pack(chunkX, chunkZ);
            ChunkAccess cached = chunkCache.get(key);
            if (cached != null) {
                return cached;
            }
            net.minecraft.world.level.chunk.LevelChunk chunk =
                    new net.minecraft.world.level.chunk.LevelChunk(this, new ChunkPos(chunkX, chunkZ));
            blocks.forEach((packed, state) -> {
                BlockPos pos = BlockPos.of(packed);
                if (pos.getX() >> 4 == chunkX && pos.getZ() >> 4 == chunkZ) {
                    int sectionIndex = chunk.getSectionIndex(pos.getY());
                    chunk.getSection(sectionIndex)
                            .setBlockState(pos.getX() & 15, pos.getY() & 15, pos.getZ() & 15, state, false);
                }
            });
            chunkCache.put(key, chunk);
            return chunk;
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
        public LevelTicks<net.minecraft.world.level.material.Fluid> getFluidTicks() {
            return fluidTicks;
        }

        @Override
        public GameRules getGameRules() {
            return official.gameRules;
        }

        @Override
        public net.minecraft.world.level.border.WorldBorder getWorldBorder() {
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
        public ServerScoreboard getScoreboard() {
            return scoreboard;
        }

        @Override
        public TickRateManager tickRateManager() {
            return tickRate;
        }

        @Override
        public void gameEvent(Holder<GameEvent> gameEvent, Vec3 position, GameEvent.Context context) {}

        @Override
        public void sendBlockUpdated(BlockPos pos, BlockState old, BlockState current, int flags) {}

        @Override
        public void levelEvent(Entity source, int type, BlockPos pos, int data) {}

        @Override
        public void playSeededSound(
                Entity except,
                double x,
                double y,
                double z,
                Holder<net.minecraft.sounds.SoundEvent> sound,
                net.minecraft.sounds.SoundSource source,
                float volume,
                float pitch,
                long seed) {}

        @Override
        public void playSeededSound(
                Entity except,
                Entity sourceEntity,
                Holder<net.minecraft.sounds.SoundEvent> sound,
                net.minecraft.sounds.SoundSource source,
                float volume,
                float pitch,
                long seed) {}

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

    private MovementOracle() {}
}
