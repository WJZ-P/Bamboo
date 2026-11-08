import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.datafixers.util.Pair;
import com.mojang.serialization.Codec;
import com.mojang.serialization.JsonOps;
import io.netty.buffer.Unpooled;
import java.io.IOException;
import java.io.Reader;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.advancements.predicates.MinMaxBounds;
import net.minecraft.core.component.predicates.PotionsPredicate;
import net.minecraft.world.item.component.FireworkExplosion;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.Vec3i;
import net.minecraft.core.component.BlockTransformer;
import net.minecraft.core.component.DataComponentPatch;
import net.minecraft.core.component.TypedDataComponent;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.resources.RegistryValidator;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.KnownPack;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.Resource;
import net.minecraft.server.packs.resources.ResourceManager;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.BlockTags;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.tags.TagNetworkSerialization;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.EquipmentSlotGroup;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.ai.attributes.Attribute;
import net.minecraft.world.entity.ai.attributes.AttributeModifier;
import net.minecraft.world.entity.animal.axolotl.Axolotl;
import net.minecraft.world.entity.animal.cow.MushroomCow;
import net.minecraft.world.entity.animal.equine.Llama;
import net.minecraft.world.entity.animal.fish.Salmon;
import net.minecraft.world.entity.animal.fish.TropicalFish;
import net.minecraft.world.entity.animal.fox.Fox;
import net.minecraft.world.entity.animal.parrot.Parrot;
import net.minecraft.world.entity.animal.rabbit.Rabbit;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.DyeColor;
import net.minecraft.world.item.HoneycombItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.SolidBucketItem;
import net.minecraft.world.item.ItemUseAnimation;
import net.minecraft.world.item.Rarity;
import net.minecraft.world.item.StandingAndWallBlockItem;
import net.minecraft.world.item.SwingAnimationType;
import net.minecraft.world.item.component.FireworkExplosion;
import net.minecraft.world.item.component.ItemAttributeModifiers;
import net.minecraft.world.item.component.MapPostProcessing;
import net.minecraft.world.item.crafting.Ingredient;
import net.minecraft.world.item.alchemy.Potion;
import net.minecraft.world.item.ItemStackTemplate;
import net.minecraft.world.item.crafting.*;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.ItemLike;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.BucketPickup;
import net.minecraft.world.level.block.SoundType;
import net.minecraft.world.level.block.CandleCakeBlock;
import net.minecraft.world.level.block.FireBlock;
import net.minecraft.world.level.block.FlowerPotBlock;
import net.minecraft.world.level.block.PitcherCropBlock;
import net.minecraft.world.level.block.SupportType;
import net.minecraft.world.level.block.WeatheringCopper;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockSetType;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.block.state.properties.NoteBlockInstrument;
import net.minecraft.world.level.block.state.properties.BooleanProperty;
import net.minecraft.world.level.block.state.properties.EnumProperty;
import net.minecraft.world.level.block.state.properties.IntegerProperty;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.block.state.properties.WoodType;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.densityfunction.DensityFunction;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.levelgen.blockpredicates.AllOfPredicate;
import net.minecraft.world.level.lighting.LightEngine;
import net.minecraft.world.level.levelgen.blockpredicates.BlockPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.CombiningPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.MatchingBlockTagPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.MatchingBlocksPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.StateTestingPredicate;
import net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.CopyPropertiesProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.RuleBasedStateProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.SimpleStateProvider;
import net.minecraft.world.level.lighting.LightEngine;
import net.minecraft.world.level.material.FlowingFluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.CollisionContext;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;

/**
 * 调用官方代码导出 Bamboo 需要的版本数据，写成一个 JSON 文件。
 *
 * <p>注册表和标签的数字 ID 取决于官方的加载顺序，所以这里直接走官方的加载流程，不自己解析数据文件。
 * 这里只读取数据，不启动服务端。
 */
public final class Extractor {
    /** 读取事实时用的世界：什么都没有，所以读到的是方块本身的性质。 */
    private static final EmptyBlockGetter EMPTY = EmptyBlockGetter.INSTANCE;
    /** 用来发现结果依赖位置的方块。第一个位置是所有表读取的基准。 */
    private static final List<BlockPos> SAMPLE_POSITIONS = List.of(
            BlockPos.ZERO,
            new BlockPos(1, 0, 1),
            new BlockPos(5, 64, -3),
            new BlockPos(-77, 12, 29),
            new BlockPos(1000, -30, -2000));
    /** 这些位的含义写在 mc-bamboo-data 的 lib.rs 里，两边要一起改。 */
    private static final long FACT_AIR = 1L << 18;
    private static final long FACT_CAN_OCCLUDE = 1L << 19;
    private static final long FACT_SOLID = 1L << 20;
    private static final long FACT_LIQUID = 1L << 21;
    private static final long FACT_REPLACEABLE = 1L << 22;
    private static final long FACT_REDSTONE_CONDUCTOR = 1L << 23;
    private static final long FACT_COLLISION_FULL_BLOCK = 1L << 24;
    private static final long FACT_SOLID_RENDER = 1L << 25;
    private static final long FACT_SIGNAL_SOURCE = 1L << 26;
    private static final long FACT_ANALOG_OUTPUT = 1L << 27;
    private static final long FACT_PROPAGATES_SKYLIGHT = 1L << 28;
    private static final long FACT_RANDOMLY_TICKING = 1L << 29;
    private static final long FACT_FLUID_RANDOMLY_TICKING = 1L << 30;
    private static final long FACT_BLOCKS_LIGHT_FROM_BELOW = 1L << 31;
    private static final long FACT_USE_SHAPE_FOR_LIGHT_OCCLUSION = 1L << 32;
    private static final long FACT_MOTION_BLOCKING = 1L << 33;
    private static final long FACT_MOTION_BLOCKING_NO_LEAVES = 1L << 34;
    private static final int LIGHT_BLOCKED = 16;
    private static final int STURDY_MASK = (1 << 18) - 1;
    /** loadRegistries 加载的静态注册表标签，itemComponents 再加载服务端资源时还要用。 */
    private static List<Registry.PendingTags<?>> STATIC_TAGS = List.of();

