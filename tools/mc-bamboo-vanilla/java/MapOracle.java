// 地图官方对拍夹具：
// 1. 用官方 NoiseBasedChunkGenerator（CARVERS 阶段）生成固定种子 8×9 区块，跑官方
//    MapItem.update 得到 scale=0 128×128 颜色、scale=1 128×128 颜色，以及含水半砖 /
//    MapColor.NONE / 虚空兜底基岩的专项方块颜色；
// 2. 用官方 MapItemSavedData.CODEC 与 MapIndex.CODEC 解码 Bamboo 写出的
//    data/minecraft/maps/0.dat 与 last_id.dat，核对 DataVersion 后再由官方重新编码写回；
// 3. 模拟玩家手持地图沿固定轨迹走 20 刻（含越界、远距消失、回图、锁定地图 6 刻），
//    调用官方 MapItem.inventoryTick 与 getUpdatePacket，把每刻的
//    ClientboundMapItemDataPacket 按 STREAM_CODEC 编码成精确字节。
//
// 只调官方类，不启动服务端，不接受 EULA。
//
// Official map oracle:
// 1. Generate a fixed-seed 8x9 chunk region through CARVERS with vanilla
//    NoiseBasedChunkGenerator and run MapItem.update for scale=0, scale=1, and
//    special-block columns (waterlogged slab, glass MapColor.NONE, void bedrock);
// 2. Decode Bamboo's maps/0.dat and maps/last_id.dat via official codecs,
//    validate DataVersion, and re-encode them with vanilla codecs;
// 3. Walk a player holding a map for 20 ticks (plus 6 ticks on a locked map)
//    through MapItem.inventoryTick / getUpdatePacket and emit exact wire bytes
//    from ClientboundMapItemDataPacket.STREAM_CODEC.

import com.google.gson.JsonArray;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.mojang.authlib.GameProfile;
import io.netty.buffer.Unpooled;
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
import java.util.EnumSet;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ForkJoinPool;
import java.util.function.Supplier;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.core.component.DataComponentInitializers;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientboundMapItemDataPacket;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.level.WorldGenRegion;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.ItemTags;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.StaticCache2D;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.player.PlayerEquipment;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.MapItem;
import net.minecraft.world.item.context.UseOnContext;
import net.minecraft.world.item.crafting.CraftingInput;
import net.minecraft.world.item.crafting.Ingredient;
import net.minecraft.world.item.crafting.MapExtendingRecipe;
import net.minecraft.world.item.crafting.TransmuteResult;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.SlabBlock;
import net.minecraft.world.level.block.entity.BannerBlockEntity;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.SlabType;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.EmptyLevelChunk;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkDependencies;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.chunk.status.ChunkStep;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;
import net.minecraft.world.level.saveddata.maps.MapBanner;
import net.minecraft.world.level.saveddata.maps.MapId;
import net.minecraft.world.level.saveddata.maps.MapIndex;
import net.minecraft.world.level.saveddata.maps.MapItemSavedData;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;
import sun.misc.Unsafe;

public final class MapOracle {
    private static final Unsafe UNSAFE = loadUnsafe();
    private static final int MIN_CX = -4;
    private static final int MAX_CX = 3;
    private static final int MIN_CZ = -5;
    private static final int MAX_CZ = 3;

