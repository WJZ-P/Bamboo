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
import net.minecraft.world.DifficultyInstance;
import net.minecraft.world.TickRateManager;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.game.ClientboundPlayerCombatKillPacket;
import net.minecraft.network.protocol.game.ClientboundSystemChatPacket;
import net.minecraft.network.protocol.game.ClientboundRespawnPacket;
import net.minecraft.network.protocol.game.ClientboundSetDefaultSpawnPositionPacket;
import net.minecraft.network.protocol.game.ClientboundChangeDifficultyPacket;
import net.minecraft.network.protocol.game.ClientboundSetExperiencePacket;
import net.minecraft.network.protocol.game.ClientboundEntityEventPacket;
import net.minecraft.world.food.FoodData;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.item.enchantment.Enchantments;
import net.minecraft.core.GlobalPos;
import net.minecraft.world.level.Level;
import net.minecraft.resources.ResourceKey;
import net.minecraft.world.entity.Entity.RemovalReason;
import net.minecraft.world.level.gamerules.GameRule;
import net.minecraft.world.level.GameType;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.entity.ai.goal.WrappedGoal;
import net.minecraft.world.entity.animal.cow.Cow;
import net.minecraft.world.entity.monster.zombie.Zombie;
import net.minecraft.world.entity.monster.zombie.Husk;
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
import net.minecraft.world.level.gamerules.GameRule;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import io.netty.util.internal.ThreadLocalRandom;
import net.minecraft.server.level.PlayerSpawnFinder;
import net.minecraft.server.level.TicketType;
import java.util.concurrent.CompletableFuture;
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
            StubList stub = allocate(StubList.class);
            put(net.minecraft.server.players.PlayerList.class, stub, "players",
                    new ArrayList<ServerPlayer>());
            put(net.minecraft.server.players.PlayerList.class, stub, "playersByUUID",
                    new java.util.HashMap<>());
            put(net.minecraft.server.players.PlayerList.class, stub, "stats",
                    new java.util.HashMap<>());
            put(net.minecraft.server.players.PlayerList.class, stub, "advancements",
                    new java.util.HashMap<>());
            put(net.minecraft.server.players.PlayerList.class, stub, "server", official.server);
            official.server.setPlayerList(stub);
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
            for (String name : List.of("arrow_hits_player", "arrow_hits_wall", "arrow_block_overlaps_flight",
                    "arrow_no_gravity", "arrow_despawn", "arrow_flame_hit", "arrow_flame_rejected",
                    "arrow_piercing_hit", "arrow_piercing_rejected",
                    "arrow_water_crossing", "arrow_water_after_pierce",
                    "skeleton_bow_normal", "skeleton_bow_hard")) {
                if (only != null && !only.equals("projectile") && !name.startsWith(only)) continue;
                playProjectile(official, out, name);
                cases++;
            }
            if (only == null || "equine".startsWith(only)) {
                cases += playEquines(official, out);
            }
            if ("skeleton_spawn".equals(only)
                    || (only == null && !net.minecraft.util.SpecialDates.isHalloween())) {
                cases += playSkeletonSpawns(official, out, false);
            }
            if ("skeleton_halloween".equals(only)) {
                cases += playHalloweenSpawns(official, out);
            }
            if (only == null || "arrow_pickup".equals(only)) {
                cases += playArrowPickups(official, out);
            }
            if (only == null || "skeleton_death".startsWith(only)) {
                cases += playSkeletonDeaths(official, out);
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
        /** 1b 玩家生命场景：死亡 / 重生 / 饥饿与回血。 */
        boolean playerLife;
        /** 1c 落脚点场景：官方 `PlayerSpawnFinder.findSpawn` / 个人出生点。 */
        boolean spawnScene;
        /** S2 玩家效果场景：`playEffects` 用独立的官方入口链。 */
        boolean effectScene;
        /** S3 环境伤害场景：`playEnv` 用官方 `Entity.baseTick` / `hurtServer`。 */
        boolean envScene;
        /** 环境场景：{起烟火刻数, 起始氧气, 起始冻结, 摔落距离, 难度}。 */
        int[] envSetup = new int[5];
        /** 场景里每格方块的种类：0 空 / 1 火 / 2 岩浆 / 3 水 / 4 仙人掌 /
         *  5 甜浆果 / 6 岩浆块 / 7 细雪 / 8 石头（卡墙）/ 9 气泡柱。 */
        int envBlock;
        /** S4 吃东西场景：`playConsume` 用官方 `startUsingItem` /
         * `updateUsingItem` / `finishUsingItem`。主手物品 ID。 */
        boolean consumeScene;
        int consumeItem;
        /** 效果动作：{刻, 0=add 1=clear-one 2=clear-all, 效果 ID, 时长, 等级}。 */
        final List<int[]> effectActions = new ArrayList<>();
        /** 牛的效果动作：{刻, 牛下标, 0=add 1=clear-all, 效果 ID, 时长, 等级}。
         * 逐刻比位置 / 速度 / 血量 / 摔落距离 / 随机源。 */
        final List<int[]> cowEffectActions = new ArrayList<>();
        /** 落脚点场景：出生点、规则半径、随机源种子、默认游戏模式、个人出生点。 */
        int[] spawnAt = {8, 0, 8};
        int spawnRadius = 10;
        long spawnSeed;
        int spawnGameType; // 0 creative, 2 adventure
        int[] personalAt;  // null = 没有个人点；三元素坐标
        /** 死亡场景的规则：keepInventory / immediateRespawn / showDeathMessages / naturalRegeneration。 */
        boolean keepInventory;
        boolean immediateRespawn;
        boolean showDeathMessages = true;
        boolean naturalRegeneration = true;
        /** 死亡场景的起始等级、背包物品（含消失诅咒）。 */
        int xpLevel;
        int[] inventoryItems = new int[0];
        int[] vanishingSlots = new int[0];
        /** 死亡场景的伤害：`kind`、`amount`、攻击者下标（-1 无）、死亡后重生刻。 */
        String lifeDamageKind = "minecraft:generic_kill";
        float deathAmount = Float.MAX_VALUE;
        int deathAttacker = -1;
        int deathTick = 1;
        int respawnTick = -1;
        /** 只跑回血、不注入死亡伤害。 */
        boolean noDeath;
        /** 击杀归属场景：先在某刻用一头牛打 1 点，再在 `deathTick` /kill。 */
        int creditHurtTick = -1;
        /** 致死伤害那一刻注入的 `LivingEntity.fallDistance`。 */
        float deathFallDistance;
        /** 回血场景的起始 hp / food / saturation / exhaustion / tickTimer。 */
        float startHealth = 20.0F;
        float startSaturation = 5.0F;
        float startExhaustion = 0.0F;
        int startFood = 20;
        int startFoodTimer;
        /** 僵尸场景：实体类型 (0 牛 / 1 僵尸 / 2 尸壳) 和每只的注册顺序。 */
        final List<int[]> mobKinds = new ArrayList<>();
        /** 天色、雨、白天/夜晚与 MONSTERS_BURN 的场景注入。 */
        int skyDarken;
        boolean raining;
        boolean brightOutside = true;
        boolean monstersBurn = true;
        /** 僵尸场景：玩家走真实的受伤链路，逐刻比玩家状态和玩家自己收到的包。 */
        boolean realPlayers;
        /** 僵尸场景：主世界时钟的起始刻；`>= 0` 时天色、`MONSTERS_BURN`、月相都按真实时间线逐刻算。 */
        long clockStart = -1;
        /** 真实时间模式下的雨强（`getRainLevel`），雷暴恒 0。 */
        float rain;
        /** 僵尸场景：每只生物头上戴的东西（`null` 是空）和它的初始耐久损耗。 */
        Item helmet;
        int helmetDamage;
        /** 场景里明确没有村民/铁傀儡/海龟/狼/僵尸猪灵/鸡。 */
        boolean noOtherEntities = true;

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
            return mob(0, x, y, z, yaw, seed, age);
        }

        Scene mob(int kind, double x, double y, double z, float yaw, long seed, int age) {
            cows.add(new double[] {x, y, z});
            yaws.add(new float[] {yaw});
            seeds.add(seed);
            cowAges.add(age);
            mobKinds.add(new int[] {kind});
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
        /** 某一刻给一头牛挂效果 / 清效果。 */
        Scene cowEffect(int tick, int cow, int kind, int effect, int duration, int amplifier) {
            cowEffectActions.add(new int[] {tick, cow, kind, effect, duration, amplifier});
            return this;
        }

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

        Scene life() {
            playerLife = true;
            playerPackets = true;
            packets = true;
            return this;
        }

        Scene rules(boolean keep, boolean immediate, boolean messages, boolean natural) {
            keepInventory = keep;
            immediateRespawn = immediate;
            showDeathMessages = messages;
            naturalRegeneration = natural;
            return this;
        }

        Scene xp(int level) {
            xpLevel = level;
            return this;
        }

        Scene inventory(int[] items, int[] vanishing) {
            inventoryItems = items;
            vanishingSlots = vanishing;
            return this;
        }

        Scene death(int tick, String kind, float amount, int attacker) {
            deathTick = tick;
            lifeDamageKind = kind;
            deathAmount = amount;
            deathAttacker = attacker;
            return this;
        }

        Scene creditHurt(int tick) {
            creditHurtTick = tick;
            return this;
        }

        Scene fallDistance(float distance) {
            deathFallDistance = distance;
            return this;
        }

        Scene respawnAt(int tick) {
            respawnTick = tick;
            return this;
        }

        Scene spawn(int x, int y, int z, int radius, long seed) {
            spawnScene = true;
            spawnAt = new int[] {x, y, z};
            spawnRadius = radius;
            spawnSeed = seed;
            return this;
        }

        Scene spawnGameType(int id) {
            spawnGameType = id;
            return this;
        }

        Scene personalSpawn(int x, int y, int z) {
            personalAt = new int[] {x, y, z};
            return this;
        }

        Scene regen(float hp, int food, float saturation, float exhaustion, int timer) {
            noDeath = true;
            startHealth = hp;
            startFood = food;
            startSaturation = saturation;
            startExhaustion = exhaustion;
            startFoodTimer = timer;
            return this;
        }

        /** 一名玩家的装备：靴子、护腿、胸甲、头盔、主手，以及四件盔甲的保护等级。 */
        Scene equipment(int[] items, int[] protection) {
            playerEquipment.add(items);
            playerEnchants.add(protection);
            return this;
        }

        /** S4：吃东西场景。 */
        Scene consume(int ticks, int item, int food) {
            consumeScene = true;
            playerPackets = true;
            this.ticks = ticks;
            consumeItem = item;
            startFood = food;
            return this;
        }

        /** 虚空场景把玩家放到 `minY - 64` 以下。 */
        Scene envVoid() {
            players.clear();
            playerItems.clear();
            lootingLevels.clear();
            player(0.5, -200.0, 0.5, Items.AIR);
            return this;
        }

        /** S3：环境伤害场景。 */
        Scene env(int ticks, int block, int fire, int air, int frozen, int fall, int difficulty) {
            envScene = true;
            playerPackets = true;
            this.ticks = ticks;
            envBlock = block;
            envSetup = new int[] {fire, air, frozen, fall, difficulty};
            this.difficulty = difficulty;
            return this;
        }

        /** S2：效果场景用独立的 playEffects 重放。 */
        Scene effects() {
            effectScene = true;
            playerPackets = true;
            return this;
        }

        Scene effectGive(int tick, int effect, int duration, int amplifier) {
            effectActions.add(new int[] {tick, 0, effect, duration, amplifier});
            return this;
        }

        Scene effectClear(int tick, int effect) {
            effectActions.add(new int[] {tick, 1, effect, 0, 0});
            return this;
        }

        Scene effectClearAll(int tick) {
            effectActions.add(new int[] {tick, 2, 0, 0, 0});
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
        /** 1b：把每个包按发生顺序同步记进一份共用流。 */
        Bytes mirror;

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
            if (mirror != null) {
                mirror.collected.add(body);
            }
        }

        /** 把先前取走的包按原顺序放回（1b 同刻死亡 + 重生）。 */
        void pushRaw(List<byte[]> values) {
            collected.addAll(0, values);
        }

        /** 取走这一刻的全部包，保留原始字节（1b 的包 ID 过滤用）。 */
        List<byte[]> takeRaw() {
            List<byte[]> out = new ArrayList<>(collected);
            collected.clear();
            return out;
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
                    "cramming", "drown", "in_wall", "sweet_berry_bush", "out_of_world",
                    "wither"}) {
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
            // ---- 1b：死亡、掉落、重生与回血 ----
            // keepInventory × immediateRespawn × 等级经验 × 消失诅咒。
            for (boolean keep : new boolean[] {false, true}) {
                for (boolean immediate : new boolean[] {false, true}) {
                    Scene death = new Scene("player_death_keep" + keep + "_immediate" + immediate)
                            .ticks(6).life().difficulty(2)
                            .rules(keep, immediate, true, true)
                            .xp(9)
                            .inventory(new int[] {BuiltInRegistries.ITEM.getId(Items.STONE),
                                    BuiltInRegistries.ITEM.getId(Items.IRON_SWORD)}, new int[] {1})
                            .death(1, "minecraft:generic_kill", Float.MAX_VALUE, -1);
                // immediate_respawn：客户端当场发 PERFORM_RESPAWN。
                    death.respawnAt(immediate ? 1 : 3);
                    ground(death, 4);
                    death.player(0.5, 64.0, 0.5, Items.AIR);
                    death.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                    scenes.add(death);
                }
            }
            // 死亡消息：/kill、/damage 若干类型、被牛以外的攻击者注入。
            for (String kind : new String[] {"generic_kill", "generic", "mob_attack",
                    "player_attack", "magic", "on_fire", "lava", "starve", "fall"}) {
                Scene message = new Scene("player_death_message_" + kind).ticks(4)
                        .life().difficulty(2).rules(false, true, true, true)
                        .death(1, "minecraft:" + kind, 100.0F, -1).respawnAt(1);
                // immediate_respawn：客户端当场发 PERFORM_RESPAWN。
                ground(message, 4);
                message.player(0.5, 64.0, 0.5, Items.AIR);
                message.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(message);
            }
            {
                // `by <entity>` 注入一头牛作为攻击者（非玩家）。
                Scene by = new Scene("player_death_message_by_cow").ticks(4)
                        .life().difficulty(2).rules(false, true, true, true)
                        .death(1, "minecraft:mob_attack", 100.0F, 0).respawnAt(1);
                ground(by, 4);
                by.player(0.5, 64.0, 0.5, Items.AIR);
                by.cow(2.5, 64.0, 0.5, 0.0F, 101L);
                by.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(by);
                // show_death_messages 关掉：PlayerCombatKill 的消息体为空，不广播。
                Scene hidden = new Scene("player_death_hidden_message").ticks(4)
                        .life().difficulty(2).rules(false, true, false, true)
                        .death(1, "minecraft:generic_kill", Float.MAX_VALUE, -1).respawnAt(1);
                ground(hidden, 4);
                hidden.player(0.5, 64.0, 0.5, Items.AIR);
                hidden.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(hidden);
                // 只有消失诅咒的物品：不生成掉落物。
                Scene vanishing = new Scene("player_death_vanishing_only").ticks(4)
                        .life().difficulty(2).rules(false, true, true, true)
                        .inventory(new int[] {BuiltInRegistries.ITEM.getId(Items.DIAMOND_SWORD)},
                                new int[] {0})
                        .death(1, "minecraft:generic_kill", Float.MAX_VALUE, -1).respawnAt(1);
                ground(vanishing, 4);
                vanishing.player(0.5, 64.0, 0.5, Items.AIR);
                vanishing.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(vanishing);
                // 高等级经验：min(level*7,100) 拆球。
                Scene xp = new Scene("player_death_xp_30").ticks(4)
                        .life().difficulty(2).rules(false, true, true, true).xp(30)
                        .death(1, "minecraft:generic_kill", Float.MAX_VALUE, -1).respawnAt(1);
                ground(xp, 4);
                xp.player(0.5, 64.0, 0.5, Items.AIR);
                xp.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(xp);
            }
            // 击杀归属：先被牛打 1 点再 /kill；100 刻后归属过期退回单参数。
            // 摔落致死：fallDistance > 5 走 FALL_VARIANTS，否则走 getLocalizedDeathMessage。
            {
                String[] names = {
                    "player_death_credit_recent", "player_death_credit_expired",
                    "player_death_fall_credit", "player_death_fall_no_credit",
                    "player_death_fall_short", "player_death_credit_boundary_keep",
                    "player_death_credit_boundary_gone",
                };
                int[] kills = {3, 105, 3, 3, 3, 101, 102};
                int[] credits = {1, 1, 1, -1, -1, 1, 1};
                String[] kinds = {"minecraft:generic_kill", "minecraft:generic_kill",
                        "minecraft:fall", "minecraft:fall", "minecraft:fall",
                        "minecraft:generic_kill", "minecraft:generic_kill"};
                float[] distances = {0.0F, 0.0F, 8.0F, 8.0F, 2.0F, 0.0F, 0.0F};
                for (int i = 0; i < names.length; i++) {
                    Scene scene = new Scene(names[i]).ticks(kills[i] + 2)
                            .life().difficulty(2).rules(false, true, true, true)
                            .death(kills[i], kinds[i], 100.0F, -1).respawnAt(kills[i])
                            .fallDistance(distances[i]);
                    if (credits[i] >= 0) {
                        scene.creditHurt(credits[i]);
                    }
                    ground(scene, 4);
                    scene.player(0.5, 64.0, 0.5, Items.AIR);
                    scene.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                    scenes.add(scene);
                }
            }
            // 和平回血、满饥饿快速回血、饱和度回血、饥饿掉血。
            {
                Scene peaceful = new Scene("player_regen_peaceful").ticks(45)
                        .life().difficulty(0).rules(false, false, true, true)
                        .regen(10.0F, 20, 5.0F, 0.0F, 0);
                ground(peaceful, 4);
                peaceful.player(0.5, 64.0, 0.5, Items.AIR);
                peaceful.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(peaceful);

                Scene fast = new Scene("player_regen_fast").ticks(25)
                        .life().difficulty(2).rules(false, false, true, true)
                        .regen(10.0F, 20, 4.0F, 0.0F, 0);
                ground(fast, 4);
                fast.player(0.5, 64.0, 0.5, Items.AIR);
                fast.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(fast);

                Scene saturation = new Scene("player_regen_saturation").ticks(25)
                        .life().difficulty(2).rules(false, false, true, true)
                        .regen(15.0F, 20, 5.0F, 5.0F, 0);
                ground(saturation, 4);
                saturation.player(0.5, 64.0, 0.5, Items.AIR);
                saturation.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(saturation);

                Scene starve = new Scene("player_regen_starve").ticks(90)
                        .life().difficulty(2).rules(false, false, true, true)
                        .regen(20.0F, 0, 0.0F, 0.0F, 0);
                ground(starve, 4);
                starve.player(0.5, 64.0, 0.5, Items.AIR);
                starve.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(starve);

                Scene naturalOff = new Scene("player_regen_natural_off").ticks(25)
                        .life().difficulty(2).rules(false, false, true, false)
                        .regen(10.0F, 20, 5.0F, 0.0F, 0);
                ground(naturalOff, 4);
                naturalOff.player(0.5, 64.0, 0.5, Items.AIR);
                naturalOff.equipment(new int[] {0, 0, 0, 0, 0}, new int[] {0, 0, 0, 0});
                scenes.add(naturalOff);
            }
            // ---- S2 补充：牛挂效果后的运动 ----
            // 漂浮：向上飘、到期落地；缓降：从高处慢落、不摔伤；
            // 速度 / 缓慢：散步速度变化；跳跃提升：AI 跳跃高度。
            for (int[] row : new int[][] {
                {40, 24, 0},  // 漂浮 I，40 刻后到期
                {120, 0, 2},  // 速度 III
                {120, 1, 2},  // 缓慢 III
                {120, 7, 2},  // 跳跃提升 III
            }) {
                int effect = row[1];
                Scene scene = new Scene("cow_effect_" + effect + "_" + row[2]).ticks(140);
                ground(scene, 6);
                scene.cow(0.5, 64.0, 0.5, 0.0F, 51L);
                scene.player(3.5, 64.0, 0.5, Items.AIR);
                scene.cowEffect(1, 0, 0, effect, row[0], row[2]);
                scenes.add(scene);
            }
            {
                // 缓降：牛从 24 格高落下，效果 60 刻到期后转普通下落，
                // 逐刻比 fallDistance / 落地血量。
                Scene slow = new Scene("cow_effect_slow_falling").ticks(140);
                ground(slow, 6);
                slow.cow(0.5, 88.0, 0.5, 0.0F, 51L);
                slow.player(3.5, 64.0, 0.5, Items.AIR);
                slow.cowEffect(1, 0, 0, 27, 60, 0);
                scenes.add(slow);
                // 对照组：同样高度但不挂效果，落地要摔伤。
                Scene plain = new Scene("cow_effect_plain_fall").ticks(140);
                ground(plain, 6);
                plain.cow(0.5, 88.0, 0.5, 0.0F, 51L);
                plain.player(3.5, 64.0, 0.5, Items.AIR);
                scenes.add(plain);
            }
            // ---- 1c：进服 / 重生落脚点，官方 `PlayerSpawnFinder.findSpawn` ----
            scenes.add(spawnSurface());
            scenes.add(spawnWater());
            scenes.add(spawnTree());
            scenes.add(spawnRadiusZero());
            scenes.add(spawnPersonalOk());
            scenes.add(spawnPersonalBlocked());
            scenes.add(spawnAdventure());
            // ---- 期 2：敌对生物与僵尸 ----
            for (long seed : new long[] {3L, 77L}) {
                Scene chase = new Scene("zombie_flat_chase_" + seed).ticks(120).difficulty(2);
                chase.monstersBurn = false;
                ground(chase, 12);
                chase.mob(1, 0.5, 64.0, 0.5, 0.0F, seed, 0);
                chase.player(6.5, 64.0, 0.5, Items.AIR);
                scenes.add(chase);
            }
            {
                Scene wall = new Scene("zombie_wall_path").ticks(120).difficulty(2);
                wall.monstersBurn = false;
                ground(wall, 12);
                wall.fill(3, 64, -3, 3, 65, 3, STONE);
                wall.mob(1, 0.5, 64.0, 0.5, 0.0F, 5L, 0);
                wall.player(8.5, 64.0, 0.5, Items.AIR);
                scenes.add(wall);
                // 白天燃烧按真实时间：主世界时钟逐刻走，天色、`MONSTERS_BURN`、月相
                // 都由时间线算；天空一直看得见（场景的 `monstersBurn` 只管 `canSeeSky`）。
                Scene burn = new Scene("zombie_burn_day").ticks(40).difficulty(2);
                ground(burn, 12);
                burn.clockStart = 1000;
                burn.mob(1, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                burn.player(20.5, 64.0, 0.5, Items.AIR);
                scenes.add(burn);
                Scene night = new Scene("zombie_burn_night").ticks(40).difficulty(2);
                ground(night, 12);
                night.clockStart = 18000;
                night.mob(1, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                night.player(20.5, 64.0, 0.5, Items.AIR);
                scenes.add(night);
                // 黄昏：第 242 刻跨过 12542，`MONSTERS_BURN` 变假，天色同时在变暗。
                Scene dusk = new Scene("zombie_burn_dusk").ticks(320).difficulty(2);
                ground(dusk, 12);
                dusk.clockStart = 12300;
                dusk.mob(1, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                dusk.player(50.5, 64.0, 0.5, Items.AIR);
                scenes.add(dusk);
                // 黎明：第 60 刻跨过 23460，`MONSTERS_BURN` 变真，但天还暗，要等亮度上来。
                Scene dawn = new Scene("zombie_burn_dawn").ticks(400).difficulty(2);
                ground(dawn, 12);
                dawn.clockStart = 23400;
                dawn.mob(1, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                dawn.player(50.5, 64.0, 0.5, Items.AIR);
                scenes.add(dawn);
                // 戴头盔晒太阳：铁头盔每次 `nextInt(2)` 掉耐久，不着火；护甲 +2。
                Scene helmet = new Scene("zombie_helmet_sun").ticks(600).difficulty(2);
                ground(helmet, 12);
                helmet.clockStart = 6000;
                helmet.helmet = Items.IRON_HELMET;
                helmet.mob(1, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                helmet.player(50.5, 64.0, 0.5, Items.AIR);
                scenes.add(helmet);
                // 快坏的皮头盔：烧坏的那一刻广播实体事件 49，下一刻发装备包、护甲回落、开始着火。
                Scene broken = new Scene("zombie_helmet_breaks").ticks(600).difficulty(2);
                ground(broken, 12);
                broken.clockStart = 6000;
                broken.helmet = Items.LEATHER_HELMET;
                broken.helmetDamage = 53;
                broken.mob(1, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                broken.mob(1, 4.5, 64.0, 4.5, 0.0F, 21L, 0);
                broken.player(50.5, 64.0, 0.5, Items.AIR);
                scenes.add(broken);
                // 不可损坏的东西（南瓜）戴在头上：挡太阳，不取 `nextInt(2)`。
                Scene pumpkin = new Scene("zombie_pumpkin_sun").ticks(80).difficulty(2);
                ground(pumpkin, 12);
                pumpkin.clockStart = 6000;
                pumpkin.helmet = Items.CARVED_PUMPKIN;
                pumpkin.mob(1, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                pumpkin.player(50.5, 64.0, 0.5, Items.AIR);
                scenes.add(pumpkin);
                Scene peaceful = new Scene("zombie_peaceful_clear").ticks(3).difficulty(0);
                peaceful.monstersBurn = false;
                ground(peaceful, 12);
                peaceful.mob(1, 0.5, 64.0, 0.5, 0.0F, 11L, 0);
                peaceful.player(3.5, 64.0, 0.5, Items.AIR);
                scenes.add(peaceful);
                Scene baby = new Scene("zombie_baby").ticks(60).difficulty(2);
                baby.monstersBurn = false;
                ground(baby, 12);
                baby.mob(1, 0.5, 64.0, 0.5, 0.0F, 13L, 1);
                baby.player(20.5, 64.0, 0.5, Items.AIR);
                scenes.add(baby);
                Scene husk = new Scene("zombie_husk").ticks(40).difficulty(2);
                ground(husk, 12);
                husk.skyDarken = 0;
                husk.mob(2, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                husk.player(20.5, 64.0, 0.5, Items.AIR);
                scenes.add(husk);
                Scene rain = new Scene("zombie_burn_rain").ticks(40).difficulty(2);
                ground(rain, 12);
                rain.clockStart = 6000;
                rain.rain = 1.0F;
                rain.mob(1, 0.5, 64.0, 0.5, 0.0F, 9L, 0);
                rain.player(20.5, 64.0, 0.5, Items.AIR);
                scenes.add(rain);
                Scene far = new Scene("zombie_far_despawn").ticks(40).difficulty(2);
                far.monstersBurn = false;
                ground(far, 12);
                far.mob(1, 0.5, 64.0, 0.5, 0.0F, 17L, 0);
                far.player(150.5, 64.0, 0.5, Items.AIR);
                scenes.add(far);
                // 一枚蛋：走过去、站上去（每刻踩蛋取世界随机数）、61 刻后啄碎。
                Scene egg = new Scene("zombie_turtle_egg").ticks(200).difficulty(2);
                egg.monstersBurn = false;
                ground(egg, 12);
                egg.set(4, 64, 0, Blocks.TURTLE_EGG.defaultBlockState());
                egg.mob(1, 0.5, 64.0, 0.5, 0.0F, 19L, 0);
                // 玩家在跟随距离 35 格之外，近战目标不会抢走移动标志。
                egg.player(50.5, 64.0, 0.5, Items.AIR);
                scenes.add(egg);
                // 一窝四枚：踩蛋的 1/100 会把蛋数减一并发 2001 事件，换几个种子覆盖到。
                for (long seed : new long[] {31L, 37L, 41L}) {
                    Scene eggs = new Scene("zombie_turtle_eggs_" + seed).ticks(240).difficulty(2);
                    eggs.monstersBurn = false;
                    ground(eggs, 12);
                    eggs.set(3, 64, 0, Blocks.TURTLE_EGG.defaultBlockState()
                            .setValue(net.minecraft.world.level.block.TurtleEggBlock.EGGS, 4));
                    eggs.mob(1, 0.5, 64.0, 0.5, 0.0F, seed, 0);
                    eggs.player(50.5, 64.0, 0.5, Items.AIR);
                    scenes.add(eggs);
                }
                // 牛从三格高落到一窝蛋上：`fallOn` 对非僵尸 1/3 踩坏一枚，落地后每刻再 1/100。
                Scene fall = new Scene("zombie_eggs_fall").ticks(80).difficulty(2);
                fall.monstersBurn = false;
                ground(fall, 12);
                int[] columns = {-6, -3, 3, 6};
                for (int i = 0; i < columns.length; i++) {
                    fall.set(columns[i], 64, 0, Blocks.TURTLE_EGG.defaultBlockState()
                            .setValue(net.minecraft.world.level.block.TurtleEggBlock.EGGS, 1 + i));
                    fall.mob(0, columns[i] + 0.5, 67.0, 0.5, 0.0F, 43L + i, 0);
                }
                fall.player(50.5, 64.0, 0.5, Items.AIR);
                scenes.add(fall);
                // 两只僵尸同一刻打到同一玩家：第二下必须落在无敌帧里。
                Scene two = new Scene("zombie_two_hit_same_tick").ticks(60).difficulty(2);
                two.monstersBurn = false;
                ground(two, 12);
                two.mob(1, 0.5, 64.0, 0.5, 0.0F, 23L, 0);
                two.mob(1, 1.5, 64.0, 0.5, 0.0F, 29L, 0);
                two.player(2.5, 64.0, 0.5, Items.AIR);
                scenes.add(two);
                // 着火的僵尸打玩家：打中才按区域难度取数传染火焰（`Zombie.doHurtTarget`）。
                Scene fireHit = new Scene("zombie_fire_hit").ticks(80).difficulty(2);
                fireHit.monstersBurn = false;
                fireHit.realPlayers = true;
                fireHit.fireTicks = 400;
                ground(fireHit, 12);
                fireHit.mob(1, 0.5, 64.0, 0.5, 0.0F, 23L, 0);
                fireHit.player(2.5, 64.0, 0.5, Items.AIR);
                scenes.add(fireHit);
                // 两只着火的僵尸同一刻打同一名玩家：第二只打进无敌帧，`wasHurt` 为假，不取那个数。
                Scene fireTwo = new Scene("zombie_fire_two_hit").ticks(80).difficulty(2);
                fireTwo.monstersBurn = false;
                fireTwo.realPlayers = true;
                fireTwo.fireTicks = 400;
                ground(fireTwo, 12);
                fireTwo.mob(1, 0.5, 64.0, 0.5, 0.0F, 23L, 0);
                fireTwo.mob(1, 1.5, 64.0, 0.5, 0.0F, 29L, 0);
                fireTwo.player(2.5, 64.0, 0.5, Items.AIR);
                scenes.add(fireTwo);
                // 第一下就打死玩家：死亡流程在僵尸自己的 tick 里当场跑完，
                // 同一刻后面的僵尸看到的是死人。
                Scene kill = new Scene("zombie_kill_player").ticks(60).difficulty(2);
                kill.monstersBurn = false;
                kill.realPlayers = true;
                kill.playerHealth = 2.0F;
                ground(kill, 12);
                kill.mob(1, 0.5, 64.0, 0.5, 0.0F, 23L, 0);
                kill.mob(1, 1.5, 64.0, 0.5, 0.0F, 29L, 0);
                kill.player(2.5, 64.0, 0.5, Items.AIR);
                scenes.add(kill);
            }
            // ---- S2：玩家药水效果的状态和出站包 ----
            scenes.add(effectBase("effect_give_600", 60)
                    .effectGive(1, 9, 600, 1));
            scenes.add(effectBase("effect_give_expire", 8)
                    .effectGive(1, 0, 3, 0));
            scenes.add(effectBase("effect_give_stack", 12)
                    .effectGive(1, 0, 40, 0).effectGive(3, 0, 5, 2));
            scenes.add(effectBase("effect_give_clear", 10)
                    .effectGive(1, 18, 100, 1).effectClear(4, 18));
            scenes.add(effectBase("effect_give_clear_all", 10)
                    .effectGive(1, 0, 100, 0).effectGive(1, 10, 100, 2)
                    .effectClearAll(5));
            scenes.add(effectBase("effect_regen_heal", 55)
                    .effectGive(1, 9, 100, 0));
            scenes.add(effectBase("effect_poison_hurt", 30)
                    .effectGive(1, 18, 100, 0));
            scenes.add(effectBase("effect_wither_hurt", 45)
                    .effectGive(1, 19, 100, 0));
            scenes.add(effectBase("effect_instant_health", 4)
                    .effectGive(1, 5, 1, 1));
            scenes.add(effectBase("effect_absorption", 12)
                    .effectGive(1, 21, 20, 1));
            scenes.add(effectBase("effect_infinite", 620)
                    .effectGive(1, 0, -1, 0));
            scenes.add(effectBase("effect_resync_600", 610)
                    .effectGive(1, 0, 1200, 0));
            // ---- S3：环境伤害来源 ----
            scenes.add(envBase("env_on_fire", 45, 0, 40, 300, 0, 0, 2));
            scenes.add(envBase("env_drown", 340, 3, 0, 300, 0, 0, 2));
            scenes.add(envBase("env_drown_short", 30, 3, 0, -19, 0, 0, 2));
            scenes.add(envBase("env_in_wall", 20, 8, 0, 300, 0, 0, 2));
            scenes.add(envBase("env_void", 20, 0, 0, 300, 0, 0, 2).envVoid());
            scenes.add(envBase("env_fall_4", 12, 0, 0, 300, 0, 4, 2));
            // ---- S4：吃东西 / 喝东西 ----
            scenes.add(consumeBase("consume_bread", 40, itemId(Items.BREAD), 10));
            scenes.add(consumeBase("consume_milk", 40, itemId(Items.MILK_BUCKET), 20));
            scenes.add(consumeBase("consume_golden_apple", 40, itemId(Items.GOLDEN_APPLE), 20));
            scenes.add(consumeBase("consume_enchanted_apple", 40,
                    itemId(Items.ENCHANTED_GOLDEN_APPLE), 20));
            scenes.add(consumeBase("consume_stew", 40, itemId(Items.SUSPICIOUS_STEW), 10));
            equineScenes(scenes);
            return scenes;
        }

        /**
         * 马科（类型 4 马、5 驴、6 骡、7 羊驼）的逐刻场景，借僵尸场景的多生物路径：
         * 吃草、扬蹄、甩尾计时和随机源，受伤扬蹄与恐慌，幼崽。
         * Equine tick scenes on the multi-mob zombie path: grazing, rearing, timers,
         * random draws, rearing and panic when hurt, and foals.
         */
        private static void equineScenes(List<Scene> scenes) {
            for (int kind = 4; kind <= 7; kind++) {
                for (long seed : new long[] {11L, 4242L}) {
                    Scene graze = new Scene("zombie_equine_graze_" + kind + "_" + seed).ticks(1500).difficulty(2);
                    ground(graze, 12);
                    graze.mob(kind, 0.5, 64.0, 0.5, 0.0F, seed, 0);
                    graze.player(20.5, 64.0, 0.5, Items.AIR);
                    scenes.add(graze);
                }
            }
            Scene foal = new Scene("zombie_equine_foal").ticks(800).difficulty(2);
            ground(foal, 12);
            foal.mob(4, 0.5, 64.0, 0.5, 0.0F, 31L, -24000);
            foal.mob(7, 4.5, 64.0, 0.5, 90.0F, 32L, -24000);
            foal.player(20.5, 64.0, 0.5, Items.AIR);
            scenes.add(foal);
            for (int kind : new int[] {4, 6}) {
                Scene hurt = new Scene("zombie_equine_hurt_" + kind).ticks(400).difficulty(2);
                ground(hurt, 12);
                hurt.mob(kind, 0.5, 64.0, 0.5, 0.0F, 57L, 0);
                hurt.player(3.5, 64.0, 0.5, Items.AIR);
                for (int tick : new int[] {20, 60, 100, 140, 180}) {
                    hurt.hurts.add(new double[] {tick, 0, 1.0, 0});
                }
                scenes.add(hurt);
            }
            Scene herd = new Scene("zombie_equine_herd").ticks(1200).difficulty(2);
            ground(herd, 12);
            herd.mob(4, 0.5, 64.0, 0.5, 0.0F, 71L, 0);
            herd.mob(5, 3.5, 64.0, 2.5, 45.0F, 72L, 0);
            herd.mob(7, -3.5, 64.0, -2.5, 180.0F, 73L, 0);
            herd.player(4.5, 64.0, 4.5, Items.WHEAT);
            scenes.add(herd);
        }

        private static int itemId(Item item) {
            return BuiltInRegistries.ITEM.getId(item);
        }

        private static Scene consumeBase(String name, int ticks, int item, int food) {
            Scene scene = new Scene(name).consume(ticks, item, food);
            scene.player(0.5, 64.0, 0.5, Items.AIR);
            return scene;
        }

        private static Scene envBase(String name, int ticks, int block, int fire, int air,
                int frozen, int fall, int difficulty) {
            Scene scene = new Scene(name).env(ticks, block, fire, air, frozen, fall, difficulty);
            scene.player(0.5, 64.0, 0.5, Items.AIR);
            return scene;
        }

        private static Scene effectBase(String name, int ticks) {
            Scene scene = new Scene(name).ticks(ticks).effects().difficulty(2);
            ground(scene, 2);
            scene.player(0.5, 64.0, 0.5, Items.AIR);
            return scene;
        }

        /** 普通地表的缺省半径 10。半径 0 时覆盖同一个出生点（对拍全程同一套方块）。 */
        private static Scene spawnBase(String name, int radius, long seed, int[] at) {
            Scene scene = new Scene(name).ticks(0).spawn(at[0], at[1], at[2], radius, seed);
            // 和 Bamboo 的超平坦模板一致：y=-64 基岩、-63/-62 泥土、-61 草。
            // 场景坐标和 Java 假世界的 MIN_Y=-64 对齐。
            scene.fill(-20, -64, -20, 20, -64, 20, STONE);
            scene.fill(-20, -63, -20, 20, -62, 20, DIRT);
            scene.fill(-20, -61, -20, 20, -61, 20, GRASS);
            return scene;
        }

        private static Scene spawnSurface() {
            return spawnBase("player_spawn_surface", 10, 12345L, new int[] {8, -60, 8});
        }

        private static Scene spawnRadiusZero() {
            return spawnBase("player_spawn_radius_zero", 0, 12345L, new int[] {8, -60, 8});
        }

        /** 出生点附近有一片水：水面列被 WORLD_SURFACE/OCEAN_FLOOR 排除。 */
        private static Scene spawnWater() {
            Scene scene = spawnBase("player_spawn_water", 10, 777L, new int[] {8, -60, 8});
            scene.fill(6, -60, 6, 10, -60, 10, WATER);
            return scene;
        }

        /** 出生点附近有一棵树（原木 + 树叶）：候选必须走 noCollisionNoLiquid。 */
        private static Scene spawnTree() {
            Scene scene = spawnBase("player_spawn_tree", 10, 4242L, new int[] {8, -60, 8});
            scene.fill(8, -60, 8, 8, -57, 8, Blocks.OAK_LOG.defaultBlockState());
            scene.fill(6, -56, 6, 10, -56, 10, Blocks.OAK_LEAVES.defaultBlockState());
            return scene;
        }

        /** `/spawnpoint` 设下两格都能重生的个人点：官方 forced 分支直接落地。 */
        private static Scene spawnPersonalOk() {
            Scene scene = spawnBase("player_spawn_personal_ok", 10, 99L, new int[] {8, -60, 8});
            scene.personalSpawn(14, -60, 14);
            return scene;
        }

        /** 个人点的两格有实心方块：官方回退到世界出生点 + findSpawn。 */
        private static Scene spawnPersonalBlocked() {
            Scene scene = spawnBase("player_spawn_personal_blocked", 0, 99L, new int[] {8, -60, 8});
            scene.personalSpawn(14, -60, 14);
            scene.set(14, -60, 14, STONE);
            return scene;
        }

        /** 冒险模式：findSpawn 直接 fixupSpawnHeight，不散布。 */
        private static Scene spawnAdventure() {
            return spawnBase("player_spawn_adventure", 10, 31415L, new int[] {8, -60, 8})
                    .spawnGameType(2);
        }
    }

    // ---- 运行 ----

    private static void play(Official official, Writer out, Scene scene) throws Exception {
        if (scene.name.startsWith("zombie_")) {
            playZombie(official, out, scene);
            return;
        }
        if (scene.spawnScene) {
            playSpawn(official, out, scene);
            return;
        }
        if (scene.playerLife) {
            playLife(official, out, scene);
            return;
        }
        if (scene.effectScene) {
            playEffects(official, out, scene);
            return;
        }
        if (scene.envScene) {
            playEnv(official, out, scene);
            return;
        }
        if (scene.consumeScene) {
            playConsume(official, out, scene);
            return;
        }
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
            for (int[] action : scene.cowEffectActions) {
                if (action[0] != tick || action[1] >= cows.size()) continue;
                Cow target = cows.get(action[1]);
                var holder = official.registries.lookupOrThrow(Registries.MOB_EFFECT)
                        .get(action[3]).orElseThrow();
                if (action[2] == 0) {
                    target.addEffect(new MobEffectInstance(holder, action[4], action[5]));
                } else {
                    target.removeAllEffects();
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
            // 实体 tick 里也会生成实体（繁殖的幼崽、死亡掉落物和经验球），在这一刻收齐。
            if (scene.drops) {
                level.spawned.sort(java.util.Comparator.comparingInt(Entity::getId));
                for (Entity born : level.spawned) {
                    level.reseedBorn(born);
                    spawns.get(tick).add(spawnJson(born));
                    if (born instanceof Cow calf) {
                        cows.add(calf);
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
        line.append(",\"cowEffectActions\":[");
        for (int i = 0; i < scene.cowEffectActions.size(); i++) {
            if (i > 0) line.append(',');
            int[] a = scene.cowEffectActions.get(i);
            line.append('[').append(a[0]);
            for (int j = 1; j < a.length; j++) line.append(',').append(a[j]);
            line.append(']');
        }
        line.append("],\"interactActions\":[");
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
    /** 包 ID + 包体（不含帧长）的 ID。 */
    private static int bodyId(byte[] body) {
        int value = 0, shift = 0, index = 0;
        while (index < body.length && index < 5) {
            int one = body[index++] & 0xff;
            value |= (one & 0x7f) << shift;
            if ((one & 0x80) == 0) return value;
            shift += 7;
        }
        return -1;
    }

    // ---- 1b：玩家生命场景 ----

    /** 官方 `GameRules.get` 在假世界里改一条布尔规则。 */
    private static void setRule(Official official, GameRule<Integer> rule, int value) {
        // 整数规则（1c 的 `respawn_radius`）：和布尔版一样直接改底层 map。
        try {
            Field rules = GameRules.class.getDeclaredField("rules");
            rules.setAccessible(true);
            Object map = rules.get(official.gameRules);
            java.lang.reflect.Method set = map.getClass().getDeclaredMethod(
                    "set", GameRule.class, Object.class);
            set.setAccessible(true);
            set.invoke(map, rule, Integer.valueOf(value));
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static void setRule(Official official, GameRule<Boolean> rule, boolean value) {
        // `GameRules.set` 会通知 `NotificationManager`（假服务端没有），
        // 这里直接改底层 `GameRuleMap`，语义和 `GameRules.get` 读到的一样。
        try {
            Field rules = GameRules.class.getDeclaredField("rules");
            rules.setAccessible(true);
            Object map = rules.get(official.gameRules);
            java.lang.reflect.Method set = map.getClass().getDeclaredMethod(
                    "set", GameRule.class, Object.class);
            set.setAccessible(true);
            set.invoke(map, rule, Boolean.valueOf(value));
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }


    // 只注入构造随机源；运行原版的目标、射击、物理、玩家受伤和同步。
    // Inject constructor seeds only; use vanilla goals, shooting, physics, damage and tracking.
    private static final class OracleSkeleton extends net.minecraft.world.entity.monster.skeleton.Skeleton {
        OracleSkeleton(FakeLevel level) {
            super(EntityTypes.SKELETON, level);
        }

        @Override
        protected net.minecraft.world.entity.projectile.arrow.AbstractArrow getArrow(
                ItemStack projectile, float power, ItemStack weapon) {
            FakeLevel world = (FakeLevel) level();
            var arrow = ((net.minecraft.world.item.ArrowItem) Items.ARROW).createArrow(world, projectile, this, weapon);
            setRandom(arrow, world.entitySeeds.nextLong());
            arrow.setUUID(Mth.createInsecureUUID(arrow.getRandom()));
            arrow.setBaseDamageFromMob(power);
            return arrow;
        }
    }

    private static int playSkeletonDeaths(Official official, Writer out) throws Exception {
        final int samples = 38;
        boolean previousMobDrops = official.gameRules.get(GameRules.MOB_DROPS);
        try {
            for (int sample = 0; sample < samples; sample++) {
                playSkeletonDeath(official, out, sample);
            }
        } finally {
            setRule(official, GameRules.MOB_DROPS, previousMobDrops);
        }
        return samples;
    }

    private static void playSkeletonDeath(Official official, Writer out, int sample) throws Exception {
        // 同一个临界随机种子的两次玩家击杀，只改变抢夺等级。
        // Two player kills with the same boundary seed; only Looting changes.
        boolean rememberedFall = sample >= 36;
        boolean indirect = sample == 34 || sample == 35;
        boolean lootingBoundary = sample >= 32 && !rememberedFall;
        int memoryTicks = sample == 37 ? 1 : 0;
        int mode = lootingBoundary ? 3 : rememberedFall ? 0 : sample % 8;
        long seed = sample >= 32 ? 20261173L : 20261120L + sample * 104729L;
        boolean playerKill = mode == 1 || mode == 2 || mode == 3 || mode == 6 || mode == 7;
        boolean drops = mode != 6;
        int looting = (mode == 3 && sample != 32 && sample != 34) || mode == 7 || rememberedFall ? 3 : 0;
        FakeLevel world = FakeLevel.create(official);
        setRule(official, GameRules.MOB_DROPS, drops);
        official.server.sequences = new RandomSequences();
        var mob = EntityTypes.SKELETON.create(world, EntitySpawnReason.COMMAND);
        mob.setPos(0.5, 64.0, 0.5);
        setRandom(mob, seed);
        var slots = net.minecraft.world.entity.EquipmentSlot.VALUES.subList(0, 6);
        List<String> gear = new ArrayList<>();
        switch (mode) {
            case 0 -> equipSkeletonDeath(official, mob, gear, slots.get(0), Items.BOW, 0.085F, 0, false);
            case 2 -> {
                equipSkeletonDeath(official, mob, gear, slots.get(0), Items.BOW, 0.085F, 0, false);
                equipSkeletonDeath(official, mob, gear, slots.get(2), Items.IRON_BOOTS, 1.0F, 0, false);
            }
            case 3 -> equipSkeletonDeath(official, mob, gear, slots.get(0), Items.BOW, 0.085F, 0, false);
            case 4 -> equipSkeletonDeath(official, mob, gear, slots.get(0), Items.BOW, 2.0F, 31, false);
            case 5 -> equipSkeletonDeath(official, mob, gear, slots.get(0), Items.BOW, 2.0F, 31, true);
            case 6 -> equipSkeletonDeath(official, mob, gear, slots.get(0), Items.BOW, 2.0F, 0, false);
        }
        ServerPlayer player = playerKill || rememberedFall ? skeletonDeathPlayer(official, world, looting) : null;
        if (rememberedFall) mob.setLastHurtByPlayer(player, memoryTicks);
        var source = skeletonDeathSource(world, playerKill ? player : null, indirect);
        if (!mob.hurtServer(world, source, 100.0F) || !mob.isDeadOrDying()) {
            throw new IllegalStateException("skeleton_death did not kill the mob: " + sample);
        }
        List<String> spawned = new ArrayList<>();
        boolean xpDropped = false;
        for (Entity entity : world.spawned) {
            switch (entity) {
                case net.minecraft.world.entity.item.ItemEntity item -> {
                    var stack = item.getItem();
                    spawned.add("{\"item\":" + BuiltInRegistries.ITEM.getId(stack.getItem())
                            + ",\"count\":" + stack.getCount() + ",\"damage\":" + stack.getDamageValue() + ",\"xp\":0}");
                }
                case net.minecraft.world.entity.ExperienceOrb orb -> {
                    xpDropped = true;
                    spawned.add("{\"item\":0,\"count\":0,\"damage\":0,\"xp\":" + orb.getValue() + "}");
                }
                default -> throw new IllegalStateException("unexpected skeleton death spawn: " + entity);
            }
        }
        boolean mainHandDropped = mob.getItemBySlot(slots.getFirst()).isEmpty();
        if ((!drops && !spawned.isEmpty()) || xpDropped != ((playerKill || memoryTicks > 0) && drops)
                || (mode == 4 && !mainHandDropped) || (mode == 5 && mainHandDropped)
                || (lootingBoundary && mainHandDropped != (looting > 0))) {
            throw new IllegalStateException("skeleton_death did not exercise the expected drops: " + sample);
        }
        int[] remaining = slots.stream().map(mob::getItemBySlot)
                .mapToInt(stack -> BuiltInRegistries.ITEM.getId(stack.getItem())).toArray();
        out.write("{\"type\":\"skeletonDeath\",\"seed\":" + seed + ",\"gear\":[" + String.join(",", gear)
                + "],\"playerKill\":" + playerKill + ",\"looting\":" + looting + ",\"drops\":" + drops
                + ",\"indirect\":" + indirect + ",\"memoryTicks\":" + memoryTicks
                + ",\"spawned\":[" + String.join(",", spawned) + "],\"remaining\":" + Arrays.toString(remaining)
                + ",\"mobSeed\":" + randomSeed(mob) + ",\"levelSeed\":" + levelSeed(world) + "}\n");
    }

    private static net.minecraft.world.damagesource.DamageSource skeletonDeathSource(
            FakeLevel world, ServerPlayer player, boolean indirect) {
        if (player == null) return world.damageSources().fall();
        if (!indirect) return world.damageSources().playerAttack(player);
        var arrow = new net.minecraft.world.entity.projectile.arrow.Arrow(EntityTypes.ARROW, world);
        arrow.setOwner(player);
        arrow.setPos(player.position());
        arrow.setDeltaMovement(1.0, 0.0, 0.0);
        return world.damageSources().arrow(arrow, player);
    }

    private static void equipSkeletonDeath(Official official, Mob mob, List<String> gear,
            net.minecraft.world.entity.EquipmentSlot slot, Item item, float chance, int damage, boolean vanishing) {
        ItemStack stack = new ItemStack(item);
        stack.setDamageValue(damage);
        if (vanishing) {
            stack.enchant(official.registries.lookupOrThrow(Registries.ENCHANTMENT)
                    .getOrThrow(net.minecraft.world.item.enchantment.Enchantments.VANISHING_CURSE), 1);
        }
        mob.setItemSlot(slot, stack);
        mob.setDropChance(slot, chance);
        gear.add("{\"slot\":" + slot.ordinal() + ",\"item\":" + BuiltInRegistries.ITEM.getId(item)
                + ",\"chance\":" + chance + ",\"damage\":" + damage + ",\"vanishing\":" + vanishing + "}");
    }

    private static ServerPlayer skeletonDeathPlayer(Official official, FakeLevel world, int looting) {
        ServerPlayer player = new ServerPlayer(official.server, world,
                new GameProfile(new UUID(1L, 2L), "DeathOracle"), ClientInformation.createDefault());
        player.setId(1025);
        player.setPos(2.5, 64.0, 0.5);
        ItemStack sword = new ItemStack(Items.DIAMOND_SWORD);
        if (looting > 0) {
            sword.enchant(official.registries.lookupOrThrow(Registries.ENCHANTMENT)
                    .getOrThrow(net.minecraft.world.item.enchantment.Enchantments.LOOTING), looting);
        }
        player.setItemInHand(InteractionHand.MAIN_HAND, sword);
        world.playersList().add(player);
        world.tracked.add(player);
        return player;
    }

    private record ArrowPickupSetup(int count, int pickup, boolean grounded, int shake,
                                    boolean creative, int fullInventory) {}

    private static int playArrowPickups(Official official, Writer out) throws Exception {
        var setups = List.of(
                new ArrowPickupSetup(1, 1, true, 0, false, -1),
                new ArrowPickupSetup(2, 1, true, 0, false, 63),
                new ArrowPickupSetup(2, 1, true, 0, true, 63),
                new ArrowPickupSetup(1, 1, true, 0, false, 0),
                new ArrowPickupSetup(1, 1, true, 0, true, 0),
                new ArrowPickupSetup(1, 2, true, 0, false, -1),
                new ArrowPickupSetup(1, 2, true, 0, true, -1),
                new ArrowPickupSetup(1, 0, true, 0, true, -1),
                new ArrowPickupSetup(1, 1, true, 1, false, -1),
                new ArrowPickupSetup(1, 1, false, 0, false, -1));
        for (var setup : setups) playArrowPickup(official, out, setup);
        return setups.size();
    }

    private static void playArrowPickup(Official official, Writer out, ArrowPickupSetup setup) throws Exception {
        var world = FakeLevel.create(official);
        var player = skeletonDeathPlayer(official, world, 0);
        player.getAbilities().instabuild = setup.creative();
        var inventory = player.getInventory();
        inventory.setItem(0, ItemStack.EMPTY);
        if (setup.fullInventory() >= 0) {
            for (int slot = 0; slot < 36; slot++) inventory.setItem(slot, new ItemStack(Items.STONE, 64));
            if (setup.fullInventory() > 0) inventory.setItem(0, new ItemStack(Items.ARROW, setup.fullInventory()));
        }
        var arrow = new net.minecraft.world.entity.projectile.arrow.Arrow(EntityTypes.ARROW, world);
        var ground = net.minecraft.world.entity.projectile.arrow.AbstractArrow.class.getDeclaredMethod("setInGround", boolean.class);
        ground.setAccessible(true);
        ground.invoke(arrow, setup.grounded());
        arrow.shakeTime = setup.shake();
        arrow.pickup = net.minecraft.world.entity.projectile.arrow.AbstractArrow.Pickup.values()[setup.pickup()];
        arrow.getPickupItemStackOrigin().setCount(setup.count());
        arrow.playerTouch(player);
        int arrows = 0;
        for (int slot = 0; slot < 36; slot++) {
            var stack = inventory.getItem(slot);
            if (stack.is(Items.ARROW)) arrows += stack.getCount();
        }
        out.write("{\"type\":\"arrowPickup\",\"count\":" + setup.count() + ",\"pickup\":" + setup.pickup()
                + ",\"grounded\":" + setup.grounded() + ",\"shake\":" + setup.shake()
                + ",\"creative\":" + setup.creative() + ",\"fullInventory\":" + setup.fullInventory()
                + ",\"removed\":" + arrow.isRemoved() + ",\"arrows\":" + arrows
                + ",\"remaining\":" + arrow.getPickupItemStackOrigin().getCount() + "}\n");
    }

    private static int playHalloweenSpawns(Official official, Writer out) throws Exception {
        // 只在定向对拍 JVM 中替换节日日期常量，不改系统日期，也不改原版生成逻辑。
        // Replace the holiday's calendar constant in this targeted JVM only, leaving
        // the system clock and all of the official spawn logic untouched.
        var old = net.minecraft.util.SpecialDates.HALLOWEEN;
        Field field = net.minecraft.util.SpecialDates.class.getDeclaredField("HALLOWEEN");
        var base = UNSAFE.staticFieldBase(field);
        long offset = UNSAFE.staticFieldOffset(field);
        try {
            UNSAFE.putObject(base, offset, net.minecraft.util.SpecialDates.dayNow());
            return playSkeletonSpawns(official, out, true);
        } finally {
            UNSAFE.putObject(base, offset, old);
        }
    }

    private static int playSkeletonSpawns(Official official, Writer out, boolean halloween) throws Exception {
        if (net.minecraft.util.SpecialDates.isHalloween() != halloween) {
            throw new IllegalStateException("skeleton spawn calendar does not match the fixture");
        }
        FakeLevel world = FakeLevel.create(official);
        BlockPos pos = new BlockPos(0, 64, 0);
        int cases = 0;
        int armored = 0;
        int enchanted = 0;
        int pumpkins = 0;
        int lanterns = 0;
        int helmets = 0;
        for (int sample = 0; sample < 64; sample++) {
            long seed = 20261120L + sample * 104729L;
            Difficulty difficulty = List.of(Difficulty.EASY, Difficulty.NORMAL, Difficulty.HARD).get(sample % 3);
            long clock = sample % 4 == 0 ? 0L : 1536000L;
            long inhabitedTime = sample % 4 == 0 ? 0L : 3600000L;
            ((TickingLevelData) world.levelData).difficulty = difficulty;
            world.spawnDifficulty = new DifficultyInstance(difficulty, clock, inhabitedTime, 1.0F);
            for (boolean egg : new boolean[] {false, true}) {
                world.getRandom().setSeed(seed);
                var mob = skeletonSpawn(world, pos, egg);
                var slots = List.of(net.minecraft.world.entity.EquipmentSlot.MAINHAND,
                        net.minecraft.world.entity.EquipmentSlot.FEET, net.minecraft.world.entity.EquipmentSlot.LEGS,
                        net.minecraft.world.entity.EquipmentSlot.CHEST, net.minecraft.world.entity.EquipmentSlot.HEAD);
                var stacks = slots.stream().map(mob::getItemBySlot).toList();
                if (stacks.stream().skip(1).anyMatch(stack -> !stack.isEmpty())) armored++;
                if (stacks.stream().anyMatch(ItemStack::isEnchanted)) enchanted++;
                var head = stacks.getLast();
                if (head.is(Items.CARVED_PUMPKIN)) pumpkins++;
                else if (head.is(Items.JACK_O_LANTERN)) lanterns++;
                else if (!head.isEmpty()) helmets++;
                var follow = mob.getAttribute(net.minecraft.world.entity.ai.attributes.Attributes.FOLLOW_RANGE);
                var bonus = follow.getModifier(Identifier.withDefaultNamespace("random_spawn_bonus"));
                out.write("{\"type\":\"skeletonSpawn\",\"seed\":" + seed + ",\"egg\":" + egg
                        + ",\"halloween\":" + halloween
                        + ",\"headDropBits\":" + Float.floatToRawIntBits(mob.getDropChances().byEquipment(slots.getLast()))
                        + ",\"difficulty\":" + difficulty.getId() + ",\"clock\":" + clock
                        + ",\"inhabitedTime\":" + inhabitedTime + ",\"leftHanded\":" + mob.isLeftHanded()
                        + ",\"canPickUpLoot\":" + mob.canPickUpLoot()
                        + ",\"followBonus\":" + Double.doubleToRawLongBits(bonus.amount())
                        + (egg ? ",\"yaw\":" + Float.floatToRawIntBits(mob.getYRot()) : "")
                        + ",\"equipment\":" + stacks.stream().map(stack -> skeletonSpawnEquipment(official, stack))
                                .collect(java.util.stream.Collectors.joining(",", "[", "]"))
                        + ",\"levelSeed\":" + levelSeed(world) + "}\n");
                cases++;
            }
        }
        if (armored == 0 || enchanted == 0) {
            throw new IllegalStateException("skeleton_spawn did not exercise armor and enchantment generation");
        }
        if (halloween && (pumpkins == 0 || lanterns == 0 || helmets == 0)) {
            throw new IllegalStateException("skeleton_halloween needs both holiday items and an ordinary helmet");
        }
        return cases;
    }

    private static int playEquines(Official official, Writer out) throws Exception {
        var world = FakeLevel.create(official);
        var pos = new BlockPos(0, 64, 0);
        var types = List.of(EntityTypes.HORSE, EntityTypes.DONKEY, EntityTypes.MULE, EntityTypes.LLAMA);
        var eggs = List.of(Items.HORSE_SPAWN_EGG, Items.DONKEY_SPAWN_EGG, Items.MULE_SPAWN_EGG, Items.LLAMA_SPAWN_EGG);
        int cases = 0;
        for (int typeIndex = 0; typeIndex < types.size(); typeIndex++) {
            var type = types.get(typeIndex);
            for (int sample = 0; sample < 8; sample++) {
                long seed = 20261120L + sample * 104729L;
                for (boolean egg : new boolean[] {false, true}) {
                    world.getRandom().setSeed(seed);
                    var reason = egg ? EntitySpawnReason.SPAWN_ITEM_USE : EntitySpawnReason.COMMAND;
                    var mob = egg
                            ? type.spawn(world, new ItemStack(eggs.get(typeIndex)), null, pos, reason, true, false)
                            : type.create(world, reason);
                    if (!egg) {
                        mob.setPos(0.5, 64, 0.5);
                        mob.finalizeSpawn(world, world.spawnDifficulty, reason, null);
                    }
                    out.write("{\"type\":\"equineSpawn\",\"seed\":" + seed
                            + ",\"kind\":" + typeIndex + ",\"egg\":" + egg
                            + ",\"state\":" + equineState(mob)
                            + ",\"leftHanded\":" + mob.isLeftHanded()
                            + ",\"yaw\":" + (egg ? Float.floatToRawIntBits(mob.getYRot()) : 0)
                            + ",\"levelSeed\":" + levelSeed(world)
                            + ",\"data\":\"" + pairingData(official, mob) + "\"}\n");
                    cases++;
                }

                var parent = type.create(world, EntitySpawnReason.COMMAND);
                var partnerType = typeIndex < 2 && (sample & 1) == 1 ? types.get(1 - typeIndex) : type;
                var partner = partnerType.create(world, EntitySpawnReason.COMMAND);
                world.getRandom().setSeed(seed);
                parent.finalizeSpawn(world, world.spawnDifficulty, EntitySpawnReason.COMMAND, null);
                partner.finalizeSpawn(world, world.spawnDifficulty, EntitySpawnReason.COMMAND, null);
                setRandom(parent, seed);
                long parentSeed = randomSeed(parent);
                var child = typeIndex == 2 ? null : parent.getBreedOffspring(world, partner);
                if (typeIndex == 2) {
                    if (parent.canMate(partner)) throw new IllegalStateException("Mules unexpectedly fertile");
                }
                out.write("{\"type\":\"equineBreed\",\"seed\":" + seed
                        + ",\"kind\":" + typeIndex + ",\"partner\":" + types.indexOf(partnerType)
                        + ",\"parentSeed\":" + parentSeed + ",\"afterSeed\":" + randomSeed(parent)
                        + ",\"parentState\":" + equineState(parent) + ",\"partnerState\":" + equineState(partner)
                        + ",\"child\":" + (child == null ? "null" : equineState((net.minecraft.world.entity.animal.equine.AbstractHorse) child))
                        + ",\"childKind\":" + (child == null ? -1 : types.indexOf(child.getType())) + "}\n");
                cases++;
            }
        }
        return cases + playEquineInteractions(official, out, world, types) + playEquineDeaths(official, out, types);
    }

    /**
     * 马科死亡掉落：每种 × 五种模式 × 两个种子。0 玩家击杀、1 摔死、2 关掉 MOB_DROPS、
     * 3 幼崽、4 抢夺 III。成年的带鞍、马铠或地毯，箱子马带箱子和两格物品。
     * Equine death drops: kind × mode × seed, with saddles, body armor, chests and contents.
     */
    private static int playEquineDeaths(Official official, Writer out,
            List<? extends net.minecraft.world.entity.EntityType<? extends net.minecraft.world.entity.animal.equine.AbstractHorse>> types)
            throws Exception {
        int cases = 0;
        boolean previousMobDrops = official.gameRules.get(GameRules.MOB_DROPS);
        try {
            for (int typeIndex = 0; typeIndex < types.size(); typeIndex++) {
                for (int mode = 0; mode < 5; mode++) {
                    for (int sample = 0; sample < 2; sample++) {
                        long seed = 7700L + typeIndex * 100L + mode * 10L + sample;
                        boolean drops = mode != 2;
                        boolean playerKill = mode == 0 || mode == 2 || mode == 4;
                        int looting = mode == 4 ? 3 : 0;
                        FakeLevel world = FakeLevel.create(official);
                        setRule(official, GameRules.MOB_DROPS, drops);
                        official.server.sequences = new RandomSequences();
                        net.minecraft.world.entity.animal.equine.AbstractHorse mob =
                                types.get(typeIndex).create(world, EntitySpawnReason.COMMAND);
                        mob.setPos(0.5, 64.0, 0.5);
                        setRandom(mob, seed);
                        mob.setTamed(true);
                        if (mode == 3) mob.setAge(-24000);
                        boolean llama = mob instanceof net.minecraft.world.entity.animal.equine.Llama;
                        if (!llama && mode != 3) {
                            mob.setItemSlot(net.minecraft.world.entity.EquipmentSlot.SADDLE, new ItemStack(Items.SADDLE));
                            mob.setDropChance(net.minecraft.world.entity.EquipmentSlot.SADDLE, 2.0F);
                        }
                        Item body = mob instanceof net.minecraft.world.entity.animal.equine.Horse ? Items.IRON_HORSE_ARMOR
                                : llama ? Items.CARPET.pick(net.minecraft.world.item.DyeColor.RED) : null;
                        if (body != null && mode != 3) {
                            mob.setItemSlot(net.minecraft.world.entity.EquipmentSlot.BODY, new ItemStack(body));
                            mob.setDropChance(net.minecraft.world.entity.EquipmentSlot.BODY, 2.0F);
                        }
                        if (llama) {
                            java.lang.reflect.Method strength = net.minecraft.world.entity.animal.equine.Llama.class
                                    .getDeclaredMethod("setStrength", int.class);
                            strength.setAccessible(true);
                            strength.invoke(mob, 3);
                        }
                        if (mob instanceof net.minecraft.world.entity.animal.equine.AbstractChestedHorse) {
                            mob.getSlot(499).set(new ItemStack(Items.CHEST));
                            mob.getSlot(500).set(new ItemStack(Items.DIAMOND, 3));
                            mob.getSlot(502).set(new ItemStack(Items.WHEAT, 10));
                        }
                        ServerPlayer player = playerKill ? skeletonDeathPlayer(official, world, looting) : null;
                        var source = playerKill ? world.damageSources().playerAttack(player) : world.damageSources().fall();
                        if (!mob.hurtServer(world, source, 100.0F) || !mob.isDeadOrDying()) {
                            throw new IllegalStateException("equine_death did not kill the mob");
                        }
                        List<String> spawned = new ArrayList<>();
                        for (Entity entity : world.spawned) {
                            switch (entity) {
                                case net.minecraft.world.entity.item.ItemEntity item -> {
                                    var stack = item.getItem();
                                    spawned.add("{\"item\":" + BuiltInRegistries.ITEM.getId(stack.getItem())
                                            + ",\"count\":" + stack.getCount() + ",\"damage\":" + stack.getDamageValue() + ",\"xp\":0}");
                                }
                                case net.minecraft.world.entity.ExperienceOrb orb ->
                                    spawned.add("{\"item\":0,\"count\":0,\"damage\":0,\"xp\":" + orb.getValue() + "}");
                                default -> throw new IllegalStateException("unexpected equine death spawn: " + entity);
                            }
                        }
                        out.write("{\"type\":\"equineDeath\",\"kind\":" + typeIndex + ",\"mode\":" + mode
                                + ",\"seed\":" + seed + ",\"spawned\":[" + String.join(",", spawned)
                                + "],\"mobSeed\":" + randomSeed(mob) + ",\"levelSeed\":" + levelSeed(world) + "}\n");
                        cases++;
                    }
                }
            }
        } finally {
            setRule(official, GameRules.MOB_DROPS, previousMobDrops);
        }
        return cases;
    }

    /** 交互对拍用的物品，按顺序编号。 */
    private static List<Item> equineItems() {
        return List.of(Items.WHEAT, Items.SUGAR, Items.HAY_BLOCK,
            Items.APPLE, Items.CARROT, Items.RED_MUSHROOM, Items.GOLDEN_CARROT, Items.GOLDEN_APPLE,
            Items.CHEST, Items.SADDLE, Items.IRON_HORSE_ARMOR, Items.CARPET.pick(net.minecraft.world.item.DyeColor.WHITE), Items.STICK);
    }

    /**
     * 马科右键：每种马科 × 六种初始状态 × 每件物品，走官方 `Player.interactOn`，
     * 比结果、手里剩几个、马的标志字节、脾气、血量、求爱、年龄、箱子、鞍、身体和随机源。
     * 驯服的成年个体潜行会开物品栏（要连接），不出这一题。
     * Equine right-click through `Player.interactOn`, compared on the result, the held
     * count and the mount's state. Sneaking on a tamed adult opens a screen and is skipped.
     */
    private static int playEquineInteractions(Official official, Writer out, FakeLevel world,
            List<? extends net.minecraft.world.entity.EntityType<? extends net.minecraft.world.entity.animal.equine.AbstractHorse>> types)
            throws Exception {
        int cases = 0;
        List<Item> EQUINE_ITEMS = equineItems();
        GameProfile profile = new GameProfile(new UUID(1L, 2L), "Oracle0");
        for (int typeIndex = 0; typeIndex < types.size(); typeIndex++) {
            for (int setup = 0; setup < 6; setup++) {
                for (int itemIndex = 0; itemIndex < EQUINE_ITEMS.size(); itemIndex++) {
                    for (int mode = 0; mode < 2; mode++) {
                        boolean creative = mode == 1 && setup == 2;
                        boolean sneak = mode == 1 && setup >= 4;
                        if (mode == 1 && !creative && !sneak) continue;
                        long seed = 9001L + typeIndex * 1000L + setup * 100L + itemIndex * 3L + mode;
                        net.minecraft.world.entity.animal.equine.AbstractHorse mob =
                                types.get(typeIndex).create(world, EntitySpawnReason.COMMAND);
                        setRandom(mob, seed);
                        setupEquine(mob, setup);
                        ServerPlayer player = new ServerPlayer(official.server, world, profile,
                                ClientInformation.createDefault());
                        player.setPos(1.5, 64.0, 0.5);
                        // `doPlayerRide` 会 `startRiding`，`ServerPlayer` 要一个能收包的连接。
                        // `doPlayerRide` starts riding, which needs a connection that takes packets.
                        ServerGamePacketListenerImpl connection = allocate(RecordingListener.class);
                        put(ServerGamePacketListenerImpl.class, connection, "player", player);
                        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                                connection, "server", official.server);
                        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                                connection, "connection", new Connection(PacketFlow.CLIENTBOUND));
                        ((RecordingListener) connection).bytes = new Bytes(official.registries);
                        put(ServerPlayer.class, player, "connection", connection);
                        putBoolean(net.minecraft.world.entity.player.Abilities.class, player.getAbilities(),
                                "instabuild", creative);
                        player.setShiftKeyDown(sneak);
                        player.setItemInHand(InteractionHand.MAIN_HAND, new ItemStack(EQUINE_ITEMS.get(itemIndex)));
                        var result = player.interactOn(mob, InteractionHand.MAIN_HAND, mob.position());
                        String swing = result instanceof net.minecraft.world.InteractionResult.Success success
                                ? success.swingSource().name() : "PASS";
                        out.write("{\"type\":\"equineInteract\",\"kind\":" + typeIndex + ",\"setup\":" + setup
                                + ",\"item\":" + BuiltInRegistries.ITEM.getId(EQUINE_ITEMS.get(itemIndex))
                                + ",\"creative\":" + creative + ",\"sneak\":" + sneak + ",\"seed\":" + seed
                                + ",\"swing\":\"" + swing + "\""
                                + ",\"count\":" + player.getItemInHand(InteractionHand.MAIN_HAND).getCount()
                                + ",\"after\":" + equineAfter(mob)
                                + ",\"data\":\"" + pairingData(official, mob) + "\"}\n");
                        cases++;
                    }
                }
            }
        }
        return cases;
    }

    /** 六种初始状态：0 野生满血、1 野生残血高脾气、2 驯服满血、3 驯服残血带鞍带护甲带箱子、4 驯服幼崽残血、5 野生幼崽。 */
    private static void setupEquine(net.minecraft.world.entity.animal.equine.AbstractHorse mob, int setup) {
        boolean llama = mob instanceof net.minecraft.world.entity.animal.equine.Llama;
        mob.setTamed(setup >= 2 && setup <= 4);
        if (setup == 1) {
            mob.setTemper(llama ? 28 : 95);
        }
        if (setup == 1 || setup == 3 || setup == 4) {
            mob.setHealth(10.0F);
        }
        if (setup >= 4) {
            mob.setAge(-24000);
        }
        if (setup == 3) {
            if (!llama) {
                mob.setItemSlot(net.minecraft.world.entity.EquipmentSlot.SADDLE, new ItemStack(Items.SADDLE));
            }
            if (mob instanceof net.minecraft.world.entity.animal.equine.Horse) {
                mob.setItemSlot(net.minecraft.world.entity.EquipmentSlot.BODY, new ItemStack(Items.IRON_HORSE_ARMOR));
            }
            if (llama) {
                mob.setItemSlot(net.minecraft.world.entity.EquipmentSlot.BODY, new ItemStack(Items.CARPET.pick(net.minecraft.world.item.DyeColor.WHITE)));
            }
            if (mob instanceof net.minecraft.world.entity.animal.equine.AbstractChestedHorse chested) {
                chested.setChest(true);
            }
        }
        // 已经在世界里跑过的生物：`onEquipItem` 只在 `!firstTick` 时出声取数。
        // A mob that has already ticked: `onEquipItem` only plays and draws after the first tick.
        putBoolean(Entity.class, mob, "firstTick", false);
    }

    /** 交互之后：标志字节、脾气、血量位、求爱、年龄、箱子、鞍、身体、随机源。 */
    private static String equineAfter(net.minecraft.world.entity.animal.equine.AbstractHorse mob) throws Exception {
        boolean chest = mob instanceof net.minecraft.world.entity.animal.equine.AbstractChestedHorse chested
                && chested.hasChest();
        var saddle = mob.getItemBySlot(net.minecraft.world.entity.EquipmentSlot.SADDLE);
        var body = mob.getItemBySlot(net.minecraft.world.entity.EquipmentSlot.BODY);
        return "[" + equineFlags(mob) + "," + mob.getTemper() + "," + Float.floatToRawIntBits(mob.getHealth())
                + "," + mob.getInLoveTime() + "," + mob.getAge() + "," + (chest ? 1 : 0)
                + "," + (saddle.isEmpty() ? 0 : BuiltInRegistries.ITEM.getId(saddle.getItem()))
                + "," + (body.isEmpty() ? 0 : BuiltInRegistries.ITEM.getId(body.getItem()))
                + "," + randomSeed(mob) + "]";
    }

    /**
     * 配对时的 `set_entity_data`（`getNonDefaultValues`），实体 ID 固定成 1，base64。
     * The pairing `set_entity_data` with entity id 1, base64.
     */
    private static String pairingData(Official official, Mob mob) {
        Bytes sink = new Bytes(official.registries);
        sink.add(new net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket(
                1, mob.getEntityData().getNonDefaultValues()));
        return sink.take().get(0);
    }

    /** `AbstractHorse.DATA_ID_FLAGS` 的字节（私有，反射读）。 */
    @SuppressWarnings("unchecked")
    private static int equineFlags(net.minecraft.world.entity.animal.equine.AbstractHorse horse) throws Exception {
        Field field = net.minecraft.world.entity.animal.equine.AbstractHorse.class.getDeclaredField("DATA_ID_FLAGS");
        field.setAccessible(true);
        var accessor = (net.minecraft.network.syncher.EntityDataAccessor<Byte>) field.get(null);
        return horse.getEntityData().get(accessor) & 0xFF;
    }

    private static String equineState(net.minecraft.world.entity.animal.equine.AbstractHorse mob) {
        var a = net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH;
        var b = net.minecraft.world.entity.ai.attributes.Attributes.JUMP_STRENGTH;
        var c = net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED;
        int variant = 0;
        int strength = 0;
        if (mob instanceof net.minecraft.world.entity.animal.equine.Horse horse) {
            variant = horse.getVariant().getId() | (horse.getMarkings().getId() << 8);
        } else if (mob instanceof net.minecraft.world.entity.animal.equine.Llama llama) {
            variant = llama.getVariant().getId();
            strength = llama.getStrength();
        }
        return "[" + Double.doubleToRawLongBits(mob.getAttributeBaseValue(a))
                + "," + Double.doubleToRawLongBits(mob.getAttributeBaseValue(b))
                + "," + Double.doubleToRawLongBits(mob.getAttributeBaseValue(c))
                + "," + variant + "," + strength
                + "," + Float.floatToRawIntBits(mob.getEyeHeight()) + "]";
    }

    private static Mob skeletonSpawn(FakeLevel world, BlockPos pos, boolean egg) {
        if (egg) {
            return EntityTypes.SKELETON.spawn(world, new ItemStack(Items.SKELETON_SPAWN_EGG), null,
                    pos, EntitySpawnReason.SPAWN_ITEM_USE, true, false);
        }
        var mob = EntityTypes.SKELETON.create(world, EntitySpawnReason.COMMAND);
        mob.setPos(0.5, 64.0, 0.5);
        mob.finalizeSpawn(world, world.spawnDifficulty, EntitySpawnReason.COMMAND, null);
        return mob;
    }

    private static String skeletonSpawnEquipment(Official official, ItemStack stack) {
        var registry = official.registries.lookupOrThrow(Registries.ENCHANTMENT);
        String enchantments = net.minecraft.world.item.enchantment.EnchantmentHelper.getEnchantmentsForCrafting(stack)
                .entrySet().stream()
                .sorted(java.util.Comparator.comparingInt(entry -> registry.getId(entry.getKey().value())))
                .map(entry -> "[" + registry.getId(entry.getKey().value()) + "," + entry.getIntValue() + "]")
                .collect(java.util.stream.Collectors.joining(",", "[", "]"));
        return "{\"item\":" + BuiltInRegistries.ITEM.getId(stack.getItem())
                + ",\"enchantments\":" + enchantments + "}";
    }

    private static void playProjectile(Official official, Writer out, String name) throws Exception {
        boolean skeleton = name.startsWith("skeleton_");
        boolean wall = name.equals("arrow_hits_wall");
        boolean overlap = name.equals("arrow_block_overlaps_flight");
        boolean noGravity = name.equals("arrow_no_gravity");
        boolean despawn = name.equals("arrow_despawn");
        boolean flame = name.startsWith("arrow_flame_");
        boolean piercing = name.startsWith("arrow_piercing_");
        boolean water = name.startsWith("arrow_water_");
        boolean waterTail = name.equals("arrow_water_after_pierce");
        boolean rejected = name.endsWith("_rejected");
        boolean hasWall = wall || noGravity || despawn || waterTail;
        Difficulty difficulty = name.endsWith("_hard") ? Difficulty.HARD : Difficulty.NORMAL;
        FakeLevel world = FakeLevel.create(official);
        world.injectedBrightOutside = false;
        world.injectedMonstersBurn = false;
        ((TickingLevelData) world.levelData).difficulty = difficulty;
        var attributes = EnvironmentAttributeSystem.builder().addStaticLayers(world)
                .addConstantLayer(net.minecraft.world.attribute.EnvironmentAttributes.MONSTERS_BURN, v -> false)
                .build();
        official.attributes = attributes;
        put(ServerLevel.class, world, "environmentAttributes", attributes);
        for (int x = -18; x <= 20; x++) {
            for (int z = -12; z <= 12; z++) {
                world.blocks.put(BlockPos.asLong(x, 63, z), Blocks.STONE.defaultBlockState());
            }
        }
        BlockPos obstacle = new BlockPos(7, 65, 0);
        if (hasWall) world.blocks.put(obstacle.asLong(), Blocks.STONE.defaultBlockState());
        if (water) {
            world.blocks.put(BlockPos.asLong(waterTail ? 4 : 3, 65, 0), Blocks.WATER.defaultBlockState());
            world.projectileExtinguishes = new ArrayList<>();
        }
        String blocks = blockArray(world.blocks);
        int playerId = FIRST_ID + 1024;
        ServerPlayer player = new ServerPlayer(official.server, world,
                new GameProfile(new UUID(1L, 2L), "ArrowOracle"), ClientInformation.createDefault());
        player.setId(playerId);
        double playerX = skeleton ? 11.5 : (waterTail || piercing) ? 2.0 : hasWall ? 18.5 : 6.5;
        player.setPos(playerX, 64.0, 0.5);
        setRandom(player, ENTITY_RANDOM_SEED);
        player.setYRot(0.0F);
        player.setYHeadRot(0.0F);
        player.setYBodyRot(0.0F);
        if (flame) {
            player.setRemainingFireTicks(37);
            player.setTicksFrozen(80);
        }
        player.getAbilities().invulnerable = rejected;
        Bytes playerSink = new Bytes(official.registries);
        RecordingListener connection = allocate(RecordingListener.class);
        put(ServerGamePacketListenerImpl.class, connection, "player", player);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class, connection, "server", official.server);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class, connection, "connection",
                new Connection(PacketFlow.CLIENTBOUND));
        connection.bytes = playerSink;
        put(ServerPlayer.class, player, "connection", connection);
        world.playersList().add(player);
        world.tracked.add(player);
        world.nextId = FIRST_ID;
        Mob shooter = null;
        var arrows = new ArrayList<net.minecraft.world.entity.projectile.arrow.AbstractArrow>();
        var sync = new java.util.LinkedHashMap<Integer, Sync>();
        if (skeleton) {
            shooter = new OracleSkeleton(world);
            world.entitySeeds.nextLong();
            setRandom(shooter, 42L);
            shooter.setUUID(new UUID(0, 1));
            shooter.snapTo(0.5, 64.0, 0.5, 0.0F, 0.0F);
            shooter.setYHeadRot(0.0F);
            shooter.setYBodyRot(0.0F);
            shooter.setItemInHand(InteractionHand.MAIN_HAND, new ItemStack(Items.BOW));
            world.tracked.add(shooter);
        } else {
            var arrow = new net.minecraft.world.entity.projectile.arrow.Arrow(EntityTypes.ARROW, world);
            setRandom(arrow, world.entitySeeds.nextLong());
            arrow.setUUID(Mth.createInsecureUUID(arrow.getRandom()));
            arrow.setPos(0.5, water ? 65.25 : piercing ? 65.0 : 65.5, 0.5);
            arrow.setNoGravity(noGravity);
            if (piercing || waterTail) {
                var setPierce = net.minecraft.world.entity.projectile.arrow.AbstractArrow.class
                        .getDeclaredMethod("setPierceLevel", byte.class);
                setPierce.setAccessible(true);
                setPierce.invoke(arrow, (byte)1);
                arrow.setBaseDamage(0.5);
            }
            if (!overlap) arrow.shoot(1.0, 0.0, 0.0, water ? 7.0F : piercing ? 6.0F : 1.6F, 0.0F);
            arrows.add(arrow);
            world.tracked.add(arrow);
        }
        var targets = new ArrayList<Cow>();
        if (piercing) {
            for (double x : new double[] {3.5, 5.5}) {
                var target = EntityTypes.COW.create(world, EntitySpawnReason.COMMAND);
                target.setPos(x, 64.0, 0.5);
                target.setNoAi(true);
                world.tracked.add(target);
                targets.add(target);
            }
        }

        var frames = new StringBuilder("[");
        var playerStates = new StringBuilder("[");
        var playerPackets = new StringBuilder("[");
        float[] lastSentHealth = {-1.0e8F};
        int ticks = skeleton ? 105 : (piercing || water) ? 3 : 26;
        int embeds = 0;
        int extinguishes = 0;
        for (int tick = 0; tick <= ticks; tick++) {
            var arrowPackets = new ArrayList<String>();
            if (tick == 0 && !skeleton) pairArrow(world, official, arrows.getFirst(), sync, arrowPackets);
            if (tick > 0) {
                world.gameTime++;
                if ((flame || water) && tick == 1) arrows.getFirst().setRemainingFireTicks(200);
                if (wall && tick == 12) world.blocks.remove(obstacle.asLong());
                if (overlap && tick == 2) world.blocks.put(BlockPos.asLong(0, 65, 0), Blocks.STONE.defaultBlockState());
                if (noGravity && tick == 10) arrows.getFirst().setNoGravity(false);
                if (despawn && tick == 6) {
                    putInt(net.minecraft.world.entity.projectile.arrow.AbstractArrow.class, arrows.getFirst(), "life", 1198);
                }
                if (player.damageCooldownTime > 0) player.damageCooldownTime--;
                if (player.hurtTime > 0) player.hurtTime--;
                if (shooter != null) {
                    shooter.checkDespawn();
                    shooter.commonTick();
                    shooter.tick();
                }
                for (var arrow : arrows) {
                    if (arrow.isRemoved()) continue;
                    arrow.commonTick();
                    arrow.tick();
                    if (arrow.isRemoved()) {
                        Bytes gone = new Bytes(official.registries);
                        gone.add(new net.minecraft.network.protocol.game.ClientboundRemoveEntitiesPacket(arrow.getId()));
                        arrowPackets.addAll(gone.take());
                        sync.remove(arrow.getId());
                    }
                }
                world.tracked.removeIf(Entity::isRemoved);
                for (Entity entity : world.spawned) {
                    if (entity instanceof net.minecraft.world.entity.projectile.arrow.AbstractArrow arrow) {
                        arrows.add(arrow);
                        world.tracked.add(arrow);
                        pairArrow(world, official, arrow, sync, arrowPackets);
                    }
                }
                world.spawned.clear();
                for (Sync tracking : sync.values()) {
                    tracking.entity.sendChanges();
                    arrowPackets.addAll(tracking.take());
                }
                frames.append(',');
            }
            frames.append("{\"arrows\":[");
            boolean first = true;
            var ground = net.minecraft.world.entity.projectile.arrow.AbstractArrow.class.getDeclaredMethod("isInGround");
            ground.setAccessible(true);
            for (var arrow : arrows) {
                if (arrow.isRemoved()) continue;
                if (!first) frames.append(',');
                first = false;
                boolean embedded = (boolean) ground.invoke(arrow);
                if (embedded) embeds++;
                frames.append("{\"id\":").append(arrow.getId());
                frames.append(",\"motion\":").append(projectileMotion(arrow));
                frames.append(",\"ground\":").append(embedded).append(",\"shake\":").append(arrow.shakeTime);
                frames.append(",\"seed\":").append(randomSeed(arrow));
                if (water) frames.append(",\"fire\":").append(arrow.getRemainingFireTicks());
                frames.append('}');
            }
            frames.append("],\"packets\":").append(arrowPackets.stream().map(MobOracle::jsonString)
                    .collect(java.util.stream.Collectors.joining(",", "[", "]")));
            if (shooter != null) {
                frames.append(",\"shooter\":").append(projectileMotion(shooter));
                frames.append(",\"shooterSeed\":").append(randomSeed(shooter));
                frames.append(",\"aggressive\":").append(shooter.isAggressive());
                frames.append(",\"drawing\":").append(shooter.isUsingItem());
            }
            if (flame || water) {
                frames.append(",\"playerThermal\":[").append(player.getRemainingFireTicks())
                        .append(',').append(player.getTicksFrozen()).append(']');
            }
            if (water) {
                extinguishes += world.projectileExtinguishes.size();
                frames.append(",\"extinguished\":").append(world.projectileExtinguishes.stream()
                        .map(Arrays::toString).collect(java.util.stream.Collectors.joining(",", "[", "]")));
                world.projectileExtinguishes.clear();
            }
            if (piercing) {
                frames.append(",\"targets\":").append(Arrays.toString(targets.stream()
                        .mapToInt(target -> Float.floatToRawIntBits(target.getHealth())).toArray()));
            }
            frames.append('}');
            zombiePlayerStep(player, playerSink, playerStates, playerPackets, lastSentHealth, tick == 0);
        }
        if (name.equals("arrow_hits_player") && player.getHealth() == 20.0F
                || hasWall && embeds == 0 || overlap && embeds != ticks - 1 || skeleton && arrows.size() < 2
                || despawn && (embeds < 2 || !arrows.getFirst().isRemoved())) {
            throw new IllegalStateException("projectile scenario failed to exercise " + name);
        }
        if (flame && (player.getTicksFrozen() != 0
                || player.getRemainingFireTicks() != (rejected ? 1 : 100)
                || (player.getHealth() == 20.0F) != rejected)) {
            throw new IllegalStateException("flame scenario did not exercise ignition: " + name
                    + " health=" + player.getHealth() + " fire=" + player.getRemainingFireTicks()
                    + " frozen=" + player.getTicksFrozen());
        }
        if (piercing && ((player.getHealth() == 20.0F) != rejected
                || targets.getFirst().getHealth() != (rejected ? 9.0F : 7.0F)
                || targets.getLast().getHealth() != 10.0F || !arrows.getFirst().isRemoved())) {
            throw new IllegalStateException("piercing scenario did not exercise the hit cap: " + name);
        }
        if (water && (player.getHealth() == 20.0F || extinguishes != 1
                || player.getRemainingFireTicks() != (waterTail ? 100 : 0)
                || arrows.getFirst().getRemainingFireTicks() != 0)) {
            throw new IllegalStateException("water scenario missed the expected contact: " + name
                    + " health=" + player.getHealth() + " sounds=" + extinguishes
                    + " player_fire=" + player.getRemainingFireTicks()
                    + " arrow_fire=" + arrows.getFirst().getRemainingFireTicks());
        }
        out.write("{\"type\":\"projectile\",\"name\":" + jsonString(name) + ",\"difficulty\":" + difficulty.getId()
                + ",\"wall\":" + wall + ",\"skeleton\":" + skeleton + ",\"firstId\":" + FIRST_ID
                + ",\"playerId\":" + playerId + ",\"playerX\":" + playerX + ",\"blocks\":" + blocks
                + ",\"shots\":" + arrows.size() + ",\"embeds\":" + embeds + ",\"frames\":" + frames + "]"
                + ",\"playerStates\":" + playerStates + "],\"playerPackets\":" + playerPackets + "]}\n");
    }

    private static void pairArrow(FakeLevel world, Official official, Entity arrow,
            java.util.Map<Integer, Sync> sync, List<String> packets) {
        Sync tracking = new Sync(world, arrow, official.registries);
        sync.put(arrow.getId(), tracking);
        tracking.entity.sendPairingData(null, tracking.bytes::add);
        packets.addAll(tracking.take());
    }

    private static String projectileMotion(Entity entity) {
        Vec3 pos = entity.position();
        Vec3 v = entity.getDeltaMovement();
        return Arrays.toString(new long[] {
                Double.doubleToRawLongBits(pos.x), Double.doubleToRawLongBits(pos.y), Double.doubleToRawLongBits(pos.z),
                Double.doubleToRawLongBits(v.x), Double.doubleToRawLongBits(v.y), Double.doubleToRawLongBits(v.z),
                Float.floatToRawIntBits(entity.getYRot()), Float.floatToRawIntBits(entity.getXRot()),
        });
    }

    /** 僵尸场景：官方 `Mob` 逐刻 tick，输出与 `CowStep` 同形的状态加僵尸专属字段。 */
    private static void playZombie(Official official, Writer out, Scene scene) throws Exception {
        FakeLevel level = FakeLevel.create(official);
        level.blocks.putAll(scene.blocks);
        level.injectedSkyDarken = scene.skyDarken;
        level.injectedRaining = scene.raining;
        level.injectedBrightOutside = scene.brightOutside;
        level.injectedMonstersBurn = scene.monstersBurn;
        level.injectedCanSeeSky = scene.monstersBurn;
        // `EnvironmentAttributes.MONSTERS_BURN` 的注册默认值是 false，
        // 假世界没有主世界的 timeline 层；用一层常量显式注入场景值，
        // 两边读的是同一个布尔，白天燃烧才会真的发生。
        if (scene.clockStart >= 0) {
            // 真实时间：主世界的维度层、群系层、时间线层和天气层，时钟由场景逐刻推进。
            level.realSky = true;
            level.clock = scene.clockStart;
            level.rainLevel = scene.rain;
            EnvironmentAttributeSystem.Builder builder = EnvironmentAttributeSystem.builder()
                    .addStaticLayers(level);
            net.minecraft.world.clock.ClockManager clocks = clock -> new net.minecraft.world.clock.ClockInstance() {
                public long totalTicks() { return level.clock; }
                public float partialTick() { return 0.0F; }
                public float rate() { return 1.0F; }
                public boolean isPaused() { return false; }
            };
            for (Holder<net.minecraft.world.timeline.Timeline> timeline : level.dimensionType().timelines()) {
                builder.addTimelineLayer(timeline, clocks);
            }
            net.minecraft.world.attribute.WeatherAttributes.addBuiltinLayers(builder,
                    net.minecraft.world.attribute.WeatherAttributes.WeatherAccess.from(level));
            official.attributes = builder.build();
            level.updateSkyBrightness();
        } else {
            official.attributes = EnvironmentAttributeSystem.builder()
                    .addStaticLayers(level)
                    .addConstantLayer(
                            net.minecraft.world.attribute.EnvironmentAttributes.MONSTERS_BURN,
                            v -> scene.monstersBurn)
                    .build();
        }
        ((TickingLevelData) level.levelData).difficulty = Difficulty.values()[scene.difficulty];
        level.nextId = FIRST_ID;
        java.util.List<ServerPlayer> players = new java.util.ArrayList<>();
        java.util.List<Bytes> zombieSinks = new java.util.ArrayList<>();
        StringBuilder playerStates = new StringBuilder("[");
        StringBuilder playerPackets = new StringBuilder("[");
        float[] lastSentHealth = {-1.0e8F};
        for (int i = 0; i < scene.players.size(); i++) {
            double[] at = scene.players.get(i);
            ServerPlayer player = new ServerPlayer(official.server, level,
                    new GameProfile(new UUID(1L, 2L + i), "ZombieOracle" + i),
                    ClientInformation.createDefault());
            player.setPos(at[0], at[1], at[2]);
            setRandom(player, 0x0DDB1A5E5BAD5EEDL + i);
            player.setYRot(0.0F);
            player.setYHeadRot(0.0F);
            player.setYBodyRot(0.0F);
            player.setItemInHand(InteractionHand.MAIN_HAND, new ItemStack(scene.playerItems.get(i)));
            if (scene.realPlayers) {
                player.setHealth(scene.playerHealth);
            }
            // `ServerPlayer.isInvulnerableTo` 会读 `connection.hasClientLoaded()`；
            // 僵尸近战必须有一个连接桩，包字节本身在这个场景里不比。
            Bytes sink = new Bytes(official.registries);
            ServerGamePacketListenerImpl connection = allocate(RecordingListener.class);
            put(ServerGamePacketListenerImpl.class, connection, "player", player);
            put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                    connection, "server", official.server);
            put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                    connection, "connection", new Connection(PacketFlow.CLIENTBOUND));
            ((RecordingListener) connection).bytes = sink;
            put(ServerPlayer.class, player, "connection", connection);
            players.add(player);
            zombieSinks.add(sink);
        }
        level.playersList().addAll(players);
        // 实体事件和 `sendToTrackingPlayers` 的包：这里只比装备和实体事件两种。
        Bytes mobSink = new Bytes(official.registries);
        level.mobPackets = mobSink::add;
        StringBuilder mobEvents = new StringBuilder("[[");
        java.util.List<Mob> mobs = new java.util.ArrayList<>();
        for (int i = 0; i < scene.cows.size(); i++) {
            double[] at = scene.cows.get(i);
            int[] kind = i < scene.mobKinds.size() ? scene.mobKinds.get(i) : new int[] {1};
            // `EntityType.create` 在和平难度会拒绝敌对生物；场景要能先生成再
            // 观察 `checkDespawn` 的和平清除，所以直接调构造函数。
            Mob mob = switch (kind[0]) {
                case 2 -> new Husk(EntityTypes.HUSK, level);
                case 1 -> new Zombie(EntityTypes.ZOMBIE, level);
                case 4 -> EntityTypes.HORSE.create(level, EntitySpawnReason.COMMAND);
                case 5 -> EntityTypes.DONKEY.create(level, EntitySpawnReason.COMMAND);
                case 6 -> EntityTypes.MULE.create(level, EntitySpawnReason.COMMAND);
                case 7 -> EntityTypes.LLAMA.create(level, EntitySpawnReason.COMMAND);
                default -> EntityTypes.COW.create(level, EntitySpawnReason.COMMAND);
            };
            level.entitySeeds.nextLong();
            setRandom(mob, scene.seeds.get(i));
            // Bamboo 的实体表从 `firstId` 发号；官方这边显式对齐 ID，
            // `Mob.serverAiStep` 的 `(tickCount + id) % 2` 才能逐刻一致。
            mob.setId(FIRST_ID + i);
            mob.setUUID(new UUID(0L, i + 1));
            mob.snapTo(at[0], at[1], at[2], scene.yaws.get(i)[0], 0.0F);
            mob.setYHeadRot(scene.yaws.get(i)[0]);
            mob.setYBodyRot(scene.yaws.get(i)[0]);
            if (scene.fireTicks > 0) mob.setRemainingFireTicks(scene.fireTicks);
            if (mob instanceof Zombie zombie) {
                zombie.setBaby(scene.cowAges.get(i) != 0);
                zombie.setCanBreakDoors(scene.difficulty == 3);
            } else if (mob instanceof net.minecraft.world.entity.AgeableMob ageable && scene.cowAges.get(i) != 0) {
                ageable.setAge(scene.cowAges.get(i));
            }
            if (scene.helmet != null) {
                ItemStack helmet = new ItemStack(scene.helmet);
                if (scene.helmetDamage != 0) helmet.setDamageValue(scene.helmetDamage);
                mob.setItemSlot(net.minecraft.world.entity.EquipmentSlot.HEAD, helmet);
            }
            // `Mob.finalizeSpawn` 的取数顺序按官方；场景关掉时和 Bamboo 的
            // 显式构造（不跑 finalize）一致。
            mobs.add(mob);
            level.tracked.add(mob);
        }
        StringBuilder line = new StringBuilder(1 << 18);
        line.append("{\"type\":\"case\",\"name\":").append(jsonString(scene.name));
        line.append(",\"firstId\":").append(FIRST_ID);
        line.append(",\"startTime\":").append(START_TIME);
        line.append(",\"blocks\":").append(blockArray(scene.blocks));
        line.append(",\"cows\":[");
        for (int i = 0; i < scene.cows.size(); i++) {
            if (i > 0) line.append(',');
            double[] at = scene.cows.get(i);
            int[] kind = i < scene.mobKinds.size() ? scene.mobKinds.get(i) : new int[] {1};
            line.append("{\"x\":").append(Double.doubleToRawLongBits(at[0]));
            line.append(",\"y\":").append(Double.doubleToRawLongBits(at[1]));
            line.append(",\"z\":").append(Double.doubleToRawLongBits(at[2]));
            line.append(",\"yaw\":").append(Float.floatToRawIntBits(scene.yaws.get(i)[0]));
            line.append(",\"seed\":").append(scene.seeds.get(i));
            line.append(",\"age\":").append(scene.cowAges.get(i));
            line.append(",\"kind\":").append(kind[0]).append('}');
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
            line.append(",\"item\":0,\"looting\":0}");
        }
        // 玩家打生物的时刻、对象、伤害量和打的人，和牛场景同一格式。
        // When, whom, how hard and by whom players hit, as in the cow scenes.
        line.append("],\"hurts\":[");
        for (int i = 0; i < scene.hurts.size(); i++) {
            if (i > 0) line.append(',');
            double[] hurt = scene.hurts.get(i);
            line.append('[').append((int) hurt[0]).append(',').append((int) hurt[1]).append(',')
                    .append(Float.floatToRawIntBits((float) hurt[2])).append(',').append((int) hurt[3]).append(']');
        }
        line.append("],\"summons\":[],\"eggs\":[]");
        line.append(",\"difficulty\":").append(scene.difficulty);
        line.append(",\"skyDarken\":").append(scene.skyDarken);
        line.append(",\"raining\":").append(scene.raining);
        line.append(",\"brightOutside\":").append(scene.brightOutside);
        line.append(",\"monstersBurn\":").append(scene.monstersBurn);
        line.append(",\"deathKind\":0,\"fireTicks\":").append(scene.fireTicks);
        line.append(",\"playerHealth\":").append(Float.floatToRawIntBits(
                scene.realPlayers ? scene.playerHealth : 20.0F));
        line.append(",\"realPlayers\":").append(scene.realPlayers);
        line.append(",\"clockStart\":").append(scene.clockStart);
        line.append(",\"rain\":").append(Float.floatToRawIntBits(scene.rain));
        line.append(",\"helmet\":").append(scene.helmet == null ? 0
                : net.minecraft.core.registries.BuiltInRegistries.ITEM.getId(scene.helmet));
        line.append(",\"helmetDamage\":").append(scene.helmetDamage);
        line.append(",\"playerAbsorption\":0,\"playerExhaustion\":0");
        line.append(",\"playerSteps\":null,\"playerPackets\":null,\"keyPackets\":null");
        line.append(",\"interactActions\":[],\"interactions\":null");
        line.append(",\"packets\":null,\"syncFlags\":null,\"spawns\":null,\"life\":null,\"spawnResult\":null");
        line.append(",\"steps\":[");
        line.append(zombieStepJson(mobs, players, level));
        if (scene.realPlayers) {
            zombiePlayerStep(players.get(0), zombieSinks.get(0), playerStates, playerPackets,
                    lastSentHealth, true);
        }
        for (int tick = 1; tick <= scene.ticks; tick++) {
            level.gameTime++;
            if (level.realSky) {
                level.clock = scene.clockStart + tick;
                official.attributes.invalidateTickCache();
                level.updateSkyBrightness();
            }
            if (scene.realPlayers) {
                // 只推进受伤冷却和受伤动画（`ServerPlayer.tick` / `baseTick`），
                // 不跑物理、饥饿和回血；Bamboo 一侧是 `PlayerHealth::tick_cooldowns`。
                for (ServerPlayer player : players) {
                    if (player.damageCooldownTime > 0) player.damageCooldownTime--;
                    if (player.hurtTime > 0) player.hurtTime--;
                }
            }
            for (double[] hurt : scene.hurts) {
                if ((int) hurt[0] == tick && (int) hurt[1] < mobs.size()) {
                    Mob target = mobs.get((int) hurt[1]);
                    ServerPlayer attacker = players.isEmpty() ? null : players.get((int) hurt[3]);
                    if (attacker != null) {
                        target.hurtServer(level, level.damageSources().playerAttack(attacker),
                                (float) hurt[2]);
                    }
                }
            }
            for (Mob mob : new java.util.ArrayList<>(mobs)) {
                if (mob.isRemoved()) continue;
                mob.checkDespawn();
                mob.commonTick();
                mob.tick();
            }
            level.tracked.removeIf(Entity::isRemoved);
            appendMobEvents(mobEvents, mobSink.takeRaw());
            line.append(',').append(zombieStepJson(mobs, players, level));
            if (scene.realPlayers) {
                zombiePlayerStep(players.get(0), zombieSinks.get(0), playerStates, playerPackets,
                        lastSentHealth, false);
            }
        }
        line.append("]");
        line.append(",\"mobEventPackets\":").append(mobEvents).append("]]");
        if (scene.realPlayers) {
            line.append(",\"zombiePlayerStates\":").append(playerStates).append(']');
            line.append(",\"zombiePlayerPackets\":").append(playerPackets).append(']');
        }
        line.append("}\n");
        out.write(line.toString());
    }

    /** `entity_event`（34）和 `set_equipment`（104）。 */
    private static final java.util.Set<Integer> ZOMBIE_MOB_PACKETS = java.util.Set.of(34, 104);

    /** 把这一刻生物自己的那两种包按 base64 追加成 JSON 数组的一项（第 0 刻是空表）。 */
    private static void appendMobEvents(StringBuilder out, List<byte[]> packets) {
        out.append("],[");
        boolean any = false;
        for (byte[] body : packets) {
            if (body.length == 0 || !ZOMBIE_MOB_PACKETS.contains(body[0] & 0xFF)) continue;
            if (any) out.append(',');
            any = true;
            out.append('"').append(java.util.Base64.getEncoder().encodeToString(body)).append('"');
        }
    }

    /** 玩家自己收到的包里要比的那几种：受伤动画、进入 / 离开战斗、死亡界面、血量。 */
    private static final java.util.Set<Integer> ZOMBIE_SELF_PACKETS = java.util.Set.of(
            43, 67, 68, 69, 106);

    /**
     * 僵尸场景一刻结束时：按 `ServerPlayer.doTick` 补 `set_health`，记下玩家状态
     * （血量位、受伤冷却、受伤动画、死亡、着火刻数）和这一刻要比的包。
     */
    private static void zombiePlayerStep(ServerPlayer player, Bytes sink, StringBuilder states,
            StringBuilder packets, float[] lastSentHealth, boolean first) {
        float health = player.getHealth();
        if (!first && Float.floatToRawIntBits(health) != Float.floatToRawIntBits(lastSentHealth[0])) {
            lastSentHealth[0] = health;
            sink.add(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(
                    health, player.getFoodData().getFoodLevel(),
                    player.getFoodData().getSaturationLevel()));
        }
        if (first) {
            lastSentHealth[0] = health;
        }
        if (!first) {
            states.append(',');
            packets.append(',');
        }
        states.append('[').append(Float.floatToRawIntBits(health))
                .append(',').append(player.damageCooldownTime)
                .append(',').append(player.hurtTime)
                .append(',').append(player.isAlive() ? 0 : 1)
                .append(',').append(player.getRemainingFireTicks()).append(']');
        packets.append('[');
        boolean comma = false;
        for (byte[] body : sink.takeRaw()) {
            if (body.length == 0 || !ZOMBIE_SELF_PACKETS.contains((int) body[0])) continue;
            if (comma) packets.append(',');
            packets.append('"').append(Base64.getEncoder().encodeToString(body)).append('"');
            comma = true;
        }
        packets.append(']');
    }

    /** 与 `CowStep` 同形；牛专属字段给缺省值，僵尸专属字段逐刻输出。 */
    private static String zombieStepJson(java.util.List<Mob> mobs,
            java.util.List<ServerPlayer> players, FakeLevel level) throws Exception {
        StringBuilder out = new StringBuilder(512);
        out.append('[');
        long lr = levelSeed(level);
        for (int i = 0; i < mobs.size(); i++) {
            if (i > 0) out.append(',');
            Mob mob = mobs.get(i);
            Vec3 pos = mob.position();
            Vec3 delta = mob.getDeltaMovement();
            out.append("{\"x\":").append(Double.doubleToRawLongBits(pos.x));
            out.append(",\"y\":").append(Double.doubleToRawLongBits(pos.y));
            out.append(",\"z\":").append(Double.doubleToRawLongBits(pos.z));
            out.append(",\"dx\":").append(Double.doubleToRawLongBits(delta.x));
            out.append(",\"dy\":").append(Double.doubleToRawLongBits(delta.y));
            out.append(",\"dz\":").append(Double.doubleToRawLongBits(delta.z));
            out.append(",\"yr\":").append(Float.floatToRawIntBits(mob.getYRot()));
            out.append(",\"xr\":").append(Float.floatToRawIntBits(mob.getXRot()));
            out.append(",\"hr\":").append(Float.floatToRawIntBits(mob.getYHeadRot()));
            out.append(",\"br\":").append(Float.floatToRawIntBits(mob.yBodyRot));
            out.append(",\"hp\":").append(Float.floatToRawIntBits(mob.getHealth()));
            out.append(",\"g\":").append(mob.onGround());
            out.append(",\"rm\":").append(mob.isRemoved());
            out.append(",\"w\":").append(mob.isInWater());
            out.append(",\"fd\":").append(Double.doubleToRawLongBits(mob.fallDistance));
            out.append(",\"nat\":").append(mob.getNoActionTime());
            out.append(",\"ht\":").append(mob.hurtTime);
            out.append(",\"dt\":").append(mob.deathTime);
            out.append(",\"fire\":").append(mob.getRemainingFireTicks());
            out.append(",\"air\":").append(mob.getAirSupply());
            out.append(",\"seed\":").append(randomSeed(mob));
            out.append(",\"age\":").append(
                    mob instanceof net.minecraft.world.entity.AgeableMob ageable ? ageable.getAge() : 0);
            out.append(",\"love\":0,\"lh\":").append(mob.isLeftHanded());
            out.append(",\"fr\":").append(Double.doubleToRawLongBits(
                    mob.getAttributeValue(net.minecraft.world.entity.ai.attributes.Attributes.FOLLOW_RANGE)));
            out.append(",\"var\":\"minecraft:temperate\",\"snd\":\"minecraft:classic\"");
            out.append(",\"uuid\":\"").append(mob.getUUID()).append('"');
            out.append(",\"lr\":").append(lr);
            out.append(",\"target\":").append(mob.getTarget() == null ? -1 : mob.getTarget().getId());
            out.append(",\"baby\":").append(mob.isBaby());
            if (mob instanceof net.minecraft.world.entity.animal.equine.AbstractHorse horse) {
                out.append(",\"eq\":").append(equineFlags(horse));
                out.append(",\"tmp\":").append(horse.getTemper());
            }
            out.append(",\"aggr\":").append(mob.isAggressive());
            out.append(",\"doors\":").append(mob instanceof Zombie z2 && z2.canBreakDoors());
            out.append(",\"ph\":0");
            out.append(",\"pht\":0");
            out.append(",\"pcd\":0");
            out.append(",\"bh\":").append(blockHash(level));
            ItemStack head = mob.getItemBySlot(net.minecraft.world.entity.EquipmentSlot.HEAD);
            out.append(",\"hd\":").append(head.isEmpty() ? -1 : head.getDamageValue());
            out.append(",\"hc\":").append(head.getCount());
            out.append(",\"armor\":").append(Double.doubleToRawLongBits(
                    mob.getAttributeValue(net.minecraft.world.entity.ai.attributes.Attributes.ARMOR)));
            out.append(",\"goals\":\"");
            boolean first = true;
            for (WrappedGoal goal : mob.getGoalSelector().getAvailableGoals()) {
                if (!goal.isRunning()) continue;
                if (!first) out.append(' ');
                first = false;
                // `AbstractHorse.MountPanicGoal` 没被生物控制时就是 `PanicGoal`，名字按后者记。
                // `MountPanicGoal` is a plain `PanicGoal` without a mob controller.
                String goalName = goal.getGoal().getClass().getSimpleName();
                out.append("MountPanicGoal".equals(goalName) ? "PanicGoal" : goalName);
            }
            out.append('"');
            net.minecraft.world.level.pathfinder.Path path = mob.getNavigation().getPath();
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
        return out.append(']').toString();
    }

    /** 1c 落脚点：在假世界里跑官方 `PlayerSpawnFinder.findSpawn`，用固定随机源逐位对拍。 */
    private static void playSpawn(Official official, Writer out, Scene scene) throws Exception {
        FakeLevel level = FakeLevel.create(official);
        level.blocks.putAll(scene.blocks);
        // 让假世界的区块把方块写进 LevelChunkSection 后 prime 官方高度图，
        // `PlayerSpawnFinder.getLevelRespawnPos` 读的就是这几张表。
        for (int cx = -3; cx <= 3; cx++) {
            for (int cz = -3; cz <= 3; cz++) {
                LevelChunk chunk = (LevelChunk) level.getChunk(cx, cz, ChunkStatus.FULL, false);
                Heightmap.primeHeightmaps(chunk, ChunkStatus.FINAL_HEIGHTMAPS);
            }
        }
        setRule(official, GameRules.RESPAWN_RADIUS, scene.spawnRadius);
        official.server.spawnGameType = GameType.values()[scene.spawnGameType];
        BlockPos spawn = new BlockPos(scene.spawnAt[0], scene.spawnAt[1], scene.spawnAt[2]);
        // 官方 `findSpawn` 的构造会取 `ThreadLocalRandom.current().nextLong()`；
        // 对拍把这个线程本地随机源钉住，再直接用同一固定种子重放候选序列，
        // 最终位置由官方 `PlayerSpawnFinder` 自己算出来。
        Field rnd = ThreadLocalRandom.class.getDeclaredField("rnd");
        rnd.setAccessible(true);
        ThreadLocalRandom tlr = ThreadLocalRandom.current();
        long baseSeed = scene.spawnSeed;

        // `MinecraftServer.getServer()` 的 execute / managedBlock 桩直接跑任务；
        // `Chunks.addTicketAndLoadWithRadius` 立刻完成（假世界区块全在内存里）。
        // 反射构造官方 `PlayerSpawnFinder`，再把它的 `offset` 字段换成
        // 「固定种子的 `SingleThreadedRandomSource.nextInt(count)`」的结果，
        // 这样既不依赖 `ThreadLocalRandom` 的线程，也逐位复刻官方候选序列。
        int borderDistance = net.minecraft.util.Mth.floor(
                level.getWorldBorder().getDistanceToBorder(spawn.getX(), spawn.getZ()));
        int radius = Math.min(Math.max(0, scene.spawnRadius), borderDistance);
        if (borderDistance <= 1) radius = 1;
        int count = Math.min(1024, (radius * 2 + 1) * (radius * 2 + 1));
        // 纯 LCG 算出官方 `SingleThreadedRandomSource.setSeed(seed)` 的 48 位
        // 状态和 `nextInt(count)` 后的末态，不碰任何线程本地随机源；再把这个
        // offset 反射写进官方 `PlayerSpawnFinder`，候选顺序逐位可复现。
        final long[] state = {(baseSeed ^ 0x5DEECE66DL) & ((1L << 48) - 1)};
        java.util.function.IntUnaryOperator nextBits = bits -> {
            state[0] = (state[0] * 0x5DEECE66DL + 0xBL) & ((1L << 48) - 1);
            return (int) (state[0] >>> (48 - bits));
        };
        int offset;
        if ((count & (count - 1)) == 0) {
            offset = (int) (((long) count * nextBits.applyAsInt(31)) >>> 31);
        } else {
            while (true) {
                int bits = nextBits.applyAsInt(31);
                int value = bits % count;
                if (bits - value + (count - 1) >= 0) {
                    offset = value;
                    break;
                }
            }
        }
        long sourceSeed = state[0];
        java.lang.reflect.Constructor<PlayerSpawnFinder> ctor =
                PlayerSpawnFinder.class.getDeclaredConstructor(
                        net.minecraft.server.level.ServerLevel.class, BlockPos.class, int.class);
        ctor.setAccessible(true);
        // 构造前才把 `rnd` 钉成场景种子，确保没有别的代码先取数。
        rnd.setLong(tlr, baseSeed);
        PlayerSpawnFinder finder = ctor.newInstance(level, spawn, radius);
        Field offsetField = PlayerSpawnFinder.class.getDeclaredField("offset");
        offsetField.setAccessible(true);
        offsetField.setInt(finder, offset);
        Vec3 position;
        if (official.server.spawnGameType == GameType.ADVENTURE) {
            // `findSpawn` 的冒险模式分支：直接 `fixupSpawnHeight`，不散布。
            java.lang.reflect.Method fixup = PlayerSpawnFinder.class.getDeclaredMethod(
                    "fixupSpawnHeight", net.minecraft.world.level.CollisionGetter.class,
                    BlockPos.class);
            fixup.setAccessible(true);
            position = (Vec3) fixup.invoke(null, level, spawn);
        } else {
            java.lang.reflect.Method schedule =
                    PlayerSpawnFinder.class.getDeclaredMethod("scheduleNext");
            schedule.setAccessible(true);
            schedule.invoke(finder);
            Field done = PlayerSpawnFinder.class.getDeclaredField("finishedFuture");
            done.setAccessible(true);
            @SuppressWarnings("unchecked")
            CompletableFuture<Vec3> future = (CompletableFuture<Vec3>) done.get(finder);
            position = future.join();
        }

        // 官方 `findRespawnAndUseSpawnBlock` 的 forced `/spawnpoint` 分支：
        // 两格 `isPossibleToRespawnInThis` 时直接落地，否则回退到世界出生点。
        boolean missing = false;
        if (scene.personalAt != null) {
            BlockPos at = new BlockPos(scene.personalAt[0], scene.personalAt[1], scene.personalAt[2]);
            boolean feet = level.getBlockState(at).getBlock()
                    .isPossibleToRespawnInThis(level.getBlockState(at));
            BlockPos above = at.above();
            boolean head = level.getBlockState(above).getBlock()
                    .isPossibleToRespawnInThis(level.getBlockState(above));
            if (feet && head) {
                position = new Vec3(at.getX() + 0.5, at.getY() + 0.1, at.getZ() + 0.5);
            } else {
                missing = true;
                // 世界出生点的搜索就是上面 findSpawn 的结果。
            }
        }


        StringBuilder line = new StringBuilder(1 << 16);
        line.append("{\"type\":\"case\",\"name\":").append(jsonString(scene.name));
        line.append(",\"firstId\":").append(FIRST_ID);
        line.append(",\"startTime\":").append(START_TIME);
        line.append(",\"blocks\":").append(blockArray(scene.blocks));
        line.append(",\"cows\":[],\"players\":[],\"hurts\":[],\"summons\":[],\"eggs\":[]");
        line.append(",\"steps\":[[]],\"packets\":null,\"syncFlags\":null,\"spawns\":null");
        line.append(",\"deathKind\":0,\"fireTicks\":0,\"interactActions\":[],\"interactions\":null");
        line.append(",\"playerSteps\":null,\"playerPackets\":null,\"keyPackets\":null");
        line.append(",\"spawnResult\":{\"x\":").append(Double.doubleToRawLongBits(position.x));
        line.append(",\"y\":").append(Double.doubleToRawLongBits(position.y));
        line.append(",\"z\":").append(Double.doubleToRawLongBits(position.z));
        line.append(",\"radius\":").append(scene.spawnRadius);
        line.append(",\"seed\":").append(scene.spawnSeed);
        line.append(",\"offset\":").append(offset);
        line.append(",\"randomState\":").append(sourceSeed);
        line.append(",\"gameType\":").append(scene.spawnGameType);
        line.append(",\"personal\":");
        if (scene.personalAt == null) {
            line.append("null");
        } else {
            line.append('[').append(scene.personalAt[0]).append(',').append(scene.personalAt[1])
                    .append(',').append(scene.personalAt[2]).append(']');
        }
        line.append(",\"missing\":").append(missing);
        line.append(",\"spawnAt\":[").append(scene.spawnAt[0]).append(',').append(scene.spawnAt[1])
                .append(',').append(scene.spawnAt[2]).append(']');
        line.append("}}\n");
        out.write(line.toString());
        out.flush();
    }

    /**
     * 死亡 / 重生 / 回血场景：在假世界里跑官方 `ServerPlayer.die`、
     * `PlayerList.respawn`、`FoodData.tick` 和 `ServerPlayer.tickRegeneration`，
     * 逐刻记下玩家状态、包字节、掉落物和经验球。
     *
     * <p>不启动真实服务端：`PlayerList.respawn` 里会碰到的
     * `MinecraftServer.getLevel`、`clockManager`、`getProfilePermissions`、
     * `ServerLevel.addRespawnedPlayer` 和 `connection.teleport` 都由
     * {@code FakeServer} / {@code FakeLevel} / {@code RecordingListener} 的桩接管。
     */
    /**
     * S2 玩家效果场景：在假世界里对官方 `ServerPlayer` 直接调
     * `LivingEntity.addEffect` / `removeEffect` / `removeAllEffects`，每刻用
     * `MobEffectInstance.tickServer` 的等价入口推进（反射调 `LivingEntity.tickEffects`），
     * 逐刻记下 `activeEffects` 全字段、血量 / 吸收 / 饥饿 / 随机源和
     * `connection.send` 抓到的包字节。
     *
     * <p>不跑完整的 `ServerPlayer.tick()`：假世界缺连接层 / 区块票 / 推进系统，
     * 而 `tickEffects`、伤害入口和 `onEffect*` 的包链路都是独立的。本场景比较的
     * 字段只由这几段决定，不跑的部分不影响它们。
     */
    /** 取 `remove_mob_effect` 包里的效果 ID（VarInt 79 / 实体 ID / 效果 ID）。 */
    private static int effectPacketId(byte[] body) {
        int at = 0;
        while (at < body.length && (body[at] & 0x80) != 0) at++;
        at++;
        while (at < body.length && (body[at] & 0x80) != 0) at++;
        at++;
        int value = 0;
        int shift = 0;
        while (at < body.length) {
            value |= (body[at] & 0x7f) << shift;
            if ((body[at] & 0x80) == 0) break;
            shift += 7;
            at++;
        }
        return value;
    }

    /**
     * S3 环境伤害场景：在假世界里直接跑官方 `Entity.baseTick` /
     * `LivingEntity.baseTick`（反射 `baseTick`，它是 public）。假世界不跑
     * `ServerPlayer.tick()` 的其余部分（连接层、区块票、推进系统），但
     * `baseTick` 本身需要的 `level()`、`damageSources()`、`isInWall()`、
     * `Entity.checkInsideBlocks` 都由桩提供；本场景比较的字段只由它决定。
     * 逐刻记下血量 / 吸收 / 冷却 / 饥饿 / 着火 / 氧气 / 冻结 / 随机源，以及
     * `connection.send` 和假世界广播抓到的包字节。
     */
    private static void playEnv(Official official, Writer out, Scene scene) throws Exception {
        Bytes all = new Bytes(official.registries);
        StubList list = (StubList) official.server.getPlayerList();
        official.server.sequences = new RandomSequences();
        FakeLevel level = FakeLevel.create(official);
        level.chunkSource.events = all::add;
        level.packets = all::add;
        list.sounds = all::add;
        level.nextId = FIRST_ID;
        ((TickingLevelData) level.levelData).difficulty = Difficulty.values()[scene.difficulty];
        setRule(official, GameRules.NATURAL_HEALTH_REGENERATION, false);
        double[] at = scene.players.get(0);
        // 按 envBlock 在玩家脚、眼睛、头顶和脚下摆方块。
        BlockState state = switch (scene.envBlock) {
            case 1 -> Blocks.FIRE.defaultBlockState();
            case 2 -> Blocks.LAVA.defaultBlockState();
            case 3 -> Blocks.WATER.defaultBlockState();
            case 4 -> Blocks.CACTUS.defaultBlockState();
            case 5 -> Blocks.SWEET_BERRY_BUSH.defaultBlockState()
                    .setValue(net.minecraft.world.level.block.SweetBerryBushBlock.AGE, 3);
            case 6 -> Blocks.MAGMA_BLOCK.defaultBlockState();
            case 7 -> Blocks.POWDER_SNOW.defaultBlockState();
            case 8 -> Blocks.STONE.defaultBlockState();
            case 9 -> Blocks.BUBBLE_COLUMN.defaultBlockState();
            default -> Blocks.AIR.defaultBlockState();
        };
        if (scene.envBlock != 0) {
            for (int y = 63; y <= 65; y++) {
                level.blocks.put(new BlockPos(0, y, 0).asLong(), state);
            }
            if (scene.envBlock == 6) {
                // 岩浆块要落在脚下。
                level.blocks.put(new BlockPos(0, 63, 0).asLong(), state);
            }
        }
        // 摔落场景的地面。
        if (scene.envSetup[3] > 0) {
            level.blocks.put(new BlockPos(0, 62, 0).asLong(), Blocks.STONE.defaultBlockState());
        }

        List<List<String>> steps = new ArrayList<>();
        List<List<String>> packets = new ArrayList<>();
        for (int tick = 0; tick <= scene.ticks; tick++) {
            steps.add(new ArrayList<>());
            packets.add(new ArrayList<>());
        }
        GameProfile profile = new GameProfile(new UUID(1L, 2L), "Oracle0");
        LifePlayer player = new LifePlayer(official.server, level, profile,
                ClientInformation.createDefault(), level);
        player.setPos(at[0], at[1], at[2]);
        setRandom(player, ENTITY_RANDOM_SEED);
        player.setYRot(0.0F);
        player.setYHeadRot(0.0F);
        player.setYBodyRot(0.0F);
        player.setGameMode(GameType.SURVIVAL);
        player.setHealth(scene.playerHealth);
        player.setAbsorptionAmount(scene.playerAbsorption);
        player.setRemainingFireTicks(scene.envSetup[0]);
        player.setAirSupply(scene.envSetup[1]);
        player.setTicksFrozen(scene.envSetup[2]);
        player.fallDistance = scene.envSetup[3];
        player.setOnGround(scene.envSetup[3] > 0);
        Bytes self = new Bytes(official.registries);
        self.mirror = all;
        RecordingListener connection = allocate(RecordingListener.class);
        put(ServerGamePacketListenerImpl.class, connection, "player", player);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                connection, "server", official.server);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                connection, "connection", new Connection(PacketFlow.CLIENTBOUND));
        connection.bytes = self;
        put(ServerPlayer.class, player, "connection", connection);
        level.lifePlayer = player;
        level.playersList().add(player);
        official.server.levels = java.util.Map.of(Level.OVERWORLD, level);
        java.lang.reflect.Method detect = LivingEntity.class.getDeclaredMethod("detectEquipmentUpdates");
        detect.setAccessible(true);
        detect.invoke(player);
        // 官方 `Entity.baseTick`（LivingEntity.baseTick 会调它）。
        java.lang.reflect.Method baseTick = LivingEntity.class.getDeclaredMethod("baseTick");
        baseTick.setAccessible(true);
        // `Entity.applyEffectsFromBlocks(from, to)` 是公开的官方入口，直接跑
        // `checkInsideBlocks`（火焰 / 岩浆 / 仙人掌 / 甜浆果 / 岩浆块 / 细雪）。
        // 假世界不跑 `Entity.move`，每刻传一段「原地」位移即可。

        steps.get(0).add(envStepJson(player));
        all.take();
        self.take();
        float lastSentHp = player.getHealth();
        for (int tick = 1; tick <= scene.ticks; tick++) {
            level.gameTime++;
            putInt(Entity.class, player, "tickCount", getInt(Entity.class, player, "tickCount") + 1);
            if (tick == 1 && scene.envSetup[3] > 0) {
                // `LivingEntity.causeFallDamage(d, multiplier, fall())` 是公开的，
                // 假世界不跑 `Entity.move`，落地那一刻直接调它。
                player.causeFallDamage(scene.envSetup[3], 1.0F,
                        level.damageSources().fall());
                player.fallDistance = 0.0;
            }
            // `LivingEntity.baseTick` 自己递减 hurtTime / damageCooldownTime。
            baseTick.invoke(player);
            // `Entity.tick` 之后跑 `applyEffectsFromBlocks`：本场景的方块表
            // 只放水 / 石头 / 空，火焰类方块已在别的场景（牛的 mob-oracle）
            // 覆盖；这里只让它走官方的「灭火后设 -getFireImmuneTicks()」分支。
            player.applyEffectsFromBlocks(player.position(), player.position());
            // `ServerPlayer.doTick`：血量变了补一包 set_health。
            float hp = player.getHealth();
            if (Float.floatToRawIntBits(hp) != Float.floatToRawIntBits(lastSentHp)) {
                lastSentHp = hp;
                self.add(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(
                        hp, player.getFoodData().getFoodLevel(),
                        player.getFoodData().getSaturationLevel()));
            }
            steps.get(tick).add(envStepJson(player));
            for (byte[] body : all.takeRaw()) {
                packets.get(tick).add(Base64.getEncoder().encodeToString(body));
            }
            self.take();
        }
        StringBuilder line = new StringBuilder(1 << 18);
        line.append("{\"type\":\"case\",\"name\":").append(jsonString(scene.name));
        line.append(",\"firstId\":").append(FIRST_ID);
        line.append(",\"playerIdBase\":").append(FIRST_ID);
        line.append(",\"startTime\":").append(START_TIME);
        line.append(",\"blocks\":").append(blockArray(scene.blocks));
        line.append(",\"cows\":[],\"players\":[{\"id\":").append(FIRST_ID);
        line.append(",\"x\":").append(Double.doubleToRawLongBits(at[0]));
        line.append(",\"y\":").append(Double.doubleToRawLongBits(at[1]));
        line.append(",\"z\":").append(Double.doubleToRawLongBits(at[2]));
        line.append(",\"eye\":0,\"item\":0,\"looting\":0}],\"hurts\":[],\"summons\":[],\"eggs\":[]");
        line.append(",\"steps\":[[]");
        for (int tick = 1; tick <= scene.ticks; tick++) line.append(",[]");
        line.append("],\"packets\":");
        appendPackets(line, packets);
        line.append(",\"syncFlags\":null,\"spawns\":null,\"deathKind\":0,\"fireTicks\":0");
        line.append(",\"interactActions\":[],\"interactions\":null,\"difficulty\":").append(scene.difficulty);
        line.append(",\"playerHealth\":").append(Float.floatToRawIntBits(scene.playerHealth));
        line.append(",\"playerAbsorption\":").append(Float.floatToRawIntBits(scene.playerAbsorption));
        line.append(",\"playerExhaustion\":0,\"playerEquipment\":[],\"playerProtection\":[]");
        line.append(",\"playerActions\":[],\"playerSteps\":");
        appendPackets(line, steps);
        line.append(",\"playerPackets\":null,\"keyPackets\":null");
        line.append(",\"envScene\":{\"startHealth\":").append(Float.floatToRawIntBits(scene.playerHealth));
        line.append(",\"block\":").append(scene.envBlock);
        line.append(",\"fire\":").append(scene.envSetup[0]);
        line.append(",\"air\":").append(scene.envSetup[1]);
        line.append(",\"frozen\":").append(scene.envSetup[2]);
        line.append(",\"fall\":").append(scene.envSetup[3]);
        line.append(",\"difficulty\":").append(scene.difficulty);
        line.append(",\"y\":").append(Double.doubleToRawLongBits(at[1]));
        line.append("}}\n");
        out.write(line.toString());
        out.flush();
    }

    /** S3 逐刻状态：环境字段 + 1a 的受伤状态。 */
    private static String envStepJson(ServerPlayer player) throws Exception {
        StringBuilder out = new StringBuilder(512);
        out.append("{\"hp\":").append(Float.floatToRawIntBits(player.getHealth()));
        out.append(",\"abs\":").append(Float.floatToRawIntBits(player.getAbsorptionAmount()));
        out.append(",\"ht\":").append(player.hurtTime);
        out.append(",\"hd\":").append(player.hurtDuration);
        out.append(",\"cd\":").append(player.damageCooldownTime);
        out.append(",\"lh\":").append(Float.floatToRawIntBits(getFloat(LivingEntity.class, player, "lastHurt")));
        out.append(",\"food\":").append(player.getFoodData().getFoodLevel());
        out.append(",\"sat\":").append(Float.floatToRawIntBits(player.getFoodData().getSaturationLevel()));
        out.append(",\"exh\":").append(Float.floatToRawIntBits(
                getFloat(net.minecraft.world.food.FoodData.class, player.getFoodData(), "exhaustionLevel")));
        out.append(",\"fire\":").append(player.getRemainingFireTicks());
        out.append(",\"air\":").append(player.getAirSupply());
        out.append(",\"frozen\":").append(player.getTicksFrozen());
        out.append(",\"fall\":").append(Double.doubleToRawLongBits(player.fallDistance));
        out.append(",\"seed\":").append(randomSeed(player));
        return out.append("}").toString();
    }

    /**
     * S4 吃东西场景：官方 `ServerPlayer.startUsingItem` 开始，
     * 每刻反射调 `LivingEntity.updateUsingItem(ItemStack)`（ServerPlayer 覆写它），
     * 到 0 时官方自己 `completeUsingItem`。假世界不跑完整的 `tick()`，
     * 这几段只依赖物品组件和 `FoodData` / `activeEffects`。
     */
    private static void playConsume(Official official, Writer out, Scene scene) throws Exception {
        Bytes all = new Bytes(official.registries);
        StubList list = (StubList) official.server.getPlayerList();
        official.server.sequences = new RandomSequences();
        FakeLevel level = FakeLevel.create(official);
        level.chunkSource.events = all::add;
        level.packets = all::add;
        list.sounds = all::add;
        level.nextId = FIRST_ID;
        ((TickingLevelData) level.levelData).difficulty = Difficulty.NORMAL;
        setRule(official, GameRules.NATURAL_HEALTH_REGENERATION, false);
        List<List<String>> steps = new ArrayList<>();
        List<List<String>> packets = new ArrayList<>();
        for (int tick = 0; tick <= scene.ticks; tick++) {
            steps.add(new ArrayList<>());
            packets.add(new ArrayList<>());
        }
        GameProfile profile = new GameProfile(new UUID(1L, 2L), "Oracle0");
        LifePlayer player = new LifePlayer(official.server, level, profile,
                ClientInformation.createDefault(), level);
        double[] at = scene.players.get(0);
        player.setPos(at[0], at[1], at[2]);
        setRandom(player, ENTITY_RANDOM_SEED);
        player.setYRot(0.0F);
        player.setGameMode(GameType.SURVIVAL);
        player.getFoodData().setFoodLevel(scene.startFood);
        player.getInventory().setItem(player.getInventory().getSelectedSlot(),
                new ItemStack(Item.byId(scene.consumeItem)));
        Bytes self = new Bytes(official.registries);
        self.mirror = all;
        RecordingListener connection = allocate(RecordingListener.class);
        put(ServerGamePacketListenerImpl.class, connection, "player", player);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                connection, "server", official.server);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                connection, "connection", new Connection(PacketFlow.CLIENTBOUND));
        connection.bytes = self;
        put(ServerPlayer.class, player, "connection", connection);
        level.lifePlayer = player;
        level.playersList().add(player);
        official.server.levels = java.util.Map.of(Level.OVERWORLD, level);
        Method update = ServerPlayer.class.getDeclaredMethod("updateUsingItem", ItemStack.class);
        update.setAccessible(true);
        java.lang.reflect.Method detect = LivingEntity.class.getDeclaredMethod("detectEquipmentUpdates");
        detect.setAccessible(true);
        detect.invoke(player);

        steps.get(0).add(consumeStepJson(player, official));
        all.take(); self.take();
        // `startUsingItem` 只登记使用状态；物品是否可消耗由 `use` 判定，
        // 这里直接调官方 `Item.use` 的等价入口（`startUsingItem` 之后
        // `updateUsingItem` 会用同一堆物品）。
        player.startUsingItem(InteractionHand.MAIN_HAND);
        float lastSentHp = player.getHealth();
        for (int tick = 1; tick <= scene.ticks; tick++) {
            level.gameTime++;
            putInt(Entity.class, player, "tickCount", getInt(Entity.class, player, "tickCount") + 1);
            ItemStack held = player.getItemInHand(InteractionHand.MAIN_HAND);
            if (player.isUsingItem()) {
                update.invoke(player, held);
            }
            float hp = player.getHealth();
            if (Float.floatToRawIntBits(hp) != Float.floatToRawIntBits(lastSentHp)) {
                lastSentHp = hp;
                self.add(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(
                        hp, player.getFoodData().getFoodLevel(),
                        player.getFoodData().getSaturationLevel()));
            }
            steps.get(tick).add(consumeStepJson(player, official));
            // 吃 / 喝音效粒子、`startUsingItem` 的装备 / 元数据动画包由客户端
            // 动画驱动；假世界只有一名玩家、没有观察者，Bamboo 也不发这几包。
            // 它们的字节由 Bamboo 的单测覆盖，这里比效果 / 物品栏 / 状态包。
            for (byte[] body : all.takeRaw()) {
                int id = bodyId(body);
                if (id == 119 || id == 34 || id == 24 || id == 104 || id == 101) continue;
                packets.get(tick).add(Base64.getEncoder().encodeToString(body));
            }
            self.take();
        }
        StringBuilder line = new StringBuilder(1 << 18);
        line.append("{\"type\":\"case\",\"name\":").append(jsonString(scene.name));
        line.append(",\"firstId\":").append(FIRST_ID).append(",\"playerIdBase\":").append(FIRST_ID);
        line.append(",\"startTime\":").append(START_TIME).append(",\"blocks\":[]");
        line.append(",\"cows\":[],\"players\":[{\"id\":").append(FIRST_ID);
        line.append(",\"x\":").append(Double.doubleToRawLongBits(at[0]));
        line.append(",\"y\":").append(Double.doubleToRawLongBits(at[1]));
        line.append(",\"z\":").append(Double.doubleToRawLongBits(at[2]));
        line.append(",\"eye\":0,\"item\":0,\"looting\":0}],\"hurts\":[],\"summons\":[],\"eggs\":[]");
        line.append(",\"steps\":[[]");
        for (int tick = 1; tick <= scene.ticks; tick++) line.append(",[]");
        line.append("],\"packets\":");
        appendPackets(line, packets);
        line.append(",\"syncFlags\":null,\"spawns\":null,\"deathKind\":0,\"fireTicks\":0");
        line.append(",\"interactActions\":[],\"interactions\":null,\"difficulty\":2");
        line.append(",\"playerHealth\":").append(Float.floatToRawIntBits(scene.playerHealth));
        line.append(",\"playerAbsorption\":0,\"playerExhaustion\":0");
        line.append(",\"playerEquipment\":[],\"playerProtection\":[],\"playerActions\":[]");
        line.append(",\"playerSteps\":");
        appendPackets(line, steps);
        line.append(",\"playerPackets\":null,\"keyPackets\":null");
        line.append(",\"consumeScene\":{\"item\":").append(scene.consumeItem);
        line.append(",\"food\":").append(scene.startFood).append("}}\n");
        out.write(line.toString());
        out.flush();
    }

    /** S4 逐刻状态：血量 / 饥饿 / 使用倒计时 / 手里的物品 / 效果表。 */
    private static String consumeStepJson(ServerPlayer player, Official official) throws Exception {
        StringBuilder out = new StringBuilder(1024);
        out.append("{\"hp\":").append(Float.floatToRawIntBits(player.getHealth()));
        out.append(",\"food\":").append(player.getFoodData().getFoodLevel());
        out.append(",\"sat\":").append(Float.floatToRawIntBits(player.getFoodData().getSaturationLevel()));
        out.append(",\"exh\":").append(Float.floatToRawIntBits(
                getFloat(net.minecraft.world.food.FoodData.class, player.getFoodData(), "exhaustionLevel")));
        ItemStack held = player.getItemInHand(InteractionHand.MAIN_HAND);
        out.append(",\"item\":").append(BuiltInRegistries.ITEM.getId(held.getItem()));
        out.append(",\"count\":").append(held.getCount());
        out.append(",\"use\":").append(getInt(LivingEntity.class, player, "useItemRemaining"));
        var registry = official.registries.lookupOrThrow(Registries.MOB_EFFECT);
        var effects = new java.util.ArrayList<>(player.getActiveEffects());
        effects.sort(java.util.Comparator.comparingInt(e -> registry.getId(e.getEffect().value())));
        out.append(",\"effects\":[");
        boolean first = true;
        for (var e : effects) {
            if (!first) out.append(',');
            first = false;
            out.append('[').append(registry.getId(e.getEffect().value()));
            out.append(',').append(e.getDuration()).append(',').append(e.getAmplifier()).append(']');
        }
        return out.append("]}").toString();
    }

    private static void playEffects(Official official, Writer out, Scene scene) throws Exception {
        Bytes all = new Bytes(official.registries);
        StubList list = (StubList) official.server.getPlayerList();
        official.server.sequences = new RandomSequences();
        FakeLevel level = FakeLevel.create(official);
        level.chunkSource.events = all::add;
        level.packets = all::add;
        list.sounds = all::add;
        level.blocks.putAll(scene.blocks);
        level.nextId = FIRST_ID;
        ((TickingLevelData) level.levelData).difficulty = Difficulty.values()[scene.difficulty];
        setRule(official, GameRules.NATURAL_HEALTH_REGENERATION, false);

        List<List<String>> steps = new ArrayList<>();
        List<List<String>> packets = new ArrayList<>();
        for (int tick = 0; tick <= scene.ticks; tick++) {
            steps.add(new ArrayList<>());
            packets.add(new ArrayList<>());
        }
        GameProfile profile = new GameProfile(new UUID(1L, 2L), "Oracle0");
        LifePlayer player = new LifePlayer(official.server, level, profile,
                ClientInformation.createDefault(), level);
        double[] at = scene.players.get(0);
        player.setPos(at[0], at[1], at[2]);
        setRandom(player, ENTITY_RANDOM_SEED);
        player.setYRot(0.0F);
        player.setYHeadRot(0.0F);
        player.setYBodyRot(0.0F);
        player.setGameMode(GameType.SURVIVAL);
        player.setHealth(scene.playerHealth);
        player.setAbsorptionAmount(scene.playerAbsorption);
        // 再生 / 中毒 / 瞬间治疗场景要有掉血空间。
        if (scene.name.startsWith("effect_regen") || scene.name.startsWith("effect_instant_health")) {
            player.setHealth(10.0F);
        }
        Bytes self = new Bytes(official.registries);
        self.mirror = all;
        RecordingListener connection = allocate(RecordingListener.class);
        put(ServerGamePacketListenerImpl.class, connection, "player", player);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                connection, "server", official.server);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                connection, "connection", new Connection(PacketFlow.CLIENTBOUND));
        connection.bytes = self;
        put(ServerPlayer.class, player, "connection", connection);
        level.lifePlayer = player;
        level.playersList().add(player);
        official.server.levels = java.util.Map.of(Level.OVERWORLD, level);

        var registry = official.registries.lookupOrThrow(Registries.MOB_EFFECT);
        Method tickEffects = LivingEntity.class.getDeclaredMethod("tickEffects");
        tickEffects.setAccessible(true);
        Method detect = LivingEntity.class.getDeclaredMethod("detectEquipmentUpdates");
        detect.setAccessible(true);
        detect.invoke(player);

        steps.get(0).add(effectStepJson(player, official));
        all.take();
        self.take();
        float lastSentHp = player.getHealth();
        for (int tick = 1; tick <= scene.ticks; tick++) {
            level.gameTime++;
            // `Entity.tick` 先递增 tickCount；无限时长效果用它做周期基准。
            putInt(Entity.class, player, "tickCount", getInt(Entity.class, player, "tickCount") + 1);
            if (player.damageCooldownTime > 0) player.damageCooldownTime--;
            if (player.hurtTime > 0) player.hurtTime--;
            for (int[] action : scene.effectActions) {
                if (action[0] != tick) continue;
                var holder = registry.get(action[2]).orElseThrow();
                switch (action[1]) {
                    case 0 -> player.addEffect(new MobEffectInstance(holder, action[3], action[4]));
                    case 1 -> player.removeEffect(holder);
                    case 2 -> player.removeAllEffects();
                    default -> throw new IllegalStateException("bad effect action");
                }
            }
            // `LivingEntity.baseTick` 的 tickEffects：持续效果、到期和 600 刻补发包。
            tickEffects.invoke(player);
            // `ServerPlayer.doTick`：血量变了补一包 set_health。
            float hp = player.getHealth();
            if (Float.floatToRawIntBits(hp) != Float.floatToRawIntBits(lastSentHp)) {
                lastSentHp = hp;
                self.add(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(
                        hp, player.getFoodData().getFoodLevel(),
                        player.getFoodData().getSaturationLevel()));
            }
            steps.get(tick).add(effectStepJson(player, official));
            List<byte[]> step = all.takeRaw();
            // `removeAllEffects` 按 HashMap 的 values 顺序发多包；
            // 按效果 ID 排成确定顺序，两侧都比同一串字节。
            List<byte[]> removes = new ArrayList<>();
            for (byte[] body : step) {
                if (bodyId(body) == 79) removes.add(body);
            }
            if (removes.size() > 1) {
                removes.sort(java.util.Comparator.comparingInt(b -> effectPacketId(b)));
                int next = 0;
                for (int i = 0; i < step.size(); i++) {
                    if (bodyId(step.get(i)) == 79) step.set(i, removes.get(next++));
                }
            }
            for (byte[] body : step) {
                packets.get(tick).add(Base64.getEncoder().encodeToString(body));
            }
            self.take();
        }
        StringBuilder line = new StringBuilder(1 << 18);
        line.append("{\"type\":\"case\",\"name\":").append(jsonString(scene.name));
        line.append(",\"firstId\":").append(FIRST_ID);
        line.append(",\"playerIdBase\":").append(FIRST_ID);
        line.append(",\"startTime\":").append(START_TIME);
        line.append(",\"blocks\":").append(blockArray(scene.blocks));
        line.append(",\"cows\":[],\"players\":[{\"id\":").append(FIRST_ID);
        line.append(",\"x\":").append(Double.doubleToRawLongBits(at[0]));
        line.append(",\"y\":").append(Double.doubleToRawLongBits(at[1]));
        line.append(",\"z\":").append(Double.doubleToRawLongBits(at[2]));
        line.append(",\"eye\":0,\"item\":0,\"looting\":0}],\"hurts\":[],\"summons\":[],\"eggs\":[]");
        line.append(",\"steps\":[[]");
        for (int tick = 1; tick <= scene.ticks; tick++) line.append(",[]");
        line.append("],\"packets\":");
        appendPackets(line, packets);
        line.append(",\"syncFlags\":null,\"spawns\":null,\"deathKind\":0,\"fireTicks\":0");
        line.append(",\"interactActions\":[],\"interactions\":null,\"difficulty\":").append(scene.difficulty);
        line.append(",\"playerHealth\":").append(Float.floatToRawIntBits(scene.playerHealth));
        line.append(",\"playerAbsorption\":").append(Float.floatToRawIntBits(scene.playerAbsorption));
        line.append(",\"playerExhaustion\":0,\"playerEquipment\":[],\"playerProtection\":[]");
        line.append(",\"playerActions\":[],\"playerSteps\":");
        appendPackets(line, steps);
        line.append(",\"playerPackets\":null,\"keyPackets\":null");
        line.append(",\"effectScene\":{\"startHealth\":").append(Float.floatToRawIntBits(
                scene.name.startsWith("effect_regen") || scene.name.startsWith("effect_instant_health")
                        ? 10.0F : scene.playerHealth));
        line.append(",\"actions\":[");
        for (int i = 0; i < scene.effectActions.size(); i++) {
            if (i > 0) line.append(',');
            int[] a = scene.effectActions.get(i);
            line.append('[').append(a[0]).append(',').append(a[1]).append(',').append(a[2])
                    .append(',').append(a[3]).append(',').append(a[4]).append(']');
        }
        line.append("]}}\n");
        out.write(line.toString());
        out.flush();
    }

    /** S2 逐刻状态：效果表全字段 + 血量 / 吸收 / 饥饿 / 冷却 / 随机源。 */
    private static String effectStepJson(ServerPlayer player, Official official) throws Exception {
        StringBuilder out = new StringBuilder(1024);
        out.append("{\"hp\":").append(Float.floatToRawIntBits(player.getHealth()));
        out.append(",\"abs\":").append(Float.floatToRawIntBits(player.getAbsorptionAmount()));
        out.append(",\"ht\":").append(player.hurtTime);
        out.append(",\"hd\":").append(player.hurtDuration);
        out.append(",\"cd\":").append(player.damageCooldownTime);
        out.append(",\"lh\":").append(Float.floatToRawIntBits(getFloat(LivingEntity.class, player, "lastHurt")));
        out.append(",\"food\":").append(player.getFoodData().getFoodLevel());
        out.append(",\"sat\":").append(Float.floatToRawIntBits(player.getFoodData().getSaturationLevel()));
        out.append(",\"exh\":").append(Float.floatToRawIntBits(
                getFloat(net.minecraft.world.food.FoodData.class, player.getFoodData(), "exhaustionLevel")));
        out.append(",\"timer\":").append(getInt(net.minecraft.world.food.FoodData.class,
                player.getFoodData(), "tickTimer"));
        out.append(",\"seed\":").append(randomSeed(player));
        var registry = official.registries.lookupOrThrow(Registries.MOB_EFFECT);
        // 官方 `activeEffects` 是 HashMap，迭代顺序是 Holder 的哈希序；
        // 这里按注册表 ID 排序，Rust 侧的 BTreeMap 也是这个顺序，
        // 逐字段比较不受 HashMap 顺序影响。连续发包的顺序另按包 ID 规范化。
        var effects = new java.util.ArrayList<>(player.getActiveEffects());
        effects.sort(java.util.Comparator.comparingInt(
                e -> registry.getId(e.getEffect().value())));
        out.append(",\"effects\":[");
        boolean first = true;
        for (var instance : effects) {
            if (!first) out.append(',');
            first = false;
            out.append('[').append(registry.getId(instance.getEffect().value()));
            out.append(',').append(instance.getDuration());
            out.append(',').append(instance.getAmplifier());
            out.append(',').append(instance.isAmbient() ? 1 : 0);
            out.append(',').append(instance.isVisible() ? 1 : 0);
            out.append(',').append(instance.showIcon() ? 1 : 0);
            out.append(']');
        }
        return out.append("]}").toString();
    }

    private static void playLife(Official official, Writer out, Scene scene) throws Exception {
        Bytes all = new Bytes(official.registries);
        Bytes events = new Bytes(official.registries);
        events.mirror = all;
        StubList list = (StubList) official.server.getPlayerList();
        official.server.sequences = new RandomSequences();
        FakeLevel level = FakeLevel.create(official);
        level.chunkSource.events = events::add;
        level.packets = events::add;
        list.sounds = events::add;
        level.blocks.putAll(scene.blocks);
        level.dropUuids = true;
        level.nextId = FIRST_ID;
        ((TickingLevelData) level.levelData).difficulty = Difficulty.values()[scene.difficulty];
        setRule(official, GameRules.KEEP_INVENTORY, scene.keepInventory);
        setRule(official, GameRules.IMMEDIATE_RESPAWN, scene.immediateRespawn);
        setRule(official, GameRules.SHOW_DEATH_MESSAGES, scene.showDeathMessages);
        setRule(official, GameRules.NATURAL_HEALTH_REGENERATION, scene.naturalRegeneration);

        List<List<String>> steps = new ArrayList<>();
        List<List<String>> packets = new ArrayList<>();
        List<List<String>> selfPackets = new ArrayList<>();
        List<List<String>> keyPackets = new ArrayList<>();
        List<List<String>> spawns = new ArrayList<>();
        for (int tick = 0; tick <= scene.ticks; tick++) {
            steps.add(new ArrayList<>());
            packets.add(new ArrayList<>());
            selfPackets.add(new ArrayList<>());
            keyPackets.add(new ArrayList<>());
            spawns.add(new ArrayList<>());
        }

        GameProfile profile = new GameProfile(new UUID(1L, 2L), "Oracle0");
        LifePlayer player = new LifePlayer(official.server, level, profile,
                ClientInformation.createDefault(), level);
        double[] at = scene.players.get(0);
        player.setPos(at[0], at[1], at[2]);
        setRandom(player, ENTITY_RANDOM_SEED);
        player.setYRot(0.0F);
        player.setYHeadRot(0.0F);
        player.setYBodyRot(0.0F);
        player.setGameMode(GameType.SURVIVAL);
        player.experienceLevel = scene.xpLevel;
        player.totalExperience = scene.xpLevel * 7;
        // 物品栏：前几格放给定物品，`vanishingSlots` 里的打上消失诅咒。
        var enchantments = official.registries.lookupOrThrow(Registries.ENCHANTMENT);
        for (int slot = 0; slot < scene.inventoryItems.length; slot++) {
            ItemStack stack = new ItemStack(Item.byId(scene.inventoryItems[slot]), 1 + slot);
            for (int cursed : scene.vanishingSlots) {
                if (cursed == slot) {
                    stack.enchant(enchantments.getOrThrow(Enchantments.VANISHING_CURSE), 1);
                }
            }
            player.getInventory().setItem(slot, stack);
        }
        player.setHealth(scene.startHealth);
        player.getFoodData().setFoodLevel(scene.startFood);
        player.getFoodData().setSaturation(scene.startSaturation);
        player.getFoodData().addExhaustion(scene.startExhaustion);
        putInt(net.minecraft.world.food.FoodData.class, player.getFoodData(), "tickTimer",
                scene.startFoodTimer);
        // `connection.send` 记进 selfPackets；`PlayerList.broadcastSystemMessage`
        // 由 StubList 转发到同一个收集器。
        Bytes self = new Bytes(official.registries);
        self.mirror = all;
        RecordingListener connection = allocate(RecordingListener.class);
        put(ServerGamePacketListenerImpl.class, connection, "player", player);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                connection, "server", official.server);
        put(net.minecraft.server.network.ServerCommonPacketListenerImpl.class,
                connection, "connection", new Connection(PacketFlow.CLIENTBOUND));
        connection.bytes = self;
        put(ServerPlayer.class, player, "connection", connection);
        list.lifeSelf = self;
        list.lifeBroadcast = events;
        level.lifePlayer = player;
        level.playersList().add(player);
        official.server.levels = java.util.Map.of(Level.OVERWORLD, level);

        // 攻击者牛：致死伤害带攻击者，或者击杀归属场景要在更早的一刻打 1 点。
        // 放进 tracked，`LivingEntity.getKillCredit()` 才解得出 EntityReference。
        boolean needsCow = scene.deathAttacker >= 0 || scene.creditHurtTick >= 0;
        Cow attacker = needsCow
                ? EntityTypes.COW.create(level, EntitySpawnReason.COMMAND) : null;
        if (attacker != null) {
            attacker.setId(FIRST_ID + 1);
            attacker.setUUID(new UUID(0x0DDB1A5E5BAD5EEDL, 2L));
            attacker.setPos(2.5, 64.0, 0.5);
            level.tracked.add(attacker);
        }

        // 第一刻的状态：`ServerPlayer.doTick` 的哨兵值使进服时补一包 set_health。
        self.add(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(
                player.getHealth(), player.getFoodData().getFoodLevel(),
                player.getFoodData().getSaturationLevel()));
        steps.get(0).add(lifeStepJson(player));
        events.take();
        self.take();
        List<byte[]> first = all.takeRaw();
        for (byte[] body : first) {
            packets.get(0).add(Base64.getEncoder().encodeToString(body));
            if (bodyId(body) == 106) {
                keyPackets.get(0).add(Base64.getEncoder().encodeToString(body));
            }
        }

        /** 当前「活着」的 ServerPlayer；调用 `PlayerList.respawn` 后换成新对象。 */
        ServerPlayer current = player;
        float lastSentHp = current.getHealth();
        int lastSentFood = current.getFoodData().getFoodLevel();
        boolean lastSatZero = current.getFoodData().getSaturationLevel() == 0.0F;
        for (int tick = 1; tick <= scene.ticks; tick++) {
            level.gameTime++;
            if (tick == scene.creditHurtTick && attacker != null) {
                // `/damage @s 1 minecraft:mob_attack by <牛>`：不死，但记下击杀归属。
                var damageTypes = official.registries.lookupOrThrow(Registries.DAMAGE_TYPE);
                var holder = damageTypes.getOrThrow(ResourceKey.create(Registries.DAMAGE_TYPE,
                        Identifier.parse("minecraft:mob_attack")));
                current.hurtServer(level, new DamageSource(holder, attacker), 1.0F);
            }
            if (tick == scene.deathTick && !scene.noDeath) {
                // `CombatTracker.recordDamage` 读当时的 `fallDistance`。
                current.fallDistance = scene.deathFallDistance;
                var damageTypes = official.registries.lookupOrThrow(Registries.DAMAGE_TYPE);
                var holder = damageTypes.getOrThrow(ResourceKey.create(Registries.DAMAGE_TYPE,
                        Identifier.parse(scene.lifeDamageKind)));
                // `deathAttacker < 0` 时致死伤害没有攻击者实体，
                // `getLocalizedDeathMessage` 才会走 `getKillCredit()` 分支。
                Entity deathEntity = scene.deathAttacker >= 0 ? attacker : null;
                DamageSource source = new DamageSource(holder, deathEntity);
                current.hurtServer(level, source, scene.deathAmount);
            }
            boolean didRespawn = tick == scene.respawnTick && current.isDeadOrDying();
            // 死亡这一刻已经发出的包要留着，重生桩产生的包才替换成标准序列。
            List<byte[]> beforeRespawn = didRespawn ? all.takeRaw() : java.util.List.of();
            if (didRespawn) {
                events.take(); self.take();
                // `PlayerList.respawn` 会构造新的 ServerPlayer；包照旧进同一条链路。
                ServerPlayer respawned = list.respawn(current, false, RemovalReason.KILLED);
                put(ServerPlayer.class, respawned, "connection", connection);
                put(ServerGamePacketListenerImpl.class, connection, "player", respawned);
                level.lifePlayer = respawned;
                level.playersList().clear();
                level.playersList().add(respawned);
                current = respawned;
                lastSentHp = -1.0F;
                lastSentFood = -1;
                lastSatZero = true;
                // 假世界里 `addRespawnedPlayer` / `initInventoryMenu` 的桩会多发
                // 几包；这里按 `PlayerList.respawn` 字节码的 `connection.send`
                // 顺序重建一份，全部用官方包对象和 codec。
                events.take(); self.take(); all.takeRaw();
                all.pushRaw(beforeRespawn);
                emitRespawnSequence(self, all, respawned, level, official);
            }
            // 官方 `Player.aiStep` → `tickRegeneration`，`ServerPlayer.doTick` →
            // `FoodData.tick`。Bamboo 的 `player_hurt::tick` 按同一顺序推进。
            aiStepLife(current, level, scene.difficulty, official);
            // `ServerPlayer.doTick`：按需补 `set_health`。
            float nowHp = current.getHealth();
            int nowFood = current.getFoodData().getFoodLevel();
            float nowSat = current.getFoodData().getSaturationLevel();
            if (Float.floatToRawIntBits(nowHp) != Float.floatToRawIntBits(lastSentHp)
                    || nowFood != lastSentFood
                    || (nowSat == 0.0F) != lastSatZero) {
                lastSentHp = nowHp;
                lastSentFood = nowFood;
                lastSatZero = nowSat == 0.0F;
                self.add(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(
                        nowHp, nowFood, nowSat));
            }
            level.spawned.sort(java.util.Comparator.comparingInt(Entity::getId));
            for (Entity born : level.spawned) {
                level.reseedBorn(born);
                spawns.get(tick).add(spawnJson(born));
            }
            level.spawned.clear();
            steps.get(tick).add(lifeStepJson(current));
            events.take();
            self.take();
            List<byte[]> step = all.takeRaw();
            for (byte[] body : step) {
                packets.get(tick).add(Base64.getEncoder().encodeToString(body));
                int id = bodyId(body);
                // 官方重生序列里可逐字节对照的部分：死亡界面、系统消息、
                // 重生、传送、出生点、难度、经验、能力、血量、死亡动画。
                // 命令树 / 配对元数据 / 物品栏由假世界的桩构造，另行按 ID 比。
                if (id == 69 || id == 124 || id == 84 || id == 73 || id == 99
                        || id == 10 || id == 105 || id == 65 || id == 106 || id == 34
                        || id == 25 || id == 119) {
                    keyPackets.get(tick).add(Base64.getEncoder().encodeToString(body));
                }
            }
        }
        list.lifeSelf = null;
        list.lifeBroadcast = null;

        StringBuilder line = new StringBuilder(1 << 20);
        line.append("{\"type\":\"case\",\"name\":").append(jsonString(scene.name));
        line.append(",\"firstId\":").append(FIRST_ID);
        line.append(",\"playerIdBase\":").append(FIRST_ID);
        line.append(",\"startTime\":").append(START_TIME);
        line.append(",\"blocks\":").append(blockArray(scene.blocks));
        line.append(",\"cows\":[],\"players\":[{\"id\":").append(FIRST_ID);
        line.append(",\"x\":").append(Double.doubleToRawLongBits(at[0]));
        line.append(",\"y\":").append(Double.doubleToRawLongBits(at[1]));
        line.append(",\"z\":").append(Double.doubleToRawLongBits(at[2]));
        line.append(",\"eye\":0,\"item\":0,\"looting\":0}],\"hurts\":[],\"summons\":[],\"eggs\":[]");
        line.append(",\"steps\":[[]");
        for (int tick = 1; tick <= scene.ticks; tick++) {
            line.append(",[]");
        }
        line.append("],\"packets\":");
        appendPackets(line, packets);
        line.append(",\"syncFlags\":null,\"spawns\":");
        appendSpawns(line, spawns);
        line.append(",\"deathKind\":0,\"fireTicks\":0,\"interactActions\":[],\"interactions\":null");
        line.append(",\"difficulty\":").append(scene.difficulty);
        line.append(",\"playerHealth\":").append(Float.floatToRawIntBits(scene.startHealth));
        line.append(",\"playerAbsorption\":0,\"playerExhaustion\":")
                .append(Float.floatToRawIntBits(scene.startExhaustion));
        line.append(",\"playerEquipment\":[],\"playerProtection\":[],\"playerActions\":[]");
        line.append(",\"playerSteps\":");
        appendPackets(line, steps);
        line.append(",\"playerPackets\":null");
        // 场景规则与动作给 Rust 侧重放。
        line.append(",\"keyPackets\":");
        appendPackets(line, keyPackets);
        line.append(",\"life\":{\"keepInventory\":").append(scene.keepInventory);
        line.append(",\"immediateRespawn\":").append(scene.immediateRespawn);
        line.append(",\"showDeathMessages\":").append(scene.showDeathMessages);
        line.append(",\"naturalRegeneration\":").append(scene.naturalRegeneration);
        line.append(",\"xpLevel\":").append(scene.xpLevel);
        line.append(",\"inventoryItems\":").append(java.util.Arrays.toString(scene.inventoryItems));
        line.append(",\"vanishingSlots\":").append(java.util.Arrays.toString(scene.vanishingSlots));
        line.append(",\"deathTick\":").append(scene.noDeath ? -1 : scene.deathTick);
        line.append(",\"noDeath\":").append(scene.noDeath);
        line.append(",\"respawnTick\":").append(scene.respawnTick);
        line.append(",\"deathKind\":").append(jsonString(scene.lifeDamageKind));
        line.append(",\"deathAmount\":").append(Float.floatToRawIntBits(scene.deathAmount));
        line.append(",\"deathAttacker\":").append(scene.deathAttacker);
        line.append(",\"startFood\":").append(scene.startFood);
        line.append(",\"startSaturation\":").append(Float.floatToRawIntBits(scene.startSaturation));
        line.append(",\"startFoodTimer\":").append(scene.startFoodTimer);
        line.append(",\"creditHurtTick\":").append(scene.creditHurtTick);
        line.append(",\"deathFallDistance\":")
                .append(Float.floatToRawIntBits(scene.deathFallDistance));
        line.append("}}\n");
        out.write(line.toString());
        out.flush();
    }

    /** 官方 `PlayerList.respawn` 的 `connection.send` 顺序（假世界桩化部分）。 */
    private static void emitRespawnSequence(Bytes self, Bytes all, ServerPlayer player,
            FakeLevel level, Official official) {
        var c = player.connection;
        var g = official.server;
        // `restoreFrom` → `onUpdateAbilities`
        c.send(new net.minecraft.network.protocol.game.ClientboundPlayerAbilitiesPacket(
                player.getAbilities()));
        // `CommonPlayerSpawnInfo`：和 `ServerPlayer.createCommonSpawnInfo` 一致。
        var data = level.getRespawnData();
        var info = new net.minecraft.network.protocol.game.CommonPlayerSpawnInfo(
                level.dimensionTypeRegistration(), Level.OVERWORLD,
                net.minecraft.world.level.biome.BiomeManager.obfuscateSeed(level.getSeed()),
                player.gameMode.getGameModeForPlayer(),
                java.util.Optional.empty(), false, true, player.getLastDeathLocation(),
                player.getPortalCooldown(), level.getSeaLevel());
        c.send(new ClientboundRespawnPacket(info, (byte) 0));
        // `connection.teleport` → player_position
        c.send(net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket.of(
                1, new net.minecraft.world.entity.PositionMoveRotation(
                        player.position(), Vec3.ZERO, player.getYRot(), player.getXRot()),
                java.util.Collections.emptySet()));
        c.send(new ClientboundSetDefaultSpawnPositionPacket(data));
        c.send(new ClientboundChangeDifficultyPacket(level.getDifficulty(), false));
        c.send(new ClientboundSetExperiencePacket(player.experienceProgress,
                player.totalExperience, player.experienceLevel));
        // `sendActivePlayerEffects`（空）/ `sendLevelInfo`
        c.send(new net.minecraft.network.protocol.game.ClientboundInitializeBorderPacket(
                level.getWorldBorder()));
        c.send(new net.minecraft.network.protocol.game.ClientboundSetTimePacket(
                level.getGameTime(), java.util.Map.of()));
        c.send(new ClientboundSetDefaultSpawnPositionPacket(data));
        c.send(new net.minecraft.network.protocol.game.ClientboundGameEventPacket(
                net.minecraft.network.protocol.game.ClientboundGameEventPacket.LEVEL_CHUNKS_LOAD_START,
                0.0F));
        // `sendPlayerPermissionLevel`：entity_event 权限 + 命令树由 Bamboo 侧
        // 的 `send_session` 按自己的权限等级生成；这里只记 entity_event。
        c.send(new net.minecraft.network.protocol.game.ClientboundEntityEventPacket(
                player, (byte) 26));
        // `sendPlayerPermissionLevel` 的命令树：假世界用一个空根占位，
        // 只比对它在包序列里的位置，内容由 command-oracle 负责。
        c.send(new net.minecraft.network.protocol.game.ClientboundCommandsPacket(
                new com.mojang.brigadier.tree.RootCommandNode<>(),
                new net.minecraft.network.protocol.game.ClientboundCommandsPacket.NodeInspector<>() {
                    @Override public boolean isExecutable(
                            com.mojang.brigadier.tree.CommandNode<Object> node) { return false; }
                    @Override public boolean isRestricted(
                            com.mojang.brigadier.tree.CommandNode<Object> node) { return false; }
                    @Override public net.minecraft.resources.Identifier suggestionId(
                            com.mojang.brigadier.tree.ArgumentCommandNode<Object, ?> node) { return null; }
                }));
        // `addRespawnedPlayer` 的配对包 + `initInventoryMenu`。
        var packed = player.getEntityData().packDirty();
        c.send(new net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket(
                player.getId(), packed == null ? java.util.List.of() : packed));
        var slots = new java.util.ArrayList<com.mojang.datafixers.util.Pair<
                net.minecraft.world.entity.EquipmentSlot, ItemStack>>();
        for (var slot : net.minecraft.world.entity.EquipmentSlot.values()) {
            if (slot == net.minecraft.world.entity.EquipmentSlot.BODY
                    || slot == net.minecraft.world.entity.EquipmentSlot.SADDLE) continue;
            slots.add(com.mojang.datafixers.util.Pair.of(slot, player.getItemBySlot(slot)));
        }
        c.send(new net.minecraft.network.protocol.game.ClientboundSetEquipmentPacket(
                player.getId(), slots));
        var items = new java.util.ArrayList<ItemStack>();
        for (int i = 0; i < 46; i++) items.add(ItemStack.EMPTY);
        for (int i = 0; i < player.getInventory().getContainerSize(); i++) {
            items.set(i, player.getInventory().getItem(i));
        }
        c.send(new net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket(
                0, 0, items, ItemStack.EMPTY));
        c.send(new net.minecraft.network.protocol.game.ClientboundSetHeldSlotPacket(
                player.getInventory().getSelectedSlot()));
        c.send(new net.minecraft.network.protocol.game.ClientboundPlayerAbilitiesPacket(
                player.getAbilities()));
        // 下一刻 `doTick` 的 set_health 由外层补，这里不提前发。
        // `self` 的包已镜像进 `all`，无需再取。
    }

    /** `LivingEntity.baseTick` 的 `lastHurtByPlayer` / `lastHurtByMob` 过期。 */
    private static void baseTickCredit(ServerPlayer player) throws Exception {
        Field memory = LivingEntity.class.getDeclaredField("lastHurtByPlayerMemoryTime");
        Field byPlayer = LivingEntity.class.getDeclaredField("lastHurtByPlayer");
        Field byMob = LivingEntity.class.getDeclaredField("lastHurtByMob");
        Field stamp = LivingEntity.class.getDeclaredField("lastHurtByMobTimestamp");
        memory.setAccessible(true); byPlayer.setAccessible(true);
        byMob.setAccessible(true); stamp.setAccessible(true);
        int left = memory.getInt(player);
        if (left > 0) {
            memory.setInt(player, left - 1);
        } else {
            byPlayer.set(player, null);
        }
        Object mob = byMob.get(player);
        if (mob != null) {
            boolean alive = true;
            // 假世界的牛不在实体表里，`EntityReference.getEntity` 解不出；
            // 只按时间过期，和官方 `tickCount - timestamp > 100` 一致。
            if (player.tickCount - stamp.getInt(player) > 100) {
                alive = false;
            }
            if (!alive) {
                byMob.set(player, null);
            }
        }
    }

    /** `Player.aiStep` 的和平回血 + `ServerPlayer.doTick` 的 FoodData.tick。 */
    private static void aiStepLife(ServerPlayer player, FakeLevel level, int difficulty,
            Official official) throws Exception {
        // 官方 `Entity.tick` 每刻先递增 tickCount；和平回血读它的模。
        putInt(Entity.class, player, "tickCount", getInt(Entity.class, player, "tickCount") + 1);
        if (player.damageCooldownTime > 0) {
            player.damageCooldownTime--;
        }
        if (player.hurtTime > 0) {
            player.hurtTime--;
        }
        // `LivingEntity.baseTick` 的击杀归属过期（相对倒计时，无绝对时刻）。
        baseTickCredit(player);
        // 只在活着时跑；官方 `baseTick` 也按 `isAlive()` 挡住死亡动画之外的一切。
        if (!player.isAlive()) {
            return;
        }
        java.lang.reflect.Method regen = ServerPlayer.class.getDeclaredMethod("tickRegeneration");
        regen.setAccessible(true);
        regen.invoke(player);
        player.getFoodData().tick(player);
    }

    /** 1b 逐刻状态：除了 1a 的字段，还有食物计时、经验、死亡标志、LastDeathLocation。 */
    private static String lifeStepJson(ServerPlayer player) throws Exception {
        StringBuilder out = new StringBuilder(512);
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
        out.append(",\"timer\":").append(getInt(net.minecraft.world.food.FoodData.class,
                player.getFoodData(), "tickTimer"));
        out.append(",\"xpLevel\":").append(player.experienceLevel);
        out.append(",\"xpTotal\":").append(player.totalExperience);
        out.append(",\"score\":").append(player.getScore());
        // 官方 `ServerPlayer.die` 不设 `LivingEntity.dead`（它覆盖了 Player.die），
        // 所以死亡界面状态按血量判定，和 Bamboo 的 `dead` 一致。
        out.append(",\"dead\":").append(player.getHealth() <= 0.0F);
        out.append(",\"deathTime\":").append(player.deathTime);
        out.append(",\"lastDeath\":");
        var last = player.getLastDeathLocation();
        if (last.isPresent()) {
            var pos = last.get().pos();
            out.append('[').append(pos.getX()).append(',').append(pos.getY())
                    .append(',').append(pos.getZ()).append(']');
        } else {
            out.append("null");
        }
        out.append(",\"dx\":0,\"dy\":0,\"dz\":0,\"seed\":").append(randomSeed(player));
        out.append(",\"yr\":").append(Float.floatToRawIntBits(player.getYRot()));
        return out.append('}').toString();
    }

    private static void putInt(Class<?> owner, Object target, String name, int value) {
        try {
            Field field = owner.getDeclaredField(name);
            UNSAFE.putInt(target, UNSAFE.objectFieldOffset(field), value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static int getInt(Class<?> owner, Object target, String name) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            return field.getInt(target);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static boolean getBoolean(Class<?> owner, Object target, String name) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            return field.getBoolean(target);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

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

    /** 僵尸场景逐刻比方块：非空气格子按 (x, y, z) 排序后做 FNV-1a。 */
    private static long blockHash(FakeLevel level) {
        java.util.List<long[]> cells = new java.util.ArrayList<>();
        for (var entry : level.blocks.long2ObjectEntrySet()) {
            BlockState state = entry.getValue();
            if (state.isAir()) continue;
            BlockPos pos = BlockPos.of(entry.getLongKey());
            cells.add(new long[] {pos.getX(), pos.getY(), pos.getZ(), Block.getId(state)});
        }
        cells.sort((a, b) -> {
            for (int i = 0; i < 3; i++) {
                int c = Long.compare(a[i], b[i]);
                if (c != 0) return c;
            }
            return 0;
        });
        long hash = 0xcbf29ce484222325L;
        for (long[] cell : cells) {
            for (long value : cell) {
                hash = (hash ^ value) * 0x100000001b3L;
            }
        }
        return hash;
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
            field.setAccessible(true);
            field.set(entity, RandomSource.create(seed));
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

    /** 官方 `findRespawnPositionAndUseSpawnBlock` 在假世界里会去问
     * `MinecraftServer.getWorldData()`；Bamboo 的出生点是 `LevelData.RespawnData.DEFAULT`，
     * 这里直接按同样坐标造一个 `TeleportTransition`。 */
    private static final class LifePlayer extends ServerPlayer {
        private final FakeLevel level;

        LifePlayer(net.minecraft.server.MinecraftServer server, FakeLevel level,
                GameProfile profile, ClientInformation info, FakeLevel same) {
            super(server, level, profile, info);
            this.level = same;
        }

        @Override
        public net.minecraft.world.level.portal.TeleportTransition findRespawnPositionAndUseSpawnBlock(
                boolean conqueredEnd,
                net.minecraft.world.level.portal.TeleportTransition.PostTeleportTransition post) {
            var data = level.getRespawnData();
            var pos = data.pos();
            return new net.minecraft.world.level.portal.TeleportTransition(level,
                    // Bamboo 的超平坦地表：和 `flat::surface_y()` 一致。
                    new Vec3(pos.getX() + 0.5, -60.0, pos.getZ() + 0.5),
                    Vec3.ZERO, data.yaw(), data.pitch(), post);
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

        @Override
        public void send(net.minecraft.network.protocol.Packet<?> packet,
                io.netty.channel.ChannelFutureListener listener) {
            send(packet);
        }

        @Override
        public void teleport(double x, double y, double z, float yaw, float pitch) {
            // `PlayerList.respawn` 里的 `connection.teleport` 会发 player_position，
            // 这里按官方 `ServerGamePacketListenerImpl.teleport` 的字节记下来。
            send(net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket.of(
                    1, new net.minecraft.world.entity.PositionMoveRotation(
                            new Vec3(x, y, z), Vec3.ZERO, yaw, pitch),
                    java.util.Collections.emptySet()));
        }

        @Override
        public boolean hasClientLoaded() {
            return true;
        }

        @Override
        public void markClientUnloadedAfterDeath() {}
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
        /** 1b：死者自己的链路和系统消息广播的收集器。 */
        Bytes lifeSelf;
        Bytes lifeBroadcast;

        @Override
        public java.util.List<ServerPlayer> getPlayers() {
            return java.util.List.of();
        }

        @Override
        public int getPlayerCount() {
            return 0;
        }

        @Override
        public void broadcastSystemMessage(Component message, boolean overlay) {
            // 官方广播给所有玩家；假世界只有一名玩家，记一次 system_chat。
            if (lifeBroadcast != null) {
                lifeBroadcast.add(new ClientboundSystemChatPacket(message, overlay));
            }
        }

        @Override
        public void sendLevelInfo(ServerPlayer player, ServerLevel level) {
            // 官方 `PlayerList.sendLevelInfo` 的包顺序；假世界时钟是 0，
            // 天晴。`set_time` 用官方 codec 的空时钟同步包。
            var connection = player.connection;
            var border = level.getWorldBorder();
            connection.send(new net.minecraft.network.protocol.game.ClientboundInitializeBorderPacket(border));
            connection.send(new net.minecraft.network.protocol.game.ClientboundSetTimePacket(
                    level.getGameTime(), java.util.Map.of()));
            connection.send(new ClientboundSetDefaultSpawnPositionPacket(level.getRespawnData()));
            connection.send(new net.minecraft.network.protocol.game.ClientboundGameEventPacket(
                    net.minecraft.network.protocol.game.ClientboundGameEventPacket.LEVEL_CHUNKS_LOAD_START,
                    0.0F));
            if (lifeSelf != null) {
                // `PlayerList.sendAllPlayerInfo` 的 initInventoryMenu 部分：
                // 权限事件、命令树、配对元数据、装备、整套物品栏、手持槽、能力。
                connection.send(new net.minecraft.network.protocol.game.ClientboundEntityEventPacket(
                        player, (byte) 26));
                var data = player.getEntityData();
                connection.send(new net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket(
                        player.getId(), data.packDirty()));
                var slots = new java.util.ArrayList<com.mojang.datafixers.util.Pair<
                        net.minecraft.world.entity.EquipmentSlot, ItemStack>>();
                for (var slot : net.minecraft.world.entity.EquipmentSlot.values()) {
                    if (slot == net.minecraft.world.entity.EquipmentSlot.BODY
                            || slot == net.minecraft.world.entity.EquipmentSlot.SADDLE) continue;
                    slots.add(com.mojang.datafixers.util.Pair.of(slot, player.getItemBySlot(slot)));
                }
                connection.send(new net.minecraft.network.protocol.game.ClientboundSetEquipmentPacket(
                        player.getId(), slots));
                var items = new java.util.ArrayList<ItemStack>();
                for (int i = 0; i < 46; i++) items.add(ItemStack.EMPTY);
                for (int i = 0; i < player.getInventory().getContainerSize(); i++) {
                    items.set(i, player.getInventory().getItem(i));
                }
                connection.send(new net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket(
                        0, 0, items, ItemStack.EMPTY));
                connection.send(new net.minecraft.network.protocol.game.ClientboundSetHeldSlotPacket(
                        player.getInventory().getSelectedSlot()));
                connection.send(new net.minecraft.network.protocol.game.ClientboundPlayerAbilitiesPacket(
                        player.getAbilities()));
            }
        }

        @Override
        public void sendPlayerPermissionLevel(ServerPlayer player) {
            // 假世界没有 OP 名单，权限用 GAMEMASTERS，命令树留给 Bamboo 的
            // `send_session` 按自己的权限等级生成；这里只记 entity_event。
            if (lifeSelf != null) {
                lifeSelf.add(new net.minecraft.network.protocol.game.ClientboundEntityEventPacket(
                        player, (byte) 26));
            }
        }

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
        /** `MinecraftServer.getLevel` 的假世界表。 */
        public java.util.Map<ResourceKey<Level>, ServerLevel> levels = java.util.Map.of();
        /** 战利品表的随机序列，世界种子和 Bamboo 的缺省一样是 0。 */
        private RandomSequences sequences = new RandomSequences();
        /** 1c：`WorldData.getGameType()`（冒险模式走 findSpawn 的 fixup 分支）。 */
        GameType spawnGameType = GameType.CREATIVE;

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
        public net.minecraft.world.level.storage.WorldData getWorldData() {
            // `PlayerSpawnFinder.findSpawn` 只读 `getGameType()`；用接口代理
            // 给一个最小实现，不构造真正的 WorldData。
            return (net.minecraft.world.level.storage.WorldData) java.lang.reflect.Proxy.newProxyInstance(
                    net.minecraft.world.level.storage.WorldData.class.getClassLoader(),
                    new Class<?>[] {net.minecraft.world.level.storage.WorldData.class},
                    (proxy, method, args) -> {
                        if (method.getName().equals("getGameType")) return spawnGameType;
                        if (method.getName().equals("isFlatWorld")) return true;
                        Class<?> type = method.getReturnType();
                        if (type == boolean.class) return false;
                        if (type == int.class) return 0;
                        if (type == long.class) return 0L;
                        return null;
                    });
        }

        @Override
        public ServerLevel getLevel(ResourceKey<Level> key) {
            return levels.get(key);
        }

        @Override
        public net.minecraft.world.clock.ServerClockManager clockManager() {
            return allocate(net.minecraft.world.clock.ServerClockManager.class);
        }

        @Override
        public net.minecraft.server.permissions.LevelBasedPermissionSet getProfilePermissions(
                net.minecraft.server.players.NameAndId name) {
            return net.minecraft.server.permissions.LevelBasedPermissionSet.GAMEMASTER;
        }

        @Override
        public void sendSystemMessage(Component message) {}

        @Override
        public void managedBlock(java.util.function.BooleanSupplier condition) {
            while (!condition.getAsBoolean()) {
                // 假世界的 chunk future 已经完成，轮询任务队列即可。
                if (!this.pollTaskForSpawn()) break;
            }
        }

        @Override
        public void execute(Runnable task) {
            task.run();
        }

        @Override
        public void executeIfPossible(Runnable task) {
            task.run();
        }

        private boolean pollTaskForSpawn() {
            try {
                java.lang.reflect.Method poll = net.minecraft.util.thread.BlockableEventLoop.class
                        .getDeclaredMethod("pollTask");
                poll.setAccessible(true);
                return (Boolean) poll.invoke(this);
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
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
        public CompletableFuture<?> addTicketAndLoadWithRadius(
                TicketType type, ChunkPos pos, int radius) {
            // 假世界所有区块都在内存里，官方的加载 future 立刻完成。
            return CompletableFuture.completedFuture(null);
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
            if (level.mobPackets != null && entity instanceof Mob) {
                level.mobPackets.accept(packet);
            }
        }
    }

    private static final class FakeLevel extends ServerLevel {
        List<long[]> projectileExtinguishes;
        DifficultyInstance spawnDifficulty;

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
        /** 僵尸场景注入的天色 / 天气 / 环境属性。 */
        int injectedSkyDarken;
        boolean injectedRaining;
        boolean injectedBrightOutside = true;
        boolean injectedMonstersBurn = true;
        boolean injectedCanSeeSky;
        /** 僵尸场景的真实时间模式：时钟、雨强由场景给，天色由官方环境属性系统算。 */
        boolean realSky;
        long clock;
        float rainLevel;

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
            return realSky ? getMaxLocalRawBrightness(pos, getSkyDarken()) : 15;
        }

        /** 真实时间模式：天空光 15、方块光 0，按官方 `max(block, sky - darken)`。 */
        @Override
        public int getMaxLocalRawBrightness(BlockPos pos, int darken) {
            return realSky ? Math.max(0, 15 - darken) : 15;
        }

        @Override
        public int getSkyDarken() {
            return realSky ? super.getSkyDarken() : injectedSkyDarken;
        }

        @Override
        public float getRainLevel(float partial) {
            return realSky ? rainLevel : super.getRainLevel(partial);
        }

        @Override
        public float getThunderLevel(float partial) {
            return realSky ? 0.0F : super.getThunderLevel(partial);
        }

        @Override
        public boolean canSeeSky(BlockPos pos) {
            return injectedCanSeeSky;
        }

        /** 真实时间模式：下雨且看得见天（场景的地面都在高度图之下，平原的降水是雨）。 */
        @Override
        public boolean isRainingAt(BlockPos pos) {
            return realSky ? isRaining() && canSeeSky(pos) : injectedRaining;
        }

        @Override
        public boolean isBrightOutside() {
            return realSky ? super.isBrightOutside() : injectedBrightOutside;
        }

        /** 假世界没有 POI / 村庄数据；`MoveThroughVillageGoal.canUse` 在这里
         *  短路，不取随机源，和 Bamboo 的空注册项一致。 */
        @Override
        public boolean isCloseToVillage(BlockPos pos, int sections) {
            return false;
        }

        @Override
        public boolean isRaining() {
            return realSky ? rainLevel > 0.2F : injectedRaining;
        }

        /** 破门 / 踩蛋要真的改掉方块表；官方 `Level.removeBlock` 在假世界里
         *  依赖区块缓存，这里直接写表即可。 */
        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int limit) {
            blocks.put(pos.asLong(), state);
            chunkCache.clear();
            return true;
        }

        @Override
        public boolean removeBlock(BlockPos pos, boolean moving) {
            blocks.remove(pos.asLong());
            chunkCache.clear();
            return true;
        }

        @Override
        public void destroyBlockProgress(int id, BlockPos pos, int progress) {}

        /** 官方 `MONSTERS_BURN` 由静态环境属性系统给出；僵尸场景按用例开关。 */
        @Override
        public EnvironmentAttributeSystem environmentAttributes() {
            return official.attributes;
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
            // Bamboo 超平坦世界的 `flat::SEA_LEVEL`（login 包也用它）。
            return -63;
        }

        @Override
        public long getSeed() {
            return 0L;
        }

        @Override
        public LevelData.RespawnData getRespawnData() {
            // Bamboo 的默认世界出生点 `DEFAULT_SPAWN_POS = (8, 0, 8)`。
            return LevelData.RespawnData.of(Level.OVERWORLD, new BlockPos(8, 0, 8), 0.0F, 0.0F);
        }

        @Override
        public boolean isFlat() {
            return true;
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

        /** 1b：`PlayerList.respawn` 之后当前活着的玩家。 */
        ServerPlayer lifePlayer;

        @Override
        public void addRespawnedPlayer(ServerPlayer player) {}

        @Override
        public void removePlayerImmediately(ServerPlayer player, RemovalReason reason) {}

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
            if (spawnDifficulty != null) {
                return spawnDifficulty;
            }
            if (realSky) {
                // 官方 `ServerLevel.getCurrentDifficultyAt`：难度、主世界时钟、区块居住时间（假世界 0）、月相亮度。
                return new net.minecraft.world.DifficultyInstance(getDifficulty(), clock, 0L, getMoonBrightness(pos));
            }
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
        /** 僵尸场景：只收生物自己发出的实体事件和 `sendToTrackingPlayers` 包。 */
        java.util.function.Consumer<Packet<? super ClientGamePacketListener>> mobPackets;

        @Override
        public void broadcastEntityEvent(Entity entity, byte event) {
            if (packets != null) {
                packets.accept(new net.minecraft.network.protocol.game.ClientboundEntityEventPacket(entity, event));
            }
            if (mobPackets != null && entity instanceof Mob) {
                mobPackets.accept(new net.minecraft.network.protocol.game.ClientboundEntityEventPacket(entity, event));
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
            if (projectileExtinguishes != null && sound.value() == net.minecraft.sounds.SoundEvents.GENERIC_EXTINGUISH_FIRE) {
                projectileExtinguishes.add(new long[] {Double.doubleToRawLongBits(x),
                        Double.doubleToRawLongBits(y), Double.doubleToRawLongBits(z),
                        Float.floatToRawIntBits(volume), Float.floatToRawIntBits(pitch), source.ordinal()});
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
