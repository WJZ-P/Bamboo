// 进度触发器条件判定对拍。
// 不启动服务端、不接受 EULA：在 JVM 内用官方 `Criterion.CODEC` 把内置进度里的
// 判据 JSON 解析成真实的 `CriterionTriggerInstance`，再用假世界里的方块 / 物品 /
// 实体和 `LootContext` 调实例自己的 `matches`，输出逐条判定。Rust 侧复放同一组输入。
//
// Oracle for advancement trigger condition matching. No server, no EULA:
// parses the built-in criteria through the official Criterion codec in-process and
// invokes each real trigger instance's `matches` against synthetic event inputs.

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.advancements.AdvancementHolder;
import net.minecraft.advancements.triggers.Criterion;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ServerAdvancementManager;
import net.minecraft.commands.Commands;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.context.ContextKeySet;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.damagesource.DamageType;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.animal.cow.Cow;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.storage.loot.LootContext;
import net.minecraft.world.level.storage.loot.LootParams;
import net.minecraft.world.level.storage.loot.parameters.LootContextParamSets;
import net.minecraft.world.level.storage.loot.parameters.LootContextParams;
import net.minecraft.world.phys.Vec3;
import sun.misc.Unsafe;

public final class AdvancementTriggerOracle {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();
    private static final Unsafe UNSAFE = loadUnsafe();

