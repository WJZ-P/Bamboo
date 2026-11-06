import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import io.netty.channel.Channel;
import io.netty.channel.ChannelInitializer;
import io.netty.channel.ChannelPipeline;
import io.netty.channel.embedded.EmbeddedChannel;
import java.io.IOException;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.net.Socket;
import java.net.SocketTimeoutException;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.UUID;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ForkJoinPool;
import java.util.function.Consumer;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.UUIDUtil;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.Connection;
import net.minecraft.network.DisconnectionDetails;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.RegistryFriendlyByteBuf;
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
import net.minecraft.network.protocol.game.ClientboundChunkBatchFinishedPacket;
import net.minecraft.network.protocol.game.ClientboundGameEventPacket;
import net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket;
import net.minecraft.network.protocol.game.ClientboundLoginPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerAbilitiesPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerInfoUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket;
import net.minecraft.network.protocol.game.ClientboundSetChunkCacheCenterPacket;
import net.minecraft.network.protocol.game.ClientboundSetTimePacket;
import net.minecraft.network.protocol.game.GameProtocols;
import net.minecraft.network.protocol.game.ServerboundAcceptTeleportationPacket;
import net.minecraft.network.protocol.game.ServerboundChunkBatchReceivedPacket;
import net.minecraft.network.protocol.login.ClientLoginPacketListener;
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
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.tags.TagNetworkSerialization;
import net.minecraft.util.SimpleBitStorage;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.levelgen.Heightmap;

/**
 * 用官方 26.3 的网络代码扮演客户端，完整走一遍进服流程，检查 Bamboo 发的每个包。
 *
 * <p>帧、数据包和 NBT 的解码全部走官方代码，注册表和标签会和官方算出的结果逐项比较，
 * 区块用官方的 LevelChunkSection 解码后再检查方块。
 *
 * <p>官方管线跑在 EmbeddedChannel 上，字节通过阻塞 socket 转发，这样不依赖 NIO。
 * 所以这里验证的是协议内容，不包括 NIO 传输层和客户端的渲染。
 */
public final class Probe {
    private static final long PING_VALUE = 0x0123456789ABCDEFL;
    private static final String PLAYER = "Kitten";
    private static final int VIEW_DISTANCE = 8;
    private static final int EXPECTED_CHUNKS = (VIEW_DISTANCE * 2 + 1) * (VIEW_DISTANCE * 2 + 1);
    private static final int SECTIONS = 24;
    private static final int LIGHT_LAYERS = SECTIONS + 2;
    private static final long JOIN_TIMEOUT_MILLIS = 60_000;

    private static final Map<String, Boolean> CHECKS = new LinkedHashMap<>();

    private static void check(String name, boolean passed) {
        CHECKS.merge(name, passed, Boolean::logicalAnd);
    }

