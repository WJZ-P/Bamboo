import com.google.gson.JsonObject;
import com.mojang.serialization.JsonOps;
import io.netty.buffer.Unpooled;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.List;
import java.util.Optional;
import java.util.Random;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.advancements.predicates.BlockPredicate;
import net.minecraft.advancements.predicates.DataComponentMatchers;
import net.minecraft.advancements.predicates.NbtPredicate;
import net.minecraft.advancements.predicates.StatePropertiesPredicate;
import net.minecraft.commands.Commands;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.IntegerProperty;
import net.minecraft.world.level.block.state.properties.Property;

/**
 * 用官方 {@code StatePropertiesPredicate} 和 {@code BlockPredicate} 给冒险模式的方块谓词出题。
 *
 * 每题抽一个真实的方块状态和一个随机谓词，按官方 {@code BlockPredicate.STREAM_CODEC}
 * 编码成十六进制写到 stdout，再附上官方的答案。谓词覆盖：
 *
 * - 方块列表：不写、空列表、包含/不包含这一格的方块、一个随机的方块标签；
 * - 属性：不写、精确值、区间（两端、只写一端、两端都不写）、错误的写法、不存在的属性名；
 * - NBT：偶尔带一个空的复合标签。
 *
 * 官方答案按 {@code BlockPredicate.matches(BlockInWorld)} 算。这个重载在
 * {@code getEntity()} 为空时只看 {@code matchesState} 和「有没有 NBT」，Bamboo 的
 * {@code predicate::matches} 复刻的就是它，所以这里不需要造一个真实的世界。
 *
 * 参数：{@code [--cases N] [--seed S]}。
 */