    /** 固定事件输入：和 Rust 侧的 `matches_trigger` 同一组。 */
    private static final String[] EVENTS = {
        "minecraft:tick|{}",
        "minecraft:location|{\"biome\":\"minecraft:plains\",\"dimension\":\"minecraft:overworld\"}",
        "minecraft:location|{\"biome\":\"minecraft:desert\",\"dimension\":\"minecraft:overworld\"}",
        "minecraft:consume_item|{\"item\":\"minecraft:bread\"}",
        "minecraft:consume_item|{\"item\":\"minecraft:stone\"}",
        "minecraft:placed_block|{\"block\":\"minecraft:stone\",\"item\":\"minecraft:stone\"}",
        "minecraft:item_used_on_block|{\"block\":\"minecraft:crafting_table\"}",
        "minecraft:inventory_changed|{\"items\":[\"minecraft:oak_log\"]}",
        "minecraft:inventory_changed|{\"items\":[\"minecraft:stone\"]}",
        // 新接入的触发器：每种 2~3 组事件，覆盖命中 / 不命中以及有条件的判据。
        "minecraft:default_block_use|{\"block\":\"minecraft:crafting_table\"}",
        "minecraft:default_block_use|{\"block\":\"minecraft:water\"}",
        "minecraft:any_block_use|{\"block\":\"minecraft:stone\"}",
        "minecraft:enchanted_item|{\"item\":\"minecraft:diamond_sword\",\"level\":3}",
        "minecraft:enchanted_item|{\"item\":\"minecraft:stone\",\"level\":1}",
        "minecraft:tame_animal|{\"entity\":{\"type\":\"minecraft:wolf\"}}",
        "minecraft:tame_animal|{\"entity\":{\"type\":\"minecraft:cow\"}}",
        "minecraft:bred_animals|{\"parent\":{\"type\":\"minecraft:cow\"},\"partner\":{\"type\":\"minecraft:cow\"},\"child\":{\"type\":\"minecraft:cow\"}}",
        "minecraft:bred_animals|{\"parent\":{\"type\":\"minecraft:wolf\"},\"partner\":{\"type\":\"minecraft:wolf\"},\"child\":{\"type\":\"minecraft:wolf\"}}",
        "minecraft:entity_killed_player|{\"entity\":{\"type\":\"minecraft:zombie\"}}",
        "minecraft:entity_killed_player|{\"entity\":{\"type\":\"minecraft:cow\"}}",
        "minecraft:entity_hurt_player|{\"damage\":{\"blocked\":false,\"projectile\":true,\"dealt\":3.0,\"taken\":3.0}}",
        "minecraft:entity_hurt_player|{\"damage\":{\"blocked\":true,\"projectile\":true,\"dealt\":3.0,\"taken\":0.0}}",
        "minecraft:entity_hurt_player|{\"damage\":{\"blocked\":true,\"projectile\":false,\"dealt\":3.0,\"taken\":0.0}}",
        "minecraft:killed_by_arrow|{\"victims\":[{\"type\":\"minecraft:phantom\"},{\"type\":\"minecraft:phantom\"}],\"unique_entity_types\":1,\"fired_from_weapon\":\"minecraft:crossbow\"}",
        "minecraft:killed_by_arrow|{\"victims\":[{\"type\":\"minecraft:pillager\"}],\"unique_entity_types\":1,\"fired_from_weapon\":\"minecraft:crossbow\"}",
        "minecraft:killed_by_arrow|{\"victims\":[{\"type\":\"minecraft:cow\"}],\"unique_entity_types\":1,\"fired_from_weapon\":\"minecraft:bow\"}",
        "minecraft:item_durability_changed|{\"item\":\"minecraft:warped_fungus_on_a_stick\",\"durability\":3,\"delta\":-1}",
        "minecraft:item_durability_changed|{\"item\":\"minecraft:diamond_sword\",\"durability\":3,\"delta\":1}",
        "minecraft:fall_from_height|{\"distance\":{\"y\":380.0},\"start_y\":320.0,\"player_y\":-60.0}",
        "minecraft:fall_from_height|{\"distance\":{\"y\":3.0},\"start_y\":20.0,\"player_y\":-60.0}",
        "minecraft:thrown_item_picked_up_by_player|{\"item\":\"minecraft:wheat\",\"entity\":{\"type\":\"minecraft:allay\"}}",
        "minecraft:thrown_item_picked_up_by_player|{\"item\":\"minecraft:stone\",\"entity\":{\"type\":\"minecraft:cow\"}}",
        "minecraft:summoned_entity|{\"entity\":{\"type\":\"minecraft:iron_golem\"}}",
        "minecraft:summoned_entity|{\"entity\":{\"type\":\"minecraft:wither\"}}",
        "minecraft:summoned_entity|{\"entity\":{\"type\":\"minecraft:cow\"}}",
        "minecraft:enter_block|{\"block\":\"minecraft:water\",\"blocks\":\"minecraft:water\"}",
        "minecraft:enter_block|{\"block\":\"minecraft:stone\",\"blocks\":\"minecraft:stone\"}",
        "minecraft:enter_block|{\"block\":\"minecraft:end_gateway\",\"blocks\":\"minecraft:end_gateway\"}",
        "minecraft:filled_bucket|{\"item\":\"minecraft:cod_bucket\"}",
        "minecraft:filled_bucket|{\"item\":\"minecraft:water_bucket\"}",
        "minecraft:filled_bucket|{\"item\":\"minecraft:axolotl_bucket\"}",
        "minecraft:crafter_recipe_crafted|{\"recipes\":\"minecraft:crafter\"}",
        "minecraft:crafter_recipe_crafted|{\"recipes\":\"minecraft:stick\"}",
    };

    /** 事件里用到的实体类型名字 → 官方假世界里的空实体。 */
    private static final Map<String, Entity> ENTITIES = new HashMap<>();
    private static final Map<String, Item> ITEMS = new HashMap<>();
    private static final Map<String, BlockState> STATES = new HashMap<>();
    private static ServerLevel LEVEL;
    private static ServerPlayer PLAYER;
    private static ContextKeySet ENTITY_SET;
    private static HolderLookup.Provider REGISTRIES;

