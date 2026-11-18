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
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.server.network.ServerCommonPacketListenerImpl;
import net.minecraft.network.Connection;
import net.minecraft.network.protocol.PacketFlow;
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
import net.minecraft.resources.Identifier;
import net.minecraft.util.RandomSource;
import net.minecraft.world.RandomSequences;
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
    /** 新实体从岛种子源取到的初始种子，按实体 ID 记下给对拍用。 */
    private static final java.util.Map<Integer, Long> BORN_SEEDS = new java.util.HashMap<>();
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
        /** 每头牛的初始 `Age`，默认 0（成年）。 */
        final List<Integer> cowAges = new ArrayList<>();
        final List<double[]> players = new ArrayList<>();
        final List<Item> playerItems = new ArrayList<>();
        /** 玩家主手物品的抢夺等级，0 表示没有。 */
        final List<Integer> lootingLevels = new ArrayList<>();
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
        /** 逐刻比死亡时生成的掉落物和经验球。 */
        boolean drops;
        /** 致死伤害的种类：0 玩家攻击，1 摔落，2 着火。 */
        int deathKind;
        /** 场景开始时给每头牛设的着火刻数。 */
        int fireTicks;
        /** 每项是（刻、牛下标、玩家下标、主手物品 ID、创造模式 1/0）。 */
        final List<int[]> interactions = new ArrayList<>();
        /** 玩家受伤场景：`getDifficulty()` 的值，缺省 NORMAL。 */
        int difficulty = 2;
        /** 玩家受伤场景：逐刻的动作，见 `PlayerAction`。 */
        final List<PlayerAction> playerActions = new ArrayList<>();
        /** 玩家受伤场景：起始装备（四件盔甲 + 主手），按物品 ID。 */
        final List<int[]> playerEquipment = new ArrayList<>();
        /** 玩家受伤场景：护甲附魔等级（保护附魔，四件盔甲各一个）。 */
        final List<int[]> playerEnchants = new ArrayList<>();
        /** 玩家受伤场景：起步血量、吸收和饥饿消耗。 */
        float playerHealth = 20.0F;
        float playerAbsorption = 0.0F;
        float playerExhaustion = 0.0F;
        /** 玩家受伤场景：逐刻比玩家自己的包字节。 */
        boolean playerPackets;

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
            return cow(x, y, z, yaw, seed, 0);
        }

        Scene cow(double x, double y, double z, float yaw, long seed, int age) {
            cows.add(new double[] {x, y, z});
            yaws.add(new float[] {yaw});
            seeds.add(seed);
            cowAges.add(age);
            return this;
        }

        Scene player(double x, double y, double z, Item item) {
            return player(x, y, z, item, 0);
        }

        Scene player(double x, double y, double z, Item item, int looting) {
            players.add(new double[] {x, y, z});
            playerItems.add(item);
            lootingLevels.add(looting);
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

        /** 这个场景逐刻比死亡掉落和经验球。 */
        Scene drops() {
            drops = true;
            return this;
        }

        /** 致死伤害的种类，见 `deathKind`。 */
        Scene deathKind(int kind) {
            deathKind = kind;
            return this;
        }

        /** 给每头牛设着火刻数，检验熟牛肉掉落。 */
        Scene fire(int ticks) {
            fireTicks = ticks;
            return this;
        }

        /** 某一刻玩家右键一头牛；`item` 是主手物品注册表 ID，`creative` 是模式。 */
        Scene interact(int tick, int cow, int player, int item, int creative) {
            interactions.add(new int[] {tick, cow, player, item, creative});
            return this;
        }

        Scene difficulty(int id) {
            difficulty = id;
            return this;
        }

        Scene playerPackets() {
            playerPackets = true;
            return this;
        }

        Scene playerHealth(float health, float absorption, float exhaustion) {
            playerHealth = health;
            playerAbsorption = absorption;
            playerExhaustion = exhaustion;
            return this;
        }

        /** 一名玩家的装备：靴子、护腿、胸甲、头盔、主手，以及四件盔甲的保护等级。 */
        Scene equipment(int[] items, int[] protection) {
            playerEquipment.add(items);
            playerEnchants.add(protection);
            return this;
        }

        /** 打玩家：`kind` 是 damage_type 的名字，`amount` 是伤害量，`at` 是来源位置。 */
        Scene hurt(int tick, String kind, float amount, double[] at) {
            playerActions.add(new PlayerAction(tick, 0, kind, amount, at, false));
            return this;
        }

        /** 玩家左键打一头牛。 */
        Scene attack(int tick, int cow) {
            playerActions.add(new PlayerAction(tick, 1, null, 0.0F, null, false));
            playerActions.get(playerActions.size() - 1).cow = cow;
            return this;
        }
    }

    /** 玩家受伤场景的一个动作：打玩家或者左键打牛。 */
    static final class PlayerAction {
        final int tick;
        /** 0 打玩家，1 左键打牛。 */
        final int action;
        final String kind;
        final float amount;
        final double[] at;
        final boolean creative;
        int cow;

        PlayerAction(int tick, int action, String kind, float amount, double[] at, boolean creative) {
            this.tick = tick;
            this.action = action;
            this.kind = kind;
            this.amount = amount;
            this.at = at;
            this.creative = creative;
        }
    }

    /** 每头牛的官方 `ServerEntity` 和一个记下包字节的假 Synchronizer。 */
    private static final class Sync {
        final ServerEntity entity;
        final Bytes bytes;

        Sync(FakeLevel level, Entity target, RegistryAccess registries) {
            bytes = new Bytes(registries);
            entity = new ServerEntity(level, target,
                    target.getType().hasUpdateInterval()
                            ? net.minecraft.world.entity.UpdateInterval.periodic(target.getType().updateInterval())
                            : net.minecraft.world.entity.UpdateInterval.NEVER,
                    target.getType().trackDeltas(), bytes);
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
            {
                // 死亡：玩家打死、摔死、烧死（熟牛肉）、带抢夺 III 打死。逐刻比掉落物、
                // 经验球和出站包（伤害事件、死亡事件、移除、生成包）。
                Scene killed = new Scene("death_player").ticks(60).drops().packets();
                ground(killed, 6);
                killed.cow(0.5, 64.0, 0.5, 0.0F, 51L);
                killed.player(2.5, 64.0, 0.5, Items.AIR);
                killed.hurts.add(new double[] {2, 0, 20.0, 0});
                scenes.add(killed);
                Scene fell = new Scene("death_fall").ticks(60).drops().deathKind(1);
                ground(fell, 6);
                fell.cow(0.5, 64.0, 0.5, 30.0F, 52L);
                fell.player(4.5, 64.0, 0.5, Items.AIR);
                fell.hurts.add(new double[] {2, 0, 20.0, 0});
                scenes.add(fell);
                Scene burned = new Scene("death_fire").ticks(60).drops().deathKind(2).fire(200);
                ground(burned, 6);
                burned.cow(0.5, 64.0, 0.5, 0.0F, 53L);
                burned.player(4.5, 64.0, 0.5, Items.AIR);
                burned.hurts.add(new double[] {3, 0, 20.0, 0});
                scenes.add(burned);
                Scene looting = new Scene("death_looting").ticks(60).drops();
                ground(looting, 6);
                looting.cow(0.5, 64.0, 0.5, 0.0F, 54L);
                looting.player(2.5, 64.0, 0.5, Items.DIAMOND_SWORD, 3);
                looting.hurts.add(new double[] {2, 0, 20.0, 0});
                scenes.add(looting);
            }
            {
                // 喂食、繁殖、成长、挤奶。逐刻比 `inLove`、`Age`、新生成的幼崽 / 经验球，
                // 以及玩家手里交回来的物品。
                Scene feed = new Scene("interact_feed").ticks(80).drops();
                ground(feed, 6);
                feed.cow(0.5, 64.0, 0.5, 0.0F, 61L);
                feed.player(1.5, 64.0, 0.5, Items.AIR);
                feed.interact(2, 0, 0, BuiltInRegistries.ITEM.getId(Items.WHEAT), 0);
                scenes.add(feed);
                Scene baby = new Scene("interact_baby").ticks(80).drops();
                ground(baby, 6);
                baby.cow(0.5, 64.0, 0.5, 0.0F, 62L, -24000);
                baby.player(1.5, 64.0, 0.5, Items.AIR);
                baby.interact(2, 0, 0, BuiltInRegistries.ITEM.getId(Items.WHEAT), 0);
                scenes.add(baby);
                Scene breed = new Scene("interact_breed").ticks(160).drops();
                ground(breed, 6);
                breed.cow(0.5, 64.0, 0.5, 0.0F, 63L);
                breed.cow(1.1, 64.0, 0.5, 0.0F, 64L);
                breed.player(3.5, 64.0, 0.5, Items.AIR);
                breed.interact(2, 0, 0, BuiltInRegistries.ITEM.getId(Items.WHEAT), 0);
                breed.interact(2, 1, 0, BuiltInRegistries.ITEM.getId(Items.WHEAT), 0);
                scenes.add(breed);
                Scene milk = new Scene("interact_milk").ticks(40).drops().packets();
                ground(milk, 6);
                milk.cow(0.5, 64.0, 0.5, 0.0F, 65L);
                milk.player(1.5, 64.0, 0.5, Items.AIR);
                milk.interact(2, 0, 0, BuiltInRegistries.ITEM.getId(Items.BUCKET), 0);
                scenes.add(milk);
                Scene creativeMilk = new Scene("interact_milk_creative").ticks(40).drops();
                ground(creativeMilk, 6);
                creativeMilk.cow(0.5, 64.0, 0.5, 0.0F, 66L);
                creativeMilk.player(1.5, 64.0, 0.5, Items.AIR);
                creativeMilk.interact(2, 0, 0, BuiltInRegistries.ITEM.getId(Items.BUCKET), 1);
                scenes.add(creativeMilk);
            }
            // ---- 玩家受伤：`player_hurt_*` ----
            // 四档难度 × 无甲 / 铁甲 / 保护 IV，每个场景三次伤害看冷却和击退。
            // 总伤害有意低于 20，血量归零的死亡 / 重生流程留到 1b。
            for (int difficulty = 0; difficulty < 4; difficulty++) {
                for (String armor : new String[] {"none", "iron", "protection"}) {
                    Scene hurt = new Scene("player_hurt_" + difficulty + "_" + armor)
                            .ticks(30).difficulty(difficulty).playerPackets().packets();
                    ground(hurt, 4);
                    hurt.player(0.5, 64.0, 0.5, Items.AIR);
                    if (!armor.equals("none")) {
                        int[] items = {BuiltInRegistries.ITEM.getId(Items.IRON_BOOTS),
                                BuiltInRegistries.ITEM.getId(Items.IRON_LEGGINGS),
                                BuiltInRegistries.ITEM.getId(Items.IRON_CHESTPLATE),
                                BuiltInRegistries.ITEM.getId(Items.IRON_HELMET),
                                BuiltInRegistries.ITEM.getId(Items.AIR)};
                        int[] protection = armor.equals("protection")
                                ? new int[] {4, 4, 4, 4} : new int[] {0, 0, 0, 0};
                        hurt.equipment(items, protection);
                    } else {
                        hurt.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                    }
                    hurt.hurt(1, "minecraft:mob_attack", 3.0F, new double[] {3.0, 64.0, 0.5});
                    hurt.hurt(2, "minecraft:mob_attack", 2.0F, new double[] {3.0, 64.0, 0.5});
                    hurt.hurt(15, "minecraft:mob_attack", 4.0F, new double[] {3.0, 64.0, 0.5});
                    scenes.add(hurt);
                }
            }
            // `/damage` 的每种伤害类型各一个场景，单独打一次，避免冷却互相挡住。
            for (String kind : new String[] {"generic", "player_attack", "mob_attack",
                    "mob_attack_no_aggro", "magic", "on_fire", "in_fire", "lava", "hot_floor",
                    "cactus", "fall", "freeze", "starve", "arrow", "thorns", "explosion",
                    "cramming", "drown", "in_wall", "sweet_berry_bush", "out_of_world"}) {
                Scene one = new Scene("player_hurt_type_" + kind).ticks(20)
                        .difficulty(2).playerPackets().packets();
                ground(one, 4);
                one.player(0.5, 64.0, 0.5, Items.AIR);
                one.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                one.hurt(1, "minecraft:" + kind, 5.0F, null);
                scenes.add(one);
            }
            {
                Scene absorption = new Scene("player_hurt_absorption").ticks(30)
                        .playerHealth(20.0F, 6.0F, 2.5F).playerPackets().packets();
                ground(absorption, 4);
                absorption.player(0.5, 64.0, 0.5, Items.AIR);
                absorption.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                absorption.hurt(1, "minecraft:mob_attack", 3.0F, null);
                absorption.hurt(2, "minecraft:mob_attack", 5.0F, null);
                scenes.add(absorption);
            }
            {
                Scene knockback = new Scene("player_hurt_knockback").ticks(30)
                        .playerPackets().packets();
                ground(knockback, 4);
                knockback.player(0.5, 64.0, 0.5, Items.AIR);
                knockback.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                knockback.hurt(1, "minecraft:player_attack", 3.0F, new double[] {2.5, 64.0, 0.5});
                scenes.add(knockback);
            }
            {
                Scene cow = new Scene("player_hurt_attack_cow").ticks(30)
                        .playerPackets().packets();
                ground(cow, 4);
                cow.player(0.5, 64.0, 0.5, Items.DIAMOND_SWORD);
                cow.cow(2.5, 64.0, 0.5, 0.0F, 91L);
                cow.equipment(new int[] {0, 0, 0, 0,
                        BuiltInRegistries.ITEM.getId(Items.DIAMOND_SWORD)}, new int[] {0, 0, 0, 0});
                cow.attack(1, 0);
                scenes.add(cow);
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
        // 每个场景从干净的战利品随机序列开始，Bamboo 一侧每个 case 也是新建一份。
        // Each scene starts with a fresh loot sequence, like a fresh Bamboo case.
        official.server.sequences = new RandomSequences();
        FakeLevel level = FakeLevel.create(official);
        if (events != null) {
            level.chunkSource.events = events::add;
            level.packets = events::add;
        }
        level.blocks.putAll(scene.blocks);
        level.dropUuids = scene.drops;
        level.nextId = FIRST_ID;
        ((TickingLevelData) level.levelData).difficulty = Difficulty.values()[scene.difficulty];
        List<ServerPlayer> players = new ArrayList<>();
        /** 玩家受伤场景：逐刻状态和这名玩家自己收到的包。 */
        List<List<String>> playerPackets = scene.playerPackets ? new ArrayList<>() : null;
        List<List<String>> playerSteps = scene.playerPackets ? new ArrayList<>() : null;
        List<Bytes> playerSinks = scene.playerPackets ? new ArrayList<>() : null;
        if (scene.playerPackets) {
            for (int tick = 0; tick <= scene.ticks; tick++) {
                playerPackets.add(new ArrayList<>());
                playerSteps.add(new ArrayList<>());
            }
        }
        for (int i = 0; i < scene.players.size(); i++) {
            double[] at = scene.players.get(i);
            GameProfile profile = new GameProfile(new UUID(1L, 2L + i), "Oracle" + i);
            ServerPlayer player = new ServerPlayer(official.server, level, profile, ClientInformation.createDefault());
            player.setPos(at[0], at[1], at[2]);
            // 官方 `ServerPlayer` 的实体随机源不带种子；对拍里按玩家 ID 固定，
            // 朝向也固定成 0，让击退的兜底随机数可比。
            setRandom(player, 0x0DDB1A5E5BAD5EEDL + i);
            // 官方构造函数用实体随机源给朝向取随机数；对拍里固定成 0，
            // 这样 `indicateDamage` 的 yaw 只由伤害方向决定。
            player.setYRot(0.0F);
            player.setYHeadRot(0.0F);
            player.setYBodyRot(0.0F);
            ItemStack held = new ItemStack(scene.playerItems.get(i));
            int looting = scene.lootingLevels.get(i);
            if (looting > 0) {
                var enchantments = official.registries.lookupOrThrow(Registries.ENCHANTMENT);
                held.enchant(enchantments.getOrThrow(net.minecraft.world.item.enchantment.Enchantments.LOOTING), looting);
            }
            player.setItemInHand(InteractionHand.MAIN_HAND, held);
            if (i < scene.playerEquipment.size()) {
                int[] items = scene.playerEquipment.get(i);
                int[] protection = scene.playerEnchants.get(i);
                var enchantments = official.registries.lookupOrThrow(Registries.ENCHANTMENT);
                var protectionHolder = enchantments.getOrThrow(
                        net.minecraft.world.item.enchantment.Enchantments.PROTECTION);
                net.minecraft.world.entity.EquipmentSlot[] armorSlots = {
                        net.minecraft.world.entity.EquipmentSlot.FEET,
                        net.minecraft.world.entity.EquipmentSlot.LEGS,
                        net.minecraft.world.entity.EquipmentSlot.CHEST,
                        net.minecraft.world.entity.EquipmentSlot.HEAD};
                for (int slot = 0; slot < 4; slot++) {
                    ItemStack armor = new ItemStack(Item.byId(items[slot]));
                    if (protection[slot] > 0) {
                        armor.enchant(protectionHolder, protection[slot]);
                    }
                    player.setItemSlot(armorSlots[slot], armor);
                }
                if (items[4] != 0) {
                    player.setItemInHand(InteractionHand.MAIN_HAND, new ItemStack(Item.byId(items[4])));
                }
            }
            if (scene.playerPackets) {
                // 只发给玩家自己的包（`set_health` / `hurt_animation`）走这里。
                // 观察者那份走 `ChunkMap.sendToTrackingPlayersAndSelf`，和牛的同步包同一条路。
                Bytes sink = new Bytes(official.registries);
                playerSinks.add(sink);
                ServerGamePacketListenerImpl connection = allocate(RecordingListener.class);
                put(ServerGamePacketListenerImpl.class, connection, "player", player);
                put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                        connection, "server", official.server);
                put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                        connection, "connection", new Connection(PacketFlow.CLIENTBOUND));
                ((RecordingListener) connection).bytes = sink;
                put(ServerPlayer.class, player, "connection", connection);
            }
            // 官方在 `Player.tick` 里从装备刷新属性；这里直接调那一段的等价入口。
            // `setItemSlot` 之后 armor 属性还没同步，必须刷新一次减伤才生效。
            // 官方在 `LivingEntity.baseTick` 里 `detectEquipmentUpdates()` 才把
            // 装备的 `attribute_modifiers` 和 location-dependent 附魔刷进属性表。
            java.lang.reflect.Method detect = LivingEntity.class.getDeclaredMethod("detectEquipmentUpdates");
            detect.setAccessible(true);
            detect.invoke(player);
            player.setHealth(scene.playerHealth);
            // `Attributes.MAX_ABSORPTION` 的基础值是 0，`setAbsorptionAmount`
            // 会把它夹成 0。对拍里把上限设成 20，让吸收血量的分支真的跑到。
            var maxAbsorption = player.getAttribute(
                    net.minecraft.world.entity.ai.attributes.Attributes.MAX_ABSORPTION);
            if (maxAbsorption != null) {
                maxAbsorption.setBaseValue(20.0);
            }
            player.setAbsorptionAmount(scene.playerAbsorption);
            if (scene.playerExhaustion > 0.0F) {
                player.getFoodData().addExhaustion(scene.playerExhaustion);
            }
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
        /** 逐刻新生成的实体（掉落物、经验球、幼崽），按生成顺序。 */
        List<List<String>> spawns = scene.drops ? new ArrayList<>() : null;
        /** 逐刻右键交互的结果与交回来的主手物品。 */
        List<List<String>> interactions = scene.interactions.isEmpty() ? null : new ArrayList<>();
        if (scene.packets) {
            packets = new ArrayList<>();
            syncFlags = new ArrayList<>();
            syncs = new ArrayList<>();
            for (int tick = 0; tick <= scene.ticks; tick++) {
                packets.add(new ArrayList<>());
                syncFlags.add(new ArrayList<>());
            }
        }
        if (scene.drops) {
            for (int tick = 0; tick <= scene.ticks; tick++) {
                spawns.add(new ArrayList<>());
            }
        }
        if (interactions != null) {
            for (int tick = 0; tick <= scene.ticks; tick++) {
                interactions.add(new ArrayList<>());
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
            if (scene.fireTicks > 0) {
                cow.setRemainingFireTicks(scene.fireTicks);
            }
            int age = scene.cowAges.get(i);
            if (age != 0) {
                cow.setAge(age);
            }
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
            line.append(",\"seed\":").append(scene.seeds.get(i));
            line.append(",\"age\":").append(scene.cowAges.get(i)).append('}');
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
            line.append(",\"item\":").append(BuiltInRegistries.ITEM.getId(scene.playerItems.get(i)));
            line.append(",\"looting\":").append(scene.lootingLevels.get(i)).append('}');
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
        if (scene.playerPackets && !players.isEmpty()) {
            playerSteps.get(0).add(playerStepJson(players.get(0)));
            // 官方 `placeNewPlayer` 之后 `ServerPlayer.doTick` 的第一次调用就补
            // `set_health`，这里把那一包记在 tick 0。
            ServerPlayer target = players.get(0);
            float health = target.getHealth();
            int food = target.getFoodData().getFoodLevel();
            float saturation = target.getFoodData().getSaturationLevel();
            playerSinks.get(0).add(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(
                    health, food, saturation));
            playerPackets.get(0).addAll(playerSinks.get(0).take());
        }
        /** 已经发过 `remove_entities` 的牛，按 `cows` 下标。 */
        java.util.Set<Integer> removed = new java.util.HashSet<>();
        // `ServerPlayer` 构造时的三个哨兵值，进服第一刻会发一包 `set_health`。
        float lastSentHealth = players.isEmpty() ? -1.0e8f : players.get(0).getHealth();
        int lastSentFood = players.isEmpty() ? -99999999 : players.get(0).getFoodData().getFoodLevel();
        boolean lastSaturationZero = players.isEmpty()
                || players.get(0).getFoodData().getSaturationLevel() == 0.0F;
        for (int tick = 1; tick <= scene.ticks; tick++) {
            level.gameTime++;
            for (int[] action : scene.interactions) {
                if (action[0] != tick) {
                    continue;
                }
                Cow target = cows.get(action[1]);
                ServerPlayer actor = players.get(action[2]);
                // 只改 `Abilities.instabuild`：`setGameMode` 会给不存在的连接发包。
                putBoolean(net.minecraft.world.entity.player.Abilities.class, actor.getAbilities(),
                        "instabuild", action[4] != 0);
                actor.setItemInHand(InteractionHand.MAIN_HAND,
                        new ItemStack(net.minecraft.world.item.Item.byId(action[3])));
                // 官方 `Player.interactOn` 是这条链的入口；牛只看手里这一堆。
                net.minecraft.world.InteractionResult result = target.mobInteract(actor, InteractionHand.MAIN_HAND);
                ItemStack held = actor.getItemInHand(InteractionHand.MAIN_HAND);
                StringBuilder one = new StringBuilder("{\"result\":\"");
                one.append(result.getClass().getSimpleName()).append('"');
                one.append(",\"item\":").append(BuiltInRegistries.ITEM.getId(held.getItem()));
                one.append(",\"count\":").append(held.getCount()).append('}');
                interactions.get(tick).add(one.toString());
                putBoolean(net.minecraft.world.entity.player.Abilities.class, actor.getAbilities(),
                        "instabuild", false);
            }
            for (double[] hurt : scene.hurts) {
                if ((int) hurt[0] == tick) {
                    Cow target = cows.get((int) hurt[1]);
                    ServerPlayer attacker = players.get((int) hurt[3]);
                    net.minecraft.world.damagesource.DamageSource source = switch (scene.deathKind) {
                        case 1 -> level.damageSources().fall();
                        case 2 -> level.damageSources().onFire();
                        default -> level.damageSources().playerAttack(attacker);
                    };
                    target.hurtServer(level, source, (float) hurt[2]);
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
            if (scene.playerPackets) {
                for (PlayerAction action : scene.playerActions) {
                    if (action.tick != tick) {
                        continue;
                    }
                    if (action.action == 0) {
                        var damageTypes = official.registries.lookupOrThrow(Registries.DAMAGE_TYPE);
                        var holder = damageTypes.getOrThrow(net.minecraft.resources.ResourceKey.create(
                                Registries.DAMAGE_TYPE, Identifier.parse(action.kind)));
                        // `when_caused_by_living_non_player` 要一个非玩家生物当造成者。
                        // 这名攻击者不放进实体表，只作伤害来源。
                        Cow attacker = EntityTypes.COW.create(level, EntitySpawnReason.COMMAND);
                        attacker.setId(1 << 24);
                        attacker.setPos(action.at == null ? 3.0 : action.at[0], 64.0,
                                action.at == null ? 0.5 : action.at[2]);
                        net.minecraft.world.damagesource.DamageSource source;
                        if (action.at == null) {
                            source = new net.minecraft.world.damagesource.DamageSource(holder, attacker);
                        } else {
                            var ctor = net.minecraft.world.damagesource.DamageSource.class
                                    .getDeclaredConstructor(net.minecraft.core.Holder.class, Entity.class,
                                            Entity.class, Vec3.class);
                            ctor.setAccessible(true);
                            source = ctor.newInstance(holder, attacker, attacker,
                                    new Vec3(action.at[0], action.at[1], action.at[2]));
                        }
                        players.get(0).hurtServer(level, source, action.amount);
                    } else {
                        // 官方 `ServerGamePacketListenerImpl.handleAttack` 的伤害入口。
                        Cow target = cows.get(action.cow);
                        players.get(0).attack(target);
                    }
                }
                // 只推进 1a 建模的三处：`ServerPlayer.tick` 递减 damageCooldownTime，
                // `LivingEntity.baseTick` 递减 hurtTime，以及 `ServerPlayer.doTick`
                // 里 `lastSentHealth` 变了就补 `set_health`。不跑 FoodData.tick /
                // tickRegeneration / 物理，那些留给 1b。
                ServerPlayer target = players.get(0);
                if (target.damageCooldownTime > 0) {
                    target.damageCooldownTime--;
                }
                if (target.hurtTime > 0) {
                    target.hurtTime--;
                }
                float health = target.getHealth();
                int food = target.getFoodData().getFoodLevel();
                float saturation = target.getFoodData().getSaturationLevel();
                if (Float.floatToRawIntBits(health) != Float.floatToRawIntBits(lastSentHealth)
                        || food != lastSentFood
                        || (saturation == 0.0F) != lastSaturationZero) {
                    lastSentHealth = health;
                    lastSentFood = food;
                    lastSaturationZero = saturation == 0.0F;
                    playerSinks.get(0).add(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(
                            health, food, saturation));
                }
                playerSteps.get(tick).add(playerStepJson(target));
                playerPackets.get(tick).addAll(playerSinks.get(0).take());
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
            // 实体 tick 里也会生成实体（繁殖的幼崽和经验球），在这一刻收齐。
            // Entity ticks also spawn things (calves, orbs); collect them after the ticks.
            if (scene.drops) {
                // 官方先构造幼崽再构造经验球，所以按实体 ID 排序后就是构造顺序；
                // Bamboo 也按这个顺序分配 ID 和种子。
                level.spawned.sort(java.util.Comparator.comparingInt(Entity::getId));
                for (Entity born : level.spawned) {
                    level.reseedBorn(born);
                    spawns.get(tick).add(spawnJson(born));
                    if (born instanceof Cow) {
                        cows.add((Cow) born);
                        level.tracked.add(born);
                    }
                    if (scene.packets) {
                        Sync sync = new Sync(level, born, official.registries);
                        syncs.add(sync);
                        sync.entity.sendPairingData(null, packet -> sync.bytes.add(packet));
                        packets.get(tick).addAll(sync.take());
                    }
                }
                level.spawned.clear();
                level.newbornCows.clear();
            }
            level.tracked.removeIf(Entity::isRemoved);
            line.append(',').append(stepJson(cows));
            if (scene.packets) {
                // 官方 `ChunkMap.removeEntity` 在实体被丢弃时发给所有观察者。
                // Vanilla's `ChunkMap.removeEntity` tells every viewer when an entity is discarded.
                for (int i = 0; i < cows.size(); i++) {
                    if (cows.get(i).isRemoved() && removed.add(i)) {
                        events.add(new net.minecraft.network.protocol.game.ClientboundRemoveEntitiesPacket(
                                cows.get(i).getId()));
                    }
                }
            }
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
        line.append(",\"spawns\":");
        appendSpawns(line, spawns);
        line.append(",\"deathKind\":").append(scene.deathKind);
        line.append(",\"fireTicks\":").append(scene.fireTicks);
        line.append(",\"interactActions\":[");
        for (int i = 0; i < scene.interactions.size(); i++) {
            if (i > 0) line.append(',');
            int[] action = scene.interactions.get(i);
            line.append('[').append(action[0]).append(',').append(action[1]).append(',')
                    .append(action[2]).append(',').append(action[3]).append(',')
                    .append(action[4]).append(']');
        }
        line.append("],\"interactions\":");
        appendSpawns(line, interactions);
        line.append(",\"difficulty\":").append(scene.difficulty);
        line.append(",\"playerHealth\":").append(Float.floatToRawIntBits(scene.playerHealth));
        line.append(",\"playerAbsorption\":").append(Float.floatToRawIntBits(scene.playerAbsorption));
        line.append(",\"playerExhaustion\":").append(Float.floatToRawIntBits(scene.playerExhaustion));
        line.append(",\"playerEquipment\":[");
        for (int i = 0; i < scene.playerEquipment.size(); i++) {
            if (i > 0) line.append(',');
            line.append('[');
            int[] items = scene.playerEquipment.get(i);
            for (int j = 0; j < items.length; j++) {
                if (j > 0) line.append(',');
                line.append(items[j]);
            }
            line.append(']');
        }
        line.append("],\"playerProtection\":[");
        for (int i = 0; i < scene.playerEnchants.size(); i++) {
            if (i > 0) line.append(',');
            line.append('[');
            int[] levels = scene.playerEnchants.get(i);
            for (int j = 0; j < levels.length; j++) {
                if (j > 0) line.append(',');
                line.append(levels[j]);
            }
            line.append(']');
        }
        line.append("],\"playerActions\":[");
        for (int i = 0; i < scene.playerActions.size(); i++) {
            if (i > 0) line.append(',');
            PlayerAction action = scene.playerActions.get(i);
            line.append("{\"tick\":").append(action.tick).append(",\"action\":").append(action.action);
            line.append(",\"kind\":").append(action.kind == null ? "null" : jsonString(action.kind));
            line.append(",\"amount\":").append(Float.floatToRawIntBits(action.amount));
            line.append(",\"at\":").append(action.at == null ? "null"
                    : ("[" + Double.doubleToRawLongBits(action.at[0]) + ","
                            + Double.doubleToRawLongBits(action.at[1]) + ","
                            + Double.doubleToRawLongBits(action.at[2]) + "]"));
            line.append(",\"cow\":").append(action.cow).append('}');
        }
        line.append("],\"playerSteps\":");
        appendPackets(line, playerSteps);
        line.append(",\"playerPackets\":");
        appendPackets(line, playerPackets);
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

    /** 逐刻的新实体，条目本身就是 JSON 对象，不再加引号。 */
    private static void appendSpawns(StringBuilder line, List<List<String>> spawns) {
        if (spawns == null) {
            line.append("null");
            return;
        }
        line.append('[');
        for (int tick = 0; tick < spawns.size(); tick++) {
            if (tick > 0) line.append(',');
            line.append('[');
            List<String> step = spawns.get(tick);
            for (int i = 0; i < step.size(); i++) {
                if (i > 0) line.append(',');
                line.append(step.get(i));
            }
            line.append(']');
        }
        line.append(']');
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

    /** 一个新生成的实体：种类、物品或经验值、位置。速度用实体自己的随机源，不比。 */
    private static String spawnJson(Entity entity) throws Exception {
        StringBuilder out = new StringBuilder("{\"id\":").append(entity.getId());
        Vec3 pos = entity.position();
        out.append(",\"x\":").append(Double.doubleToRawLongBits(pos.x));
        out.append(",\"y\":").append(Double.doubleToRawLongBits(pos.y));
        out.append(",\"z\":").append(Double.doubleToRawLongBits(pos.z));
        if (entity instanceof net.minecraft.world.entity.item.ItemEntity item) {
            out.append(",\"kind\":\"item\",\"item\":")
                    .append(BuiltInRegistries.ITEM.getId(item.getItem().getItem()));
            out.append(",\"count\":").append(item.getItem().getCount());
        } else if (entity instanceof net.minecraft.world.entity.ExperienceOrb orb) {
            out.append(",\"kind\":\"orb\",\"value\":").append(orb.getValue());
        } else if (entity instanceof Cow calf) {
            // 幼崽的构造随机数和朝向在生成时定下，作为场景起点交给 Bamboo。
            out.append(",\"kind\":\"cow\",\"value\":").append(calf.getAge());
            out.append(",\"seed\":").append(randomSeed(calf));
            out.append(",\"hr\":").append(Float.floatToRawIntBits(calf.getYHeadRot()));
            out.append(",\"uuid\":\"").append(calf.getUUID()).append('"');
        } else {
            out.append(",\"kind\":\"other\"");
        }
        return out.append('}').toString();
    }

    private static String jsonString(String text) {
        StringBuilder out = new StringBuilder("\"");
        for (char c : text.toCharArray()) {
            if (c == '"' || c == '\\') out.append('\\');
            out.append(c);
        }
        return out.append('"').toString();
    }

    /** 玩家受伤场景的逐刻状态：血量、吸收、冷却、饥饿、速度和随机源。 */
    private static String playerStepJson(ServerPlayer player) throws Exception {
        StringBuilder out = new StringBuilder(256);
        out.append("{\"hp\":").append(Float.floatToRawIntBits(player.getHealth()));
        out.append(",\"abs\":").append(Float.floatToRawIntBits(player.getAbsorptionAmount()));
        out.append(",\"ht\":").append(player.hurtTime);
        out.append(",\"hd\":").append(player.hurtDuration);
        out.append(",\"cd\":").append(player.damageCooldownTime);
        out.append(",\"lh\":").append(Float.floatToRawIntBits(
                getFloat(LivingEntity.class, player, "lastHurt")));
        out.append(",\"food\":").append(player.getFoodData().getFoodLevel());
        out.append(",\"sat\":").append(Float.floatToRawIntBits(player.getFoodData().getSaturationLevel()));
        out.append(",\"exh\":").append(Float.floatToRawIntBits(
                getFloat(net.minecraft.world.food.FoodData.class, player.getFoodData(), "exhaustionLevel")));
        out.append(",\"dx\":").append(Double.doubleToRawLongBits(player.getDeltaMovement().x));
        out.append(",\"dy\":").append(Double.doubleToRawLongBits(player.getDeltaMovement().y));
        out.append(",\"dz\":").append(Double.doubleToRawLongBits(player.getDeltaMovement().z));
        out.append(",\"seed\":").append(randomSeed(player));
        out.append(",\"yr\":").append(Float.floatToRawIntBits(player.getYRot()));
        return out.append('}').toString();
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
            out.append(",\"love\":").append(cow.getInLoveTime());
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

    private static float getFloat(Class<?> owner, Object target, String name) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            return field.getFloat(target);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static void putBoolean(Class<?> owner, Object target, String name, boolean value) {
        try {
            Field field = owner.getDeclaredField(name);
            UNSAFE.putBoolean(target, UNSAFE.objectFieldOffset(field), value);
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

    /** 只记 `connection.send` 的包：玩家自己收到的 `set_health` / `hurt_animation`。 */
    private static final class RecordingListener extends ServerGamePacketListenerImpl {
        private RecordingListener() {
            super(null, null, null, null);
        }

        Bytes bytes;

        @Override
        public void send(net.minecraft.network.protocol.Packet<?> packet) {
            @SuppressWarnings("unchecked")
            net.minecraft.network.protocol.Packet<? super net.minecraft.network.protocol.game.ClientGamePacketListener> cast =
                    (net.minecraft.network.protocol.Packet<? super net.minecraft.network.protocol.game.ClientGamePacketListener>) packet;
            bytes.add(cast);
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
        /** 战利品表的随机序列，世界种子和 Bamboo 的缺省一样是 0。 */
        private RandomSequences sequences = new RandomSequences();

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        static FakeServer create(ReloadableServerResources managers, GameRules rules) {
            FakeServer server = allocate(FakeServer.class);
            server.managers = managers;
            server.rules = rules;
            server.sequences = new RandomSequences();
            return server;
        }

        @Override
        public net.minecraft.world.item.crafting.RecipeManager getRecipeManager() {
            return managers.getRecipeManager();
        }

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            // 死亡场景要跑真的 `minecraft:entities/cow` 战利品表。
            // Death scenes need the real `minecraft:entities/cow` loot table.
            return managers.fullRegistries();
        }

        @Override
        public RandomSource getRandomSequence(Identifier key) {
            return sequences.get(key, 0L);
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
        private Difficulty difficulty = Difficulty.NORMAL;

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
            return difficulty;
        }

        @Override
        public boolean isDifficultyLocked() {
            return false;
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
            level.spawned = new ArrayList<>();
            level.newbornCows = new ArrayList<>();
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
            if (entity instanceof Cow calf && dropUuids) {
                newbornCows.add(calf);
            }
            spawned.add(entity);
            return true;
        }

        /** 按幼崽 / 掉落物的构造顺序给新实体换上岛的种子源，并重放构造取数。 */
        void reseedBorn(Entity entity) {
            if (!dropUuids) {
                return;
            }
            if (entity instanceof Cow calf) {
                long seed = entitySeeds.nextLong();
                BORN_SEEDS.put(calf.getId(), seed);
                setRandom(calf, seed);
                RandomSource random = calf.getRandom();
                calf.setUUID(Mth.createInsecureUUID(random));
                random.nextFloat();
                return;
            }
            if (entity instanceof net.minecraft.world.entity.item.ItemEntity
                    || entity instanceof net.minecraft.world.entity.ExperienceOrb) {
                entity.setUUID(new UUID(0x0000BAAB00000000L, entity.getId()));
                long seed = entitySeeds.nextLong();
                BORN_SEEDS.put(entity.getId(), seed);
                setRandom(entity, seed);
                RandomSource random = entity.getRandom();
                if (entity instanceof net.minecraft.world.entity.item.ItemEntity) {
                    random.nextFloat();
                    entity.setYRot(random.nextFloat() * 360.0F);
                    double dx = random.nextDouble() * 0.2 - 0.1;
                    double dz = random.nextDouble() * 0.2 - 0.1;
                    entity.setDeltaMovement(dx, 0.2, dz);
                } else {
                    entity.setYRot(random.nextFloat() * 360.0F);
                    double dx = (random.nextDouble() * 0.2 - 0.1) * 2.0;
                    double dy = random.nextDouble() * 0.2 * 2.0;
                    double dz = (random.nextDouble() * 0.2 - 0.1) * 2.0;
                    entity.setDeltaMovement(dx, dy, dz);
                }
            }
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
        /** 这一刻由 `addFreshEntity` 生成的实体，死亡对拍比它们的种类、位置和速度。 */
        List<Entity> spawned;
        /** 死亡场景给掉落物和球用确定性 UUID。 */
        boolean dropUuids;
        /** 交互/繁殖场景新生成的幼崽，由 `play` 收进牛名单继续 tick。 */
        List<Cow> newbornCows;

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
            return levelData != null ? ((TickingLevelData) levelData).difficulty : Difficulty.NORMAL;
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
