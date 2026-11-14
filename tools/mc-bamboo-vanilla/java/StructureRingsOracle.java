import com.mojang.datafixers.util.Pair;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
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
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.structure.StructureSet;
import net.minecraft.world.level.levelgen.structure.placement.ConcentricRingsStructurePlacement;

/**
 * 用官方 26.3 的 ChunkGeneratorStructureState.getRingPositionsFor 和
 * BiomeSource.findBiomeHorizontal 对拍要塞环位置与群系吸附。
 *
 * <p>按官方 WorldLoader 的前半段读数据包注册表（和 ChunkgenOracle 一样），不启动服务端、
 * 不读存档、不接受 EULA。输出纯文本：每个种子一段 "RINGS seed n" + n 行 "x z"，
 * 再加若干 "B seed ox oz rx rz biome"（findBiomeHorizontal 的结果，或 NONE）。
 */
public final class StructureRingsOracle {
    private static final long[] SEEDS = {0L, 1L, -1L, 42L, 12345678901L, -98765432123L};
    private static final int[][] ORIGINS = {
        {0, 0}, {1000, 1000}, {-1000, 1000}, {1000, -1000}, {-1000, -1000},
        {4096, 0}, {0, 4096}, {-4096, 0}, {0, -4096}, {500, -300},
        {-500, 300}, {12345, -6789}, {-12345, 6789}, {100, 100}, {-100, -100}, {255, 127},
    };

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            RegistryAccess.Frozen registries = loadRegistries(resources).compositeAccess();
            run(registries);
        }
    }

    private static LayeredRegistryAccess<RegistryLayer> loadRegistries(CloseableResourceManager resources) {
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

    private static void run(RegistryAccess registries) {
        HolderLookup.RegistryLookup<net.minecraft.world.level.levelgen.synth.NormalNoise> noiseLookup =
                registries.lookupOrThrow(net.minecraft.core.registries.Registries.NOISE);
        Registry<NoiseGeneratorSettings> settingsReg =
                registries.lookupOrThrow(net.minecraft.core.registries.Registries.NOISE_SETTINGS);
        NoiseGeneratorSettings settings = settingsReg.getValue(Identifier.withDefaultNamespace("overworld"));
        if (settings == null) {
            throw new IllegalStateException("missing minecraft:overworld noise settings");
        }
        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(
                MultiNoiseBiomeSource.createFromPreset(
                        registries.lookupOrThrow(
                                        net.minecraft.core.registries.Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                                .getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD)),
                settingsReg.wrapAsHolder(settings));
        BiomeSource biomeSource = generator.getBiomeSource();
        Registry<StructureSet> setsReg =
                registries.lookupOrThrow(net.minecraft.core.registries.Registries.STRUCTURE_SET);
        StructureSet strongholds = setsReg.getValue(Identifier.withDefaultNamespace("strongholds"));
        ConcentricRingsStructurePlacement placement =
                (ConcentricRingsStructurePlacement) strongholds.placement();

        StringBuilder out = new StringBuilder();
        for (long seed : SEEDS) {
            RandomState randomState = RandomState.create(noiseLookup, seed, settings);
            ChunkGeneratorStructureState state = ChunkGeneratorStructureState.createForNormal(
                    randomState, seed, new ChunkPos(0, 0), biomeSource, setsReg);
            state.ensureStructuresGenerated();
            List<ChunkPos> positions = state.getRingPositionsFor(placement);
            if (positions == null) {
                throw new IllegalStateException("no ring positions for seed " + seed);
            }
            out.append("RINGS ").append(seed).append(' ').append(positions.size()).append('\n');
            for (ChunkPos pos : positions) {
                out.append(pos.x()).append(' ').append(pos.z()).append('\n');
            }

            for (int[] origin : ORIGINS) {
                int ox = origin[0];
                int oz = origin[1];
                // 和环位置里的 fork 一样，搜索随机源由种子和坐标确定。
                // Like the ring fork, the search source is derived from seed and coords.
                long searchSeed = seed ^ ((long) ox * 341873128712L + (long) oz * 132897987541L);
                RandomSource search = RandomSource.create(searchSeed);
                Pair<BlockPos, Holder<Biome>> result = biomeSource.findBiomeHorizontal(
                        ox, 0, oz, 112, placement.preferredBiomes()::contains, search, randomState);
                if (result == null) {
                    out.append("B ").append(seed).append(' ').append(ox).append(' ').append(oz)
                            .append(" NONE\n");
                } else {
                    BlockPos pos = result.getFirst();
                    String biome = result.getSecond().unwrapKey().orElseThrow().identifier().toString();
                    out.append("B ").append(seed).append(' ').append(ox).append(' ').append(oz)
                            .append(' ').append(pos.getX()).append(' ').append(pos.getZ())
                            .append(' ').append(biome).append('\n');
                }
            }
        }
        System.out.print(out);
    }
}