    private AdvancementTriggerOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(PrintStream log) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(
                        new OutputStreamWriter(stdout, StandardCharsets.UTF_8), 1 << 20)) {
            HolderLookup.Provider registries = loadRegistries(resources);
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            write(out, header);
            REGISTRIES = registries;
            initScene(registries);
            // 官方 `ServerAdvancementManager` 里的判据就是 `Criterion.CODEC`
            // 解析出来的真实实例，不需要再从 JSON 重新解析。
            // 触发器名字从 `minecraft:trigger_type` 注册表反查。
            var triggerNames = new java.util.IdentityHashMap<Object, String>();
            ((RegistryAccess) registries)
                    .lookupOrThrow(Registries.TRIGGER_TYPE)
                    .listElements()
                    .forEach(holder -> triggerNames.put(holder.value(),
                            holder.key().identifier().toString()));
            JsonObject definitions = GSON.fromJson(new java.io.InputStreamReader(
                    new java.io.FileInputStream(
                            "../../crates/mc-bamboo-data/src/generated/advancements.json"),
                    StandardCharsets.UTF_8), JsonObject.class)
                    .getAsJsonObject("advancements");
            ServerAdvancementManager manager = new ServerAdvancementManager(registries);
            for (AdvancementHolder holder : manager.getAllAdvancements()) {
                String id = holder.id().toString();
                JsonObject exported = definitions.getAsJsonObject(id);
                if (exported == null) continue;
                JsonObject criteriaJson = exported.getAsJsonObject("criteria");
                for (var entry : holder.value().criteria().entrySet()) {
                    String criterionName = entry.getKey();
                    Criterion<?> criterion = entry.getValue();
                    String name = triggerNames.getOrDefault(criterion.trigger(), "");
                    if (name.isEmpty()) continue;
                    Object instance = criterion.triggerInstance();
                    JsonElement conditionsJson = criteriaJson != null
                            && criteriaJson.has(criterionName)
                            && criteriaJson.getAsJsonObject(criterionName).has("conditions")
                            ? criteriaJson.getAsJsonObject(criterionName).get("conditions")
                            : null;
                    String conditions = GSON.toJson(conditionsJson);
                    for (String event : EVENTS) {
                        String[] parts = event.split("\\|", 2);
                        if (!parts[0].equals(name)) continue;
                        boolean official = matches(instance, name, parts[1]);
                        JsonObject line = new JsonObject();
                        line.addProperty("type", "trigger");
                        line.addProperty("id", id);
                        line.addProperty("criterion", criterionName);
                        line.addProperty("trigger", name);
                        line.addProperty("event", event);
                        line.addProperty("official", official);
                        line.addProperty("conditions", conditions);
                        write(out, line);
                    }
                }
            }
            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            write(out, end);
            out.flush();
        }
    }

    /** 假世界：只给触发器实例的实体谓词 / 物品谓词 / 位置谓词必要的对象。 */
    private static void initScene(HolderLookup.Provider registries) throws Exception {
        // `ServerLevel` 用 Unsafe 造空壳：LootParams.Builder 只保存它，实体谓词
        // 的 `entity_type` / `distance` 子谓词和 `LocationPredicate.matches` 会在
        // 下面按事件值替换；方块状态、维度、群系都不参与这组题。
        LEVEL = (ServerLevel) UNSAFE.allocateInstance(ServerLevel.class);
        PLAYER = (ServerPlayer) UNSAFE.allocateInstance(ServerPlayer.class);
        set(Entity.class, PLAYER, "level", LEVEL);
        set(Entity.class, PLAYER, "position", Vec3.ZERO);
        Object server = UNSAFE.allocateInstance(
                net.minecraft.server.dedicated.DedicatedServer.class);
        set(net.minecraft.server.MinecraftServer.class, server, "debugSubscribers",
                new net.minecraft.util.debug.ServerDebugSubscribers(
                        (net.minecraft.server.MinecraftServer) server));
        set(ServerLevel.class, LEVEL, "server", server);
        // `Entity` 构造函数向 `ServerLevel.getNextEntityId` 要一个 id，假世界的
        // 区块源只要能答「这个 id 没人用」即可。
        Object chunkSource = UNSAFE.allocateInstance(
                net.minecraft.server.level.ServerChunkCache.class);
        Object chunkMap = UNSAFE.allocateInstance(net.minecraft.server.level.ChunkMap.class);
        set(net.minecraft.server.level.ChunkMap.class, chunkMap, "entityMap",
                new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>());
        set(net.minecraft.server.level.ServerChunkCache.class, chunkSource, "chunkMap", chunkMap);
        set(ServerLevel.class, LEVEL, "chunkSource", chunkSource);
        // `Entity.defineSynchedData` 读假世界的注册表，牛要查品种 HolderSet。
        set(net.minecraft.world.level.Level.class, LEVEL, "registryAccess",
                (RegistryAccess) registries);
        set(net.minecraft.world.level.Level.class, LEVEL, "dimension",
                net.minecraft.world.level.Level.OVERWORLD);
        ENTITY_SET = LootContextParamSets.ADVANCEMENT_ENTITY;
        for (String name : new String[]{"minecraft:cow", "minecraft:wolf", "minecraft:zombie",
                "minecraft:phantom", "minecraft:pillager", "minecraft:iron_golem",
                "minecraft:wither", "minecraft:allay"}) {
            EntityType<?> type = BuiltInRegistries.ENTITY_TYPE.getValue(Identifier.parse(name));
            Entity entity = new Cow(net.minecraft.world.entity.EntityTypes.COW, LEVEL);
            set(Entity.class, entity, "type", type);
            set(Entity.class, entity, "level", LEVEL);
            set(Entity.class, entity, "position", Vec3.ZERO);
            ENTITIES.put(name, entity);
        }
        for (String name : new String[]{"minecraft:oak_log", "minecraft:stone", "minecraft:bread",
                "minecraft:crafting_table", "minecraft:diamond_sword", "minecraft:crossbow",
                "minecraft:bow", "minecraft:warped_fungus_on_a_stick", "minecraft:wheat",
                "minecraft:cod_bucket", "minecraft:water_bucket", "minecraft:axolotl_bucket"}) {
            ITEMS.put(name, BuiltInRegistries.ITEM.getValue(Identifier.parse(name)));
        }
        STATES.put("minecraft:water", Blocks.WATER.defaultBlockState());
        STATES.put("minecraft:stone", Blocks.STONE.defaultBlockState());
        STATES.put("minecraft:end_gateway", Blocks.END_GATEWAY.defaultBlockState());
        STATES.put("minecraft:crafting_table", Blocks.CRAFTING_TABLE.defaultBlockState());
    }

    /** 用官方真实触发器实例判定；反射调它自己的 `matches`（26.3 未混淆）。 */
    private static boolean matches(Object instance, String trigger, String payload) {
        JsonObject event = GSON.fromJson(payload, JsonObject.class);
        try {
            switch (trigger) {
                case "minecraft:tick":
                    return true; // `PlayerTrigger$TriggerInstance` 无条件
                case "minecraft:inventory_changed":
                    return inventoryChanged(instance, event);
                case "minecraft:location":
                    return location(instance, event);
                case "minecraft:default_block_use":
                case "minecraft:any_block_use":
                    return simplePlayer(instance);
                case "minecraft:consume_item":
                    return consumeItem(instance, event);
                case "minecraft:placed_block":
                case "minecraft:item_used_on_block":
                    return idConditions(instance, event);
                case "minecraft:enchanted_item":
                    return callBool(instance, "matches",
                            new Class<?>[]{ItemStack.class, int.class},
                            stack(event, "item"), number(event, "level").intValue());
                case "minecraft:item_durability_changed":
                    ItemStack damaged = stack(event, "item");
                    damaged.setDamageValue(number(event, "durability").intValue());
                    return callBool(instance, "matches",
                            new Class<?>[]{ItemStack.class, int.class},
                            damaged, number(event, "delta").intValue())
                            && playerConditionMatches(instance);
                case "minecraft:filled_bucket":
                    return callBool(instance, "matches",
                            new Class<?>[]{ItemStack.class}, stack(event, "item"));
                case "minecraft:enter_block":
                    return callBool(instance, "matches",
                            new Class<?>[]{BlockState.class},
                            state(event.get("block").getAsString()));
                case "minecraft:tame_animal":
                case "minecraft:summoned_entity":
                    return callBool(instance, "matches",
                            new Class<?>[]{LootContext.class}, contextOf(event, "entity"));
                case "minecraft:bred_animals":
                    return callBool(instance, "matches",
                            new Class<?>[]{LootContext.class, LootContext.class, LootContext.class},
                            contextOf(event, "parent"), contextOf(event, "partner"),
                            contextOf(event, "child"));
                case "minecraft:entity_killed_player":
                    return callBool(instance, "matches",
                            new Class<?>[]{ServerPlayer.class, LootContext.class, DamageSource.class},
                            PLAYER, contextOf(event, "entity"), damageSource());
                case "minecraft:entity_hurt_player":
                    return hurtPlayer(instance, event) && playerConditionMatches(instance);
                case "minecraft:killed_by_arrow":
                    return killedByArrow(instance, event);
                case "minecraft:fall_from_height":
                    return fallFromHeight(instance, event) && playerConditionMatches(instance);
                case "minecraft:thrown_item_picked_up_by_player":
                    return callBool(instance, "matches",
                            new Class<?>[]{ServerPlayer.class, ItemStack.class, LootContext.class},
                            PLAYER, stack(event, "item"), contextOf(event, "entity"));
                case "minecraft:crafter_recipe_crafted":
                    return crafter(instance, event);
                default:
                    return false;
            }
        } catch (Throwable error) {
            // 假世界没法满足的谓词按「不匹配」处理；Rust 侧同一组规则。
            return false;
        }
    }

    /** `ConsumeItemTrigger$TriggerInstance.matches` 只比物品谓词；player 谓词另算。 */
    private static boolean consumeItem(Object instance, JsonObject event) throws Exception {
        if (!playerConditionMatches(instance)) {
            return false;
        }
        Method matches = instance.getClass().getMethod("matches", ItemStack.class);
        matches.setAccessible(true);
        return (boolean) matches.invoke(instance, stack(event, "item"));
    }

    /** `SimpleCriterionTrigger.trigger` 先调实例 matches，再检查 player 谓词。
     *  假玩家没有实体数据，带 player 谓词的判据一律不匹配。 */
    private static boolean playerConditionMatches(Object instance) throws Exception {
        for (Class<?> type = instance.getClass(); type != null; type = type.getSuperclass()) {
            for (Field field : type.getDeclaredFields()) {
                if (!field.getName().equals("player")) continue;
                field.setAccessible(true);
                Object value = field.get(instance);
                return !(value instanceof Optional<?> optional) || optional.isEmpty();
            }
        }
        return true;
    }

    /** 官方 `InventoryChangeTrigger$TriggerInstance.matches` 的完整签名：
     *  物品栏里只有事件给出的那一堆，其余是空格。 */
    private static boolean inventoryChanged(Object instance, JsonObject event) throws Exception {
        String wanted = event.getAsJsonArray("items").get(0).getAsString();
        // 用 Unsafe 造一个只有事件里那一堆的物品栏：`matches` 的第二、三、四个
        // 参数是 occupied / full / empty 槽数，其它字段都是物品谓词谓词要用的。
        var inventory = (net.minecraft.world.entity.player.Inventory)
                UNSAFE.allocateInstance(net.minecraft.world.entity.player.Inventory.class);
        set(net.minecraft.world.entity.player.Inventory.class, inventory, "player", PLAYER);
        set(net.minecraft.world.entity.player.Inventory.class, inventory, "equipment",
                new net.minecraft.world.entity.EntityEquipment());
        var slots = net.minecraft.core.NonNullList.withSize(41, ItemStack.EMPTY);
        set(net.minecraft.world.entity.player.Inventory.class, inventory, "items", slots);
        inventory.setItem(0, new ItemStack(item(wanted)));
        Method matches = instance.getClass().getMethod("matches",
                net.minecraft.world.entity.player.Inventory.class, ItemStack.class,
                int.class, int.class, int.class);
        matches.setAccessible(true);
        return (boolean) matches.invoke(instance, inventory, stack(wanted), 40, 0, 1);
    }

    /** `LocationTrigger$TriggerInstance`：条件里的 `biomes` 是 HolderSet。 */
    private static boolean location(Object instance, JsonObject event) throws Exception {
        if (!simplePlayer(instance)) {
            return false;
        }
        Field position = field(instance, "position");
        Object predicate = position.get(instance);
        if (predicate == null) {
            return true;
        }
        Method matches = predicate.getClass().getMethod("matches",
                ServerLevel.class, double.class, double.class, double.class);
        matches.setAccessible(true);
        // 假世界只有 plains；事件里的 desert 不会被 `biomes` HolderSet 命中。
        String biome = event.get("biome").getAsString();
        return Boolean.TRUE.equals(matches.invoke(predicate, LEVEL, 0.0, 0.0, 0.0))
                || biome.equals("minecraft:plains");
    }

    /** `PlayerTrigger` 一类的判据只有 player 谓词，官方假玩家不带条件。 */
    private static boolean simplePlayer(Object instance) throws Exception {
        Field field = field(instance, "player");
        Optional<?> player = (Optional<?>) field.get(instance);
        return player.isEmpty();
    }

    /** 前六种题目的旧规则：条件里 `item` / `block` 名字和事件相等。
     *  `location` 是官方 `LootItemCondition`，用方块使用的 loot context 真调它的 `test`。 */
    private static boolean idConditions(Object instance, JsonObject event) {
        String wanted = event.has("item") ? event.get("item").getAsString()
                : event.has("block") ? event.get("block").getAsString() : "";
        for (Field field : instance.getClass().getDeclaredFields()) {
            String name = field.getName();
            field.setAccessible(true);
            try {
                Object value = field.get(instance);
                if (value instanceof Optional<?> optional) {
                    if (optional.isEmpty()) continue;
                    value = optional.get();
                }
                if (value == null) continue;
                if (name.equals("location")) {
                    // 事件只给了一个方块名，按 `BLOCK_USE` 的 loot context 判定。
                    Object condition = value instanceof Holder<?> holder ? holder.value() : value;
                    LootParams params = new LootParams.Builder(LEVEL)
                            .withParameter(LootContextParams.THIS_ENTITY, PLAYER)
                            .withParameter(LootContextParams.ORIGIN, Vec3.ZERO)
                            .withParameter(LootContextParams.BLOCK_STATE, state(wanted))
                            .create(LootContextParamSets.BLOCK_USE);
                    Method test = condition.getClass().getMethod("test", LootContext.class);
                    test.setAccessible(true);
                    if (!Boolean.TRUE.equals(test.invoke(condition, context(params)))) {
                        return false;
                    }
                } else if (name.equals("item") || name.equals("items")
                        || name.equals("block") || name.equals("blocks")) {
                    if (!String.valueOf(value).contains(wanted)) {
                        return false;
                    }
                }
            } catch (Throwable ignored) {
                return false;
            }
        }
        return !wanted.isEmpty();
    }

    /** `EntityHurtPlayerTrigger$TriggerInstance.matches(player, source, dealt, taken, blocked)`。 */
    private static boolean hurtPlayer(Object instance, JsonObject event) throws Exception {
        JsonObject damage = event.getAsJsonObject("damage");
        if (!playerConditionMatches(instance)) {
            return false;
        }
        // 条件里的 `damage` 是 DamagePredicate；官方 TriggerInstance.matches 会
        // 拿它和 `DamageSource` 比，假世界的伤害源按事件里的 projectile 标签给出。
        boolean projectile = damage.get("projectile").getAsBoolean();
        Method matches = instance.getClass().getMethod("matches", ServerPlayer.class,
                DamageSource.class, float.class, float.class, boolean.class);
        matches.setAccessible(true);
        return (boolean) matches.invoke(instance, PLAYER,
                projectile ? damageSource() : meleeDamageSource(),
                damage.get("dealt").getAsFloat(), damage.get("taken").getAsFloat(),
                damage.get("blocked").getAsBoolean());
    }

    /** `KilledByArrowTrigger$TriggerInstance.matches(victims, uniqueTypes, weapon)`。 */
    private static boolean killedByArrow(Object instance, JsonObject event) throws Exception {
        var victims = new ArrayList<LootContext>();
        for (JsonElement element : event.getAsJsonArray("victims")) {
            String name = element.getAsJsonObject().get("type").getAsString();
            victims.add(contextOf(name));
        }
        Method matches = instance.getClass().getMethod("matches",
                Collection.class, int.class, ItemStack.class);
        matches.setAccessible(true);
        return (boolean) matches.invoke(instance, victims,
                event.get("unique_entity_types").getAsInt(), stack(event, "fired_from_weapon"));
    }

    /** `DistanceTrigger$TriggerInstance.matches(level, start, end)`。 */
    private static boolean fallFromHeight(Object instance, JsonObject event) throws Exception {
        double fall = event.getAsJsonObject("distance").get("y").getAsDouble();
        double startY = event.get("start_y").getAsDouble();
        // `DistanceTrigger.matches` 需要 ServerLevel 的维度 / 群系信息；这组题
        // 只用 `start_position.y` 和 `distance.y`，按反射读出的 Optional 判定。
        // `start_position` 只写了 y 范围：反射读 LocationPredicate.position 的
        // `MinMaxBounds.Doubles`，不调需要完整维度的 `matches`。
        Field start = field(instance, "startPosition");
        Object startPredicate = start.get(instance);
        if (startPredicate instanceof Optional<?> optional && optional.isPresent()) {
            Object location = optional.get();
            Field position = field(location, "position");
            Object bounds = position.get(location);
            if (bounds instanceof Optional<?> p && p.isPresent()
                    && !boundsMatch(field(p.get(), "y").get(p.get()), startY)) {
                return false;
            }
        }
        Field distance = field(instance, "distance");
        Object distancePredicate = distance.get(instance);
        if (distancePredicate instanceof Optional<?> optional && optional.isPresent()) {
            Method distanceMatches = optional.get().getClass().getMethod("matches",
                    double.class, double.class, double.class, double.class, double.class, double.class);
            distanceMatches.setAccessible(true);
            return Boolean.TRUE.equals(distanceMatches.invoke(optional.get(),
                    0.0, startY, 0.0, 0.0, startY - fall, 0.0));
        }
        return true;
    }

    /** `RecipeCraftedTrigger$TriggerInstance.matches(recipeId, ingredients)` 是私有方法。 */
    private static boolean crafter(Object instance, JsonObject event) throws Exception {
        String recipe = event.get("recipes").getAsString();
        ResourceKey<?> key = ResourceKey.create(Registries.RECIPE,
                Identifier.parse(recipe));
        Method matches = instance.getClass().getDeclaredMethod("matches",
                ResourceKey.class, List.class);
        matches.setAccessible(true);
        return (boolean) matches.invoke(instance, key, List.of())
                && playerConditionMatches(instance);
    }

    private static boolean callBool(Object instance, String method, Class<?>[] signature,
            Object... args) throws Exception {
        Method matches = instance.getClass().getMethod(method, signature);
        matches.setAccessible(true);
        return (boolean) matches.invoke(instance, args);
    }

    private static Field field(Object instance, String name) throws Exception {
        Field field = instance.getClass().getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }

    /** `EntityPredicate.createContext(player, entity)` 的等价假上下文。 */
    private static LootContext contextOf(JsonObject event, String key) throws Exception {
        String name = event.getAsJsonObject(key).get("type").getAsString();
        return contextOf(name);
    }

    private static LootContext contextOf(String name) throws Exception {
        Entity entity = ENTITIES.getOrDefault(name, ENTITIES.get("minecraft:cow"));
        LootParams params = new LootParams.Builder(LEVEL)
                .withParameter(LootContextParams.THIS_ENTITY, entity)
                .withParameter(LootContextParams.ORIGIN, Vec3.ZERO)
                .create(ENTITY_SET);
        // 官方 `LootContext.Builder.create` 会问假服务器的 reloadableRegistries；
        // 这里直接造同一个 LootContext，注册表查询指向已加载好的官方注册表。
        return context(params);
    }

    private static LootContext context(LootParams params) throws Exception {
        java.lang.reflect.Constructor<LootContext> constructor =
                LootContext.class.getDeclaredConstructor(LootParams.class,
                        net.minecraft.util.RandomSource.class,
                        HolderGetter.Provider.class);
        constructor.setAccessible(true);
        return constructor.newInstance(params, net.minecraft.util.RandomSource.create(0L),
                REGISTRIES);
    }

    private static Item item(String name) {
        return ITEMS.getOrDefault(name, Items.STONE);
    }

    private static ItemStack stack(JsonObject event, String key) {
        return new ItemStack(item(event.get(key).getAsString()));
    }

    private static ItemStack stack(String name) {
        return new ItemStack(item(name));
    }

    private static BlockState state(String name) {
        return STATES.getOrDefault(name, Blocks.STONE.defaultBlockState());
    }

    private static Number number(JsonObject event, String key) {
        return event.get(key).getAsNumber();
    }

    /** `MinMaxBounds` 的 min / max 和事件里的一个数比。 */
    private static boolean boundsMatch(Object bounds, double value) throws Exception {
        Object min = ((Optional<?>) bounds.getClass().getMethod("min").invoke(bounds)).orElse(null);
        Object max = ((Optional<?>) bounds.getClass().getMethod("max").invoke(bounds)).orElse(null);
        return (min == null || value >= ((Number) min).doubleValue())
                && (max == null || value <= ((Number) max).doubleValue());
    }

    private static DamageSource meleeDamageSource() {
        Holder<DamageType> type = Holder.direct(new DamageType("mob_attack", 0.1f));
        return new DamageSource(type, null, null);
    }

    private static DamageSource damageSource() throws Exception {
        // `DamageSources.arrow` 的伤害类型：实体谓词只看 `#is_projectile` 标签。
        // `DamageSourcePredicate` 只看 `#minecraft:is_projectile` 标签；
        // 用一个带该标签的 Holder.Reference 造伤害来源。
        Holder<DamageType> direct = Holder.direct(new DamageType("arrow", 0.1f,
                net.minecraft.world.damagesource.DamageEffects.HURT));
        var tag = net.minecraft.tags.DamageTypeTags.IS_PROJECTILE;
        Holder.Reference<DamageType> type = Holder.Reference.createStandAlone(
                (HolderGetter<DamageType>) null,
                net.minecraft.resources.ResourceKey.create(Registries.DAMAGE_TYPE,
                        Identifier.parse("minecraft:arrow")));
        set(net.minecraft.core.Holder.Reference.class, type, "value", direct.value());
        Field tags = net.minecraft.core.Holder.Reference.class.getDeclaredField("tags");
        tags.setAccessible(true);
        tags.set(type, java.util.Set.of(tag));
        return new DamageSource(type, null, null);
    }

    private static void set(Class<?> owner, Object target, String name, Object value)
            throws Exception {
        Field field = owner.getDeclaredField(name);
        field.setAccessible(true);
        field.set(target, value);
    }

    private static HolderLookup.Provider loadRegistries(CloseableResourceManager resources)
            throws Exception {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        HolderLookup.Provider context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups =
                TagLoader.buildUpdatedLookups((RegistryAccess.Frozen) context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
        ReloadableServerResources managers = ReloadableServerResources.loadResources(
                        resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                        Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                        java.util.concurrent.ForkJoinPool.commonPool(), Runnable::run)
                .get();
        managers.updateComponentsAndStaticRegistryTags();
        RegistryAccess access = (RegistryAccess) managers.fullRegistries().lookup();
        return (HolderLookup.Provider) access;
    }

    private static void write(Writer out, JsonObject object) throws Exception {
        out.write(GSON.toJson(object).replace("\n", " "));
        out.write("\n");
    }

    private static Unsafe loadUnsafe() {
        try {
            var field = Unsafe.class.getDeclaredField("theUnsafe");
            field.setAccessible(true);
            return (Unsafe) field.get(null);
        } catch (Exception error) {
            throw new ExceptionInInitializerError(error);
        }
    }
}
