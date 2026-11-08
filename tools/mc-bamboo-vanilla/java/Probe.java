import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import io.netty.channel.Channel;
import io.netty.channel.ChannelHandlerContext;
import io.netty.channel.ChannelInboundHandlerAdapter;
import io.netty.channel.ChannelInitializer;
import io.netty.channel.ChannelPipeline;
import io.netty.channel.embedded.EmbeddedChannel;
import java.io.IOException;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.net.Socket;
import java.net.SocketTimeoutException;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.BitSet;
import java.util.EnumMap;
import java.util.EnumSet;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.UUID;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ForkJoinPool;
import java.util.function.BooleanSupplier;
import java.util.function.Consumer;
import net.minecraft.ChatFormatting;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.UUIDUtil;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.Connection;
import net.minecraft.network.DisconnectionDetails;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.PacketDecoder;
import net.minecraft.network.VarInt;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.LastSeenMessages;
import net.minecraft.network.chat.TextColor;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.PacketFlow;
import net.minecraft.network.protocol.common.ClientboundDisconnectPacket;
import net.minecraft.network.protocol.common.ClientboundKeepAlivePacket;
import net.minecraft.network.protocol.common.ClientboundUpdateTagsPacket;
import net.minecraft.network.protocol.common.ServerboundClientInformationPacket;
import net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket;
import net.minecraft.network.protocol.common.ServerboundKeepAlivePacket;
import net.minecraft.network.protocol.common.custom.BrandPayload;
import net.minecraft.network.protocol.configuration.ClientConfigurationPacketListener;
import net.minecraft.network.protocol.configuration.ClientboundRegistryDataPacket;
import net.minecraft.network.protocol.configuration.ClientboundSelectKnownPacks;
import net.minecraft.network.protocol.configuration.ClientboundUpdateEnabledFeaturesPacket;
import net.minecraft.network.protocol.configuration.ConfigurationProtocols;
import net.minecraft.network.protocol.configuration.ServerboundFinishConfigurationPacket;
import net.minecraft.network.protocol.configuration.ServerboundSelectKnownPacks;
import net.minecraft.network.protocol.game.ClientGamePacketListener;
import net.minecraft.network.protocol.game.ClientboundAddEntityPacket;
import net.minecraft.network.protocol.game.ClientboundAwardStatsPacket;
import net.minecraft.network.protocol.game.ClientboundBlockChangedAckPacket;
import net.minecraft.network.protocol.game.ClientboundBlockUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundBundlePacket;
import net.minecraft.network.protocol.game.ClientboundChunkBatchFinishedPacket;
import net.minecraft.network.protocol.game.ClientboundContainerSetSlotPacket;
import net.minecraft.network.protocol.game.ClientboundDisguisedChatPacket;
import net.minecraft.network.protocol.game.ClientboundEntityPositionSyncPacket;
import net.minecraft.network.protocol.game.ClientboundForgetLevelChunkPacket;
import net.minecraft.network.protocol.game.ClientboundGameEventPacket;
import net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket;
import net.minecraft.network.protocol.game.ClientboundLevelEventPacket;
import net.minecraft.network.protocol.game.ClientboundLightUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData;
import net.minecraft.network.protocol.game.ClientboundLoginPacket;
import net.minecraft.network.protocol.game.ClientboundMoveEntityPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerAbilitiesPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerInfoRemovePacket;
import net.minecraft.network.protocol.game.ClientboundPlayerInfoUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket;
import net.minecraft.network.protocol.game.ClientboundRemoveEntitiesPacket;
import net.minecraft.network.protocol.game.ClientboundRotateHeadPacket;
import net.minecraft.network.protocol.game.ClientboundSetChunkCacheCenterPacket;
import net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket;
import net.minecraft.network.protocol.game.ClientboundSetEquipmentPacket;
import net.minecraft.network.protocol.game.ClientboundSetHeldSlotPacket;
import net.minecraft.network.protocol.game.ClientboundSetTimePacket;
import net.minecraft.network.protocol.game.ClientboundSwingAnimationPacket;
import net.minecraft.network.protocol.game.ClientboundSystemChatPacket;
import net.minecraft.network.protocol.game.ClientboundUpdateAttributesPacket;
import net.minecraft.network.protocol.game.GameProtocols;
import net.minecraft.network.protocol.game.ServerboundAcceptTeleportationPacket;
import net.minecraft.network.protocol.game.ServerboundChatCommandPacket;
import net.minecraft.network.protocol.game.ServerboundChatPacket;
import net.minecraft.network.protocol.game.ServerboundChunkBatchReceivedPacket;
import net.minecraft.network.protocol.game.ServerboundClientCommandPacket;
import net.minecraft.network.protocol.game.ServerboundMovePlayerPacket;
import net.minecraft.network.protocol.game.ServerboundPickItemFromBlockPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerAbilitiesPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerActionPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerCommandPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerInputPacket;
import net.minecraft.network.protocol.game.ServerboundPunchPacket;
import net.minecraft.network.protocol.game.ServerboundSetCarriedItemPacket;
import net.minecraft.network.protocol.game.ServerboundSetCreativeModeSlotPacket;
import net.minecraft.network.protocol.game.ServerboundUseItemOnPacket;
import net.minecraft.network.protocol.game.ServerboundUseItemPacket;
import net.minecraft.network.protocol.game.VecDeltaCodec;
import net.minecraft.network.protocol.login.ClientLoginPacketListener;
import net.minecraft.network.protocol.login.ClientboundLoginCompressionPacket;
import net.minecraft.network.protocol.login.ClientboundLoginDisconnectPacket;
import net.minecraft.network.protocol.login.ClientboundLoginFinishedPacket;
import net.minecraft.network.protocol.login.ServerboundHelloPacket;
import net.minecraft.network.protocol.login.ServerboundLoginAcknowledgedPacket;
import net.minecraft.network.protocol.ping.ClientboundPongResponsePacket;
import net.minecraft.network.protocol.ping.ServerboundPingRequestPacket;
import net.minecraft.network.protocol.status.ClientStatusPacketListener;
import net.minecraft.network.protocol.status.ClientboundStatusResponsePacket;
import net.minecraft.network.protocol.status.ServerStatus;
import net.minecraft.network.protocol.status.ServerboundStatusRequestPacket;
import net.minecraft.network.syncher.SynchedEntityData;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.level.ChunkTrackingView;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.server.level.ParticleStatus;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.tags.TagNetworkSerialization;
import net.minecraft.util.SimpleBitStorage;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.HumanoidArm;
import net.minecraft.world.entity.Pose;
import net.minecraft.world.entity.PositionPath;
import net.minecraft.world.entity.player.Abilities;
import net.minecraft.world.entity.player.ChatVisiblity;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.SwingAnimationType;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.RotatedPillarBlock;
import net.minecraft.world.level.block.SlabBlock;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.entity.BlockEntityTypes;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.SlabType;
import net.minecraft.world.level.chunk.DataLayer;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.ticks.ProtoChunkTicks;

/**
 * 用官方 26.3 的网络代码扮演客户端，检查 Bamboo 发的每个包。
 *
 * <p>第一个客户端完整走一遍进服流程。第二个客户端进来之后，两名玩家轮流走动、转头、跳、潜行、
 * 冲刺、挥手、换手持物、破坏和放置方块、聊天，第三个客户端晚到，检查改过的区块，最后有人离开。
 *
 * <p>帧、数据包和 NBT 的解码全部走官方代码，注册表和标签会和官方算出的结果逐项比较，
 * 区块用官方的 LevelChunkSection 解码后再检查方块。官方解码器发现包末尾有多余字节时会抛异常，
 * 连接随之关闭，这里把原因记成一项失败。
 *
 * <p>官方管线跑在 EmbeddedChannel 上，字节通过阻塞 socket 转发，这样不依赖 NIO。
 * 所以这里验证的是协议内容，不包括 NIO 传输层和客户端的渲染。
 *
 * <p>脚本的每一步都有时限。一步超时后这一段剩下的步骤跳过，下一段继续，结束时列出所有失败。
 */
public final class Probe {
    private static final long PING_VALUE = 0x0123456789ABCDEFL;
    private static final String PLAYER = "Kitten";
    private static final String SECOND_PLAYER = "Mochi";
    private static final String LATE_PLAYER = "Latecomer";
    /** 客户端请求的视距，和 Bamboo 的默认值相同，所以 chunkRadius 和跟踪范围都用它。 */
    private static final int VIEW_DISTANCE = 8;
    private static final int ALL_SKIN_PARTS = 0x7f;
    private static final int NO_HAT_SKIN_PARTS = 0x3f;
    /** 官方服务器默认的 `network-compression-threshold`。 */
    private static final int COMPRESSION_THRESHOLD = 256;
    /** 出生点所在的区块。 */
    private static final ChunkPos SPAWN_CHUNK = new ChunkPos(0, 0);
    /** 全部区块到齐之后走到这个区块，让服务端发 center、forget 和新区块。 */
    private static final ChunkPos WALK_TARGET = new ChunkPos(1, 0);
    private static final int FIRST_BATCH_CHUNKS = 9;
    /** 第二个心跳能到，说明服务端接受了第一个回应。 */
    private static final int KEEP_ALIVES_TO_ANSWER = 2;
    private static final int SECTIONS = 24;
    private static final int LIGHT_LAYERS = SECTIONS + 2;
    private static final long JOIN_TIMEOUT_MILLIS = 60_000;
    /** 脚本一步的时限。 */
    private static final long STEP_TIMEOUT_MILLIS = 5_000;
    /** 断言“没有收到”之前等的时间，够世界线程跑 6 tick。 */
    private static final long SETTLE_MILLIS = 300;
    private static final long TICK_MILLIS = 50;
    /** 所有步骤都有时限，这是最后的保险：比如服务端不读 socket 时写操作会一直挂着。 */
    private static final long WATCHDOG_MILLIS = 480_000;
    private static final int READ_TIMEOUT_MILLIS = 5;
    private static final double SPAWN_X = 8.5;
    private static final double SPAWN_Y = -60.0;
    private static final double SPAWN_Z = 8.5;
    /** 走路时一步的距离，不是 1/4096 的整数倍，这样取整错了就对不上。 */
    private static final double WALK_STEP = 0.2137;
    private static final int WALK_STEPS = 20;
    /** 定点数的精度。 */
    private static final double POSITION_TOLERANCE = 1.0 / 4096.0;
    private static final int METADATA_SHARED_FLAGS = 0;
    private static final int METADATA_POSE = 6;
    private static final int METADATA_HEALTH = 9;
    private static final int METADATA_MAIN_HAND = 15;
    private static final int METADATA_SKIN_PARTS = 16;
    private static final byte FLAG_SHIFT = 0x02;
    private static final byte FLAG_SPRINTING = 0x08;
    private static final int LEVEL_EVENT_BREAK = 2001;
    private static final int HOTBAR_START = 36;
    private static final int OFF_HAND_SLOT = 45;
    private static final int STACK = 64;
    private static final String CHAT_TEXT = "你好，竹子🐾";
    private static final String UNKNOWN_COMMAND = "Unknown or incomplete command. See below for error";

    /** M2 会发的全部包。收到别的说明服务端走了规格之外的路径。 */
    private static final Set<String> M2_PACKETS = Set.of(
            "handleAddEntity",
            "handleAwardStats",
            "handleBlockChangedAck",
            "handleBlockUpdate",
            "handleBundlePacket",
            "handleChunkBatchFinished",
            "handleChunkBatchStart",
            "handleContainerSetSlot",
            "handleDisconnect",
            "handleDisguisedChat",
            "handleEntityPositionSync",
            "handleForgetLevelChunk",
            "handleGameEvent",
            "handleKeepAlive",
            "handleLevelChunkWithLight",
            "handleLevelEvent",
            "handleLightUpdatePacket",
            "handleLogin",
            "handleMoveEntity",
            "handleMovePlayer",
            "handlePlayerAbilities",
            "handlePlayerInfoRemove",
            "handlePlayerInfoUpdate",
            "handleRemoveEntities",
            "handleRotateMob",
            "handleSetChunkCacheCenter",
            "handleSetEntityData",
            "handleSetEquipment",
            "handleSetHeldSlot",
            "handleSetTime",
            "handleSwingAnimation",
            "handleSystemChat",
            "handleUpdateAttributes");