public final class PredicateOracle {
    private static final int OUTPUT_BUFFER = 1 << 16;

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            Options options = Options.parse(args);
            run(options, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(Options options, PrintStream log) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        // 官方日志默认写 stdout，要抢在日志框架初始化之前换掉。
        System.setOut(log);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            RegistryAccess registries = loadRegistries(resources);
            new Oracle(registries, out, options, log).run();
        }
    }

    /**
     * 和 `WorldLoader.load` 一样加载注册表和标签，再加载服务端资源。
     *
     * 方块标签是静态注册表的一部分，`PendingTags.apply` 之后 `registryAccess` 里的
     * 方块注册表才带标签，所以照 ComponentOracle 的做法走一遍完整加载。
     */
    private static RegistryAccess loadRegistries(CloseableResourceManager resources) throws Exception {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                .join();
        LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
        ReloadableServerResources managers = ReloadableServerResources
                .loadResources(
                        resources,
                        layers,
                        staticTags,
                        FeatureFlags.DEFAULT_FLAGS,
                        Commands.CommandSelection.DEDICATED,
                        LevelBasedPermissionSet.GAMEMASTER,
                        ForkJoinPool.commonPool(),
                        Runnable::run)
                .get();
        managers.updateComponentsAndStaticRegistryTags();
        return (RegistryAccess) managers.fullRegistries().lookup();
    }

    /** 命令行参数。 */
    private record Options(int cases, long seed) {
        static Options parse(String[] args) {
            int cases = 20000;
            long seed = 1;
            for (int index = 0; index < args.length; index++) {
                switch (args[index]) {
                    case "--cases" -> cases = Integer.parseInt(args[++index]);
                    case "--seed" -> seed = Long.parseLong(args[++index]);
                    default -> throw new IllegalArgumentException("unknown argument: " + args[index]);
                }
            }
            if (cases < 0) {
                throw new IllegalArgumentException("--cases must not be negative");
            }
            return new Options(cases, seed);
        }
    }

    private static final class Oracle {
        private final RegistryAccess registries;
        private final Registry<Block> blockRegistry;
        private final List<Block> blocks;
        private final List<BlockState> states;
        private final List<HolderSet.Named<Block>> tags;
        private final Writer out;
        private final Options options;
        private final PrintStream log;
        private final Random random;
        /** 整数改写成哪一套十进制数字。按顺序轮换，每种文字都会出到。 */
        private int unicodeScripts;

        Oracle(RegistryAccess registries, Writer out, Options options, PrintStream log) {
            this.registries = registries;
            this.blockRegistry = registries.lookupOrThrow(Registries.BLOCK);
            this.blocks = blockRegistry.stream().toList();
            List<BlockState> allStates = new ArrayList<>();
            for (Block block : blocks) {
                allStates.addAll(block.getStateDefinition().getPossibleStates());
            }
            this.states = List.copyOf(allStates);
            this.tags = blockRegistry.getTags()
                    .sorted(Comparator.comparing(tag -> tag.key().location().toString()))
                    .toList();
            this.out = out;
            this.options = options;
            this.log = log;
            this.random = new Random(options.seed());
        }

        void run() throws Exception {
            line("{\"type\":\"header\",\"version\":"
                    + quote(SharedConstants.getCurrentVersion().id())
                    + ",\"protocol\":"
                    + SharedConstants.getCurrentVersion().protocolVersion()
                    + ",\"states\":"
                    + states.size()
                    + "}");
            long matches = 0;
            for (int index = 0; index < options.cases(); index++) {
                boolean answer = writeCase(index);
                if (answer) {
                    matches++;
                }
            }
            line("{\"type\":\"end\",\"cases\":" + options.cases() + ",\"matches\":" + matches + "}");
            out.flush();
            log.println("wrote " + options.cases() + " cases (" + states.size() + " states, "
                    + tags.size() + " tags), " + matches + " matches");
        }

        /** 写一道题，返回官方的答案。 */
        private boolean writeCase(int index) throws Exception {
            BlockState state = states.get(random.nextInt(states.size()));
            BlockPredicate predicate = new BlockPredicate(
                    randomBlocks(state), randomProperties(state), randomNbt(), DataComponentMatchers.ANY);

            RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
            BlockPredicate.STREAM_CODEC.encode(buffer, predicate);
            byte[] bytes = new byte[buffer.readableBytes()];
            buffer.readBytes(bytes);

            // `matches(BlockInWorld)` 在没有方块实体时就是这个式子。
            boolean answer = predicate.matchesState(state) && predicate.nbt().isEmpty();
            line("{\"type\":\"case\",\"index\":"
                    + index
                    + ",\"state\":"
                    + Block.getId(state)
                    + ",\"predicate\":\""
                    + hex(bytes)
                    + "\",\"matches\":"
                    + answer
                    + "}");
            return answer;
        }

        /** 方块列表：一半的题不写，其余的用空列表、直接列表或者标签。 */
        private Optional<HolderSet<Block>> randomBlocks(BlockState state) {
            return switch (random.nextInt(6)) {
                case 0, 1, 2 -> Optional.empty();
                case 3 -> Optional.of(HolderSet.direct(Block::builtInRegistryHolder, List.<Block>of()));
                case 4 -> Optional.of(HolderSet.direct(Block::builtInRegistryHolder, randomBlockList(state)));
                default -> Optional.of(tags.get(random.nextInt(tags.size())));
            };
        }

        /** 1 到 3 个方块，有一半机会把这一格的方块放进去。 */
        private List<Block> randomBlockList(BlockState state) {
            List<Block> chosen = new ArrayList<>();
            int count = 1 + random.nextInt(3);
            for (int index = 0; index < count; index++) {
                if (index == 0 && random.nextBoolean()) {
                    chosen.add(state.getBlock());
                } else {
                    chosen.add(blocks.get(random.nextInt(blocks.size())));
                }
            }
            return chosen;
        }

        /** 属性：六分之一的题不写，其余写 1 到 3 条。 */
        private Optional<StatePropertiesPredicate> randomProperties(BlockState state) {
            if (random.nextInt(6) == 0) {
                return Optional.empty();
            }
            Collection<Property<?>> ownProperties = state.getBlock().getStateDefinition().getProperties();
            List<Property<?>> own = List.copyOf(ownProperties);
            JsonObject json = new JsonObject();
            int count = 1 + random.nextInt(3);
            for (int index = 0; index < count; index++) {
                Property<?> property = pickProperty(own);
                String name = property != null ? property.getName() : otherPropertyName();
                if (random.nextInt(5) < 3) {
                    json.addProperty(name, exactValue(state, property));
                } else {
                    json.add(name, rangedValue(state, property));
                }
            }
            StatePropertiesPredicate predicate =
                    StatePropertiesPredicate.CODEC.parse(JsonOps.INSTANCE, json).getOrThrow();
            return Optional.of(predicate);
        }

        /** 八成从这一格的方块上拿属性，其余从别的方块上拿，偶尔拿一个不存在的名字。 */
        private Property<?> pickProperty(List<Property<?>> own) {
            if (!own.isEmpty() && random.nextInt(5) != 0) {
                return own.get(random.nextInt(own.size()));
            }
            if (random.nextBoolean()) {
                Block other = blocks.get(random.nextInt(blocks.size()));
                List<Property<?>> properties = List.copyOf(other.getStateDefinition().getProperties());
                if (!properties.isEmpty()) {
                    return properties.get(random.nextInt(properties.size()));
                }
            }
            return null;
        }

        private String otherPropertyName() {
            if (random.nextBoolean()) {
                Block other = blocks.get(random.nextInt(blocks.size()));
                List<Property<?>> properties = List.copyOf(other.getStateDefinition().getProperties());
                if (!properties.isEmpty()) {
                    return properties.get(random.nextInt(properties.size())).getName();
                }
            }
            return "no_such_property";
        }

        /** 精确值：四成是这一格现在的值，三成换一个合法值，其余写垃圾。 */
        private String exactValue(BlockState state, Property<?> property) {
            Property<?> own = ownProperty(state, property);
            if (own instanceof IntegerProperty && random.nextInt(5) == 0) {
                return unicodeDigits(serialized(state, own));
            }
            if (own != null) {
                int mode = random.nextInt(10);
                if (mode < 4) {
                    return serialized(state, own);
                }
                if (mode < 7) {
                    List<String> values = names(own);
                    if (!values.isEmpty()) {
                        return values.get(random.nextInt(values.size()));
                    }
                }
            }
            return garbageValue();
        }

        /** 区间：两端都写、只写一端、两端都不写。端点也混进合法值和垃圾。 */
        private JsonObject rangedValue(BlockState state, Property<?> property) {
            JsonObject range = new JsonObject();
            int shape = random.nextInt(4);
            // 0 只写 min，1 只写 max，2 两端都写，3 两端都不写。
            if (shape == 0 || shape == 2) {
                range.addProperty("min", boundValue(state, property));
            }
            if (shape == 1 || shape == 2) {
                range.addProperty("max", boundValue(state, property));
            }
            return range;
        }

        private String boundValue(BlockState state, Property<?> property) {
            Property<?> own = ownProperty(state, property);
            if (own instanceof IntegerProperty && random.nextInt(5) == 0) {
                return unicodeDigits(serialized(state, own));
            }
            if (own != null) {
                int mode = random.nextInt(10);
                if (mode < 5) {
                    return serialized(state, own);
                }
                if (mode < 8) {
                    List<String> values = names(own);
                    if (!values.isEmpty()) {
                        return values.get(random.nextInt(values.size()));
                    }
                }
            }
            return garbageValue();
        }

        private Optional<NbtPredicate> randomNbt() {
            return random.nextInt(25) == 0 ? Optional.of(new NbtPredicate(new CompoundTag())) : Optional.empty();
        }

        /** 这个方块自己的同名属性。属性是从别的方块上挑来的，不一定属于这一格。 */
        private static Property<?> ownProperty(BlockState state, Property<?> property) {
            return property == null ? null : state.getBlock().getStateDefinition().getProperty(property.getName());
        }

        /** 这一格现在这个属性的序列化名。 */
        @SuppressWarnings({"rawtypes", "unchecked"})
        private static String serialized(BlockState state, Property property) {
            return property.getName(state.getValue(property));
        }

        /** 属性所有合法值的序列化名。 */
        @SuppressWarnings({"rawtypes", "unchecked"})
        private static List<String> names(Property property) {
            List<String> names = new ArrayList<>();
            for (Object value : property.getPossibleValues()) {
                names.add(property.getName((Comparable) value));
            }
            return names;
        }

        /** 属性解析不出来的写法，整数、布尔、枚举都可能碰到。 */
        private String garbageValue() {
            return GARBAGE[random.nextInt(GARBAGE.length)];
        }

        /** 把 ASCII 数字换成一套 {@code Character.digit} 认的十进制数字，数值不变。 */
        private String unicodeDigits(String ascii) {
            int zero = UNICODE_ZERO[this.unicodeScripts % UNICODE_ZERO.length];
            this.unicodeScripts++;
            StringBuilder out = new StringBuilder(ascii.length());
            for (int index = 0; index < ascii.length(); index++) {
                char c = ascii.charAt(index);
                if (c >= '0' && c <= '9') {
                    out.append((char) (zero + (c - '0')));
                } else {
                    out.append(c);
                }
            }
            return out.toString();
        }

        private void line(String text) throws Exception {
            out.write(text);
            out.write('\n');
        }
    }

    /**
     * {@code Integer.parseInt} 会认的基本多文种平面十进制数字起点，每个区间连续 10 个码元。
     * 和 {@code crates/mc-bamboo-blocks/src/predicate.rs} 的 {@code DECIMAL_DIGIT_ZERO} 是同一张表。
     */
    private static final int[] UNICODE_ZERO = {
        0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66,
        0x0CE6, 0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946,
        0x19D0, 0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0,
        0xA9F0, 0xAA50, 0xABF0, 0xFF10,
    };

    private static final String[] GARBAGE = {
        "", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "16", "64",
        "-1", "+3", "03", "999", "2147483648", "-2147483649",
        // 全角、阿拉伯-印度数字能解析；全角加号、上标、下标和补充平面的数字不能。
        "\uFF13", "\u0663", "\uFF11\uFF12", "\uFF10\uFF13", "-\uFF13", "+\uFF13",
        "\uFF0B\uFF13", "\u00B3", "\u2083", "\uD801\uDCA0",
        "\uFF19\uFF19\uFF19\uFF19\uFF19\uFF19\uFF19\uFF19\uFF19\uFF19",
        "true", "false", "TRUE", "False",
        "x", "y", "z", "up", "down", "north", "south", "east", "west",
        "diagonal", "not a value", "minecraft:stone", "",
    };

    private static String hex(byte[] bytes) {
        StringBuilder text = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            text.append(Character.forDigit((value >> 4) & 0xf, 16));
            text.append(Character.forDigit(value & 0xf, 16));
        }
        return text.toString();
    }

    private static String quote(String text) {
        StringBuilder result = new StringBuilder(text.length() + 2);
        result.append('"');
        for (int index = 0; index < text.length(); index++) {
            char c = text.charAt(index);
            switch (c) {
                case '"' -> result.append("\\\"");
                case '\\' -> result.append("\\\\");
                case '\n' -> result.append("\\n");
                case '\r' -> result.append("\\r");
                case '\t' -> result.append("\\t");
                default -> {
                    if (c < 0x20 || c > 0x7e) {
                        result.append(String.format("\\u%04x", (int) c));
                    } else {
                        result.append(c);
                    }
                }
            }
        }
        result.append('"');
        return result.toString();
    }

    private PredicateOracle() {}
}
