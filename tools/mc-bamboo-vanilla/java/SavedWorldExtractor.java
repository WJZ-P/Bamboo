// 导出官方 `WorldGenSettings.CODEC` 编出的默认值和超平坦模板对应的
// `world_gen_settings.dat` 内容，给 mc-bamboo-data 的生成流程用。
//
// 不启动服务端、不接受 EULA。stdout 输出一行 JSON：
//   {"flat": "<SNBT>", "default": "<SNBT>"}
// `flat` 的主世界生成器是 minecraft:flat，地层与 Bamboo 的超平坦模板一致
// （基岩 1、泥土 2、草方块 1），官方服务端打开这个世界时新生成的区块和
// Bamboo 一致。层来自命令行参数，由 Rust 侧从 mc-bamboo-world 的模板读取。

import com.google.gson.JsonObject;
import com.mojang.serialization.Codec;
import com.mojang.serialization.JsonOps;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.NbtUtils;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.commands.Commands;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.WorldDimensions;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.levelgen.WorldGenSettings;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.flat.FlatLayerInfo;
import net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorSettings;
import net.minecraft.world.level.levelgen.FlatLevelSource;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;
import net.minecraft.world.level.levelgen.structure.StructureSet;

public final class SavedWorldExtractor {
    private SavedWorldExtractor() {
    }

    public static void main(String[] args) throws Exception {
        PrintStream log = System.err;
        try {
            System.setOut(log);
            FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
            SharedConstants.tryDetectVersion();
            Bootstrap.bootStrap();
            PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
            repository.reload();
            repository.setSelected(List.of("vanilla"));
            try (CloseableResourceManager resources =
                            new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
                RegistryAccess registries = loadRegistries(resources);
                JsonObject out = new JsonObject();
                out.addProperty("flat", NbtUtils.structureToSnbt(flatTag(registries, args)));
                try (Writer writer = new OutputStreamWriter(stdout, StandardCharsets.UTF_8)) {
                    writer.write(out.toString());
                    writer.write('\n');
                    writer.flush();
                }
            }
        } catch (Throwable error) {
            error.printStackTrace(log);
            System.exit(1);
        }
    }

    /** 命令行给的是 Rust 侧模板的层：blockName height blockName height ...。 */
    private static CompoundTag flatTag(RegistryAccess registries, String[] args) throws Exception {
        if (args.length < 2 || args.length % 2 != 0) {
            throw new IllegalArgumentException("usage: SavedWorldExtractor <block> <height> ...");
        }
        // 官方默认 `WorldGenSettings`（含下界、末地）用 codec 编成 NBT，
        // 再把主世界那一维的生成器换成 minecraft:flat，层照 Bamboo 的模板。
        CompoundTag defaults = (CompoundTag) WorldGenSettings.CODEC
                .encodeStart(RegistryOps.create(NbtOps.INSTANCE, registries),
                        new WorldGenSettings(new WorldOptions(0L, true, false),
                                defaultDimensions(registries)))
                .getOrThrow();
        CompoundTag dimensions = defaults.getCompoundOrEmpty("dimensions");
        CompoundTag overworld = dimensions.getCompoundOrEmpty("minecraft:overworld");
        CompoundTag generator = new CompoundTag();
        generator.putString("type", "minecraft:flat");
        CompoundTag settings = new CompoundTag();
        settings.putString("biome", "minecraft:plains");
        settings.putByte("features", (byte) 0);
        settings.putByte("lakes", (byte) 0);
        var layers = new net.minecraft.nbt.ListTag();
        for (int i = 0; i < args.length; i += 2) {
            String name = args[i];
            int height = Integer.parseInt(args[i + 1]);
            CompoundTag layer = new CompoundTag();
            layer.putString("block", name);
            layer.putInt("height", height);
            layers.add(layer);
        }
        settings.put("layers", layers);
        generator.put("settings", settings);
        overworld.put("generator", generator);
        overworld.putString("type", "minecraft:overworld");
        dimensions.put("minecraft:overworld", overworld);
        defaults.put("dimensions", dimensions);
        defaults.putLong("seed", 0L);
        defaults.putByte("generate_structures", (byte) 1);
        defaults.putByte("bonus_chest", (byte) 0);
        // 用官方 codec 解回来再编出去，确认它能解析并且字段都合法。
        WorldGenSettings parsed = WorldGenSettings.CODEC
                .parse(RegistryOps.create(NbtOps.INSTANCE, registries), defaults)
                .getOrThrow();
        return (CompoundTag) WorldGenSettings.CODEC
                .encodeStart(RegistryOps.create(NbtOps.INSTANCE, registries), parsed)
                .getOrThrow();
    }

    private static WorldDimensions defaultDimensions(RegistryAccess registries) {
        return WorldPresets.createNormalWorldDimensions(registries);
    }

    /** 和 LightOracle 一样，只加载世界层注册表，不构造 ServerLevel。 */
    private static RegistryAccess loadRegistries(CloseableResourceManager resources) throws Exception {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
        ReloadableServerResources managers = ReloadableServerResources
                .loadResources(resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                        Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                        java.util.concurrent.ForkJoinPool.commonPool(), Runnable::run)
                .get();
        managers.updateComponentsAndStaticRegistryTags();
        return (RegistryAccess) managers.fullRegistries().lookup();
    }
}