    public static void main(String[] args) throws Exception {
        Path output = Path.of(args[0]);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));

        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            LayeredRegistryAccess<RegistryLayer> layers = loadRegistries(resources);

            JsonObject root = new JsonObject();
            root.add("version", version());
            root.add("knownPacks", knownPacks(resources));
            root.add("enabledFeatures", enabledFeatures());
            root.add("syncedRegistries", syncedRegistries(layers));
            root.add("tags", tags(layers));
            root.add("blockStates", blockStates());
            root.add("blocks", blocks());
            root.add("items", items());
            root.add("stateTables", new StateTables().build());
            root.add("lightTables", new LightTables().build());
            root.add("blockMaps", blockMaps(resources, layers.compositeAccess()));
            root.add("blockTransformers", blockTransformers(layers.compositeAccess()));
            root.add("recipes", recipes(layers.compositeAccess()));
            root.add("heightmaps", heightmaps());
            root.add("soundTables", new SoundTables().build());
            root.add("attributeDefaults", attributeDefaults());
            root.add("componentEnums", componentEnums());
            root.add("worldgen", worldgen(layers.compositeAccess()));
            // 这一步会加载服务端资源，物品的默认组件要绑定上去才读得到，放在最后。
            root.add("itemComponents", itemComponents(resources, layers));

            Files.writeString(output, new GsonBuilder().create().toJson(root));
        }

        // 官方的后台线程不是守护线程，不主动退出进程就不会结束。
        System.exit(0);
    }

    /** 和 WorldLoader.load 的前半段相同：加载静态注册表的标签，再加载世界层注册表。 */
    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(CloseableResourceManager resources) {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        STATIC_TAGS = staticTags;
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        List<HolderLookup.RegistryLookup<?>> recipeContext = new ArrayList<>(lookups);
        world.listRegistries().forEach(recipeContext::add);
        RegistryAccess.Frozen recipes = RegistryDataLoader
                .load(resources, recipeContext,
                        List.of(new RegistryDataLoader.RegistryData<>(
                                Registries.RECIPE, Recipe.DIRECT_CODEC, RegistryValidator.none())),
                        ForkJoinPool.commonPool())
                .join();
        return initial.replaceFrom(RegistryLayer.WORLD, world).replaceFrom(RegistryLayer.RELOADABLE, recipes);
    }

    /**
     * 每个物品的默认组件，按官方 `DataComponentPatch.STREAM_CODEC` 编成十六进制。
     *
     * 默认组件是数据驱动的，`Item.components()` 要等 `ReloadableServerResources` 把它们绑上去
     * 才读得到，所以这里照 WorldLoader.load 的后半段再加载一次服务端资源。
     * `PendingTags.apply` 只是重新绑定同样的内容，重复调用没有副作用。
     */
    private static JsonArray itemComponents(
            CloseableResourceManager resources, LayeredRegistryAccess<RegistryLayer> layers) throws Exception {
        ReloadableServerResources managers = ReloadableServerResources
                .loadResources(resources, layers, STATIC_TAGS, FeatureFlags.DEFAULT_FLAGS,
                        Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                        ForkJoinPool.commonPool(), Runnable::run)
                .get();
        managers.updateComponentsAndStaticRegistryTags();
        RegistryAccess registries = (RegistryAccess) managers.fullRegistries().lookup();

        JsonArray items = new JsonArray();
        for (Item item : BuiltInRegistries.ITEM) {
            // 官方的 DataComponentMap 超过 8 项就换成按身份哈希排序的映射，遍历顺序每次运行都可能不同。
            // 组件补丁里的顺序没有含义，所以这里按组件类型的注册表 ID 排一遍，生成的字节才稳定。
            List<TypedDataComponent<?>> components = new ArrayList<>();
            for (TypedDataComponent<?> component : item.components()) {
                components.add(component);
            }
            components.sort(Comparator.comparingInt(
                    component -> BuiltInRegistries.DATA_COMPONENT_TYPE.getId(component.type())));

            DataComponentPatch.Builder patch = DataComponentPatch.builder();
            for (TypedDataComponent<?> component : components) {
                patch.set(component);
            }
            RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
            DataComponentPatch.STREAM_CODEC.encode(buffer, patch.build());
            byte[] bytes = new byte[buffer.readableBytes()];
            buffer.readBytes(bytes);

            StringBuilder hex = new StringBuilder(bytes.length * 2);
            for (byte value : bytes) {
                hex.append(Character.forDigit((value >> 4) & 0xf, 16));
                hex.append(Character.forDigit(value & 0xf, 16));
            }
            items.add(hex.toString());
        }
        return items;
    }

    /**
     * 按 ID 查表的枚举：连续的只要常量个数，稀疏的要整张 ID 表。
     *
     * 官方 `ByIdMap.continuous` 要求 ID 刚好是 0 到个数减一的排列，不然构造时就抛异常，
     * 所以连续的枚举只导出个数就够。越界时怎么折回（ZERO、CLAMP、WRAP）是代码，留在 Rust 里。
     */
    private static JsonObject componentEnums() {
        JsonObject counts = new JsonObject();
        counts.addProperty("rarity", Rarity.values().length);
        counts.addProperty("dye_color", DyeColor.values().length);
        counts.addProperty("item_use_animation", ItemUseAnimation.values().length);
        counts.addProperty("swing_animation_type", SwingAnimationType.values().length);
        counts.addProperty("equipment_slot", EquipmentSlot.values().length);
        counts.addProperty("equipment_slot_group", EquipmentSlotGroup.values().length);
        counts.addProperty("attribute_operation", AttributeModifier.Operation.values().length);
        counts.addProperty("attribute_display_type", ItemAttributeModifiers.Display.Type.values().length);
        counts.addProperty("firework_shape", FireworkExplosion.Shape.values().length);
        counts.addProperty("map_post_processing", MapPostProcessing.values().length);
        counts.addProperty("fox_variant", Fox.Variant.values().length);
        counts.addProperty("axolotl_variant", Axolotl.Variant.values().length);
        counts.addProperty("salmon_size", Salmon.Variant.values().length);
        counts.addProperty("parrot_variant", Parrot.Variant.values().length);
        counts.addProperty("mooshroom_variant", MushroomCow.Variant.values().length);
        counts.addProperty("llama_variant", Llama.Variant.values().length);
        counts.addProperty("horse_variant", net.minecraft.world.entity.animal.equine.Variant.values().length);

        JsonObject sparse = new JsonObject();
        JsonArray patterns = new JsonArray();
        for (TropicalFish.Pattern pattern : TropicalFish.Pattern.values()) {
            patterns.add(pattern.getPackedId());
        }
        sparse.add("tropical_fish_pattern", patterns);
        JsonArray rabbits = new JsonArray();
        for (Rabbit.Variant variant : Rabbit.Variant.values()) {
            rabbits.add(variant.id());
        }
        sparse.add("rabbit_variant", rabbits);

        JsonObject enums = new JsonObject();
        enums.add("counts", counts);
        enums.add("sparse", sparse);
        return enums;
    }

    /** 官方的 `Heightmap.Types`，区块包按 `id` 排列，`id` 也是网络上的键。 */
    private static JsonArray heightmaps() throws ReflectiveOperationException {
        JsonArray types = new JsonArray();
        for (Heightmap.Types type : Heightmap.Types.values()) {
            JsonObject entry = new JsonObject();
            entry.addProperty("name", type.getSerializedName());
            entry.addProperty("id", (int) readField(Heightmap.Types.class, "id", type));
            entry.addProperty("sendToClient", type.sendToClient());
            entry.addProperty("keepAfterWorldgen", type.keepAfterWorldgen());
            types.add(entry);
        }
        return types;
    }

    private static JsonObject version() {
        var current = SharedConstants.getCurrentVersion();
        JsonObject version = new JsonObject();
        version.addProperty("name", current.id());
        version.addProperty("protocol", current.protocolVersion());
        version.addProperty("dataVersion", current.dataVersion().version());
        return version;
    }

    private static JsonArray knownPacks(CloseableResourceManager resources) {
        JsonArray packs = new JsonArray();
        resources.listPacks()
                .flatMap(pack -> pack.location().knownPackInfo().stream())
                .forEach(pack -> packs.add(knownPack(pack)));
        return packs;
    }

    private static JsonObject knownPack(KnownPack pack) {
        JsonObject object = new JsonObject();
        object.addProperty("namespace", pack.namespace());
        object.addProperty("id", pack.id());
        object.addProperty("version", pack.version());
        return object;
    }

    private static JsonArray enabledFeatures() {
        JsonArray features = new JsonArray();
        FeatureFlags.REGISTRY.toNames(FeatureFlags.DEFAULT_FLAGS).stream()
                .map(Identifier::toString)
                .sorted()
                .forEach(features::add);
        return features;
    }

    /** 按官方的同步顺序列出注册表。条目顺序决定了它们的数字 ID。 */
    private static JsonArray syncedRegistries(LayeredRegistryAccess<RegistryLayer> layers) {
        RegistryAccess world = layers.getAccessFrom(RegistryLayer.WORLD);
        JsonArray registries = new JsonArray();
        for (RegistryDataLoader.RegistryData<?> data : RegistryDataLoader.SYNCHRONIZED_REGISTRIES) {
            world.lookup(data.key()).ifPresent(registry -> {
                JsonArray entries = new JsonArray();
                registry.listElements().forEach(element -> entries.add(element.key().identifier().toString()));

                JsonObject object = new JsonObject();
                object.addProperty("name", data.key().identifier().toString());
                object.add("entries", entries);
                registries.add(object);
            });
        }
        return registries;
    }

    /** 官方发给客户端的标签内容。排序只是为了让生成结果稳定，便于看差异。 */
    private static JsonArray tags(LayeredRegistryAccess<RegistryLayer> layers) {
        Map<ResourceKey<? extends Registry<?>>, TagNetworkSerialization.NetworkPayload> payloads =
                TagNetworkSerialization.serializeTagsToNetwork(layers);

        JsonArray groups = new JsonArray();
        payloads.entrySet().stream()
                .sorted(Comparator.comparing(entry -> entry.getKey().identifier().toString()))
                .forEach(entry -> {
                    JsonArray tags = new JsonArray();
                    entry.getValue().tags().entrySet().stream()
                            .sorted(Comparator.comparing(tag -> tag.getKey().toString()))
                            .forEach(tag -> {
                                JsonArray ids = new JsonArray();
                                tag.getValue().forEach(ids::add);

                                JsonObject object = new JsonObject();
                                object.addProperty("name", tag.getKey().toString());
                                object.add("ids", ids);
                                tags.add(object);
                            });

                    JsonObject group = new JsonObject();
                    group.addProperty("registry", entry.getKey().identifier().toString());
                    group.add("tags", tags);
                    groups.add(group);
                });
        return groups;
    }

    private static JsonObject blockStates() {
        JsonObject defaults = new JsonObject();
        BuiltInRegistries.BLOCK.forEach(block -> defaults.addProperty(
                BuiltInRegistries.BLOCK.getKey(block).toString(),
                Block.BLOCK_STATE_REGISTRY.getId(block.defaultBlockState())));

        JsonObject states = new JsonObject();
        states.addProperty("total", Block.BLOCK_STATE_REGISTRY.size());
        states.add("defaults", defaults);
        return states;
    }

    /** 按注册表 ID 排列的方块。除了状态 ID，其余字段都只看默认状态。 */
    private static JsonArray blocks() throws ReflectiveOperationException {
        JsonArray blocks = new JsonArray();
        for (Block block : BuiltInRegistries.BLOCK) {
            BlockState state = block.defaultBlockState();
            JsonArray states = new JsonArray();
            block.getStateDefinition().getPossibleStates()
                    .forEach(possible -> states.add(Block.BLOCK_STATE_REGISTRY.getId(possible)));

            JsonObject object = new JsonObject();
            object.addProperty("name", BuiltInRegistries.BLOCK.getKey(block).toString());
            object.add("states", states);
            object.addProperty("defaultState", Block.BLOCK_STATE_REGISTRY.getId(state));
            object.addProperty("item", BuiltInRegistries.ITEM.getId(block.asItem()));
            object.addProperty("hasCollision", hasPlacementCollision(state));
            object.addProperty("hasFluid", !state.getFluidState().isEmpty());
            object.addProperty("twoBlocks", takesTwoBlocks(block, state));
            object.addProperty("pushReaction", state.getPistonPushReaction().name().toLowerCase(Locale.ROOT));
            // 活塞推不动也不打碎的方块，`BlockBehaviour.Properties.strength(-1.0F)`。
            object.addProperty("unbreakable", state.getDestroySpeed(EmptyBlockGetter.INSTANCE, BlockPos.ZERO) == -1.0F);
            addBlockEntityType(object, state);
            object.addProperty("class", className(block.getClass()));
            object.add("ancestors", ancestors(block.getClass()));
            object.add("properties", properties(block));
            object.add("fields", fields(block));
            object.addProperty("instrument", state.instrument().getSerializedName());
            object.addProperty("destroyTime", block.defaultDestroyTime());
            object.addProperty("requiresCorrectToolForDrops", state.requiresCorrectToolForDrops());
            object.addProperty("dynamicShape", block.hasDynamicShape());
            addOffset(object, block);
            SoundEvent pickupSound = block instanceof BucketPickup pickup ? pickup.getPickupSound().orElse(null) : null;
            if (pickupSound != null) {
                object.addProperty("pickupSound", BuiltInRegistries.SOUND_EVENT.getId(pickupSound));
            }
            blocks.add(object);
        }
        return blocks;
    }

    /** 属性的默认值。玩家没有状态效果、没有附魔时的起点。 */
    private static JsonObject attributeDefaults() {
        JsonObject defaults = new JsonObject();
        for (Attribute attribute : BuiltInRegistries.ATTRIBUTE) {
            defaults.addProperty(
                    BuiltInRegistries.ATTRIBUTE.getKey(attribute).toString(),
                    attribute.getDefaultValue());
        }
        return defaults;
    }

    /** 分派表按类名走。匿名类没有名字，官方以后用了要先决定它算哪一类。 */
    private static String className(Class<?> type) {
        String name = type.getSimpleName();
        if (name.isEmpty()) {
            throw new IllegalStateException(type + " 没有类名");
        }
        return name;
    }

    /** 这个类、它的父类和方块包里的接口，也就是 instanceof 能成立的全部名字。 */
    private static JsonArray ancestors(Class<?> blockClass) {
        Set<String> names = new TreeSet<>();
        for (Class<?> type = blockClass; type != Block.class; type = type.getSuperclass()) {
            names.add(className(type));
            addInterfaces(type, names);
        }
        JsonArray ancestors = new JsonArray();
        names.forEach(ancestors::add);
        return ancestors;
    }

    private static void addInterfaces(Class<?> type, Set<String> names) {
        for (Class<?> parent : type.getInterfaces()) {
            if (parent.getPackage().equals(Block.class.getPackage())) {
                names.add(className(parent));
            }
            addInterfaces(parent, names);
        }
    }

    /** 按官方 StateDefinition 的顺序列出属性。状态 ID 的算法依赖这个顺序，所以这里校验一遍。 */
    private static JsonArray properties(Block block) {
        List<Property<?>> properties = List.copyOf(block.getStateDefinition().getProperties());
        checkCartesianProduct(block, properties);

        JsonArray result = new JsonArray();
        for (Property<?> property : properties) {
            JsonObject object = new JsonObject();
            object.addProperty("name", property.getName());
            object.add("values", valueNames(property));
            describeType(property, object);
            result.add(object);
        }
        return result;
    }

    private static <T extends Comparable<T>> JsonArray valueNames(Property<T> property) {
        JsonArray names = new JsonArray();
        property.getPossibleValues().forEach(value -> names.add(property.getName(value)));
        return names;
    }

    /**
     * 属性的官方类型，以及 {@code Comparable.compareTo} 用的顺序。
     *
     * <p>{@code StatePropertiesPredicate} 的区间比较按属性值的 {@code compareTo} 来，
     * 三种属性类各有各的顺序：
     * <ul>
     *   <li>{@code IntegerProperty} 按数值比，取值是 {@code min..=max} 的连续整数；</li>
     *   <li>{@code BooleanProperty} 的取值列表是 {@code [true, false]}，但 {@code false < true}；</li>
     *   <li>{@code EnumProperty} 按枚举常量的声明序号比，取值列表可能是常量的乱序子集。</li>
     * </ul>
     */
    private static void describeType(Property<?> property, JsonObject object) {
        if (property instanceof IntegerProperty) {
            List<?> values = property.getPossibleValues();
            object.addProperty("type", "integer");
            object.addProperty("min", (Integer) values.get(0));
            object.addProperty("max", (Integer) values.get(values.size() - 1));
        } else if (property instanceof BooleanProperty) {
            object.addProperty("type", "boolean");
        } else if (property instanceof EnumProperty<?> enumProperty) {
            object.addProperty("type", "enum");
            JsonArray ordinals = new JsonArray();
            enumProperty.getPossibleValues().forEach(value -> ordinals.add(((Enum<?>) value).ordinal()));
            object.add("ordinals", ordinals);
        } else {
            throw new IllegalStateException("未知的属性类型：" + property.getClass().getName());
        }
    }

    /** 方块的第 n 个状态就是属性值的笛卡尔积的第 n 项，最后一个属性变化最快。 */
    private static void checkCartesianProduct(Block block, List<Property<?>> properties) {
        List<BlockState> states = block.getStateDefinition().getPossibleStates();
        for (int index = 0; index < states.size(); index++) {
            int rest = index;
            for (int position = properties.size() - 1; position >= 0; position--) {
                Property<?> property = properties.get(position);
                List<?> values = property.getPossibleValues();
                Object expected = values.get(rest % values.size());
                rest /= values.size();
                if (!expected.equals(states.get(index).getValue(property))) {
                    throw new IllegalStateException(block + " 的状态不是属性值的笛卡尔积：" + states.get(index));
                }
            }
            int id = Block.BLOCK_STATE_REGISTRY.getId(states.get(index));
            if (id != Block.BLOCK_STATE_REGISTRY.getId(states.get(0)) + index) {
                throw new IllegalStateException(block + " 的状态 ID 不连续：" + states.get(index));
            }
        }
    }

    /**
     * 类的构造参数。Java 里它们是字段或者只读方法，Rust 的行为是按类共享的，只能按方块查。
     *
     * <p>这里不挑选字段，能表达的全部导出，这样移植时不用回来改生成器。子类的同名字段优先。
     */
    private static JsonArray fields(Block block) throws ReflectiveOperationException {
        Map<String, JsonObject> fields = new LinkedHashMap<>();
        for (Class<?> type = block.getClass(); type != Block.class; type = type.getSuperclass()) {
            for (Field field : type.getDeclaredFields()) {
                if (!Modifier.isStatic(field.getModifiers())) {
                    field.setAccessible(true);
                    addField(fields, field.getName(), field.get(block));
                }
            }
            for (Method method : type.getDeclaredMethods()) {
                if (isFactMethod(method)) {
                    method.setAccessible(true);
                    addMethodResult(fields, method, block);
                }
            }
        }
        JsonArray result = new JsonArray();
        new TreeMap<>(fields).values().forEach(result::add);
        return result;
    }

    private static boolean isFactMethod(Method method) {
        Class<?> type = method.getReturnType();
        return !Modifier.isStatic(method.getModifiers())
                && !method.isSynthetic()
                && method.getParameterCount() == 0
                && (type == int.class || type == boolean.class || type == float.class || type == double.class
                        || Block.class.isAssignableFrom(type)
                        || ItemLike.class.isAssignableFrom(type)
                        || type.isEnum());
    }

    private static void addMethodResult(Map<String, JsonObject> fields, Method method, Block block)
            throws IllegalAccessException {
        try {
            addField(fields, method.getName() + "()", method.invoke(block));
        } catch (InvocationTargetException error) {
            // 个别类的方法只在世界里才有意义，这样的方法不算事实。
        }
    }

    private static void addField(Map<String, JsonObject> fields, String name, Object value) {
        if (fields.containsKey(name)) {
            return;
        }
        if (value instanceof BlockSetType setType) {
            addSetType(fields, name, setType);
            return;
        }
        if (value instanceof WoodType woodType) {
            addField(fields, name + ".name", new Name(woodType.name()));
            addField(fields, name + ".fenceGateClose", BuiltInRegistries.SOUND_EVENT.getId(woodType.fenceGateClose()));
            addField(fields, name + ".fenceGateOpen", BuiltInRegistries.SOUND_EVENT.getId(woodType.fenceGateOpen()));
            addSetType(fields, name + ".setType", woodType.setType());
            return;
        }
        JsonObject typed = typedValue(value);
        if (typed != null) {
            JsonObject object = new JsonObject();
            object.addProperty("name", name);
            object.add("value", typed);
            fields.put(name, object);
        }
    }

    private static void addSetType(Map<String, JsonObject> fields, String name, BlockSetType type) {
        addField(fields, name + ".name", new Name(type.name()));
        addField(fields, name + ".canOpenByHand", type.canOpenByHand());
        addField(fields, name + ".canOpenByWindCharge", type.canOpenByWindCharge());
        addField(fields, name + ".canButtonBeActivatedByArrows", type.canButtonBeActivatedByArrows());
        addField(fields, name + ".pressurePlateSensitivity", type.pressurePlateSensitivity());
        // 门、活板门、栅栏门、按钮、压力板的声音。和别的注册表 ID 一样存整数。
        addField(fields, name + ".doorClose", BuiltInRegistries.SOUND_EVENT.getId(type.doorClose()));
        addField(fields, name + ".doorOpen", BuiltInRegistries.SOUND_EVENT.getId(type.doorOpen()));
        addField(fields, name + ".trapdoorClose", BuiltInRegistries.SOUND_EVENT.getId(type.trapdoorClose()));
        addField(fields, name + ".trapdoorOpen", BuiltInRegistries.SOUND_EVENT.getId(type.trapdoorOpen()));
        addField(fields, name + ".pressurePlateClickOff",
                BuiltInRegistries.SOUND_EVENT.getId(type.pressurePlateClickOff()));
        addField(fields, name + ".pressurePlateClickOn",
                BuiltInRegistries.SOUND_EVENT.getId(type.pressurePlateClickOn()));
        addField(fields, name + ".buttonClickOff", BuiltInRegistries.SOUND_EVENT.getId(type.buttonClickOff()));
        addField(fields, name + ".buttonClickOn", BuiltInRegistries.SOUND_EVENT.getId(type.buttonClickOn()));
    }

    /** 枚举和记录的名字。和普通字符串分开，普通字符串字段不导出。 */
    private record Name(String value) {}

    private static JsonObject typedValue(Object value) {
        JsonObject typed = new JsonObject();
        switch (value) {
            case null -> {
                return null;
            }
            case Boolean bool -> typed.addProperty("bool", bool);
            case Integer number -> typed.addProperty("int", number);
            case Float number -> typed.addProperty("float", number);
            case Double number -> typed.addProperty("double", number);
            case Block block -> typed.addProperty("block", BuiltInRegistries.BLOCK.getId(block));
            case BlockState state -> typed.addProperty("state", Block.BLOCK_STATE_REGISTRY.getId(state));
            case ItemLike item -> typed.addProperty("item", BuiltInRegistries.ITEM.getId(item.asItem()));
            case Direction direction -> typed.addProperty("direction", direction.get3DDataValue());
            case TagKey<?> tag when tag.registry().equals(Registries.BLOCK) ->
                    typed.addProperty("tag", tag.location().toString());
            case ResourceKey<?> key when key.isFor(Registries.BLOCK) -> typed.addProperty(
                    "block", BuiltInRegistries.BLOCK.getId(BuiltInRegistries.BLOCK.getValue(key.identifier())));
            case ResourceKey<?> key when key.isFor(Registries.ITEM) -> typed.addProperty(
                    "item", BuiltInRegistries.ITEM.getId(BuiltInRegistries.ITEM.getValue(key.identifier())));
            case Name name -> typed.addProperty("name", name.value());
            case Enum<?> constant -> typed.addProperty("name", constant.name().toLowerCase(Locale.ROOT));
            default -> {
                return null;
            }
        }
        return typed;
    }

    /** 植物的渲染偏移。形状跟着偏移走的方块，碰撞和支撑都和位置有关，见 StateTables。 */
    private static void addOffset(JsonObject object, Block block) throws ReflectiveOperationException {
        BlockState state = block.defaultBlockState();
        if (!state.hasOffsetFunction()) {
            return;
        }
        JsonObject offset = new JsonObject();
        offset.addProperty("vertical", SAMPLE_POSITIONS.stream().anyMatch(pos -> state.getOffset(pos).y != 0.0));
        offset.addProperty("maxHorizontal", protectedFloat(block, "getMaxHorizontalOffset"));
        offset.addProperty("maxVertical", protectedFloat(block, "getMaxVerticalOffset"));
        offset.addProperty("movesShape", StateTables.movesShape(block));
        object.add("offset", offset);
    }

    private static float protectedFloat(Block block, String name) throws ReflectiveOperationException {
        Method method = BlockBehaviour.class.getDeclaredMethod(name);
        method.setAccessible(true);
        return (float) method.invoke(block);
    }

    /** 和 BlockItem.canPlace 用同一种上下文，scaffolding 只在这种上下文里没有碰撞。 */
    private static boolean hasPlacementCollision(BlockState state) {
        return !state.getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO, CollisionContext.placementContext(null))
                .isEmpty();
    }

    /** 官方放下时占两格。pitcher_crop 也有 half 属性，但它放下时只占一格，长大后才有上半。 */
    private static boolean takesTwoBlocks(Block block, BlockState state) {
        return (state.hasProperty(BlockStateProperties.DOUBLE_BLOCK_HALF) && !(block instanceof PitcherCropBlock))
                || state.hasProperty(BlockStateProperties.BED_PART);
    }

    /** 这里不能用 newBlockEntity，部分类型的构造函数要读绑定后的组件。区块包要写类型 ID，所以类型必须唯一。 */
    private static void addBlockEntityType(JsonObject object, BlockState state) {
        List<BlockEntityType<?>> types = BuiltInRegistries.BLOCK_ENTITY_TYPE.stream()
                .filter(type -> type.isValid(state))
                .toList();
        if (types.size() != (state.hasBlockEntity() ? 1 : 0)) {
            throw new IllegalStateException(state + " 的方块实体类型对不上：" + types);
        }
        types.forEach(type -> object.addProperty("blockEntityType", BuiltInRegistries.BLOCK_ENTITY_TYPE.getId(type)));
    }

    /** 按注册表 ID 排列的物品。物品组件要绑定之后才能读，那部分从官方报告里取。 */
    private static JsonArray items() throws ReflectiveOperationException {
        JsonArray items = new JsonArray();
        for (Item item : BuiltInRegistries.ITEM) {
            JsonObject object = new JsonObject();
            object.addProperty("name", BuiltInRegistries.ITEM.getKey(item).toString());
            object.addProperty("class", item.getClass().getSimpleName());
            if (item instanceof BlockItem blockItem) {
                object.addProperty("block", BuiltInRegistries.BLOCK.getId(blockItem.getBlock()));
            }
            if (item instanceof StandingAndWallBlockItem wallItem) {
                Block wallBlock = (Block) readField(StandingAndWallBlockItem.class, "wallBlock", wallItem);
                Direction attachment = (Direction) readField(StandingAndWallBlockItem.class, "attachmentDirection", wallItem);
                object.addProperty("wallBlock", BuiltInRegistries.BLOCK.getId(wallBlock));
                object.addProperty("attachment", attachment.get3DDataValue());
            }
            if (item instanceof SolidBucketItem solidBucket) {
                // 细雪桶放下了自己的声音，不用方块声音表里的放置声音。
                SoundEvent placeSound = (SoundEvent) readField(SolidBucketItem.class, "placeSound", solidBucket);
                object.addProperty("placeSound", BuiltInRegistries.SOUND_EVENT.getId(placeSound));
            }
            object.addProperty("overridesCanDestroyBlock", overridesCanDestroyBlock(item));
            items.add(object);
        }
        return items;
    }

    /** 官方数据包的全部配方，按注册表的顺序。`getRecipeFor` 取第一条匹配的，先后由这个顺序决定。 */
    private static JsonObject recipes(RegistryAccess registries) throws ReflectiveOperationException {
        JsonArray entries = new JsonArray();
        for (RecipeHolder<?> holder : new RecipeManager(registries).getRecipes()) {
            entries.add(recipe(holder));
        }
        JsonObject object = new JsonObject();
        object.add("potions", potions());
        object.add("entries", entries);
        return object;
    }

    /** 药水注册表的条目，顺序就是 ID。酿造配方的药水条件引用的数字是这里的下标。 */
    private static JsonArray potions() {
        JsonArray potions = new JsonArray();
        for (Potion potion : BuiltInRegistries.POTION) {
            potions.add(BuiltInRegistries.POTION.getKey(potion).toString());
        }
        return potions;
    }

    private static JsonObject recipe(RecipeHolder<?> holder) throws ReflectiveOperationException {
        Recipe<?> recipe = holder.value();
        JsonObject object = new JsonObject();
        object.addProperty("id", holder.id().identifier().toString());
        object.addProperty("type", BuiltInRegistries.RECIPE_SERIALIZER.getKey(recipe.getSerializer()).toString());
        if (recipe instanceof ShapedRecipe shaped) {
            object.addProperty("kind", "shaped");
            object.addProperty("width", shaped.getWidth());
            object.addProperty("height", shaped.getHeight());
            object.add("pattern", pattern(shaped.getIngredients()));
            object.add("result", stack((ItemStackTemplate) readField(ShapedRecipe.class, "result", shaped)));
        } else if (recipe instanceof ShapelessRecipe shapeless) {
            object.addProperty("kind", "shapeless");
            object.add("ingredients", ingredients((List<Ingredient>) readField(ShapelessRecipe.class, "ingredients", shapeless)));
            object.add("result", stack((ItemStackTemplate) readField(ShapelessRecipe.class, "result", shapeless)));
        } else if (recipe instanceof TransmuteRecipe transmute) {
            object.addProperty("kind", "transmute");
            object.add("input", ingredient((Ingredient) readField(TransmuteRecipe.class, "input", transmute)));
            object.add("material", ingredient((Ingredient) readField(TransmuteRecipe.class, "material", transmute)));
            MinMaxBounds.Ints bounds = (MinMaxBounds.Ints) readField(TransmuteRecipe.class, "materialCount", transmute);
            object.addProperty("minMaterial", bounds.min().orElse(1));
            object.addProperty("maxMaterial", bounds.max().orElse(8));
            transmuteResult(object, (TransmuteResult) readField(TransmuteRecipe.class, "result", transmute));
            object.addProperty("addMaterialCount", (boolean) readField(TransmuteRecipe.class, "addMaterialCountToResult", transmute));
        } else if (recipe instanceof DyeRecipe dye) {
            object.addProperty("kind", "dye");
            object.add("target", ingredient((Ingredient) readField(DyeRecipe.class, "target", dye)));
            object.add("dye", ingredient((Ingredient) readField(DyeRecipe.class, "dye", dye)));
            object.add("result", stack((ItemStackTemplate) readField(DyeRecipe.class, "result", dye)));
        } else if (recipe instanceof ImbueRecipe imbue) {
            object.addProperty("kind", "imbue");
            object.add("source", ingredient((Ingredient) readField(ImbueRecipe.class, "source", imbue)));
            object.add("material", ingredient((Ingredient) readField(ImbueRecipe.class, "material", imbue)));
            object.add("result", stack((ItemStackTemplate) readField(ImbueRecipe.class, "result", imbue)));
        } else if (recipe instanceof AbstractCookingRecipe cooking) {
            String kind = recipe instanceof SmeltingRecipe ? "smelting"
                    : recipe instanceof BlastingRecipe ? "blasting"
                    : recipe instanceof SmokingRecipe ? "smoking" : "campfire_cooking";
            object.addProperty("kind", kind);
            object.add("ingredient", ingredient((Ingredient) readField(SingleItemRecipe.class, "input", cooking)));
            object.add("result", stack((ItemStackTemplate) readField(SingleItemRecipe.class, "result", cooking)));
            object.addProperty("experience", cooking.experience());
            object.addProperty("time", cooking.cookingTime());
        } else if (recipe instanceof StonecutterRecipe stonecutting) {
            object.addProperty("kind", "stonecutting");
            object.add("ingredient", ingredient((Ingredient) readField(SingleItemRecipe.class, "input", stonecutting)));
            object.add("result", stack((ItemStackTemplate) readField(SingleItemRecipe.class, "result", stonecutting)));
        } else if (recipe instanceof SmithingTransformRecipe transform) {
            object.addProperty("kind", "smithing_transform");
            object.add("template", optionalIngredient(transform.templateIngredient()));
            object.add("base", ingredient(transform.baseIngredient()));
            object.add("addition", optionalIngredient(transform.additionIngredient()));
            object.add("result", stack((ItemStackTemplate) readField(SmithingTransformRecipe.class, "result", transform)));
        } else if (recipe instanceof SmithingTrimRecipe trim) {
            object.addProperty("kind", "smithing_trim");
            object.add("template", ingredient(trim.templateIngredient().orElseThrow()));
            object.add("base", ingredient(trim.baseIngredient()));
            object.add("addition", ingredient(trim.additionIngredient().orElseThrow()));
            Holder<?> pattern = (Holder<?>) readField(SmithingTrimRecipe.class, "pattern", trim);
            object.addProperty("pattern", pattern.unwrapKey().orElseThrow().identifier().toString());
        } else if (recipe instanceof BrewingRecipe brewing) {
            object.addProperty("kind", "brewing");
            object.add("input", potionIngredient(brewing.getInput()));
            object.add("reagent", potionIngredient(brewing.getReagent()));
            object.add("result", stack(brewing.getOutput()));
        } else if (recipe instanceof FireworkRocketRecipe rocket) {
            object.addProperty("kind", "firework_rocket");
            object.add("shell", ingredient((Ingredient) readField(FireworkRocketRecipe.class, "shell", rocket)));
            object.add("fuel", ingredient((Ingredient) readField(FireworkRocketRecipe.class, "fuel", rocket)));
            object.add("star", ingredient((Ingredient) readField(FireworkRocketRecipe.class, "star", rocket)));
            object.add("result", stack((ItemStackTemplate) readField(FireworkRocketRecipe.class, "result", rocket)));
        } else if (recipe instanceof FireworkStarRecipe star) {
            object.addProperty("kind", "firework_star");
            JsonArray shapes = new JsonArray();
            Map<?, Ingredient> shapeIngredients = (Map<?, Ingredient>) readField(FireworkStarRecipe.class, "shapes", star);
            for (Map.Entry<?, Ingredient> entry : shapeIngredients.entrySet()) {
                JsonObject shape = new JsonObject();
                shape.addProperty("shape", ((FireworkExplosion.Shape) entry.getKey()).getSerializedName());
                shape.add("ingredient", ingredient(entry.getValue()));
                shapes.add(shape);
            }
            object.add("shapes", shapes);
            object.add("trail", ingredient((Ingredient) readField(FireworkStarRecipe.class, "trail", star)));
            object.add("twinkle", ingredient((Ingredient) readField(FireworkStarRecipe.class, "twinkle", star)));
            object.add("fuel", ingredient((Ingredient) readField(FireworkStarRecipe.class, "fuel", star)));
            object.add("dye", ingredient((Ingredient) readField(FireworkStarRecipe.class, "dye", star)));
            object.add("result", stack((ItemStackTemplate) readField(FireworkStarRecipe.class, "result", star)));
        } else if (recipe instanceof FireworkStarFadeRecipe fade) {
            object.addProperty("kind", "firework_star_fade");
            object.add("target", ingredient((Ingredient) readField(FireworkStarFadeRecipe.class, "target", fade)));
            object.add("dye", ingredient((Ingredient) readField(FireworkStarFadeRecipe.class, "dye", fade)));
            object.add("result", stack((ItemStackTemplate) readField(FireworkStarFadeRecipe.class, "result", fade)));
        } else if (recipe instanceof MapExtendingRecipe map) {
            object.addProperty("kind", "mapextending");
            object.add("map", ingredient((Ingredient) readField(MapExtendingRecipe.class, "map", map)));
            object.add("material", ingredient((Ingredient) readField(MapExtendingRecipe.class, "material", map)));
            transmuteResult(object, (TransmuteResult) readField(MapExtendingRecipe.class, "result", map));
        } else if (recipe instanceof BannerDuplicateRecipe banner) {
            object.addProperty("kind", "banner_duplicate");
            object.add("banner", ingredient((Ingredient) readField(BannerDuplicateRecipe.class, "banner", banner)));
            object.add("result", stack((ItemStackTemplate) readField(BannerDuplicateRecipe.class, "result", banner)));
        } else if (recipe instanceof BookCloningRecipe book) {
            object.addProperty("kind", "book_cloning");
            object.add("source", ingredient((Ingredient) readField(BookCloningRecipe.class, "source", book)));
            object.add("material", ingredient((Ingredient) readField(BookCloningRecipe.class, "material", book)));
            MinMaxBounds.Ints bounds = (MinMaxBounds.Ints) readField(BookCloningRecipe.class, "allowedGenerations", book);
            object.addProperty("minGeneration", bounds.min().orElse(0));
            object.addProperty("maxGeneration", bounds.max().orElse(Integer.MAX_VALUE));
            object.add("result", stack((ItemStackTemplate) readField(BookCloningRecipe.class, "result", book)));
        } else if (recipe instanceof ShieldDecorationRecipe shield) {
            object.addProperty("kind", "shield_decoration");
            object.add("banner", ingredient((Ingredient) readField(ShieldDecorationRecipe.class, "banner", shield)));
            object.add("target", ingredient((Ingredient) readField(ShieldDecorationRecipe.class, "target", shield)));
            object.add("result", stack((ItemStackTemplate) readField(ShieldDecorationRecipe.class, "result", shield)));
        } else if (recipe instanceof RepairItemRecipe) {
            object.addProperty("kind", "repair_item");
        } else if (recipe instanceof DecoratedPotRecipe pot) {
            object.addProperty("kind", "decorated_pot");
            object.add("back", ingredient((Ingredient) readField(DecoratedPotRecipe.class, "backPattern", pot)));
            object.add("left", ingredient((Ingredient) readField(DecoratedPotRecipe.class, "leftPattern", pot)));
            object.add("right", ingredient((Ingredient) readField(DecoratedPotRecipe.class, "rightPattern", pot)));
            object.add("front", ingredient((Ingredient) readField(DecoratedPotRecipe.class, "frontPattern", pot)));
            object.add("result", stack((ItemStackTemplate) readField(DecoratedPotRecipe.class, "result", pot)));
        } else {
            throw new IllegalStateException("unsupported recipe " + holder.id());
        }
        return object;
    }

    private static JsonArray pattern(List<Optional<Ingredient>> ingredients) throws ReflectiveOperationException {
        JsonArray pattern = new JsonArray();
        for (Optional<Ingredient> ingredient : ingredients) {
            pattern.add(optionalIngredient(ingredient));
        }
        return pattern;
    }

    private static JsonArray ingredients(List<Ingredient> ingredients) throws ReflectiveOperationException {
        JsonArray array = new JsonArray();
        for (Ingredient ingredient : ingredients) {
            array.add(ingredient(ingredient));
        }
        return array;
    }

    private static JsonElement optionalIngredient(Optional<Ingredient> ingredient) throws ReflectiveOperationException {
        return ingredient.isPresent() ? ingredient(ingredient.get()) : JsonNull.INSTANCE;
    }

    /** 标签留着名字，物品展开成注册表 ID。这个名字同时用来检查 mc-bamboo-data 里的标签表。 */
    @SuppressWarnings("unchecked")
    private static JsonObject ingredient(Ingredient ingredient) throws ReflectiveOperationException {
        HolderSet<Item> values = (HolderSet<Item>) readField(Ingredient.class, "values", ingredient);
        JsonObject object = new JsonObject();
        values.unwrapKey().ifPresent(tag -> object.addProperty("tag", tag.location().toString()));
        TreeSet<Integer> ids = new TreeSet<>();
        for (Holder<Item> item : values) {
            ids.add(BuiltInRegistries.ITEM.getId(item.value()));
        }
        JsonArray items = new JsonArray();
        ids.forEach(items::add);
        object.add("items", items);
        return object;
    }

    private static JsonObject potionIngredient(PotionIngredient ingredient) throws ReflectiveOperationException {
        JsonObject object = new JsonObject();
        object.add("item", ingredient(ingredient.ingredient()));
        Optional<PotionsPredicate> predicate = ingredient.potions();
        if (predicate.isPresent()) {
            if (predicate.get().effects().isPresent()) {
                throw new IllegalStateException("potion effects predicates are not supported");
            }
            JsonArray potions = new JsonArray();
            for (Holder<Potion> potion : predicate.get().potions().orElseThrow()) {
                potions.add(BuiltInRegistries.POTION.getId(potion.value()));
            }
            object.add("potions", potions);
        }
        return object;
    }

    private static JsonObject stack(ItemStackTemplate template) {
        JsonObject object = new JsonObject();
        object.addProperty("item", BuiltInRegistries.ITEM.getId(template.item().value()));
        object.addProperty("count", template.count());
        object.addProperty("hasComponents", !template.components().isEmpty());
        return object;
    }

    private static void transmuteResult(JsonObject object, TransmuteResult result) {
        result.item().ifPresent(item -> object.addProperty("resultItem", BuiltInRegistries.ITEM.getId(item.value())));
        object.addProperty("resultCount", result.count());
        object.addProperty("resultHasComponents", !result.components().isEmpty());
    }

    private static Object readField(Class<?> owner, String name, Object target) throws ReflectiveOperationException {
        Field field = owner.getDeclaredField(name);
        field.setAccessible(true);
        return field.get(target);
    }

    /** 官方写在静态表里的方块对应关系。 */
    private static JsonObject blockMaps(ResourceManager resources, HolderLookup.Provider registries)
            throws ReflectiveOperationException, IOException {
        JsonObject maps = new JsonObject();
        maps.add("flowerPots", blockPairs(staticMap(FlowerPotBlock.class, "POTTED_BY_CONTENT")));
        maps.add("candleCakes", blockPairs(staticMap(CandleCakeBlock.class, "BY_CANDLE")));
        maps.add("weathering", blockPairs(WeatheringCopper.NEXT_BY_BLOCK.get()));
        maps.add("waxables", blockPairs(HoneycombItem.WAXABLES.get()));
        maps.add("fireOdds", fireOdds());
        maps.add("ignitedByLava", ignitedByLava());
        maps.add("campfireInputs", campfireInputs(resources, registries));
        maps.add("compostLayers", compostLayers(resources));
        return maps;
    }

    /** 数据包 context_int_provider/compostable 下的整数提供器原文，键是物品组件引用它用的名字。 */
    private static JsonObject compostLayers(ResourceManager resources) throws IOException {
        String directory = "context_int_provider/";
        Map<String, JsonElement> providers = new TreeMap<>();
        for (var entry : resources.listResources(directory + "compostable", id -> id.getPath().endsWith(".json")).entrySet()) {
            Identifier file = entry.getKey();
            String path = file.getPath();
            String name = file.getNamespace() + ":" + path.substring(directory.length(), path.length() - ".json".length());
            try (Reader reader = entry.getValue().openAsReader()) {
                providers.put(name, JsonParser.parseReader(reader));
            }
        }
        JsonObject layers = new JsonObject();
        providers.forEach(layers::add);
        return layers;
    }

    /**
     * 世界生成的纯计算部分要用的几个注册表。
     *
     * <p>噪声参数、密度函数和维度设置按官方 codec 原样编回 JSON，Bamboo 用同一套 codec 的语义解析，
     * 所以升级版本时数据和解析一起动，不会对不上。密度函数里对注册表的引用还是引用（字符串），
     * Bamboo 解析时才展开，和官方编译前跑 `DfRewriteRule.INLINE_REFERENCE` 的效果一样。
     *
     * <p>多噪声群系预设不走 codec：官方的 `Climate.Parameter` 在 codec 里是浮点，
     * 而内存里是「原值乘一万再截断」的定点数，来回换算会差一两个最低位，
     * 所以这里直接导出定点数。顺序就是官方 `ParameterList` 里的顺序，R 树靠它建。
     */
    private static JsonObject worldgen(RegistryAccess registries) {
        RegistryOps<JsonElement> ops = registries.createSerializationContext(JsonOps.INSTANCE);
        JsonObject worldgen = new JsonObject();
        worldgen.add("noise", encodeRegistry(registries, ops, Registries.NOISE, NormalNoise.DIRECT_CODEC));
        worldgen.add("density_function",
                encodeRegistry(registries, ops, Registries.DENSITY_FUNCTION, DensityFunction.CODEC));
        worldgen.add("noise_settings",
                encodeRegistry(registries, ops, Registries.NOISE_SETTINGS, NoiseGeneratorSettings.DIRECT_CODEC));
        worldgen.add("multi_noise_presets", multiNoisePresets(registries));
        worldgen.add("end_biomes", endBiomes());
        return worldgen;
    }

    /** 一个注册表的全部条目，按名字排序，每一条用给定的 codec 编成 JSON。 */
    private static <T> JsonObject encodeRegistry(RegistryAccess registries, RegistryOps<JsonElement> ops,
            ResourceKey<Registry<T>> key, Codec<T> codec) {
        Registry<T> registry = registries.lookupOrThrow(key);
        JsonObject entries = new JsonObject();
        for (Identifier name : registry.keySet().stream().sorted().toList()) {
            entries.add(name.toString(), codec.encodeStart(ops, registry.getValue(name)).getOrThrow());
        }
        return entries;
    }

    /** 两个多噪声群系预设，每条是群系 ID 加十三个定点数（六个区间的上下界和 offset）。 */
    private static JsonObject multiNoisePresets(RegistryAccess registries) {
        Registry<MultiNoiseBiomeSourceParameterList> registry =
                registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST);
        JsonObject presets = new JsonObject();
        for (Identifier name : registry.keySet().stream().sorted().toList()) {
            JsonArray entries = new JsonArray();
            for (Pair<Climate.ParameterPoint, Holder<Biome>> entry : registry.getValue(name).parameters().values()) {
                Climate.ParameterPoint point = entry.getFirst();
                JsonArray parameters = new JsonArray();
                for (Climate.Parameter parameter : List.of(point.temperature(), point.humidity(),
                        point.continentalness(), point.erosion(), point.depth(), point.weirdness())) {
                    parameters.add(parameter.min());
                    parameters.add(parameter.max());
                }
                parameters.add(point.offset());
                JsonObject item = new JsonObject();
                item.addProperty("biome", entry.getSecond().unwrapKey().orElseThrow().identifier().toString());
                item.add("parameters", parameters);
                entries.add(item);
            }
            presets.add(name.toString(), entries);
        }
        return presets;
    }

    /** TheEndBiomeSource 的五个群系，顺序和官方构造参数一致。 */
    private static JsonObject endBiomes() {
        JsonObject biomes = new JsonObject();
        biomes.addProperty("end", Biomes.THE_END.identifier().toString());
        biomes.addProperty("highlands", Biomes.END_HIGHLANDS.identifier().toString());
        biomes.addProperty("midlands", Biomes.END_MIDLANDS.identifier().toString());
        biomes.addProperty("islands", Biomes.SMALL_END_ISLANDS.identifier().toString());
        biomes.addProperty("barrens", Biomes.END_BARRENS.identifier().toString());
        return biomes;
    }

    /** BlockBehaviour.Properties.ignitedByLava 是整个方块的属性，按方块 ID 列出来。 */
    private static JsonArray ignitedByLava() {
        JsonArray ids = new JsonArray();
        for (Block block : BuiltInRegistries.BLOCK) {
            if (block.defaultBlockState().ignitedByLava()) {
                ids.add(BuiltInRegistries.BLOCK.getId(block));
            }
        }
        return ids;
    }

    /** RecipePropertySet.CAMPFIRE_INPUT：官方加载配方时从营火配方的原料收集出这个集合。 */
    private static JsonArray campfireInputs(ResourceManager resources, HolderLookup.Provider registries) throws IOException {
        RegistryOps<JsonElement> ops = registries.createSerializationContext(JsonOps.INSTANCE);
        Set<Integer> inputs = new TreeSet<>();
        for (Resource recipe : resources.listResources("recipe", id -> id.getPath().endsWith(".json")).values()) {
            try (Reader reader = recipe.openAsReader()) {
                JsonObject json = JsonParser.parseReader(reader).getAsJsonObject();
                if ("minecraft:campfire_cooking".equals(json.get("type").getAsString())) {
                    Ingredient.CODEC.parse(ops, json.get("ingredient")).getOrThrow().items()
                            .forEach(item -> inputs.add(BuiltInRegistries.ITEM.getId(item.value())));
                }
            }
        }
        JsonArray ids = new JsonArray();
        inputs.forEach(ids::add);
        return ids;
    }

    @SuppressWarnings("unchecked")
    private static Map<Block, Block> staticMap(Class<?> owner, String name) throws ReflectiveOperationException {
        return (Map<Block, Block>) readField(owner, name, null);
    }

    /** 官方的表是 HashMap，这里按键的 ID 排序，生成结果才稳定。 */
    private static JsonArray blockPairs(Map<? extends Block, ? extends Block> map) {
        JsonArray pairs = new JsonArray();
        map.entrySet().stream()
                .sorted(Comparator.comparingInt(entry -> BuiltInRegistries.BLOCK.getId(entry.getKey())))
                .forEach(entry -> {
                    JsonArray pair = new JsonArray();
                    pair.add(BuiltInRegistries.BLOCK.getId(entry.getKey()));
                    pair.add(BuiltInRegistries.BLOCK.getId(entry.getValue()));
                    pairs.add(pair);
                });
        return pairs;
    }

    /** FireBlock 的两张实例表，判定火能不能附着在旁边的方块上。 */
    @SuppressWarnings("unchecked")
    private static JsonArray fireOdds() throws ReflectiveOperationException {
        Map<Block, Integer> ignite = (Map<Block, Integer>) readField(FireBlock.class, "igniteOdds", Blocks.FIRE);
        Map<Block, Integer> burn = (Map<Block, Integer>) readField(FireBlock.class, "burnOdds", Blocks.FIRE);
        JsonArray odds = new JsonArray();
        ignite.keySet().stream()
                .sorted(Comparator.comparingInt(BuiltInRegistries.BLOCK::getId))
                .forEach(block -> {
                    JsonArray entry = new JsonArray();
                    entry.add(BuiltInRegistries.BLOCK.getId(block));
                    entry.add(ignite.get(block));
                    entry.add(burn.getOrDefault(block, 0));
                    odds.add(entry);
                });
        return odds;
    }

    /**
     * 世界层的 BlockTransformer 注册表：物品的 block_transformer 组件指向这里的条目，斧子去皮、锄头耕地、锹压土径都是它。
     *
     * <p>只导出改方块的部分。状态提供器和方块判定只认识官方数据用到的几种，遇到别的直接报错：
     * 升级版本时要先在 mc-bamboo-blocks 里实现它，再加进这里。
     */
    private static JsonArray blockTransformers(RegistryAccess registries) throws ReflectiveOperationException {
        Registry<BlockTransformer> registry = registries.lookupOrThrow(Registries.BLOCK_TRANSFORMER);
        JsonArray transformers = new JsonArray();
        for (Identifier name : registry.keySet().stream().sorted().toList()) {
            JsonArray transforms = new JsonArray();
            for (BlockTransformer.BlockTransformData data : registry.getValue(name).transforms()) {
                transforms.add(blockTransform(data));
            }
            JsonObject transformer = new JsonObject();
            transformer.addProperty("name", name.toString());
            transformer.add("transforms", transforms);
            transformers.add(transformer);
        }
        return transformers;
    }

    private static JsonObject blockTransform(BlockTransformer.BlockTransformData data) throws ReflectiveOperationException {
        JsonArray faces = new JsonArray();
        data.disallowedFaces().forEach(face -> faces.add(face.get3DDataValue()));
        JsonObject transform = new JsonObject();
        transform.add("provider", stateProvider(data.blockStateProvider().value()));
        transform.add("disallowedFaces", faces);
        transform.addProperty("updateFromNeighbors", data.updateFromNeighbors());
        // 没配声音时字段是 `SoundEvents.EMPTY`，不导出，生成器按 `None` 处理。
        SoundEvent sound = data.sound().value();
        if (sound != SoundEvents.EMPTY) {
            transform.addProperty("sound", BuiltInRegistries.SOUND_EVENT.getId(sound));
        }
        // `TransformParticle` 发的等级事件，`NONE` 是 0。
        transform.addProperty("particle", data.particle().getLevelEvent());
        // `transform_type == COPPER_CHEST`：改完方块还要给大箱子的另一半发一次粒子。
        transform.addProperty(
                "copperChest",
                data.transformType() == BlockTransformer.TransformType.COPPER_CHEST);
        return transform;
    }

    private static JsonObject stateProvider(BlockStateProvider provider) throws ReflectiveOperationException {
        JsonObject typed = new JsonObject();
        switch (provider) {
            case SimpleStateProvider simple -> typed.addProperty("simple", Block.BLOCK_STATE_REGISTRY.getId(simple.state()));
            case CopyPropertiesProvider copy -> typed.add("copyProperties", stateProvider(copy.source().value()));
            case RuleBasedStateProvider rules -> typed.add("ruleBased", ruleBased(rules));
            default -> throw new IllegalStateException("unsupported block state provider " + provider.getClass().getName());
        }
        return typed;
    }

    private static JsonObject ruleBased(RuleBasedStateProvider provider) throws ReflectiveOperationException {
        JsonArray rules = new JsonArray();
        for (RuleBasedStateProvider.Rule rule : provider.rules()) {
            JsonObject object = new JsonObject();
            object.add("ifTrue", blockPredicate(rule.ifTrue()));
            object.add("then", stateProvider(rule.then().value()));
            rules.add(object);
        }
        JsonObject object = new JsonObject();
        object.add("rules", rules);
        if (provider.fallback() != null) {
            object.add("fallback", stateProvider(provider.fallback().value()));
        }
        return object;
    }

    @SuppressWarnings("unchecked")
    private static JsonObject blockPredicate(BlockPredicate predicate) throws ReflectiveOperationException {
        JsonObject typed = new JsonObject();
        switch (predicate) {
            case MatchingBlocksPredicate matching -> {
                JsonObject object = predicateOffset(matching);
                JsonArray ids = new JsonArray();
                ((HolderSet<Block>) readField(MatchingBlocksPredicate.class, "blocks", matching))
                        .forEach(block -> ids.add(BuiltInRegistries.BLOCK.getId(block.value())));
                object.add("blocks", ids);
                typed.add("matchingBlocks", object);
            }
            case MatchingBlockTagPredicate matching -> {
                JsonObject object = predicateOffset(matching);
                TagKey<Block> tag = (TagKey<Block>) readField(MatchingBlockTagPredicate.class, "tag", matching);
                object.addProperty("tag", tag.location().toString());
                typed.add("matchingBlockTag", object);
            }
            case AllOfPredicate all -> {
                JsonArray predicates = new JsonArray();
                for (BlockPredicate part : (List<BlockPredicate>) readField(CombiningPredicate.class, "predicates", all)) {
                    predicates.add(blockPredicate(part));
                }
                typed.add("allOf", predicates);
            }
            default -> throw new IllegalStateException("unsupported block predicate " + predicate.getClass().getName());
        }
        return typed;
    }

    /** 判定的那一格相对被点的方块的位置。 */
    private static JsonObject predicateOffset(StateTestingPredicate predicate) throws ReflectiveOperationException {
        Vec3i offset = (Vec3i) readField(StateTestingPredicate.class, "offset", predicate);
        JsonArray xyz = new JsonArray();
        xyz.add(offset.getX());
        xyz.add(offset.getY());
        xyz.add(offset.getZ());
        JsonObject object = new JsonObject();
        object.add("offset", xyz);
        return object;
    }

    /** 覆盖之后的结果不再由 tool 组件决定，组件报告里看不出来。 */
    private static boolean overridesCanDestroyBlock(Item item) throws NoSuchMethodException {
        return item.getClass()
                .getMethod("canDestroyBlock", ItemStack.class, BlockState.class, Level.class, BlockPos.class, LivingEntity.class)
                .getDeclaringClass() != Item.class;
    }

    /**
     * 每个状态一项的表和去重后的形状表。
     *
     * <p>全部用 EmptyBlockGetter 在 BlockPos.ZERO 读取，这样读到的是方块本身的性质。结果和位置有关的状态在这里
     * 发现：形状跟着渲染偏移走的方块把形状还原到偏移之前，运行时再加上偏移；其余的不一致直接报错。
     */
    private static final class StateTables {
        private final JsonArray facts = new JsonArray();
        private final JsonArray fluid = new JsonArray();
        private final JsonArray lightDampening = new JsonArray();
        private final JsonArray collision = new JsonArray();
        private final JsonArray outline = new JsonArray();
        private final JsonArray support = new JsonArray();
        private final JsonArray placementCollision = new JsonArray();
        private final JsonArray offsetSturdySamples = new JsonArray();
        private final JsonArray fluidStates = new JsonArray();
        private final Map<FluidState, Integer> fluidIndexes = new HashMap<>();
        private final JsonArray shapes = new JsonArray();
        private final JsonArray shapeFaces = new JsonArray();
        private final Map<String, Integer> shapeIndexes = new HashMap<>();
        private static final Map<Block, Boolean> MOVES_SHAPE = new HashMap<>();

        JsonObject build() {
            // 这两个形状的下标固定，读生成结果时好认。
            shapeIndex(Shapes.empty());
            shapeIndex(Shapes.block());
            for (int id = 0; id < Block.BLOCK_STATE_REGISTRY.size(); id++) {
                addState(id, Block.BLOCK_STATE_REGISTRY.byId(id));
            }

            JsonObject tables = new JsonObject();
            tables.add("facts", facts);
            tables.add("fluid", fluid);
            tables.add("lightDampening", lightDampening);
            tables.add("collision", collision);
            tables.add("outline", outline);
            tables.add("support", support);
            tables.add("placementCollision", placementCollision);
            tables.add("offsetSturdySamples", offsetSturdySamples);
            tables.add("fluidStates", fluidStates);
            tables.add("shapes", shapes);
            tables.add("shapeFaces", shapeFaces);
            return tables;
        }

        private void addState(int id, BlockState state) {
            boolean movesShape = movesShape(state.getBlock());
            checkPositionFacts(id, state, movesShape);

            facts.add(stateFacts(state, BlockPos.ZERO));
            fluid.add(fluidIndex(state.getFluidState()));
            lightDampening.add(state.getLightDampening());

            VoxelShape collisionShape = state.getCollisionShape(EMPTY, BlockPos.ZERO, CollisionContext.empty());
            collision.add(shapeIndex(canonical(state, collisionShape, movesShape)));
            outline.add(shapeIndex(canonical(state, state.getShape(EMPTY, BlockPos.ZERO), movesShape)));
            support.add(shapeIndex(canonical(state, state.getBlockSupportShape(EMPTY, BlockPos.ZERO), movesShape)));
            addPlacementCollision(id, state, collisionShape);
        }

        /** 放置检查用 placementContext，和普通上下文不同的状态单独列出来。 */
        private void addPlacementCollision(int id, BlockState state, VoxelShape collisionShape) {
            VoxelShape shape = state.getCollisionShape(EMPTY, BlockPos.ZERO, CollisionContext.placementContext(null));
            if (!shapeKey(shape).equals(shapeKey(collisionShape))) {
                JsonArray entry = new JsonArray();
                entry.add(id);
                entry.add(shapeIndex(shape));
                placementCollision.add(entry);
            }
        }

        /** 形状的渲染偏移是按坐标算的，这里还原到偏移之前，运行时再按真实坐标加上。 */
        private static VoxelShape canonical(BlockState state, VoxelShape shape, boolean movesShape) {
            if (!movesShape) {
                return shape;
            }
            Vec3 offset = state.getOffset(BlockPos.ZERO);
            return shape.move(-offset.x, -offset.y, -offset.z);
        }

        /** 这个方块有没有状态的形状随位置变化。 */
        static boolean movesShape(Block block) {
            return MOVES_SHAPE.computeIfAbsent(block, StateTables::findMovingShape);
        }

        private static boolean findMovingShape(Block block) {
            for (BlockState state : block.getStateDefinition().getPossibleStates()) {
                String expected = shapeKeys(state, BlockPos.ZERO);
                for (BlockPos pos : SAMPLE_POSITIONS) {
                    if (!shapeKeys(state, pos).equals(expected)) {
                        return true;
                    }
                }
            }
            return false;
        }

        private static String shapeKeys(BlockState state, BlockPos pos) {
            return shapeKey(state.getCollisionShape(EMPTY, pos, CollisionContext.empty()))
                    + "|" + shapeKey(state.getShape(EMPTY, pos))
                    + "|" + shapeKey(state.getBlockSupportShape(EMPTY, pos));
        }

        /** 形状可以随位置变，但只能是跟着偏移平移。其余事实不能变，变了说明生成的表不够用。 */
        private void checkPositionFacts(int id, BlockState state, boolean movesShape) {
            long expected = stateFacts(state, BlockPos.ZERO);
            Vec3 zeroOffset = state.getOffset(BlockPos.ZERO);
            long mask = movesShape ? ~STURDY_MASK : ~0L;
            for (BlockPos pos : SAMPLE_POSITIONS) {
                long found = stateFacts(state, pos);
                if ((found & mask) != (expected & mask)) {
                    throw new IllegalStateException(state + " 的事实和位置有关：" + pos);
                }
                if (movesShape) {
                    Vec3 delta = state.getOffset(pos).subtract(zeroOffset);
                    VoxelShape moved = state.getShape(EMPTY, BlockPos.ZERO).move(delta.x, delta.y, delta.z);
                    if (!shapeKey(moved).equals(shapeKey(state.getShape(EMPTY, pos)))) {
                        throw new IllegalStateException(state + " 的形状不只是跟着偏移平移：" + pos);
                    }
                    addOffsetSturdySample(id, pos, (int) (found & STURDY_MASK));
                }
            }
        }

        private void addOffsetSturdySample(int id, BlockPos pos, int found) {
            JsonArray sample = new JsonArray();
            sample.add(id);
            sample.add(pos.getX());
            sample.add(pos.getY());
            sample.add(pos.getZ());
            sample.add(found & STURDY_MASK);
            offsetSturdySamples.add(sample);
        }

        /** isSolid 和 liquid 官方标了过时，但方块类还在用，这里要原样读出来。 */
        @SuppressWarnings("deprecation")
        private static long stateFacts(BlockState state, BlockPos pos) {
            long facts = 0;
            for (SupportType support : SupportType.values()) {
                for (Direction direction : Direction.values()) {
                    if (state.isFaceSturdy(EMPTY, pos, direction, support)) {
                        facts |= 1 << (support.ordinal() * 6 + direction.get3DDataValue());
                    }
                }
            }
            facts |= state.isAir() ? FACT_AIR : 0;
            facts |= state.canOcclude() ? FACT_CAN_OCCLUDE : 0;
            facts |= state.isSolid() ? FACT_SOLID : 0;
            facts |= state.liquid() ? FACT_LIQUID : 0;
            facts |= state.canBeReplaced() ? FACT_REPLACEABLE : 0;
            facts |= state.isRedstoneConductor(EMPTY, pos) ? FACT_REDSTONE_CONDUCTOR : 0;
            facts |= state.isCollisionShapeFullBlock(EMPTY, pos) ? FACT_COLLISION_FULL_BLOCK : 0;
            facts |= state.isSolidRender() ? FACT_SOLID_RENDER : 0;
            facts |= state.isSignalSource() ? FACT_SIGNAL_SOURCE : 0;
            facts |= state.hasAnalogOutputSignal() ? FACT_ANALOG_OUTPUT : 0;
            facts |= state.propagatesSkylightDown() ? FACT_PROPAGATES_SKYLIGHT : 0;
            facts |= state.isRandomlyTicking() ? FACT_RANDOMLY_TICKING : 0;
            facts |= state.getFluidState().isRandomlyTicking() ? FACT_FLUID_RANDOMLY_TICKING : 0;
            // 下面是石头：不用形状挡光的完整方块，草方块、菌岩这些也是。
            BlockState below = Blocks.STONE.defaultBlockState();
            boolean blocksLight = LightEngine.getLightDampeningInto(below, state, Direction.UP, 0) == LIGHT_BLOCKED;
            facts |= blocksLight ? FACT_BLOCKS_LIGHT_FROM_BELOW : 0;
            facts |= state.useShapeForLightOcclusion() ? FACT_USE_SHAPE_FOR_LIGHT_OCCLUSION : 0;
            facts |= state.is(BlockTags.BLOCKS_MOTION_IN_HEIGHTMAP) ? FACT_MOTION_BLOCKING : 0;
            facts |= state.is(BlockTags.BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES) ? FACT_MOTION_BLOCKING_NO_LEAVES : 0;
            return facts;
        }

        private int fluidIndex(FluidState state) {
            return fluidIndexes.computeIfAbsent(state, added -> {
                JsonObject object = new JsonObject();
                object.addProperty("fluid", BuiltInRegistries.FLUID.getId(added.getType()));
                object.addProperty("source", added.isSource());
                object.addProperty("amount", added.getAmount());
                object.addProperty("falling", added.hasProperty(FlowingFluid.FALLING) && added.getValue(FlowingFluid.FALLING));
                object.addProperty("legacyBlock", Block.BLOCK_STATE_REGISTRY.getId(added.createLegacyBlock()));
                fluidStates.add(object);
                return fluidStates.size() - 1;
            });
        }

        private int shapeIndex(VoxelShape shape) {
            String key = shapeKey(shape);
            Integer known = shapeIndexes.get(key);
            if (known != null) {
                return known;
            }
            JsonArray boxes = new JsonArray();
            for (AABB box : shape.toAabbs()) {
                JsonArray numbers = new JsonArray();
                numbers.add(box.minX);
                numbers.add(box.minY);
                numbers.add(box.minZ);
                numbers.add(box.maxX);
                numbers.add(box.maxY);
                numbers.add(box.maxZ);
                boxes.add(numbers);
            }
            shapes.add(boxes);
            shapeFaces.add(faceFacts(shape));
            shapeIndexes.put(key, shapes.size() - 1);
            return shapes.size() - 1;
        }

        private static String shapeKey(VoxelShape shape) {
            return shape.toAabbs().toString();
        }

        /** 低 6 位是 Block.isFaceFull，高 6 位是这个面的切片不为空。Rust 的形状运算用它验证。 */
        private static int faceFacts(VoxelShape shape) {
            int facts = 0;
            for (Direction direction : Direction.values()) {
                if (Block.isFaceFull(shape, direction)) {
                    facts |= 1 << direction.get3DDataValue();
                }
                if (!shape.getFaceShape(direction).isEmpty()) {
                    facts |= 1 << (6 + direction.get3DDataValue());
                }
            }
            return facts;
        }
    }

    /**
     * 音效表：每个状态的 `getSoundType`、去重后的 `SoundType`、`sound_event` 注册表和音符盒乐器。
     *
     * <p>`SoundType` 的五个声音都是注册表 ID，Rust 侧直接拿来发 `sound` 包。注册表顺序就是网络上的
     * Holder ID，所以事件表按注册表顺序排列。
     */
    private static final class SoundTables {
        private final JsonArray stateTypes = new JsonArray();
        private final JsonArray types = new JsonArray();
        private final Map<List<Object>, Integer> typeIndexes = new HashMap<>();
        private final JsonArray events = new JsonArray();

        JsonObject build() {
            for (int id = 0; id < Block.BLOCK_STATE_REGISTRY.size(); id++) {
                stateTypes.add(soundTypeIndex(Block.BLOCK_STATE_REGISTRY.byId(id).getSoundType()));
            }
            for (SoundEvent event : BuiltInRegistries.SOUND_EVENT) {
                JsonObject entry = new JsonObject();
                entry.addProperty("name", BuiltInRegistries.SOUND_EVENT.getKey(event).toString());
                event.fixedRange().ifPresent(range -> entry.addProperty("fixedRange", range));
                events.add(entry);
            }

            JsonObject tables = new JsonObject();
            tables.add("stateTypes", stateTypes);
            tables.add("types", types);
            tables.add("events", events);
            tables.add("instruments", instruments());
            tables.add("sources", sources());
            return tables;
        }

        private int soundTypeIndex(SoundType type) {
            List<Object> key = List.of(
                    type.getVolume(),
                    type.getPitch(),
                    soundId(type.getBreakSound()),
                    soundId(type.getStepSound()),
                    soundId(type.getPlaceSound()),
                    soundId(type.getHitSound()),
                    soundId(type.getFallSound()));
            return typeIndexes.computeIfAbsent(key, ignored -> {
                JsonArray entry = new JsonArray();
                entry.add(type.getVolume());
                entry.add(type.getPitch());
                entry.add(soundId(type.getBreakSound()));
                entry.add(soundId(type.getStepSound()));
                entry.add(soundId(type.getPlaceSound()));
                entry.add(soundId(type.getHitSound()));
                entry.add(soundId(type.getFallSound()));
                types.add(entry);
                return types.size() - 1;
            });
        }

        private static int soundId(SoundEvent event) {
            return BuiltInRegistries.SOUND_EVENT.getId(event);
        }

        private static JsonArray instruments() {
            JsonArray instruments = new JsonArray();
            for (NoteBlockInstrument instrument : NoteBlockInstrument.values()) {
                JsonObject entry = new JsonObject();
                entry.addProperty("name", instrument.getSerializedName());
                entry.addProperty("sound", soundId(instrument.getSoundEvent().value()));
                entry.addProperty("tunable", instrument.isTunable());
                entry.addProperty("customSound", instrument.hasCustomSound());
                instruments.add(entry);
            }
            return instruments;
        }

        /** 声音来源的枚举，Rust 侧按序号发，名字只用来核对顺序。 */
        private static JsonArray sources() {
            JsonArray sources = new JsonArray();
            for (SoundSource source : SoundSource.values()) {
                sources.add(source.getName());
            }
            return sources;
        }
    }

    /**
     * 光照引擎要的表：每个状态的发光等级，和光照引擎看到的 6 个面的遮挡形状。
     *
     * <p>面的形状用 LightEngine.getOcclusionShape 取，它已经把 canOcclude 和 useShapeForLightOcclusion 算进去了。
     * 去重后两两之间的 Shapes.faceShapeOccludes 也在这里算好，Rust 只查表，不做形状运算。
     */
    private static final class LightTables {
        private final JsonArray emission = new JsonArray();
        private final JsonArray occlusion = new JsonArray();
        private final JsonArray occlusionFaces = new JsonArray();
        private final Map<String, Integer> occlusionIndexes = new HashMap<>();
        private final List<VoxelShape> faces = new ArrayList<>();
        private final Map<String, Integer> faceIndexes = new HashMap<>();

        JsonObject build() {
            // 空形状是 0，完整方块是 1，和 StateTables 的形状表一样固定下来。
            faceIndex(Shapes.empty());
            faceIndex(Shapes.block());
            occlusionIndex(new int[6]);
            for (int id = 0; id < Block.BLOCK_STATE_REGISTRY.size(); id++) {
                BlockState state = Block.BLOCK_STATE_REGISTRY.byId(id);
                emission.add(state.getLightEmission());
                occlusion.add(occlusionIndex(stateFaces(state)));
            }

            JsonObject tables = new JsonObject();
            tables.add("emission", emission);
            tables.add("occlusion", occlusion);
            tables.add("occlusionFaces", occlusionFaces);
            tables.addProperty("faceCount", faces.size());
            tables.add("faceOccludes", faceOccludes());
            return tables;
        }

        private int[] stateFaces(BlockState state) {
            int[] ids = new int[6];
            for (Direction direction : Direction.values()) {
                ids[direction.get3DDataValue()] = faceIndex(LightEngine.getOcclusionShape(state, direction));
            }
            return ids;
        }

        private int occlusionIndex(int[] ids) {
            return occlusionIndexes.computeIfAbsent(Arrays.toString(ids), key -> {
                JsonArray array = new JsonArray();
                for (int id : ids) {
                    array.add(id);
                }
                occlusionFaces.add(array);
                return occlusionFaces.size() - 1;
            });
        }

        private int faceIndex(VoxelShape face) {
            return faceIndexes.computeIfAbsent(face.toAabbs().toString(), key -> {
                faces.add(face);
                return faces.size() - 1;
            });
        }

        /** 第 a * faceCount + b 位是 faceShapeOccludes(a, b)，每 64 位一个 long，低位在前。 */
        private JsonArray faceOccludes() {
            int count = faces.size();
            long[] words = new long[(count * count + 63) / 64];
            for (int a = 0; a < count; a++) {
                for (int b = 0; b < count; b++) {
                    if (Shapes.faceShapeOccludes(faces.get(a), faces.get(b))) {
                        int bit = a * count + b;
                        words[bit / 64] |= 1L << (bit % 64);
                    }
                }
            }
            JsonArray array = new JsonArray();
            for (long word : words) {
                array.add(word);
            }
            return array;
        }
    }
}
