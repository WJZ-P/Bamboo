import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.Vec3i;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelResource;
import net.minecraft.world.level.storage.LevelStorageSource;

/**
 * 官方 jigsaw 对拍。构造真实的 StructureTemplateManager（从 vanilla 数据包加载
 * data/minecraft/structure/**.nbt），把 Structure.GenerationContext 的
 * structureTemplateManager 从 null 换成它。
 *
 * <p>templates 模式：逐个 get(id) 加载模板，输出 size / save() 的 blocks 数 /
 * getJigsaws(ZERO, NONE) 的拼图方块数，用来验证模板确实被加载。
 *
 * <p>Compare jigsaw against vanilla with a real StructureTemplateManager loaded
 * from the vanilla data pack. No server, no save, no EULA.
 *
 * <p>Usage: JigsawOracle templates &lt;queries.txt&gt;
 */
public final class JigsawOracle {
    private JigsawOracle() {
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 2 || !args[0].equals("templates")) {
            throw new IllegalArgumentException("usage: JigsawOracle templates <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            Path temp = Files.createTempDirectory("jigsaw-stm");
            LevelStorageSource storageSource = LevelStorageSource.createDefault(temp);
            try (LevelStorageSource.LevelStorageAccess storage = storageSource.createAccess("world")) {
                Files.createDirectories(storage.getLevelPath(LevelResource.GENERATED_DIR));
                StructureTemplateManager manager = new StructureTemplateManager(
                        resources, storage, DataFixers.getDataFixer(),
                        registries.lookupOrThrow(Registries.BLOCK));
                runTemplates(manager, Path.of(args[1]));
            }
        }
    }

    private static void runTemplates(StructureTemplateManager manager, Path queries) throws Exception {
        List<String> lines = Files.readAllLines(queries, StandardCharsets.UTF_8);
        StringBuilder out = new StringBuilder();
        for (String line : lines) {
            if (line.isBlank()) {
                continue;
            }
            Identifier id = Identifier.parse(line.trim());
            StructureTemplate template = manager.get(id).orElseThrow(
                    () -> new IllegalStateException("template not loaded: " + id));
            Vec3i size = template.getSize();
            CompoundTag saved = template.save(new CompoundTag());
            int blocks = saved.getListOrEmpty("blocks").size();
            int jigsaws = template.getJigsaws(BlockPos.ZERO, Rotation.NONE).size();
            out.append(id).append('|').append(size.getX()).append('|')
                    .append(size.getY()).append('|').append(size.getZ()).append('|')
                    .append(blocks).append('|').append(jigsaws).append('\n');
        }
        System.out.print(out);
    }

    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(
            CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(Registry.PendingTags::apply);
        return initial.replaceFrom(RegistryLayer.WORLD, world);
    }
}
