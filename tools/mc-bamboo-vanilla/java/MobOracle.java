import com.mojang.authlib.GameProfile;
import it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap;
import it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap;
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
import java.util.Arrays;
import java.util.Collection;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.ForkJoinPool;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import io.netty.buffer.Unpooled;
import java.util.Base64;
import net.minecraft.network.PacketProcessor;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientGamePacketListener;
import net.minecraft.network.protocol.game.GameProtocols;
import net.minecraft.server.level.ServerEntity;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.PlayerAdvancements;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.ServerScoreboard;
import net.minecraft.server.dedicated.DedicatedPlayerList;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ChunkMap;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.stats.ServerStatsCounter;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.AbortableIterationConsumer;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.Difficulty;
import net.minecraft.world.TickRateManager;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.entity.ai.goal.WrappedGoal;
import net.minecraft.world.entity.animal.cow.Cow;
import net.minecraft.world.entity.boss.enderdragon.EnderDragonPart;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.SlabBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.SlabType;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.pathfinder.Node;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.ticks.LevelTicks;
import sun.misc.Unsafe;

/**
 * 官方牛在假世界里逐刻的状态。每个场景一行 JSON：场景本身（方块、牛、玩家、伤害动作）加上
 * 每一刻牛的位置、速度、朝向、生命、路径、正在跑的目标和随机源状态。
 *
 * <p>不启动服务端、不读存档、不接受 EULA：世界是 {@code ServerLevel} 的一个子类，方块放在
 * 一张表里，区块按需建出来给寻路和流体扫描读。实体循环照 {@code ServerLevel.tick}：
 * 先 {@code checkDespawn}，再 {@code commonTick} 和 {@code tick}。
 *
 * <p>Official cows in a fake world, one JSON line per scene with every tick's state.
 */
