// 官方 26.3 进度树的对拍：加载内置进度定义，导出父子、判据、requirements、显示、
// 奖励、当次 JVM 的兄弟顺序、坐标和单条 update 包的字节。
//
// 不启动服务端、不读存档、不接受 EULA。stdout 每行一个 JSON 对象。
// 兄弟顺序由官方 `AdvancementNode.children`（身份哈希）决定，默认 JVM 每次进程都不同；
// Rust 侧拿这个顺序当输入，用自己的排版算法算坐标再逐字节比。

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.serialization.JsonOps;
import io.netty.buffer.Unpooled;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.advancements.Advancement;
import net.minecraft.advancements.AdvancementHolder;
import net.minecraft.advancements.AdvancementNode;
import net.minecraft.advancements.AdvancementProgress;
import net.minecraft.advancements.AdvancementRequirements;
import net.minecraft.advancements.triggers.Criterion;
import net.minecraft.advancements.CriterionProgress;
import net.minecraft.advancements.DisplayInfo;
import net.minecraft.commands.Commands;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundUpdateAdvancementsPacket;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ServerAdvancementManager;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;

public final class AdvancementOracle {
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();

    private AdvancementOracle() {
    }

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
        String command = args.length == 0 ? "tree" : args[0];
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // 默认 JVM 的身份哈希按线程内的确定性序列发号；先往一个 identity hash set
        // 插若干对象，让三次独立进程拿到不同的兄弟顺序（仍是不加 -XX:hashCode 的默认 JVM）。
        int salt = args.length > 1 ? Integer.parseInt(args[1]) : 0;
        for (int i = 0; i < salt; i++) {
            new it.unimi.dsi.fastutil.objects.ReferenceOpenHashSet<Object>()
                    .add(new Object());
        }
        ServerAdvancementManager manager = new ServerAdvancementManager(REGISTRIES);
        try (Writer out = new BufferedWriter(
                new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("total", manager.getAllAdvancements().size());
            header.addProperty("salt", salt);
            write(out, header);
            if (command.equals("tree") || command.equals("layout")) {
                emitOrder(out, manager);
                if (command.equals("tree")) {
                    emitDefinitions(out, manager);
                } else {
                    emitLayout(out, manager);
                }
            } else if (command.equals("save")) {
                emitSave(out);
            } else {
                throw new IllegalArgumentException("unknown command: " + command);
            }
            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            write(out, end);
            out.flush();
        }
    }

    private static void emitDefinitions(Writer out, ServerAdvancementManager manager) throws Exception {
        // 每个进度一行：id、父、判据、requirements、显示、奖励。
        for (AdvancementHolder holder : manager.getAllAdvancements()) {
            Advancement value = holder.value();
            JsonObject item = new JsonObject();
            item.addProperty("type", "definition");
            item.addProperty("id", holder.id().toString());
            item.addProperty("parent", value.parent().map(Identifier::toString).orElse(""));
            item.add("criteria", criteria(value.criteria()));
            JsonArray requirements = new JsonArray();
            for (List<String> group : value.requirements().requirements()) {
                JsonArray names = new JsonArray();
                group.forEach(names::add);
                requirements.add(names);
            }
            item.add("requirements", requirements);
            item.add("display", display(value.display()));
            item.add("rewards", rewards(value.rewards()));
            item.addProperty("telemetry", value.sendsTelemetryEvent());
            write(out, item);
        }
    }

    private static void emitOrder(Writer out, ServerAdvancementManager manager) throws Exception {
        // 当次 JVM 的兄弟顺序：节点 id -> 直接子节点的 id 列表（含无显示的节点）。
        JsonObject order = new JsonObject();
        order.addProperty("type", "order");
        JsonObject children = new JsonObject();
        for (AdvancementNode node : manager.tree().nodes()) {
            JsonArray names = new JsonArray();
            for (AdvancementNode child : node.children()) {
                names.add(child.holder().id().toString());
            }
            children.add(node.holder().id().toString(), names);
        }
        order.add("children", children);
        write(out, order);
    }

