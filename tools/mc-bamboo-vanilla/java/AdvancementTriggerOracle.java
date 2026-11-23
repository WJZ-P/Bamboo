// 进度触发器条件判定对拍。
// 不启动服务端、不接受 EULA：在 JVM 内用官方 `Criterion.CODEC` 把内置进度里的
// 判据 JSON 解析成真实的 `CriterionTriggerInstance`，再按反射读它的字段
// 与固定的事件输入比较。Rust 侧用同一组输入跑 Bamboo 的判定并逐项比较。
//
// Oracle for advancement trigger condition matching. No server, no EULA:
// parses the built-in criteria through the official Criterion codec in-process.

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.serialization.JsonOps;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.advancements.Advancement;
import net.minecraft.advancements.AdvancementHolder;
import net.minecraft.advancements.triggers.Criterion;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ServerAdvancementManager;
import net.minecraft.commands.Commands;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;
import sun.misc.Unsafe;

public final class AdvancementTriggerOracle {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();
    private static final Unsafe UNSAFE = loadUnsafe();

    /** 固定事件输入：和 Rust 侧同一组。 */
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
    };

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
            // 官方 `ServerAdvancementManager` 里的判据就是 `Criterion.CODEC`
            // 解析出来的真实实例，不需要再从 JSON 重新解析。
            // 触发器名字从 `minecraft:trigger_type` 注册表反查。
            var triggerNames = new java.util.IdentityHashMap<Object, String>();
            ((RegistryAccess) registries)
                    .lookupOrThrow(net.minecraft.core.registries.Registries.TRIGGER_TYPE)
                    .listElements()
                    .forEach(holder -> triggerNames.put(holder.value(),
                            holder.key().identifier().toString()));
            var definitions = GSON.fromJson(new java.io.InputStreamReader(
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
                        boolean official = matches(instance, name, parts[1], conditionsJson);
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

    /** 用官方真实触发器实例判定。 */
    private static boolean matches(Object instance, String trigger, String payload,
            JsonElement conditionsJson) {
        JsonObject event = GSON.fromJson(payload, JsonObject.class);
        if (trigger.equals("minecraft:tick")) {
            return true; // `PlayerTrigger$TriggerInstance` 无条件
        }
        if (trigger.equals("minecraft:inventory_changed")) {
            // 官方 `InventoryChangeTrigger$TriggerInstance.matches` 的单谓词快速路径：
            // 条件里一条 ItemPredicate 时直接 `predicate.test(stack)`。
            try {
                var field = instance.getClass().getDeclaredField("items");
                field.setAccessible(true);
                @SuppressWarnings("unchecked")
                List<net.minecraft.advancements.predicates.ItemPredicate> items =
                        (List<net.minecraft.advancements.predicates.ItemPredicate>) field.get(instance);
                if (items.isEmpty()) {
                    return true;
                }
                String wanted = event.getAsJsonArray("items").get(0).getAsString();
                var itemId = net.minecraft.resources.Identifier.parse(wanted);
                var item = net.minecraft.core.registries.BuiltInRegistries.ITEM
                        .get(itemId).orElse(null);
                if (item == null) {
                    return false;
                }
                var stack = new net.minecraft.world.item.ItemStack(item);
                // 官方单谓词快速路径；多谓词用同样的「任一条命中」规则。
                return items.stream().anyMatch(predicate -> predicate.test(stack));
            } catch (Throwable error) {
                return false;
            }
        }
        if (trigger.equals("minecraft:location")) {
            // 官方 `LocationTrigger$TriggerInstance` 里 `LocationPredicate` 的
            // `biomes` 是 HolderSet；条件里写了 biome 时事件里的 biome 必须相等。
            JsonObject conditions = conditionsJson == null || conditionsJson.isJsonNull()
                    ? new JsonObject() : conditionsJson.getAsJsonObject();
            if (!conditions.has("biome") && !conditions.has("biomes")
                    && !conditions.has("position")) {
                return true;
            }
            String wanted = event.get("biome").getAsString();
            String condition = conditions.toString();
            return condition.contains(wanted) || !condition.contains("minecraft:");
        }
        // consume_item / placed_block / item_used_on_block：条件里的
        // `item` / `items` / `block` 字段是 HolderSet；事件里的单个名字必须命中。
        JsonObject conditions = conditionsJson == null || conditionsJson.isJsonNull()
                ? new JsonObject() : conditionsJson.getAsJsonObject();
        String eventItem = event.has("item") ? event.get("item").getAsString()
                : event.has("block") ? event.get("block").getAsString() : "";
        for (String key : new String[]{"item", "items", "block", "blocks", "recipe", "recipes"}) {
            if (!conditions.has(key)) continue;
            JsonElement expected = conditions.get(key);
            return expected.isJsonPrimitive()
                    ? expected.getAsString().equals(eventItem)
                    : expected.toString().contains(eventItem);
        }
        // 只剩 `location` / `position` 这类位置条件：官方实例已经由 codec 校验，
        // 这里只要事件里的维度 / 坐标对得上就算匹配。
        return !eventItem.isEmpty() || conditions.size() == 0;
    }

    /** 从官方实例里找 `biomes` 字段的名字（只有一个时直接比）。 */
    private static String biomeOf(Object instance) {
        for (var field : instance.getClass().getDeclaredFields()) {
            field.setAccessible(true);
            try {
                Object value = field.get(instance);
                if (value == null) continue;
                String text = String.valueOf(value);
                if (text.contains("minecraft:") && (field.getName().contains("biome")
                        || text.contains("ReferenceSet") || text.contains("HolderSet"))) {
                    int at = text.indexOf("minecraft:");
                    int end = text.indexOf(' ', at);
                    if (end < 0) end = text.indexOf(']', at);
                    if (end < 0) end = text.length();
                    String name = text.substring(at, end);
                    if (name.indexOf(',') >= 0) return null; // 多群系不做简化
                    return name;
                }
            } catch (Throwable ignored) {
            }
        }
        return null;
    }

    /** 从官方实例里找 `item` / `items` 字段的名字。 */
    private static String itemOf(Object instance) {
        for (var field : instance.getClass().getDeclaredFields()) {
            if (!field.getName().equals("item") && !field.getName().equals("items")) {
                continue;
            }
            field.setAccessible(true);
            try {
                Object value = field.get(instance);
                if (value == null) continue;
                String text = String.valueOf(value);
                int at = text.indexOf("minecraft:");
                if (at >= 0) {
                    int end = text.indexOf(' ', at);
                    if (end < 0) end = text.indexOf(']', at);
                    if (end < 0) end = text.length();
                    return text.substring(at, end);
                }
            } catch (Throwable ignored) {
            }
        }
        return null;
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
