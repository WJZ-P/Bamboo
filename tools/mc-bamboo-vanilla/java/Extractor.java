import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.KnownPack;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.tags.TagNetworkSerialization;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.block.Block;

/**
 * 调用官方代码导出 Bamboo 需要的版本数据，写成一个 JSON 文件。
 *
 * <p>注册表和标签的数字 ID 取决于官方的加载顺序，所以这里直接走官方的加载流程，不自己解析数据文件。
 * 这里只读取数据，不启动服务端。
 */
public final class Extractor {
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
}
