import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.block.Mirror;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.block.state.BlockState;

/**
 * 对拍 `BlockState.mirror(mirror)` 再 `rotate(rotation)`：结构件 `placeBlock` 在写方块前
 * 就做这两步。四种 `(mirror, rotation)` 组合来自 `StructurePiece.setOrientation`。
 *
 * <p>Rust 写 queries.txt，每条一行一个方块状态串（`BlockStateParser` 的语法）。本类对每种
 * 组合输出一行 `m=<mirror> r=<rotation> <状态串>`，Rust 侧用 `state_transform::transform`
 * 复算并逐行比。只 Bootstrap + 读 vanilla 数据包，不启服务端、不读存档、不接受 EULA。
 *
 * <p>Oracle for `BlockState.mirror(mirror)` then `rotate(rotation)`, the two steps
 * `StructurePiece.placeBlock` runs before writing. The four `(mirror, rotation)` pairs come
 * from `StructurePiece.setOrientation`. Bootstrap only.
 *
 * <p>Usage: TransformOracle &lt;queries.txt&gt;
 */
public final class TransformOracle {
    /** `setOrientation` 产生的四组 `(mirror, rotation)`，顺序固定。 */
    // The four `(mirror, rotation)` pairs `setOrientation` can produce, in fixed order.
    private static final Mirror[] MIRRORS = {
        Mirror.NONE, Mirror.LEFT_RIGHT, Mirror.LEFT_RIGHT, Mirror.NONE
    };
    private static final Rotation[] ROTATIONS = {
        Rotation.NONE, Rotation.NONE, Rotation.CLOCKWISE_90, Rotation.CLOCKWISE_90
    };

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: TransformOracle <queries.txt>");
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));

        StringBuilder out = new StringBuilder();
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            for (String line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
                if (line.isBlank()) {
                    continue;
                }
                String text = line.trim();
                BlockState state = BlockStateParser
                        .parseForBlock(registries.lookupOrThrow(Registries.BLOCK), text, true)
                        .blockState();
                for (int i = 0; i < MIRRORS.length; i++) {
                    BlockState result = state.mirror(MIRRORS[i]).rotate(ROTATIONS[i]);
                    out.append("T ").append(i).append(' ').append(text).append(' ')
                            .append(result).append('\n');
                }
            }
        }
        System.out.print(out);
    }

    private static Layered loadRegistries(CloseableResourceManager resources) {
        var initial = RegistryLayer.createRegistryAccess();
        List<net.minecraft.core.Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<net.minecraft.core.HolderLookup.RegistryLookup<?>> lookups =
                TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        staticTags.forEach(net.minecraft.core.Registry.PendingTags::apply);
        return new Layered(initial.replaceFrom(RegistryLayer.WORLD, world));
    }

    /** `LayeredRegistryAccess` 的薄包装，只为避开长泛型。 */
    // Thin wrapper over `LayeredRegistryAccess` to avoid the long generic.
    private record Layered(net.minecraft.core.LayeredRegistryAccess<RegistryLayer> access) {
        RegistryAccess.Frozen compositeAccess() {
            return access.compositeAccess();
        }
    }
}