public final class MobOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final int FIRST_ID = 1 << 24;
    private static final long START_TIME = 1000L;
    /** Bamboo 的 `ENTITY_RANDOM_SEED`：岛的实体种子源的初始种子。 */
    private static final long ENTITY_RANDOM_SEED = 0x0DDB1A5E5BAD5EEDL;
    private static final Unsafe UNSAFE = loadUnsafe();
    private static ServerStatsCounter stats;
    private static PlayerAdvancements advancements;

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            String only = null;
            for (int i = 0; i < args.length; i++) {
                if ("--only".equals(args[i]) && i + 1 < args.length) {
                    only = args[++i];
                } else {
                    throw new IllegalArgumentException(args[i]);
                }
            }
            run(log, only);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(PrintStream log, String only) throws Exception {
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
            Official official = Official.load(resources);
            stats = new ServerStatsCounter(official.server, Path.of("mob-oracle-stats.json"));
            advancements = allocate(PlayerAdvancements.class);
            put(PlayerAdvancements.class, advancements, "activeTriggers", new IdentityHashMap<>());
            official.server.setPlayerList(allocate(StubList.class));
            put(MinecraftServer.class, official.server, "packetProcessor", new PacketProcessor(Thread.currentThread()));
            out.write("{\"type\":\"header\",\"version\":\"");
            out.write(SharedConstants.getCurrentVersion().id());
            out.write("\",\"protocol\":");
            out.write(Integer.toString(SharedConstants.getCurrentVersion().protocolVersion()));
            out.write(",\"tables\":" + tableHashes());
            out.write("}\n");
            int cases = 0;
            for (Scene scene : Scenes.all()) {
                if (only != null && !scene.name.startsWith(only)) {
                    continue;
                }
                play(official, out, scene);
                cases++;
            }
            out.write("{\"type\":\"end\",\"cases\":" + cases + "}\n");
            out.flush();
        }
    }

    /** {@code Mth} 的三张查表的位模式哈希，Rust 侧逐位比较。 */
    private static String tableHashes() throws Exception {
        Field sinField = Mth.class.getDeclaredField("SIN");
        sinField.setAccessible(true);
        float[] table = (float[]) sinField.get(null);
        long sin = 0xcbf29ce484222325L;
        for (float value : table) {
            sin = (sin ^ (Float.floatToRawIntBits(value) & 0xffffffffL)) * 0x100000001b3L;
        }
        Field asinField = Mth.class.getDeclaredField("ASIN_TAB");
        Field cosField = Mth.class.getDeclaredField("COS_TAB");
        asinField.setAccessible(true);
        cosField.setAccessible(true);
        double[] asin = (double[]) asinField.get(null);
        double[] cos = (double[]) cosField.get(null);
        long atan = 0xcbf29ce484222325L;
        for (int i = 0; i < asin.length; i++) {
            atan = (atan ^ Double.doubleToRawLongBits(asin[i])) * 0x100000001b3L;
            atan = (atan ^ Double.doubleToRawLongBits(cos[i])) * 0x100000001b3L;
        }
        StringBuilder out = new StringBuilder();
        out.append('[').append(sin).append(',').append(atan).append("],\"asin\":[");
        for (int i = 0; i < asin.length; i++) {
            if (i > 0) out.append(',');
            out.append(Double.doubleToRawLongBits(asin[i]));
        }
        out.append("],\"cos\":[");
        for (int i = 0; i < cos.length; i++) {
            if (i > 0) out.append(',');
            out.append(Double.doubleToRawLongBits(cos[i]));
        }
        out.append(']');
        return out.toString();
    }

    // ---- 场景 ----

    /** 一个场景：方块、牛、玩家和中途的伤害。 */
    static final class Scene {
        final String name;
        final Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        final List<double[]> cows = new ArrayList<>();
        final List<Long> seeds = new ArrayList<>();
        final List<float[]> yaws = new ArrayList<>();
        final List<double[]> players = new ArrayList<>();
        final List<Item> playerItems = new ArrayList<>();
        /** 每项是（刻、牛的下标、伤害量、来源）。来源 0 是玩家 0 的攻击。 */
        final List<double[]> hurts = new ArrayList<>();
        /** `/summon`：（刻、x、y、z）和可选的 SNBT。 */
        final List<double[]> summons = new ArrayList<>();
        final List<String> summonNbt = new ArrayList<>();
        /** 刷怪蛋：（刻、生成格 x、y、z、是否 `shouldOffsetYMore`）。 */
        final List<int[]> eggs = new ArrayList<>();
        int ticks = 600;
        boolean baby;
        /** 逐刻比出站包序列；一个观察者一直在范围内。 */
        boolean packets;

        Scene(String name) {
            this.name = name;
        }

        void set(int x, int y, int z, BlockState state) {
            blocks.put(BlockPos.asLong(x, y, z), state);
        }

        void fill(int x0, int y0, int z0, int x1, int y1, int z1, BlockState state) {
            for (int x = x0; x <= x1; x++) {
                for (int y = y0; y <= y1; y++) {
                    for (int z = z0; z <= z1; z++) {
                        set(x, y, z, state);
                    }
                }
            }
        }

        Scene cow(double x, double y, double z, float yaw, long seed) {
            cows.add(new double[] {x, y, z});
            yaws.add(new float[] {yaw});
            seeds.add(seed);
            return this;
        }

        Scene player(double x, double y, double z, Item item) {
            players.add(new double[] {x, y, z});
            playerItems.add(item);
            return this;
        }

        Scene ticks(int ticks) {
            this.ticks = ticks;
            return this;
        }

        Scene summon(int tick, double x, double y, double z, String nbt) {
            summons.add(new double[] {tick, x, y, z});
            summonNbt.add(nbt);
            return this;
        }

        Scene egg(int tick, int x, int y, int z, boolean offsetMore) {
            eggs.add(new int[] {tick, x, y, z, offsetMore ? 1 : 0});
            return this;
        }

        /** 这个场景逐刻比出站包序列，一个观察者始终在范围内。 */
        Scene packets() {
            packets = true;
            return this;
        }
    }

    /** 每头牛的官方 `ServerEntity` 和一个记下包字节的假 Synchronizer。 */
    private static final class Sync {
        final ServerEntity entity;
        final Bytes bytes;

        Sync(FakeLevel level, Cow cow, RegistryAccess registries) {
            bytes = new Bytes(registries);
            entity = new ServerEntity(level, cow,
                    cow.getType().hasUpdateInterval()
                            ? net.minecraft.world.entity.UpdateInterval.periodic(cow.getType().updateInterval())
                            : net.minecraft.world.entity.UpdateInterval.NEVER,
                    cow.getType().trackDeltas(), bytes);
        }

        List<String> take() {
            return bytes.take();
        }
    }

    /** 把官方包按 `StreamCodec` 写成「包 ID 的 VarInt + 包体」。 */
    private static final class Bytes implements ServerEntity.Synchronizer {
        private final ProtocolInfo<ClientGamePacketListener> protocol;
        private final RegistryAccess registries;
        private final List<byte[]> collected = new ArrayList<>();

        Bytes(RegistryAccess registries) {
            this.registries = registries;
            this.protocol = GameProtocols.CLIENTBOUND_TEMPLATE.bind(
                    RegistryFriendlyByteBuf.decorator(registries));
        }

        @SuppressWarnings("unchecked")
        private void add(Packet<? super ClientGamePacketListener> packet) {
            RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
            protocol.codec().encode(buffer, packet);
            byte[] body = new byte[buffer.readableBytes()];
            buffer.readBytes(body);
            collected.add(body);
        }

        /** 取走这一刻的全部包，每个包单独一个 base64 串。 */
        List<String> take() {
            List<String> out = new ArrayList<>();
            for (byte[] body : collected) {
                out.add(Base64.getEncoder().encodeToString(body));
            }
            collected.clear();
            return out;
        }

        @Override
        public void sendToTrackingPlayers(Packet<? super ClientGamePacketListener> packet) {
            add(packet);
        }

        @Override
        public void sendToTrackingPlayersAndSelf(Packet<? super ClientGamePacketListener> packet) {
            add(packet);
        }

        @Override
        public void sendToTrackingPlayersFiltered(Packet<? super ClientGamePacketListener> packet,
                java.util.function.Predicate<ServerPlayer> filter) {
            add(packet);
        }
    }

    static final class Scenes {
        static final BlockState GRASS = Blocks.GRASS_BLOCK.defaultBlockState();
        static final BlockState DIRT = Blocks.DIRT.defaultBlockState();
        static final BlockState STONE = Blocks.STONE.defaultBlockState();
        static final BlockState WATER = Blocks.WATER.defaultBlockState();
        static final BlockState LAVA = Blocks.LAVA.defaultBlockState();

        static void ground(Scene scene, int radius) {
            scene.fill(-radius, 62, -radius, radius, 62, radius, DIRT);
            scene.fill(-radius, 63, -radius, radius, 63, radius, GRASS);
        }

        static List<Scene> all() {
            List<Scene> scenes = new ArrayList<>();
            long[] seeds = {1L, 42L, 9001L, -7L};
            for (long seed : seeds) {
                Scene flat = new Scene("flat_" + seed).ticks(2000);
                ground(flat, 12);
                flat.cow(0.5, 64.0, 0.5, 0.0F, seed);
                // 32 格内有玩家，`checkDespawn` 每刻把 `noActionTime` 清零，牛一直闲逛。
                // A player within 32 blocks keeps `noActionTime` at zero, so the cow keeps strolling.
                flat.player(20.5, 64.0, 0.5, Items.AIR);
                scenes.add(flat);
            }
            {
                Scene alone = new Scene("alone").ticks(2000);
                ground(alone, 12);
                alone.cow(0.5, 64.0, 0.5, 0.0F, 5L);
                scenes.add(alone);
            }
            for (long seed : new long[] {3L, 77L}) {
                Scene steps = new Scene("steps_" + seed).ticks(2000);
                ground(steps, 12);
                BlockState slab = Blocks.STONE_SLAB.defaultBlockState();
                for (int x = -12; x <= 12; x++) {
                    for (int z = -12; z <= 12; z++) {
                        int h = Math.floorMod(x * 7 + z * 13 + (int) seed, 11);
                        if (h == 0) {
                            steps.set(x, 64, z, STONE);
                        } else if (h == 1) {
                            steps.set(x, 64, z, slab);
                        } else if (h == 2 && Math.abs(x) > 2) {
                            steps.set(x, 64, z, Blocks.OAK_STAIRS.defaultBlockState());
                        } else if (h == 3 && Math.abs(z) > 3) {
                            steps.set(x, 64, z, STONE);
                            steps.set(x, 65, z, STONE);
                        }
                    }
                }
                steps.fill(-1, 64, -1, 1, 65, 1, Blocks.AIR.defaultBlockState());
                steps.cow(0.5, 64.0, 0.5, 30.0F, seed);
                steps.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(steps);
            }
            for (long seed : new long[] {5L, 6L}) {
                Scene water = new Scene("water_edge_" + seed).ticks(2000);
                ground(water, 12);
                water.fill(2, 60, -5, 8, 63, 5, WATER);
                water.fill(2, 59, -5, 8, 59, 5, STONE);
                water.cow(0.5, 64.0, 0.5, -90.0F, seed);
                water.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(water);
                Scene swim = new Scene("water_in_" + seed).ticks(1000);
                ground(swim, 12);
                swim.fill(-4, 60, -4, 4, 63, 4, WATER);
                swim.fill(-4, 59, -4, 4, 59, 4, STONE);
                swim.cow(0.5, 62.0, 0.5, 45.0F, seed);
                swim.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(swim);
            }
            for (long seed : new long[] {8L, 12L}) {
                Scene cliff = new Scene("cliff_" + seed).ticks(2000);
                cliff.fill(-12, 52, -12, 12, 52, 12, STONE);
                cliff.fill(-12, 53, -12, 12, 53, 12, GRASS);
                cliff.fill(-4, 54, -4, 4, 62, 4, DIRT);
                cliff.fill(-4, 63, -4, 4, 63, 4, GRASS);
                cliff.cow(0.5, 64.0, 0.5, 10.0F, seed);
                cliff.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(cliff);
            }
            for (long seed : new long[] {13L, 21L}) {
                Scene pen = new Scene("fence_" + seed).ticks(2000);
                ground(pen, 8);
                BlockState fence = Blocks.OAK_FENCE.defaultBlockState();
                for (int i = -3; i <= 3; i++) {
                    pen.set(i, 64, -3, fence);
                    pen.set(i, 64, 3, fence);
                    pen.set(-3, 64, i, fence);
                    pen.set(3, 64, i, fence);
                }
                pen.cow(0.5, 64.0, 0.5, 0.0F, seed);
                pen.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(pen);
            }
            {
                Scene fall = new Scene("fall_damage").ticks(600);
                ground(fall, 6);
                fall.cow(0.5, 75.0, 0.5, 0.0F, 31L);
                scenes.add(fall);
                Scene death = new Scene("fall_death").ticks(600);
                ground(death, 6);
                death.cow(0.5, 120.0, 0.5, 0.0F, 32L);
                scenes.add(death);
            }
            for (long seed : new long[] {17L, 18L}) {
                Scene panic = new Scene("panic_" + seed).ticks(800);
                ground(panic, 12);
                panic.cow(0.5, 64.0, 0.5, 0.0F, seed);
                panic.player(3.5, 64.0, 0.5, Items.AIR);
                panic.hurts.add(new double[] {40, 0, 2.0, 0});
                panic.hurts.add(new double[] {45, 0, 3.0, 0});
                panic.hurts.add(new double[] {400, 0, 1.0, 0});
                scenes.add(panic);
            }
            for (long seed : new long[] {23L, 24L}) {
                Scene look = new Scene("look_player_" + seed).ticks(1500);
                ground(look, 12);
                look.cow(0.5, 64.0, 0.5, 0.0F, seed);
                look.player(3.5, 64.0, 2.5, Items.AIR);
                scenes.add(look);
            }
            {
                Scene tempt = new Scene("tempt_wheat").ticks(800);
                ground(tempt, 12);
                tempt.cow(0.5, 64.0, 0.5, 0.0F, 25L);
                tempt.player(6.5, 64.0, 0.5, Items.WHEAT);
                scenes.add(tempt);
                // 对称的墙：绕左边和绕右边一样远，A* 的平局规则决定走哪边。
                // A symmetric wall: both detours cost the same, so A*'s tie rules pick the side.
                for (int variant = 0; variant < 3; variant++) {
                    Scene wall = new Scene("tempt_wall_" + variant).ticks(600);
                    ground(wall, 12);
                    int half = 2 + variant;
                    wall.fill(-half, 64, 3, half, 65, 3, STONE);
                    wall.cow(0.5, 64.0, 0.5, 0.0F, 40L + variant);
                    wall.player(0.5, 64.0, 6.5 + variant, Items.WHEAT);
                    scenes.add(wall);
                }
                Scene maze = new Scene("tempt_maze").ticks(800);
                ground(maze, 12);
                for (int x = -6; x <= 6; x += 3) {
                    maze.fill(x, 64, -6, x, 65, 6, STONE);
                    maze.set(x, 64, (x * 5) % 7, Blocks.AIR.defaultBlockState());
                    maze.set(x, 65, (x * 5) % 7, Blocks.AIR.defaultBlockState());
                }
                maze.cow(-7.5, 64.0, 0.5, 0.0F, 44L);
                maze.player(1.5, 64.0, 0.5, Items.WHEAT);
                scenes.add(maze);
            }
            for (long seed : new long[] {27L, 28L}) {
                Scene hazard = new Scene("hazards_" + seed).ticks(2000);
                ground(hazard, 10);
                hazard.fill(3, 63, -2, 5, 63, 2, LAVA);
                hazard.set(-3, 64, 0, Blocks.CACTUS.defaultBlockState());
                hazard.set(-3, 64, 3, Blocks.CACTUS.defaultBlockState());
                hazard.set(0, 64, -4, Blocks.FIRE.defaultBlockState());
                hazard.set(1, 63, 4, Blocks.MAGMA_BLOCK.defaultBlockState());
                hazard.cow(0.5, 64.0, 0.5, 0.0F, seed);
                hazard.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(hazard);
            }
            {
                Scene cactus = new Scene("cactus_touch").ticks(600);
                ground(cactus, 8);
                cactus.set(1, 64, 0, Blocks.CACTUS.defaultBlockState());
                cactus.cow(0.6, 64.0, 0.5, 0.0F, 29L);
                scenes.add(cactus);
                Scene lava = new Scene("lava_in").ticks(600);
                ground(lava, 8);
                lava.fill(-2, 63, -2, 2, 63, 2, LAVA);
                lava.cow(0.5, 64.0, 0.5, 0.0F, 30L);
                scenes.add(lava);
            }
            {
                Scene magma = new Scene("magma_stand").ticks(600);
                ground(magma, 8);
                magma.set(0, 63, 0, Blocks.MAGMA_BLOCK.defaultBlockState());
                magma.cow(0.5, 64.0, 0.5, 0.0F, 35L);
                magma.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(magma);
                Scene berry = new Scene("berry_bush").ticks(600);
                ground(berry, 8);
                berry.fill(-1, 64, -1, 1, 64, 1,
                        Blocks.SWEET_BERRY_BUSH.defaultBlockState().setValue(
                                net.minecraft.world.level.block.SweetBerryBushBlock.AGE, 3));
                berry.cow(0.5, 64.0, 0.5, 0.0F, 36L);
                berry.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(berry);
                Scene web = new Scene("cobweb").ticks(600);
                ground(web, 8);
                web.set(0, 64, 0, Blocks.COBWEB.defaultBlockState());
                web.set(0, 65, 0, Blocks.COBWEB.defaultBlockState());
                web.cow(0.5, 66.0, 0.5, 0.0F, 37L);
                web.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(web);
                Scene snow = new Scene("powder_snow").ticks(800);
                ground(snow, 8);
                snow.fill(-1, 63, -1, 1, 63, 1, Blocks.POWDER_SNOW.defaultBlockState());
                snow.cow(0.5, 64.0, 0.5, 0.0F, 38L);
                snow.player(-20.5, 70.0, 0.5, Items.AIR);
                scenes.add(snow);
                Scene kill = new Scene("player_kill").ticks(600);
                ground(kill, 8);
                kill.cow(0.5, 64.0, 0.5, 0.0F, 39L);
                kill.player(2.5, 64.0, 0.5, Items.AIR);
                kill.hurts.add(new double[] {20, 0, 6.0, 0});
                kill.hurts.add(new double[] {60, 0, 6.0, 0});
                scenes.add(kill);
            }
            {
                // 生成入口：`/summon` 不带 NBT（跑 `finalizeSpawn`）、带 NBT（不跑）、UUID 重复；
                // 刷怪蛋点在地面上、点在台阶上（`getYOffset` 落到台阶面上）。
                // Spawn entry points: bare and NBT summons, a duplicate UUID, and spawn eggs.
                Scene bare = new Scene("summon_bare").ticks(600);
                ground(bare, 10);
                bare.summon(1, 2.5, 64.0, 0.5, null);
                bare.summon(3, -1.5, 64.0, 2.5, null);
                bare.player(6.5, 64.0, 0.5, Items.AIR);
                scenes.add(bare);
                Scene tagged = new Scene("summon_nbt").ticks(600);
                ground(tagged, 10);
                tagged.summon(1, 0.5, 64.0, 0.5,
                        "{Age:-300,Rotation:[45.0f,10.0f],Health:5.5f,variant:\"minecraft:warm\",sound_variant:\"minecraft:moody\",UUID:[I;1,2,3,4],PersistenceRequired:1b}");
                tagged.summon(2, 3.5, 64.0, 0.5, "{UUID:[I;1,2,3,4]}");
                tagged.summon(4, -2.5, 66.0, -2.5, "{NoAI:1b,Motion:[0.2d,0.3d,0.0d],Silent:1b}");
                scenes.add(tagged);
                Scene eggs = new Scene("spawn_egg").ticks(600);
                ground(eggs, 10);
                eggs.set(5, 64, 5, Blocks.STONE_SLAB.defaultBlockState());
                eggs.egg(1, 0, 64, 0, true);
                eggs.egg(2, 5, 65, 5, true);
                eggs.egg(5, -3, 64, 2, false);
                eggs.player(-6.5, 64.0, 0.5, Items.AIR);
                scenes.add(eggs);
            }
            {
                Scene herd = new Scene("two_cows").ticks(1000);
                ground(herd, 10);
                herd.cow(0.5, 64.0, 0.5, 0.0F, 33L);
                herd.cow(0.9, 64.0, 0.7, 90.0F, 34L);
                scenes.add(herd);
            }
            {
                // 出站包逐刻比字节：闲逛、挨打（速度包和实体事件）、幼年和富字段的元数据、
                // /summon 和刷怪蛋生成时的配对包。一个观察者始终在范围内。
                Scene idle = new Scene("packets_idle").ticks(400).packets();
                ground(idle, 8);
                idle.cow(0.5, 64.0, 0.5, 0.0F, 41L);
                idle.player(4.5, 64.0, 0.5, Items.AIR);
                scenes.add(idle);
                Scene hurt = new Scene("packets_hurt").ticks(400).packets();
                ground(hurt, 8);
                hurt.cow(0.5, 64.0, 0.5, 0.0F, 42L);
                hurt.player(2.5, 64.0, 0.5, Items.AIR);
                hurt.hurts.add(new double[] {60, 0, 3.0, 0});
                scenes.add(hurt);
                Scene tagged = new Scene("packets_nbt").ticks(400).packets();
                ground(tagged, 8);
                tagged.summon(1, 0.5, 64.0, 0.5,
                        "{Age:-300,Rotation:[45.0f,10.0f],Health:5.5f,variant:\"minecraft:warm\",sound_variant:\"minecraft:moody\",UUID:[I;1,2,3,4],Silent:1b}");
                tagged.player(4.5, 64.0, 0.5, Items.AIR);
                scenes.add(tagged);
                Scene eggs = new Scene("packets_egg").ticks(400).packets();
                ground(eggs, 8);
                eggs.egg(1, 0, 64, 0, true);
                eggs.player(4.5, 64.0, 0.5, Items.AIR);
                scenes.add(eggs);
            }
            return scenes;
        }
    }

    // ---- 运行 ----

    private static void play(Official official, Writer out, Scene scene) throws Exception {
        // 每个包场景一份共用收集器：实体事件、音效和同步包按发生顺序进同一串字节。
        Bytes events = scene.packets ? new Bytes(official.registries) : null;
        StubList list = (StubList) official.server.getPlayerList();
        if (events != null) {
            list.sounds = events::add;
        }
        FakeLevel level = FakeLevel.create(official);
        if (events != null) {
            level.chunkSource.events = events::add;
            level.packets = events::add;
        }
        level.blocks.putAll(scene.blocks);
        level.nextId = FIRST_ID;
        List<ServerPlayer> players = new ArrayList<>();
        for (int i = 0; i < scene.players.size(); i++) {
            double[] at = scene.players.get(i);
            GameProfile profile = new GameProfile(new UUID(1L, 2L + i), "Oracle" + i);
            ServerPlayer player = new ServerPlayer(official.server, level, profile, ClientInformation.createDefault());
            player.setPos(at[0], at[1], at[2]);
            player.setItemInHand(InteractionHand.MAIN_HAND, new ItemStack(scene.playerItems.get(i)));
            players.add(player);
        }
        level.playersList().addAll(players);
        List<Cow> cows = new ArrayList<>();
        int playerIds = level.nextId;
        /** 逐刻的出站包字节；外层数组是刻，内层是那一刻这一头牛的全部包。 */
        List<List<String>> packets = null;
        /** 逐刻的同步标志位，Rust 侧喂给 `TrackedEntity::step`。 */
        List<List<String>> syncFlags = null;
        /** 每头牛一个官方 `ServerEntity`，按 `cows` 的下标对齐。 */
        List<Sync> syncs = null;
        if (scene.packets) {
            packets = new ArrayList<>();
            syncFlags = new ArrayList<>();
            syncs = new ArrayList<>();
            for (int tick = 0; tick <= scene.ticks; tick++) {
                packets.add(new ArrayList<>());
                syncFlags.add(new ArrayList<>());
            }
        }
        for (int i = 0; i < scene.cows.size(); i++) {
            Cow cow = EntityTypes.COW.create(level, EntitySpawnReason.COMMAND);
            double[] at = scene.cows.get(i);
            // Bamboo 生成这头牛时从种子源取过一个数，随后被场景的种子盖掉；这里照样消耗一个。
            // Bamboo draws one seed when spawning it before the scene seed overrides it.
            level.entitySeeds.nextLong();
            setRandom(cow, scene.seeds.get(i));
            // 场景里的牛 UUID 固定成 (0, 下标 + 1)，Bamboo 一侧照设。
            cow.setUUID(new UUID(0L, i + 1));
            cow.snapTo(at[0], at[1], at[2], scene.yaws.get(i)[0], 0.0F);
            cow.setYHeadRot(scene.yaws.get(i)[0]);
            cow.setYBodyRot(scene.yaws.get(i)[0]);
            cows.add(cow);
            level.tracked.add(cow);
        }
        if (scene.packets) {
            // 先给每头牛建 ServerEntity；配对包在 tick 0 由 `sendPairingData` 发出。
            // Create a ServerEntity per cow; sendPairingData emits the tick-0 packets.
            for (Cow cow : cows) {
                Sync sync = new Sync(level, cow, official.registries);
                syncs.add(sync);
                sync.entity.sendPairingData(null, packet -> sync.bytes.add(packet));
                packets.get(0).addAll(sync.take());
                syncFlags.get(0).add(flags(cow));
            }
        }
        StringBuilder line = new StringBuilder(1 << 20);
        line.append("{\"type\":\"case\",\"name\":\"").append(scene.name).append('"');
        line.append(",\"firstId\":").append(cows.isEmpty() ? level.nextId : cows.get(0).getId());
        line.append(",\"playerIdBase\":").append(playerIds);
        line.append(",\"startTime\":").append(START_TIME);
        line.append(",\"blocks\":").append(blockArray(scene.blocks));
        line.append(",\"cows\":[");
        for (int i = 0; i < cows.size(); i++) {
            if (i > 0) line.append(',');
            double[] at = scene.cows.get(i);
            line.append("{\"x\":").append(Double.doubleToRawLongBits(at[0]));
            line.append(",\"y\":").append(Double.doubleToRawLongBits(at[1]));
            line.append(",\"z\":").append(Double.doubleToRawLongBits(at[2]));
            line.append(",\"yaw\":").append(Float.floatToRawIntBits(scene.yaws.get(i)[0]));
            line.append(",\"seed\":").append(scene.seeds.get(i)).append('}');
        }
        line.append("],\"players\":[");
        for (int i = 0; i < players.size(); i++) {
            if (i > 0) line.append(',');
            ServerPlayer player = players.get(i);
            line.append("{\"id\":").append(player.getId());
            line.append(",\"x\":").append(Double.doubleToRawLongBits(player.getX()));
            line.append(",\"y\":").append(Double.doubleToRawLongBits(player.getY()));
            line.append(",\"z\":").append(Double.doubleToRawLongBits(player.getZ()));
            line.append(",\"eye\":").append(Float.floatToRawIntBits(player.getEyeHeight()));
            line.append(",\"item\":").append(BuiltInRegistries.ITEM.getId(scene.playerItems.get(i))).append('}');
        }
        line.append("],\"hurts\":[");
        for (int i = 0; i < scene.hurts.size(); i++) {
            if (i > 0) line.append(',');
            double[] hurt = scene.hurts.get(i);
            line.append('[').append((int) hurt[0]).append(',').append((int) hurt[1]).append(',')
                    .append(Float.floatToRawIntBits((float) hurt[2])).append(',').append((int) hurt[3]).append(']');
        }
        line.append("],\"summons\":[");
        for (int i = 0; i < scene.summons.size(); i++) {
            if (i > 0) line.append(',');
            double[] summon = scene.summons.get(i);
            line.append("{\"tick\":").append((int) summon[0]);
            line.append(",\"x\":").append(Double.doubleToRawLongBits(summon[1]));
            line.append(",\"y\":").append(Double.doubleToRawLongBits(summon[2]));
            line.append(",\"z\":").append(Double.doubleToRawLongBits(summon[3]));
            String nbt = scene.summonNbt.get(i);
            line.append(",\"nbt\":").append(nbt == null ? "null" : jsonString(nbt)).append('}');
        }
        line.append("],\"eggs\":[");
        for (int i = 0; i < scene.eggs.size(); i++) {
            if (i > 0) line.append(',');
            int[] egg = scene.eggs.get(i);
            line.append('[').append(egg[0]).append(',').append(egg[1]).append(',').append(egg[2])
                    .append(',').append(egg[3]).append(',').append(egg[4]).append(']');
        }
        line.append("],\"steps\":[");
        line.append(stepJson(cows));
        for (int tick = 1; tick <= scene.ticks; tick++) {
            level.gameTime++;
            for (double[] hurt : scene.hurts) {
                if ((int) hurt[0] == tick) {
                    Cow target = cows.get((int) hurt[1]);
                    ServerPlayer attacker = players.get((int) hurt[3]);
                    target.hurtServer(level, level.damageSources().playerAttack(attacker), (float) hurt[2]);
                }
            }
            for (int i = 0; i < scene.summons.size(); i++) {
                double[] summon = scene.summons.get(i);
                if ((int) summon[0] != tick) {
                    continue;
                }
                Cow cow = summon(level, official, new Vec3(summon[1], summon[2], summon[3]), scene.summonNbt.get(i));
                if (cow != null) {
                    cows.add(cow);
                    if (scene.packets) {
                        addSync(level, official, cow, syncs, packets, tick);
                    }
                }
            }
            for (int[] egg : scene.eggs) {
                if (egg[0] != tick) {
                    continue;
                }
                if (scene.packets) {
                    level.eggSeed = level.entitySeeds.nextLong();
                }
                Cow cow = EntityTypes.COW.spawn(level, new ItemStack(Items.COW_SPAWN_EGG), null,
                        new BlockPos(egg[1], egg[2], egg[3]), EntitySpawnReason.SPAWN_ITEM_USE, true, egg[4] != 0);
                if (!scene.packets) {
                    reseed(level, cow, 2);
                }
                level.tracked.add(cow);
                cows.add(cow);
                if (scene.packets) {
                    addSync(level, official, cow, syncs, packets, tick);
                }
            }
            for (Cow cow : new ArrayList<>(cows)) {
                if (cow.isRemoved()) {
                    continue;
                }
                cow.checkDespawn();
                cow.commonTick();
                cow.tick();
            }
            level.tracked.removeIf(Entity::isRemoved);
            line.append(',').append(stepJson(cows));
            if (scene.packets) {
                // 这一刻的顺序：中途生成的配对包、实体 tick 里的事件/音效、
                // 然后每头牛按 uid 顺序的同步包。
                // Order: mid-tick pairings, events during entity ticks, then each
                // cow's sync packets in id order.
                List<String> step = packets.get(tick);
                step.addAll(events.take());
                for (int i = 0; i < cows.size(); i++) {
                    Cow cow = cows.get(i);
                    Sync sync = syncs.get(i);
                    syncFlags.get(tick).add(flags(cow));
                    sync.entity.sendChanges();
                    step.addAll(sync.take());
                }
            }
        }
        line.append("],\"packets\":");
        appendPackets(line, packets);
        line.append(",\"syncFlags\":");
        appendPackets(line, syncFlags);
        line.append("}\n");
        out.write(line.toString());
        out.flush();
    }

    /**
     * {@code SummonCommand.createEntity}：和命令一样的路径，失败（UUID 重复）时返回空。
     * 实体随机源换成 Bamboo 种子源取的种子，构造时的 UUID 和朝向照样重取。
     */
    private static Cow summon(FakeLevel level, Official official, Vec3 pos, String nbt) throws Exception {
        net.minecraft.commands.CommandSourceStack source = new net.minecraft.commands.CommandSourceStack(
                net.minecraft.commands.CommandSource.NULL, pos, net.minecraft.world.phys.Vec2.ZERO, level,
                LevelBasedPermissionSet.GAMEMASTER, net.minecraft.network.chat.Component.literal("Oracle"), official.server);
        net.minecraft.nbt.CompoundTag tag = nbt == null
                ? new net.minecraft.nbt.CompoundTag()
                : net.minecraft.nbt.TagParser.parseCompoundFully(nbt);
        UUID given = tag.read("UUID", net.minecraft.core.UUIDUtil.CODEC).orElse(null);
        int before = level.nextId;
        Entity entity;
        try {
            entity = net.minecraft.server.commands.SummonCommand.createEntity(
                    source, EntityTypes.COW.builtInRegistryHolder(), pos, tag, nbt == null);
        } catch (com.mojang.brigadier.exceptions.CommandSyntaxException error) {
            // 失败时官方已经分配过 ID、构造过实体；Bamboo 也一样取过号和种子。
            level.entitySeeds.nextLong();
            return null;
        }
        Cow cow = (Cow) entity;
        reseed(level, cow, 0);
        if (given != null) {
            cow.setUUID(given);
        }
        return cow;
    }

    /**
     * 按 Bamboo 的种子源给新实体换随机源，重放构造时的 UUID（两个 {@code nextLong}）和朝向
     * （一个 {@code nextFloat}），再重放 {@code extraFloats} 个 {@code nextFloat}。
     */
    private static void reseed(FakeLevel level, Cow cow, int extraFloats) {
        setRandom(cow, level.entitySeeds.nextLong());
        RandomSource random = cow.getRandom();
        cow.setUUID(Mth.createInsecureUUID(random));
        random.nextFloat();
        for (int i = 0; i < extraFloats; i++) {
            random.nextFloat();
        }
    }

    /** 某一刻的全部出站包，每个包一个 base64 串。 */
    private static void appendPackets(StringBuilder line, List<List<String>> packets) {
        if (packets == null) {
            line.append("null");
            return;
        }
        line.append('[');
        for (int tick = 0; tick < packets.size(); tick++) {
            if (tick > 0) line.append(',');
            line.append('[');
            List<String> step = packets.get(tick);
            for (int i = 0; i < step.size(); i++) {
                if (i > 0) line.append(',');
                line.append(jsonString(step.get(i)));
            }
            line.append(']');
        }
        line.append(']');
    }

    /** `Entity` 上影响 `ServerEntity.sendChanges` 的标志位。 */
    private static String flags(Entity entity) {
        StringBuilder out = new StringBuilder();
        out.append(entity.needsSync ? '1' : '0');
        out.append(entity.syncPosition ? '1' : '0');
        out.append(entity.syncVelocity ? '1' : '0');
        out.append(entity.getRequiresPrecisePosition() ? '1' : '0');
        out.append(entity instanceof LivingEntity living && living.isFallFlying() ? '1' : '0');
        return out.toString();
    }

    /** 生成后中途出现的牛：这一刻发一次配对包。 */
    private static void addSync(FakeLevel level, Official official, Cow cow, List<Sync> syncs,
            List<List<String>> packets, int tick) {
        Sync sync = new Sync(level, cow, official.registries);
        syncs.add(sync);
        sync.entity.sendPairingData(null, packet -> sync.bytes.add(packet));
        packets.get(tick).addAll(sync.take());
    }

    private static String jsonString(String text) {
        StringBuilder out = new StringBuilder("\"");
        for (char c : text.toCharArray()) {
            if (c == '"' || c == '\\') out.append('\\');
            out.append(c);
        }
        return out.append('"').toString();
    }

    private static String stepJson(List<Cow> cows) throws Exception {
        StringBuilder out = new StringBuilder(512);
        out.append('[');
        for (int i = 0; i < cows.size(); i++) {
            if (i > 0) out.append(',');
            Cow cow = cows.get(i);
            Vec3 pos = cow.position();
            Vec3 delta = cow.getDeltaMovement();
            out.append("{\"x\":").append(Double.doubleToRawLongBits(pos.x));
            out.append(",\"y\":").append(Double.doubleToRawLongBits(pos.y));
            out.append(",\"z\":").append(Double.doubleToRawLongBits(pos.z));
            out.append(",\"dx\":").append(Double.doubleToRawLongBits(delta.x));
            out.append(",\"dy\":").append(Double.doubleToRawLongBits(delta.y));
            out.append(",\"dz\":").append(Double.doubleToRawLongBits(delta.z));
            out.append(",\"yr\":").append(Float.floatToRawIntBits(cow.getYRot()));
            out.append(",\"xr\":").append(Float.floatToRawIntBits(cow.getXRot()));
            out.append(",\"hr\":").append(Float.floatToRawIntBits(cow.getYHeadRot()));
            out.append(",\"br\":").append(Float.floatToRawIntBits(cow.yBodyRot));
            out.append(",\"hp\":").append(Float.floatToRawIntBits(cow.getHealth()));
            out.append(",\"g\":").append(cow.onGround());
            out.append(",\"rm\":").append(cow.isRemoved());
            out.append(",\"w\":").append(cow.isInWater());
            out.append(",\"fd\":").append(Double.doubleToRawLongBits(cow.fallDistance));
            out.append(",\"nat\":").append(cow.getNoActionTime());
            out.append(",\"ht\":").append(cow.hurtTime);
            out.append(",\"dt\":").append(cow.deathTime);
            out.append(",\"fire\":").append(cow.getRemainingFireTicks());
            out.append(",\"air\":").append(cow.getAirSupply());
            out.append(",\"seed\":").append(randomSeed(cow));
            out.append(",\"age\":").append(cow.getAge());
            out.append(",\"lh\":").append(cow.isLeftHanded());
            out.append(",\"fr\":").append(Double.doubleToRawLongBits(
                    cow.getAttributeValue(net.minecraft.world.entity.ai.attributes.Attributes.FOLLOW_RANGE)));
            out.append(",\"var\":\"").append(cow.getVariant().unwrapKey().orElseThrow().identifier()).append('"');
            Method sound = Cow.class.getDeclaredMethod("getSoundVariant");
            sound.setAccessible(true);
            Holder<?> soundVariant = (Holder<?>) sound.invoke(cow);
            out.append(",\"snd\":\"").append(soundVariant.unwrapKey().orElseThrow().identifier()).append('"');
            out.append(",\"uuid\":\"").append(cow.getUUID()).append('"');
            out.append(",\"lr\":").append(levelSeed(cow.level()));
            out.append(",\"goals\":\"");
            boolean first = true;
            for (WrappedGoal goal : cow.getGoalSelector().getAvailableGoals()) {
                if (goal.isRunning()) {
                    if (!first) out.append(' ');
                    first = false;
                    out.append(goal.getGoal().getClass().getSimpleName());
                }
            }
            out.append('"');
            net.minecraft.world.level.pathfinder.Path path = cow.getNavigation().getPath();
            out.append(",\"path\":");
            if (path == null) {
                out.append("null");
            } else {
                out.append('[').append(path.getNextNodeIndex());
                for (int n = 0; n < path.getNodeCount(); n++) {
                    Node node = path.getNode(n);
                    out.append(',').append(node.x).append(',').append(node.y).append(',').append(node.z);
                }
                out.append(']');
            }
            out.append('}');
        }
        out.append(']');
        return out.toString();
    }

    private static long levelSeed(Level level) throws Exception {
        RandomSource random = level.getRandom();
        Field field = random.getClass().getDeclaredField("seed");
        field.setAccessible(true);
        return ((AtomicLong) field.get(random)).get();
    }

    private static long randomSeed(Entity entity) throws Exception {
        RandomSource random = entity.getRandom();
        Field field = random.getClass().getDeclaredField("seed");
        field.setAccessible(true);
        return ((AtomicLong) field.get(random)).get();
    }

    private static String blockArray(Long2ObjectOpenHashMap<BlockState> blocks) {
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
            if (!first) out.append(',');
            first = false;
            BlockPos pos = BlockPos.of(key);
            out.append('[').append(pos.getX()).append(',').append(pos.getY()).append(',').append(pos.getZ())
                    .append(',').append(Block.getId(state)).append(']');
        }
        out.append(']');
        return out.toString();
    }

    private static void setRandom(Entity entity, long seed) {
        try {
            Field field = Entity.class.getDeclaredField("random");
            UNSAFE.putObject(entity, UNSAFE.objectFieldOffset(field), RandomSource.create(seed));
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    // ---- 假服务端和假世界 ----

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

        /** 环境音：把官方的音效包也记进同一刻的字节流。 */
        java.util.function.Consumer<Packet<? super ClientGamePacketListener>> sounds;

        @Override
        public void broadcast(Player except, double x, double y, double z, double range,
                net.minecraft.resources.ResourceKey<Level> dimension, Packet<?> packet) {
            if (sounds != null) {
                @SuppressWarnings("unchecked")
                Packet<? super ClientGamePacketListener> cast =
                        (Packet<? super ClientGamePacketListener>) packet;
                sounds.accept(cast);
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
        final net.minecraft.world.level.border.WorldBorder border = new net.minecraft.world.level.border.WorldBorder();
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
            Official official = new Official(managers);
            FakeLevel sample = FakeLevel.create(official);
            official.attributes = EnvironmentAttributeSystem.builder().addStaticLayers(sample).build();
            return official;
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

    /** 游戏时间可以前进的世界数据。 */
    private static final class TickingLevelData implements LevelData {
        private final FakeLevel level;

        TickingLevelData(FakeLevel level) {
            this.level = level;
        }

        @Override
        public LevelData.RespawnData getRespawnData() {
            return LevelData.RespawnData.DEFAULT;
        }

        @Override
        public long getGameTime() {
            return level.gameTime;
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

    /** 区块缓存：寻路和地面搜索走 {@code getChunkNow}，交给假世界的区块表。 */
    private static final class Chunks extends ServerChunkCache {
        FakeLevel level;
        /** 实体事件和音效走这里，和同步包一起记进同一刻的字节流。 */
        java.util.function.Consumer<Packet<? super ClientGamePacketListener>> events;

        private Chunks() {
            super(null, null, null, null, null, null, 0, 0, false, null);
        }

        @Override
        public LevelChunk getChunkNow(int chunkX, int chunkZ) {
            return (LevelChunk) level.getChunk(chunkX, chunkZ, ChunkStatus.FULL, false);
        }

        @Override
        public void move(ServerPlayer player) {}

        @Override
        public void sendToTrackingPlayersAndSelf(Entity entity,
                Packet<? super ClientGamePacketListener> packet) {
            if (events != null) {
                events.accept(packet);
            }
        }

        @Override
        public void sendToTrackingPlayers(Entity entity,
                Packet<? super ClientGamePacketListener> packet) {
            if (events != null) {
                events.accept(packet);
            }
        }
    }

    private static final class FakeLevel extends ServerLevel {
        private Official official;
        private Long2ObjectOpenHashMap<BlockState> blocks;
        private LevelTicks<Block> blockTicks;
        private LevelTicks<net.minecraft.world.level.material.Fluid> fluidTicks;
        private Chunks chunkSource;
        private Long2ObjectOpenHashMap<ChunkAccess> chunkCache;
        private ServerScoreboard scoreboard;
        private TickRateManager tickRate;
        private LevelData levelData;
        private List<Entity> tracked;
        private long gameTime;
        private int nextId;
        /** Bamboo 的岛实体种子源，按生成先后取数。 */
        private RandomSource entitySeeds;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        static FakeLevel create(Official official) {
            FakeLevel level = allocate(FakeLevel.class);
            level.official = official;
            level.blocks = new Long2ObjectOpenHashMap<>();
            level.blockTicks = new LevelTicks<>(key -> true);
            level.fluidTicks = new LevelTicks<>(key -> true);
            level.chunkSource = allocate(Chunks.class);
            level.chunkSource.level = level;
            ChunkMap map = allocate(ChunkMap.class);
            put(ChunkMap.class, map, "entityMap", new Int2ObjectOpenHashMap<>());
            put(ServerChunkCache.class, level.chunkSource, "chunkMap", map);
            put(Level.class, level, "neighborUpdater", new net.minecraft.world.level.redstone.CollectingNeighborUpdater(level, 1_000_000));
            put(Level.class, level, "random", RandomSource.create(1L));
            put(Level.class, level, "soundSeedGenerator", RandomSource.create(12345L));
            put(Level.class, level, "dimensionTypeRegistration", official.overworld);
            put(Level.class, level, "dimension", Level.OVERWORLD);
            put(Level.class, level, "registryAccess", official.registries);
            put(Level.class, level, "palettedContainerFactory", PalettedContainerFactory.create(official.registries));
            put(Level.class, level, "damageSources", official.damageSources);
            put(Level.class, level, "thread", Thread.currentThread());
            put(ServerLevel.class, level, "players", new ArrayList<ServerPlayer>());
            put(ServerLevel.class, level, "server", official.server);
            put(ServerLevel.class, level, "chunkSource", level.chunkSource);
            if (official.attributes != null) {
                put(ServerLevel.class, level, "environmentAttributes", official.attributes);
            }
            level.chunkCache = new Long2ObjectOpenHashMap<>();
            level.scoreboard = new ServerScoreboard(official.server);
            level.tickRate = new TickRateManager();
            level.tracked = new ArrayList<>();
            level.gameTime = START_TIME;
            level.levelData = new TickingLevelData(level);
            level.nextId = 1;
            level.entitySeeds = RandomSource.create(ENTITY_RANDOM_SEED);
            return level;
        }

        @SuppressWarnings("unchecked")
        List<ServerPlayer> playersList() {
            try {
                Field field = ServerLevel.class.getDeclaredField("players");
                field.setAccessible(true);
                return (List<ServerPlayer>) field.get(this);
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
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
        public int getMaxLocalRawBrightness(BlockPos pos) {
            return 15;
        }

        @Override
        public int getMaxLocalRawBrightness(BlockPos pos, int darken) {
            return 15;
        }

        private net.minecraft.server.waypoints.ServerWaypointManager waypoints;

        @Override
        public net.minecraft.server.waypoints.ServerWaypointManager getWaypointManager() {
            // 构造函数没跑，字段初始化也没跑，要用时再建。
            if (waypoints == null) {
                waypoints = new net.minecraft.server.waypoints.ServerWaypointManager();
            }
            return waypoints;
        }

        @Override
        public boolean isPositionEntityTicking(BlockPos pos) {
            return true;
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
            LevelChunk chunk = new LevelChunk(this, new ChunkPos(chunkX, chunkZ));
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
        public int getNextEntityId() {
            return nextId++;
        }

        @Override
        public boolean addFreshEntity(Entity entity) {
            // `EntityType.spawn` 在加入世界之后才叫 `playAmbientSound`。官方实体的
            // 随机源不带种子，这里换成 Bamboo 的种子源并只重放构造时的三个数，
            // 这样那一声的音高也是确定的（其余场景的牛仍由场景显式给种子）。
            // `EntityType.spawn` plays the ambient sound after adding the entity.
            // Vanilla's entity random is unseeded; replace it with Bamboo's seed
            // source and replay only the constructor draws so the pitch is fixed.
            if (eggSeed != null) {
                setRandom(entity, eggSeed);
                RandomSource random = entity.getRandom();
                entity.setUUID(Mth.createInsecureUUID(random));
                random.nextFloat();
                eggSeed = null;
            }
            return true;
        }

        /** UUID 和已有实体重复时放不进去，其余放进实体名单。 */
        @Override
        public boolean tryAddFreshEntityWithPassengers(Entity entity) {
            for (Entity existing : tracked) {
                if (existing.getUUID().equals(entity.getUUID())) {
                    return false;
                }
            }
            tracked.add(entity);
            return true;
        }

        /** 下一次 `addFreshEntity` 要用的种子（只在刷怪蛋场景里设）。 */
        Long eggSeed;

        @Override
        public net.minecraft.world.DifficultyInstance getCurrentDifficultyAt(BlockPos pos) {
            return new net.minecraft.world.DifficultyInstance(Difficulty.NORMAL, gameTime, 0L, 0.0F);
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
            return levelData;
        }

        @Override
        public long getGameTime() {
            return gameTime;
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
        public Holder<Biome> getBiome(BlockPos pos) {
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
        public <T extends net.minecraft.core.particles.ParticleOptions> int sendParticles(
                T particle, double x, double y, double z, int count, double xd, double yd, double zd, double speed) {
            return 0;
        }

        /** 实体事件、伤害事件和音效都记进同一刻的包序列。 */
        java.util.function.Consumer<Packet<? super ClientGamePacketListener>> packets;

        @Override
        public void broadcastEntityEvent(Entity entity, byte event) {
            if (packets != null) {
                packets.accept(new net.minecraft.network.protocol.game.ClientboundEntityEventPacket(entity, event));
            }
        }

        @Override
        public void broadcastDamageEvent(Entity entity, net.minecraft.world.damagesource.DamageSource source) {
            if (packets != null) {
                packets.accept(new net.minecraft.network.protocol.game.ClientboundDamageEventPacket(entity, source));
            }
        }

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
                long seed) {
            if (packets != null) {
                packets.accept(new net.minecraft.network.protocol.game.ClientboundSoundPacket(
                        sound, source, x, y, z, volume, pitch, seed));
            }
        }

        @Override
        public void playSeededSound(
                Entity except,
                Entity sourceEntity,
                Holder<net.minecraft.sounds.SoundEvent> sound,
                net.minecraft.sounds.SoundSource source,
                float volume,
                float pitch,
                long seed) {
            playSeededSound(except, sourceEntity.getX(), sourceEntity.getY(), sourceEntity.getZ(),
                    sound, source, volume, pitch, seed);
        }

        @Override
        public Collection<EnderDragonPart> dragonParts() {
            return List.of();
        }

        @Override
        protected LevelEntityGetter<Entity> getEntities() {
            return new TrackedEntities(tracked);
        }

        @Override
        public void updatePOIOnBlockStateChange(BlockPos pos, BlockState oldState, BlockState newState) {}
    }

    private static final class TrackedEntities implements LevelEntityGetter<Entity> {
        private final List<Entity> entities;

        TrackedEntities(List<Entity> entities) {
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
        public Entity get(UUID id) {
            for (Entity entity : entities) {
                if (entity.getUUID().equals(id)) {
                    return entity;
                }
            }
            return null;
        }

        @Override
        public Iterable<Entity> getAll() {
            return entities;
        }

        @Override
        public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AbortableIterationConsumer<U> consumer) {
            for (Entity entity : entities) {
                U cast = type.tryCast(entity);
                if (cast != null && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }

        @Override
        public void get(AABB box, Consumer<Entity> output) {
            for (Entity entity : entities) {
                if (entity.getBoundingBox().intersects(box)) {
                    output.accept(entity);
                }
            }
        }

        @Override
        public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AABB box, AbortableIterationConsumer<U> consumer) {
            for (Entity entity : entities) {
                U cast = type.tryCast(entity);
                if (cast != null && entity.getBoundingBox().intersects(box) && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }
    }

    private MobOracle() {}
}
