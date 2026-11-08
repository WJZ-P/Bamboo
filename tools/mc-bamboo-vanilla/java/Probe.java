import com.mojang.authlib.GameProfile;
import com.mojang.authlib.properties.Property;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import it.unimi.dsi.fastutil.shorts.ShortIterator;
import it.unimi.dsi.fastutil.shorts.ShortOpenHashSet;
import io.netty.channel.Channel;
import it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap;
import io.netty.channel.ChannelHandlerContext;
import io.netty.channel.ChannelInboundHandlerAdapter;
import io.netty.channel.ChannelInitializer;
import io.netty.channel.ChannelPipeline;
import io.netty.channel.embedded.EmbeddedChannel;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.math.BigInteger;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.net.Socket;
import java.net.SocketTimeoutException;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.security.KeyFactory;
import java.security.KeyPairGenerator;
import java.security.PrivateKey;
import java.security.PublicKey;
import java.security.Signature;
import java.security.spec.PKCS8EncodedKeySpec;
import java.time.Duration;
import java.time.Instant;
import java.util.Base64;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.BitSet;
import java.util.Collection;
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
import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.ParseResults;
import com.mojang.brigadier.arguments.ArgumentType;
import com.mojang.brigadier.builder.ArgumentBuilder;
import com.mojang.brigadier.builder.LiteralArgumentBuilder;
import com.mojang.brigadier.builder.RequiredArgumentBuilder;
import com.mojang.brigadier.context.CommandContextBuilder;
import javax.crypto.Cipher;
import javax.crypto.SecretKey;
import net.minecraft.ChatFormatting;
import net.minecraft.SharedConstants;
import net.minecraft.commands.CommandBuildContext;
import net.minecraft.commands.arguments.ArgumentSignatures;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.SectionPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.UUIDUtil;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.network.Connection;
import net.minecraft.network.DisconnectionDetails;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.HashedStack;
import net.minecraft.network.PacketDecoder;
import net.minecraft.network.PacketSendListener;
import net.minecraft.network.VarInt;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.LastSeenMessages;
import net.minecraft.network.chat.MessageSignature;
import net.minecraft.network.chat.MessageSignatureCache;
import net.minecraft.network.chat.PlayerChatMessage;
import net.minecraft.network.chat.RemoteChatSession;
import net.minecraft.network.chat.SignedMessageBody;
import net.minecraft.network.chat.SignedMessageChain;
import net.minecraft.network.chat.SignedMessageLink;
import net.minecraft.network.chat.SignedMessageValidator;
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
import net.minecraft.network.protocol.game.ClientboundBlockEntityDataPacket;
import net.minecraft.network.protocol.game.ClientboundBlockEventPacket;
import net.minecraft.network.protocol.game.ClientboundBlockUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundBundlePacket;
import net.minecraft.network.protocol.game.ClientboundChunkBatchFinishedPacket;
import net.minecraft.network.protocol.game.ClientboundCommandSuggestionsPacket;
import net.minecraft.network.protocol.game.ClientboundCommandsPacket;
import net.minecraft.network.protocol.game.ClientboundContainerClosePacket;
import net.minecraft.network.protocol.game.ClientboundEntityEventPacket;
import net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket;
import net.minecraft.network.protocol.game.ClientboundContainerSetSlotPacket;
import net.minecraft.network.protocol.game.ClientboundDisguisedChatPacket;
import net.minecraft.network.protocol.game.ClientboundEntityEventPacket;
import net.minecraft.network.protocol.game.ClientboundEntityPositionSyncPacket;
import net.minecraft.network.protocol.game.ClientboundForgetLevelChunkPacket;
import net.minecraft.network.protocol.game.ClientboundGameEventPacket;
import net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket;
import net.minecraft.network.protocol.game.ClientboundLevelEventPacket;
import net.minecraft.network.protocol.game.ClientboundLightUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData;
import net.minecraft.network.protocol.game.ClientboundLoginPacket;
import net.minecraft.network.protocol.game.ClientboundOpenBookPacket;
import net.minecraft.network.protocol.game.ClientboundOpenScreenPacket;
import net.minecraft.network.protocol.game.ClientboundMoveEntityPacket;
import net.minecraft.network.protocol.game.ClientboundOpenSignEditorPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerAbilitiesPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerChatPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerInfoRemovePacket;
import net.minecraft.network.protocol.game.ClientboundPlayerInfoUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket;
import net.minecraft.network.protocol.game.ClientboundRemoveEntitiesPacket;
import net.minecraft.network.protocol.game.ClientboundRotateHeadPacket;
import net.minecraft.network.protocol.game.ClientboundSectionBlocksUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundSetChunkCacheCenterPacket;
import net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket;
import net.minecraft.network.protocol.game.ClientboundSetEquipmentPacket;
import net.minecraft.network.protocol.game.ClientboundSetHeldSlotPacket;
import net.minecraft.network.protocol.game.ClientboundSetTimePacket;
import net.minecraft.network.protocol.game.ClientboundSoundPacket;
import net.minecraft.network.protocol.game.ClientboundSwingAnimationPacket;
import net.minecraft.network.protocol.game.ClientboundSystemChatPacket;
import net.minecraft.network.protocol.game.ClientboundUpdateAttributesPacket;
import net.minecraft.network.protocol.game.GameProtocols;
import net.minecraft.network.protocol.game.ServerboundAcceptTeleportationPacket;
import net.minecraft.network.protocol.game.ServerboundChatCommandPacket;
import net.minecraft.network.protocol.game.ServerboundChatCommandSignedPacket;
import net.minecraft.network.protocol.game.ServerboundChatSessionUpdatePacket;
import net.minecraft.network.protocol.game.ServerboundCommandSuggestionPacket;
import net.minecraft.network.protocol.game.ServerboundChatPacket;
import net.minecraft.network.protocol.game.ServerboundChunkBatchReceivedPacket;
import net.minecraft.network.protocol.game.ServerboundClientCommandPacket;
import net.minecraft.network.protocol.game.ServerboundContainerClickPacket;
import net.minecraft.network.protocol.game.ServerboundContainerClosePacket;
import net.minecraft.network.protocol.game.ServerboundEditBookPacket;
import net.minecraft.network.protocol.game.ServerboundMovePlayerPacket;
import net.minecraft.network.protocol.game.ServerboundPickItemFromBlockPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerAbilitiesPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerActionPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerCommandPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerInputPacket;
import net.minecraft.network.protocol.game.ServerboundPunchPacket;
import net.minecraft.network.protocol.game.ServerboundSetCarriedItemPacket;
import net.minecraft.network.protocol.game.ServerboundSetCreativeModeSlotPacket;
import net.minecraft.network.protocol.game.ServerboundSignUpdatePacket;
import net.minecraft.network.protocol.game.ServerboundUseItemOnPacket;
import net.minecraft.network.protocol.game.ServerboundUseItemPacket;
import net.minecraft.network.protocol.game.VecDeltaCodec;
import net.minecraft.network.protocol.login.ClientLoginPacketListener;
import net.minecraft.network.protocol.login.ClientboundHelloPacket;
import net.minecraft.network.protocol.login.ClientboundLoginCompressionPacket;
import net.minecraft.network.protocol.login.ClientboundLoginDisconnectPacket;
import net.minecraft.network.protocol.login.ClientboundLoginFinishedPacket;
import net.minecraft.network.protocol.login.ServerboundHelloPacket;
import net.minecraft.network.protocol.login.ServerboundKeyPacket;
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
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.minecraft.tags.TagLoader;
import net.minecraft.tags.TagNetworkSerialization;
import net.minecraft.util.Crypt;
import net.minecraft.util.SignatureValidator;
import net.minecraft.util.Signer;
import net.minecraft.world.entity.player.ProfilePublicKey;
import net.minecraft.util.SimpleBitStorage;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.HumanoidArm;
import net.minecraft.world.entity.Pose;
import net.minecraft.world.entity.PositionPath;
import net.minecraft.world.entity.player.Abilities;
import net.minecraft.world.entity.player.ChatVisiblity;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.item.DyeColor;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.entity.Relative;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.StairBlock;
import net.minecraft.world.item.SwingAnimationType;
import net.minecraft.world.item.component.WritableBookContent;
import net.minecraft.world.item.component.WrittenBookContent;
import net.minecraft.world.item.component.DebugStickState;
import net.minecraft.world.item.component.ResolvableProfile;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.Enchantments;
import net.minecraft.world.item.enchantment.ItemEnchantments;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.DoorBlock;
import net.minecraft.world.level.block.RotatedPillarBlock;
import net.minecraft.world.level.block.SlabBlock;
import net.minecraft.world.level.block.entity.BannerPattern;
import net.minecraft.world.level.block.entity.BannerPatternLayers;
import net.minecraft.world.level.block.entity.BannerPatterns;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.entity.BlockEntityTypes;
import net.minecraft.world.level.block.entity.SignTextSlot;
import net.minecraft.world.level.block.piston.PistonBaseBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
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
 * <p>多给两个参数时，最后一名玩家连的是另一个开了正版验证的服务端，加密和 session server
 * 那一段也跟着检查；参数由 `mc-bamboo-vanilla probe` 传进来，少给就跳过那一段。
 *
 * <p>脚本的每一步都有时限。一步超时后这一段剩下的步骤跳过，下一段继续，结束时列出所有失败。
 */
public final class Probe {
    private static final long PING_VALUE = 0x0123456789ABCDEFL;
    private static final String PLAYER = "Kitten";
    private static final String SECOND_PLAYER = "Mochi";
    private static final String LATE_PLAYER = "Latecomer";
    /** 走正版验证进服的那名玩家，它连的是另一个服务端，所以用一个别处没有的名字。 */
    private static final String ONLINE_PLAYER = "Sakura";
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
    private static final String SIGN_TEXT = "竹子会写字";
    /** 改过名的剑，组件 `custom_name` 要发给别的玩家。 */
    private static final String SWORD_NAME = "小猫的剑";
    /** 改过名的箱子，打开时的菜单标题就是它。 */
    private static final String CHEST_NAME = "小猫的箱子";
    private static final String UNKNOWN_COMMAND = "Unknown or incomplete command. See below for error";
    /** `LivingEntity.entityEventForEquipmentBreak` 的主手。 */
    // Main hand in `LivingEntity.entityEventForEquipmentBreak`.
    private static final byte EQUIPMENT_BREAK_MAIN_HAND = 47;