    private static final Map<String, Boolean> CHECKS = new LinkedHashMap<>();
    /** 第一个玩家进来时世界还没改过，它的第一个区块一定要对一次光照的答案。 */
    private static volatile boolean flatLightChecked;

    private static void check(String name, boolean passed) {
        CHECKS.merge(name, passed, Boolean::logicalAnd);
    }

    public static void main(String[] args) throws Exception {
        String host = args[0];
        int port = Integer.parseInt(args[1]);
        startWatchdog();
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            LayeredRegistryAccess<RegistryLayer> layers = loadRegistries(resources);
            RegistryAccess.Frozen registries = layers.compositeAccess();
            // 物品的默认组件要绑定之后才能构造 ItemStack。
            BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build((HolderLookup.Provider) registries)
                    .forEach(pending -> pending.apply());
            checkTrailingBytesAreRejected(registries);
            try {
                probeStatus(host, port);
                new Scenario(host, port, layers).run();
                check("chunk: an untouched chunk had its light compared", flatLightChecked);
            } catch (Exception error) {
                // 官方解码器发现格式错误时会断开连接，这里记成一项失败，照常输出汇总。
                check("connection: no error while talking to the server (" + error + ")", false);
            }
        }

        long failed = CHECKS.values().stream().filter(passed -> !passed).count();
        CHECKS.forEach((name, passed) -> System.out.println("PROBE " + (passed ? "ok   " : "FAIL ") + name));
        System.out.println("PROBE RESULT " + (CHECKS.size() - failed) + "/" + CHECKS.size() + " checks passed");
        System.exit(failed == 0 ? 0 : 1);
    }

    private static void startWatchdog() {
        Thread watchdog = new Thread(() -> {
            try {
                Thread.sleep(WATCHDOG_MILLIS);
            } catch (InterruptedException interrupted) {
                return;
            }
            System.out.println("PROBE FAIL the probe was still running after " + WATCHDOG_MILLIS + " ms");
            Runtime.getRuntime().halt(1);
        }, "probe-watchdog");
        watchdog.setDaemon(true);
        watchdog.start();
    }

    /** 和 WorldLoader.load 的前半段相同，得到官方的注册表和标签。 */
    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(CloseableResourceManager resources) {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }

    /** 确认多余字节真的抓得到：官方解码器要拒绝 block_changed_ack 后面多一个字节的包。 */
    private static void checkTrailingBytesAreRejected(RegistryAccess.Frozen registries) {
        ProtocolInfo<ClientGamePacketListener> protocol =
                GameProtocols.CLIENTBOUND_TEMPLATE.bind(RegistryFriendlyByteBuf.decorator(registries));
        EmbeddedChannel channel = new EmbeddedChannel(new PacketDecoder<>(protocol));
        byte[] packetId = {4};
        byte[] sequence = {1};
        byte[] extra = {0};
        boolean rejected = false;
        try {
            channel.writeInbound(Unpooled.wrappedBuffer(packetId, sequence, extra));
        } catch (Exception error) {
            rejected = String.valueOf(error).contains("larger than I expected");
        }
        check("decode: the official decoder rejects trailing bytes", rejected);
    }

    private static void probeStatus(String host, int port) throws IOException {
        CountDownLatch finished = new CountDownLatch(1);
        try (Bridge bridge = new Bridge(host, port)) {
            Connection connection = bridge.connection;
            connection.initiateServerboundStatusConnection(host, port, new ClientStatusPacketListener() {
                @Override
                public void handleStatusResponse(ClientboundStatusResponsePacket packet) {
                    ServerStatus status = packet.status();
                    int protocol = SharedConstants.getCurrentVersion().protocolVersion();
                    check("status: protocol matches the official client",
                            status.version().map(version -> version.protocol() == protocol).orElse(false));
                    check("status: player counts are present", status.players().isPresent());
                    connection.send(new ServerboundPingRequestPacket(PING_VALUE));
                }

                @Override
                public void handlePongResponse(ClientboundPongResponsePacket packet) {
                    check("status: pong echoes the ping", packet.time() == PING_VALUE);
                    finished.countDown();
                }

                @Override
                public void onDisconnect(DisconnectionDetails details) {
                    finished.countDown();
                }

                @Override
                public boolean isAcceptingMessages() {
                    return connection.isConnected();
                }
            });
            connection.send(ServerboundStatusRequestPacket.INSTANCE);
            bridge.pumpUntil(finished, 10_000);
        }
    }

    private static boolean near(Vec3 first, Vec3 second) {
        return Math.abs(first.x - second.x) <= POSITION_TOLERANCE
                && Math.abs(first.y - second.y) <= POSITION_TOLERANCE
                && Math.abs(first.z - second.z) <= POSITION_TOLERANCE;
    }

    /** 官方的包不公开这个字段，客户端要靠 Level 才取得到实体。 */
    private static int privateInt(Object packet, Class<?> owner, String name) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            return field.getInt(packet);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    /** 一步超时了，这一段剩下的步骤没法做。 */
    private static final class StepFailed extends RuntimeException {
        StepFailed(String step) {
            super(step);
        }
    }

    @FunctionalInterface
    private interface Steps {
        void run() throws IOException;
    }

    /** 多名玩家的脚本：所有客户端在一个线程里轮流转发字节，这样脚本可以按顺序写。 */
    private static final class Scenario {
        private final String host;
        private final int port;
        private final LayeredRegistryAccess<RegistryLayer> layers;
        private final List<Join> clients = new ArrayList<>();

        Scenario(String host, int port, LayeredRegistryAccess<RegistryLayer> layers) {
            this.host = host;
            this.port = port;
            this.layers = layers;
        }

        void run() throws IOException {
            Join kitten = connect(PLAYER, HumanoidArm.LEFT, true);
            section("first player", () -> {
                await("join: " + PLAYER + " got the spawn chunks and walked", JOIN_TIMEOUT_MILLIS, kitten::walkDone);
            });

            Join mochi = connect(SECOND_PLAYER, HumanoidArm.RIGHT, false);
            section("second player", () -> {
                await("join: " + SECOND_PLAYER + " got the spawn chunks", JOIN_TIMEOUT_MILLIS, mochi::spawnChunksDone);
                checkJoinMessages(kitten, mochi);
            });
            section("pairing", () -> {
                checkPairing(kitten, mochi);
                checkPairing(mochi, kitten);
            });
            for (Join[] pair : List.of(new Join[] {kitten, mochi}, new Join[] {mochi, kitten})) {
                Join actor = pair[0];
                Join observer = pair[1];
                String names = " (" + actor.player + " seen by " + observer.player + ")";
                section("movement" + names, () -> movement(actor, observer));
                section("metadata" + names, () -> metadata(actor, observer));
                section("swing" + names, () -> swing(actor, observer));
                section("equipment" + names, () -> equipment(actor, observer));
                section("blocks" + names, () -> blocks(actor, observer));
                section("placement" + names, () -> placement(actor, observer));
                section("inventory" + names, () -> inventory(actor, observer));
                section("chat" + names, () -> chat(actor, observer));
            }

            Join late = connect(LATE_PLAYER, HumanoidArm.RIGHT, false);
            section("late joiner", () -> {
                await("join: " + LATE_PLAYER + " got the spawn chunks", JOIN_TIMEOUT_MILLIS, late::spawnChunksDone);
                checkEditedChunks(kitten, mochi, late);
            });
            section("leaving", () -> leaving(kitten, mochi, late));

            section("long running", () -> {
                await("play: keep-alives and the latency broadcast arrived", JOIN_TIMEOUT_MILLIS, kitten::longRunningDone);
            });

            for (Join client : clients) {
                client.finishChecks();
            }
            for (Join client : clients) {
                client.close();
            }
        }

        private Join connect(String name, HumanoidArm arm, boolean walks) throws IOException {
            Join client = new Join(host, port, layers, name, arm, walks);
            clients.add(client);
            client.start();
            return client;
        }

        /** 一段脚本。超时的步骤已经记成失败，这里只说明后面的步骤跳过了。 */
        private void section(String name, Steps steps) throws IOException {
            try {
                steps.run();
            } catch (StepFailed failed) {
                System.out.println("PROBE stopped section \"" + name + "\" at: " + failed.getMessage());
                check("section: " + name + " ran to the end", false);
            }
        }

        private void pumpAll() throws IOException {
            for (Join client : clients) {
                client.pump();
            }
        }

        private void pumpFor(long millis) throws IOException {
            long deadline = System.currentTimeMillis() + millis;
            while (System.currentTimeMillis() < deadline) {
                pumpAll();
            }
        }

        private void await(String what, BooleanSupplier condition) throws IOException {
            await(what, STEP_TIMEOUT_MILLIS, condition);
        }

        /** 等条件成立。超时时记一项失败，跳过这一段剩下的步骤。 */
        private void await(String what, long timeoutMillis, BooleanSupplier condition) throws IOException {
            long deadline = System.currentTimeMillis() + timeoutMillis;
            while (!condition.getAsBoolean()) {
                if (System.currentTimeMillis() >= deadline) {
                    check(what, false);
                    throw new StepFailed(what);
                }
                pumpAll();
            }
            check(what, true);
        }

        private void checkJoinMessages(Join first, Join second) throws IOException {
            String expected = second.player + " joined the game";
            await("chat: " + first.player + " sees the join message", () -> first.systemTexts().contains(expected));
            check("chat: the join message is yellow",
                    first.systemMessages.stream()
                            .filter(message -> message.content().getString().equals(expected))
                            .allMatch(message -> isColored(message.content(), ChatFormatting.YELLOW)));
            check("chat: the join message is not an overlay",
                    first.systemMessages.stream().noneMatch(ClientboundSystemChatPacket::overlay));
            check("chat: the tab entry comes before the join message",
                    first.eventIndex("tab+ " + second.player) < first.eventIndex("system " + expected));
            await("tab: " + first.player + " lists " + second.player, () -> first.tab.containsKey(second.uuid));
            await("tab: " + second.player + " lists " + first.player, () -> second.tab.containsKey(first.uuid));
            check("chat: a new player does not see its own join message",
                    !second.systemTexts().contains(expected));
        }

        private void checkPairing(Join observer, Join target) throws IOException {
            String names = observer.player + " sees " + target.player;
            await("entity: " + names, () -> observer.remotes.containsKey(target.entityId));
            Remote remote = observer.remotes.get(target.entityId);
            check("entity: uuid matches the tab entry", remote.uuid.equals(target.uuid));
            check("entity: " + names + " at the real position", near(remote.position, target.position));
            check("entity: " + names + " after its tab entry",
                    observer.eventIndex("tab+ " + target.player) < observer.eventIndex("add " + target.entityId));
        }

        private static boolean isColored(Component component, ChatFormatting color) {
            return TextColor.fromLegacyFormat(color).equals(component.getStyle().getColor());
        }

        private void walkAndWait(Join actor, Join observer, double deltaZ) throws IOException {
            Vec3 start = actor.position;
            for (int step = 1; step <= WALK_STEPS; step += 1) {
                actor.moveTo(start.add(0.0, 0.0, deltaZ * step), true);
                pumpFor(TICK_MILLIS);
            }
            Vec3 end = actor.position;
            await("movement: " + observer.player + " tracks " + actor.player + " to the real position",
                    () -> near(observer.remote(actor).position, end));
        }

        private void movement(Join actor, Join observer) throws IOException {
            // 先让服务端知道这名玩家站在地上，之后 onGround 不再变。
            actor.moveTo(actor.position, true);
            await("movement: " + actor.player + " stands on the ground",
                    () -> observer.remote(actor).onGround);
            Remote remote = observer.remote(actor);

            int syncs = remote.syncs;
            int moves = remote.moves;
            walkAndWait(actor, observer, WALK_STEP);
            check("movement: walking uses deltas", remote.moves - moves >= 5);
            check("movement: walking needs no full sync", remote.syncs == syncs);
            check("movement: walking keeps onGround", remote.onGround);

            actor.send(new ServerboundMovePlayerPacket.Rot(90.0f, -45.0f, true, false));
            await("movement: " + observer.player + " sees the rotation",
                    () -> remote.yRot == 90.0f && remote.xRot == -45.0f);
            await("movement: " + observer.player + " sees the head yaw", () -> remote.yHeadRot == 90.0f);
            actor.send(new ServerboundMovePlayerPacket.Rot(0.0f, 0.0f, true, false));
            await("movement: rotation goes back to zero", () -> remote.yRot == 0.0f && remote.yHeadRot == 0.0f);

            Vec3 ground = actor.position;
            int beforeJump = remote.syncs;
            actor.moveTo(ground.add(0.0, 1.25, 0.0), false);
            await("movement: jumping is a full sync", () -> remote.syncs > beforeJump);
            check("movement: the sync has the jump position", near(remote.position, actor.position));
            check("movement: the sync says the player is in the air", !remote.onGround);

            int beforeLanding = remote.syncs;
            actor.moveTo(ground, true);
            await("movement: landing is a full sync", () -> remote.syncs > beforeLanding);
            check("movement: the sync has the landing position", near(remote.position, ground));
            check("movement: the sync says the player landed", remote.onGround);

            int beforeTeleport = remote.syncs;
            Vec3 far = ground.add(10.0, 0.0, 0.0);
            actor.moveTo(far, true);
            await("movement: a move too big for a delta is a full sync", () -> remote.syncs > beforeTeleport);
            check("movement: the big move ends at the real position", near(remote.position, far));
            actor.moveTo(ground, true);
            await("movement: and back", () -> near(remote.position, ground));
        }

        private static Object dataValue(Join viewer, Join target, int index) {
            return viewer.metadata.getOrDefault(target.entityId, Map.of()).get(index);
        }

        private void metadata(Join actor, Join observer) throws IOException {
            actor.send(new ServerboundPlayerInputPacket(new Input(false, false, false, false, false, true, false)));
            for (Join viewer : List.of(actor, observer)) {
                await("metadata: " + viewer.player + " sees " + actor.player + " sneak",
                        () -> Byte.valueOf(FLAG_SHIFT).equals(dataValue(viewer, actor, METADATA_SHARED_FLAGS))
                                && dataValue(viewer, actor, METADATA_POSE) == Pose.CROUCHING);
            }

            actor.send(new ServerboundPlayerInputPacket(Input.EMPTY));
            for (Join viewer : List.of(actor, observer)) {
                await("metadata: " + viewer.player + " sees " + actor.player + " stand up",
                        () -> Byte.valueOf((byte) 0).equals(dataValue(viewer, actor, METADATA_SHARED_FLAGS))
                                && dataValue(viewer, actor, METADATA_POSE) == Pose.STANDING);
            }

            actor.send(actor.playerCommand(ServerboundPlayerCommandPacket.Action.START_SPRINTING));
            await("metadata: " + observer.player + " sees " + actor.player + " sprint",
                    () -> Byte.valueOf(FLAG_SPRINTING).equals(dataValue(observer, actor, METADATA_SHARED_FLAGS)));
            actor.send(actor.playerCommand(ServerboundPlayerCommandPacket.Action.STOP_SPRINTING));
            await("metadata: " + observer.player + " sees " + actor.player + " stop sprinting",
                    () -> Byte.valueOf((byte) 0).equals(dataValue(observer, actor, METADATA_SHARED_FLAGS)));

            // 飞行时按潜行键是下降，只改 0 号的潜行位，姿势不变。
            Abilities flying = new Abilities();
            flying.flying = true;
            actor.send(new ServerboundPlayerAbilitiesPacket(flying));
            actor.send(new ServerboundPlayerInputPacket(new Input(false, false, false, false, false, true, false)));
            await("metadata: flying with shift only sets the flag",
                    () -> Byte.valueOf(FLAG_SHIFT).equals(dataValue(observer, actor, METADATA_SHARED_FLAGS)));
            check("metadata: flying with shift keeps the pose",
                    dataValue(observer, actor, METADATA_POSE) == Pose.STANDING);
            actor.send(new ServerboundPlayerInputPacket(Input.EMPTY));
            actor.send(new ServerboundPlayerAbilitiesPacket(new Abilities()));
            await("metadata: the flag clears again",
                    () -> Byte.valueOf((byte) 0).equals(dataValue(observer, actor, METADATA_SHARED_FLAGS)));

            // 滑翔被拒绝：只有自己收到一个不带滑翔位的 0 号。
            int observerData = observer.entityDataPackets;
            int ownData = actor.entityDataPackets;
            actor.send(actor.playerCommand(ServerboundPlayerCommandPacket.Action.START_FALL_FLYING));
            await("metadata: fall flying is refused to the sender", () -> actor.entityDataPackets > ownData);
            check("metadata: the refusal clears the fall flying flag",
                    Byte.valueOf((byte) 0).equals(dataValue(actor, actor, METADATA_SHARED_FLAGS)));
            pumpFor(SETTLE_MILLIS);
            check("metadata: the refusal stays private", observer.entityDataPackets == observerData);

            actor.send(actor.clientInformation(NO_HAT_SKIN_PARTS, actor.arm));
            await("metadata: " + observer.player + " sees the new skin parts",
                    () -> Byte.valueOf((byte) NO_HAT_SKIN_PARTS).equals(dataValue(observer, actor, METADATA_SKIN_PARTS)));
            await("tab: " + observer.player + " sees the hat go", () -> Boolean.FALSE.equals(observer.hats.get(actor.uuid)));
            actor.send(actor.clientInformation(ALL_SKIN_PARTS, actor.arm));
            await("metadata: the skin parts come back",
                    () -> Byte.valueOf((byte) ALL_SKIN_PARTS).equals(dataValue(observer, actor, METADATA_SKIN_PARTS)));
            await("tab: the hat comes back", () -> Boolean.TRUE.equals(observer.hats.get(actor.uuid)));
        }

        private void swing(Join actor, Join observer) throws IOException {
            Remote remote = observer.remote(actor);
            int swings = remote.swings.size();
            actor.send(ServerboundPunchPacket.INSTANCE);
            await("swing: " + observer.player + " sees " + actor.player + " punch", () -> remote.swings.size() > swings);
            ClientboundSwingAnimationPacket packet = remote.swings.getLast();
            check("swing: punching uses the main hand", packet.hand() == InteractionHand.MAIN_HAND);
            check("swing: the animation is the default one",
                    packet.animation().type() == SwingAnimationType.WHACK && packet.animation().duration() == 6);
        }

        private static boolean holds(Remote remote, EquipmentSlot slot, Item item, int count) {
            ItemStack stack = remote.equipment.get(slot);
            return stack != null && stack.is(item) && stack.getCount() == count;
        }

        private static boolean isEmpty(Remote remote, EquipmentSlot slot) {
            ItemStack stack = remote.equipment.get(slot);
            return stack != null && stack.isEmpty();
        }

        private void equipment(Join actor, Join observer) throws IOException {
            Remote remote = observer.remote(actor);
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START, new ItemStack(Items.STONE, STACK)));
            await("equipment: " + observer.player + " sees the stone in the main hand",
                    () -> holds(remote, EquipmentSlot.MAINHAND, Items.STONE, STACK));

            actor.send(new ServerboundSetCarriedItemPacket(1));
            await("equipment: changing to an empty slot empties the main hand",
                    () -> isEmpty(remote, EquipmentSlot.MAINHAND));
            actor.send(new ServerboundSetCarriedItemPacket(0));
            await("equipment: changing back shows the stone again",
                    () -> holds(remote, EquipmentSlot.MAINHAND, Items.STONE, STACK));

            actor.send(new ServerboundSetCreativeModeSlotPacket(5, new ItemStack(Items.DIAMOND_HELMET)));
            await("equipment: the helmet is on the head",
                    () -> holds(remote, EquipmentSlot.HEAD, Items.DIAMOND_HELMET, 1));
            actor.send(new ServerboundSetCreativeModeSlotPacket(5, ItemStack.EMPTY));
            await("equipment: the helmet comes off", () -> isEmpty(remote, EquipmentSlot.HEAD));
        }

        private void blocks(Join actor, Join observer) throws IOException {
            BlockPos feet = BlockPos.containing(actor.position);
            BlockPos broken = feet.offset(2, -1, 0);
            BlockPos ground = feet.offset(-2, -1, 0);
            BlockPos placed = ground.above();
            BlockPos chestGround = feet.offset(-2, -1, 2);
            BlockPos chest = chestGround.above();
            actor.edits.add(new Edit(broken, Blocks.AIR.defaultBlockState()));
            actor.edits.add(new Edit(placed, Blocks.STONE.defaultBlockState()));
            BlockState air = Blocks.AIR.defaultBlockState();
            BlockState grass = Blocks.GRASS_BLOCK.defaultBlockState();
            BlockState stone = Blocks.STONE.defaultBlockState();
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START, new ItemStack(Items.STONE, STACK)));
            actor.send(new ServerboundSetCarriedItemPacket(0));

            int ownEvents = actor.levelEvents.size();
            int otherEvents = observer.levelEvents.size();
            int sequence = actor.nextSequence();
            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.START_DESTROY_BLOCK, broken, Direction.UP, sequence));
            await("blocks: " + actor.player + " gets the ack for breaking", () -> actor.lastAck >= sequence);
            check("blocks: the update comes before the ack",
                    actor.eventIndex("block " + broken.toShortString() + " " + Block.getId(air))
                            < actor.eventIndex("ack " + sequence));
            await("blocks: " + observer.player + " sees the block go", () -> air.equals(observer.blocks.get(broken)));
            check("blocks: the observer gets the break event", observer.levelEvents.size() == otherEvents + 1);
            ClientboundLevelEventPacket event = observer.levelEvents.getLast();
            check("blocks: the break event is 2001", event.getType() == LEVEL_EVENT_BREAK);
            check("blocks: the break event names the position", event.getPos().equals(broken));
            check("blocks: the break event names the old state", event.getData() == Block.getId(grass));
            check("blocks: the break event is local", !event.isGlobalEvent());
            check("blocks: the event comes before the update",
                    observer.eventIndex("event " + broken.toShortString())
                            < observer.eventIndex("block " + broken.toShortString() + " " + Block.getId(air)));
            pumpFor(SETTLE_MILLIS);
            check("blocks: the breaker gets no break event", actor.levelEvents.size() == ownEvents);

            Remote remote = observer.remote(actor);
            int swings = remote.swings.size();
            int observerAcks = observer.acks;
            int placing = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(ground, 1.0), Direction.UP, ground, false),
                    placing));
            await("blocks: " + actor.player + " gets the ack for placing", () -> actor.lastAck >= placing);
            check("blocks: the clicked block is reported", grass.equals(actor.blocks.get(ground)));
            check("blocks: the placed block is reported", stone.equals(actor.blocks.get(placed)));
            check("blocks: the placed block comes before the ack",
                    actor.eventIndex("block " + placed.toShortString() + " " + Block.getId(stone))
                            < actor.eventIndex("ack " + placing));
            await("blocks: " + observer.player + " sees the placed block", () -> stone.equals(observer.blocks.get(placed)));
            await("blocks: " + observer.player + " sees the placing swing", () -> remote.swings.size() > swings);
            pumpFor(SETTLE_MILLIS);
            check("blocks: acks only go to the player who acted", observer.acks == observerAcks);

            // 把方块放进自己身体，只回 ack，世界不变。
            BlockPos underFeet = feet.below();
            int refused = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(underFeet, 1.0), Direction.UP, underFeet, false),
                    refused));
            await("blocks: a refused placement is acked", () -> actor.lastAck >= refused);
            check("blocks: a refused placement reports air", air.equals(actor.blocks.get(feet)));

            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 1, new ItemStack(Items.CHEST)));
            actor.send(new ServerboundSetCarriedItemPacket(1));
            int chestSequence = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(chestGround, 1.0), Direction.UP, chestGround, false),
                    chestSequence));
            await("blocks: " + observer.player + " sees the chest",
                    () -> observer.blocks.containsKey(chest) && observer.blocks.get(chest).is(Blocks.CHEST));
            actor.edits.add(new Edit(chest, observer.blocks.get(chest)));
            actor.send(new ServerboundSetCarriedItemPacket(0));

            // use_item 没有效果，只要 ack。
            int using = actor.nextSequence();
            actor.send(new ServerboundUseItemPacket(
                    InteractionHand.MAIN_HAND, using, 0.0f, 0.0f));
            await("blocks: use_item is acked", () -> actor.lastAck >= using);
        }

        /** 放下的方块要和官方算出来的状态一样，失去支撑的方块要和支撑一起消失。 */
        private void placement(Join actor, Join observer) throws IOException {
            BlockPos stone = BlockPos.containing(actor.position).offset(-2, 0, 0);
            BlockPos log = stone.west();
            BlockPos slab = stone.north();
            BlockPos torch = log.above();
            BlockState air = Blocks.AIR.defaultBlockState();
            BlockState sideways = Blocks.OAK_LOG.defaultBlockState().setValue(RotatedPillarBlock.AXIS, Direction.Axis.X);
            BlockState upperSlab = Blocks.OAK_SLAB.defaultBlockState().setValue(SlabBlock.TYPE, SlabType.TOP);
            BlockState standing = Blocks.TORCH.defaultBlockState();
            actor.send(new ServerboundSetCarriedItemPacket(1));

            useOn(actor, Items.OAK_LOG, stone, Direction.WEST, Vec3.atCenterOf(stone).add(-0.5, 0.0, 0.0));
            await("placement: the log lies along the clicked face", () -> sideways.equals(observer.blocks.get(log)));
            useOn(actor, Items.OAK_SLAB, stone, Direction.NORTH, Vec3.atCenterOf(stone).add(0.0, 0.25, -0.5));
            await("placement: the slab goes to the clicked half", () -> upperSlab.equals(observer.blocks.get(slab)));
            useOn(actor, Items.TORCH, log, Direction.UP, Vec3.upFromBottomCenterOf(log, 1.0));
            await("placement: the torch stands on the log", () -> standing.equals(observer.blocks.get(torch)));

            int ownEvents = actor.levelEvents.size();
            int otherEvents = observer.levelEvents.size();
            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.START_DESTROY_BLOCK, log, Direction.UP, actor.nextSequence()));
            await("placement: the torch goes with the log", () -> air.equals(observer.blocks.get(torch)));
            pumpFor(SETTLE_MILLIS);
            check("placement: the observer gets both break events", observer.levelEvents.size() == otherEvents + 2);
            check("placement: the breaker only gets the event of the torch",
                    actor.levelEvents.size() == ownEvents + 1
                            && actor.levelEvents.getLast().getData() == Block.getId(standing));

            actor.cleared.add(log);
            actor.cleared.add(torch);
            actor.edits.add(new Edit(slab, upperSlab));
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 1, new ItemStack(Items.CHEST)));
            actor.send(new ServerboundSetCarriedItemPacket(0));
        }

        /** 手持 `item` 右键方块，等服务端确认。物品放在第 1 格，这一段开始时已经选中它。 */
        private void useOn(Join actor, Item item, BlockPos clicked, Direction face, Vec3 location) throws IOException {
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 1, new ItemStack(item)));
            int sequence = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND, new BlockHitResult(location, face, clicked, false), sequence));
            await("placement: " + actor.player + " gets the ack", () -> actor.lastAck >= sequence);
        }

        private void inventory(Join actor, Join observer) throws IOException {
            BlockPos feet = BlockPos.containing(actor.position);
            BlockPos stone = feet.offset(-2, 0, 0);
            BlockPos dirt = feet.below(2);

            // 石头在快捷栏第 0 格，选中第 2 格时中键把选中格切回去。
            actor.send(new ServerboundSetCarriedItemPacket(2));
            actor.send(new ServerboundPickItemFromBlockPacket(stone, false));
            await("inventory: picking a block on the hotbar selects it", () -> actor.heldSlot == 0);
            actor.send(new ServerboundSetCarriedItemPacket(0));

            // 泥土不在物品栏里，放进下一个空格：第 0 格是石头，第 1 格是箱子。
            actor.send(new ServerboundPickItemFromBlockPacket(dirt, false));
            await("inventory: picking a new block selects the next empty slot", () -> actor.heldSlot == 2);
            await("inventory: the picked block is in that slot",
                    () -> actor.slots.containsKey(HOTBAR_START + 2) && actor.slots.get(HOTBAR_START + 2).is(Items.DIRT));
            actor.send(new ServerboundSetCarriedItemPacket(2));

            Remote remote = observer.remote(actor);
            await("inventory: the observer sees the picked block",
                    () -> holds(remote, EquipmentSlot.MAINHAND, Items.DIRT, 1));

            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.SWAP_ITEM_WITH_OFFHAND, BlockPos.ZERO, Direction.DOWN));
            await("inventory: swapping hands updates the off hand",
                    () -> actor.slots.containsKey(OFF_HAND_SLOT) && actor.slots.get(OFF_HAND_SLOT).is(Items.DIRT));
            await("inventory: swapping hands empties the main hand",
                    () -> actor.slots.get(HOTBAR_START + 2).isEmpty());
            await("inventory: the observer sees both hands",
                    () -> isEmpty(remote, EquipmentSlot.MAINHAND) && holds(remote, EquipmentSlot.OFFHAND, Items.DIRT, 1));

            actor.send(new ServerboundSetCarriedItemPacket(0));
            await("inventory: the stone is back in the main hand",
                    () -> holds(remote, EquipmentSlot.MAINHAND, Items.STONE, STACK));
            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.DROP_ITEM, BlockPos.ZERO, Direction.DOWN));
            await("inventory: Q takes one item", () -> holds(remote, EquipmentSlot.MAINHAND, Items.STONE, STACK - 1));
            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.DROP_ALL_ITEMS, BlockPos.ZERO, Direction.DOWN));
            await("inventory: ctrl+Q takes the stack", () -> isEmpty(remote, EquipmentSlot.MAINHAND));
        }

        private void chat(Join actor, Join observer) throws IOException {
            String expected = "<" + actor.player + "> " + CHAT_TEXT;
            actor.send(new ServerboundChatPacket(
                    CHAT_TEXT, Instant.now(), 0L, Optional.empty(), new LastSeenMessages.Update(0, new BitSet(), (byte) 1)));
            await("chat: " + actor.player + " sees its own message", () -> actor.chats.contains(expected));
            await("chat: " + observer.player + " sees the message", () -> observer.chats.contains(expected));
            check("chat: the narration names the speaker",
                    actor.narrations.contains(actor.player + " says " + CHAT_TEXT));

            int ownMessages = actor.systemMessages.size();
            int otherMessages = observer.systemMessages.size();
            actor.send(new ServerboundChatCommandPacket("foo bar"));
            await("chat: an unknown command is answered", () -> actor.systemTexts().contains(UNKNOWN_COMMAND));
            ClientboundSystemChatPacket answer = actor.systemMessages.get(ownMessages);
            check("chat: the error is red", isColored(answer.content(), ChatFormatting.RED));
            check("chat: the error is not an overlay", !answer.overlay());
            pumpFor(SETTLE_MILLIS);
            check("chat: the error only goes to the sender", observer.systemMessages.size() == otherMessages);

            int ownStats = actor.stats;
            int otherStats = observer.stats;
            actor.send(new ServerboundClientCommandPacket(ServerboundClientCommandPacket.Action.REQUEST_STATS));
            await("chat: the statistics screen is answered", () -> actor.stats > ownStats);
            pumpFor(SETTLE_MILLIS);
            check("chat: statistics only go to the requester", observer.stats == otherStats);
        }

        /** 后来的玩家看到的区块要有之前所有的改动，方块实体也要在列表里。 */
        private void checkEditedChunks(Join first, Join second, Join late) throws IOException {
            await("late: the tab list holds three players", () -> late.tab.size() == 3);
            await("late: " + LATE_PLAYER + " sees both players",
                    () -> late.remotes.containsKey(first.entityId) && late.remotes.containsKey(second.entityId));
            for (Join editor : List.of(first, second)) {
                for (Edit edit : editor.edits) {
                    check("late: the chunk has every edit", edit.state.equals(late.blocks.get(edit.pos)));
                    if (edit.state.is(Blocks.CHEST)) {
                        check("late: the chest is in the block entity list",
                                late.blockEntities.get(edit.pos) == BlockEntityTypes.CHEST);
                    }
                }
                for (BlockPos pos : editor.cleared) {
                    check("late: blocks that went away are not in the chunk", !late.blocks.containsKey(pos));
                }
            }
        }

        private void leaving(Join stays, Join leaves, Join late) throws IOException {
            String expected = leaves.player + " left the game";
            int leaverId = leaves.entityId;
            leaves.finishChecks();
            leaves.close();
            clients.remove(leaves);
            for (Join watcher : List.of(stays, late)) {
                await("leave: " + watcher.player + " sees the left message", () -> watcher.systemTexts().contains(expected));
                await("leave: " + watcher.player + " drops the entity", () -> !watcher.remotes.containsKey(leaverId));
                await("leave: " + watcher.player + " drops the tab entry", () -> !watcher.tab.containsKey(leaves.uuid));
                check("leave: the message comes first",
                        watcher.eventIndex("system " + expected) < watcher.eventIndex("remove " + leaverId));
                check("leave: the entity goes before the tab entry",
                        watcher.eventIndex("remove " + leaverId) < watcher.eventIndex("tab- " + leaves.uuid));
            }
        }
    }

    /** 一次改动：这名玩家让世界的这一格变成了这个状态。 */
    private record Edit(BlockPos pos, BlockState state) {}

    /** 客户端眼里的另一名玩家。位置用官方的 VecDeltaCodec 累加。 */
    private static final class Remote {
        final UUID uuid;
        final VecDeltaCodec codec = new VecDeltaCodec();
        final Map<EquipmentSlot, ItemStack> equipment = new EnumMap<>(EquipmentSlot.class);
        final List<ClientboundSwingAnimationPacket> swings = new ArrayList<>();
        Vec3 position;
        float yRot;
        float xRot;
        float yHeadRot;
        boolean onGround;
        int moves;
        int syncs;

        Remote(ClientboundAddEntityPacket packet) {
            uuid = packet.getUUID();
            position = new Vec3(packet.getX(), packet.getY(), packet.getZ());
            codec.setBase(position);
            yRot = packet.getYRot();
            xRot = packet.getXRot();
            yHeadRot = packet.getYHeadRot();
        }
    }

    /** 一个官方客户端：进服流程，以及它收到的所有东西。 */
    private static final class Join {
        final String player;
        final UUID uuid;
        final HumanoidArm arm;
        private final String host;
        private final int port;
        private final boolean walks;
        private final LayeredRegistryAccess<RegistryLayer> layers;
        private final RegistryAccess.Frozen registries;
        private final PalettedContainerFactory containers;
        private final Bridge bridge;
        private final Connection connection;
        private ClientGamePacketListener gameListener;

        private final Map<String, List<String>> receivedRegistries = new HashMap<>();
        private final Map<String, Integer> playPackets = new TreeMap<>();
        private Map<ResourceKey<? extends Registry<?>>, TagNetworkSerialization.NetworkPayload> receivedTags;
        private final Set<ChunkPos> spawnChunks = new HashSet<>();
        private final Set<ChunkPos> firstBatch = new HashSet<>();
        private final Set<ChunkPos> walkEntered = new HashSet<>();
        private final Set<ChunkPos> walkForgotten = new HashSet<>();
        private final Set<ChunkPos> expectedSpawnChunks = chunksOf(SPAWN_CHUNK);
        private final Set<ChunkPos> expectedEntered = new HashSet<>();
        private final Set<ChunkPos> expectedForgotten = new HashSet<>();
        private int chunks;
        private int batchTotal;
        private boolean firstChunkChecked;
        private boolean spawnCenterChecked;
        private boolean walked;
        private boolean walkCenterArrived;
        private boolean forgetBeforeCenter;
        private int keepAlives;
        private boolean latencyUpdated;
        private boolean compressionSeen;
        private boolean disconnected;
        private boolean ownDataChecked;
        private boolean inBundle;
        private boolean closed;
        private String disconnectReason = "";
        private int sequence;

        int entityId;
        Vec3 position = new Vec3(SPAWN_X, SPAWN_Y, SPAWN_Z);
        final Map<UUID, String> tab = new HashMap<>();
        final Map<UUID, Boolean> hats = new HashMap<>();
        final Map<Integer, Remote> remotes = new HashMap<>();
        final Map<Integer, Map<Integer, Object>> metadata = new HashMap<>();
        final Map<BlockPos, BlockState> blocks = new HashMap<>();
        final Map<BlockPos, BlockEntityType<?>> blockEntities = new HashMap<>();
        final Map<Integer, ItemStack> slots = new HashMap<>();
        final List<Edit> edits = new ArrayList<>();
        /** 放下又消失的方块。地面上的空气 `recordChunk` 不记，所以不能写成 `Edit`。 */
        final List<BlockPos> cleared = new ArrayList<>();
        final List<String> events = new ArrayList<>();
        final List<ClientboundLevelEventPacket> levelEvents = new ArrayList<>();
        final List<ClientboundSystemChatPacket> systemMessages = new ArrayList<>();
        final List<String> chats = new ArrayList<>();
        final List<String> narrations = new ArrayList<>();
        int entityDataPackets;
        int heldSlot = -1;
        int lastAck = -1;
        int acks;
        int stats;

        Join(
                String host,
                int port,
                LayeredRegistryAccess<RegistryLayer> layers,
                String player,
                HumanoidArm arm,
                boolean walks) throws IOException {
            this.host = host;
            this.port = port;
            this.layers = layers;
            this.player = player;
            this.uuid = UUIDUtil.createOfflinePlayerUUID(player);
            this.arm = arm;
            this.walks = walks;
            this.registries = layers.compositeAccess();
            this.containers = PalettedContainerFactory.create(registries);
            this.bridge = new Bridge(host, port);
            this.connection = bridge.connection;
            ChunkTrackingView.difference(
                    ChunkTrackingView.of(SPAWN_CHUNK, VIEW_DISTANCE),
                    ChunkTrackingView.of(WALK_TARGET, VIEW_DISTANCE),
                    expectedEntered::add,
                    expectedForgotten::add);
        }

        /** 官方的跟踪范围，也就是服务端要发的全部区块。 */
        private static Set<ChunkPos> chunksOf(ChunkPos center) {
            Set<ChunkPos> positions = new HashSet<>();
            ChunkTrackingView.of(center, VIEW_DISTANCE).forEach(positions::add);
            return positions;
        }

        void start() {
            connection.initiateServerboundPlayConnection(host, port, listener(ClientLoginPacketListener.class, Map.of(
                    "handleLoginFinished", packet -> onLoginFinished((ClientboundLoginFinishedPacket) packet),
                    "handleCompression", packet -> onCompression((ClientboundLoginCompressionPacket) packet),
                    "handleDisconnect", packet -> onLoginRefused((ClientboundLoginDisconnectPacket) packet))));
            connection.send(new ServerboundHelloPacket(player, uuid));
        }

        /** 转发一轮字节。服务端主动断开或者官方解码器报错时，连接会在这里结束。 */
        void pump() throws IOException {
            if (closed) {
                return;
            }
            bridge.pump();
            if (bridge.channelClosed()) {
                closed = true;
                DisconnectionDetails details = connection.getDisconnectionDetails();
                String reason = details == null ? "none" : details.reason().getString();
                System.out.println("PROBE connection of " + player + " closed: " + reason);
                check("connection: " + player + " stayed connected (" + reason + ")", false);
            }
        }

        void send(Packet<?> packet) throws IOException {
            connection.send(packet);
            bridge.flushOutbound();
        }

        void moveTo(Vec3 target, boolean onGround) throws IOException {
            position = target;
            send(new ServerboundMovePlayerPacket.Pos(target, onGround, false));
        }

        int nextSequence() {
            sequence += 1;
            return sequence;
        }

        /** 官方的构造函数要一个 Entity，这里用官方解码器造这个包。 */
        Packet<?> playerCommand(ServerboundPlayerCommandPacket.Action action) {
            FriendlyByteBuf buffer = new FriendlyByteBuf(Unpooled.buffer());
            buffer.writeVarInt(entityId);
            buffer.writeVarInt(action.ordinal());
            buffer.writeVarInt(0);
            return ServerboundPlayerCommandPacket.STREAM_CODEC.decode(buffer);
        }

        Packet<?> clientInformation(int skinParts, HumanoidArm mainHand) {
            return new ServerboundClientInformationPacket(new ClientInformation(
                    "en_us",
                    VIEW_DISTANCE,
                    ChatVisiblity.FULL,
                    true,
                    skinParts,
                    mainHand,
                    false,
                    true,
                    ParticleStatus.ALL));
        }

        Remote remote(Join other) {
            Remote remote = remotes.get(other.entityId);
            if (remote == null) {
                throw new StepFailed(player + " does not see " + other.player);
            }
            return remote;
        }

        /** 这件事在收到的包里的位置，没发生时是最大值，这样“先于”的比较一定失败。 */
        int eventIndex(String event) {
            int index = events.indexOf(event);
            return index < 0 ? Integer.MAX_VALUE : index;
        }

        List<String> systemTexts() {
            return systemMessages.stream().map(message -> message.content().getString()).toList();
        }

        boolean spawnChunksDone() {
            return spawnChunks.equals(expectedSpawnChunks);
        }

        boolean walkDone() {
            return spawnChunksDone() && walkEntered.size() == expectedEntered.size();
        }

        /** 区块全部到齐、走过一个区块、收到两个心跳和一次延迟广播之后就可以结束了。 */
        boolean longRunningDone() {
            return walkDone() && latencyUpdated && keepAlives >= KEEP_ALIVES_TO_ANSWER;
        }

        void finishChecks() {
            check("decode: every packet of " + player + " decoded without trailing bytes", !closed);
            check("play: chunks arrived", spawnChunks.equals(expectedSpawnChunks));
            check("play: batch sizes add up to the chunk count", batchTotal == chunks);
            check("play: first batch is the three by three around the spawn", firstBatchIsAroundTheSpawn());
            check("play: tab list has the player", tab.containsKey(uuid));
            check("play: was not disconnected", !disconnected);
            check("compression: " + player + " received compressed frames", bridge.compressedFrames > 0);
            check("compression: " + player + " received raw frames below the threshold", bridge.rawFrames > 0);
            check("compression: " + player + " never compressed below the threshold",
                    bridge.belowThresholdFrames == 0);
            playPackets.keySet().forEach(name -> check("play: M2 sends " + name, M2_PACKETS.contains(name)));
            if (walks) {
                check("walk: center moved first", walkCenterArrived && !forgetBeforeCenter);
                check("walk: forgotten chunks match the official difference", walkForgotten.equals(expectedForgotten));
                check("walk: new chunks match the official difference", walkEntered.equals(expectedEntered));
                check("play: stayed connected through a keep-alive", playPackets.containsKey("handleKeepAlive"));
                check("play: the answer to the first keep-alive was accepted", keepAlives >= KEEP_ALIVES_TO_ANSWER);
                check("play: latency reached the tab list", latencyUpdated);
            }
            if (disconnected) {
                System.out.println("PROBE disconnected: " + disconnectReason);
            }
            System.out.println("PROBE play packets of " + player + ": " + playPackets);
            System.out.println("PROBE chunks of " + player + ": " + spawnChunks.size() + " at the spawn, "
                    + walkForgotten.size() + " forgotten and " + walkEntered.size() + " new after walking");
        }

        void close() throws IOException {
            if (!closed) {
                closed = true;
                bridge.close();
            }
        }

        private boolean firstBatchIsAroundTheSpawn() {
            if (firstBatch.size() != FIRST_BATCH_CHUNKS) {
                return false;
            }
            return firstBatch.stream().allMatch(pos -> Math.abs(pos.x()) <= 1 && Math.abs(pos.z()) <= 1);
        }

        /** 和官方客户端一样：收到登录压缩包之后才把管线换成压缩帧。 */
        private void onCompression(ClientboundLoginCompressionPacket packet) {
            int threshold = packet.getCompressionThreshold();
            compressionSeen = true;
            check("login: " + player + " was told the official compression threshold",
                    threshold == COMPRESSION_THRESHOLD);
            bridge.enableCompression(threshold);
        }

        private void onLoginRefused(ClientboundLoginDisconnectPacket packet) {
            System.out.println("PROBE login refused: " + packet.reason().getString());
            check("login: accepted", false);
        }

        private void onLoginFinished(ClientboundLoginFinishedPacket packet) {
            check("login: name is kept", packet.gameProfile().name().equals(player));
            check("login: uuid uses the offline algorithm", packet.gameProfile().id().equals(uuid));
            check("login: session id is present", packet.sessionId() != null);
            check("login: compression comes before the login success of " + player, compressionSeen);

            connection.setupInboundProtocol(ConfigurationProtocols.CLIENTBOUND, listener(
                    ClientConfigurationPacketListener.class, Map.of(
                            "handleEnabledFeatures",
                            value -> onEnabledFeatures((ClientboundUpdateEnabledFeaturesPacket) value),
                            "handleSelectKnownPacks",
                            value -> onSelectKnownPacks((ClientboundSelectKnownPacks) value),
                            "handleRegistryData",
                            value -> onRegistryData((ClientboundRegistryDataPacket) value),
                            "handleUpdateTags",
                            value -> receivedTags = ((ClientboundUpdateTagsPacket) value).tags(),
                            "handleConfigurationFinished", value -> onConfigurationFinished(),
                            "handleDisconnect", value -> onDisconnected((ClientboundDisconnectPacket) value))));
            connection.send(ServerboundLoginAcknowledgedPacket.INSTANCE);
            connection.setupOutboundProtocol(ConfigurationProtocols.SERVERBOUND);
            connection.send(new ServerboundCustomPayloadPacket(new BrandPayload("vanilla")));
            // 默认值的视距是 2，这里要和真实客户端一样报告自己的渲染距离。
            connection.send(clientInformation(ALL_SKIN_PARTS, arm));
        }

        private void onEnabledFeatures(ClientboundUpdateEnabledFeaturesPacket packet) {
            check("configuration: vanilla feature is enabled",
                    packet.features().contains(Identifier.parse("minecraft:vanilla")));
        }

        private void onSelectKnownPacks(ClientboundSelectKnownPacks packet) {
            check("configuration: core pack is offered", !packet.knownPacks().isEmpty());
            connection.send(new ServerboundSelectKnownPacks(packet.knownPacks()));
        }

        private void onRegistryData(ClientboundRegistryDataPacket packet) {
            check("configuration: registry entries carry no data",
                    packet.entries().stream().allMatch(entry -> entry.data().isEmpty()));
            receivedRegistries.put(
                    packet.registry().identifier().toString(),
                    packet.entries().stream().map(entry -> entry.id().toString()).toList());
        }

        private void onConfigurationFinished() {
            compareRegistries();
            compareTags();

            ProtocolInfo<ClientGamePacketListener> inbound =
                    GameProtocols.CLIENTBOUND_TEMPLATE.bind(RegistryFriendlyByteBuf.decorator(registries));
            gameListener = listener(ClientGamePacketListener.class, playHandlers());
            connection.setupInboundProtocol(inbound, gameListener);
            connection.send(ServerboundFinishConfigurationPacket.INSTANCE);
            connection.setupOutboundProtocol(GameProtocols.SERVERBOUND_TEMPLATE.bind(
                    RegistryFriendlyByteBuf.decorator(registries), new GameProtocols.Context() {
                        @Override
                        public boolean hasInfiniteMaterials() {
                            return true;
                        }

                        @Override
                        public boolean canUseCommandBlocks() {
                            return false;
                        }
                    }));
        }

        /** 注册表的顺序决定数字 ID，所以名字和顺序都必须和官方完全相同。 */
        private void compareRegistries() {
            RegistryAccess world = layers.getAccessFrom(RegistryLayer.WORLD);
            int expectedRegistries = 0;
            for (RegistryDataLoader.RegistryData<?> data : RegistryDataLoader.SYNCHRONIZED_REGISTRIES) {
                var registry = world.lookup(data.key());
                if (registry.isEmpty()) {
                    continue;
                }
                expectedRegistries += 1;
                List<String> expected = new ArrayList<>();
                registry.get().listElements().forEach(element -> expected.add(element.key().identifier().toString()));
                String name = data.key().identifier().toString();
                check("configuration: registries match the official order",
                        expected.equals(receivedRegistries.get(name)));
            }
            check("configuration: every synced registry arrived", receivedRegistries.size() == expectedRegistries);
        }

        private void compareTags() {
            var expected = TagNetworkSerialization.serializeTagsToNetwork(layers);
            check("configuration: tags arrived", receivedTags != null);
            check("configuration: tags cover the same registries",
                    receivedTags != null && expected.keySet().equals(receivedTags.keySet()));
            if (receivedTags == null) {
                return;
            }
            expected.forEach((registry, payload) -> {
                var received = receivedTags.get(registry);
                check("configuration: tags match the official contents",
                        received != null && payload.tags().equals(received.tags()));
            });
        }

        private Map<String, Consumer<Object>> playHandlers() {
            Map<String, Consumer<Object>> handlers = new HashMap<>();
            handlers.put("handleLogin", value -> onLogin((ClientboundLoginPacket) value));
            handlers.put("handlePlayerAbilities", value -> onAbilities((ClientboundPlayerAbilitiesPacket) value));
            handlers.put("handleMovePlayer", value -> onPosition((ClientboundPlayerPositionPacket) value));
            handlers.put("handlePlayerInfoUpdate", value -> onPlayerInfo((ClientboundPlayerInfoUpdatePacket) value));
            handlers.put("handlePlayerInfoRemove", value -> onPlayerInfoRemove((ClientboundPlayerInfoRemovePacket) value));
            handlers.put("handleSetTime", value -> onSetTime((ClientboundSetTimePacket) value));
            handlers.put("handleGameEvent", value -> onGameEvent((ClientboundGameEventPacket) value));
            handlers.put("handleSetChunkCacheCenter",
                    value -> onChunkCenter((ClientboundSetChunkCacheCenterPacket) value));
            handlers.put("handleForgetLevelChunk",
                    value -> onForgetChunk((ClientboundForgetLevelChunkPacket) value));
            handlers.put("handleLevelChunkWithLight",
                    value -> onChunk((ClientboundLevelChunkWithLightPacket) value));
            handlers.put("handleLightUpdatePacket",
                    value -> onLightUpdate((ClientboundLightUpdatePacket) value));
            handlers.put("handleChunkBatchFinished",
                    value -> onBatchFinished((ClientboundChunkBatchFinishedPacket) value));
            handlers.put("handleSetEntityData", value -> onEntityData((ClientboundSetEntityDataPacket) value));
            handlers.put("handleUpdateAttributes",
                    value -> onAttributes((ClientboundUpdateAttributesPacket) value));
            handlers.put("handleKeepAlive", value -> onKeepAlive((ClientboundKeepAlivePacket) value));
            handlers.put("handleDisconnect", value -> onDisconnected((ClientboundDisconnectPacket) value));
            handlers.put("handleBundlePacket", value -> onBundle((ClientboundBundlePacket) value));
            handlers.put("handleAddEntity", value -> onAddEntity((ClientboundAddEntityPacket) value));
            handlers.put("handleRemoveEntities", value -> onRemoveEntities((ClientboundRemoveEntitiesPacket) value));
            handlers.put("handleMoveEntity", value -> onMoveEntity((ClientboundMoveEntityPacket) value));
            handlers.put("handleEntityPositionSync",
                    value -> onPositionSync((ClientboundEntityPositionSyncPacket) value));
            handlers.put("handleRotateMob", value -> onRotateHead((ClientboundRotateHeadPacket) value));
            handlers.put("handleSwingAnimation", value -> onSwing((ClientboundSwingAnimationPacket) value));
            handlers.put("handleSetEquipment", value -> onEquipment((ClientboundSetEquipmentPacket) value));
            handlers.put("handleBlockUpdate", value -> onBlockUpdate((ClientboundBlockUpdatePacket) value));
            handlers.put("handleBlockChangedAck", value -> onAck((ClientboundBlockChangedAckPacket) value));
            handlers.put("handleLevelEvent", value -> onLevelEvent((ClientboundLevelEventPacket) value));
            handlers.put("handleContainerSetSlot", value -> onSetSlot((ClientboundContainerSetSlotPacket) value));
            handlers.put("handleSetHeldSlot", value -> heldSlot = ((ClientboundSetHeldSlotPacket) value).slot());
            handlers.put("handleSystemChat", value -> onSystemChat((ClientboundSystemChatPacket) value));
            handlers.put("handleDisguisedChat", value -> onDisguisedChat((ClientboundDisguisedChatPacket) value));
            handlers.put("handleAwardStats", value -> onAwardStats((ClientboundAwardStatsPacket) value));
            return handlers;
        }

        private void onLogin(ClientboundLoginPacket packet) {
            var spawn = packet.commonPlayerSpawnInfo();
            entityId = packet.playerId();
            check("play: login lists the overworld", packet.levels().contains(Level.OVERWORLD));
            check("play: dimension type is the overworld", spawn.dimensionType().is(BuiltinDimensionTypes.OVERWORLD));
            check("play: spawn dimension is the overworld", spawn.dimension().equals(Level.OVERWORLD));
            check("play: game mode is creative", spawn.gameType() == GameType.CREATIVE);
            check("play: no previous game mode", spawn.previousGameType().isEmpty());
            check("play: world is flat", spawn.isFlat());
            check("play: sea level matches the flat preset", spawn.seaLevel() == -63);
            check("play: view distance", packet.chunkRadius() == VIEW_DISTANCE);
            check("play: offline mode", !packet.onlineMode());
        }

        private void onAbilities(ClientboundPlayerAbilitiesPacket packet) {
            check("play: creative abilities", packet.canFly() && packet.canInstabuild() && packet.isInvulnerable());
        }

        private void onPosition(ClientboundPlayerPositionPacket packet) {
            var target = packet.change().position();
            check("play: spawn is above the grass", target.y() == SPAWN_Y);
            check("play: position is absolute", packet.relatives().isEmpty());
            connection.send(new ServerboundAcceptTeleportationPacket(
                    packet.id(), target.x(), target.y(), target.z(), 0f, 0f));
        }

        private void onPlayerInfo(ClientboundPlayerInfoUpdatePacket packet) {
            // 不带 ADD_PLAYER 的包里 profile 是 null，这种包是周期性的延迟广播和帽子变化。
            var actions = packet.actions();
            if (actions.equals(EnumSet.of(ClientboundPlayerInfoUpdatePacket.Action.UPDATE_HAT))) {
                packet.entries().forEach(entry -> hats.put(entry.profileId(), entry.showHat()));
                return;
            }
            if (!actions.contains(ClientboundPlayerInfoUpdatePacket.Action.ADD_PLAYER)) {
                onLatencyUpdate(packet);
                return;
            }
            check("play: tab update carries all eight actions",
                    actions.equals(EnumSet.allOf(ClientboundPlayerInfoUpdatePacket.Action.class)));
            for (var entry : packet.entries()) {
                String name = entry.profile().name();
                check("play: tab uuid uses the offline algorithm",
                        entry.profileId().equals(UUIDUtil.createOfflinePlayerUUID(name)));
                check("play: tab entry is listed", entry.listed());
                check("play: tab entry has the game mode", entry.gameMode() == GameType.CREATIVE);
                check("play: tab entry has no chat session", entry.chatSession() == null);
                check("play: tab entry has no display name", entry.displayName() == null);
                check("play: tab entry shows the hat", entry.showHat());
                tab.put(entry.profileId(), name);
                hats.put(entry.profileId(), entry.showHat());
                events.add("tab+ " + name);
            }
        }

        private void onPlayerInfoRemove(ClientboundPlayerInfoRemovePacket packet) {
            for (UUID removed : packet.profileIds()) {
                check("tab: removed players were listed", tab.containsKey(removed));
                tab.remove(removed);
                events.add("tab- " + removed);
            }
        }

        private void onLatencyUpdate(ClientboundPlayerInfoUpdatePacket packet) {
            check("play: periodic tab update only carries the latency",
                    packet.actions().equals(EnumSet.of(ClientboundPlayerInfoUpdatePacket.Action.UPDATE_LATENCY)));
            check("play: periodic tab update names the player",
                    packet.entries().stream().anyMatch(entry -> entry.profileId().equals(uuid)));
            check("play: periodic tab update lists everyone online", packet.entries().size() == tab.size());
            latencyUpdated = true;
        }

        private void onSetTime(ClientboundSetTimePacket packet) {
            check("play: one clock is synced", packet.clockUpdates().size() == 1);
            packet.clockUpdates().forEach((clock, state) -> {
                check("play: clock is the overworld one",
                        clock.unwrapKey().map(key -> key.identifier().getPath().equals("overworld")).orElse(false));
                check("play: clock is at noon", state.totalTicks() == 6000);
            });
        }

        private void onGameEvent(ClientboundGameEventPacket packet) {
            check("play: chunk loading event", packet.getEvent() == ClientboundGameEventPacket.LEVEL_CHUNKS_LOAD_START);
        }

        private void onChunkCenter(ClientboundSetChunkCacheCenterPacket packet) {
            if (walked) {
                walkCenterArrived |= packet.getX() == WALK_TARGET.x() && packet.getZ() == WALK_TARGET.z();
                return;
            }
            if (!spawnCenterChecked) {
                spawnCenterChecked = true;
                check("play: chunk center is the spawn chunk", packet.getX() == 0 && packet.getZ() == 0);
            }
        }

        private void onForgetChunk(ClientboundForgetLevelChunkPacket packet) {
            if (walks && !walkDone()) {
                forgetBeforeCenter |= !walkCenterArrived;
                walkForgotten.add(packet.pos());
            }
        }

        private void onChunk(ClientboundLevelChunkWithLightPacket packet) {
            chunks += 1;
            ChunkPos pos = new ChunkPos(packet.x(), packet.z());
            // 之后的走动也会带来新区块，开头的检查只看第一次。
            if (walked && !walkDone()) {
                walkEntered.add(pos);
            } else if (!spawnChunksDone()) {
                spawnChunks.add(pos);
            }
            if (firstBatch.size() < FIRST_BATCH_CHUNKS) {
                firstBatch.add(pos);
            }
            if (!firstChunkChecked) {
                firstChunkChecked = true;
                check("play: nearest chunk comes first", packet.x() == 0 && packet.z() == 0);
                checkChunkContents(packet);
            }
            recordChunk(packet);
        }

        private void onEntityData(ClientboundSetEntityDataPacket packet) {
            entityDataPackets += 1;
            var values = packet.packedItems();
            Map<Integer, Object> known = metadata.computeIfAbsent(packet.id(), id -> new HashMap<>());
            values.forEach(value -> known.put(value.id(), value.value()));
            if (packet.id() == entityId && !ownDataChecked) {
                ownDataChecked = true;
                check("play: own entity data has two values", values.size() == 2);
                check("play: main hand comes from the client information",
                        values.getFirst().id() == METADATA_MAIN_HAND && values.getFirst().value() == arm);
                check("play: skin parts come from the client information",
                        values.getLast().id() == METADATA_SKIN_PARTS
                                && values.getLast().value().equals((byte) ALL_SKIN_PARTS));
            }
            check("metadata: indices go up", isSorted(values));
        }

        private static boolean isSorted(List<SynchedEntityData.DataValue<?>> values) {
            for (int index = 1; index < values.size(); index += 1) {
                if (values.get(index - 1).id() >= values.get(index).id()) {
                    return false;
                }
            }
            return true;
        }

        private void onAttributes(ClientboundUpdateAttributesPacket packet) {
            check("play: attributes are only sent to the player itself", packet.getEntityId() == entityId);
            var names = packet.getValues().stream()
                    .map(snapshot -> snapshot.attribute().unwrapKey().orElseThrow().identifier().getPath())
                    .toList();
            check("play: creative reach attributes",
                    names.equals(List.of("block_interaction_range", "entity_interaction_range")));
            check("play: reach attributes have the creative bonus",
                    packet.getValues().stream().allMatch(snapshot -> snapshot.modifiers().size() == 1));
        }

        /** 把 bundle 里的包按顺序交给同一个监听器，和 ClientPacketListener.handleBundlePacket 一样。 */
        private void onBundle(ClientboundBundlePacket packet) {
            List<Packet<? super ClientGamePacketListener>> inner = new ArrayList<>();
            packet.subPackets().forEach(inner::add);
            check("bundle: bundles are not empty", !inner.isEmpty());
            if (inner.getFirst() instanceof ClientboundAddEntityPacket) {
                checkPairingBundle(inner);
            }
            inBundle = true;
            inner.forEach(subPacket -> subPacket.handle(gameListener));
            inBundle = false;
        }

        /** 配对包的顺序固定：add_entity、set_entity_data，手里有东西时才有 set_equipment。 */
        private void checkPairingBundle(List<Packet<? super ClientGamePacketListener>> inner) {
            var added = (ClientboundAddEntityPacket) inner.getFirst();
            boolean hasData = inner.size() > 1 && inner.get(1) instanceof ClientboundSetEntityDataPacket;
            check("bundle: metadata follows add_entity", hasData);
            if (!hasData) {
                return;
            }
            var data = (ClientboundSetEntityDataPacket) inner.get(1);
            check("bundle: metadata belongs to the new entity", data.id() == added.getId());
            Map<Integer, Object> values = new HashMap<>();
            data.packedItems().forEach(value -> values.put(value.id(), value.value()));
            check("bundle: metadata has the health", Float.valueOf(20.0f).equals(values.get(METADATA_HEALTH)));
            check("bundle: metadata has the skin parts",
                    Byte.valueOf((byte) ALL_SKIN_PARTS).equals(values.get(METADATA_SKIN_PARTS)));
            String name = tab.get(added.getUUID());
            if (PLAYER.equals(name)) {
                check("bundle: metadata has the left hand of " + PLAYER, values.get(METADATA_MAIN_HAND) == HumanoidArm.LEFT);
            }
            if (SECOND_PLAYER.equals(name)) {
                check("bundle: a right hand is the default and is left out", !values.containsKey(METADATA_MAIN_HAND));
            }
        }

        private void onAddEntity(ClientboundAddEntityPacket packet) {
            check("entity: add_entity is in a bundle", inBundle);
            check("entity: the client's own entity is never added", packet.getId() != entityId);
            check("entity: players use the player entity type", packet.getType() == EntityTypes.PLAYER);
            check("entity: add_entity comes after the tab entry", tab.containsKey(packet.getUUID()));
            check("entity: no initial movement", packet.getMovement().equals(Vec3.ZERO));
            check("entity: data is zero", packet.getData() == 0);
            remotes.put(packet.getId(), new Remote(packet));
            events.add("add " + packet.getId());
        }

        private void onRemoveEntities(ClientboundRemoveEntitiesPacket packet) {
            packet.entityIds().forEach((int id) -> {
                check("entity: removed entities were known", remotes.containsKey(id));
                remotes.remove(id);
                events.add("remove " + id);
            });
        }

        private Remote knownRemote(int id, String kind) {
            Remote remote = remotes.get(id);
            check("entity: " + kind + " only arrives for known entities", remote != null);
            return remote;
        }

        private void onMoveEntity(ClientboundMoveEntityPacket packet) {
            int id = privateInt(packet, ClientboundMoveEntityPacket.class, "entityId");
            Remote remote = knownRemote(id, "move_entity");
            if (remote == null) {
                return;
            }
            remote.moves += 1;
            if (packet.hasPosition()) {
                PositionPath path = packet.getPositionDelta().decode(remote.codec);
                remote.codec.setBase(path.endPosition());
                remote.position = path.endPosition();
            }
            if (packet.hasRotation()) {
                remote.yRot = packet.getYRot();
                remote.xRot = packet.getXRot();
            }
            remote.onGround = packet.isOnGround();
        }

        private void onPositionSync(ClientboundEntityPositionSyncPacket packet) {
            Remote remote = knownRemote(packet.id(), "entity_position_sync");
            if (remote == null) {
                return;
            }
            remote.syncs += 1;
            remote.position = packet.position().endPosition();
            remote.codec.setBase(remote.position);
            remote.yRot = packet.yRot();
            remote.xRot = packet.xRot();
            remote.onGround = packet.onGround();
        }

        private void onRotateHead(ClientboundRotateHeadPacket packet) {
            Remote remote = knownRemote(privateInt(packet, ClientboundRotateHeadPacket.class, "entityId"), "rotate_head");
            if (remote != null) {
                remote.yHeadRot = packet.getYHeadRot();
            }
        }

        private void onSwing(ClientboundSwingAnimationPacket packet) {
            Remote remote = knownRemote(packet.entityId(), "swing_animation");
            if (remote != null) {
                remote.swings.add(packet);
            }
        }

        private void onEquipment(ClientboundSetEquipmentPacket packet) {
            Remote remote = knownRemote(packet.getEntity(), "set_equipment");
            if (remote != null) {
                packet.getSlots().forEach(slot -> remote.equipment.put(slot.getFirst(), slot.getSecond()));
            }
        }

        private void onBlockUpdate(ClientboundBlockUpdatePacket packet) {
            BlockPos pos = packet.getPos().immutable();
            blocks.put(pos, packet.getBlockState());
            events.add("block " + pos.toShortString() + " " + Block.getId(packet.getBlockState()));
        }

        private void onAck(ClientboundBlockChangedAckPacket packet) {
            check("blocks: acks go up", packet.sequence() > lastAck);
            lastAck = packet.sequence();
            acks += 1;
            events.add("ack " + packet.sequence());
        }

        private void onLevelEvent(ClientboundLevelEventPacket packet) {
            levelEvents.add(packet);
            events.add("event " + packet.getPos().toShortString());
        }

        private void onSetSlot(ClientboundContainerSetSlotPacket packet) {
            check("inventory: the player inventory is container zero", packet.getContainerId() == 0);
            slots.put(packet.getSlot(), packet.getItem());
        }

        private void onSystemChat(ClientboundSystemChatPacket packet) {
            systemMessages.add(packet);
            events.add("system " + packet.content().getString());
        }

        private void onDisguisedChat(ClientboundDisguisedChatPacket packet) {
            chats.add(packet.chatType().decorate(packet.message()).getString());
            narrations.add(packet.chatType().decorateNarration(packet.message()).getString());
            check("chat: player chat has no target", packet.chatType().targetName().isEmpty());
        }

        private void onAwardStats(ClientboundAwardStatsPacket packet) {
            stats += 1;
            check("chat: statistics are empty", packet.stats().isEmpty());
        }

        private LevelChunkSection[] decodeSections(ClientboundLevelChunkWithLightPacket packet) {
            FriendlyByteBuf buffer = packet.chunkData().getReadBuffer();
            LevelChunkSection[] sections = new LevelChunkSection[SECTIONS];
            for (int index = 0; index < SECTIONS; index += 1) {
                sections[index] = new LevelChunkSection(containers);
                sections[index].read(buffer);
            }
            check("chunk: all sections were consumed", buffer.readableBytes() == 0);
            checkChunkData(packet, sections);
            return sections;
        }

        /** 区块段头的计数和每种高度图都由官方代码从解出来的方块重算，再和收到的值比较。 */
        private void checkChunkData(ClientboundLevelChunkWithLightPacket packet, LevelChunkSection[] sections) {
            for (int index = 0; index < sections.length; index += 1) {
                int receivedBlocks = privateInt(sections[index], LevelChunkSection.class, "nonEmptyBlockCount");
                int receivedFluids = privateInt(sections[index], LevelChunkSection.class, "fluidCount");
                sections[index].recalcBlockCounts();
                check("chunk: section " + index + " block count",
                        receivedBlocks == privateInt(sections[index], LevelChunkSection.class, "nonEmptyBlockCount"));
                check("chunk: section " + index + " fluid count",
                        receivedFluids == privateInt(sections[index], LevelChunkSection.class, "fluidCount"));
            }

            ProtoChunk chunk = new ProtoChunk(
                    new ChunkPos(packet.x(), packet.z()),
                    UpgradeData.EMPTY,
                    sections,
                    new ProtoChunkTicks<>(),
                    new ProtoChunkTicks<>(),
                    LevelHeightAccessor.create(-64, SECTIONS * 16),
                    containers,
                    null);
            Heightmap.primeHeightmaps(chunk, ChunkStatus.FINAL_HEIGHTMAPS);

            Map<Heightmap.Types, long[]> received = packet.chunkData().getHeightmaps();
            for (Heightmap.Types type : Heightmap.Types.values()) {
                check("chunk: " + type + " is sent exactly when the client needs it",
                        type.sendToClient() == received.containsKey(type));
            }
            for (Map.Entry<Heightmap.Types, long[]> entry : received.entrySet()) {
                long[] expected = chunk.getOrCreateHeightmapUnprimed(entry.getKey()).getRawData();
                check("chunk: " + entry.getKey() + " heightmap",
                        Arrays.equals(entry.getValue(), expected));
            }
        }

        /** 记下这个区块里改过的位置和方块实体，后来的玩家要靠它们检查改动。 */
        private void recordChunk(ClientboundLevelChunkWithLightPacket packet) {
            LevelChunkSection ground = decodeSections(packet)[0];
            for (int z = 0; z < 16; z += 1) {
                for (int x = 0; x < 16; x += 1) {
                    for (int y = 0; y < 16; y += 1) {
                        BlockState state = ground.getBlockState(x, y, z);
                        boolean flat = state.is(Blocks.BEDROCK) || state.is(Blocks.DIRT)
                                || state.is(Blocks.GRASS_BLOCK) || state.isAir();
                        if (!flat || (state.isAir() && y < 4)) {
                            blocks.put(new BlockPos(packet.x() * 16 + x, y - 64, packet.z() * 16 + z), state);
                        }
                    }
                }
            }
            packet.chunkData().forEachBlockEntityTag(packet.x(), packet.z(), (pos, type, tag) -> {
                check("chunk: block entities have no NBT", tag == null);
                blockEntities.put(pos.immutable(), type);
            });
        }

        /** 用官方代码解码区块，再检查地层、高度图和光照。 */
        private void checkChunkContents(ClientboundLevelChunkWithLightPacket packet) {
            LevelChunkSection[] sections = decodeSections(packet);

            LevelChunkSection ground = sections[0];
            check("chunk: bedrock at the bottom", ground.getBlockState(3, 0, 5).is(Blocks.BEDROCK));
            check("chunk: dirt above the bedrock", ground.getBlockState(3, 1, 5).is(Blocks.DIRT)
                    && ground.getBlockState(3, 2, 5).is(Blocks.DIRT));
            check("chunk: grass on top", ground.getBlockState(3, 3, 5).is(Blocks.GRASS_BLOCK));
            check("chunk: air above the grass", ground.getBlockState(3, 4, 5).isAir());
            check("chunk: corners match the layers", ground.getBlockState(15, 3, 15).is(Blocks.GRASS_BLOCK));
            check("chunk: biome is plains", ground.getNoiseBiome(0, 0, 0).is(Biomes.PLAINS));
            boolean upperEmpty = true;
            for (int index = 1; index < SECTIONS; index += 1) {
                upperEmpty &= sections[index].hasOnlyAir();
            }
            check("chunk: upper sections are air", upperEmpty);

            Map<Heightmap.Types, long[]> heightmaps = packet.chunkData().getHeightmaps();
            check("chunk: heightmaps are the client ones", heightmaps.keySet().equals(Set.of(
                    Heightmap.Types.WORLD_SURFACE,
                    Heightmap.Types.MOTION_BLOCKING,
                    Heightmap.Types.MOTION_BLOCKING_NO_LEAVES)));
            long[] surface = heightmaps.get(Heightmap.Types.WORLD_SURFACE);
            if (surface != null) {
                SimpleBitStorage storage = new SimpleBitStorage(9, 256, surface);
                check("chunk: heightmap points above the grass", storage.get(0) == 4 && storage.get(255) == 4);
            }

            checkLightStructure("chunk", packet.lightData());
            // 后进来的玩家收到的第一个区块可能已经被别人改过了，光照也跟着变，只有原样的区块能对答案。
            if (isUntouchedFlat(sections)) {
                checkFlatLight(packet.lightData());
            }
        }

        /** 每一格都和超平坦的地层一样。 */
        private static boolean isUntouchedFlat(LevelChunkSection[] sections) {
            Block[] layers = {Blocks.BEDROCK, Blocks.DIRT, Blocks.DIRT, Blocks.GRASS_BLOCK};
            for (int y = 0; y < 16; y += 1) {
                for (int z = 0; z < 16; z += 1) {
                    for (int x = 0; x < 16; x += 1) {
                        BlockState state = sections[0].getBlockState(x, y, z);
                        if (y < layers.length ? !state.is(layers[y]) : !state.isAir()) {
                            return false;
                        }
                    }
                }
            }
            for (int index = 1; index < SECTIONS; index += 1) {
                if (!sections[index].hasOnlyAir()) {
                    return false;
                }
            }
            return true;
        }

        /**
         * 官方超平坦区块的光照：只存地面那段和它上下各一段。世界下面那段没写过，是空的；地面那段草方块以上是 15；
         * 再上面一段整段是 15，更高的段不存，客户端按 15 算。方块光三段都是空的。
         */
        private void checkFlatLight(ClientboundLightUpdatePacketData light) {
            flatLightChecked = true;
            check("chunk: sky light is stored around the ground only",
                    light.skyYMask().equals(bits(1, 2)) && light.emptySkyYMask().equals(bits(0)));
            check("chunk: block light is stored empty around the ground",
                    light.blockYMask().isEmpty() && light.emptyBlockYMask().equals(bits(0, 1, 2))
                            && light.blockUpdates().isEmpty());
            if (light.skyUpdates().size() != 2) {
                check("chunk: one sky light array per stored section", false);
                return;
            }
            DataLayer ground = new DataLayer(light.skyUpdates().get(0));
            DataLayer above = new DataLayer(light.skyUpdates().get(1));
            boolean groundLit = true;
            boolean aboveLit = true;
            for (int y = 0; y < 16; y += 1) {
                for (int z = 0; z < 16; z += 1) {
                    for (int x = 0; x < 16; x += 1) {
                        groundLit &= ground.get(x, y, z) == (y < 4 ? 0 : 15);
                        aboveLit &= above.get(x, y, z) == 15;
                    }
                }
            }
            check("chunk: sky light starts above the grass", groundLit);
            check("chunk: sky light is full above the ground section", aboveLit);
        }

        /** light_update 只带变了的段，形状和区块包里的光照一样。 */
        private void onLightUpdate(ClientboundLightUpdatePacket packet) {
            checkLightStructure("light", packet.lightData());
        }

        /** 两个掩码不重叠，有数据的段一段一个数组，都在 26 段里。 */
        private static void checkLightStructure(String scope, ClientboundLightUpdatePacketData light) {
            check(scope + ": light masks stay inside the light sections",
                    light.skyYMask().length() <= LIGHT_LAYERS && light.blockYMask().length() <= LIGHT_LAYERS
                            && light.emptySkyYMask().length() <= LIGHT_LAYERS
                            && light.emptyBlockYMask().length() <= LIGHT_LAYERS);
            check(scope + ": a light section is either stored or empty",
                    !light.skyYMask().intersects(light.emptySkyYMask())
                            && !light.blockYMask().intersects(light.emptyBlockYMask()));
            check(scope + ": one light array per stored section",
                    light.skyUpdates().size() == light.skyYMask().cardinality()
                            && light.blockUpdates().size() == light.blockYMask().cardinality());
        }

        private static BitSet bits(int... indexes) {
            BitSet set = new BitSet();
            for (int index : indexes) {
                set.set(index);
            }
            return set;
        }

        private void onBatchFinished(ClientboundChunkBatchFinishedPacket packet) {
            batchTotal += packet.batchSize();
            connection.send(new ServerboundChunkBatchReceivedPacket(20f));
            if (walks && !walked && spawnChunks.size() == expectedSpawnChunks.size()) {
                walked = true;
                double blockX = WALK_TARGET.x() * 16 + SPAWN_X;
                position = new Vec3(blockX, SPAWN_Y, SPAWN_Z);
                connection.send(new ServerboundMovePlayerPacket.Pos(blockX, SPAWN_Y, SPAWN_Z, true, false));
            }
        }

        private void onKeepAlive(ClientboundKeepAlivePacket packet) {
            connection.send(new ServerboundKeepAlivePacket(packet.getId()));
            keepAlives += 1;
        }

        private void onDisconnected(ClientboundDisconnectPacket packet) {
            disconnected = true;
            disconnectReason = packet.reason().getString();
        }

        /** 用动态代理实现监听器，只处理关心的方法，其余的包只做计数。 */
        @SuppressWarnings("unchecked")
        private <T> T listener(Class<T> type, Map<String, Consumer<Object>> handlers) {
            InvocationHandler invocation = (proxy, method, args) -> {
                if (method.getDeclaringClass() == Object.class) {
                    return method.getName().equals("toString")
                            ? "Probe listener"
                            : method.getName().equals("hashCode") ? System.identityHashCode(proxy) : proxy == args[0];
                }
                if (method.isDefault()) {
                    return InvocationHandler.invokeDefault(proxy, method, args);
                }
                switch (method.getName()) {
                    case "isAcceptingMessages":
                        return true;
                    case "onDisconnect":
                        return null;
                    default:
                        break;
                }
                if (type == ClientGamePacketListener.class) {
                    playPackets.merge(method.getName(), 1, Integer::sum);
                }
                Consumer<Object> handler = handlers.get(method.getName());
                if (handler != null) {
                    handler.accept(args[0]);
                }
                return null;
            };
            return (T) Proxy.newProxyInstance(type.getClassLoader(), new Class<?>[] {type}, invocation);
        }
    }

    /** 在内存里运行官方管线，并把它的字节通过真实的 TCP socket 收发。 */
    private static final class Bridge implements AutoCloseable {
        /** 官方管线上分帧之后、解压之前的位置，用来检查每个包的长度前缀。 */
        private static final String FRAME_PROBE = "frame_probe";

        final Connection connection = new Connection(PacketFlow.CLIENTBOUND);
        private final EmbeddedChannel channel;
        private final Socket socket;
        private final byte[] chunk = new byte[1 << 16];
        private int threshold;
        /** 解压前的帧里长度前缀不为零的个数，也就是真的压缩过的包。 */
        int compressedFrames;
        /** 解压前的帧里长度前缀为零的个数，小包就是原样发的。 */
        int rawFrames;
        /** 长度前缀不为零但小于阈值的帧。官方不会这样发，客户端也会拒绝。 */
        int belowThresholdFrames;

        Bridge(String host, int port) throws IOException {
            socket = new Socket(host, port);
            socket.setSoTimeout(READ_TIMEOUT_MILLIS);
            socket.setTcpNoDelay(true);
            channel = new EmbeddedChannel(new ChannelInitializer<Channel>() {
                @Override
                protected void initChannel(Channel created) {
                    ChannelPipeline pipeline = created.pipeline();
                    Connection.configureSerialization(pipeline, PacketFlow.CLIENTBOUND, false, null);
                    connection.configurePacketHandler(pipeline);
                }
            });
        }

        /** 官方管线关了这条连接。解码出错时走这条路。 */
        boolean channelClosed() {
            return !channel.isOpen();
        }

        /**
         * 和官方客户端收到登录压缩包之后做的一样，把管线切换到压缩帧。
         *
         * <p>切换完再把探针挂在解压前面：分帧之后、解压之前的帧体带着长度前缀，
         * 非零就说明服务端真的压了这一个包。
         */
        void enableCompression(int threshold) {
            this.threshold = threshold;
            connection.setupCompression(threshold, false);
            channel.pipeline().addBefore("decompress", FRAME_PROBE, new FrameProbe());
        }

        /** 看一眼还没解压的帧体，只读长度前缀，不动它的读指针。 */
        private final class FrameProbe extends ChannelInboundHandlerAdapter {
            @Override
            public void channelRead(ChannelHandlerContext context, Object message) {
                if (message instanceof ByteBuf frame && frame.isReadable()) {
                    int declared = VarInt.read(frame.duplicate());
                    if (declared == 0) {
                        rawFrames += 1;
                    } else {
                        compressedFrames += 1;
                        if (declared < threshold) {
                            belowThresholdFrames += 1;
                        }
                    }
                }
                context.fireChannelRead(message);
            }
        }

        /** 转发一轮：先发管线里排队的字节，再读一次 socket，读不到时最多等 READ_TIMEOUT_MILLIS。 */
        void pump() throws IOException {
            channel.runPendingTasks();
            flushOutbound();
            if (channelClosed()) {
                return;
            }
            try {
                int read = socket.getInputStream().read(chunk);
                if (read < 0) {
                    check("connection: server kept the socket open", false);
                    connection.disconnect(Component.literal("the server closed the socket"));
                    return;
                }
                channel.writeInbound(Unpooled.copiedBuffer(chunk, 0, read));
            } catch (SocketTimeoutException ignored) {
                // 暂时没有数据可读。
            }
        }

        void pumpUntil(CountDownLatch finished, long timeoutMillis) throws IOException {
            long deadline = System.currentTimeMillis() + timeoutMillis;
            while (finished.getCount() > 0 && System.currentTimeMillis() < deadline && !channelClosed()) {
                pump();
            }
            flushOutbound();
            check("connection: finished before the deadline", finished.getCount() == 0);
        }

        void flushOutbound() throws IOException {
            ByteBuf outbound;
            while ((outbound = channel.readOutbound()) != null) {
                byte[] bytes = new byte[outbound.readableBytes()];
                outbound.readBytes(bytes);
                outbound.release();
                socket.getOutputStream().write(bytes);
            }
        }

        @Override
        public void close() throws IOException {
            socket.close();
            channel.finishAndReleaseAll();
        }
    }
}
