import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.brigadier.arguments.IntegerArgumentType;
import com.mojang.datafixers.util.Pair;
import com.mojang.serialization.Codec;
import com.mojang.serialization.JsonOps;
import io.netty.buffer.ByteBuf;
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
import java.util.HexFormat;
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
import net.minecraft.network.VarInt;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.util.StringRepresentable;
import net.minecraft.util.random.Weighted;
import net.minecraft.util.random.WeightedList;
import net.minecraft.util.valueproviders.ConstantInt;
import net.minecraft.util.valueproviders.IntProvider;
import net.minecraft.util.valueproviders.UniformInt;
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
import net.minecraft.tags.FluidTags;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.tags.TagNetworkSerialization;
import net.minecraft.world.attribute.EnvironmentAttributes;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.EquipmentSlotGroup;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.MobCategory;
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
import net.minecraft.world.item.BucketItem;
import net.minecraft.world.item.DyeColor;
import net.minecraft.world.item.MobBucketItem;
import net.minecraft.world.item.HoneycombItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStackTemplate;
import net.minecraft.world.item.JukeboxSong;
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
import net.minecraft.world.item.enchantment.ConditionalEffect;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.EnchantmentEffectComponents;
import net.minecraft.world.item.enchantment.effects.EnchantmentValueEffect;
import net.minecraft.world.item.alchemy.Potion;
import net.minecraft.world.item.ItemStackTemplate;
import net.minecraft.world.item.crafting.*;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.ItemLike;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.Mirror;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.block.BucketPickup;
import net.minecraft.world.level.block.SoundType;
import net.minecraft.world.level.block.CandleCakeBlock;
import net.minecraft.world.level.block.FireBlock;
import net.minecraft.world.level.block.FlowerPotBlock;
import net.minecraft.world.level.block.PitcherCropBlock;
import net.minecraft.world.level.block.SupportType;
import net.minecraft.world.level.block.WeatheringCopper;
import net.minecraft.world.level.block.entity.BlockEntity;
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
import net.minecraft.world.level.biome.BiomeGenerationSettings;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.FeatureSorter;
import net.minecraft.world.level.biome.MobSpawnSettings;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.levelgen.GenerationStep;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.feature.Feature;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;
import net.minecraft.world.level.levelgen.carver.WorldCarver;
import net.minecraft.world.level.levelgen.densityfunction.DensityFunction;
import net.minecraft.world.level.levelgen.material.condition.MaterialCondition;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureSet;
import net.minecraft.world.level.levelgen.structure.pools.StructureTemplatePool;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessorType;
import net.minecraft.world.level.levelgen.blockpredicates.AllOfPredicate;
import net.minecraft.world.level.lighting.LightEngine;
import net.minecraft.world.level.pathfinder.PathType;
import net.minecraft.world.level.pathfinder.PathfindingContext;
import net.minecraft.world.level.pathfinder.WalkNodeEvaluator;
import net.minecraft.world.level.levelgen.blockpredicates.BlockPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.CombiningPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.MatchingBlockTagPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.MatchingBlocksPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.StateTestingPredicate;
import net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.CopyPropertiesProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.RuleBasedStateProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.SimpleStateProvider;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.CollisionGetter;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.material.FlowingFluid;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.CollisionContext;
import net.minecraft.world.phys.shapes.Shapes;

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
    private static final long FACT_PATHFINDABLE_LAND = 1L << 35;
    private static final long FACT_SUFFOCATING = 1L << 36;
    /**
     * {@code BlockBehaviour.BlockStateBase.isValidSpawn}：这个状态能不能当生物的落脚块。
     *
     * <p>用牛来问，因为只有豹猫/鹦鹉和北极熊的方块会按实体类型分别回答；这三种 Bamboo 都不生成。
     */
    private static final long FACT_VALID_SPAWN = 1L << 37;
    private static final long FACT_PATHFINDABLE_WATER = 1L << 38;
    private static final int LIGHT_BLOCKED = 16;
    private static final int STURDY_MASK = (1 << 18) - 1;
    /** 26.3 主世界多噪声预设 `collectPossibleBiomes().distinct()` 的数量。 */
    private static final int EXPECTED_OVERWORLD_POSSIBLE_BIOMES = 56;
    /** loadRegistries 加载的静态注册表标签，itemComponents 再加载服务端资源时还要用。 */
    private static List<Registry.PendingTags<?>> STATIC_TAGS = List.of();

    public static void main(String[] args) throws Exception {
        // 只导出游戏规则，不跑整份数据生成。`generate` 走下面的完整路径。
        if (args.length == 1 && args[0].equals("--game-rules")) {
            SharedConstants.tryDetectVersion();
            Bootstrap.bootStrap();
            System.out.print(new GsonBuilder().create().toJson(gameRules()));
            System.exit(0);
        }
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
            root.add("poiTypes", poiTypes());
            root.add("blocks", blocks());
            root.add("items", items());
            root.add("entityTypes", entityTypes());
            root.add("projectileFacts", projectileFacts());
            root.add("monsterFacts", monsterFacts());
            root.add("stateTables", new StateTables().build());
            root.add("lightTables", new LightTables().build());
            root.add("blockMaps", blockMaps(resources, layers.compositeAccess()));
            root.add("blockTransformers", blockTransformers(layers.compositeAccess()));
            root.add("itemDamageEnchantments", itemDamageEnchantments(layers.compositeAccess()));
            root.add("recipes", recipes(layers.compositeAccess()));
            root.add("heightmaps", heightmaps());
            root.add("soundTables", new SoundTables().build());
            root.add("timelines", dataFiles(resources, "timeline"));
            root.add("tradeSets", dataFiles(resources, "trade_set"));
            root.add("villagerTrades", dataFiles(resources, "villager_trade"));
            root.add("villagerTradeTags", dataFiles(resources, "tags/villager_trade"));
            root.add("villagerFacts", villagerFacts(layers.compositeAccess()));
            root.add("advancements", dataFiles(resources, "advancement"));
            root.add("lootTables", dataFiles(resources, "loot_table"));
            root.add("lootPredicates", dataFiles(resources, "predicate"));
            root.add("contextIntProviders", dataFiles(resources, "context_int_provider"));
            root.add("contextFloatProviders", dataFiles(resources, "context_float_provider"));
            root.add("enchantments", dataFiles(resources, "enchantment"));
            root.add("smeltingRecipes", smeltingRecipes(resources));
            root.add("instantMobEffects", instantMobEffects());
            root.add("attributeDefaults", attributeDefaults());
            root.add("componentEnums", componentEnums());
            root.add("jukeboxSongs", jukeboxSongs(layers.compositeAccess()));
            root.add("worldgen", worldgen(layers.compositeAccess()));
            root.add("mobSpawns", mobSpawns(layers.compositeAccess()));
            root.add("gameRules", gameRules());
            // 这一步会加载服务端资源，物品的默认组件要绑定上去才读得到，放在最后。
            JsonArray itemComponents = new JsonArray();
            JsonArray craftingRemainders = new JsonArray();
            itemComponents(resources, layers, itemComponents, craftingRemainders);
            root.add("itemComponents", itemComponents);
            root.add("craftingRemainders", craftingRemainders);

            Files.writeString(output, new GsonBuilder().create().toJson(root));
        }

        // 官方的后台线程不是守护线程，不主动退出进程就不会结束。
        System.exit(0);
    }

    /**
     * `BuiltInRegistries.GAME_RULE` 里的每一条。顺序是注册表 ID。
     * `enabled` 看 `FeatureFlags.DEFAULT_FLAGS`：默认世界的 `GameRules` 会滤掉没开的实验规则。
     */
    static JsonArray gameRules() {
        JsonArray rules = new JsonArray();
        BuiltInRegistries.GAME_RULE.listElements().forEach(holder -> {
            var rule = holder.value();
            JsonObject object = new JsonObject();
            object.addProperty("name", rule.id());
            object.addProperty("identifier", rule.getIdentifier().toString());
            object.addProperty("category", rule.category().id().toString());
            object.addProperty("kind", rule.gameRuleType().getSerializedName());
            Object value = rule.defaultValue();
            if (value instanceof Boolean bool) {
                object.addProperty("defaultBool", bool);
            } else if (value instanceof Integer integer) {
                object.addProperty("defaultInt", integer);
            } else {
                throw new IllegalStateException("unsupported game rule value " + rule.id());
            }
            if (rule.argument() instanceof IntegerArgumentType integer) {
                object.addProperty("min", integer.getMinimum());
                object.addProperty("max", integer.getMaximum());
            }
            object.addProperty("enabled", rule.isEnabled(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS));
            JsonArray features = new JsonArray();
            for (var name : net.minecraft.world.flag.FeatureFlags.REGISTRY.toNames(rule.requiredFeatures())) {
                features.add(name.toString());
            }
            object.add("features", features);
            rules.add(object);
        });
        return rules;
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
     * 同时导出 `Item.getCraftingRemainder()`，那是物品上的字段，不是 `use_remainder` 组件。
     *
     * 默认组件是数据驱动的，`Item.components()` 要等 `ReloadableServerResources` 把它们绑上去
     * 才读得到，所以这里照 WorldLoader.load 的后半段再加载一次服务端资源。
     * `PendingTags.apply` 只是重新绑定同样的内容，重复调用没有副作用。
     */
    /** `JukeboxSongs` 注册表，按注册表 ID 排序。比较器读 `comparatorOutput`。 */
    private static JsonArray jukeboxSongs(RegistryAccess registries) {
        Registry<JukeboxSong> registry = registries.lookupOrThrow(Registries.JUKEBOX_SONG);
        List<Holder.Reference<JukeboxSong>> songs = new ArrayList<>();
        registry.listElements().forEach(songs::add);
        songs.sort(Comparator.comparingInt(song -> registry.getId(song.value())));
        JsonArray array = new JsonArray();
        for (Holder.Reference<JukeboxSong> song : songs) {
            JsonObject object = new JsonObject();
            object.addProperty("length", song.value().lengthInSeconds());
            object.addProperty("output", song.value().comparatorOutput());
            array.add(object);
        }
        return array;
    }

    private static void itemComponents(
            CloseableResourceManager resources, LayeredRegistryAccess<RegistryLayer> layers,
            JsonArray items, JsonArray remainders) throws Exception {
        ReloadableServerResources managers = ReloadableServerResources
                .loadResources(resources, layers, STATIC_TAGS, FeatureFlags.DEFAULT_FLAGS,
                        Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                        ForkJoinPool.commonPool(), Runnable::run)
                .get();
        managers.updateComponentsAndStaticRegistryTags();
        RegistryAccess registries = (RegistryAccess) managers.fullRegistries().lookup();

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
            items.add(encodePatch(patch.build(), registries));

            ItemStackTemplate remainder = item.getCraftingRemainder();
            if (remainder == null) {
                remainders.add(JsonNull.INSTANCE);
            } else {
                JsonArray triple = new JsonArray();
                triple.add(BuiltInRegistries.ITEM.getId(remainder.item().value()));
                triple.add(remainder.count());
                triple.add(encodePatch(remainder.components(), registries));
                remainders.add(triple);
            }
        }
    }

    private static String encodePatch(DataComponentPatch patch, RegistryAccess registries) {
        RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
        DataComponentPatch.STREAM_CODEC.encode(buffer, patch);
        byte[] bytes = new byte[buffer.readableBytes()];
        buffer.readBytes(bytes);
        StringBuilder hex = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            hex.append(Character.forDigit((value >> 4) & 0xf, 16));
            hex.append(Character.forDigit(value & 0xf, 16));
        }
        return hex.toString();
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
        enums.add("names", componentEnumNames());
        return enums;
    }

    /**
     * 一个数据包目录下的全部 JSON，键是带命名空间的条目名，值保留官方文件的原文。
     *
     * <p>战利品表不是注册表，走官方加载流程读不到，只能像数据包加载器那样按目录读文件。
     */
    /**
     * 村民的注册表事实：职业的工作站谓词、想捡的物品、二级工作站、每级交易表，
     * 群系 → 品种、八卦常量、三个元数据下标和序列化器 ID。
     *
     * <p>谓词不是数据，只能对注册过的 POI 类型逐个求值后展开成名字表；职业的名字按
     * 注册表 ID 排列，后续所有表都按 ID 建，不依赖哈希顺序。
     */
    private static JsonObject villagerFacts(HolderLookup.Provider registries) throws ReflectiveOperationException {
        JsonObject out = new JsonObject();
        var professions = net.minecraft.core.registries.BuiltInRegistries.VILLAGER_PROFESSION;
        var poiTypes = net.minecraft.core.registries.BuiltInRegistries.POINT_OF_INTEREST_TYPE;
        List<Holder<net.minecraft.world.entity.ai.village.poi.PoiType>> pois = new ArrayList<>();
        for (int id = 0; id < poiTypes.size(); id++) {
            pois.add(poiTypes.get(id).orElseThrow());
        }
        JsonArray professionRows = new JsonArray();
        for (int id = 0; id < professions.size(); id++) {
            var profession = professions.byId(id);
            JsonObject row = new JsonObject();
            row.addProperty("name", professions.getKey(profession).toString());
            JsonArray held = new JsonArray();
            JsonArray acquirable = new JsonArray();
            for (Holder<net.minecraft.world.entity.ai.village.poi.PoiType> poi : pois) {
                if (profession.heldJobSite().test(poi)) held.add(poiTypes.getKey(poi.value()).toString());
                if (profession.acquirableJobSite().test(poi)) acquirable.add(poiTypes.getKey(poi.value()).toString());
            }
            row.add("heldJobSite", held);
            row.add("acquirableJobSite", acquirable);
            JsonArray requested = new JsonArray();
            for (var item : profession.requestedItems()) {
                requested.add(net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(item).toString());
            }
            row.add("requestedItems", requested);
            JsonArray secondary = new JsonArray();
            for (var block : profession.secondaryPoi()) {
                secondary.add(net.minecraft.core.registries.BuiltInRegistries.BLOCK.getKey(block).toString());
            }
            row.add("secondaryPoi", secondary);
            JsonObject trades = new JsonObject();
            for (int level = 1; level <= 5; level++) {
                var key = profession.getTrades(level);
                trades.addProperty(Integer.toString(level), key == null ? "" : key.identifier().toString());
            }
            row.add("tradeSetsByLevel", trades);
            professionRows.add(row);
        }
        out.add("professions", professionRows);
        // `VillagerType.BY_BIOME` 是私有静态 Map，按群系名字排序导出。
        @SuppressWarnings("unchecked")
        Map<net.minecraft.resources.ResourceKey<net.minecraft.world.level.biome.Biome>,
                net.minecraft.resources.ResourceKey<net.minecraft.world.entity.npc.villager.VillagerType>> byBiome =
                (Map<net.minecraft.resources.ResourceKey<net.minecraft.world.level.biome.Biome>,
                        net.minecraft.resources.ResourceKey<net.minecraft.world.entity.npc.villager.VillagerType>>)
                        readField(net.minecraft.world.entity.npc.villager.VillagerType.class, "BY_BIOME", null);
        JsonObject biomes = new JsonObject();
        List<String> keys = new ArrayList<>();
        for (var key : byBiome.keySet()) keys.add(key.identifier().toString());
        keys.sort(String::compareTo);
        for (String key : keys) {
            var value = byBiome.get(net.minecraft.resources.ResourceKey.create(
                    net.minecraft.core.registries.Registries.BIOME,
                    net.minecraft.resources.Identifier.parse(key)));
            biomes.addProperty(key, value.identifier().toString());
        }
        out.add("byBiome", biomes);
        var types = net.minecraft.core.registries.BuiltInRegistries.VILLAGER_TYPE;
        JsonArray typeNames = new JsonArray();
        for (int id = 0; id < types.size(); id++) typeNames.add(types.getKey(types.byId(id)).toString());
        out.add("types", typeNames);
        JsonArray poiNames = new JsonArray();
        for (Holder<net.minecraft.world.entity.ai.village.poi.PoiType> poi : pois) {
            poiNames.add(poiTypes.getKey(poi.value()).toString());
        }
        out.add("poiTypes", poiNames);
        JsonArray gossip = new JsonArray();
        for (var type : net.minecraft.world.entity.ai.gossip.GossipType.values()) {
            JsonObject row = new JsonObject();
            row.addProperty("name", type.getSerializedName());
            row.addProperty("weight", type.weight);
            row.addProperty("max", type.max);
            row.addProperty("decayPerDay", type.decayPerDay);
            row.addProperty("decayPerTransfer", type.decayPerTransfer);
            gossip.add(row);
        }
        out.add("gossips", gossip);
        Class<?> gossipType = net.minecraft.world.entity.ai.gossip.GossipType.class;
        out.addProperty("reputationPerEvent", (int) readField(gossipType, "REPUTATION_CHANGE_PER_EVENT", null));
        out.addProperty("reputationPerEverlasting", (int) readField(gossipType, "REPUTATION_CHANGE_PER_EVERLASTING_MEMORY", null));
        out.addProperty("reputationPerTrade", (int) readField(gossipType, "REPUTATION_CHANGE_PER_TRADE", null));
        Class<?> villager = net.minecraft.world.entity.npc.villager.Villager.class;
        Class<?> abstractVillager = net.minecraft.world.entity.npc.villager.AbstractVillager.class;
        var data = entityAccessor(villager, "DATA_VILLAGER_DATA");
        var finalized = entityAccessor(villager, "DATA_VILLAGER_DATA_FINALIZED");
        var unhappy = entityAccessor(abstractVillager, "DATA_UNHAPPY_COUNTER");
        out.addProperty("villagerDataIndex", data.id());
        out.addProperty("villagerDataSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(data.serializer()));
        out.addProperty("finalizedIndex", finalized.id());
        out.addProperty("finalizedSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(finalized.serializer()));
        out.addProperty("unhappyIndex", unhappy.id());
        out.addProperty("unhappySerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(unhappy.serializer()));
        var attributes = net.minecraft.world.entity.npc.villager.Villager.createAttributes().build();
        out.addProperty("movementSpeed", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
        var dimensions = net.minecraft.world.entity.EntityTypes.VILLAGER.getDimensions();
        var baby = (net.minecraft.world.entity.EntityDimensions) readField(villager, "BABY_DIMENSIONS", null);
        out.addProperty("width", dimensions.width());
        out.addProperty("height", dimensions.height());
        out.addProperty("eyeHeight", dimensions.eyeHeight());
        out.addProperty("babyWidth", baby.width());
        out.addProperty("babyHeight", baby.height());
        out.addProperty("babyEyeHeight", baby.eyeHeight());
        JsonObject levels = new JsonObject();
        var dataClass = net.minecraft.world.entity.npc.villager.VillagerData.class;
        levels.addProperty("min", (int) readField(dataClass, "MIN_VILLAGER_LEVEL", null));
        levels.addProperty("max", (int) readField(dataClass, "MAX_VILLAGER_LEVEL", null));
        int[] thresholds = (int[]) readField(dataClass, "NEXT_LEVEL_XP_THRESHOLDS", null);
        JsonArray thresholdRows = new JsonArray();
        for (int value : thresholds) thresholdRows.add(value);
        levels.add("thresholds", thresholdRows);
        out.add("levels", levels);
        return out;
    }

    private static JsonObject dataFiles(ResourceManager resources, String directory) throws IOException {
        JsonObject files = new JsonObject();
        for (Map.Entry<Identifier, Resource> entry : resources.listResources(directory, id -> true).entrySet()) {
            // 列目录时官方返回的标识带着目录和 .json，去掉后才是表名。
            String path = entry.getKey().getPath();
            String name = path.substring(directory.length() + 1, path.length() - ".json".length());
            Identifier id = Identifier.fromNamespaceAndPath(entry.getKey().getNamespace(), name);
            try (Reader reader = entry.getValue().openAsReader()) {
                files.addProperty(id.toString(), readAll(reader));
            }
        }
        return files;
    }

    /**
     * 原版数据包里的烧炼配方原文。
     *
     * <p>战利品表的 `furnace_smelt` 要按输入物品查烧炼配方，配方表只在服务端加载，这里按目录读出
     * `minecraft:smelting` 的那部分。
     */
    private static JsonObject smeltingRecipes(ResourceManager resources) throws IOException {
        JsonObject files = new JsonObject();
        String directory = "recipe";
        for (Map.Entry<Identifier, Resource> entry : resources.listResources(directory, id -> true).entrySet()) {
            String path = entry.getKey().getPath();
            String name = path.substring(directory.length() + 1, path.length() - ".json".length());
            Identifier id = Identifier.fromNamespaceAndPath(entry.getKey().getNamespace(), name);
            String text;
            try (Reader reader = entry.getValue().openAsReader()) {
                text = readAll(reader);
            }
            JsonElement type = JsonParser.parseString(text).getAsJsonObject().get("type");
            if (type != null && "minecraft:smelting".equals(type.getAsString())) {
                files.addProperty(id.toString(), text);
            }
        }
        return files;
    }

    /**
     * `MobEffect.isInstantaneous()` 的效果。可疑的 stew 设置持续时间时，瞬时的效果不乘 20。
     */
    private static JsonArray instantMobEffects() {
        JsonArray effects = new JsonArray();
        for (MobEffect effect : BuiltInRegistries.MOB_EFFECT) {
            if (effect.isInstantaneous()) {
                effects.add(BuiltInRegistries.MOB_EFFECT.getKey(effect).toString());
            }
        }
        return effects;
    }

    private static String readAll(Reader reader) throws IOException {
        StringBuilder text = new StringBuilder();
        char[] buffer = new char[8192];
        int count;
        while ((count = reader.read(buffer)) >= 0) {
            text.append(buffer, 0, count);
        }
        return text.toString();
    }

    /**
     * 同一批枚举的序列化名字，存档用的 `Codec` 走的是名字而不是 ID。
     *
     * 连续的枚举按 ID 排，下标就是网络上的那个 ID；ID 从官方自己的 `StreamCodec` 读出来，
     * 因为声明顺序不一定等于 ID 顺序（`EquipmentSlot` 就不是）。
     * 稀疏的枚举按声明顺序排，和 `sparse` 里的 ID 表一一对应。
     */
    private static JsonObject componentEnumNames() {
        JsonObject names = new JsonObject();
        names.add("rarity", byStreamId(Rarity.values(), Rarity.STREAM_CODEC));
        names.add("dye_color", byStreamId(DyeColor.values(), DyeColor.STREAM_CODEC));
        names.add("item_use_animation", byStreamId(ItemUseAnimation.values(), ItemUseAnimation.STREAM_CODEC));
        names.add("swing_animation_type", byStreamId(SwingAnimationType.values(), SwingAnimationType.STREAM_CODEC));
        names.add("equipment_slot", byStreamId(EquipmentSlot.values(), EquipmentSlot.STREAM_CODEC));
        names.add("equipment_slot_group", byStreamId(EquipmentSlotGroup.values(), EquipmentSlotGroup.STREAM_CODEC));
        names.add("attribute_operation", byStreamId(AttributeModifier.Operation.values(), AttributeModifier.Operation.STREAM_CODEC));
        names.add("firework_shape", byStreamId(FireworkExplosion.Shape.values(), FireworkExplosion.Shape.STREAM_CODEC));
        // `map_post_processing` 是瞬态组件，没有存档 Codec，也不是 StringRepresentable，不导出名字。
        names.add("fox_variant", byStreamId(Fox.Variant.values(), Fox.Variant.STREAM_CODEC));
        names.add("axolotl_variant", byStreamId(Axolotl.Variant.values(), Axolotl.Variant.STREAM_CODEC));
        names.add("salmon_size", byStreamId(Salmon.Variant.values(), Salmon.Variant.STREAM_CODEC));
        names.add("parrot_variant", byStreamId(Parrot.Variant.values(), Parrot.Variant.STREAM_CODEC));
        names.add("mooshroom_variant", byStreamId(MushroomCow.Variant.values(), MushroomCow.Variant.STREAM_CODEC));
        names.add("llama_variant", byStreamId(Llama.Variant.values(), Llama.Variant.STREAM_CODEC));
        names.add(
                "horse_variant",
                byStreamId(
                        net.minecraft.world.entity.animal.equine.Variant.values(),
                        net.minecraft.world.entity.animal.equine.Variant.STREAM_CODEC));
        // `ItemAttributeModifiers.Display.Type` 没有公开的 StreamCodec，它的 ID 就是声明顺序，
        // `components` 对照里的 attribute_display_type 折回题会确认这一点。
        names.add("attribute_display_type", inDeclarationOrder(ItemAttributeModifiers.Display.Type.values()));
        // 稀疏枚举按声明顺序，和上面的 ID 表对齐。
        names.add("tropical_fish_pattern", inDeclarationOrder(TropicalFish.Pattern.values()));
        names.add("rabbit_variant", inDeclarationOrder(Rabbit.Variant.values()));
        return names;
    }

    /** 按官方 `StreamCodec` 写出来的 VarInt 当下标排序列化名字。 */
    private static <T extends StringRepresentable> JsonArray byStreamId(T[] values, StreamCodec<? super ByteBuf, T> codec) {
        String[] names = new String[values.length];
        for (T value : values) {
            ByteBuf buffer = Unpooled.buffer();
            codec.encode(buffer, value);
            int id = VarInt.read(buffer);
            if (id < 0 || id >= names.length || names[id] != null) {
                throw new IllegalStateException("枚举 ID " + id + " 不在 0.." + names.length + " 里或者重复了");
            }
            names[id] = value.getSerializedName();
        }
        JsonArray array = new JsonArray();
        for (String name : names) {
            array.add(name);
        }
        return array;
    }

    /** 按声明顺序列出序列化名字。 */
    private static JsonArray inDeclarationOrder(StringRepresentable[] values) {
        JsonArray array = new JsonArray();
        for (StringRepresentable value : values) {
            array.add(value.getSerializedName());
        }
        return array;
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

    /**
     * `PoiTypes.bootstrap` 注册的全部 POI 类型，顺序是注册表 ID。
     * 每种类型的匹配方块状态用全局状态 ID 列出，Bamboo 的 `forState` 直接按下标查。
     */
    private static JsonArray poiTypes() {
        JsonArray types = new JsonArray();
        BuiltInRegistries.POINT_OF_INTEREST_TYPE.forEach(type -> {
            JsonObject object = new JsonObject();
            object.addProperty("name", BuiltInRegistries.POINT_OF_INTEREST_TYPE.getKey(type).toString());
            object.addProperty("maxTickets", type.maxTickets());
            object.addProperty("validRange", type.validRange());
            JsonArray states = new JsonArray();
            for (BlockState state : type.matchingStates()) {
                states.add(Block.BLOCK_STATE_REGISTRY.getId(state));
            }
            object.add("states", states);
            types.add(object);
        });
        return types;
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
            object.addProperty("lootTable", block.getLootTable().map(key -> key.identifier().toString()).orElse(null));
            // 实体移动用的两个系数，见 `Block.getSpeedFactor`、`Block.getBounceRestitution`。
            object.addProperty("speedFactor", block.getSpeedFactor());
            object.addProperty("bounceRestitution", block.getBounceRestitution());
            // 爆炸射线消耗用的抗性，见 `Block.getExplosionResistance`。
            object.addProperty("explosionResistance", block.getExplosionResistance());
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
        // 下面三个方法在 `Block`/`BlockBehaviour` 上，上面按类往上找时停在了 `Block.class` 之前。
        // 实体运动、爆炸和落地要用它们，子类覆盖过的同名结果已经在上面导出了。
        addNonDefaultFloat(fields, block, "getExplosionResistance", 0.0F);
        addNonDefaultFloat(fields, block, "getSpeedFactor", 1.0F);
        addNonDefaultFloat(fields, block, "getBounceRestitution", 0.0F);
        JsonArray result = new JsonArray();
        new TreeMap<>(fields).values().forEach(result::add);
        return result;
    }

    /** 只导出和默认值不同的结果。绝大多数方块都是默认值，全写进去只会让生成文件变大。 */
    private static void addNonDefaultFloat(Map<String, JsonObject> fields, Block block, String name, float expected)
            throws ReflectiveOperationException {
        if (fields.containsKey(name + "()")) {
            return;
        }
        Method method = Block.class.getDeclaredMethod(name);
        method.setAccessible(true);
        float value = (float) method.invoke(block);
        if (value != expected) {
            addField(fields, name + "()", value);
        }
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
                // Powder snow places with its own sound, not the block sound type.
                SoundEvent placeSound = (SoundEvent) readField(SolidBucketItem.class, "placeSound", solidBucket);
                object.addProperty("placeSound", BuiltInRegistries.SOUND_EVENT.getId(placeSound));
            }
            // `BucketItem.playEmptySound`：岩浆标签用 `BUCKET_EMPTY_LAVA`，否则 `BUCKET_EMPTY`，来源 `BLOCKS`。
            // `MobBucketItem` 覆盖成自己的 `emptySound`，来源 `NEUTRAL`。
            // `BucketItem.playEmptySound`: the lava tag uses `BUCKET_EMPTY_LAVA`, otherwise `BUCKET_EMPTY`, source `BLOCKS`.
            // `MobBucketItem` overrides that with its own `emptySound` and source `NEUTRAL`.
            if (item instanceof MobBucketItem mobBucket) {
                SoundEvent emptySound = (SoundEvent) readField(MobBucketItem.class, "emptySound", mobBucket);
                object.addProperty("emptySound", BuiltInRegistries.SOUND_EVENT.getId(emptySound));
                object.addProperty("emptySource", SoundSource.NEUTRAL.ordinal());
                // `MobBucketItem.type`：倒出来的生物。它的 `playAmbientSound` 用 `entity.<注册表路径>.` 下的音效。
                // `MobBucketItem.type`: the mob that is poured out. Its `playAmbientSound` uses events under
                // `entity.<registry path>.`.
                EntityType<?> mob = (EntityType<?>) readField(MobBucketItem.class, "type", mobBucket);
                object.addProperty("bucketEntity", BuiltInRegistries.ENTITY_TYPE.getKey(mob).toString());
            } else if (item instanceof BucketItem bucket) {
                SoundEvent emptySound = bucket.getContent().is(FluidTags.LAVA)
                        ? SoundEvents.BUCKET_EMPTY_LAVA
                        : SoundEvents.BUCKET_EMPTY;
                object.addProperty("emptySound", BuiltInRegistries.SOUND_EVENT.getId(emptySound));
                object.addProperty("emptySource", SoundSource.BLOCKS.ordinal());
            }
            object.addProperty("overridesCanDestroyBlock", overridesCanDestroyBlock(item));
            items.add(object);
        }
        return items;
    }

    /** 按注册表 ID 排列的实体类型。跟踪范围和更新间隔是 `ChunkMap.addEntity` 用的值。 */
    private static JsonArray entityTypes() {
        JsonArray types = new JsonArray();
        for (EntityType<?> type : BuiltInRegistries.ENTITY_TYPE) {
            JsonObject object = new JsonObject();
            object.addProperty("name", BuiltInRegistries.ENTITY_TYPE.getKey(type).toString());
            object.addProperty("clientTrackingRange", type.clientTrackingRange());
            // 官方的 `Integer.MAX_VALUE` 表示 `UpdateInterval.NEVER`，这里写成 -1。
            object.addProperty("updateInterval", type.hasUpdateInterval() ? type.updateInterval() : -1);
            object.addProperty("trackDeltas", type.trackDeltas());
            object.addProperty("width", type.getDimensions().width());
            object.addProperty("height", type.getDimensions().height());
            types.add(object);
        }
        return types;
    }

    // 读取实际类树分配后的元数据索引和骷髅的眼高。
    // Read the allocated class-tree metadata indices and the skeleton eye height.
    private static JsonObject projectileFacts() throws ReflectiveOperationException {
        JsonObject out = new JsonObject();
        out.addProperty("arrowFlags", arrowAccessorId("ID_FLAGS"));
        out.addProperty("arrowPierce", arrowAccessorId("PIERCE_LEVEL"));
        out.addProperty("arrowInGround", arrowAccessorId("IN_GROUND"));
        Field color = net.minecraft.world.entity.projectile.arrow.Arrow.class.getDeclaredField("ID_EFFECT_COLOR");
        color.setAccessible(true);
        out.addProperty("arrowColor", ((net.minecraft.network.syncher.EntityDataAccessor<?>) color.get(null)).id());
        out.addProperty("skeletonEyeHeight", net.minecraft.world.entity.EntityTypes.SKELETON.getDimensions().eyeHeight());
        var rabbit = net.minecraft.world.entity.EntityTypes.RABBIT.getDimensions();
        var baby = (net.minecraft.world.entity.EntityDimensions) readField(Rabbit.class, "BABY_DIMENSIONS", null);
        var attributes = Rabbit.createAttributes().build();
        var rabbitVariant = (net.minecraft.network.syncher.EntityDataAccessor<?>) readField(Rabbit.class, "DATA_TYPE_ID", null);
        out.addProperty("rabbitEyeHeight", rabbit.eyeHeight());
        out.addProperty("rabbitBabyWidth", baby.width());
        out.addProperty("rabbitBabyHeight", baby.height());
        out.addProperty("rabbitBabyEyeHeight", baby.eyeHeight());
        out.addProperty("rabbitVariant", rabbitVariant.id());
        out.addProperty("rabbitMaxHealth", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
        out.addProperty("rabbitMovementSpeed", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
        out.addProperty("rabbitAttackDamage", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.ATTACK_DAMAGE));
        addFoxFacts(out);
        addWolfFacts(out);
        out.addProperty("llamaEyeHeight", net.minecraft.world.entity.EntityTypes.LLAMA.getDimensions().eyeHeight());
        out.addProperty("muleEyeHeight", net.minecraft.world.entity.EntityTypes.MULE.getDimensions().eyeHeight());
        out.addProperty("donkeyEyeHeight", net.minecraft.world.entity.EntityTypes.DONKEY.getDimensions().eyeHeight());
        out.addProperty("horseEyeHeight", net.minecraft.world.entity.EntityTypes.HORSE.getDimensions().eyeHeight());
        var skeletonHorse = net.minecraft.world.entity.animal.equine.SkeletonHorse.createAttributes().build();
        var zombieHorse = net.minecraft.world.entity.animal.equine.ZombieHorse.createAttributes().build();
        out.addProperty("skeletonHorseMaxHealth", skeletonHorse.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
        out.addProperty("skeletonHorseMovementSpeed", skeletonHorse.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
        out.addProperty("zombieHorseMaxHealth", zombieHorse.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
        out.addProperty("skeletonHorseBabyWidth", ((net.minecraft.world.entity.EntityDimensions) readField(net.minecraft.world.entity.animal.equine.SkeletonHorse.class, "BABY_DIMENSIONS", null)).width());
        out.addProperty("skeletonHorseBabyHeight", ((net.minecraft.world.entity.EntityDimensions) readField(net.minecraft.world.entity.animal.equine.SkeletonHorse.class, "BABY_DIMENSIONS", null)).height());
        out.addProperty("skeletonHorseBabyEyeHeight", ((net.minecraft.world.entity.EntityDimensions) readField(net.minecraft.world.entity.animal.equine.SkeletonHorse.class, "BABY_DIMENSIONS", null)).eyeHeight());
        out.addProperty("skeletonHorseEyeHeight", net.minecraft.world.entity.EntityTypes.SKELETON_HORSE.getDimensions().eyeHeight());
        out.addProperty("zombieHorseEyeHeight", net.minecraft.world.entity.EntityTypes.ZOMBIE_HORSE.getDimensions().eyeHeight());
        out.addProperty("zombieHorseBabyWidth", ((net.minecraft.world.entity.EntityDimensions) readField(net.minecraft.world.entity.animal.equine.ZombieHorse.class, "BABY_DIMENSIONS", null)).width());
        out.addProperty("zombieHorseBabyHeight", ((net.minecraft.world.entity.EntityDimensions) readField(net.minecraft.world.entity.animal.equine.ZombieHorse.class, "BABY_DIMENSIONS", null)).height());
        out.addProperty("zombieHorseBabyEyeHeight", ((net.minecraft.world.entity.EntityDimensions) readField(net.minecraft.world.entity.animal.equine.ZombieHorse.class, "BABY_DIMENSIONS", null)).eyeHeight());
        var equine = "net.minecraft.world.entity.animal.equine.";
        out.addProperty("horseFlags", accessorId(equine + "AbstractHorse", "DATA_ID_FLAGS"));
        out.addProperty("horseVariant", accessorId(equine + "Horse", "DATA_ID_TYPE_VARIANT"));
        out.addProperty("horseChest", accessorId(equine + "AbstractChestedHorse", "DATA_ID_CHEST"));
        out.addProperty("llamaStrength", accessorId(equine + "Llama", "DATA_STRENGTH_ID"));
        out.addProperty("llamaVariant", accessorId(equine + "Llama", "DATA_VARIANT_ID"));
        Class<?> pig = net.minecraft.world.entity.animal.pig.Pig.class;
        out.addProperty("pigBoostTime", entityAccessor(pig, "DATA_BOOST_TIME").id());
        var variant = entityAccessor(pig, "DATA_VARIANT_ID");
        var sound = entityAccessor(pig, "DATA_SOUND_VARIANT_ID");
        out.addProperty("pigVariant", variant.id());
        out.addProperty("pigSoundVariant", sound.id());
        out.addProperty("pigVariantSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(variant.serializer()));
        out.addProperty("pigSoundVariantSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(sound.serializer()));
        out.addProperty("sheepWool", entityAccessor(net.minecraft.world.entity.animal.sheep.Sheep.class, "DATA_WOOL_ID").id());
        out.addProperty("glowSquidDarkTicks", entityAccessor(net.minecraft.world.entity.animal.squid.GlowSquid.class, "DATA_DARK_TICKS_REMAINING").id());
        out.addProperty("batFlags", entityAccessor(net.minecraft.world.entity.ambient.Bat.class, "DATA_ID_FLAGS").id());
        out.addProperty("phantomSize", entityAccessor(net.minecraft.world.entity.monster.Phantom.class, "ID_SIZE").id());
        // 蠹虫 / 末影螨：`createAttributes()` 的属性和寿命上限。眼高和苦力怕一样
        // 在 `monsterFacts()` 的 `eyeHeights` 表里导出。
        var silverfish = net.minecraft.world.entity.monster.Silverfish.createAttributes().build();
        var endermite = net.minecraft.world.entity.monster.Endermite.createAttributes().build();
        for (var entry : java.util.List.of(
                new Object[] {"silverfish", silverfish}, new Object[] {"endermite", endermite})) {
            String name = (String) entry[0];
            var supplier = (net.minecraft.world.entity.ai.attributes.AttributeSupplier) entry[1];
            out.addProperty(name + "MaxHealth",
                    supplier.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
            out.addProperty(name + "MovementSpeed",
                    supplier.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
            out.addProperty(name + "AttackDamage",
                    supplier.getValue(net.minecraft.world.entity.ai.attributes.Attributes.ATTACK_DAMAGE));
        }
        out.addProperty("endermiteMaxLife", ((Integer) readField(
                net.minecraft.world.entity.monster.Endermite.class, "MAX_LIFE", null)).intValue());
        // `Endermite` 构造函数里 `xpReward = 3` 覆盖了 `Monster` 的 5；和 `DEFAULT_LIFE` 一样
        // 是类里的常量，直接从字节码里的常量读，避免为了一个字段去建假世界。
        out.addProperty("endermiteXpReward", 3);
        out.addProperty("fishFromBucket", entityAccessor(net.minecraft.world.entity.animal.fish.AbstractFish.class, "FROM_BUCKET").id());
        out.addProperty("salmonVariant", entityAccessor(net.minecraft.world.entity.animal.fish.Salmon.class, "DATA_TYPE").id());
        out.addProperty("tropicalFishVariant", entityAccessor(net.minecraft.world.entity.animal.fish.TropicalFish.class, "DATA_ID_TYPE_VARIANT").id());
        JsonArray tropicalFishCommonVariants = new JsonArray();
        TropicalFish.COMMON_VARIANTS.forEach(common -> tropicalFishCommonVariants.add(common.getPackedId()));
        out.add("tropicalFishCommonVariants", tropicalFishCommonVariants);
        out.addProperty("pufferfishPuffState", entityAccessor(net.minecraft.world.entity.animal.fish.Pufferfish.class, "PUFF_STATE").id());
        out.addProperty("turtleHasEgg", entityAccessor(net.minecraft.world.entity.animal.turtle.Turtle.class, "HAS_EGG").id());
        out.addProperty("turtleLayingEgg", entityAccessor(net.minecraft.world.entity.animal.turtle.Turtle.class, "LAYING_EGG").id());
        out.addProperty("dolphinGotFish", entityAccessor(net.minecraft.world.entity.animal.dolphin.Dolphin.class, "GOT_FISH").id());
        out.addProperty("dolphinMoistness", entityAccessor(net.minecraft.world.entity.animal.dolphin.Dolphin.class, "MOISTNESS_LEVEL").id());
        Class<?> axolotl = net.minecraft.world.entity.animal.axolotl.Axolotl.class;
        out.addProperty("axolotlVariant", entityAccessor(axolotl, "DATA_VARIANT").id());
        out.addProperty("axolotlPlayingDead", entityAccessor(axolotl, "DATA_PLAYING_DEAD").id());
        out.addProperty("axolotlFromBucket", entityAccessor(axolotl, "FROM_BUCKET").id());
        Class<?> chicken = net.minecraft.world.entity.animal.chicken.Chicken.class;
        var chickenVariant = entityAccessor(chicken, "DATA_VARIANT_ID");
        var chickenSound = entityAccessor(chicken, "DATA_SOUND_VARIANT_ID");
        out.addProperty("chickenVariant", chickenVariant.id());
        out.addProperty("chickenSoundVariant", chickenSound.id());
        out.addProperty("chickenVariantSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(chickenVariant.serializer()));
        out.addProperty("chickenSoundVariantSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(chickenSound.serializer()));
        out.addProperty("cubeMobSize", entityAccessor(net.minecraft.world.entity.monster.cubemob.AbstractCubeMob.class, "ID_SIZE").id());
        out.addProperty("slimeEyeHeight", net.minecraft.world.entity.EntityTypes.SLIME.getDimensions().eyeHeight());
        var armadilloState = entityAccessor(net.minecraft.world.entity.animal.armadillo.Armadillo.class, "ARMADILLO_STATE");
        out.addProperty("armadilloState", armadilloState.id());
        out.addProperty("armadilloStateSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(armadilloState.serializer()));
        Class<?> goat = net.minecraft.world.entity.animal.goat.Goat.class;
        out.addProperty("goatScreaming", entityAccessor(goat, "DATA_IS_SCREAMING_GOAT").id());
        out.addProperty("goatLeftHorn", entityAccessor(goat, "DATA_HAS_LEFT_HORN").id());
        out.addProperty("goatRightHorn", entityAccessor(goat, "DATA_HAS_RIGHT_HORN").id());
        Class<?> frog = net.minecraft.world.entity.animal.frog.Frog.class;
        var frogVariant = entityAccessor(frog, "DATA_VARIANT_ID");
        var frogTongue = entityAccessor(frog, "DATA_TONGUE_TARGET_ID");
        out.addProperty("frogVariant", frogVariant.id());
        out.addProperty("frogVariantSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(frogVariant.serializer()));
        out.addProperty("frogTongueTarget", frogTongue.id());
        out.addProperty("frogTongueTargetSerializer", net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(frogTongue.serializer()));
        out.addProperty("mooshroomType", entityAccessor(net.minecraft.world.entity.animal.cow.MushroomCow.class, "DATA_TYPE").id());
        out.add("stewFlowers", stewFlowers());
        out.addProperty("polarBearStanding", entityAccessor(net.minecraft.world.entity.animal.polarbear.PolarBear.class, "DATA_STANDING_ID").id());
        Class<?> panda = net.minecraft.world.entity.animal.panda.Panda.class;
        out.addProperty("pandaUnhappy", entityAccessor(panda, "UNHAPPY_COUNTER").id());
        out.addProperty("pandaSneeze", entityAccessor(panda, "SNEEZE_COUNTER").id());
        out.addProperty("pandaEat", entityAccessor(panda, "EAT_COUNTER").id());
        out.addProperty("pandaMainGene", entityAccessor(panda, "MAIN_GENE_ID").id());
        out.addProperty("pandaHiddenGene", entityAccessor(panda, "HIDDEN_GENE_ID").id());
        out.addProperty("pandaFlags", entityAccessor(panda, "DATA_ID_FLAGS").id());
        out.addProperty("ocelotTrusting", entityAccessor(net.minecraft.world.entity.animal.feline.Ocelot.class, "DATA_TRUSTING").id());
        addGolemFacts(out);
        Class<?> cat = net.minecraft.world.entity.animal.feline.Cat.class;
        out.addProperty("catVariant", entityAccessor(cat, "DATA_VARIANT_ID").id());
        out.addProperty("catIsLying", entityAccessor(cat, "IS_LYING").id());
        out.addProperty("catRelaxStateOne", entityAccessor(cat, "RELAX_STATE_ONE").id());
        out.addProperty("catCollar", entityAccessor(cat, "DATA_COLLAR_COLOR").id());
        out.addProperty("catSoundVariant", entityAccessor(cat, "DATA_SOUND_VARIANT_ID").id());
        // 鹦鹉的 DATA_VARIANT_ID 用 INT 序列化器，index 紧随 TamableAnimal 之后。
        // The parrot's DATA_VARIANT_ID uses the INT serializer; its index follows TamableAnimal.
        Class<?> parrot = net.minecraft.world.entity.animal.parrot.Parrot.class;
        out.addProperty("parrotVariant", entityAccessor(parrot, "DATA_VARIANT_ID").id());
        // 蜜蜂的两个元数据 index、属性、眼高和常量都由类本身导出，别在 Rust 侧手写。
        // Bee metadata indices, attributes, eye height and constants come from the class.
        Class<?> bee = net.minecraft.world.entity.animal.bee.Bee.class;
        out.addProperty("beeFlags", entityAccessor(bee, "DATA_FLAGS_ID").id());
        out.addProperty("beeAngerEndTime", entityAccessor(bee, "DATA_ANGER_END_TIME").id());
        var beeAttributes = net.minecraft.world.entity.animal.bee.Bee.createAttributes().build();
        out.addProperty("beeMaxHealth", beeAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
        out.addProperty("beeFlyingSpeed", beeAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.FLYING_SPEED));
        out.addProperty("beeMovementSpeed", beeAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
        out.addProperty("beeAttackDamage", beeAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.ATTACK_DAMAGE));
        out.addProperty("beeEyeHeight", net.minecraft.world.entity.EntityTypes.BEE.getDimensions().eyeHeight());
        out.addProperty("beeTicksPerFlap", (int) readField(bee, "TICKS_PER_FLAP", null));
        out.addProperty("beeFlapDegreesPerTick", (float) readField(bee, "FLAP_DEGREES_PER_TICK", null));
        var angerTime = (net.minecraft.util.valueproviders.UniformInt) readField(bee, "PERSISTENT_ANGER_TIME", null);
        out.addProperty("beeAngerMinSeconds", angerTime.minInclusive());
        out.addProperty("beeAngerMaxSeconds", angerTime.maxInclusive());
        var hive = net.minecraft.world.level.block.entity.BeehiveBlockEntity.class;
        out.addProperty("beehiveMaxOccupants", (int) readField(hive, "MAX_OCCUPANTS", null));
        out.addProperty("beehiveMinTicksNectarless", (int) readField(hive, "MIN_OCCUPATION_TICKS_NECTARLESS", null));
        out.addProperty("beehiveMinTicksNectar", (int) readField(hive, "MIN_OCCUPATION_TICKS_NECTAR", null));
        out.addProperty("beehiveReenterTicks", (int) readField(hive, "MIN_TICKS_BEFORE_REENTERING_HIVE", null));
        // Parrot.getAmbient 在 nextInt(1000)==0 时 nextInt(MOB_SOUND_MAP.size()) 挑模仿的音效。
        // Parrot.getAmbient draws nextInt(MOB_SOUND_MAP.size()) to pick an imitated sound.
        java.lang.reflect.Field soundMap = parrot.getDeclaredField("MOB_SOUND_MAP");
        soundMap.setAccessible(true);
        out.addProperty("parrotSoundMapSize", ((java.util.Map<?, ?>) soundMap.get(null)).size());
        out.add("potionEffects", potionEffects());
        var effectRegistry = BuiltInRegistries.MOB_EFFECT;
        JsonArray colors = new JsonArray();
        for (int id = 0; id < effectRegistry.size(); id++) {
            colors.add(effectRegistry.byId(id).getColor());
        }
        out.add("effectColors", colors);
        return out;
    }

    /**
     * 药水注册表每条药水自带的效果列表，顺序就是注册表 ID。
     * `PotionContents.forEachEffect` 和药水箭的 `doPostHurtEffects` 都读它。
     */
    private static JsonArray potionEffects() {
        JsonArray out = new JsonArray();
        for (net.minecraft.world.item.alchemy.Potion potion : BuiltInRegistries.POTION) {
            JsonArray effects = new JsonArray();
            for (net.minecraft.world.effect.MobEffectInstance effect : potion.getEffects()) {
                JsonArray entry = new JsonArray();
                entry.add(BuiltInRegistries.MOB_EFFECT.getId(effect.getEffect().value()));
                entry.add(effect.getDuration());
                entry.add(effect.getAmplifier());
                effects.add(entry);
            }
            out.add(effects);
        }
        return out;
    }

    /**
     * `SuspiciousEffectHolder.tryGet(item)` 不为空的物品和它的迷之炖菜效果：
     * `[物品 ID, [[效果 ID, 刻数], ...]]`，按物品注册表顺序。棕色哞菇吃花时读它。
     *
     * Items with a `SuspiciousEffectHolder` and their stew effects, in item registry
     * order: `[item id, [[effect id, ticks], ...]]`. Brown mooshrooms read it.
     */
    private static JsonArray stewFlowers() {
        JsonArray out = new JsonArray();
        for (net.minecraft.world.item.Item item : BuiltInRegistries.ITEM) {
            var holder = net.minecraft.world.level.block.SuspiciousEffectHolder.tryGet(item);
            if (holder == null) {
                continue;
            }
            JsonArray effects = new JsonArray();
            for (var entry : holder.getSuspiciousEffects().effects()) {
                JsonArray pair = new JsonArray();
                pair.add(BuiltInRegistries.MOB_EFFECT.getId(entry.effect().value()));
                pair.add(entry.duration());
                effects.add(pair);
            }
            JsonArray row = new JsonArray();
            row.add(BuiltInRegistries.ITEM.getId(item));
            row.add(effects);
            out.add(row);
        }
        return out;
    }

    /** 狐狸的尺寸、元数据下标和属性。 */
    private static void addFoxFacts(JsonObject out) throws ReflectiveOperationException {
        Class<?> fox = net.minecraft.world.entity.animal.fox.Fox.class;
        var adult = net.minecraft.world.entity.EntityTypes.FOX.getDimensions();
        var baby = (net.minecraft.world.entity.EntityDimensions) readField(fox, "BABY_DIMENSIONS", null);
        var attributes = net.minecraft.world.entity.animal.fox.Fox.createAttributes().build();
        out.addProperty("foxEyeHeight", adult.eyeHeight());
        out.addProperty("foxBabyWidth", baby.width());
        out.addProperty("foxBabyHeight", baby.height());
        out.addProperty("foxBabyEyeHeight", baby.eyeHeight());
        for (String[] accessor : new String[][] {
                {"foxTrusted0", "DATA_TRUSTED_ID_0"}, {"foxTrusted1", "DATA_TRUSTED_ID_1"},
                {"foxType", "DATA_TYPE_ID"}, {"foxFlags", "DATA_FLAGS_ID"}}) {
            var data = (net.minecraft.network.syncher.EntityDataAccessor<?>) readField(fox, accessor[1], null);
            out.addProperty(accessor[0], data.id());
        }
        out.addProperty("foxMaxHealth", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
        out.addProperty("foxMovementSpeed", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
        out.addProperty("foxAttackDamage", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.ATTACK_DAMAGE));
        out.addProperty("foxSafeFallDistance", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.SAFE_FALL_DISTANCE));
        out.addProperty("foxFollowRange", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.FOLLOW_RANGE));
    }

    /** 狼的尺寸、元数据下标（含 `TamableAnimal` 的两项）和属性。 */
    private static void addWolfFacts(JsonObject out) throws ReflectiveOperationException {
        Class<?> wolf = net.minecraft.world.entity.animal.wolf.Wolf.class;
        Class<?> tamable = net.minecraft.world.entity.TamableAnimal.class;
        var adult = net.minecraft.world.entity.EntityTypes.WOLF.getDimensions();
        var baby = (net.minecraft.world.entity.EntityDimensions) readField(wolf, "BABY_DIMENSIONS", null);
        var attributes = net.minecraft.world.entity.animal.wolf.Wolf.createAttributes().build();
        out.addProperty("wolfEyeHeight", adult.eyeHeight());
        out.addProperty("wolfBabyWidth", baby.width());
        out.addProperty("wolfBabyHeight", baby.height());
        out.addProperty("wolfBabyEyeHeight", baby.eyeHeight());
        for (String[] accessor : new String[][] {
                {"tameFlags", "DATA_FLAGS_ID"}, {"tameOwner", "DATA_OWNERUUID_ID"}}) {
            var data = (net.minecraft.network.syncher.EntityDataAccessor<?>) readField(tamable, accessor[1], null);
            out.addProperty(accessor[0], data.id());
        }
        for (String[] accessor : new String[][] {
                {"wolfInterested", "DATA_INTERESTED_ID"}, {"wolfCollar", "DATA_COLLAR_COLOR"},
                {"wolfAngerEndTime", "DATA_ANGER_END_TIME"}, {"wolfVariant", "DATA_VARIANT_ID"},
                {"wolfSoundVariant", "DATA_SOUND_VARIANT_ID"}}) {
            var data = (net.minecraft.network.syncher.EntityDataAccessor<?>) readField(wolf, accessor[1], null);
            out.addProperty(accessor[0], data.id());
        }
        out.addProperty("wolfMaxHealth", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
        out.addProperty("wolfMovementSpeed", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
        out.addProperty("wolfAttackDamage", attributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.ATTACK_DAMAGE));
    }

    /** 两种傀儡的元数据 index、属性、尺寸和裂纹阈值，按官方类分配。 */
    private static void addGolemFacts(JsonObject out) throws ReflectiveOperationException {
        Class<?> iron = net.minecraft.world.entity.animal.golem.IronGolem.class;
        Class<?> snow = net.minecraft.world.entity.animal.golem.SnowGolem.class;
        var ironAttributes = net.minecraft.world.entity.animal.golem.IronGolem.createAttributes().build();
        var snowAttributes = net.minecraft.world.entity.animal.golem.SnowGolem.createAttributes().build();
        var ironDimensions = net.minecraft.world.entity.EntityTypes.IRON_GOLEM.getDimensions();
        var snowDimensions = net.minecraft.world.entity.EntityTypes.SNOW_GOLEM.getDimensions();
        out.addProperty("ironGolemFlags", entityAccessor(iron, "DATA_FLAGS_ID").id());
        out.addProperty("snowGolemPumpkin", entityAccessor(snow, "DATA_PUMPKIN_ID").id());
        out.addProperty("ironGolemEyeHeight", ironDimensions.eyeHeight());
        out.addProperty("snowGolemEyeHeight", snowDimensions.eyeHeight());
        out.addProperty("ironGolemMaxHealth", ironAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
        out.addProperty("ironGolemMovementSpeed", ironAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
        out.addProperty("ironGolemKnockbackResistance", ironAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.KNOCKBACK_RESISTANCE));
        out.addProperty("ironGolemAttackDamage", ironAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.ATTACK_DAMAGE));
        out.addProperty("ironGolemStepHeight", ironAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.STEP_HEIGHT));
        out.addProperty("snowGolemMaxHealth", snowAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MAX_HEALTH));
        out.addProperty("snowGolemMovementSpeed", snowAttributes.getValue(net.minecraft.world.entity.ai.attributes.Attributes.MOVEMENT_SPEED));
        // Crackiness.GOLEM 的三个私有阈值，按官方静态构造时的值。
        Class<?> crackiness = net.minecraft.world.entity.Crackiness.class;
        var golem = readField(crackiness, "GOLEM", null);
        for (String[] field : new String[][] {
                {"golemCrackinessLow", "fractionLow"}, {"golemCrackinessMedium", "fractionMedium"},
                {"golemCrackinessHigh", "fractionHigh"}}) {
            var member = crackiness.getDeclaredField(field[1]);
            member.setAccessible(true);
            out.addProperty(field[0], member.getFloat(golem));
        }
    }

    /** 任意类的私有 `EntityDataAccessor` 静态字段分到的 index。 */
    private static int accessorId(String owner, String name) throws ReflectiveOperationException {
        Field field = Class.forName(owner).getDeclaredField(name);
        field.setAccessible(true);
        return ((net.minecraft.network.syncher.EntityDataAccessor<?>) field.get(null)).id();
    }

    private static net.minecraft.network.syncher.EntityDataAccessor<?> entityAccessor(Class<?> type, String name) throws ReflectiveOperationException {
        Field field = type.getDeclaredField(name);
        field.setAccessible(true);
        return (net.minecraft.network.syncher.EntityDataAccessor<?>) field.get(null);
    }

    private static int arrowAccessorId(String name) throws ReflectiveOperationException {
        Field field = net.minecraft.world.entity.projectile.arrow.AbstractArrow.class.getDeclaredField(name);
        field.setAccessible(true);
        return ((net.minecraft.network.syncher.EntityDataAccessor<?>) field.get(null)).id();
    }

    // 敌对生物各自定义的元数据按类树分配后的 index，以及实体类型的眼高。
    // 键是生成的常量名，生成器原样写成 `pub const`。
    // Class-tree metadata indices of the hostile mobs and their eye heights.
    // Keys are the generated constant names.
    private static JsonObject monsterFacts() throws ReflectiveOperationException {
        JsonObject ids = new JsonObject();
        Class<?> creeper = net.minecraft.world.entity.monster.Creeper.class;
        Class<?> spider = net.minecraft.world.entity.monster.spider.Spider.class;
        Class<?> enderman = net.minecraft.world.entity.monster.Enderman.class;
        ids.addProperty("CREEPER_SWELL_DIR", accessorId(creeper, "DATA_SWELL_DIR"));
        ids.addProperty("CREEPER_POWERED", accessorId(creeper, "DATA_IS_POWERED"));
        ids.addProperty("CREEPER_IGNITED", accessorId(creeper, "DATA_IS_IGNITED"));
        ids.addProperty("SPIDER_FLAGS", accessorId(spider, "DATA_FLAGS_ID"));
        ids.addProperty("ENDERMAN_CARRY_STATE", accessorId(enderman, "DATA_CARRY_STATE"));
        ids.addProperty("ENDERMAN_CREEPY", accessorId(enderman, "DATA_CREEPY"));
        ids.addProperty("ENDERMAN_STARED_AT", accessorId(enderman, "DATA_STARED_AT"));
        JsonObject eyes = new JsonObject();
        eyes.addProperty("CREEPER_EYE_HEIGHT", net.minecraft.world.entity.EntityTypes.CREEPER.getDimensions().eyeHeight());
        eyes.addProperty("SPIDER_EYE_HEIGHT", net.minecraft.world.entity.EntityTypes.SPIDER.getDimensions().eyeHeight());
        eyes.addProperty("CAVE_SPIDER_EYE_HEIGHT", net.minecraft.world.entity.EntityTypes.CAVE_SPIDER.getDimensions().eyeHeight());
        eyes.addProperty("ENDERMAN_EYE_HEIGHT", net.minecraft.world.entity.EntityTypes.ENDERMAN.getDimensions().eyeHeight());
        eyes.addProperty("SILVERFISH_EYE_HEIGHT", net.minecraft.world.entity.EntityTypes.SILVERFISH.getDimensions().eyeHeight());
        eyes.addProperty("ENDERMITE_EYE_HEIGHT", net.minecraft.world.entity.EntityTypes.ENDERMITE.getDimensions().eyeHeight());
        JsonObject out = new JsonObject();
        out.add("dataIds", ids);
        out.add("eyeHeights", eyes);
        return out;
    }

    private static int accessorId(Class<?> owner, String name) throws ReflectiveOperationException {
        Field field = owner.getDeclaredField(name);
        field.setAccessible(true);
        return ((net.minecraft.network.syncher.EntityDataAccessor<?>) field.get(null)).id();
    }

    /** 官方数据包的全部配方，按注册表的顺序。`getRecipeFor` 取第一条匹配的，先后由这个顺序决定。 */
    private static JsonObject recipes(RegistryAccess registries) throws ReflectiveOperationException {
        JsonArray entries = new JsonArray();
        for (RecipeHolder<?> holder : new RecipeManager(registries).getRecipes()) {
            entries.add(recipe(holder, registries));
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

    private static JsonObject recipe(RecipeHolder<?> holder, RegistryAccess registries)
            throws ReflectiveOperationException {
        Recipe<?> recipe = holder.value();
        JsonObject object = new JsonObject();
        object.addProperty("id", holder.id().identifier().toString());
        object.addProperty("type", BuiltInRegistries.RECIPE_SERIALIZER.getKey(recipe.getSerializer()).toString());
        if (recipe instanceof ShapedRecipe shaped) {
            object.addProperty("kind", "shaped");
            object.addProperty("width", shaped.getWidth());
            object.addProperty("height", shaped.getHeight());
            object.add("pattern", pattern(shaped.getIngredients()));
            object.add("result", stack((ItemStackTemplate) readField(ShapedRecipe.class, "result", shaped), registries));
        } else if (recipe instanceof ShapelessRecipe shapeless) {
            object.addProperty("kind", "shapeless");
            object.add("ingredients", ingredients((List<Ingredient>) readField(ShapelessRecipe.class, "ingredients", shapeless)));
            object.add("result", stack((ItemStackTemplate) readField(ShapelessRecipe.class, "result", shapeless), registries));
        } else if (recipe instanceof TransmuteRecipe transmute) {
            object.addProperty("kind", "transmute");
            object.add("input", ingredient((Ingredient) readField(TransmuteRecipe.class, "input", transmute)));
            object.add("material", ingredient((Ingredient) readField(TransmuteRecipe.class, "material", transmute)));
            MinMaxBounds.Ints bounds = (MinMaxBounds.Ints) readField(TransmuteRecipe.class, "materialCount", transmute);
            object.addProperty("minMaterial", bounds.min().orElse(1));
            object.addProperty("maxMaterial", bounds.max().orElse(8));
            transmuteResult(object, (TransmuteResult) readField(TransmuteRecipe.class, "result", transmute), registries);
            object.addProperty("addMaterialCount", (boolean) readField(TransmuteRecipe.class, "addMaterialCountToResult", transmute));
        } else if (recipe instanceof DyeRecipe dye) {
            object.addProperty("kind", "dye");
            object.add("target", ingredient((Ingredient) readField(DyeRecipe.class, "target", dye)));
            object.add("dye", ingredient((Ingredient) readField(DyeRecipe.class, "dye", dye)));
            object.add("result", stack((ItemStackTemplate) readField(DyeRecipe.class, "result", dye), registries));
        } else if (recipe instanceof ImbueRecipe imbue) {
            object.addProperty("kind", "imbue");
            object.add("source", ingredient((Ingredient) readField(ImbueRecipe.class, "source", imbue)));
            object.add("material", ingredient((Ingredient) readField(ImbueRecipe.class, "material", imbue)));
            object.add("result", stack((ItemStackTemplate) readField(ImbueRecipe.class, "result", imbue), registries));
        } else if (recipe instanceof AbstractCookingRecipe cooking) {
            String kind = recipe instanceof SmeltingRecipe ? "smelting"
                    : recipe instanceof BlastingRecipe ? "blasting"
                    : recipe instanceof SmokingRecipe ? "smoking" : "campfire_cooking";
            object.addProperty("kind", kind);
            object.add("ingredient", ingredient((Ingredient) readField(SingleItemRecipe.class, "input", cooking)));
            object.add("result", stack((ItemStackTemplate) readField(SingleItemRecipe.class, "result", cooking), registries));
            object.addProperty("experience", cooking.experience());
            object.addProperty("time", cooking.cookingTime());
        } else if (recipe instanceof StonecutterRecipe stonecutting) {
            object.addProperty("kind", "stonecutting");
            object.add("ingredient", ingredient((Ingredient) readField(SingleItemRecipe.class, "input", stonecutting)));
            object.add("result", stack((ItemStackTemplate) readField(SingleItemRecipe.class, "result", stonecutting), registries));
        } else if (recipe instanceof SmithingTransformRecipe transform) {
            object.addProperty("kind", "smithing_transform");
            object.add("template", optionalIngredient(transform.templateIngredient()));
            object.add("base", ingredient(transform.baseIngredient()));
            object.add("addition", optionalIngredient(transform.additionIngredient()));
            object.add("result", stack((ItemStackTemplate) readField(SmithingTransformRecipe.class, "result", transform), registries));
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
            object.add("result", stack(brewing.getOutput(), registries));
        } else if (recipe instanceof FireworkRocketRecipe rocket) {
            object.addProperty("kind", "firework_rocket");
            object.add("shell", ingredient((Ingredient) readField(FireworkRocketRecipe.class, "shell", rocket)));
            object.add("fuel", ingredient((Ingredient) readField(FireworkRocketRecipe.class, "fuel", rocket)));
            object.add("star", ingredient((Ingredient) readField(FireworkRocketRecipe.class, "star", rocket)));
            object.add("result", stack((ItemStackTemplate) readField(FireworkRocketRecipe.class, "result", rocket), registries));
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
            object.add("result", stack((ItemStackTemplate) readField(FireworkStarRecipe.class, "result", star), registries));
        } else if (recipe instanceof FireworkStarFadeRecipe fade) {
            object.addProperty("kind", "firework_star_fade");
            object.add("target", ingredient((Ingredient) readField(FireworkStarFadeRecipe.class, "target", fade)));
            object.add("dye", ingredient((Ingredient) readField(FireworkStarFadeRecipe.class, "dye", fade)));
            object.add("result", stack((ItemStackTemplate) readField(FireworkStarFadeRecipe.class, "result", fade), registries));
        } else if (recipe instanceof MapExtendingRecipe map) {
            object.addProperty("kind", "mapextending");
            object.add("map", ingredient((Ingredient) readField(MapExtendingRecipe.class, "map", map)));
            object.add("material", ingredient((Ingredient) readField(MapExtendingRecipe.class, "material", map)));
            transmuteResult(object, (TransmuteResult) readField(MapExtendingRecipe.class, "result", map), registries);
        } else if (recipe instanceof BannerDuplicateRecipe banner) {
            object.addProperty("kind", "banner_duplicate");
            object.add("banner", ingredient((Ingredient) readField(BannerDuplicateRecipe.class, "banner", banner)));
            object.add("result", stack((ItemStackTemplate) readField(BannerDuplicateRecipe.class, "result", banner), registries));
        } else if (recipe instanceof BookCloningRecipe book) {
            object.addProperty("kind", "book_cloning");
            object.add("source", ingredient((Ingredient) readField(BookCloningRecipe.class, "source", book)));
            object.add("material", ingredient((Ingredient) readField(BookCloningRecipe.class, "material", book)));
            MinMaxBounds.Ints bounds = (MinMaxBounds.Ints) readField(BookCloningRecipe.class, "allowedGenerations", book);
            object.addProperty("minGeneration", bounds.min().orElse(0));
            object.addProperty("maxGeneration", bounds.max().orElse(Integer.MAX_VALUE));
            object.add("result", stack((ItemStackTemplate) readField(BookCloningRecipe.class, "result", book), registries));
        } else if (recipe instanceof ShieldDecorationRecipe shield) {
            object.addProperty("kind", "shield_decoration");
            object.add("banner", ingredient((Ingredient) readField(ShieldDecorationRecipe.class, "banner", shield)));
            object.add("target", ingredient((Ingredient) readField(ShieldDecorationRecipe.class, "target", shield)));
            object.add("result", stack((ItemStackTemplate) readField(ShieldDecorationRecipe.class, "result", shield), registries));
        } else if (recipe instanceof RepairItemRecipe) {
            object.addProperty("kind", "repair_item");
        } else if (recipe instanceof DecoratedPotRecipe pot) {
            object.addProperty("kind", "decorated_pot");
            object.add("back", ingredient((Ingredient) readField(DecoratedPotRecipe.class, "backPattern", pot)));
            object.add("left", ingredient((Ingredient) readField(DecoratedPotRecipe.class, "leftPattern", pot)));
            object.add("right", ingredient((Ingredient) readField(DecoratedPotRecipe.class, "rightPattern", pot)));
            object.add("front", ingredient((Ingredient) readField(DecoratedPotRecipe.class, "frontPattern", pot)));
            object.add("result", stack((ItemStackTemplate) readField(DecoratedPotRecipe.class, "result", pot), registries));
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

    private static JsonObject stack(ItemStackTemplate template, RegistryAccess registries) {
        JsonObject object = new JsonObject();
        object.addProperty("item", BuiltInRegistries.ITEM.getId(template.item().value()));
        object.addProperty("count", template.count());
        object.addProperty("components", hex(template.components(), registries));
        return object;
    }

    private static void transmuteResult(JsonObject object, TransmuteResult result, RegistryAccess registries) {
        result.item().ifPresent(item -> object.addProperty("resultItem", BuiltInRegistries.ITEM.getId(item.value())));
        object.addProperty("resultCount", result.count());
        object.addProperty("resultComponents", hex(result.components(), registries));
    }

    /** 组件补丁按官方的网络编码写成十六进制，Bamboo 那边用同一套编解码还原。 */
    private static String hex(DataComponentPatch patch, RegistryAccess registries) {
        RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
        DataComponentPatch.STREAM_CODEC.encode(buffer, patch);
        byte[] bytes = new byte[buffer.readableBytes()];
        buffer.readBytes(bytes);
        return HexFormat.of().formatHex(bytes);
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
        worldgen.add("material_rule",
                encodeRegistry(registries, ops, Registries.MATERIAL_RULE, MaterialRule.DIRECT_CODEC));
        worldgen.add("material_condition",
                encodeRegistry(registries, ops, Registries.MATERIAL_CONDITION, MaterialCondition.DIRECT_CODEC));
        worldgen.add("carver", encodeRegistry(registries, ops, Registries.CARVER, WorldCarver.DIRECT_CODEC));
        worldgen.add("biome_carvers", biomeCarvers(registries));
        worldgen.add("feature", encodeRegistry(registries, ops, Registries.FEATURE, Feature.DIRECT_CODEC));
        worldgen.add("placed_feature",
                encodeRegistry(registries, ops, Registries.PLACED_FEATURE, PlacedFeature.DIRECT_CODEC));
        // `Holder<BlockStateProvider>` 字符串引用的注册表（`cave_vines_body` 等 8 条）。
        // 地物配置里以字符串出现，Rust 侧解析时查这张表。
        // The registry behind `Holder<BlockStateProvider>` string references (8 entries such
        // as `cave_vines_body`); feature configs name them as strings, and Rust resolves
        // them against this table.
        worldgen.add("block_state_provider",
                encodeRegistry(registries, ops, Registries.BLOCK_STATE_PROVIDER,
                        BlockStateProvider.DIRECT_CODEC));
        worldgen.add("biome_features", biomeFeatures(registries));
        worldgen.add("features_per_step", featuresPerStep(registries));
        worldgen.add("multi_noise_presets", multiNoisePresets(registries));
        worldgen.add("end_biomes", endBiomes());
        worldgen.add("structure_set",
                encodeRegistry(registries, ops, Registries.STRUCTURE_SET, StructureSet.DIRECT_CODEC));
        worldgen.add("structure",
                encodeRegistry(registries, ops, Registries.STRUCTURE, Structure.DIRECT_CODEC));
        worldgen.add("template_pool",
                encodeRegistry(registries, ops, Registries.TEMPLATE_POOL, StructureTemplatePool.DIRECT_CODEC));
        worldgen.add("processor_list",
                encodeRegistry(registries, ops, Registries.PROCESSOR_LIST, StructureProcessorType.DIRECT_CODEC));
        return worldgen;
    }

    /** 每个群系 11 个生成步骤的 `PlacedFeature` 列表，按官方 `features()` 的顺序。 */
    private static JsonObject biomeFeatures(RegistryAccess registries) {
        Registry<Biome> biomes = registries.lookupOrThrow(Registries.BIOME);
        JsonObject result = new JsonObject();
        for (Identifier name : biomes.keySet().stream().sorted().toList()) {
            JsonArray steps = new JsonArray();
            for (HolderSet<PlacedFeature> step :
                    biomes.getValue(name).getGenerationSettings().features()) {
                JsonArray entries = new JsonArray();
                for (Holder<PlacedFeature> feature : step) {
                    entries.add(feature.unwrapKey().orElseThrow().identifier().toString());
                }
                steps.add(entries);
            }
            result.add(name.toString(), steps);
        }
        return result;
    }

    /**
     * `FeatureSorter` 排好的每步全局顺序。
     *
     * <p>`ChunkGenerator.applyBiomeDecoration` 用这个列表的下标做
     * `setFeatureSeed(decorationSeed, index, step)`，所以顺序必须和官方一致；
     * 自己重推要复刻 `ImmutableSet` 的迭代序、`TreeMap` 比较器和 DFS 的邻接表顺序，
     * 不如直接从官方的 `featuresPerStep` supplier 读。
     */
    private static JsonArray featuresPerStep(RegistryAccess registries) {
        Registry<NoiseGeneratorSettings> settings = registries.lookupOrThrow(Registries.NOISE_SETTINGS);
        Registry<PlacedFeature> placed = registries.lookupOrThrow(Registries.PLACED_FEATURE);
        ChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settings.wrapAsHolder(
                        settings.getValue(Identifier.withDefaultNamespace("overworld"))));
        // 官方生成器用这套群系源的全部可能群系排地物。这里的数量是主世界预设里去重后的
        // 群系数，和 `minecraft:worldgen/biome` 注册表的条目数不必相同；换版本时靠下面的
        // 断言发现变化，不要直接改数字。
        int possibleBiomes = generator.getBiomeSource().possibleBiomes().size();
        if (possibleBiomes != EXPECTED_OVERWORLD_POSSIBLE_BIOMES) {
            throw new IllegalStateException("unexpected overworld possibleBiomes count: "
                    + possibleBiomes + ", expected " + EXPECTED_OVERWORLD_POSSIBLE_BIOMES);
        }
        Object rawSupplier;
        try {
            rawSupplier = readField(ChunkGenerator.class, "featuresPerStep", generator);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot read ChunkGenerator.featuresPerStep", error);
        }
        @SuppressWarnings("unchecked")
        java.util.function.Supplier<List<FeatureSorter.StepFeatureData>> supplier =
                (java.util.function.Supplier<List<FeatureSorter.StepFeatureData>>) rawSupplier;
        List<FeatureSorter.StepFeatureData> steps = supplier.get();
        if (steps.size() != GenerationStep.Decoration.values().length) {
            throw new IllegalStateException("unexpected featuresPerStep size " + steps.size());
        }
        JsonArray result = new JsonArray();
        for (FeatureSorter.StepFeatureData step : steps) {
            JsonArray entries = new JsonArray();
            for (PlacedFeature feature : step.features()) {
                entries.add(placed.getKey(feature).toString());
            }
            result.add(entries);
        }
        return result;
    }

    /**
     * 自然刷怪要用的数据：`MobCategory` 的上限表，以及每个群系的 `Biome.getMobSettings()`
     * （按类别的加权刷怪表和刷怪代价）。
     *
     * <p>`SpawnerData.count()` 在官方生成器里只有常量与均匀分布两种，这里显式展开成
     * `min` / `max`；出现别的 `IntProvider` 直接报错，逼着换版本时重新核对。
     * 条目顺序就是 `WeightedList.unwrap()` 的顺序，Rust 侧按同一顺序累加权重取随机项。
     *
     * <p>Natural spawning data: the `MobCategory` caps and every biome's
     * `Biome.getMobSettings()` (weighted spawn lists per category plus spawn costs).
     * `SpawnerData.count()` is only a constant or a uniform range in the vanilla
     * generator, so it is flattened to `min` / `max`; any other `IntProvider` fails
     * loudly. Entry order is `WeightedList.unwrap()` order, which the Rust side uses
     * to walk the cumulative weights.
     */
    private static JsonObject mobSpawns(RegistryAccess registries) {
        JsonObject result = new JsonObject();
        JsonArray categories = new JsonArray();
        for (MobCategory category : MobCategory.values()) {
            JsonObject entry = new JsonObject();
            entry.addProperty("name", category.getName());
            entry.addProperty("max", category.getMaxInstancesPerChunk());
            entry.addProperty("friendly", category.isFriendly());
            entry.addProperty("persistent", category.isPersistent());
            entry.addProperty("despawnDistance", category.getDespawnDistance());
            entry.addProperty("noDespawnDistance", category.getNoDespawnDistance());
            categories.add(entry);
        }
        result.add("categories", categories);

        JsonObject biomes = new JsonObject();
        Registry<Biome> biomeRegistry = registries.lookupOrThrow(Registries.BIOME);
        for (Identifier name : biomeRegistry.keySet().stream().sorted().toList()) {
            // 26.3 的刷怪设置是群系的环境属性 `NATURAL_MOB_SPAWNS`，按 overlay 叠在空表上。
            // 26.3 keeps spawn settings in the positional biome attribute
            // `NATURAL_MOB_SPAWNS`; overlay them onto the empty table.
            MobSpawnSettings settings = biomeRegistry.getValue(name).getAttributes()
                    .applyModifier(EnvironmentAttributes.NATURAL_MOB_SPAWNS, MobSpawnSettings.EMPTY);
            JsonObject biome = new JsonObject();
            JsonArray byCategory = new JsonArray();
            for (MobCategory category : MobCategory.values()) {
                WeightedList<MobSpawnSettings.SpawnerData> spawns = settings.getMobsToSpawn(category);
                if (spawns.isEmpty()) {
                    continue;
                }
                JsonObject row = new JsonObject();
                row.addProperty("category", category.getName());
                JsonArray entries = new JsonArray();
                for (Weighted<MobSpawnSettings.SpawnerData> weighted : spawns.unwrap()) {
                    MobSpawnSettings.SpawnerData data = weighted.value();
                    JsonObject spawn = new JsonObject();
                    spawn.addProperty("type", BuiltInRegistries.ENTITY_TYPE.getKey(data.type()).toString());
                    spawn.addProperty("weight", weighted.weight());
                    IntProvider count = data.count();
                    if (count instanceof ConstantInt constant) {
                        spawn.addProperty("min", constant.value());
                        spawn.addProperty("max", constant.value());
                    } else if (count instanceof UniformInt uniform) {
                        spawn.addProperty("min", uniform.minInclusive());
                        spawn.addProperty("max", uniform.maxInclusive());
                    } else {
                        throw new IllegalStateException("unsupported spawn count " + count);
                    }
                    entries.add(spawn);
                }
                row.add("entries", entries);
                byCategory.add(row);
            }
            biome.add("categories", byCategory);

            JsonArray costs = new JsonArray();
            Map<String, MobSpawnSettings.MobSpawnCost> costsByName = new TreeMap<>();
            settings.allSpawnCosts().forEach((type, cost) ->
                    costsByName.put(BuiltInRegistries.ENTITY_TYPE.getKey(type).toString(), cost));
            for (Map.Entry<String, MobSpawnSettings.MobSpawnCost> cost : costsByName.entrySet()) {
                JsonObject entry = new JsonObject();
                entry.addProperty("type", cost.getKey());
                entry.addProperty("charge", cost.getValue().charge());
                entry.addProperty("energyBudget", cost.getValue().energyBudget());
                costs.add(entry);
            }
            biome.add("costs", costs);
            // 生成期刷怪的概率：先过主世界维度层，再过群系层（`EnvironmentAttributeSystem` 的顺序）。
            // World-generation creature odds: overworld dimension layer, then the biome layer.
            float creatureProbability = biomeRegistry.getValue(name).getAttributes().applyModifier(
                    EnvironmentAttributes.CREATURE_WORLD_GEN_SPAWN_PROBABILITY,
                    registries.lookupOrThrow(Registries.DIMENSION_TYPE)
                            .getValue(net.minecraft.world.level.dimension.BuiltinDimensionTypes.OVERWORLD)
                            .attributes()
                            .applyModifier(EnvironmentAttributes.CREATURE_WORLD_GEN_SPAWN_PROBABILITY,
                                    EnvironmentAttributes.CREATURE_WORLD_GEN_SPAWN_PROBABILITY.defaultValue()));
            biome.addProperty("creatureSpawnProbability", Integer.toUnsignedLong(Float.floatToRawIntBits(creatureProbability)));
            // `SNOW_GOLEM_MELTS` 叠在主世界缺省值（false）上，得到这个群系的最终值。
            // Overlay SNOW_GOLEM_MELTS on the overworld default (false).
            boolean melts = biomeRegistry.getValue(name).getAttributes().applyModifier(
                    EnvironmentAttributes.SNOW_GOLEM_MELTS,
                    registries.lookupOrThrow(Registries.DIMENSION_TYPE)
                            .getValue(net.minecraft.world.level.dimension.BuiltinDimensionTypes.OVERWORLD)
                            .attributes()
                            .applyModifier(EnvironmentAttributes.SNOW_GOLEM_MELTS, false));
            biome.addProperty("snowGolemMelt", melts);
            biomes.add(name.toString(), biome);
        }
        result.add("biomes", biomes);
        return result;
    }

    /** 每个群系的 `BiomeGenerationSettings.getCarvers()`，顺序就是官方的迭代顺序。 */
    private static JsonObject biomeCarvers(RegistryAccess registries) {
        Registry<Biome> biomes = registries.lookupOrThrow(Registries.BIOME);
        JsonObject result = new JsonObject();
        for (Identifier name : biomes.keySet().stream().sorted().toList()) {
            JsonArray carvers = new JsonArray();
            for (Holder<WorldCarver> carver : biomes.getValue(name).getGenerationSettings().getCarvers()) {
                carvers.add(carver.unwrapKey().orElseThrow().identifier().toString());
            }
            result.add(name.toString(), carvers);
        }
        return result;
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
        transform.addProperty("consumeOnUse", data.consumeOnUse());
        transform.addProperty("itemDamagePerUse", data.itemDamagePerUse());
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

    /**
     * 带 {@code minecraft:item_damage} 效果的附魔。耐久附魔是数据驱动的，这里用官方 codec 把效果原样编出来。
     *
     * <p>没有这个效果的附魔不出现。效果列表的顺序就是 {@code Enchantment.getEffects} 的顺序。
     *
     * <p>Enchantments whose {@code minecraft:item_damage} list is non-empty. The codec output is kept as-is, in
     * {@code Enchantment.getEffects} order.
     */
    private static JsonArray itemDamageEnchantments(RegistryAccess registries) {
        Registry<Enchantment> registry = registries.lookupOrThrow(Registries.ENCHANTMENT);
        Codec<List<ConditionalEffect<EnchantmentValueEffect>>> codec =
                ConditionalEffect.codec(EnchantmentValueEffect.CODEC).listOf();
        RegistryOps<JsonElement> ops = registries.createSerializationContext(JsonOps.INSTANCE);
        JsonArray enchantments = new JsonArray();
        for (Identifier name : registry.keySet().stream().sorted().toList()) {
            List<ConditionalEffect<EnchantmentValueEffect>> effects =
                    registry.getValue(name).getEffects(EnchantmentEffectComponents.ITEM_DAMAGE);
            if (effects.isEmpty()) {
                continue;
            }
            JsonObject entry = new JsonObject();
            entry.addProperty("name", name.toString());
            entry.add("effects", codec.encodeStart(ops, effects).getOrThrow());
            enchantments.add(entry);
        }
        return enchantments;
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
        // 每个状态的四个 rotate 结果（NONE / CLOCKWISE_90 / CLOCKWISE_180 /
        // COUNTERCLOCKWISE_90，即 Rotation.ordinal()）和三个 mirror 结果（NONE /
        // LEFT_RIGHT / FRONT_BACK，即 Mirror.ordinal()）。结构件放置要按方向和 mirror 改
        // 方块状态，这张表让 Rust 侧直接查表，不用重写每个方块的 rotate/mirror。
        // Four rotate results per state (Rotation.ordinal()) and three mirror results
        // (Mirror.ordinal()); structure-piece placement looks them up instead of
        // reimplementing each block's rotate/mirror.
        private final JsonArray rotation = new JsonArray();
        private final JsonArray mirror = new JsonArray();
        private final JsonArray pathTypes = new JsonArray();
        private final JsonArray neighborPathTypes = new JsonArray();
        private final JsonArray collisionShapesAgree = new JsonArray();
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
            tables.add("rotation", rotation);
            tables.add("mirror", mirror);
            tables.add("pathTypes", pathTypes);
            tables.add("neighborPathTypes", neighborPathTypes);
            tables.add("collisionShapesAgree", collisionShapesAgree);
            return tables;
        }

        private void addState(int id, BlockState state) {
            boolean movesShape = movesShape(state.getBlock());
            checkPositionFacts(id, state, movesShape);

            facts.add(stateFacts(state, BlockPos.ZERO));
            fluid.add(fluidIndex(state.getFluidState()));
            for (Rotation value : Rotation.values()) {
                rotation.add(Block.BLOCK_STATE_REGISTRY.getId(state.rotate(value)));
            }
            for (Mirror value : Mirror.values()) {
                mirror.add(Block.BLOCK_STATE_REGISTRY.getId(state.mirror(value)));
            }
            lightDampening.add(state.getLightDampening());

            VoxelShape collisionShape = state.getCollisionShape(EMPTY, BlockPos.ZERO, CollisionContext.empty());
            collision.add(shapeIndex(canonical(state, collisionShape, movesShape)));
            outline.add(shapeIndex(canonical(state, state.getShape(EMPTY, BlockPos.ZERO), movesShape)));
            support.add(shapeIndex(canonical(state, state.getBlockSupportShape(EMPTY, BlockPos.ZERO), movesShape)));
            addPlacementCollision(id, state, collisionShape);
            // 空上下文和放置上下文的形状不同就不能预算快路径。
            // A fast path is only valid when empty and placement contexts agree.
            collisionShapesAgree.add(
                    shapeKey(collisionShape).equals(shapeKey(
                            state.getCollisionShape(EMPTY, BlockPos.ZERO,
                                    CollisionContext.placementContext(null)))));
            addPathTypes(state);
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

        /** 官方 `WalkNodeEvaluator.getPathTypeFromState` 以及作为邻居时的分类。 */
        /** Official `WalkNodeEvaluator.getPathTypeFromState` and neighbour classification. */
        private void addPathTypes(BlockState state) {
            if (state.getBlock().hasDynamicShape()) {
                // 形状依赖世界或方块实体，只能运行时算。
                // Depends on the world or a block entity: compute at runtime.
                pathTypes.add((String) null);
                neighborPathTypes.add((String) null);
                return;
            }
            SingleStateView view = new SingleStateView();
            view.state = state;
            view.centre = BlockPos.ZERO;
            pathTypes.add(PathTypes.Evaluator.fromState(view, BlockPos.ZERO).name());
            view.centre = PathTypes.CENTRE;
            PathType changed = PathType.OPEN;
            for (int[] offset : PathTypes.OFFSETS) {
                changed = WalkNodeEvaluator.checkNeighbourBlocks(PathTypes.context(view),
                        PathTypes.CENTRE.getX() + offset[0],
                        PathTypes.CENTRE.getY() + offset[1],
                        PathTypes.CENTRE.getZ() + offset[2], changed);
            }
            neighborPathTypes.add(changed.name());
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
            // 寻路起点向下找落脚点时问的 `isPathfindable(LAND)`，窒息判定的 `isSuffocating`。
            facts |= state.isPathfindable(net.minecraft.world.level.pathfinder.PathComputationType.LAND)
                    ? FACT_PATHFINDABLE_LAND : 0;
            facts |= state.isSuffocating(EMPTY, pos) ? FACT_SUFFOCATING : 0;
            facts |= state.isPathfindable(net.minecraft.world.level.pathfinder.PathComputationType.WATER)
                    ? FACT_PATHFINDABLE_WATER : 0;
            facts |= state.isValidSpawn(EMPTY, pos, EntityTypes.COW) ? FACT_VALID_SPAWN : 0;
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
                // 爆炸射线里取方块和流体抗性的较大者，见 `FluidState.getExplosionResistance`。
                object.addProperty("explosionResistance", added.getExplosionResistance());
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

    /** 寻路类型表用的官方 `PathfindingContext`，只有一格假世界，不启动服务端。 */
    /** A `PathfindingContext` over a one-cell fake world for the path type table. */
    private static final class PathTypes {
        static final BlockPos CENTRE = new BlockPos(1, 1, 1);
        static final int[][] OFFSETS = offsets();
        private static final PathfindingContext CONTEXT;

        static {
            try {
                Field unsafeField = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
                unsafeField.setAccessible(true);
                Object unsafe = unsafeField.get(null);
                var allocate = unsafe.getClass().getMethod("allocateInstance", Class.class);
                CONTEXT = (PathfindingContext) allocate.invoke(unsafe, PathfindingContext.class);
                set(CONTEXT, "cache", null);
                set(CONTEXT, "mobPosition", BlockPos.ZERO);
                set(CONTEXT, "mutablePos", new BlockPos.MutableBlockPos());
            } catch (Exception error) {
                throw new ExceptionInInitializerError(error);
            }
        }

        static PathfindingContext context(SingleStateView view) {
            try {
                set(CONTEXT, "level", view);
            } catch (Exception error) {
                throw new IllegalStateException(error);
            }
            return CONTEXT;
        }

        private static void set(Object target, String name, Object value) throws Exception {
            Field field = PathfindingContext.class.getDeclaredField(name);
            field.setAccessible(true);
            field.set(target, value);
        }

        private static int[][] offsets() {
            List<int[]> result = new ArrayList<>();
            for (int x = -1; x <= 1; x++) {
                for (int y = -1; y <= 1; y++) {
                    for (int z = -1; z <= 1; z++) {
                        if (x != 0 || z != 0) {
                            result.add(new int[]{x, y, z});
                        }
                    }
                }
            }
            return result.toArray(new int[0][]);
        }

        /** 官方 protected static 方法从子类转发出来。 */
        /** Forward the protected static official method. */
        private static final class Evaluator extends WalkNodeEvaluator {
            static PathType fromState(net.minecraft.world.level.BlockGetter view, BlockPos pos) {
                return getPathTypeFromState(view, pos);
            }
        }
    }

    /** 3×3×3 的假世界：中心是查的状态，其余是空气。 */
    /** A 3³ fake world: the queried state at the centre, air everywhere else. */
    private static final class SingleStateView implements CollisionGetter {
        BlockState state;
        BlockPos centre = BlockPos.ZERO;

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return pos.equals(centre) ? state : Blocks.AIR.defaultBlockState();
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            // 官方 `getPathTypeFromState` 从 BlockState 自己取流体，不读这个视图；
            // 26 个邻居都是空气。
            // Official `getPathTypeFromState` reads the fluid from the BlockState
            // itself, not from this view; all 26 neighbours are air.
            return Fluids.EMPTY.defaultFluidState();
        }

        @Override
        public BlockEntity getBlockEntity(BlockPos pos) {
            return null;
        }

        @Override
        public int getMinY() {
            return -64;
        }

        @Override
        public int getHeight() {
            return 384;
        }

        @Override
        public WorldBorder getWorldBorder() {
            return null;
        }

        @Override
        public net.minecraft.world.level.BlockGetter getChunkForCollisions(int x, int z) {
            return this;
        }

        @Override
        public List<VoxelShape> getEntityCollisions(Entity entity, AABB box) {
            return List.of();
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
