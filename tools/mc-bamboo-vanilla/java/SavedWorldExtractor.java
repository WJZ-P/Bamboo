// 导出官方 `WorldGenSettings.CODEC` 编出的超平坦 `world_gen_settings.dat` 内容，
// 给 mc-bamboo-data 的生成流程用。
//
// 不启动服务端、不接受 EULA。stdout 输出一行 JSON：{"flat": "<SNBT>"}。
// Bamboo 只建模主世界，所以 `dimensions` 里只有 minecraft:overworld，生成器是
// minecraft:flat，地层与 Bamboo 的超平坦模板一致（基岩 1、泥土 2、草方块 1）。
// 层来自命令行参数，由 Rust 侧从 mc-bamboo-world 的模板读取。

import com.google.gson.JsonObject;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
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
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.levelgen.WorldGenSettings;
import net.minecraft.world.level.levelgen.WorldOptions;

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
                out.addProperty("flat", compactSnbt(flatTag(registries, args)));
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
    private static CompoundTag flatTag(RegistryAccess registries, String[] args) {
        if (args.length < 2 || args.length % 2 != 0) {
            throw new IllegalArgumentException("usage: SavedWorldExtractor <block> <height> ...");
        }
        RegistryOps<Tag> ops = RegistryOps.create(NbtOps.INSTANCE, registries);
        // 先让官方 codec 编一次默认的 `WorldGenSettings`，取主世界那一维的
        // `type`（维度类型），再把生成器换成 minecraft:flat、层照 Bamboo 的模板。
        // 只保留主世界：Bamboo 没建模下界和末地，存档里也不该声明它们的生成器。
        CompoundTag defaults = (CompoundTag) WorldGenSettings.CODEC
                .encodeStart(ops, new WorldGenSettings(new WorldOptions(0L, true, false),
                        WorldPresets.createNormalWorldDimensions(registries)))
                .getOrThrow();
        CompoundTag overworld = defaults.getCompoundOrEmpty("dimensions")
                .getCompoundOrEmpty("minecraft:overworld");
        CompoundTag generator = new CompoundTag();
        generator.putString("type", "minecraft:flat");
        CompoundTag settings = new CompoundTag();
        settings.putString("biome", "minecraft:plains");
        settings.putByte("features", (byte) 0);
        settings.putByte("lakes", (byte) 0);
        ListTag layers = new ListTag();
        for (int i = 0; i < args.length; i += 2) {
            CompoundTag layer = new CompoundTag();
            layer.putString("block", args[i]);
            layer.putInt("height", Integer.parseInt(args[i + 1]));
            layers.add(layer);
        }
        settings.put("layers", layers);
        generator.put("settings", settings);
        overworld.put("generator", generator);
        overworld.putString("type", "minecraft:overworld");

        CompoundTag dimensions = new CompoundTag();
        dimensions.put("minecraft:overworld", overworld);
        CompoundTag data = new CompoundTag();
        data.putByte("bonus_chest", (byte) 0);
        data.putLong("seed", 0L);
        data.putByte("generate_structures", (byte) 1);
        data.put("dimensions", dimensions);
        // 用官方 codec 解回来再编出去：字段不合法会在这里报错，编出的标签
        // 才是官方运行时真正会读到的形态（比如缺省字段的默认值）。
        WorldGenSettings parsed = WorldGenSettings.CODEC.parse(ops, data).getOrThrow();
        return (CompoundTag) WorldGenSettings.CODEC.encodeStart(ops, parsed).getOrThrow();
    }

    /**
     * 官方 `Tag.toString()` 的紧凑形式，但键序固定。
     *
     * 官方 `NbtUtils.structureToSnbt` 会先塞进结构模板的 data/palette，再按四空格
     * 缩进打印；`CompoundTag.toString()` 又按键名排序。存档常量是一行、按下表顺序，
     * 重跑导出才能得到同一份文本。每个复合标签都核对键集合，官方 codec 改了字段
     * 就显式报错，而不是悄悄换掉存档内容。
     */
    private static String compactSnbt(CompoundTag root) {
        StringBuilder out = new StringBuilder();
        writeCompound(root, List.of("bonus_chest", "seed", "generate_structures", "dimensions"), out);
        return out.toString();
    }

    private static final List<String> DIMENSION_KEYS = List.of("minecraft:overworld");
    private static final List<String> STEM_KEYS = List.of("generator", "type");
    private static final List<String> GENERATOR_KEYS = List.of("settings", "type");
    private static final List<String> SETTINGS_KEYS = List.of("features", "biome", "layers", "lakes");
    private static final List<String> LAYER_KEYS = List.of("block", "height");

    private static void writeCompound(CompoundTag tag, List<String> order, StringBuilder out) {
        Set<String> actual = new LinkedHashSet<>(tag.keySet());
        if (actual.size() != order.size() || !actual.containsAll(order)) {
            throw new IllegalStateException("world_gen_settings 的键变了：" + actual + "，预期 " + order);
        }
        out.append('{');
        for (int i = 0; i < order.size(); i++) {
            if (i != 0) {
                out.append(", ");
            }
            String key = order.get(i);
            out.append(simpleKey(key) ? key : StringTag.quoteAndEscape(key)).append(": ");
            writeValue(tag.get(key), key, out);
        }
        out.append('}');
    }

    private static void writeValue(Tag value, String key, StringBuilder out) {
        if (value instanceof CompoundTag compound) {
            List<String> order = switch (key) {
                case "dimensions" -> DIMENSION_KEYS;
                case "minecraft:overworld" -> STEM_KEYS;
                case "generator" -> GENERATOR_KEYS;
                case "settings" -> SETTINGS_KEYS;
                default -> LAYER_KEYS;
            };
            writeCompound(compound, order, out);
        } else if (value instanceof ListTag list) {
            out.append('[');
            for (int i = 0; i < list.size(); i++) {
                if (i != 0) {
                    out.append(", ");
                }
                Tag layer = list.get(i);
                if (!(layer instanceof CompoundTag compound)) {
                    throw new IllegalStateException("layers 里有非复合标签：" + layer);
                }
                writeCompound(compound, LAYER_KEYS, out);
            }
            out.append(']');
        } else {
            // 官方 `StringTagVisitor` 的叶子写法：0b / 0L / 1 / "minecraft:plains"。
            out.append(value);
        }
    }

    /** 官方 `SnbtPrinterTagVisitor.SIMPLE_VALUE`：`[A-Za-z0-9._+-]+`。 */
    private static boolean simpleKey(String key) {
        return key.matches("[A-Za-z0-9._+-]+");
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