    private static void emitLayout(Writer out, ServerAdvancementManager manager) throws Exception {
        RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registryAccess());
        // 关键：官方 `AdvancementNode.children` 是身份哈希集合，两次遍历之间身份哈希
        // 发号会推进，顺序未必相同。这里先为每个根重建一次 `TreeNodePosition` 树，
        // 把「排版真正看到的直接孩子顺序」导出，再让同一棵树跑完三步并读坐标。
        // Rust 侧拿这份顺序当输入，比较的才是同一份树。
        Class<?> positionClass = Class.forName("net.minecraft.advancements.TreeNodePosition");
        java.lang.reflect.Constructor<?> ctor = positionClass.getDeclaredConstructor(
                AdvancementNode.class, positionClass, positionClass, int.class, int.class);
        ctor.setAccessible(true);
        java.lang.reflect.Field nodeField = positionClass.getDeclaredField("node");
        nodeField.setAccessible(true);
        java.lang.reflect.Field childrenField = positionClass.getDeclaredField("children");
        childrenField.setAccessible(true);
        java.lang.reflect.Field xField = positionClass.getDeclaredField("x");
        xField.setAccessible(true);
        java.lang.reflect.Field yField = positionClass.getDeclaredField("y");
        yField.setAccessible(true);
        java.lang.reflect.Method first = positionClass.getDeclaredMethod("firstWalk");
        first.setAccessible(true);
        java.lang.reflect.Method second = positionClass.getDeclaredMethod(
                "secondWalk", float.class, int.class, float.class);
        second.setAccessible(true);
        java.lang.reflect.Method third = positionClass.getDeclaredMethod("thirdWalk", float.class);
        third.setAccessible(true);
        java.util.Map<String, Object> positions = new java.util.LinkedHashMap<>();
        for (AdvancementNode root : manager.tree().roots()) {
            if (root.advancement().display().isEmpty()) {
                continue;
            }
            Object position = ctor.newInstance(root, null, null, 1, 0);
            java.util.ArrayDeque<Object> queue = new java.util.ArrayDeque<>();
            queue.add(position);
            while (!queue.isEmpty()) {
                Object current = queue.remove();
                AdvancementNode node = (AdvancementNode) nodeField.get(current);
                String id = node.holder().id().toString();
                positions.put(id, current);
                JsonObject order = new JsonObject();
                order.addProperty("type", "layout-node");
                order.addProperty("id", id);
                @SuppressWarnings("unchecked")
                List<Object> children = (List<Object>) childrenField.get(current);
                JsonArray names = new JsonArray();
                for (Object child : children) {
                    names.add(((AdvancementNode) nodeField.get(child)).holder().id().toString());
                    queue.add(child);
                }
                order.add("children", names);
                write(out, order);
            }
            first.invoke(position);
            float minimum = (float) second.invoke(position, 0.0f, 0, yField.getFloat(position));
            if (minimum < 0.0f) {
                third.invoke(position, -minimum);
            }
        }
        // 全部显示节点的坐标，以及每个节点单独组成一条 update 包时的字节。
        List<AdvancementNode> shown = new ArrayList<>();
        for (AdvancementNode node : manager.tree().nodes()) {
            if (node.advancement().display().isPresent()) {
                shown.add(node);
            }
        }
        shown.sort(Comparator.comparing(node -> node.holder().id().toString()));
        for (AdvancementNode node : shown) {
            Object position = positions.get(node.holder().id().toString());
            ClientboundUpdateAdvancementsPacket packet = new ClientboundUpdateAdvancementsPacket(
                    true,
                    List.of(ClientboundUpdateAdvancementsPacket.PositionedAdvancement.fromNode(node)),
                    java.util.Set.of(),
                    Map.of(),
                    true);
            buffer.clear();
            ClientboundUpdateAdvancementsPacket.STREAM_CODEC.encode(buffer, packet);
            byte[] bytes = new byte[buffer.readableBytes()];
            buffer.getBytes(buffer.readerIndex(), bytes);
            JsonObject item = new JsonObject();
            item.addProperty("type", "position");
            item.addProperty("id", node.holder().id().toString());
            item.addProperty("x", xField.getInt(position));
            item.addProperty("y", yField.getFloat(position));
            item.addProperty("packet", Base64.getEncoder().encodeToString(bytes));
            write(out, item);
        }
    }


    private static void emitSave(Writer out) throws Exception {
        // 官方 `PlayerAdvancements$Data.CODEC` 写出的 JSON，让 Rust 侧比较键顺序和格式。
        Map<Identifier, AdvancementProgress> map = new LinkedHashMap<>();
        AdvancementProgress first = new AdvancementProgress();
        first.update(new AdvancementRequirements(List.of(List.of("a", "b"), List.of("c"))));
        setCriterion(first, "a", java.time.Instant.parse("2026-01-02T03:04:05Z"));
        setCriterion(first, "c", java.time.Instant.parse("2026-02-03T04:05:06Z"));
        map.put(Identifier.parse("minecraft:story/root"), first);
        AdvancementProgress second = new AdvancementProgress();
        second.update(new AdvancementRequirements(List.of(List.of("only"))));
        map.put(Identifier.parse("minecraft:adventure/root"), second);
        JsonObject root = (JsonObject) dataCodec().encodeStart(JsonOps.INSTANCE, dataObject(map))
                .getOrThrow();
        root.addProperty("DataVersion", SharedConstants.getCurrentVersion().dataVersion().version());
        JsonObject item = new JsonObject();
        item.addProperty("type", "save");
        item.addProperty("json", GSON.toJson(root));
        write(out, item);
    }

    private static void setCriterion(AdvancementProgress progress, String name, java.time.Instant instant)
            throws Exception {
        // CriterionProgress(intant) 是公开的；grant() 用当前时刻，对拍要用固定时刻。
        Field criteria = AdvancementProgress.class.getDeclaredField("criteria");
        criteria.setAccessible(true);
        @SuppressWarnings("unchecked")
        Map<String, CriterionProgress> values = (Map<String, CriterionProgress>) criteria.get(progress);
        values.put(name, new CriterionProgress(instant));
    }

    private static Object dataObject(Map<Identifier, AdvancementProgress> map) throws Exception {
        Class<?> data = Class.forName("net.minecraft.server.PlayerAdvancements$Data");
        java.lang.reflect.Constructor<?> constructor = data.getDeclaredConstructor(Map.class);
        constructor.setAccessible(true);
        return constructor.newInstance(map);
    }

    @SuppressWarnings("unchecked")
    private static com.mojang.serialization.Codec<Object> dataCodec() throws Exception {
        Class<?> data = Class.forName("net.minecraft.server.PlayerAdvancements$Data");
        Field field = data.getDeclaredField("CODEC");
        field.setAccessible(true);
        return (com.mojang.serialization.Codec<Object>) field.get(null);
    }

    private static JsonObject criteria(Map<String, Criterion<?>> criteria) {
        JsonObject object = new JsonObject();
        for (Map.Entry<String, Criterion<?>> entry : criteria.entrySet()) {
            JsonObject criterion = new JsonObject();
            criterion.addProperty("trigger", TRIGGERS.getOrDefault(
                    entry.getValue().trigger(), entry.getValue().trigger().toString()));
            object.add(entry.getKey(), criterion);
        }
        return object;
    }

    private static JsonObject display(Optional<DisplayInfo> display) {
        JsonObject object = new JsonObject();
        if (display.isEmpty()) {
            return object;
        }
        DisplayInfo info = display.get();
        object.addProperty("frame", info.type().getSerializedName());
        object.addProperty("showToast", info.showToast());
        object.addProperty("announceToChat", info.announceToChat());
        object.addProperty("hidden", info.hidden());
        object.addProperty("background", info.background()
                .map(background -> background.id().toString()).orElse(""));
        return object;
    }

    private static JsonObject rewards(net.minecraft.advancements.AdvancementRewards rewards) {
        JsonObject object = new JsonObject();
        object.addProperty("experience", rewards.experience());
        JsonArray loot = new JsonArray();
        rewards.loot().stream().forEach(holder -> loot.add(holder.unwrapKey()
                .map(key -> key.identifier().toString()).orElse("")));
        object.add("loot", loot);
        JsonArray recipes = new JsonArray();
        rewards.recipes().forEach(name -> recipes.add(name.identifier().toString()));
        object.add("recipes", recipes);
        object.addProperty("function", rewards.function().map(function -> function.toString()).orElse(""));
        return object;
    }

    private static void write(Writer out, JsonObject object) throws Exception {
        out.write(GSON.toJson(object).replace("\n", " "));
        out.write("\n");
    }

    private static RegistryAccess registryAccess() {
        return (RegistryAccess) REGISTRIES;
    }

    private static HolderLookup.Provider REGISTRIES;
    private static final Map<Object, String> TRIGGERS = new LinkedHashMap<>();

    private static HolderLookup.Provider buildRegistries() throws Exception {
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags = TagLoader.loadTagsForExistingRegistries(
                resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups,
                RegistryDataLoader.WORLD_REGISTRIES,
                java.util.concurrent.ForkJoinPool.commonPool()).join();
        LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
        ReloadableServerResources managers = ReloadableServerResources.loadResources(
                resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                java.util.concurrent.ForkJoinPool.commonPool(), Runnable::run).get();
        managers.updateComponentsAndStaticRegistryTags();
        return (RegistryAccess) managers.fullRegistries().lookup();
    }

    static {
        try {
            SharedConstants.tryDetectVersion();
            Bootstrap.bootStrap();
            REGISTRIES = buildRegistries();
            RegistryAccess access = registryAccess();
            access.lookupOrThrow(net.minecraft.core.registries.Registries.TRIGGER_TYPE)
                    .listElements()
                    .forEach(holder -> TRIGGERS.put(holder.value(), holder.key().identifier().toString()));
        } catch (Exception error) {
            throw new ExceptionInInitializerError(error);
        }
    }

}