    /** M2 会发的全部包。收到别的说明服务端走了规格之外的路径。 */
    private static final Set<String> M2_PACKETS = Set.of(
            "handleAddEntity",
            "handleAwardStats",
            "handleBlockChangedAck",
            "handleBlockDestruction",
            "handleBlockEntityData",
            "handleBlockEvent",
            "handleBlockUpdate",
            "handleBundlePacket",
            "handleChunkBatchFinished",
            "handleChunkBatchStart",
            "handleChunkBlocksUpdate",
            "handleCommandSuggestions",
            "handleCommands",
            "handleContainerClose",
            "handleContainerContent",
            "handleContainerSetSlot",
            "handleDisconnect",
            "handleDisguisedChat",
            "handleEntityEvent",
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
            "handleOpenBook",
            "handleOpenScreen",
            "handleOpenSignEditor",
            "handlePlayerAbilities",
            "handlePlayerChat",
            "handlePlayerInfoRemove",
            "handlePlayerInfoUpdate",
            "handleRemoveEntities",
            "handleRotateMob",
            "handleSetChunkCacheCenter",
            // 光标变化时官方发 `set_cursor_item`。关菜单把材料退回物品栏时发 `set_player_inventory`。
            // Vanilla sends `set_cursor_item` when the cursor changes, and `set_player_inventory` when closing a menu returns items.
            "handleSetCursorItem",
            "handleSetEntityData",
            "handleSetEntityMotion",
            "handleSetEquipment",
            "handleSetHeldSlot",
            "handleSetPlayerInventory",
            "handleSetTime",
            "handleSoundEvent",
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
        // 多出来的两个参数是开了正版验证的服务端和假 session server，没给就不查那一段。
        String onlineAddress = args.length > 2 ? args[2] : null;
        String sessionUrl = args.length > 3 ? args[3] : null;
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
                if (onlineAddress != null && sessionUrl != null) {
                    probeOnline(onlineAddress, sessionUrl, layers);
                }
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

    /**
     * 开了正版验证的那一段：这名玩家连另一个服务端，走完官方客户端的加密登录流程。
     *
     * <p>假 session server 由 {@code mc-bamboo-vanilla probe} 启动。客户端先把自己算出的
     * serverId 哈希报给它，服务端随后拿同一个哈希来问才拿得到档案，所以两边的算法必须一致。
     */
    private static void probeOnline(
            String address, String sessionUrl, LayeredRegistryAccess<RegistryLayer> layers) throws IOException {
        int colon = address.lastIndexOf(':');
        String host = address.substring(0, colon);
        int port = Integer.parseInt(address.substring(colon + 1));
        Join sakura = new Join(host, port, layers, ONLINE_PLAYER, HumanoidArm.RIGHT, false, sessionUrl);
        try {
            sakura.start();
            long deadline = System.currentTimeMillis() + JOIN_TIMEOUT_MILLIS;
            while (!sakura.onlineJoinDone() && !sakura.closed && System.currentTimeMillis() < deadline) {
                sakura.pump();
            }
            check("online: " + ONLINE_PLAYER + " joined through the encrypted login", sakura.onlineJoinDone());
            sakura.checkDigestTheServerAsked();
            sakura.finishChecks();
        } finally {
            sakura.close();
        }
        probeSecureChat(host, port, layers, sessionUrl);
    }

    /**
     * 两名正版玩家互发签名消息。接收方用官方 {@code SignedMessageValidator} 验过才算通过。
     * 改过一个字节的签名必须被拒绝，而且下一句也会因为链断了被拒绝。
     *
     * <p>Two online players exchange signed messages. The receiver accepts one only when the official
     * {@code SignedMessageValidator} does. A signature with one flipped byte is rejected, and the
     * next message is rejected because the chain is broken.
     */
    private static void probeSecureChat(
            String host, int port, LayeredRegistryAccess<RegistryLayer> layers, String sessionUrl) throws IOException {
        Join momo = new Join(host, port, layers, "Momo", HumanoidArm.RIGHT, false, sessionUrl);
        Join nori = new Join(host, port, layers, "Nori", HumanoidArm.LEFT, false, sessionUrl);
        try {
            momo.start();
            waitOnline(momo);
            nori.start();
            waitOnline(nori);
            momo.installChatSession(sessionUrl);
            nori.installChatSession(sessionUrl);
            waitUntil(momo, "secure: Momo sees Nori's chat session", () -> momo.hasPeerSession(nori.uuid));
            waitUntil(nori, "secure: Nori sees Momo's chat session", () -> nori.hasPeerSession(momo.uuid));

            momo.sendSignedChat("signed hello");
            waitUntil(nori, "secure: Nori's client verified Momo's chat", () -> nori.verifiedSigned("signed hello"));
            waitUntil(momo, "secure: Momo's client verified its own chat", () -> momo.verifiedSigned("signed hello"));

            int noriBefore = nori.verifiedCount();
            momo.sendTamperedChat("tampered");
            waitUntil(momo, "secure: a tampered signature is rejected",
                    () -> momo.sawTranslation("chat.disabled.invalid_signature"));
            pumpBoth(momo, nori, 400);
            check("secure: the tampered message was not delivered", nori.verifiedCount() == noriBefore);

            momo.sendSignedChat("after the break");
            waitUntil(momo, "secure: the next message is rejected because the chain broke",
                    () -> momo.sawTranslation("chat.disabled.chain_broken"));
            pumpBoth(momo, nori, 400);
            check("secure: a broken chain is not delivered", !nori.verifiedSigned("after the break"));

            nori.sendSignedEmote("waves");
            waitUntil(momo, "secure: Momo's client verified the signed /me", () -> momo.verifiedSigned("waves"));
        } finally {
            momo.close();
            nori.close();
        }
    }

    private static void waitOnline(Join join) throws IOException {
        long deadline = System.currentTimeMillis() + JOIN_TIMEOUT_MILLIS;
        while (!join.onlineJoinDone() && !join.closed && System.currentTimeMillis() < deadline) {
            join.pump();
        }
        check("secure: " + join.player + " joined", join.onlineJoinDone());
    }

    private static void waitUntil(Join join, String what, BooleanSupplier ready) throws IOException {
        long deadline = System.currentTimeMillis() + 10_000;
        while (!ready.getAsBoolean() && !join.closed && System.currentTimeMillis() < deadline) {
            join.pump();
        }
        check(what, ready.getAsBoolean());
    }

    private static void pumpBoth(Join first, Join second, long millis) throws IOException {
        long deadline = System.currentTimeMillis() + millis;
        while (System.currentTimeMillis() < deadline) {
            first.pump();
            second.pump();
        }
    }

    /** 问一次假 session server，回复体当成 UTF-8 文本。 */
    private static String httpGet(String url) throws IOException {
        try (InputStream body = URI.create(url).toURL().openStream()) {
            return new String(body.readAllBytes(), StandardCharsets.UTF_8).trim();
        }
    }

    private static boolean near(Vec3 first, Vec3 second) {
        return Math.abs(first.x - second.x) <= POSITION_TOLERANCE
                && Math.abs(first.y - second.y) <= POSITION_TOLERANCE
                && Math.abs(first.z - second.z) <= POSITION_TOLERANCE;
    }

    /** 告示牌一面第 `line` 行，`front_text`/`back_text` 里 `messages` 的字符串。 */
    private static String signLine(CompoundTag tag, String side, int line) {
        if (tag == null) {
            return null;
        }
        return tag.getCompound(side)
                .flatMap(text -> text.getList("messages"))
                .filter(messages -> messages.size() > line)
                .flatMap(messages -> messages.getString(line))
                .orElse(null);
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

    /** 和客户端 `ClientPacketListener` 一样，把命令包还原成 Brigadier 节点。
     *
     * <p>Rebuild the command packet into Brigadier nodes the same way `ClientPacketListener` does.
     */
    private static ClientboundCommandsPacket.NodeBuilder<Object> commandNodes() {
        return new ClientboundCommandsPacket.NodeBuilder<>() {
            @Override
            public ArgumentBuilder<Object, ?> createLiteral(String id) {
                return LiteralArgumentBuilder.literal(id);
            }

            @Override
            public ArgumentBuilder<Object, ?> createArgument(
                    String id, ArgumentType<?> argumentType, Identifier suggestionId) {
                return RequiredArgumentBuilder.argument(id, argumentType);
            }

            @Override
            public ArgumentBuilder<Object, ?> configure(
                    ArgumentBuilder<Object, ?> builder, boolean executable, boolean restricted) {
                if (executable) {
                    builder.executes(context -> 0);
                }
                return builder;
            }
        };
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
                section("sounds" + names, () -> sounds(actor, observer));
                section("pistons" + names, () -> pistons(actor, observer));
                section("signs" + names, () -> signs(actor, observer));
                section("inventory" + names, () -> inventory(actor, observer));
                section("components" + names, () -> components(actor, observer));
                section("chat" + names, () -> chat(actor, observer));
            }

            section("tool durability", () -> toolDurability(kitten, mochi));
            section("world entities", () -> worldEntities(kitten, mochi));
            section("books", () -> books(kitten));
            section("containers", () -> containers(kitten, mochi));
            section("crafting", () -> crafting(kitten));

            Join late = connect(LATE_PLAYER, HumanoidArm.RIGHT, false);
            section("late joiner", () -> {
                await("join: " + LATE_PLAYER + " got the spawn chunks", JOIN_TIMEOUT_MILLIS, late::spawnChunksDone);
                checkEditedChunks(kitten, mochi, late);
            });
            section("commands", () -> commands(kitten, mochi, late));
            section("leaving", () -> leaving(kitten, mochi, late));

            section("long running", () -> {
                await("play: keep-alives and the latency broadcast arrived", JOIN_TIMEOUT_MILLIS, kitten::longRunningDone);
                await("play: another client received the latency update", JOIN_TIMEOUT_MILLIS,
                        () -> late.latencyUpdated);
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
            int ownSections = actor.sectionUpdates.size();
            int otherSections = observer.sectionUpdates.size();
            int breaking = actor.nextSequence();
            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.START_DESTROY_BLOCK, log, Direction.UP, breaking));
            await("placement: the torch goes with the log", () -> air.equals(observer.blocks.get(torch)));
            pumpFor(SETTLE_MILLIS);
            check("placement: the observer gets both break events", observer.levelEvents.size() == otherEvents + 2);
            check("placement: the breaker only gets the event of the torch",
                    actor.levelEvents.size() == ownEvents + 1
                            && actor.levelEvents.getLast().getData() == Block.getId(standing));

            // 原木和火把在同一段里、同一 tick 没了，官方把两格放进一个 section_blocks_update。
            List<BlockPos> together = officialOrder(log, torch);
            check("placement: the log and the torch arrive in one section update in the official order",
                    observer.sectionUpdates.size() == otherSections + 1
                            && observer.sectionUpdates.getLast().equals(together));
            check("placement: the breaker gets the same section update",
                    actor.sectionUpdates.size() == ownSections + 1
                            && actor.sectionUpdates.getLast().equals(together));
            check("placement: the section update decodes to air",
                    air.equals(observer.blocks.get(log)) && air.equals(observer.blocks.get(torch)));
            check("placement: the section update comes before the ack",
                    actor.eventIndex("block " + torch.toShortString() + " " + Block.getId(air))
                            < actor.eventIndex("ack " + breaking));

            actor.cleared.add(log);
            actor.cleared.add(torch);
            actor.edits.add(new Edit(slab, upperSlab));
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 1, new ItemStack(Items.CHEST)));
            actor.send(new ServerboundSetCarriedItemPacket(0));
        }

        /**
         * 放门和开门都会发声音包，坐标是方块中心，动手的玩家自己听不到。
         * 放下的音量和音高来自 `SoundType`，开门的是随机音高。
         */
        private void sounds(Join actor, Join observer) throws IOException {
            // 两个玩家隔着 16 格。先朝观察者走一格，门的声音才落在 16 格的广播范围内。
            double step = observer.position.x < actor.position.x ? -WALK_STEP : WALK_STEP;
            Vec3 start = actor.position;
            for (int walked = 1; walked <= 5; walked += 1) {
                actor.moveTo(start.add(step * walked, 0.0, 0.0), true);
                pumpFor(TICK_MILLIS);
            }
            // 自己脚下这一格一定是空气，点它就把门放在这一格，声音的坐标就是方块中心。
            BlockPos door = BlockPos.containing(actor.position);
            Vec3 center = Vec3.atCenterOf(door);
            actor.send(new ServerboundSetCarriedItemPacket(1));
            int otherSounds = observer.sounds.size();
            int ownSounds = actor.sounds.size();

            useOn(actor, Items.OAK_DOOR, door, Direction.UP, Vec3.upFromBottomCenterOf(door, 1.0));
            // 放置失败时服务端也会把这一格当空气发回来，所以要看到门本身。
            await("sound: " + observer.player + " sees the door", () -> {
                BlockState state = observer.blocks.get(door);
                return state != null && state.is(Blocks.OAK_DOOR);
            });
            await("sound: " + observer.player + " hears the door being placed",
                    () -> observer.sounds.size() == otherSounds + 1);
            ClientboundSoundPacket placed = observer.sounds.getLast();
            check("sound: the place sound is the wood one", placed.getSound().value() == SoundEvents.WOOD_PLACE);
            check("sound: the place sound comes from the block", placed.getSource() == SoundSource.BLOCKS);
            check("sound: the place sound is at the block center",
                    near(center, new Vec3(placed.getX(), placed.getY(), placed.getZ())));
            check("sound: the place sound has the block volume", placed.getVolume() == 1.0f);
            check("sound: the place sound has the block pitch", placed.getPitch() == 0.8f);
            pumpFor(SETTLE_MILLIS);
            check("sound: the placer hears nothing", actor.sounds.size() == ownSounds);

            // `DoorBlock.playSound`：`getRandom().nextFloat() * 0.1F + 0.9F`。
            int beforeOpen = observer.sounds.size();
            int ownBeforeOpen = actor.sounds.size();
            int sequence = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.atCenterOf(door), Direction.UP, door, false),
                    sequence));
            await("sound: the door opens", () -> observer.blocks.containsKey(door)
                    && observer.blocks.get(door).getValue(DoorBlock.OPEN));
            await("sound: " + observer.player + " hears the door open",
                    () -> observer.sounds.size() == beforeOpen + 1);
            ClientboundSoundPacket opened = observer.sounds.getLast();
            check("sound: the open sound is the wooden door one",
                    opened.getSound().value() == SoundEvents.WOODEN_DOOR_OPEN);
            check("sound: the open sound comes from the block", opened.getSource() == SoundSource.BLOCKS);
            check("sound: the open sound is at the door",
                    near(center, new Vec3(opened.getX(), opened.getY(), opened.getZ())));
            check("sound: the open sound has volume one", opened.getVolume() == 1.0f);
            check("sound: the open sound has a random pitch",
                    opened.getPitch() >= 0.9f && opened.getPitch() < 1.0f);
            pumpFor(SETTLE_MILLIS);
            check("sound: the door opener hears nothing", actor.sounds.size() == ownBeforeOpen);

            // 门会挡住另一个玩家这一段，从下半截破坏，两格一起走。
            int breaking = actor.nextSequence();
            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.START_DESTROY_BLOCK, door, Direction.UP, breaking));
            await("sound: the door is gone", () -> actor.lastAck >= breaking);
            actor.cleared.add(door);
            actor.cleared.add(door.above());

            // 走回原来的位置，后面的段落用和以前一样的坐标。
            for (int walked = 4; walked >= 0; walked -= 1) {
                actor.moveTo(start.add(step * walked, 0.0, 0.0), true);
                pumpFor(TICK_MILLIS);
            }
        }

        private static boolean extended(BlockState state) {
            return state != null && state.is(Blocks.STICKY_PISTON) && state.getValue(PistonBaseBlock.EXTENDED);
        }

        private static boolean retracted(BlockState state) {
            return state != null && state.is(Blocks.STICKY_PISTON) && !state.getValue(PistonBaseBlock.EXTENDED);
        }

        /** 粘性活塞推出方块再拉回来：客户端要收到方块事件，被推的方块要出现在新位置。 */
        private void pistons(Join actor, Join observer) throws IOException {
            // 动手的人先往北走了约 4 格，再往南放活塞就出了观察者的 16 格。
            // 朝观察者走近不到一格，活塞放在北面 3 格，测完走回去。
            double step = observer.position.x < actor.position.x ? -WALK_STEP : WALK_STEP;
            Vec3 start = actor.position;
            for (int walked = 1; walked <= 3; walked += 1) {
                actor.moveTo(start.add(step * walked, 0.0, 0.0), true);
                pumpFor(TICK_MILLIS);
            }
            BlockPos ground = BlockPos.containing(actor.position).offset(0, -1, -3);
            BlockPos piston = ground.above();
            BlockPos stone = piston.north();
            BlockPos pushed = stone.north();
            BlockPos power = ground.west().above();
            BlockState air = Blocks.AIR.defaultBlockState();
            BlockState stoneState = Blocks.STONE.defaultBlockState();

            // 朝北放：yaw 0 时官方取玩家的朝向，活塞朝玩家的反方向。
            actor.send(new ServerboundMovePlayerPacket.Rot(0.0f, 0.0f, true, false));
            actor.send(new ServerboundSetCarriedItemPacket(1));
            useOn(actor, Items.STICKY_PISTON, ground, Direction.UP, Vec3.upFromBottomCenterOf(ground, 1.0));
            await("pistons: " + observer.player + " sees the sticky piston",
                    () -> retracted(observer.blocks.get(piston)));
            useOn(actor, Items.STONE, ground.north(), Direction.UP, Vec3.upFromBottomCenterOf(ground.north(), 1.0));
            await("pistons: " + observer.player + " sees the block to push",
                    () -> stoneState.equals(observer.blocks.get(stone)));

            int extending = observer.blockEvents.size();
            int heard = observer.sounds.size();
            useOn(actor, Items.REDSTONE_BLOCK, ground.west(), Direction.UP, Vec3.upFromBottomCenterOf(ground.west(), 1.0));
            await("pistons: the piston extends", () -> extended(observer.blocks.get(piston)));
            await("pistons: the head arrives",
                    () -> observer.blocks.get(stone) != null && observer.blocks.get(stone).is(Blocks.PISTON_HEAD));
            await("pistons: the block lands one step further", () -> stoneState.equals(observer.blocks.get(pushed)));
            pumpFor(SETTLE_MILLIS);
            check("pistons: extending sends one block event", observer.blockEvents.size() == extending + 1);
            ClientboundBlockEventPacket extend = observer.blockEvents.getLast();
            check("pistons: the extension event names the piston",
                    extend.getPos().equals(piston) && extend.getBlock() == Blocks.STICKY_PISTON && extend.getB0() == 0);
            boolean heardExtend = false;
            for (ClientboundSoundPacket sound : observer.sounds.subList(heard, observer.sounds.size())) {
                if (sound.getSound().value() != SoundEvents.PISTON_EXTEND) {
                    continue;
                }
                heardExtend = true;
                check("pistons: the extend sound comes from the block", sound.getSource() == SoundSource.BLOCKS);
                check("pistons: the extend sound is at the piston",
                        near(Vec3.atCenterOf(piston), new Vec3(sound.getX(), sound.getY(), sound.getZ())));
                check("pistons: the extend sound has volume 0.5", sound.getVolume() == 0.5f);
                check("pistons: the extend sound has the piston pitch",
                        sound.getPitch() >= 0.6f && sound.getPitch() < 0.85f);
            }
            check("pistons: the other client hears the piston extend", heardExtend);

            int retracting = observer.blockEvents.size();
            int breaking = actor.nextSequence();
            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.START_DESTROY_BLOCK, power, Direction.UP, breaking));
            await("pistons: the power block breaks", () -> actor.lastAck >= breaking);
            await("pistons: the piston retracts", () -> retracted(observer.blocks.get(piston)));
            await("pistons: the block comes back", () -> stoneState.equals(observer.blocks.get(stone)));
            await("pistons: the space behind is open again", () -> air.equals(observer.blocks.get(pushed)));
            pumpFor(SETTLE_MILLIS);
            check("pistons: retracting sends one block event", observer.blockEvents.size() == retracting + 1);
            ClientboundBlockEventPacket retract = observer.blockEvents.getLast();
            check("pistons: the retraction event names the piston",
                    retract.getPos().equals(piston) && retract.getBlock() == Blocks.STICKY_PISTON && retract.getB0() == 1);
            for (int walked = 2; walked >= 0; walked -= 1) {
                actor.moveTo(start.add(step * walked, 0.0, 0.0), true);
                pumpFor(TICK_MILLIS);
            }
        }

        /** 放下的告示牌只给放置者打开编辑界面，写进去的字另一个客户端从方块实体数据里读得到。 */
        private void signs(Join actor, Join observer) throws IOException {
            BlockPos ground = BlockPos.containing(actor.position).offset(2, -1, 2);
            BlockPos sign = ground.above();
            BlockState standing = Blocks.OAK_SIGN.defaultBlockState();
            int observerEditors = observer.signEditors.size();

            actor.send(new ServerboundSetCarriedItemPacket(1));
            useOn(actor, Items.OAK_SIGN, ground, Direction.UP, Vec3.upFromBottomCenterOf(ground, 1.0));
            await("signs: " + actor.player + " gets the editor", () -> actor.signEditors.containsKey(sign));
            check("signs: the editor opens on the front", actor.signEditors.get(sign) == SignTextSlot.FRONT);
            await("signs: " + observer.player + " sees the sign", () -> standing.equals(observer.blocks.get(sign)));
            pumpFor(SETTLE_MILLIS);
            check("signs: only the placer gets the editor", observer.signEditors.size() == observerEditors);

            actor.send(new ServerboundSignUpdatePacket(
                    sign, List.of(SIGN_TEXT, "", "", ""), SignTextSlot.FRONT));
            await("signs: " + actor.player + " reads the text back",
                    () -> SIGN_TEXT.equals(signLine(actor.blockEntityTags.get(sign), "front_text", 0)));
            await("signs: " + observer.player + " reads the text",
                    () -> SIGN_TEXT.equals(signLine(observer.blockEntityTags.get(sign), "front_text", 0)));
            actor.edits.add(new Edit(sign, standing));
        }

        /**
         * 官方 `ChunkHolder.blockChanged` 按 `changed` 的先后把段内坐标记进 `ShortOpenHashSet`，
         * `section_blocks_update` 里的格子就是这个集合读出来的顺序。
         */
        private static List<BlockPos> officialOrder(BlockPos... changed) {
            ShortOpenHashSet set = new ShortOpenHashSet();
            for (BlockPos pos : changed) {
                set.add(SectionPos.sectionRelativePos(pos));
            }
            SectionPos section = SectionPos.of(changed[0]);
            List<BlockPos> order = new ArrayList<>();
            for (ShortIterator iterator = set.iterator(); iterator.hasNext(); ) {
                order.add(section.relativeToBlockPos(iterator.nextShort()));
            }
            return order;
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

        /**
         * 物品组件：校验过的组件要发给别的玩家，放下的方块实体也要带上物品里的数据。
         *
         * 组件和 NBT 都由官方的编解码器解，所以这些检查过了就说明官方客户端解得开。
         */
        private void components(Join actor, Join observer) throws IOException {
            Remote remote = observer.remote(actor);
            Holder<Enchantment> sharpness = actor.registries
                    .lookupOrThrow(Registries.ENCHANTMENT)
                    .getOrThrow(Enchantments.SHARPNESS);
            ItemEnchantments.Mutable stored = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
            stored.set(sharpness, 3);
            ItemStack book = new ItemStack(Items.ENCHANTED_BOOK);
            book.set(DataComponents.STORED_ENCHANTMENTS, stored.toImmutable());
            ItemStack sword = new ItemStack(Items.DIAMOND_SWORD);
            sword.set(DataComponents.CUSTOM_NAME, Component.literal(SWORD_NAME));

            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 2, book));
            actor.send(new ServerboundSetCreativeModeSlotPacket(OFF_HAND_SLOT, sword));
            actor.send(new ServerboundSetCarriedItemPacket(2));
            await("components: the observer sees the enchanted book", () -> {
                ItemStack held = remote.equipment.get(EquipmentSlot.MAINHAND);
                return held != null
                        && held.is(Items.ENCHANTED_BOOK)
                        && held.getOrDefault(DataComponents.STORED_ENCHANTMENTS, ItemEnchantments.EMPTY)
                                .getLevel(sharpness) == 3;
            });
            await("components: the observer sees the renamed sword", () -> {
                ItemStack held = remote.equipment.get(EquipmentSlot.OFFHAND);
                Component name = held == null ? null : held.get(DataComponents.CUSTOM_NAME);
                return name != null && SWORD_NAME.equals(name.getString());
            });

            // `debug_stick_state` 在线上是 NBT，我们只能校验最外层。它属于发来它的玩家：
            // 换手时服务端把主手和副手两格都发回给这个玩家，副手那一份要原样带着组件。
            ItemStack debugStick = new ItemStack(Items.DEBUG_STICK);
            debugStick.set(DataComponents.DEBUG_STICK_STATE, new DebugStickState(Map.of(
                    BuiltInRegistries.BLOCK.wrapAsHolder(Blocks.OAK_LOG), BlockStateProperties.AXIS)));
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 2, debugStick));
            actor.send(new ServerboundSetCarriedItemPacket(2));
            actor.send(new ServerboundPlayerActionPacket(
                    ServerboundPlayerActionPacket.Action.SWAP_ITEM_WITH_OFFHAND, BlockPos.ZERO, Direction.DOWN));
            await("components: the owner gets the components we only shape check after moving them", () -> {
                ItemStack moved = actor.slots.get(OFF_HAND_SLOT);
                if (moved == null || !moved.is(Items.DEBUG_STICK)) {
                    return false;
                }
                DebugStickState state = moved.getOrDefault(DataComponents.DEBUG_STICK_STATE, DebugStickState.EMPTY);
                return state.properties().containsValue(BlockStateProperties.AXIS);
            });

            // 带图案的旗帜：图案要进方块实体，再编成 NBT 发给所有人。
            BlockPos bannerGround = BlockPos.containing(actor.position).offset(2, -1, -2);
            BlockPos banner = bannerGround.above();
            BannerPatternLayers layers = bannerLayers(actor.registries);
            ItemStack bannerItem = new ItemStack(Items.BANNER.white());
            bannerItem.set(DataComponents.BANNER_PATTERNS, layers);
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 1, bannerItem));
            actor.send(new ServerboundSetCarriedItemPacket(1));
            int placing = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(bannerGround, 1.0), Direction.UP, bannerGround, false),
                    placing));
            await("components: " + actor.player + " gets the ack for the banner", () -> actor.lastAck >= placing);
            await("components: " + observer.player + " sees the banner",
                    () -> observer.blocks.get(banner) != null
                            && observer.blocks.get(banner).is(Blocks.BANNER.white()));
            await("components: the banner patterns reach the observer",
                    () -> layers.equals(bannerPatterns(observer.blockEntityTags.get(banner), observer.registries)));
            await("components: the banner patterns come back to the placer",
                    () -> layers.equals(bannerPatterns(actor.blockEntityTags.get(banner), actor.registries)));
            actor.edits.add(new Edit(banner, observer.blocks.get(banner)));

            // 头颅的档案：我们写的 NBT 要能被官方的 `ResolvableProfile.CODEC` 解回来。
            BlockPos headGround = BlockPos.containing(actor.position).offset(0, -1, -2);
            BlockPos head = headGround.above();
            ResolvableProfile profile = ResolvableProfile.createUnresolved(actor.player);
            ItemStack headItem = new ItemStack(Items.PLAYER_HEAD);
            headItem.set(DataComponents.PROFILE, profile);
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 1, headItem));
            int placingHead = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(headGround, 1.0), Direction.UP, headGround, false),
                    placingHead));
            await("components: " + actor.player + " gets the ack for the head", () -> actor.lastAck >= placingHead);
            await("components: the head profile reaches the observer",
                    () -> profile.equals(headProfile(observer.blockEntityTags.get(head), observer.registries)));

            // 改了名的箱子：打开时的标题就是那个名字。
            BlockPos chestGround = BlockPos.containing(actor.position).offset(-2, -1, -2);
            BlockPos namedChest = chestGround.above();
            ItemStack chestItem = new ItemStack(Items.CHEST);
            chestItem.set(DataComponents.CUSTOM_NAME, Component.literal(CHEST_NAME));
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 1, chestItem));
            int placingChest = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(chestGround, 1.0), Direction.UP, chestGround, false),
                    placingChest));
            await("components: " + actor.player + " gets the ack for the named chest",
                    () -> actor.lastAck >= placingChest);
            await("components: " + actor.player + " sees the named chest",
                    () -> actor.blocks.get(namedChest) != null && actor.blocks.get(namedChest).is(Blocks.CHEST));
            openChest(actor, namedChest);
            check("components: the menu title is the custom name",
                    actor.openMenuTitle != null && CHEST_NAME.equals(actor.openMenuTitle.getString()));
            actor.send(new ServerboundContainerClosePacket(actor.openContainerId));
            pumpFor(TICK_MILLIS);

            // 清掉这一段用的槽位，后面几段按空物品栏算。
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 1, ItemStack.EMPTY));
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 2, ItemStack.EMPTY));
            actor.send(new ServerboundSetCreativeModeSlotPacket(OFF_HAND_SLOT, ItemStack.EMPTY));
            actor.send(new ServerboundSetCarriedItemPacket(0));
            await("components: the hands are empty again",
                    () -> isEmpty(remote, EquipmentSlot.OFFHAND) && isEmpty(remote, EquipmentSlot.MAINHAND));
        }

        /** 一面红色花朵图案的旗帜，和探针放下去的那一面一样。 */
        private static BannerPatternLayers bannerLayers(RegistryAccess.Frozen registries) {
            Holder<BannerPattern> flower = registries
                    .lookupOrThrow(Registries.BANNER_PATTERN)
                    .getOrThrow(BannerPatterns.FLOWER);
            return new BannerPatternLayers(List.of(new BannerPatternLayers.Layer(flower, DyeColor.RED)));
        }

        /** 方块实体数据里的旗帜图案，按官方 `BannerPatternLayers.CODEC` 解。 */
        private static BannerPatternLayers bannerPatterns(CompoundTag tag, RegistryAccess.Frozen registries) {
            Tag patterns = tag == null ? null : tag.get("patterns");
            if (patterns == null) {
                return null;
            }
            return BannerPatternLayers.CODEC
                    .parse(registries.createSerializationContext(NbtOps.INSTANCE), patterns)
                    .result()
                    .orElse(null);
        }

        /** 方块实体数据里的头颅档案，按官方 `ResolvableProfile.CODEC` 解。 */
        private static ResolvableProfile headProfile(CompoundTag tag, RegistryAccess.Frozen registries) {
            Tag profile = tag == null ? null : tag.get("profile");
            if (profile == null) {
                return null;
            }
            return ResolvableProfile.CODEC
                    .parse(registries.createSerializationContext(NbtOps.INSTANCE), profile)
                    .result()
                    .orElse(null);
        }

        /**
         * 下落的沙子被客户端看到：生成、往下移动、落地后变回方块。
         * 打火石点燃的 TNT 出现，大约 80 tick 后炸掉旁边的石头；红石块也会点燃另一块 TNT。
         *
         * The client sees a falling sand block spawn, move down, and become a block again.
         * Flint-and-steel TNT appears and breaks a neighbouring stone after about 80 ticks.
         * A redstone block lights a second TNT.
         */
        private void worldEntities(Join actor, Join observer) throws IOException {
            Vec3 origin = actor.position;
            BlockPos feet = BlockPos.containing(origin);
            // 柱子要够高，下落才会超过跟踪间隔，客户端才看得到移动。人得跟着往上走，创造模式够不着 40 格。
            // The pillar has to be tall enough that the fall outlasts the tracking interval, so the client
            // sees a move. The player has to climb with it: creative reach cannot touch a 40-block column.
            BlockPos columnBase = feet.offset(4, -1, -4);
            actor.send(new ServerboundSetCarriedItemPacket(1));
            for (int i = 0; i < 40; i++) {
                BlockPos clicked = columnBase.above(i);
                if (i % 3 == 0) {
                    actor.moveTo(new Vec3(clicked.getX() - 1.5, clicked.getY() + 1.0, clicked.getZ() + 0.5), false);
                }
                useOn(actor, Items.STONE, clicked, Direction.UP, Vec3.upFromBottomCenterOf(clicked, 1.0));
            }
            BlockPos support = columnBase.above(40);
            actor.moveTo(new Vec3(support.getX() - 1.5, support.getY(), support.getZ() + 0.5), false);
            useOn(actor, Items.SAND, support, Direction.EAST, Vec3.atCenterOf(support).add(0.5, 0.0, 0.0));
            await("falling: the observer sees the falling block", 10_000,
                    () -> findEntity(observer, EntityTypes.FALLING_BLOCK) != null);
            int fallingId = findEntity(observer, EntityTypes.FALLING_BLOCK);
            Remote falling = observer.remotes.get(fallingId);
            check("falling: the block state is sand", falling.data == Block.getId(Blocks.SAND.defaultBlockState()));
            double spawnedY = falling.position.y;
            int seen = falling.moves + falling.syncs;
            await("falling: the falling block moves", 10_000, () -> {
                Remote remote = observer.remotes.get(fallingId);
                return remote != null && remote.moves + remote.syncs > seen;
            });
            Remote moved = observer.remotes.get(fallingId);
            check("falling: the block is lower than where it spawned", moved != null && moved.position.y < spawnedY);
            BlockPos landing = columnBase.east().above();
            BlockState sand = Blocks.SAND.defaultBlockState();
            await("falling: the sand lands", 15_000, () -> sand.equals(observer.blocks.get(landing)));
            await("falling: the entity is removed", 10_000, () -> !observer.remotes.containsKey(fallingId));
            actor.edits.add(new Edit(landing, sand));

            BlockPos tntGround = feet.offset(0, -1, 20);
            // 站在旁边，不要站进即将放下去的 TNT 里。
            // Stand beside the spot so the player is not inside the TNT about to be placed.
            actor.moveTo(new Vec3(tntGround.getX() - 1.5, origin.y, tntGround.getZ() + 0.5), true);
            useOn(actor, Items.TNT, tntGround, Direction.UP, Vec3.upFromBottomCenterOf(tntGround, 1.0));
            BlockPos tnt = tntGround.above();
            useOn(actor, Items.STONE, tnt, Direction.NORTH, Vec3.atCenterOf(tnt).add(0.0, 0.0, -0.5));
            BlockPos broken = tnt.north();
            await("tnt: the stone next to it is there", () -> observer.blocks.get(broken) != null && observer.blocks.get(broken).is(Blocks.STONE));
            long started = System.currentTimeMillis();
            useOn(actor, Items.FLINT_AND_STEEL, tnt, Direction.UP, Vec3.upFromBottomCenterOf(tnt, 1.0));
            await("tnt: the observer sees the primed tnt", 10_000, () -> findEntity(observer, EntityTypes.TNT) != null);
            int tntId = findEntity(observer, EntityTypes.TNT);
            Remote primed = observer.remotes.get(tntId);
            int primedSeen = primed == null ? 0 : primed.moves + primed.syncs;
            await("tnt: the primed tnt moves", 10_000, () -> {
                Remote remote = observer.remotes.get(tntId);
                return remote != null && remote.moves + remote.syncs > primedSeen;
            });
            await("tnt: the explosion breaks the stone", 20_000,
                    () -> Blocks.AIR.defaultBlockState().equals(observer.blocks.get(broken)));
            long elapsed = System.currentTimeMillis() - started;
            check("tnt: the explosion waits about 80 ticks", elapsed >= 2_000 && elapsed <= 20_000);
            await("tnt: the primed tnt is removed", 10_000, () -> !observer.remotes.containsKey(tntId));
            // 爆炸把这两格变回空气。后来的玩家只从区块第 0 段记录 y < -60 的空气，
            // 这里的空气和原来的空气分不出来，所以不记成改动。
            // The blast turns these cells back into air. A later player only records air below y=-60
            // from chunk section 0, so this air is indistinguishable from the original cell.

            BlockPos redGround = feet.offset(-20, -1, 0);
            actor.moveTo(new Vec3(redGround.getX() - 1.5, origin.y, redGround.getZ() + 0.5), true);
            useOn(actor, Items.TNT, redGround, Direction.UP, Vec3.upFromBottomCenterOf(redGround, 1.0));
            BlockPos redTnt = redGround.above();
            useOn(actor, Items.REDSTONE_BLOCK, redTnt, Direction.NORTH, Vec3.atCenterOf(redTnt).add(0.0, 0.0, -0.5));
            await("tnt: redstone lights another primed tnt", 10_000, () -> findEntity(observer, EntityTypes.TNT) != null);
            await("tnt: the redstone-lit block is gone", 10_000,
                    () -> Blocks.AIR.defaultBlockState().equals(observer.blocks.get(redTnt)));
            actor.moveTo(origin, true);
            actor.send(new ServerboundSetCarriedItemPacket(0));
        }

        /**
         * 观察者目前看得到的这种实体，没有时是 null。
         *
         * The entity of this type the observer can currently see, or null.
         */
        private Integer findEntity(Join join, EntityType<?> type) {
            for (var entry : join.remotes.entrySet()) {
                if (entry.getValue().type == type) {
                    return entry.getKey();
                }
            }
            return null;
        }

        /**
         * 官方客户端保存书与笔再署名。署名不改本地物品，服务端必须把成书同步回来，
         * 标题、作者和页面用官方组件 codec 解出来。
         */
        private void books(Join actor) throws IOException {
            int inventorySlot = 4;
            int menuSlot = HOTBAR_START + inventorySlot;
            List<String> pages = List.of("Hello", "第二页");
            actor.send(new ServerboundSetCreativeModeSlotPacket(menuSlot, new ItemStack(Items.WRITABLE_BOOK)));
            actor.send(new ServerboundEditBookPacket(inventorySlot, pages, Optional.empty()));
            await("book: the edited pages come back", () -> writablePages(actor.slots.get(menuSlot), pages));
            check("book: editing keeps it a book and quill", actor.slots.get(menuSlot).is(Items.WRITABLE_BOOK));

            String title = "My Title";
            actor.send(new ServerboundEditBookPacket(inventorySlot, pages, Optional.of(title)));
            await("book: signing turns it into a written book",
                    () -> actor.slots.getOrDefault(menuSlot, ItemStack.EMPTY).is(Items.WRITTEN_BOOK));
            ItemStack stack = actor.slots.get(menuSlot);
            WrittenBookContent book = stack.get(DataComponents.WRITTEN_BOOK_CONTENT);
            check("book: the written content is present", book != null);
            if (book == null) {
                return;
            }
            check("book: the count is unchanged", stack.getCount() == 1);
            check("book: the title matches", title.equals(book.title().raw()));
            check("book: the title was not filtered", book.title().filtered().isEmpty());
            check("book: the author is the player name", actor.player.equals(book.author()));
            check("book: generation is the original", book.generation() == 0);
            check("book: the book is already resolved", book.resolved());
            check("book: the pages match and were not filtered", writtenPages(book, pages));

            int before = actor.openBooks;
            actor.send(new ServerboundSetCarriedItemPacket(inventorySlot));
            int using = actor.nextSequence();
            actor.send(new ServerboundUseItemPacket(InteractionHand.MAIN_HAND, using, 0.0f, 0.0f));
            await("book: using a written book opens it",
                    () -> actor.openBooks == before + 1 && actor.openBook == InteractionHand.MAIN_HAND);
            check("book: the open book is in the main hand", actor.openBook == InteractionHand.MAIN_HAND);

            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 5, new ItemStack(Items.WRITABLE_BOOK)));
            actor.send(new ServerboundSetCarriedItemPacket(5));
            int quill = actor.nextSequence();
            actor.send(new ServerboundUseItemPacket(InteractionHand.MAIN_HAND, quill, 0.0f, 0.0f));
            await("book: using a book and quill is acked", () -> actor.lastAck >= quill);
            check("book: a book and quill does not open", actor.openBooks == before + 1);
        }

        /** 书与笔的页面：原文对得上，没有过滤文本。 */
        private static boolean writablePages(ItemStack stack, List<String> pages) {
            if (stack == null || !stack.is(Items.WRITABLE_BOOK)) {
                return false;
            }
            WritableBookContent content = stack.get(DataComponents.WRITABLE_BOOK_CONTENT);
            if (content == null || content.pages().size() != pages.size()) {
                return false;
            }
            for (int i = 0; i < pages.size(); i++) {
                var page = content.pages().get(i);
                if (!pages.get(i).equals(page.raw()) || page.filtered().isPresent()) {
                    return false;
                }
            }
            return true;
        }

        /** 成书的页面是字面文本组件。 */
        private static boolean writtenPages(WrittenBookContent book, List<String> pages) {
            if (book.pages().size() != pages.size()) {
                return false;
            }
            for (int i = 0; i < pages.size(); i++) {
                var page = book.pages().get(i);
                if (!pages.get(i).equals(page.raw().getString()) || page.filtered().isPresent()) {
                    return false;
                }
            }
            return true;
        }

        /**
         * 箱子：打开、放进物品、关上、再打开还在，另一名玩家打开能看到同一份内容。
         * 打开和关闭的人数也通过方块事件同步给附近的人。
         */
        private void containers(Join opener, Join watcher) throws IOException {
            BlockPos chest = chestOf(opener);
            // 快捷栏第 3 格放苹果，后面 Shift 点击把它推进箱子。官方不回应这个包，
            // 所以内容要等打开菜单时的完整内容来确认。
            opener.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 3, new ItemStack(Items.APPLE, 7)));
            pumpFor(TICK_MILLIS);

            int openerEvents = opener.blockEvents.size();
            int watcherEvents = watcher.blockEvents.size();
            int firstId = openChest(opener, chest);
            check("container: the chest menu is three rows", MenuType.GENERIC_9x3.equals(opener.openMenuType));
            check("container: the chest menu has sixty-three slots", opener.lastContent.size() == 63);
            check("container: a new menu starts at state one", opener.lastContentState == 1);
            check("container: the apple is in the opened inventory",
                    is(opener.menuSlots.get(54 + 3), Items.APPLE, 7));
            await("container: the open event reaches both players",
                    () -> chestEventCount(opener, chest, openerEvents) == 1
                            && chestEventCount(watcher, chest, watcherEvents) == 1);
            check("container: opening sets the open count to one", lastChestEvent(opener, chest).getB1() == 1);

            // 箱子有 27 格，后面是玩家背包 27 格，再往后才是快捷栏，所以快捷栏第 3 格是 54 + 3。
            int clickState = opener.menuStateId;
            int hotbarSlot = 54 + 3;
            opener.send(opener.click(hotbarSlot, 0, ContainerInput.QUICK_MOVE));
            await("container: shift-click moves the apple into the chest",
                    () -> is(opener.menuSlots.get(0), Items.APPLE, 7)
                            && opener.menuSlots.getOrDefault(hotbarSlot, ItemStack.EMPTY).isEmpty());
            check("container: two slots changed, one state id each", opener.menuStateId == clickState + 2);
            check("container: the shifted stack keeps its count", opener.menuSlots.get(0).getCount() == 7);

            opener.send(new ServerboundContainerClosePacket(opener.openContainerId));
            await("container: closing brings the open count to zero",
                    () -> isClosedChest(opener, chest));

            // 再打开，苹果还在。菜单 ID 换了，内容从头发。
            int secondId = openChest(opener, chest);
            check("container: reopening uses a new container id", secondId != firstId);
            check("container: the apple was kept", is(opener.menuSlots.get(0), Items.APPLE, 7));
            check("container: the reopened menu starts at state one", opener.lastContentState == 1);

            // 第二名玩家走到箱子旁边再打开，看到同一份内容，开盖人数变成二。
            watcher.moveTo(new Vec3(chest.getX() + 0.5, SPAWN_Y, chest.getZ() + 2.5), true);
            pumpFor(TICK_MILLIS);
            openChest(watcher, chest);
            await("container: the second player sees the apple", () -> is(watcher.menuSlots.get(0), Items.APPLE, 7));
            await("container: the open count reaches two", () -> lastChestEvent(opener, chest).getB1() == 2);
            watcher.send(new ServerboundContainerClosePacket(watcher.openContainerId));
            await("container: the open count goes back to one", () -> lastChestEvent(opener, chest).getB1() == 1);
            opener.send(new ServerboundContainerClosePacket(opener.openContainerId));
            await("container: closing the chest is seen by the watcher",
                    () -> isClosedChest(watcher, chest));
        }

        /**
         * 放下工作台，把一根橡木原木放进合成格，结果格应是四块橡木木板。
         * Place a crafting table, put an oak log in the grid, and expect four oak planks.
         */
        private void crafting(Join actor) throws IOException {
            // (2, -1, 2) 上已经有告示牌。换一格空地，避免放不下去。
            // A sign already stands at (2, -1, 2). Use an empty cell so the table can be placed.
            BlockPos ground = BlockPos.containing(actor.position).offset(1, -1, 2);
            BlockPos table = ground.above();
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 6, new ItemStack(Items.CRAFTING_TABLE)));
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 7, new ItemStack(Items.OAK_LOG)));
            actor.send(new ServerboundSetCarriedItemPacket(6));
            int placing = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(ground, 1.0), Direction.UP, ground, false),
                    placing));
            await("crafting: the table is placed",
                    () -> actor.blocks.containsKey(table) && actor.blocks.get(table).is(Blocks.CRAFTING_TABLE));

            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START + 8, ItemStack.EMPTY));
            actor.send(new ServerboundSetCarriedItemPacket(8));
            actor.openMenuType = null;
            actor.lastContentState = -1;
            actor.menuSlots.clear();
            int opening = actor.nextSequence();
            actor.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(table, 1.0), Direction.UP, table, false),
                    opening));
            await("crafting: the screen opens", () -> MenuType.CRAFTING.equals(actor.openMenuType));
            await("crafting: the grid arrives", () -> actor.lastContentState >= 0);
            check("crafting: the menu has forty-six slots", actor.lastContent.size() == 46);

            int logSlot = 37 + 7;
            actor.send(actor.click(logSlot, 0, ContainerInput.PICKUP));
            actor.send(actor.click(1, 0, ContainerInput.PICKUP));
            await("crafting: one log becomes four oak planks",
                    () -> is(actor.menuSlots.get(0), Items.OAK_PLANKS, 4)
                            && is(actor.menuSlots.get(1), Items.OAK_LOG, 1));
            actor.send(new ServerboundContainerClosePacket(actor.openContainerId));
            pumpFor(TICK_MILLIS);
        }

        /** 这名玩家在 `blocks` 里放下的箱子。 */
        private static BlockPos chestOf(Join client) {
            for (Edit edit : client.edits) {
                if (edit.state.is(Blocks.CHEST)) {
                    return edit.pos;
                }
            }
            throw new StepFailed(client.player + " placed no chest");
        }

        /** 右键箱子并等到 open_screen 和完整的 container_set_content 都到了，返回菜单 ID。 */
        private int openChest(Join client, BlockPos chest) throws IOException {
            client.openMenuType = null;
            client.lastContentState = -1;
            client.menuSlots.clear();
            int sequence = client.nextSequence();
            client.send(new ServerboundUseItemOnPacket(
                    InteractionHand.MAIN_HAND,
                    new BlockHitResult(Vec3.upFromBottomCenterOf(chest, 1.0), Direction.UP, chest, false),
                    sequence));
            await("container: " + client.player + " gets the ack for opening", () -> client.lastAck >= sequence);
            await("container: " + client.player + " gets the chest screen", () -> client.openMenuType != null);
            await("container: " + client.player + " gets the chest content", () -> client.lastContentState >= 0);
            return client.openContainerId;
        }

        private static int chestEventCount(Join client, BlockPos pos, int start) {
            int count = 0;
            for (int index = start; index < client.blockEvents.size(); index += 1) {
                ClientboundBlockEventPacket event = client.blockEvents.get(index);
                if (event.getPos().equals(pos) && event.getB0() == 1 && event.getBlock() == Blocks.CHEST) {
                    count += 1;
                }
            }
            return count;
        }

        private static ClientboundBlockEventPacket lastChestEvent(Join client, BlockPos pos) {
            for (int index = client.blockEvents.size() - 1; index >= 0; index -= 1) {
                ClientboundBlockEventPacket event = client.blockEvents.get(index);
                if (event.getPos().equals(pos) && event.getB0() == 1 && event.getBlock() == Blocks.CHEST) {
                    return event;
                }
            }
            return null;
        }

        /** 最后一条开盖事件是 0，说明服务器认为盖子已经合上。 */
        private static boolean isClosedChest(Join client, BlockPos pos) {
            ClientboundBlockEventPacket event = lastChestEvent(client, pos);
            return event != null && event.getB1() == 0;
        }

        private static boolean is(ItemStack stack, Item item, int count) {
            return stack != null && stack.is(item) && stack.getCount() == count;
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

        /**
         * 耐久只剩 1 的镐挖掉一块草方块。先用正式的 `/gamemode survival`（命令包不带前导斜杠），
         * 客户端收到损坏事件，快捷栏那格变空，再用 `/gamemode creative` 回到创造模式。
         *
         * A pickaxe with one durability mines one grass block. The official `/gamemode survival`
         * command (the packet has no leading slash) runs first. The client gets the break event,
         * that hotbar slot becomes empty, then `/gamemode creative` restores creative.
         */
        private void toolDurability(Join actor, Join observer) throws IOException {
            ItemStack pick = new ItemStack(Items.DIAMOND_PICKAXE);
            pick.setDamageValue(pick.getMaxDamage() - 1);
            actor.send(new ServerboundSetCreativeModeSlotPacket(HOTBAR_START, pick));
            actor.send(new ServerboundSetCarriedItemPacket(0));
            // 创造模式写槽不回包。清掉旧记录，后面必须看到这一格被写成空。
            //
            // Creative slot writes are not echoed. Drop the old record so the empty slot has to arrive.
            actor.slots.remove(HOTBAR_START);
            BlockPos feet = BlockPos.containing(actor.position);
            BlockPos grass = feet.offset(3, -1, 1);
            actor.edits.add(new Edit(grass, Blocks.AIR.defaultBlockState()));
            int ownEvents = actor.entityEvents.size();
            int otherEvents = observer.entityEvents.size();
            boolean survival = false;
            try {
                actor.gameModeParam = Float.NaN;
                actor.send(new ServerboundChatCommandPacket("gamemode survival"));
                await("durability: the game mode becomes survival", () -> actor.gameModeParam == 0.0f);
                survival = true;
                // 生存模式不会在进度到 1 时自己挖穿，停手后不够 0.7 会转成延迟破坏，几刻后挖掉。
                //
                // Survival does not finish a block when progress reaches 1. Stopping under 0.7 becomes a delayed break.
                int sequence = actor.nextSequence();
                actor.send(new ServerboundPlayerActionPacket(
                        ServerboundPlayerActionPacket.Action.START_DESTROY_BLOCK, grass, Direction.UP, sequence));
                actor.send(new ServerboundPlayerActionPacket(
                        ServerboundPlayerActionPacket.Action.STOP_DESTROY_BLOCK, grass, Direction.UP, actor.nextSequence()));
                await("durability: the pickaxe breaks", () -> actor.entityEvents.size() > ownEvents
                        && actor.slots.containsKey(HOTBAR_START)
                        && actor.slots.get(HOTBAR_START).isEmpty());
                check("durability: the break event is 47",
                        actor.entityEvents.get(ownEvents) == EQUIPMENT_BREAK_MAIN_HAND);
                await("durability: the observer sees the break", () -> observer.entityEvents.size() > otherEvents);
                check("durability: the observer's break event is 47",
                        observer.entityEvents.get(otherEvents) == EQUIPMENT_BREAK_MAIN_HAND);
            } finally {
                if (survival) {
                    actor.gameModeParam = Float.NaN;
                    actor.send(new ServerboundChatCommandPacket("gamemode creative"));
                    await("durability: the game mode returns to creative", () -> actor.gameModeParam == 1.0f);
                }
            }
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
                    if (edit.state.is(Blocks.OAK_SIGN)) {
                        check("late: the sign is in the block entity list",
                                late.blockEntities.get(edit.pos) == BlockEntityTypes.SIGN);
                        check("late: the chunk carries the sign text",
                                SIGN_TEXT.equals(signLine(late.blockEntityTags.get(edit.pos), "front_text", 0)));
                    }
                    if (edit.state.is(Blocks.BANNER.white())) {
                        check("late: the banner is in the block entity list",
                                late.blockEntities.get(edit.pos) == BlockEntityTypes.BANNER);
                        check("late: the chunk carries the banner patterns",
                                bannerLayers(late.registries)
                                        .equals(bannerPatterns(late.blockEntityTags.get(edit.pos), late.registries)));
                    }
                }
                for (BlockPos pos : editor.cleared) {
                    check("late: blocks that went away are not in the chunk", !late.blocks.containsKey(pos));
                }
            }
        }

        /**
         * 用客户端收到的命令树解析，再把几条命令真的发出去。
         *
         * <p>Parse with the command tree the client received, then send a few commands for real.
         */
        private void commands(Join actor, Join observer, Join late) throws IOException {
            check("commands: literals the client can run parse",
                    actor.commandParses("list")
                            && actor.commandParses("help")
                            && actor.commandParses("gamemode survival")
                            && actor.commandParses("tp ~1 ~ ~")
                            && actor.commandParses("teleport " + observer.player)
                            && actor.commandParses("time set noon")
                            && actor.commandParses("time query gametime")
                            && actor.commandParses("give " + actor.player + " stone 3")
                            && actor.commandParses("clear " + actor.player + " stone")
                            && actor.commandParses("setblock 4 -59 4 stone")
                            && actor.commandParses("setblock 4 -58 4 oak_stairs[facing=east]")
                            && actor.commandParses("fill 6 -59 6 7 -59 7 dirt")
                            && actor.commandParses("say hi")
                            && actor.commandParses("me waves")
                            && actor.commandParses("msg " + observer.player + " ping")
                            && actor.commandParses("tell " + observer.player + " ping")
                            && actor.commandParses("w " + late.player + " ping")
                            && actor.commandParses("kick " + late.player));
            check("commands: incomplete or unknown input fails in the client tree",
                    !actor.commandParses("nope")
                            && !actor.commandParses("gamemode")
                            && !actor.commandParses("time set")
                            && !actor.commandParses("give " + actor.player)
                            && !actor.commandParses("setblock 1 2 3")
                            && !actor.commandParses("setblock 4 -58 4 oak_stairs[facing=foo]")
                            && !actor.commandParses("kick"));

            actor.send(new ServerboundChatCommandPacket("time set noon"));
            await("commands: noon is already the clock",
                    () -> containsText(actor, "already at time marker"));

            actor.send(new ServerboundChatCommandPacket("time set 1000"));
            await("commands: time set is announced", () -> containsText(actor, "Set minecraft:overworld to 1000 tick(s)"));
            check("commands: other ops see the admin broadcast",
                    containsText(observer, "[Kitten: Set minecraft:overworld to 1000 tick(s)]"));

            BlockPos stone = new BlockPos(4, -59, 4);
            BlockPos stairs = new BlockPos(4, -58, 4);
            actor.send(new ServerboundChatCommandPacket("setblock 4 -59 4 stone"));
            await("commands: setblock places stone", () -> actor.blocks.get(stone) != null && actor.blocks.get(stone).is(Blocks.STONE));
            await("commands: the other player sees the stone",
                    () -> observer.blocks.get(stone) != null && observer.blocks.get(stone).is(Blocks.STONE));
            check("commands: setblock announces the position", containsText(actor, "Changed the block at 4, -59, 4"));

            actor.send(new ServerboundChatCommandPacket("setblock 4 -58 4 oak_stairs[facing=east]"));
            await("commands: stairs keep the named facing", () -> {
                BlockState state = actor.blocks.get(stairs);
                return state != null && state.is(Blocks.OAK_STAIRS) && state.getValue(StairBlock.FACING) == Direction.EAST;
            });

            actor.send(new ServerboundChatCommandPacket("fill 6 -59 6 7 -59 7 dirt"));
            await("commands: fill covers the rectangle", () -> filledWithDirt(actor));
            check("commands: fill announces the count", containsText(actor, "Successfully filled 4 block(s)"));

            actor.send(new ServerboundChatCommandPacket("gamemode survival"));
            await("commands: gamemode tells the player", () -> containsText(actor, "Set own game mode to Survival Mode"));
            await("commands: the tab list shows survival", () -> actor.ownGameMode == GameType.SURVIVAL);
            check("commands: survival cannot fly", !actor.canFly);

            // 前面的物品栏脚本放过石头。先清掉，再给 3 个，数量才对得上。
            //
            // Earlier inventory steps left stone behind. Clear it first so a give of 3 is exact.
            actor.send(new ServerboundChatCommandPacket("clear " + actor.player + " stone"));
            await("commands: the old stone is gone", () -> stoneCount(actor) == 0);
            actor.send(new ServerboundChatCommandPacket("give " + actor.player + " stone 3"));
            await("commands: give puts three stone in the inventory", () -> stoneCount(actor) == 3);
            check("commands: give announces the stack", containsText(actor, "Gave 3"));
            actor.send(new ServerboundChatCommandPacket("clear " + actor.player + " stone"));
            await("commands: clear removes the three stone", () -> stoneCount(actor) == 0);
            check("commands: clear announces the removal", containsText(actor, "Removed 3 item(s) from player " + actor.player));

            Join.sendLooseCommand(actor, "say hi");
            await("commands: say reaches both players",
                    () -> actor.chats.stream().anyMatch(text -> text.contains("[" + actor.player + "] hi"))
                            && observer.chats.stream().anyMatch(text -> text.contains("[" + actor.player + "] hi")));
            Join.sendLooseCommand(actor, "me waves");
            await("commands: me reaches the other player",
                    () -> observer.chats.stream().anyMatch(text -> text.contains(actor.player) && text.contains("waves")));
            Join.sendLooseCommand(actor, "msg " + observer.player + " ping");
            await("commands: msg shows the outgoing whisper",
                    () -> actor.chats.stream().anyMatch(text -> text.contains("You whisper to " + observer.player + ": ping")));
            await("commands: msg shows the incoming whisper",
                    () -> observer.chats.stream().anyMatch(text -> text.contains("whispers to you: ping")));

            actor.send(new ServerboundChatCommandPacket("list"));
            await("commands: list counts the three players", () -> containsText(actor, "There are 3 of a max of"));
            actor.send(new ServerboundChatCommandPacket("help gamemode"));
            await("commands: help prints the gamemode usage", () -> containsText(actor, "/gamemode"));

            // 落在走路之后的区块 (1, 0) 里，避免再改跟踪范围。
            //
            // Land inside chunk (1, 0), where the walk already left the player, so tracking does not change.
            // 整数坐标按官方 `Vec3Argument` 把 x/z 对齐到方块中心。
            //
            // Integer coordinates are centred on the block, matching official `Vec3Argument`.
            actor.send(new ServerboundChatCommandPacket("tp 24 -60 8"));
            await("commands: an absolute teleport moves the player",
                    () -> Math.abs(actor.position.x - 24.5) < 0.01 && Math.abs(actor.position.z - 8.5) < 0.01);
            check("commands: teleport announces the destination", containsText(actor, "Teleported " + actor.player));
            actor.send(new ServerboundChatCommandPacket("tp ~1 ~ ~"));
            await("commands: a relative teleport adds one block", () -> Math.abs(actor.position.x - 25.5) < 0.01);

            actor.suggestions = null;
            actor.send(new ServerboundCommandSuggestionPacket(7, "/g"));
            await("commands: literal suggestions arrive",
                    () -> actor.suggestions != null && actor.suggestions.id() == 7);
            check("commands: g completes gamemode and give", suggestionTexts(actor).contains("gamemode")
                    && suggestionTexts(actor).contains("give"));
            actor.suggestions = null;
            actor.send(new ServerboundCommandSuggestionPacket(8, "/teleport "));
            await("commands: player-name suggestions arrive",
                    () -> actor.suggestions != null && actor.suggestions.id() == 8);
            check("commands: teleport completes online names",
                    suggestionTexts(actor).contains(actor.player)
                            && suggestionTexts(actor).contains(observer.player)
                            && suggestionTexts(actor).contains(late.player));

            actor.send(new ServerboundChatCommandPacket("gamemode creative"));
            await("commands: creative is restored", () -> actor.ownGameMode == GameType.CREATIVE);
            actor.send(new ServerboundChatCommandPacket("time set 6000"));
            await("commands: the clock returns to noon", () -> containsText(actor, "Set minecraft:overworld to 6000 tick(s)"));
        }

        private static int stoneCount(Join client) {
            return client.slots.values().stream()
                    .filter(stack -> stack.is(Items.STONE))
                    .mapToInt(ItemStack::getCount)
                    .sum();
        }

        private static boolean containsText(Join client, String part) {
            return client.systemTexts().stream().anyMatch(text -> text.contains(part));
        }

        private static boolean filledWithDirt(Join client) {
            for (int x = 6; x <= 7; x += 1) {
                for (int z = 6; z <= 7; z += 1) {
                    BlockState state = client.blocks.get(new BlockPos(x, -59, z));
                    if (state == null || !state.is(Blocks.DIRT)) {
                        return false;
                    }
                }
            }
            return true;
        }

        private static List<String> suggestionTexts(Join client) {
            if (client.suggestions == null) {
                return List.of();
            }
            return client.suggestions.suggestions().stream().map(ClientboundCommandSuggestionsPacket.Entry::text).toList();
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
        final EntityType<?> type;
        final int data;
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
            type = packet.getType();
            data = packet.getData();
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
        /** 服务端最后认下来的 UUID。离线模式里它就是客户端自己算出的那个。 */
        UUID uuid;
        final HumanoidArm arm;
        private final String host;
        private final int port;
        private final boolean walks;
        /** 客户端按玩家名算出的 UUID，hello 包里报的就是它。 */
        private final UUID offlineUuid;
        /** 假 session server 的根地址。非空就表示这条连接要走正版验证。 */
        private final String sessionUrl;
        private SignatureValidator serviceValidator;
        private SignedMessageChain.Encoder chatEncoder;
        private final MessageSignatureCache signatureCache = MessageSignatureCache.createDefault();
        private final Map<UUID, SignedMessageValidator> peerValidators = new HashMap<>();
        private final Map<UUID, UUID> peerSessionIds = new HashMap<>();
        private final List<String> signedLines = new ArrayList<>();
        private int nextChatIndex;
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
        /** 客户端自己算出来的 serverId 哈希，服务端问 session server 时要用同一个。 */
        private String reportedDigest = "";
        /** 假 session server 说好要发的档案，服务端转过来的必须和它一致。 */
        private UUID expectedUuid;
        private String expectedSkin = "";
        private String expectedSignature = "";

        int entityId;
        Vec3 position = new Vec3(SPAWN_X, SPAWN_Y, SPAWN_Z);
        CommandDispatcher<Object> commands = new CommandDispatcher<>();
        ClientboundCommandSuggestionsPacket suggestions;
        GameType ownGameMode = GameType.CREATIVE;
        boolean canFly = true;
        private boolean commandTreeChecked;
        private boolean permissionEventChecked;
        private boolean abilitiesChecked;
        private boolean attributesChecked;
        private boolean timeChecked;
        private boolean loadEventChecked;
        private boolean spawnPositionChecked;
        final Map<UUID, String> tab = new HashMap<>();
        final Map<UUID, Boolean> hats = new HashMap<>();
        final Map<Integer, Remote> remotes = new HashMap<>();
        final Map<Integer, Map<Integer, Object>> metadata = new HashMap<>();
        final Map<BlockPos, BlockState> blocks = new HashMap<>();
        final Map<BlockPos, BlockEntityType<?>> blockEntities = new HashMap<>();
        /** 方块实体数据里的 NBT，告示牌的文字在这里。 */
        final Map<BlockPos, CompoundTag> blockEntityTags = new HashMap<>();
        /** 收到过编辑界面的告示牌，值是玩家要编辑的那一面。 */
        final Map<BlockPos, SignTextSlot> signEditors = new HashMap<>();
        final Map<Integer, ItemStack> slots = new HashMap<>();
        /** 打开的菜单里的槽，和容器 0 的 `slots` 分开记。 */
        final Map<Integer, ItemStack> menuSlots = new HashMap<>();
        /** 活塞伸出、收回的动画事件，箱子开合的人数也走这个包。 */
        final List<ClientboundBlockEventPacket> blockEvents = new ArrayList<>();
        final List<Edit> edits = new ArrayList<>();
        /** 放下又消失的方块。地面上的空气 `recordChunk` 不记，所以不能写成 `Edit`。 */
        final List<BlockPos> cleared = new ArrayList<>();
        final List<String> events = new ArrayList<>();
        /** 每个 `section_blocks_update` 里的格子，按包里的顺序。 */
        final List<List<BlockPos>> sectionUpdates = new ArrayList<>();
        final List<ClientboundLevelEventPacket> levelEvents = new ArrayList<>();
        final List<ClientboundSoundPacket> sounds = new ArrayList<>();
        final List<ClientboundSystemChatPacket> systemMessages = new ArrayList<>();
        final List<String> chats = new ArrayList<>();
        final List<String> narrations = new ArrayList<>();
        int entityDataPackets;
        int heldSlot = -1;
        int lastAck = -1;
        /** 最近一次 `CHANGE_GAME_MODE` 的参数。还没收到时是 NaN。 */
        // The latest `CHANGE_GAME_MODE` parameter. NaN until one arrives.
        float gameModeParam = Float.NaN;
        /** 收到的实体事件字节。物品损坏是 47 或 48。 */
        // Entity-event bytes. An item break is 47 or 48.
        final List<Byte> entityEvents = new ArrayList<>();
        int acks;
        int stats;
        int openContainerId = -1;
        /** 右键成书收到的 `open_book`。
         *
         * <p>The `open_book` packet from using a written book.
         */
        InteractionHand openBook;
        int openBooks;
        MenuType<?> openMenuType;
        /** 最后一次 `open_screen` 的标题。 */
        Component openMenuTitle;
        int menuStateId = -1;
        int lastContentState = -1;
        List<ItemStack> lastContent = List.of();

        Join(
                String host,
                int port,
                LayeredRegistryAccess<RegistryLayer> layers,
                String player,
                HumanoidArm arm,
                boolean walks) throws IOException {
            this(host, port, layers, player, arm, walks, null);
        }

        Join(
                String host,
                int port,
                LayeredRegistryAccess<RegistryLayer> layers,
                String player,
                HumanoidArm arm,
                boolean walks,
                String sessionUrl) throws IOException {
            this.host = host;
            this.port = port;
            this.layers = layers;
            this.player = player;
            this.offlineUuid = UUIDUtil.createOfflinePlayerUUID(player);
            this.uuid = offlineUuid;
            this.arm = arm;
            this.walks = walks;
            this.sessionUrl = sessionUrl;
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
                    "handleHello", packet -> onHello((ClientboundHelloPacket) packet),
                    "handleDisconnect", packet -> onLoginRefused((ClientboundLoginDisconnectPacket) packet))));
            connection.send(new ServerboundHelloPacket(player, offlineUuid));
        }

        /** 这条连接要不要走正版验证。 */
        private boolean online() {
            return sessionUrl != null;
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

        /** 正版验证那一段只要走到进服：区块到齐，而且 tab 里是 session server 给的身份。 */
        boolean onlineJoinDone() {
            return spawnChunksDone() && tab.containsKey(uuid);
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

        /**
         * 官方 {@code ClientHandshakePacketListenerImpl.handleHello}：算哈希、发密钥包、切换加密。
         *
         * <p>真客户端这一步去问 Mojang 的 {@code joinServer}，这里把哈希报给假 session server，
         * 它拿这个哈希等服务端来问。密钥包写出去之后才切换加密，和官方的发送监听器一样。
         */
        private void onHello(ClientboundHelloPacket packet) {
            if (!online()) {
                check("login: an offline server does not ask for a key", false);
                return;
            }
            check("online: the server asks the client to authenticate", packet.shouldAuthenticate());
            check("online: the server id is empty like the official one", packet.getServerId().isEmpty());
            check("online: the challenge is four bytes", packet.getChallenge().length == 4);
            try {
                SecretKey secret = Crypt.generateSecretKey();
                PublicKey publicKey = packet.getPublicKey();
                reportedDigest =
                        new BigInteger(Crypt.digestData(packet.getServerId(), publicKey, secret)).toString(16);
                Cipher decrypt = Crypt.getCipher(Cipher.DECRYPT_MODE, secret);
                Cipher encrypt = Crypt.getCipher(Cipher.ENCRYPT_MODE, secret);
                ServerboundKeyPacket key = new ServerboundKeyPacket(secret, publicKey, packet.getChallenge());
                expectProfile(reportedDigest);
                connection.send(key, PacketSendListener.thenRun(
                        () -> connection.setEncryptionKey(decrypt, encrypt)));
            } catch (Exception error) {
                check("online: the client could answer the key request (" + error + ")", false);
            }
        }

        /** 把自己算出的哈希报给假 session server，拿回这次它要发的档案。 */
        private void expectProfile(String digest) throws IOException {
            String[] fields = httpGet(sessionUrl + "/probe/expect?username=" + player + "&hash=" + digest).split(" ");
            expectedUuid = UUID.fromString(fields[0]);
            expectedSkin = fields[1];
            expectedSignature = fields[2];
        }

        /** 服务端问 session server 的哈希，要和客户端自己算出来的一字不差。 */
        void checkDigestTheServerAsked() throws IOException {
            String asked = httpGet(sessionUrl + "/probe/asked");
            check("online: the server asked the session server about the client's digest",
                    !reportedDigest.isEmpty() && asked.equals(reportedDigest));
        }

        /** 皮肤要原样转发，签名也要带着，不然客户端不显示披风。 */
        private void checkSkin(String what, Collection<Property> textures) {
            check(what, textures.size() == 1
                    && textures.stream().allMatch(property -> property.value().equals(expectedSkin)
                            && expectedSignature.equals(property.signature())));
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
            var profile = packet.gameProfile();
            check("login: name is kept", profile.name().equals(player));
            if (online()) {
                check("online: the login profile uses the uuid from the session server",
                        profile.id().equals(expectedUuid));
                check("online: the login uuid is not the offline one", !profile.id().equals(offlineUuid));
                checkSkin("online: the login profile carries the signed skin",
                        profile.properties().get("textures"));
            } else {
                check("login: uuid uses the offline algorithm", profile.id().equals(offlineUuid));
                check("login: an offline profile has no skin", profile.properties().isEmpty());
            }
            uuid = profile.id();
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
            handlers.put("handleCommands", value -> onCommands((ClientboundCommandsPacket) value));
            handlers.put("handleEntityEvent", value -> onEntityEvent((ClientboundEntityEventPacket) value));
            handlers.put("handleCommandSuggestions", value -> suggestions = (ClientboundCommandSuggestionsPacket) value);
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
            handlers.put("handleChunkBlocksUpdate",
                    value -> onSectionBlocksUpdate((ClientboundSectionBlocksUpdatePacket) value));
            handlers.put("handleBlockEntityData",
                    value -> onBlockEntityData((ClientboundBlockEntityDataPacket) value));
            handlers.put("handleOpenSignEditor",
                    value -> onOpenSignEditor((ClientboundOpenSignEditorPacket) value));
            handlers.put("handleBlockChangedAck", value -> onAck((ClientboundBlockChangedAckPacket) value));
            handlers.put("handleBlockEvent", value -> onBlockEvent((ClientboundBlockEventPacket) value));
            handlers.put("handleLevelEvent", value -> onLevelEvent((ClientboundLevelEventPacket) value));
            handlers.put("handleSoundEvent", value -> onSound((ClientboundSoundPacket) value));
            handlers.put("handleOpenBook", value -> onOpenBook((ClientboundOpenBookPacket) value));
            handlers.put("handleOpenScreen", value -> onOpenScreen((ClientboundOpenScreenPacket) value));
            handlers.put("handleContainerContent",
                    value -> onContainerContent((ClientboundContainerSetContentPacket) value));
            handlers.put("handleContainerClose", value -> onContainerClose((ClientboundContainerClosePacket) value));
            handlers.put("handleContainerSetSlot", value -> onSetSlot((ClientboundContainerSetSlotPacket) value));
            handlers.put("handleSetHeldSlot", value -> heldSlot = ((ClientboundSetHeldSlotPacket) value).slot());
            handlers.put("handleSystemChat", value -> onSystemChat((ClientboundSystemChatPacket) value));
            handlers.put("handleDisguisedChat", value -> onDisguisedChat((ClientboundDisguisedChatPacket) value));
            handlers.put("handlePlayerChat", value -> onPlayerChat((ClientboundPlayerChatPacket) value));
            handlers.put("handleAwardStats", value -> onAwardStats((ClientboundAwardStatsPacket) value));
            return handlers;
        }

        /** 官方客户端用收到的树解析。重定向要看最深一层有没有可执行节点。
         *
         * <p>Parse with the tree the client received. A redirect counts only when its deepest node executes.
         */
        boolean commandParses(String input) {
            try {
                ParseResults<Object> results = commands.parse(input, null);
                CommandContextBuilder<Object> context = results.getContext();
                while (context.getChild() != null) {
                    context = context.getChild();
                }
                return context.getCommand() != null && !results.getReader().canRead();
            } catch (RuntimeException error) {
                return false;
            }
        }

        private void onCommands(ClientboundCommandsPacket packet) {
            try {
                commands = new CommandDispatcher<>(packet.getRoot(
                        CommandBuildContext.simple(registries, FeatureFlags.VANILLA_SET), commandNodes()));
            } catch (RuntimeException error) {
                check("commands: the client accepted the command tree (" + error + ")", false);
                return;
            }
            if (!commandTreeChecked) {
                commandTreeChecked = true;
                check("commands: the client accepted the command tree", !commands.getRoot().getChildren().isEmpty());
            }
        }

        private void onEntityEvent(ClientboundEntityEventPacket packet) {
            entityEvents.add(packet.getEventId());
            if (permissionEventChecked) {
                return;
            }
            permissionEventChecked = true;
            int entity = privateInt(packet, ClientboundEntityEventPacket.class, "entityId");
            check("commands: the permission event names this player", entity == entityId);
            // 离线探针的工作目录里 ops.json 把这几名玩家写成 4。正版那台服务端没有这份名单。
            //
            // The offline probe's ops.json lists these players at level 4. The online-mode
            // server has no op list.
            if (online()) {
                check("commands: a player absent from ops.json is level 0", packet.getEventId() == 24);
            } else {
                check("commands: the permission event is owners", packet.getEventId() == 28);
            }
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
            if (online()) {
                check("online: the login packet says the server is in online mode", packet.onlineMode());
                check("online: the login packet enforces secure chat", packet.enforcesSecureChat());
            } else {
                check("play: offline mode", !packet.onlineMode());
            }
        }

        private void onAbilities(ClientboundPlayerAbilitiesPacket packet) {
            canFly = packet.canFly();
            // 进服第一包是创造。后面的 `/gamemode` 会再发。
            //
            // The first packet is creative. Later `/gamemode` packets replace it.
            if (!abilitiesChecked) {
                abilitiesChecked = true;
                check("play: creative abilities", packet.canFly() && packet.canInstabuild() && packet.isInvulnerable());
            }
        }

        private void onPosition(ClientboundPlayerPositionPacket packet) {
            var incoming = packet.change().position();
            var relatives = packet.relatives();
            // 相对轴上的值是差值，绝对轴上的值就是落点。
            //
            // A relative axis carries a delta. An absolute axis carries the destination.
            double x = relatives.contains(Relative.X) ? position.x + incoming.x : incoming.x;
            double y = relatives.contains(Relative.Y) ? position.y + incoming.y : incoming.y;
            double z = relatives.contains(Relative.Z) ? position.z + incoming.z : incoming.z;
            if (!spawnPositionChecked) {
                spawnPositionChecked = true;
                check("play: spawn is above the grass", incoming.y == SPAWN_Y);
                check("play: position is absolute", relatives.isEmpty());
            }
            position = new Vec3(x, y, z);
            connection.send(new ServerboundAcceptTeleportationPacket(packet.id(), x, y, z, 0f, 0f));
        }

        private void onPlayerInfo(ClientboundPlayerInfoUpdatePacket packet) {
            // 不带 ADD_PLAYER 的包里 profile 是 null，这种包是周期性的延迟广播、帽子变化和模式变化。
            var actions = packet.actions();
            if (actions.equals(EnumSet.of(ClientboundPlayerInfoUpdatePacket.Action.UPDATE_GAME_MODE))) {
                packet.entries().forEach(entry -> {
                    if (entry.profileId().equals(uuid)) {
                        ownGameMode = entry.gameMode();
                    }
                });
                return;
            }
            if (actions.equals(EnumSet.of(ClientboundPlayerInfoUpdatePacket.Action.INITIALIZE_CHAT))) {
                rememberChatSessions(packet);
                return;
            }
            if (actions.equals(EnumSet.of(ClientboundPlayerInfoUpdatePacket.Action.UPDATE_HAT))) {
                packet.entries().forEach(entry -> hats.put(entry.profileId(), entry.showHat()));
                return;
            }
            if (actions.equals(EnumSet.of(ClientboundPlayerInfoUpdatePacket.Action.UPDATE_GAME_MODE))) {
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
                if (online()) {
                    if (name.equals(player)) {
                        check("online: the tab uuid comes from the session server",
                                entry.profileId().equals(expectedUuid));
                    } else {
                        check("online: another player's tab uuid is not the offline one",
                                !entry.profileId().equals(UUIDUtil.createOfflinePlayerUUID(name)));
                    }
                    checkSkin("online: the skin from the session server reached the tab list",
                            entry.profile().properties().get("textures"));
                } else {
                    check("play: tab uuid uses the offline algorithm",
                            entry.profileId().equals(UUIDUtil.createOfflinePlayerUUID(name)));
                    check("play: an offline tab entry has no skin", entry.profile().properties().isEmpty());
                }
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
            check("play: latency is a non-negative millisecond count",
                    packet.entries().stream().allMatch(entry -> entry.latency() >= 0));
            latencyUpdated = true;
        }

        private void onSetTime(ClientboundSetTimePacket packet) {
            // 登录只核对第一包。后面的 `/time` 会改主世界钟，也可能带上末地钟。
            //
            // Only the first packet is the login clock. Later `/time` changes it and may include the end clock.
            if (timeChecked) {
                return;
            }
            timeChecked = true;
            check("play: one clock is synced", packet.clockUpdates().size() == 1);
            packet.clockUpdates().forEach((clock, state) -> {
                check("play: clock is the overworld one",
                        clock.unwrapKey().map(key -> key.identifier().getPath().equals("overworld")).orElse(false));
                check("play: clock is at noon", state.totalTicks() == 6000);
            });
        }

        private void onGameEvent(ClientboundGameEventPacket packet) {
            if (loadEventChecked) {
                check("play: later game events are mode changes",
                        packet.getEvent() == ClientboundGameEventPacket.CHANGE_GAME_MODE);
                if (packet.getEvent() == ClientboundGameEventPacket.CHANGE_GAME_MODE) {
                    gameModeParam = packet.getParam();
                }
                return;
            }
            loadEventChecked = true;
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
            // 进服第一包是创造触及距离。`/gamemode` 之后还会再发。
            //
            // The first packet is the creative reach. `/gamemode` sends another one later.
            if (attributesChecked) {
                return;
            }
            attributesChecked = true;
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
            if (inner.getFirst() instanceof ClientboundAddEntityPacket added) {
                if (added.getType() == EntityTypes.PLAYER) {
                    checkPairingBundle(inner);
                } else {
                    checkWorldEntityBundle(inner, added);
                }
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

        /**
         * 下落方块和点燃的 TNT 不是玩家：没有血量和皮肤，下落方块的 data 是方块状态。
         *
         * Falling blocks and primed TNT are not players: no health or skin. A falling block's
         * data is its block state.
         */
        private void checkWorldEntityBundle(List<Packet<? super ClientGamePacketListener>> inner, ClientboundAddEntityPacket added) {
            if (added.getType() == EntityTypes.FALLING_BLOCK) {
                check("bundle: a falling block carries its block state", added.getData() != 0);
                boolean hasData = inner.size() > 1 && inner.get(1) instanceof ClientboundSetEntityDataPacket;
                check("bundle: falling block metadata follows add_entity", hasData);
                if (hasData) {
                    var data = (ClientboundSetEntityDataPacket) inner.get(1);
                    check("bundle: falling block metadata names the entity", data.id() == added.getId());
                    check("bundle: falling block metadata has a start position",
                            data.packedItems().stream().anyMatch(item -> item.id() == 8));
                }
            }
            if (added.getType() == EntityTypes.TNT) {
                check("bundle: primed tnt data is zero", added.getData() == 0);
            }
        }

        private void onAddEntity(ClientboundAddEntityPacket packet) {
            check("entity: add_entity is in a bundle", inBundle);
            check("entity: the client's own entity is never added", packet.getId() != entityId);
            boolean player = packet.getType() == EntityTypes.PLAYER;
            boolean falling = packet.getType() == EntityTypes.FALLING_BLOCK;
            boolean tnt = packet.getType() == EntityTypes.TNT;
            check("entity: only players, falling blocks and primed tnt are added", player || falling || tnt);
            if (player) {
                check("entity: add_entity comes after the tab entry", tab.containsKey(packet.getUUID()));
                check("entity: no initial movement", packet.getMovement().equals(Vec3.ZERO));
                check("entity: data is zero", packet.getData() == 0);
            }
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
            recordBlock(packet.getPos(), packet.getBlockState());
        }

        /** 和官方客户端一样，用包自己的 `runUpdates` 把每一格交出来。 */
        private void onSectionBlocksUpdate(ClientboundSectionBlocksUpdatePacket packet) {
            List<BlockPos> positions = new ArrayList<>();
            packet.runUpdates((pos, state) -> {
                positions.add(pos.immutable());
                recordBlock(pos, state);
            });
            check("blocks: a section update carries more than one block", positions.size() > 1);
            sectionUpdates.add(positions);
        }

        private void recordBlock(BlockPos pos, BlockState state) {
            BlockPos immutable = pos.immutable();
            blocks.put(immutable, state);
            events.add("block " + immutable.toShortString() + " " + Block.getId(state));
        }

        private void onBlockEntityData(ClientboundBlockEntityDataPacket packet) {
            // 有内容要发给客户端的方块实体：告示牌的文字，旗帜、头颅和陶罐从物品带来的组件。
            check("blocks: block entity data only comes from block entities we fill in",
                    packet.getType() == BlockEntityTypes.SIGN
                            || packet.getType() == BlockEntityTypes.HANGING_SIGN
                            || packet.getType() == BlockEntityTypes.BANNER
                            || packet.getType() == BlockEntityTypes.SKULL
                            || packet.getType() == BlockEntityTypes.DECORATED_POT);
            blockEntityTags.put(packet.getPos().immutable(), packet.getTag());
            events.add("data " + packet.getPos().toShortString());
        }

        private void onOpenSignEditor(ClientboundOpenSignEditorPacket packet) {
            signEditors.put(packet.pos().immutable(), packet.slot());
            events.add("editor " + packet.pos().toShortString());
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

        private void onSound(ClientboundSoundPacket packet) {
            sounds.add(packet);
        }

        private void onBlockEvent(ClientboundBlockEventPacket packet) {
            blockEvents.add(packet);
            events.add("blockevent " + packet.getPos().toShortString());        }

        private void onSetSlot(ClientboundContainerSetSlotPacket packet) {
            if (packet.getContainerId() == 0) {
                slots.put(packet.getSlot(), packet.getItem());
                return;
            }
            check("container: slot changes belong to the open menu",
                    packet.getContainerId() == openContainerId);
            check("container: the state id goes up", packet.getStateId() > menuStateId);
            menuStateId = packet.getStateId();
            menuSlots.put((int) packet.getSlot(), packet.getItem());
        }

        private void onOpenBook(ClientboundOpenBookPacket packet) {
            openBook = packet.hand();
            openBooks++;
        }

        private void onOpenScreen(ClientboundOpenScreenPacket packet) {
            openContainerId = packet.getContainerId();
            openMenuType = packet.getType();
            openMenuTitle = packet.getTitle();
            menuSlots.clear();
            menuStateId = -1;
            events.add("open " + openContainerId);
        }

        private void onContainerContent(ClientboundContainerSetContentPacket packet) {
            check("container: content names the open menu", packet.containerId() == openContainerId);
            menuStateId = packet.stateId();
            lastContentState = packet.stateId();
            lastContent = packet.items();
            menuSlots.clear();
            for (int index = 0; index < packet.items().size(); index += 1) {
                menuSlots.put(index, packet.items().get(index));
            }
        }

        private void onContainerClose(ClientboundContainerClosePacket packet) {
            check("container: close names the open menu", packet.getContainerId() == openContainerId);
            events.add("close " + packet.getContainerId());
        }

        /** 一次菜单点击。客户端不预测：没有改过的槽，光标是空的。 */
        Packet<?> click(int slot, int button, ContainerInput input) {
            return new ServerboundContainerClickPacket(
                    openContainerId,
                    menuStateId,
                    (short) slot,
                    (byte) button,
                    input,
                    new Int2ObjectOpenHashMap<>(),
                    HashedStack.EMPTY);
        }

        private void onSystemChat(ClientboundSystemChatPacket packet) {
            systemMessages.add(packet);
            events.add("system " + packet.content().getString());
        }

        void installChatSession(String root) throws IOException {
            try {
                byte[] pkcs8 = Base64.getDecoder().decode(httpGet(root + "/probe/service-private"));
                KeyFactory factory = KeyFactory.getInstance("RSA");
                PrivateKey service = factory.generatePrivate(new PKCS8EncodedKeySpec(pkcs8));
                var crt = (java.security.interfaces.RSAPrivateCrtKey) service;
                PublicKey servicePublic = factory.generatePublic(
                        new java.security.spec.RSAPublicKeySpec(crt.getModulus(), crt.getPublicExponent()));
                serviceValidator = SignatureValidator.from(servicePublic, "SHA1withRSA");

                KeyPairGenerator generator = KeyPairGenerator.getInstance("RSA");
                generator.initialize(2048);
                var pair = generator.generateKeyPair();
                Instant expires = Instant.now().plus(Duration.ofDays(1));
                UUID sessionId = UUID.randomUUID();
                byte[] keyBytes = pair.getPublic().getEncoded();
                ByteBuffer buffer = ByteBuffer.allocate(24 + keyBytes.length).order(ByteOrder.BIG_ENDIAN);
                buffer.putLong(uuid.getMostSignificantBits());
                buffer.putLong(uuid.getLeastSignificantBits());
                buffer.putLong(expires.toEpochMilli());
                buffer.put(keyBytes);
                Signature signer = Signature.getInstance("SHA1withRSA");
                signer.initSign(service);
                signer.update(buffer.array());
                byte[] keySignature = signer.sign();
                var data = new ProfilePublicKey.Data(expires, pair.getPublic(), keySignature);
                send(new ServerboundChatSessionUpdatePacket(new RemoteChatSession.Data(sessionId, data)));
                chatEncoder = new SignedMessageChain(uuid, sessionId)
                        .encoder(Signer.from(pair.getPrivate(), "SHA256withRSA"));
            } catch (Exception error) {
                check("secure: " + player + " could install a chat session (" + error + ")", false);
            }
        }

        boolean hasPeerSession(UUID id) {
            return peerValidators.containsKey(id);
        }

        boolean verifiedSigned(String text) {
            return signedLines.stream().anyMatch(line -> line.contains(text));
        }

        int verifiedCount() {
            return signedLines.size();
        }

        boolean sawTranslation(String key) {
            for (ClientboundSystemChatPacket message : systemMessages) {
                if (message.content().getContents()
                        instanceof net.minecraft.network.chat.contents.TranslatableContents contents
                        && contents.getKey().equals(key)) {
                    return true;
                }
            }
            return false;
        }

        void sendSignedChat(String text) throws IOException {
            sendSigned(text, null);
        }

        void sendSignedEmote(String action) throws IOException {
            sendSigned(action, "action");
        }

        void sendTamperedChat(String text) throws IOException {
            if (chatEncoder == null) {
                check("secure: " + player + " has a chat encoder", false);
                return;
            }
            Instant time = Instant.now();
            LastSeenMessages.Update update = emptyLastSeen();
            SignedMessageBody body = new SignedMessageBody(text, time, 7L, LastSeenMessages.EMPTY);
            MessageSignature signature = chatEncoder.pack(body);
            byte[] raw = signature.bytes().clone();
            raw[0] ^= 0x01;
            send(new ServerboundChatPacket(text, time, 7L, Optional.of(new MessageSignature(raw)), update));
        }

        private void sendSigned(String text, String argument) throws IOException {
            if (chatEncoder == null) {
                check("secure: " + player + " has a chat encoder", false);
                return;
            }
            Instant time = Instant.now();
            LastSeenMessages.Update update = emptyLastSeen();
            SignedMessageBody body = new SignedMessageBody(text, time, 7L, LastSeenMessages.EMPTY);
            MessageSignature signature = chatEncoder.pack(body);
            if (argument == null) {
                send(new ServerboundChatPacket(text, time, 7L, Optional.of(signature), update));
            } else {
                var entries = List.of(new ArgumentSignatures.Entry(argument, signature));
                send(new ServerboundChatCommandSignedPacket(
                        "me " + text, time, 7L, new ArgumentSignatures(entries), update));
            }
        }

        /** 离线客户端对带 `message` 参数的命令发签名包，但签名个数是 0。 */
        private static void sendLooseCommand(Join actor, String command) throws IOException {
            actor.send(new ServerboundChatCommandSignedPacket(
                    command,
                    Instant.now(),
                    0L,
                    new ArgumentSignatures(List.of()),
                    new LastSeenMessages.Update(0, new BitSet(), (byte) 1)));
        }

        private static LastSeenMessages.Update emptyLastSeen() {
            return new LastSeenMessages.Update(0, new BitSet(20), (byte) 1);
        }

        private void rememberChatSessions(ClientboundPlayerInfoUpdatePacket packet) {
            if (serviceValidator == null) {
                return;
            }
            for (var entry : packet.entries()) {
                RemoteChatSession.Data data = entry.chatSession();
                if (data == null) {
                    continue;
                }
                try {
                    String name = tab.getOrDefault(entry.profileId(), player);
                    RemoteChatSession session = data.validate(new GameProfile(entry.profileId(), name), serviceValidator);
                    peerSessionIds.put(entry.profileId(), session.sessionId());
                    peerValidators.put(
                            entry.profileId(),
                            session.createMessageValidator(ProfilePublicKey.EXPIRY_GRACE_PERIOD));
                } catch (Exception error) {
                    check("secure: " + player + " accepted a peer profile key (" + error + ")", false);
                }
            }
        }

        private void onPlayerChat(ClientboundPlayerChatPacket packet) {
            check("secure: player chat indexes are contiguous", packet.globalIndex() == nextChatIndex);
            nextChatIndex += 1;
            var body = packet.body().unpack(signatureCache);
            check("secure: last-seen signatures unpack", body.isPresent());
            if (body.isEmpty()) {
                return;
            }
            MessageSignature signature = packet.signature().orElse(null);
            if (signature == null) {
                Component content = packet.unsignedContent().orElseGet(() -> Component.literal(body.get().content()));
                chats.add(packet.chatType().decorate(content).getString());
                narrations.add(packet.chatType().decorateNarration(content).getString());
                String type = packet.chatType().chatType().unwrapKey().map(key -> key.identifier().getPath()).orElse("");
                if (type.equals("chat") || type.equals("say_command") || type.equals("emote_command")) {
                    check("chat: player chat has no target", packet.chatType().targetName().isEmpty());
                }
                return;
            }
            signatureCache.push(body.get(), signature);
            SignedMessageValidator validator = peerValidators.get(packet.sender());
            UUID sessionId = peerSessionIds.get(packet.sender());
            if (validator == null || sessionId == null) {
                check("secure: player chat has a session and a signature", false);
                return;
            }
            SignedMessageLink link = new SignedMessageLink(packet.index(), packet.sender(), sessionId);
            PlayerChatMessage message = new PlayerChatMessage(
                    link,
                    signature,
                    body.get(),
                    packet.unsignedContent().orElse(null),
                    packet.filterMask());
            PlayerChatMessage accepted = validator.updateAndValidate(message);
            check("secure: the official validator accepted the message", accepted != null);
            if (accepted != null) {
                signedLines.add(packet.chatType().decorate(accepted.decoratedContent()).getString());
            }
        }

        private void onDisguisedChat(ClientboundDisguisedChatPacket packet) {
            chats.add(packet.chatType().decorate(packet.message()).getString());
            narrations.add(packet.chatType().decorateNarration(packet.message()).getString());
            // `/msg` 带目标名字。普通聊天、`/say` 和 `/me` 没有。
            //
            // `/msg` carries a target name. Player chat, `/say`, and `/me` do not.
            String type = packet.chatType().chatType().unwrapKey().map(key -> key.identifier().getPath()).orElse("");
            if (type.equals("chat") || type.equals("say_command") || type.equals("emote_command")) {
                check("chat: player chat has no target", packet.chatType().targetName().isEmpty());
            }
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
                // 告示牌的文字和旗帜的图案都在 `getUpdateTag` 里。旗帜走 `saveWithoutMetadata`，
                // 哪怕没有图案也带一张空的组件表，所以它的标签一定不是空的。
                if (isSign(type)) {
                    check("chunk: signs carry their text", tag != null);
                } else if (type == BlockEntityTypes.BANNER) {
                    check("chunk: banners carry their update tag", tag != null);
                } else if (type != BlockEntityTypes.SKULL && type != BlockEntityTypes.DECORATED_POT) {
                    check("chunk: block entities have no NBT", tag == null);
                }
                if (tag != null) {
                    blockEntityTags.put(pos.immutable(), tag);
                }
                blockEntities.put(pos.immutable(), type);
            });
        }

        private static boolean isSign(BlockEntityType<?> type) {
            return type == BlockEntityTypes.SIGN || type == BlockEntityTypes.HANGING_SIGN;
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