    public static void main(String[] args) throws Exception {
        String host = args[0];
        int port = Integer.parseInt(args[1]);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            LayeredRegistryAccess<RegistryLayer> layers = loadRegistries(resources);
            try {
                probeStatus(host, port);
                new Join(host, port, layers).run();
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

    /** 一次完整的进服流程。 */
    private static final class Join {
        private final String host;
        private final int port;
        private final LayeredRegistryAccess<RegistryLayer> layers;
        private final RegistryAccess.Frozen registries;
        private final Bridge bridge;
        private final Connection connection;
        private final CountDownLatch finished = new CountDownLatch(1);

        private final Map<String, List<String>> receivedRegistries = new HashMap<>();
        private final Map<String, Integer> playPackets = new TreeMap<>();
        private Map<ResourceKey<? extends Registry<?>>, TagNetworkSerialization.NetworkPayload> receivedTags;
        private int chunks;
        private int batchTotal;
        private boolean firstChunkChecked;
        private boolean disconnected;
        private String disconnectReason = "";

        Join(String host, int port, LayeredRegistryAccess<RegistryLayer> layers) throws IOException {
            this.host = host;
            this.port = port;
            this.layers = layers;
            this.registries = layers.compositeAccess();
            this.bridge = new Bridge(host, port);
            this.connection = bridge.connection;
        }

        void run() throws IOException {
            connection.initiateServerboundPlayConnection(host, port, listener(ClientLoginPacketListener.class, Map.of(
                    "handleLoginFinished", packet -> onLoginFinished((ClientboundLoginFinishedPacket) packet),
                    "handleDisconnect", packet -> onLoginRefused((ClientboundLoginDisconnectPacket) packet))));
            connection.send(new ServerboundHelloPacket(PLAYER, UUIDUtil.createOfflinePlayerUUID(PLAYER)));
            bridge.pumpUntil(finished, JOIN_TIMEOUT_MILLIS);

            check("play: chunks arrived", chunks == EXPECTED_CHUNKS);
            check("play: batch sizes add up to the chunk count", batchTotal == chunks);
            check("play: stayed connected through a keep-alive", playPackets.containsKey("handleKeepAlive"));
            check("play: was not disconnected", !disconnected);
            if (disconnected) {
                System.out.println("PROBE disconnected: " + disconnectReason);
            }
            System.out.println("PROBE play packets: " + playPackets);
            bridge.close();
        }

        private void onLoginRefused(ClientboundLoginDisconnectPacket packet) {
            System.out.println("PROBE login refused: " + packet.reason().getString());
            check("login: accepted", false);
            finished.countDown();
        }

        private void onLoginFinished(ClientboundLoginFinishedPacket packet) {
            check("login: name is kept", packet.gameProfile().name().equals(PLAYER));
            check("login: uuid uses the offline algorithm",
                    packet.gameProfile().id().equals(UUIDUtil.createOfflinePlayerUUID(PLAYER)));
            check("login: session id is present", packet.sessionId() != null);

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
            connection.send(new ServerboundClientInformationPacket(ClientInformation.createDefault()));
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
            connection.setupInboundProtocol(inbound, listener(ClientGamePacketListener.class, playHandlers()));
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
            handlers.put("handleSetTime", value -> onSetTime((ClientboundSetTimePacket) value));
            handlers.put("handleGameEvent", value -> onGameEvent((ClientboundGameEventPacket) value));
            handlers.put("handleSetChunkCacheCenter",
                    value -> onChunkCenter((ClientboundSetChunkCacheCenterPacket) value));
            handlers.put("handleLevelChunkWithLight",
                    value -> onChunk((ClientboundLevelChunkWithLightPacket) value));
            handlers.put("handleChunkBatchFinished",
                    value -> onBatchFinished((ClientboundChunkBatchFinishedPacket) value));
            handlers.put("handleKeepAlive", value -> onKeepAlive((ClientboundKeepAlivePacket) value));
            handlers.put("handleDisconnect", value -> onDisconnected((ClientboundDisconnectPacket) value));
            return handlers;
        }

        private void onLogin(ClientboundLoginPacket packet) {
            var spawn = packet.commonPlayerSpawnInfo();
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
            var position = packet.change().position();
            check("play: spawn is above the grass", position.y() == -60.0);
            check("play: position is absolute", packet.relatives().isEmpty());
            connection.send(new ServerboundAcceptTeleportationPacket(
                    packet.id(), position.x(), position.y(), position.z(), 0f, 0f));
        }

        private void onPlayerInfo(ClientboundPlayerInfoUpdatePacket packet) {
            check("play: tab list has the player",
                    packet.entries().size() == 1 && packet.entries().getFirst().profile().name().equals(PLAYER));
            check("play: tab entry is listed", packet.entries().getFirst().listed());
            check("play: tab entry has the game mode", packet.entries().getFirst().gameMode() == GameType.CREATIVE);
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
            check("play: chunk center is the spawn chunk", packet.getX() == 0 && packet.getZ() == 0);
        }

        private void onChunk(ClientboundLevelChunkWithLightPacket packet) {
            chunks += 1;
            if (!firstChunkChecked) {
                firstChunkChecked = true;
                check("play: nearest chunk comes first", packet.x() == 0 && packet.z() == 0);
                checkChunkContents(packet);
            }
        }

        /** 用官方代码解码区块，再检查地层、高度图和光照。 */
        private void checkChunkContents(ClientboundLevelChunkWithLightPacket packet) {
            PalettedContainerFactory factory = PalettedContainerFactory.create(registries);
            FriendlyByteBuf buffer = packet.chunkData().getReadBuffer();
            LevelChunkSection[] sections = new LevelChunkSection[SECTIONS];
            for (int index = 0; index < SECTIONS; index += 1) {
                sections[index] = new LevelChunkSection(factory);
                sections[index].read(buffer);
            }
            check("chunk: all sections were consumed", buffer.readableBytes() == 0);

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

            var light = packet.lightData();
            check("chunk: layer below the world is dark", light.emptySkyYMask().get(0));
            check("chunk: every other layer has sky light",
                    light.skyYMask().cardinality() == LIGHT_LAYERS - 1 && !light.skyYMask().get(0));
            check("chunk: one light array per layer", light.skyUpdates().size() == LIGHT_LAYERS - 1);
            check("chunk: light arrays are full size",
                    light.skyUpdates().stream().allMatch(layer -> layer.length == 2048));
            check("chunk: no block light", light.blockYMask().isEmpty() && light.blockUpdates().isEmpty());
        }

        private void onBatchFinished(ClientboundChunkBatchFinishedPacket packet) {
            batchTotal += packet.batchSize();
            connection.send(new ServerboundChunkBatchReceivedPacket(20f));
        }

        private void onKeepAlive(ClientboundKeepAlivePacket packet) {
            connection.send(new ServerboundKeepAlivePacket(packet.getId()));
            finished.countDown();
        }

        private void onDisconnected(ClientboundDisconnectPacket packet) {
            disconnected = true;
            disconnectReason = packet.reason().getString();
            finished.countDown();
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
                        finished.countDown();
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
        final Connection connection = new Connection(PacketFlow.CLIENTBOUND);
        private final EmbeddedChannel channel;
        private final Socket socket;

        Bridge(String host, int port) throws IOException {
            socket = new Socket(host, port);
            socket.setSoTimeout(20);
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

        void pumpUntil(CountDownLatch finished, long timeoutMillis) throws IOException {
            long deadline = System.currentTimeMillis() + timeoutMillis;
            byte[] chunk = new byte[1 << 16];
            while (finished.getCount() > 0 && System.currentTimeMillis() < deadline) {
                channel.runPendingTasks();
                flushOutbound();
                try {
                    int read = socket.getInputStream().read(chunk);
                    if (read < 0) {
                        check("connection: server kept the socket open", false);
                        return;
                    }
                    channel.writeInbound(Unpooled.copiedBuffer(chunk, 0, read));
                } catch (SocketTimeoutException ignored) {
                    // 暂时没有数据可读。
                }
            }
            flushOutbound();
            check("connection: finished before the deadline", finished.getCount() == 0);
        }

        private void flushOutbound() throws IOException {
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
