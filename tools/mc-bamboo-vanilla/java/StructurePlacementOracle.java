import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.Vec3i;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.structure.placement.AbstractSpreadingStructurePlacement;
import net.minecraft.world.level.levelgen.structure.placement.RandomSpreadStructurePlacement;
import net.minecraft.world.level.levelgen.structure.placement.RandomSpreadType;

/**
 * 用官方 26.3 的 RandomSpreadStructurePlacement 对拍结构放置。
 *
 * <p>不加载注册表、不启动服务端：直接从导出的 worldgen.json 读 structure_set 的参数，
 * 用公开构造器造官方 placement，对每条 (seed, x, z) 查询算 getPotentialStructureChunk
 * 和 applyAdditionalChunkRestrictions。输出纯文本，Rust 侧逐行比。
 *
 * <p>Usage: StructurePlacementOracle &lt;worldgen.json&gt; &lt;queries.txt&gt;
 * queries.txt 每行是 "seed x z"。输出先是 SETS 数量 + 这些名字，接着每行的
 * "R name seed x z px pz restrict"（放置）；然后一行 "SELECT n"，再每行
 * "S name seed x z a,b,c"（createStructures 加权选择的官方尝试序列）。
 */
public final class StructurePlacementOracle {
    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            throw new IllegalArgumentException("usage: StructurePlacementOracle <worldgen.json> <queries.txt>");
        }
        // StructurePlacement 的静态 CODEC 引用 BuiltInRegistries，必须先 bootstrap。
        // StructurePlacement's static CODEC references BuiltInRegistries, so bootstrap first.
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        JsonObject root;
        try (Reader reader = Files.newBufferedReader(Path.of(args[0]), StandardCharsets.UTF_8)) {
            root = JsonParser.parseReader(reader).getAsJsonObject();
        }
        JsonObject sets = root.getAsJsonObject("structure_set");

        // 官方构造器参数全部来自 worldgen.json；缺省值照 codec 默认补。
        List<String> names = new ArrayList<>();
        List<RandomSpreadStructurePlacement> placements = new ArrayList<>();
        for (Map.Entry<String, JsonElement> entry : sets.entrySet()) {
            JsonObject placement = entry.getValue().getAsJsonObject().getAsJsonObject("placement");
            String type = placement.get("type").getAsString();
            if (!type.equals("minecraft:random_spread")) {
                continue; // concentric_rings 需要 ChunkGeneratorStructureState，本轮对拍跳过
            }
            names.add(entry.getKey());
            placements.add(build(placement));
        }

        List<String> queries = Files.readAllLines(Path.of(args[1]), StandardCharsets.UTF_8);

        StringBuilder out = new StringBuilder();
        out.append("SETS ").append(names.size()).append('\n');
        for (String name : names) {
            out.append(name).append('\n');
        }
        for (int i = 0; i < names.size(); i++) {
            String name = names.get(i);
            RandomSpreadStructurePlacement placement = placements.get(i);
            for (String query : queries) {
                if (query.isBlank()) {
                    continue;
                }
                String[] parts = query.trim().split("\\s+");
                long seed = Long.parseLong(parts[0]);
                int x = Integer.parseInt(parts[1]);
                int z = Integer.parseInt(parts[2]);
                ChunkPos pos = placement.getPotentialStructureChunk(seed, x, z);
                boolean restrict = placement.applyAdditionalChunkRestrictions(x, z, seed);
                out.append("R ").append(name).append(' ').append(seed).append(' ').append(x).append(' ')
                        .append(z).append(' ').append(pos.x()).append(' ').append(pos.z()).append(' ')
                        .append(restrict ? 1 : 0).append('\n');
            }
        }

        // createStructures 的加权选择：对全部结构集（含 concentric_rings）输出官方
        // setLargeFeatureSeed + nextInt(total) 的尝试序列。选择序列本身不依赖
        // isStructureChunk，所有查询点都算，覆盖更广。
        // Weighted selection of createStructures: emit the official
        // setLargeFeatureSeed + nextInt(total) attempt order for every structure set.
        // The sequence does not depend on isStructureChunk, so all queries are used.
        out.append("SELECT ").append(sets.size()).append('\n');
        for (Map.Entry<String, JsonElement> entry : sets.entrySet()) {
            JsonArray structures = entry.getValue().getAsJsonObject().getAsJsonArray("structures");
            for (String query : queries) {
                if (query.isBlank()) {
                    continue;
                }
                String[] parts = query.trim().split("\\s+");
                long seed = Long.parseLong(parts[0]);
                int x = Integer.parseInt(parts[1]);
                int z = Integer.parseInt(parts[2]);
                WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
                random.setLargeFeatureSeed(seed, x, z);
                List<JsonElement> options = new ArrayList<>();
                structures.forEach(options::add);
                int total = 0;
                for (JsonElement option : options) {
                    total += option.getAsJsonObject().get("weight").getAsInt();
                }
                List<String> sequence = new ArrayList<>();
                while (!options.isEmpty()) {
                    int choice = random.nextInt(total);
                    int index = 0;
                    for (int k = 0; k < options.size(); k++) {
                        choice -= options.get(k).getAsJsonObject().get("weight").getAsInt();
                        if (choice < 0) {
                            break;
                        }
                        index++;
                    }
                    JsonObject selected = options.get(index).getAsJsonObject();
                    sequence.add(selected.get("structure").getAsString());
                    total -= selected.get("weight").getAsInt();
                    options.remove(index);
                }
                out.append("S ").append(entry.getKey()).append(' ').append(seed).append(' ').append(x)
                        .append(' ').append(z).append(' ').append(String.join(",", sequence)).append('\n');
            }
        }
        System.out.print(out);
    }

    private static RandomSpreadStructurePlacement build(JsonObject placement) {
        int spacing = placement.get("spacing").getAsInt();
        int separation = placement.get("separation").getAsInt();
        int salt = placement.get("salt").getAsInt();
        float frequency = placement.has("frequency") ? placement.get("frequency").getAsFloat() : 1.0F;
        String methodName = placement.has("frequency_reduction_method")
                ? placement.get("frequency_reduction_method").getAsString() : "default";
        String spreadName = placement.has("spread_type")
                ? placement.get("spread_type").getAsString() : "linear";
        return new RandomSpreadStructurePlacement(
                Vec3i.ZERO,
                method(methodName),
                frequency,
                salt,
                Optional.empty(), // exclusion_zone 不影响这两个被对拍的方法
                spacing,
                separation,
                spread(spreadName));
    }

    private static AbstractSpreadingStructurePlacement.FrequencyReductionMethod method(String name) {
        for (AbstractSpreadingStructurePlacement.FrequencyReductionMethod value :
                AbstractSpreadingStructurePlacement.FrequencyReductionMethod.values()) {
            if (value.getSerializedName().equals(name)) {
                return value;
            }
        }
        throw new IllegalArgumentException("unknown frequency_reduction_method " + name);
    }

    private static RandomSpreadType spread(String name) {
        return RandomSpreadType.valueOf(name.toUpperCase(Locale.ROOT));
    }
}
