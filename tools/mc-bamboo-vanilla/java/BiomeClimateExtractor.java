import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.serialization.JsonOps;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.biome.Biome;

/**
 * 只导出天气要用到的群系气候设置，按 `minecraft:worldgen/biome` 的注册表 ID 排列。
 * 不启动服务端，不涉及 EULA。命令行参数是输出文件的路径。
 */
public class BiomeClimateExtractor {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));

        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            LayeredRegistryAccess<RegistryLayer> layers = loadRegistries(resources);
            RegistryAccess registries = layers.compositeAccess();
            Registry<Biome> biomes = registries.lookupOrThrow(Registries.BIOME);
            RegistryOps<com.google.gson.JsonElement> ops = registries.createSerializationContext(JsonOps.INSTANCE);

            JsonArray rows = new JsonArray();
            for (Biome biome : biomes) {
                JsonObject encoded = Biome.DIRECT_CODEC.encodeStart(ops, biome).getOrThrow().getAsJsonObject();
                JsonObject row = new JsonObject();
                row.addProperty("id", biomes.getId(biome));
                row.addProperty("name", biomes.getKey(biome).toString());
                row.addProperty("hasPrecipitation", encoded.get("has_precipitation").getAsBoolean());
                row.addProperty("temperature", encoded.get("temperature").getAsFloat());
                row.addProperty("frozen", encoded.has("temperature_modifier")
                        && encoded.get("temperature_modifier").getAsString().equals("frozen"));
                rows.add(row);
            }
            Files.writeString(Path.of(args[0]),
                    new GsonBuilder().setPrettyPrinting().create().toJson(rows));
        }
        System.exit(0);
    }

    /** 和 Extractor.loadRegistries 加载世界层注册表的那一段相同。 */
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
        lookups = new ArrayList<>(lookups);
        world.listRegistries().forEach(lookups::add);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }
}