    private MapOracle() {}

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
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        if (args.length != 7) {
            throw new IllegalArgumentException(
                    "usage: MapOracle <seed> <in_map0.dat> <in_last_id.dat> <out_map0.dat> <out_last_id.dat> <in_banner_added.dat> <in_banner_removed.dat>");
        }
        long seed = Long.parseLong(args[0]);
        Path inMapPath = Path.of(args[1]);
        Path inIndexPath = Path.of(args[2]);
        Path outMapPath = Path.of(args[3]);
        Path outIndexPath = Path.of(args[4]);
        Path inBannerAddedPath = Path.of(args[5]);
        Path inBannerRemovedPath = Path.of(args[6]);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8))) {
            RegistryAccess.Frozen registries = loadRegistries(resources);
            BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(registries)
                    .forEach(DataComponentInitializers.PendingComponents::apply);
            RegistryOps<Tag> ops = RegistryOps.create(NbtOps.INSTANCE, registries);
            int expectedDataVersion = SharedConstants.getCurrentVersion().dataVersion().version();

            // 1. 生成固定种子 8×9 区块（CARVERS 阶段，真实服务端路径）。
            FakeLevel level = FakeLevel.create(registries);
            Map<Long, ProtoChunk> protos = generateTerrain(registries, seed);
            for (var entry : protos.entrySet()) {
                level.putChunk(entry.getKey(), new LevelChunk(level, entry.getValue(), null));
            }

            MapItem filledMap = (MapItem) Items.FILLED_MAP;
            FakePlayer player = FakePlayer.create(level, "Oracle");

            // 1a. scale=0 纯生成地形 128×128 颜色。
            MapItemSavedData scale0 = MapItemSavedData.createFresh(
                    0.0, 0.0, (byte) 0, true, false, Level.OVERWORLD);
            player.moveToPos(0.5, 80.0, 0.5, 0.0f);
            for (int step = 0; step < 16; step++) {
                filledMap.update(level, player, scale0);
            }

            // 2. 玩家手持地图沿固定轨迹走 20 刻（scale=0）、增量探索 10 刻（scale=1）与锁定地图 6 刻，
            //    收集每刻数据包精确字节。
            JsonArray walkPackets = runWalkScenario(level, registries, filledMap, (byte) 0, false);
            JsonArray scale1WalkPackets = runWalkScenario(level, registries, filledMap, (byte) 1, false);
            JsonArray lockedWalkPackets = runWalkScenario(level, registries, filledMap, (byte) 0, true);

            // 1b. 在区块 (0, 0) 放置 4 根专项测试柱（含水下置石半砖、玻璃盖金块、虚空柱、3 格水），
            //     同时测 scale=1（2×2 众数）和 scale=0 专项像素。
            ProtoChunk p00 = protos.get(ChunkPos.pack(0, 0));
            placeSpecialColumns(p00);
            Heightmap.primeHeightmaps(p00, EnumSet.of(Heightmap.Types.WORLD_SURFACE));
            level.putChunk(ChunkPos.pack(0, 0), new LevelChunk(level, p00, null));

            MapItemSavedData special0 = MapItemSavedData.createFresh(
                    0.0, 0.0, (byte) 0, true, false, Level.OVERWORLD);
            player.moveToPos(0.5, 80.0, 0.5, 0.0f);
            for (int step = 0; step < 16; step++) {
                filledMap.update(level, player, special0);
            }

            MapItemSavedData scale1 = MapItemSavedData.createFresh(
                    0.0, 0.0, (byte) 1, true, false, Level.OVERWORLD);
            player.moveToPos(0.5, 80.0, 0.5, 0.0f);
            for (int step = 0; step < 16; step++) {
                filledMap.update(level, player, scale1);
            }

            // 2b. 第二期场景（放在 scale1 之后，避免修改共享 ProtoChunk 段影响第一期地形对拍）：
            //     右键带名旗帜添加 / 取消标记、打掉旗帜后 checkBanners 消失时机与包、工作台 map_extending。
            JsonObject bannerToggleReport = runBannerToggleScenario(
                    registries, protos, filledMap, ops, expectedDataVersion,
                    inBannerAddedPath, inBannerRemovedPath);
            JsonObject bannerBreakReport = runBannerBreakScenario(registries, protos, filledMap);
            JsonArray mapExtendingReport = runMapExtendingScenario(registries, filledMap);

            // 3. 存档往返：官方 codec 解码 Bamboo 写出的 0.dat 和 last_id.dat，再重新编码写回。
            CompoundTag mapRoot = NbtIo.readCompressed(inMapPath, NbtAccounter.unlimitedHeap());
            int mapDataVersion = mapRoot.getIntOr("DataVersion", -1);
            if (mapDataVersion != expectedDataVersion) {
                throw new IllegalStateException(
                        "map 0.dat DataVersion=" + mapDataVersion + ", expected " + expectedDataVersion);
            }
            Tag mapDataTag = mapRoot.get("data");
            if (mapDataTag == null) {
                throw new IllegalStateException("map 0.dat missing data compound");
            }
            MapItemSavedData decodedMap = MapItemSavedData.CODEC.parse(ops, mapDataTag).getOrThrow();
            Tag reencodedMap = MapItemSavedData.CODEC.encodeStart(ops, decodedMap).getOrThrow();
            CompoundTag outMapRoot = new CompoundTag();
            outMapRoot.putInt("DataVersion", expectedDataVersion);
            outMapRoot.put("data", reencodedMap);
            NbtIo.writeCompressed(outMapRoot, outMapPath);

            CompoundTag idxRoot = NbtIo.readCompressed(inIndexPath, NbtAccounter.unlimitedHeap());
            int idxDataVersion = idxRoot.getIntOr("DataVersion", -1);
            if (idxDataVersion != expectedDataVersion) {
                throw new IllegalStateException(
                        "last_id.dat DataVersion=" + idxDataVersion + ", expected " + expectedDataVersion);
            }
            Tag idxDataTag = idxRoot.get("data");
            if (idxDataTag == null) {
                throw new IllegalStateException("last_id.dat missing data compound");
            }
            MapIndex decodedIdx = MapIndex.CODEC.parse(NbtOps.INSTANCE, idxDataTag).getOrThrow();
            Tag reencodedIdx = MapIndex.CODEC.encodeStart(NbtOps.INSTANCE, decodedIdx).getOrThrow();
            CompoundTag outIdxRoot = new CompoundTag();
            outIdxRoot.putInt("DataVersion", expectedDataVersion);
            outIdxRoot.put("data", reencodedIdx);
            NbtIo.writeCompressed(outIdxRoot, outIndexPath);

            JsonObject report = new JsonObject();
            report.addProperty("data_version", expectedDataVersion);
            report.addProperty("scale0_colors", toHex(scale0.colors));
            report.addProperty("special0_colors", toHex(special0.colors));
            report.addProperty("scale1_colors", toHex(scale1.colors));
            report.add("walk_packets", walkPackets);
            report.add("scale1_walk_packets", scale1WalkPackets);
            report.add("locked_walk_packets", lockedWalkPackets);
            report.add("banner_toggle", bannerToggleReport);
            report.add("banner_break", bannerBreakReport);
            report.add("map_extending", mapExtendingReport);

            JsonObject savedSummary = new JsonObject();
            savedSummary.addProperty("dimension", decodedMap.dimension.identifier().toString());
            savedSummary.addProperty("center_x", decodedMap.centerX);
            savedSummary.addProperty("center_z", decodedMap.centerZ);
            savedSummary.addProperty("scale", decodedMap.scale);
            savedSummary.addProperty("locked", decodedMap.locked);
            savedSummary.addProperty("colors", toHex(decodedMap.colors));
            savedSummary.addProperty("last_map_id_next", decodedIdx.getNextMapId().id());
            report.add("saved", savedSummary);

            out.write(report.toString());
            out.write('\n');
            out.flush();
        }
    }

    private static JsonArray runWalkScenario(
            FakeLevel level, RegistryAccess registries, MapItem filledMap, byte scale, boolean locked)
            throws Exception {
        MapId mapId = new MapId(0);
        MapItemSavedData data = MapItemSavedData.createFresh(
                0.0, 0.0, scale, true, false, Level.OVERWORLD);
        if (locked) {
            data = data.locked();
        }
        level.setMapData(mapId, data);

        FakePlayer player = FakePlayer.create(level, "Oracle");
        ItemStack stack = new ItemStack(Items.FILLED_MAP);
        stack.set(DataComponents.MAP_ID, mapId);
        player.getInventory().setItem(0, stack);

        double[][] steps;
        if (locked) {
            steps = new double[][] {
                {0.5, 0.5, 0.0},
                {4.5, 2.5, 45.0},
                {8.5, 4.5, 90.0},
                {12.5, 6.5, 135.0},
                {16.5, 8.5, 180.0},
                {20.5, 10.5, -90.0},
            };
        } else if (scale == 1) {
            // scale=1（center=(64,64), radius=64 像素 = 128 格）：玩家从西北向东南每刻走 12 格，
            // 逐刻把新的已加载地形纳入半径，触发多个局部脏矩形补丁包。
            steps = new double[][] {
                {-60.5, -60.5, -45.0},
                {-48.5, -48.5, -35.0},
                {-36.5, -36.5, -25.0},
                {-24.5, -24.5, -15.0},
                {-12.5, -12.5, -5.0},
                {-0.5, -0.5, 5.0},
                {11.5, 11.5, 15.0},
                {23.5, 23.5, 25.0},
                {35.5, 35.5, 35.0},
                {47.5, 47.5, 45.0},
            };
        } else {
            steps = new double[][] {
                {-20.5, -15.5, -180.0},
                {-17.3, -13.4, -152.5},
                {-14.1, -11.3, -125.0},
                {-10.9, -9.2, -97.5},
                {-7.7, -7.1, -70.0},
                {-4.5, -5.0, -42.5},
                {-1.3, -2.9, -15.0},
                {1.9, -0.8, 12.5},
                {5.1, 1.3, 40.0},
                {8.3, 3.4, 67.5},
                {11.5, 5.5, 95.0},
                {14.7, 7.6, 122.5},
                {17.9, 9.7, 150.0},
                {21.1, 11.8, 177.5},
                {24.3, 13.9, 205.0},
                {90.5, 10.5, 45.0},
                {95.5, 12.5, 135.0},
                {420.5, 10.5, 90.0},
                {12.5, -10.5, 180.0},
                {14.0, -8.0, 0.0},
            };
        }

        JsonArray out = new JsonArray();
        for (int t = 0; t < steps.length; t++) {
            level.gameTime = t + 1;
            player.moveToPos(steps[t][0], 80.0, steps[t][1], (float) steps[t][2]);
            filledMap.inventoryTick(stack, level, player, EquipmentSlot.MAINHAND);
            Packet<?> packet = data.getUpdatePacket(mapId, player);
            if (packet == null) {
                out.add(JsonNull.INSTANCE);
            } else {
                ClientboundMapItemDataPacket mapPkt = (ClientboundMapItemDataPacket) packet;
                RegistryFriendlyByteBuf buf = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
                ClientboundMapItemDataPacket.STREAM_CODEC.encode(buf, mapPkt);
                byte[] bytes = new byte[buf.readableBytes()];
                buf.readBytes(bytes);
                out.add(toHex(bytes));
            }
        }
        return out;
    }

    private static String encodePacket(RegistryAccess registries, Packet<?> packet) {
        ClientboundMapItemDataPacket mapPkt = (ClientboundMapItemDataPacket) packet;
        RegistryFriendlyByteBuf buf = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
        ClientboundMapItemDataPacket.STREAM_CODEC.encode(buf, mapPkt);
        byte[] bytes = new byte[buf.readableBytes()];
        buf.readBytes(bytes);
        return toHex(bytes);
    }

    private static void addPacket(JsonArray arr, RegistryAccess registries, Packet<?> packet) {
        if (packet == null) {
            arr.add(JsonNull.INSTANCE);
        } else {
            arr.add(encodePacket(registries, packet));
        }
    }

    private static BlockState blockByName(String name) {
        return BuiltInRegistries.BLOCK.getValue(Identifier.withDefaultNamespace(name)).defaultBlockState();
    }

    private static String encodeItemStack(RegistryAccess registries, ItemStack stack) {
        RegistryFriendlyByteBuf buf = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
        ItemStack.OPTIONAL_STREAM_CODEC.encode(buf, stack);
        byte[] bytes = new byte[buf.readableBytes()];
        buf.readBytes(bytes);
        return toHex(bytes);
    }

    private static JsonArray bannersToJson(MapItemSavedData data) {
        JsonArray arr = new JsonArray();
        List<MapBanner> sorted = new ArrayList<>(data.getBanners());
        sorted.sort(java.util.Comparator.comparing(MapBanner::getId));
        for (MapBanner b : sorted) {
            JsonObject o = new JsonObject();
            o.addProperty("x", b.pos().getX());
            o.addProperty("y", b.pos().getY());
            o.addProperty("z", b.pos().getZ());
            o.addProperty("color", b.color().getSerializedName());
            if (b.name().isPresent()) {
                o.addProperty("name", b.name().get().getString());
            } else {
                o.add("name", JsonNull.INSTANCE);
            }
            arr.add(o);
        }
        return arr;
    }

    private static MapItemSavedData decodeSavedMap(
            Path path, RegistryOps<Tag> ops, int expectedDataVersion) throws Exception {
        CompoundTag root = NbtIo.readCompressed(path, NbtAccounter.unlimitedHeap());
        int dv = root.getIntOr("DataVersion", -1);
        if (dv != expectedDataVersion) {
            throw new IllegalStateException(
                    path + " DataVersion=" + dv + ", expected " + expectedDataVersion);
        }
        Tag dataTag = root.get("data");
        if (dataTag == null) {
            throw new IllegalStateException(path + " missing data compound");
        }
        return MapItemSavedData.CODEC.parse(ops, dataTag).getOrThrow();
    }

    /** 复制一份 ProtoChunk 的方块段与高度图，避免改到与其它场景共用的生成结果。 */
    private static ProtoChunk copyProto(
            RegistryAccess.Frozen registries, ProtoChunk source) {
        PalettedContainerFactory factory = PalettedContainerFactory.create(registries);
        ProtoChunk copy = new ProtoChunk(
                source.getPos(), UpgradeData.EMPTY, source.getHeightAccessorForGeneration(),
                factory, null);
        LevelChunkSection[] sections = source.getSections();
        LevelChunkSection[] target = copy.getSections();
        for (int index = 0; index < sections.length; index++) {
            LevelChunkSection from = sections[index];
            LevelChunkSection to = target[index];
            for (int y = 0; y < 16; y++) {
                for (int x = 0; x < 16; x++) {
                    for (int z = 0; z < 16; z++) {
                        to.setBlockState(x, y, z, from.getBlockState(x, y, z));
                    }
                }
            }
            to.fillBiomesFromNoise(
                    (qx, qy, qz) -> from.getNoiseBiome(qx, qy, qz), 0, 0, 0);
        }
        copy.setPersistedStatus(source.getPersistedStatus());
        Heightmap.primeHeightmaps(copy, EnumSet.of(Heightmap.Types.WORLD_SURFACE));
        return copy;
    }

    private static FakeLevel buildBannerLevel(
            RegistryAccess.Frozen registries,
            Map<Long, ProtoChunk> protos,
            BlockPos bannerPos,
            BlockState bannerState,
            String customName) throws Exception {
        FakeLevel level = FakeLevel.create(registries);
        long targetKey = ChunkPos.pack(bannerPos.getX() >> 4, bannerPos.getZ() >> 4);
        for (var entry : protos.entrySet()) {
            ProtoChunk source = entry.getValue();
            ProtoChunk proto = entry.getKey() == targetKey ? copyProto(registries, source) : source;
            if (entry.getKey() == targetKey) {
                // 在 ProtoChunk 上放旗帜，避开 `LevelChunk.setBlockState` 对
                // `Level.getChunkSource().getLightEngine()` 的依赖（FakeLevel 没有 ChunkSource）。
                proto.setBlockState(bannerPos.below(), Blocks.STONE.defaultBlockState(), 0);
                proto.setBlockState(bannerPos, bannerState, 0);
                for (int y = bannerPos.getY() + 1; y < 320; y++) {
                    proto.setBlockState(
                            new BlockPos(bannerPos.getX(), y, bannerPos.getZ()),
                            Blocks.AIR.defaultBlockState(), 0);
                }
                Heightmap.primeHeightmaps(proto, EnumSet.of(Heightmap.Types.WORLD_SURFACE));
                BannerBlockEntity be = new BannerBlockEntity(bannerPos, bannerState);
                if (customName != null) {
                    setField(BannerBlockEntity.class, be, "name", Component.literal(customName));
                }
                proto.setBlockEntity(be);
                level.putBlockEntity(bannerPos, be);
            }
            level.putChunk(entry.getKey(), new LevelChunk(level, proto, null));
        }
        return level;
    }

    private static JsonObject runBannerToggleScenario(
            RegistryAccess.Frozen registries,
            Map<Long, ProtoChunk> protos,
            MapItem filledMap,
            RegistryOps<Tag> ops,
            int expectedDataVersion,
            Path inBannerAddedPath,
            Path inBannerRemovedPath) throws Exception {
        BlockPos bannerPos = new BlockPos(5, 120, -3);
        FakeLevel level = buildBannerLevel(
                registries, protos, bannerPos, blockByName("red_banner"), "Camp Alpha");
        MapId mapId = new MapId(0);
        MapItemSavedData data = MapItemSavedData.createFresh(
                0.0, 0.0, (byte) 0, true, false, Level.OVERWORLD);
        level.setMapData(mapId, data);

        FakePlayer player = FakePlayer.create(level, "Oracle");
        ItemStack stack = new ItemStack(Items.FILLED_MAP);
        stack.set(DataComponents.MAP_ID, mapId);
        player.getInventory().setItem(0, stack);
        player.moveToPos(0.5, 80.0, 0.5, 0.0f);

        JsonArray packets = new JsonArray();
        // Tick 0: 初始同步。
        level.gameTime = 1;
        filledMap.inventoryTick(stack, level, player, EquipmentSlot.MAINHAND);
        addPacket(packets, registries, data.getUpdatePacket(mapId, player));

        // 第一次右键旗帜：添加红色带名旗帜标记。
        BlockHitResult hit = new BlockHitResult(
                new Vec3(5.5, 120.5, -2.5), Direction.UP, bannerPos, false);
        InteractionResult r1 = filledMap.useOn(new UseOnContext(player, InteractionHand.MAIN_HAND, hit));
        JsonArray vanillaAfterAdd = bannersToJson(data);
        for (int i = 0; i < 5; i++) {
            level.gameTime = 2 + i;
            filledMap.inventoryTick(stack, level, player, EquipmentSlot.MAINHAND);
            addPacket(packets, registries, data.getUpdatePacket(mapId, player));
        }

        // 第二次右键同一面旗帜：取消标记。
        InteractionResult r2 = filledMap.useOn(new UseOnContext(player, InteractionHand.MAIN_HAND, hit));
        JsonArray vanillaAfterRemove = bannersToJson(data);
        for (int i = 0; i < 5; i++) {
            level.gameTime = 7 + i;
            filledMap.inventoryTick(stack, level, player, EquipmentSlot.MAINHAND);
            addPacket(packets, registries, data.getUpdatePacket(mapId, player));
        }

        // 用官方 CODEC 解码 Bamboo 写出的添加后 / 取消后存档，核对 banners 列表。
        MapItemSavedData bambooAdded = decodeSavedMap(inBannerAddedPath, ops, expectedDataVersion);
        MapItemSavedData bambooRemoved = decodeSavedMap(inBannerRemovedPath, ops, expectedDataVersion);

        JsonObject obj = new JsonObject();
        obj.addProperty("add_success", r1.consumesAction());
        obj.addProperty("remove_success", r2.consumesAction());
        obj.add("packets", packets);
        obj.add("vanilla_after_add", vanillaAfterAdd);
        obj.add("vanilla_after_remove", vanillaAfterRemove);
        obj.add("bamboo_saved_after_add", bannersToJson(bambooAdded));
        obj.add("bamboo_saved_after_remove", bannersToJson(bambooRemoved));
        return obj;
    }

    private static JsonObject runBannerBreakScenario(
            RegistryAccess.Frozen registries,
            Map<Long, ProtoChunk> protos,
            MapItem filledMap) throws Exception {
        BlockPos bannerPos = new BlockPos(5, 120, -3);
        FakeLevel level = buildBannerLevel(
                registries, protos, bannerPos, blockByName("blue_wall_banner"), "Watchtower");
        MapId mapId = new MapId(0);
        MapItemSavedData data = MapItemSavedData.createFresh(
                0.0, 0.0, (byte) 0, true, false, Level.OVERWORLD);
        level.setMapData(mapId, data);

        FakePlayer player = FakePlayer.create(level, "Oracle");
        ItemStack stack = new ItemStack(Items.FILLED_MAP);
        stack.set(DataComponents.MAP_ID, mapId);
        player.getInventory().setItem(0, stack);
        player.moveToPos(0.5, 80.0, 0.5, 0.0f);

        long tick = 0;
        // 先跑 16 刻探索完 128×128，避免后续颜色变化级联干扰 checkBanners 精确列扫时机。
        for (int i = 0; i < 16; i++) {
            level.gameTime = ++tick;
            filledMap.inventoryTick(stack, level, player, EquipmentSlot.MAINHAND);
            data.getUpdatePacket(mapId, player);
        }

        // 右键添加蓝色墙旗帜标记，再跑 5 刻把标记包送出并清脏。
        BlockHitResult hit = new BlockHitResult(
                new Vec3(5.5, 120.5, -2.5), Direction.UP, bannerPos, false);
        filledMap.useOn(new UseOnContext(player, InteractionHand.MAIN_HAND, hit));
        for (int i = 0; i < 5; i++) {
            level.gameTime = ++tick;
            filledMap.inventoryTick(stack, level, player, EquipmentSlot.MAINHAND);
            data.getUpdatePacket(mapId, player);
        }

        // 打掉旗帜（方块置 AIR，移除方块实体）。直接改 LevelChunk 段，避免
        // `LevelChunk.setBlockState` 对 `getChunkSource().getLightEngine()` 的依赖。
        LevelChunk targetChunk = level.getChunk(bannerPos.getX() >> 4, bannerPos.getZ() >> 4);
        int sectionIndex = targetChunk.getSectionIndex(bannerPos.getY());
        LevelChunkSection section = targetChunk.getSection(sectionIndex);
        section.setBlockState(
                SectionPos.sectionRelative(bannerPos.getX()),
                SectionPos.sectionRelative(bannerPos.getY()),
                SectionPos.sectionRelative(bannerPos.getZ()),
                Blocks.AIR.defaultBlockState());
        Heightmap.primeHeightmaps(targetChunk, EnumSet.of(Heightmap.Types.WORLD_SURFACE));
        level.removeBlockEntity(bannerPos);

        // 连续跑 20 刻，记录每刻的旗帜剩余数和发出的地图包精确字节。
        JsonArray bannerCounts = new JsonArray();
        JsonArray packets = new JsonArray();
        for (int i = 0; i < 20; i++) {
            level.gameTime = ++tick;
            filledMap.inventoryTick(stack, level, player, EquipmentSlot.MAINHAND);
            bannerCounts.add(data.getBanners().size());
            addPacket(packets, registries, data.getUpdatePacket(mapId, player));
        }

        JsonObject obj = new JsonObject();
        obj.add("banner_counts", bannerCounts);
        obj.add("packets", packets);
        return obj;
    }

    private static JsonArray runMapExtendingScenario(
            RegistryAccess.Frozen registries, MapItem filledMap) throws Exception {
        FakeLevel level = FakeLevel.create(registries);
        level.setMapData(new MapId(0),
                MapItemSavedData.createFresh(0.0, 0.0, (byte) 0, true, false, Level.OVERWORLD));
        level.setMapData(new MapId(1),
                MapItemSavedData.createFresh(0.0, 0.0, (byte) 3, true, false, Level.OVERWORLD));
        level.setMapData(new MapId(2),
                MapItemSavedData.createFresh(0.0, 0.0, (byte) 4, true, false, Level.OVERWORLD));
        level.setMapData(new MapId(3),
                MapItemSavedData.createFresh(0.0, 0.0, (byte) 0, true, false, Level.OVERWORLD).locked());
        level.nextMapId = 4;

        MapExtendingRecipe recipe = new MapExtendingRecipe(
                Ingredient.of(registries.lookupOrThrow(Registries.ITEM)
                        .getOrThrow(ItemTags.EXTENDABLE_MAPS)),
                Ingredient.of(Items.PAPER),
                TransmuteResult.KEEP_INPUT_ITEM);

        List<ItemStack> centers = new ArrayList<>();
        ItemStack map0 = new ItemStack(Items.FILLED_MAP);
        map0.set(DataComponents.MAP_ID, new MapId(0));
        map0.set(DataComponents.CUSTOM_NAME, Component.literal("Expedition"));
        centers.add(map0);

        ItemStack map1 = new ItemStack(Items.FILLED_MAP);
        map1.set(DataComponents.MAP_ID, new MapId(1));
        centers.add(map1);

        ItemStack map2 = new ItemStack(Items.FILLED_MAP);
        map2.set(DataComponents.MAP_ID, new MapId(2));
        centers.add(map2);

        ItemStack map3 = new ItemStack(Items.FILLED_MAP);
        map3.set(DataComponents.MAP_ID, new MapId(3));
        centers.add(map3);

        centers.add(new ItemStack(Items.FILLED_MAP)); // 无 MAP_ID

        JsonArray out = new JsonArray();
        for (ItemStack center : centers) {
            List<ItemStack> grid = new ArrayList<>(9);
            for (int i = 0; i < 9; i++) {
                grid.add(i == 4 ? center.copy() : new ItemStack(Items.PAPER));
            }
            CraftingInput input = CraftingInput.of(3, 3, grid);
            boolean matched = recipe.matches(input, level);
            JsonObject entry = new JsonObject();
            entry.addProperty("matched", matched);
            if (matched) {
                ItemStack preview = recipe.assemble(input);
                entry.addProperty("preview_hex", encodeItemStack(registries, preview));
                ItemStack crafted = preview.copy();
                filledMap.onCraftedPostProcess(crafted, level);
                entry.addProperty("crafted_hex", encodeItemStack(registries, crafted));
                MapId newId = crafted.get(DataComponents.MAP_ID);
                MapItemSavedData newData = newId == null ? null : level.getMapData(newId);
                if (newId != null && newData != null) {
                    entry.addProperty("new_map_id", newId.id());
                    entry.addProperty("new_scale", newData.scale);
                    entry.addProperty("new_center_x", newData.centerX);
                    entry.addProperty("new_center_z", newData.centerZ);
                    entry.addProperty("new_locked", newData.locked);
                }
            }
            out.add(entry);
        }
        return out;
    }

    private static void placeSpecialColumns(ProtoChunk chunk) {
        // (0, 120, 0): 含水下置石半砖（顶面非完整支撑面 → getCorrectStateForFluidBlock 取水色）
        chunk.setBlockState(new BlockPos(0, 119, 0), Blocks.STONE.defaultBlockState(), 0);
        BlockState waterloggedSlab = Blocks.STONE_SLAB.defaultBlockState()
                .setValue(SlabBlock.TYPE, SlabType.BOTTOM)
                .setValue(SlabBlock.WATERLOGGED, true);
        chunk.setBlockState(new BlockPos(0, 120, 0), waterloggedSlab, 0);
        for (int y = 121; y < 320; y++) {
            chunk.setBlockState(new BlockPos(0, y, 0), Blocks.AIR.defaultBlockState(), 0);
        }

        // (1, 120, 0): 玻璃（MapColor.NONE）盖在金块上 → 向下跳过玻璃取金块色
        chunk.setBlockState(new BlockPos(1, 119, 0), Blocks.GOLD_BLOCK.defaultBlockState(), 0);
        chunk.setBlockState(new BlockPos(1, 120, 0), Blocks.GLASS.defaultBlockState(), 0);
        for (int y = 121; y < 320; y++) {
            chunk.setBlockState(new BlockPos(1, y, 0), Blocks.AIR.defaultBlockState(), 0);
        }

        // (2, *, 0): 整列空气 → y <= minY 兜底基岩
        for (int y = -64; y < 320; y++) {
            chunk.setBlockState(new BlockPos(2, y, 0), Blocks.AIR.defaultBlockState(), 0);
        }

        // (3, 120..=122, 0): 3 格水压在石头上
        chunk.setBlockState(new BlockPos(3, 119, 0), Blocks.STONE.defaultBlockState(), 0);
        for (int y = 120; y <= 122; y++) {
            chunk.setBlockState(new BlockPos(3, y, 0), Blocks.WATER.defaultBlockState(), 0);
        }
        for (int y = 123; y < 320; y++) {
            chunk.setBlockState(new BlockPos(3, y, 0), Blocks.AIR.defaultBlockState(), 0);
        }
    }

    private static Map<Long, ProtoChunk> generateTerrain(
            RegistryAccess.Frozen registries, long seed) throws Exception {
        var noiseLookup = registries.lookupOrThrow(Registries.NOISE);
        Registry<NoiseGeneratorSettings> settingsReg =
                registries.lookupOrThrow(Registries.NOISE_SETTINGS);
        NoiseGeneratorSettings gs = settingsReg.getValueOrThrow(NoiseGeneratorSettings.OVERWORLD);
        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(gs));
        RandomState state = RandomState.create(noiseLookup, seed, gs);
        BiomeResolver resolver = ((BiomeSource) generator.getBiomeSource()).createUncachedResolver(state);

        Method doFill = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                "doFill", NoiseChunk.class, ChunkAccess.class);
        doFill.setAccessible(true);
        Method buildSurface = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                "buildSurface", ChunkAccess.class, NoiseChunk.class, RandomState.class,
                BiomeManager.class, Set.class, MaterialRule.class);
        buildSurface.setAccessible(true);
        Method generateCarvers = NoiseBasedChunkGenerator.class.getDeclaredMethod(
                "generateCarvers", ChunkAccess.class, Blender.class, NoiseChunk.class,
                RandomState.class, BiomeManager.class, WorldGenRegion.class, MaterialRule.class);
        generateCarvers.setAccessible(true);
        Field pickerField = NoiseBasedChunkGenerator.class.getDeclaredField("globalFluidPicker");
        pickerField.setAccessible(true);
        @SuppressWarnings("unchecked")
        Supplier<Aquifer.FluidPicker> picker = (Supplier<Aquifer.FluidPicker>) pickerField.get(generator);

        LevelHeightAccessor dims = LevelHeightAccessor.create(
                gs.noiseSettings().minY(), gs.noiseSettings().height());
        PalettedContainerFactory factory = PalettedContainerFactory.create(registries);
        MaterialRule materialRule = gs.materialRule().value();

        // 先填 (MIN_CX-1..MAX_CX+1) × (MIN_CZ-1..MAX_CZ+1) 的群系，供 3×3 possibleBiomes 复用。
        Map<Long, ProtoChunk> biomeChunks = new HashMap<>();
        for (int cx = MIN_CX - 1; cx <= MAX_CX + 1; cx++) {
            for (int cz = MIN_CZ - 1; cz <= MAX_CZ + 1; cz++) {
                ChunkPos pos = new ChunkPos(cx, cz);
                ProtoChunk p = new ProtoChunk(pos, UpgradeData.EMPTY, dims, factory, null);
                generator.createBiomes(state, Blender.empty(), null, p).join();
                p.setPersistedStatus(ChunkStatus.BIOMES);
                biomeChunks.put(pos.pack(), p);
            }
        }

        // 跑每个目标区块的 CARVERS + prime FINAL_HEIGHTMAPS。
        Map<Long, ProtoChunk> result = new HashMap<>();
        for (int cx = MIN_CX; cx <= MAX_CX; cx++) {
            for (int cz = MIN_CZ; cz <= MAX_CZ; cz++) {
                ChunkPos pos = new ChunkPos(cx, cz);
                ProtoChunk center = biomeChunks.get(pos.pack());
                ProtoChunk[][] neighbors = new ProtoChunk[17][17];
                for (int dx = -8; dx <= 8; dx++) {
                    for (int dz = -8; dz <= 8; dz++) {
                        ProtoChunk cached = biomeChunks.get(ChunkPos.pack(cx + dx, cz + dz));
                        if (cached != null) {
                            neighbors[dx + 8][dz + 8] = cached;
                        } else {
                            ProtoChunk empty = new ProtoChunk(
                                    new ChunkPos(cx + dx, cz + dz), UpgradeData.EMPTY, dims, factory, null);
                            empty.setPersistedStatus(ChunkStatus.BIOMES);
                            neighbors[dx + 8][dz + 8] = empty;
                        }
                    }
                }
                WorldGenRegion region = makeRegion(center, neighbors, pos, seed, resolver);
                Set<Holder<Biome>> possibleBiomes = new HashSet<>();
                for (int dz = -1; dz <= 1; dz++) {
                    for (int dx = -1; dx <= 1; dx++) {
                        region.getChunk(cx + dx, cz + dz).collectBiomesInPalette(possibleBiomes);
                    }
                }
                DensityVolume volume = new DensityVolume(
                        16, gs.noiseSettings().height(), 16,
                        pos.getMinBlockX(), gs.noiseSettings().minY(), pos.getMinBlockZ());
                try (NoiseChunk noiseChunk = new NoiseChunk(
                        state, Beardifier.EMPTY, gs, picker.get(), Blender.empty(), volume)) {
                    doFill.invoke(generator, noiseChunk, center);
                    buildSurface.invoke(generator, center, noiseChunk, state,
                            region.getBiomeManager(), possibleBiomes, materialRule);
                    generateCarvers.invoke(generator, center, Blender.empty(), noiseChunk,
                            state, region.getBiomeManager(), region, materialRule);
                }
                center.setPersistedStatus(ChunkStatus.TERRAIN);
                Heightmap.primeHeightmaps(center, ChunkStatus.FINAL_HEIGHTMAPS);
                result.put(pos.pack(), center);
            }
        }
        return result;
    }

    private static WorldGenRegion makeRegion(
            ProtoChunk center, ProtoChunk[][] neighbors, ChunkPos pos, long seed,
            BiomeResolver resolver) throws Exception {
        WorldGenRegion region = (WorldGenRegion) UNSAFE.allocateInstance(WorldGenRegion.class);
        StaticCache2D<ChunkAccess> cache = StaticCache2D.create(pos.x(), pos.z(), 8,
                (x, z) -> neighbors[x - pos.x() + 8][z - pos.z() + 8]);
        List<ChunkStatus> deps = new ArrayList<>();
        deps.add(ChunkStatus.BIOMES);
        deps.add(ChunkStatus.BIOMES);
        for (int i = 2; i < 9; i++) {
            deps.add(ChunkStatus.STRUCTURE_STARTS);
        }
        ChunkDependencies direct = new ChunkDependencies(
                com.google.common.collect.ImmutableList.copyOf(deps));
        ChunkStep step = new ChunkStep(ChunkStatus.TERRAIN, direct, direct, 1, null);
        setField(WorldGenRegion.class, region, "cache", cache);
        setField(WorldGenRegion.class, region, "center", center);
        setField(WorldGenRegion.class, region, "generatingStep", step);
        setField(WorldGenRegion.class, region, "uncachedBiomeResolver", resolver);
        setField(WorldGenRegion.class, region, "biomeManager",
                new BiomeManager(region, BiomeManager.obfuscateSeed(seed)));
        return region;
    }

    private static RegistryAccess.Frozen loadRegistries(CloseableResourceManager resources) {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
    }

    private static String toHex(byte[] bytes) {
        char[] hex = new char[bytes.length * 2];
        final char[] digits = "0123456789abcdef".toCharArray();
        for (int i = 0; i < bytes.length; i++) {
            int v = bytes[i] & 0xff;
            hex[i * 2] = digits[v >>> 4];
            hex[i * 2 + 1] = digits[v & 0x0f];
        }
        return new String(hex);
    }

    private static Unsafe loadUnsafe() {
        try {
            Field f = Unsafe.class.getDeclaredField("theUnsafe");
            f.setAccessible(true);
            return (Unsafe) f.get(null);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
    }

    private static void setField(Class<?> owner, Object target, String name, Object value) {
        try {
            Field f = owner.getDeclaredField(name);
            f.setAccessible(true);
            f.set(target, value);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("failed to set " + owner.getName() + "." + name, e);
        }
    }

    private static final class FakeLevel extends ServerLevel {
        private RegistryAccess.Frozen registries;
        private Map<Long, LevelChunk> chunks;
        private Map<MapId, MapItemSavedData> maps;
        private Map<BlockPos, BlockEntity> blockEntities;
        private EmptyLevelChunk emptyChunk;
        long gameTime;
        int nextMapId;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        static FakeLevel create(RegistryAccess.Frozen registries) throws Exception {
            FakeLevel level = (FakeLevel) UNSAFE.allocateInstance(FakeLevel.class);
            level.registries = registries;
            level.chunks = new HashMap<>();
            level.maps = new HashMap<>();
            level.blockEntities = new HashMap<>();
            Holder<DimensionType> overworld = registries
                    .lookupOrThrow(Registries.DIMENSION_TYPE)
                    .getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            setField(Level.class, level, "dimension", Level.OVERWORLD);
            setField(Level.class, level, "dimensionTypeRegistration", overworld);
            setField(Level.class, level, "isClientSide", false);
            setField(Level.class, level, "thread", Thread.currentThread());
            setField(Level.class, level, "palettedContainerFactory",
                    PalettedContainerFactory.create(registries));
            Holder<Biome> plains = registries.lookupOrThrow(Registries.BIOME)
                    .getOrThrow(Biomes.PLAINS);
            level.emptyChunk = new EmptyLevelChunk(level, new ChunkPos(0, 0), plains);
            return level;
        }

        void putChunk(long packedPos, LevelChunk chunk) {
            chunks.put(packedPos, chunk);
        }

        void putBlockEntity(BlockPos pos, BlockEntity be) {
            blockEntities.put(pos.immutable(), be);
        }

        @Override
        public void removeBlockEntity(BlockPos pos) {
            blockEntities.remove(pos);
        }

        @Override
        public BlockEntity getBlockEntity(BlockPos pos) {
            return blockEntities.get(pos);
        }

        @Override
        public MapId getFreeMapId() {
            return new MapId(nextMapId++);
        }

        @Override
        public LevelChunk getChunk(int cx, int cz) {
            LevelChunk found = chunks.get(ChunkPos.pack(cx, cz));
            return found != null ? found : emptyChunk;
        }

        @Override
        public MapItemSavedData getMapData(MapId mapId) {
            return maps.get(mapId);
        }

        @Override
        public void setMapData(MapId mapId, MapItemSavedData data) {
            maps.put(mapId, data);
        }

        @Override
        public RegistryAccess registryAccess() {
            return registries;
        }

        @Override
        public long getGameTime() {
            return gameTime;
        }
    }

    private static final class FakePlayer extends ServerPlayer {
        private FakePlayer() {
            super(null, null, null, null);
        }

        static FakePlayer create(FakeLevel level, String name) throws Exception {
            FakePlayer player = (FakePlayer) UNSAFE.allocateInstance(FakePlayer.class);
            player.setId(1);
            setField(Entity.class, player, "level", level);
            setField(Entity.class, player, "position", new Vec3(0.5, 80.0, 0.5));
            setField(Entity.class, player, "blockPosition", BlockPos.ZERO);
            setField(Entity.class, player, "chunkPosition", ChunkPos.ZERO);
            PlayerEquipment equipment = new PlayerEquipment(player);
            setField(net.minecraft.world.entity.LivingEntity.class, player, "equipment", equipment);
            setField(Player.class, player, "inventory", new Inventory(player, equipment));
            setField(Player.class, player, "gameProfile",
                    new GameProfile(UUID.nameUUIDFromBytes(new byte[] {1}), name));
            return player;
        }

        void moveToPos(double x, double y, double z, float yaw) {
            setField(Entity.class, this, "position", new Vec3(x, y, z));
            this.setYRot(yaw);
        }
    }
}
