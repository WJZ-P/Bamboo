import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.io.IOException;
import java.io.Reader;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.Vec3i;
import net.minecraft.core.component.BlockTransformer;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.KnownPack;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.Resource;
import net.minecraft.server.packs.resources.ResourceManager;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.tags.TagNetworkSerialization;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.HoneycombItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.StandingAndWallBlockItem;
import net.minecraft.world.item.crafting.Ingredient;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.ItemLike;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
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
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.block.state.properties.WoodType;
import net.minecraft.world.level.levelgen.blockpredicates.AllOfPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.BlockPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.CombiningPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.MatchingBlockTagPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.MatchingBlocksPredicate;
import net.minecraft.world.level.levelgen.blockpredicates.StateTestingPredicate;
import net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.CopyPropertiesProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.RuleBasedStateProvider;
import net.minecraft.world.level.levelgen.feature.stateproviders.SimpleStateProvider;
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
    private static final int FACT_AIR = 1 << 18;
    private static final int FACT_CAN_OCCLUDE = 1 << 19;
    private static final int FACT_SOLID = 1 << 20;
    private static final int FACT_LIQUID = 1 << 21;
    private static final int FACT_REPLACEABLE = 1 << 22;
    private static final int FACT_REDSTONE_CONDUCTOR = 1 << 23;
    private static final int FACT_COLLISION_FULL_BLOCK = 1 << 24;
    private static final int FACT_SOLID_RENDER = 1 << 25;
    private static final int FACT_SIGNAL_SOURCE = 1 << 26;
    private static final int FACT_ANALOG_OUTPUT = 1 << 27;
    private static final int FACT_PROPAGATES_SKYLIGHT = 1 << 28;
    private static final int STURDY_MASK = (1 << 18) - 1;

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
            root.add("blockMaps", blockMaps(resources, layers.compositeAccess()));
            root.add("blockTransformers", blockTransformers(layers.compositeAccess()));

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
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
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
            addBlockEntityType(object, state);
            object.addProperty("class", className(block.getClass()));
            object.add("ancestors", ancestors(block.getClass()));
            object.add("properties", properties(block));
            object.add("fields", fields(block));
            object.addProperty("instrument", state.instrument().getSerializedName());
            object.addProperty("dynamicShape", block.hasDynamicShape());
            addOffset(object, block);
            blocks.add(object);
        }
        return blocks;
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
            result.add(object);
        }
        return result;
    }

    private static <T extends Comparable<T>> JsonArray valueNames(Property<T> property) {
        JsonArray names = new JsonArray();
        property.getPossibleValues().forEach(value -> names.add(property.getName(value)));
        return names;
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
            object.addProperty("overridesCanDestroyBlock", overridesCanDestroyBlock(item));
            items.add(object);
        }
        return items;
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
        maps.add("campfireInputs", campfireInputs(resources, registries));
        return maps;
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
            int expected = stateFacts(state, BlockPos.ZERO);
            Vec3 zeroOffset = state.getOffset(BlockPos.ZERO);
            for (BlockPos pos : SAMPLE_POSITIONS) {
                int found = stateFacts(state, pos);
                int mask = movesShape ? ~STURDY_MASK : ~0;
                if ((found & mask) != (expected & mask)) {
                    throw new IllegalStateException(state + " 的事实和位置有关：" + pos);
                }
                if (movesShape) {
                    Vec3 delta = state.getOffset(pos).subtract(zeroOffset);
                    VoxelShape moved = state.getShape(EMPTY, BlockPos.ZERO).move(delta.x, delta.y, delta.z);
                    if (!shapeKey(moved).equals(shapeKey(state.getShape(EMPTY, pos)))) {
                        throw new IllegalStateException(state + " 的形状不只是跟着偏移平移：" + pos);
                    }
                    addOffsetSturdySample(id, pos, found);
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
        private static int stateFacts(BlockState state, BlockPos pos) {
            int facts = 0;
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
}
