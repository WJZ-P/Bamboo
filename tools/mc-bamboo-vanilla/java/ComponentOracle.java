import com.google.common.hash.HashCode;
import io.netty.buffer.Unpooled;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.ForkJoinPool;
import java.util.function.ObjIntConsumer;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponentPatch;
import net.minecraft.core.component.DataComponentType;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.component.TypedDataComponent;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.ByteArrayTag;
import net.minecraft.nbt.ByteTag;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.DoubleTag;
import net.minecraft.nbt.FloatTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.IntTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.LongArrayTag;
import net.minecraft.nbt.LongTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.ShortTag;
import net.minecraft.nbt.SnbtPrinterTagVisitor;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.nbt.TagParser;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.HashedPatchMap;
import net.minecraft.network.HashedStack;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.HashOps;
import net.minecraft.util.Unit;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.EquipmentSlotGroup;
import net.minecraft.world.entity.ai.attributes.AttributeModifier;
import net.minecraft.world.entity.animal.axolotl.Axolotl;
import net.minecraft.world.entity.animal.cow.MushroomCow;
import net.minecraft.world.entity.animal.equine.Llama;
import net.minecraft.world.entity.animal.fish.Salmon;
import net.minecraft.world.entity.animal.fish.TropicalFish;
import net.minecraft.world.entity.animal.fox.Fox;
import net.minecraft.world.entity.animal.parrot.Parrot;
import net.minecraft.world.entity.animal.rabbit.Rabbit;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.CreativeModeTab;
import net.minecraft.world.item.CreativeModeTabs;
import net.minecraft.world.item.DyeColor;
import net.minecraft.world.item.Instrument;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.ItemUseAnimation;
import net.minecraft.world.item.JukeboxSong;
import net.minecraft.world.item.Rarity;
import net.minecraft.world.item.SwingAnimationType;
import net.minecraft.world.item.component.FireworkExplosion;
import net.minecraft.world.item.component.ItemAttributeModifiers;
import net.minecraft.world.item.component.MapPostProcessing;
import net.minecraft.world.entity.decoration.painting.PaintingVariant;
import net.minecraft.world.item.equipment.trim.ArmorTrim;
import net.minecraft.world.item.equipment.trim.TrimMaterial;
import net.minecraft.world.item.equipment.trim.TrimPattern;

/**
 * 用官方代码给物品组件的编解码出题。
 *
 * 第一步把一大批真实的组件补丁按官方 {@code DataComponentPatch.STREAM_CODEC} 编码成十六进制写到
 * stdout；Bamboo 解码再编码，把自己写出来的字节从 stdin 回传；第二步用官方代码解回来，
 * 和原来的补丁比 {@code equals}。两步都对才算一致。
 *
 * 题目来自三处：每个物品的默认组件、创造模式物品栏里的全部物品、还有一批手写的
 * SNBT（按 {@code /give} 的组件语法）覆盖其余的组件类型。三个瞬态组件和只能由客户端
 * 内联发来的 {@code Holder} 没有持久化编解码器，直接在这里用官方类构造。
 */
public final class ComponentOracle {
    private static final int OUTPUT_BUFFER = 1 << 16;

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(PrintStream log) throws Exception {
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
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER);
                BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8))) {
            RegistryAccess registries = loadRegistries(resources);
            new Oracle(registries, out, in, log).run();
        }
    }

    /**
     * 和 `WorldLoader.load` 一样加载注册表和标签，再加载服务端资源。
     *
     * 物品的默认组件是数据驱动的，不走 `ReloadableServerResources` 的话
     * `Item.components()` 会抛「Components not bound yet」。
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

    /**
     * 一道题：一个名字加一个组件补丁。
     *
     * `item` 只在「某个物品的默认组件」这种题里有值，Bamboo 会再拿生成的数据比一遍。
     */
    private record Case(String name, DataComponentPatch patch, int item) {
        Case(String name, DataComponentPatch patch) {
            this(name, patch, -1);
        }
    }

    private static final class Oracle {
        private final RegistryAccess registries;
        private final RegistryOps<Tag> ops;
        private final RegistryOps<HashCode> hashOps;
        private final Writer out;
        private final BufferedReader in;
        private final PrintStream log;
        private final List<Case> cases = new ArrayList<>();
        private final List<ArchiveCase> archive = new ArrayList<>();

        Oracle(RegistryAccess registries, Writer out, BufferedReader in, PrintStream log) {
            this.registries = registries;
            this.ops = registries.createSerializationContext(NbtOps.INSTANCE);
            this.hashOps = registries.createSerializationContext(HashOps.CRC32C_INSTANCE);
            this.out = out;
            this.in = in;
            this.log = log;
        }

        void run() throws Exception {
            collectItemDefaults();
            collectCreativeTabs();
            collectHandWritten();
            collectTransient();

            writeHeader();
            for (int index = 0; index < cases.size(); index++) {
                writeCase(index, cases.get(index));
            }
            writeEnumFolds();
            writeArchive();
            writeSnbt();
            line("{\"type\":\"end\",\"cases\":" + cases.size() + "}");
            out.flush();

            verify();
            out.flush();
        }

        /**
         * 每个物品的默认组件，整份搬进一个补丁里。
         *
         * 组件按注册表 ID 排序，和 Extractor 生成 `item_components.rs` 时一样：
         * 官方的 `DataComponentMap` 超过 8 项就按身份哈希排序，遍历顺序每次运行都可能不同。
         */
        private void collectItemDefaults() {
            for (Item item : BuiltInRegistries.ITEM) {
                List<TypedDataComponent<?>> components = new ArrayList<>();
                for (TypedDataComponent<?> component : item.components()) {
                    components.add(component);
                }
                components.sort(Comparator.comparingInt(
                        component -> BuiltInRegistries.DATA_COMPONENT_TYPE.getId(component.type())));

                DataComponentPatch.Builder builder = DataComponentPatch.builder();
                for (TypedDataComponent<?> component : components) {
                    builder.set(component);
                }
                cases.add(new Case(
                        "defaults " + BuiltInRegistries.ITEM.getKey(item),
                        builder.build(),
                        BuiltInRegistries.ITEM.getId(item)));
            }
        }

        /** 创造模式物品栏重建后的全部物品，附魔书、药水、烟花、唱片这些都在里面。 */
        private void collectCreativeTabs() {
            CreativeModeTabs.tryRebuildTabContents(FeatureFlags.DEFAULT_FLAGS, true, registries);
            for (CreativeModeTab tab : CreativeModeTabs.allTabs()) {
                String name = BuiltInRegistries.CREATIVE_MODE_TAB.getKey(tab) + " ";
                for (ItemStack stack : tab.getSearchTabDisplayItems()) {
                    DataComponentPatch patch = stack.getComponentsPatch();
                    if (!patch.isEmpty()) {
                        cases.add(new Case(name + stack.getItem(), patch));
                    }
                }
            }
        }

        /** 手写的 SNBT，按 `/give` 的组件语法，覆盖默认组件碰不到的类型。 */
        private void collectHandWritten() throws Exception {
            for (String snbt : HAND_WRITTEN) {
                try {
                    CompoundTag tag = TagParser.parseCompoundFully(snbt);
                    DataComponentPatch patch = DataComponentPatch.CODEC.parse(ops, tag).getOrThrow();
                    cases.add(new Case("snbt " + snbt, patch));
                } catch (Exception error) {
                    line("{\"type\":\"skipped\",\"case\":" + quote(snbt) + ",\"error\":" + quote(String.valueOf(error.getMessage())) + "}");
                }
            }
        }

        /** 没有持久化编解码器的组件，还有只能内联发来的 `Holder`。 */
        private void collectTransient() {
            Holder<SoundEvent> inlineSound =
                    Holder.direct(new SoundEvent(Identifier.parse("bamboo:oracle.sound"), Optional.of(17.0F)));
            Holder<SoundEvent> plainSound =
                    Holder.direct(new SoundEvent(Identifier.parse("bamboo:oracle.plain"), Optional.empty()));
            Holder<TrimMaterial> inlineMaterial =
                    Holder.direct(new TrimMaterial(Identifier.parse("bamboo:palette"), Component.literal("material")));
            Holder<TrimPattern> inlinePattern = Holder.direct(
                    new TrimPattern(Identifier.parse("bamboo:asset"), Component.literal("pattern"), true));

            cases.add(new Case("transient creative_slot_lock", DataComponentPatch.builder()
                    .set(DataComponents.CREATIVE_SLOT_LOCK, Unit.INSTANCE)
                    .build()));
            cases.add(new Case("transient map_post_processing lock", DataComponentPatch.builder()
                    .set(DataComponents.MAP_POST_PROCESSING, MapPostProcessing.LOCK)
                    .build()));
            cases.add(new Case("transient map_post_processing scale", DataComponentPatch.builder()
                    .set(DataComponents.MAP_POST_PROCESSING, MapPostProcessing.SCALE)
                    .build()));
            cases.add(new Case("transient additional_trade_cost", DataComponentPatch.builder()
                    .set(DataComponents.ADDITIONAL_TRADE_COST, 7)
                    .build()));
            cases.add(new Case("inline sound_event", DataComponentPatch.builder()
                    .set(DataComponents.BREAK_SOUND, inlineSound)
                    .set(DataComponents.NOTE_BLOCK_SOUND, Identifier.parse("bamboo:note"))
                    .build()));
            cases.add(new Case("inline instrument", DataComponentPatch.builder()
                    .set(
                            DataComponents.INSTRUMENT,
                            new net.minecraft.world.item.component.InstrumentComponent(Holder.direct(
                                    new Instrument(plainSound, 7.0F, 256.0F, 1, Component.literal("horn")))))
                    .build()));
            cases.add(new Case("inline jukebox_song", DataComponentPatch.builder()
                    .set(
                            DataComponents.JUKEBOX_PLAYABLE,
                            new net.minecraft.world.item.JukeboxPlayable(Holder.direct(
                                    new JukeboxSong(inlineSound, Component.literal("song"), 12.5F, 9))))
                    .build()));
            cases.add(new Case("inline trim", DataComponentPatch.builder()
                    .set(DataComponents.TRIM, new ArmorTrim(inlineMaterial, inlinePattern))
                    .set(DataComponents.PROVIDES_TRIM_MATERIAL, inlineMaterial)
                    .build()));
            cases.add(new Case("inline banner_pattern", DataComponentPatch.builder()
                    .set(
                            DataComponents.BANNER_PATTERNS,
                            new net.minecraft.world.level.block.entity.BannerPatternLayers(List.of(
                                    new net.minecraft.world.level.block.entity.BannerPatternLayers.Layer(
                                            Holder.direct(new net.minecraft.world.level.block.entity.BannerPattern(
                                                    Identifier.parse("bamboo:pattern"), "block.bamboo.pattern")),
                                            net.minecraft.world.item.DyeColor.LIME))))
                    .build()));
            cases.add(new Case("inline painting_variant", DataComponentPatch.builder()
                    .set(
                            DataComponents.PAINTING_VARIANT,
                            Holder.direct(new PaintingVariant(
                                    3,
                                    2,
                                    Identifier.parse("bamboo:painting"),
                                    Optional.of(Component.literal("title")),
                                    Optional.empty())))
                    .build()));
        }

        private void writeHeader() throws Exception {
            StringBuilder types = new StringBuilder();
            Registry<DataComponentType<?>> registry = BuiltInRegistries.DATA_COMPONENT_TYPE;
            for (DataComponentType<?> type : registry) {
                if (types.length() > 0) {
                    types.append(',');
                }
                types.append(quote(String.valueOf(registry.getKey(type))));
            }
            line("{\"type\":\"header\",\"version\":"
                    + quote(SharedConstants.getCurrentVersion().id())
                    + ",\"protocol\":"
                    + SharedConstants.getCurrentVersion().protocolVersion()
                    + ",\"component_types\":["
                    + types
                    + "]}");
        }

        private void writeCase(int index, Case one) throws Exception {
            RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
            DataComponentPatch.STREAM_CODEC.encode(buffer, one.patch());
            byte[] bytes = new byte[buffer.readableBytes()];
            buffer.readBytes(bytes);
            line("{\"type\":\"case\",\"index\":"
                    + index
                    + ",\"name\":"
                    + quote(one.name())
                    + ",\"item\":"
                    + one.item()
                    + ",\"bytes\":\""
                    + hex(bytes)
                    + "\"}");
        }

        /**
         * 按 ID 查表的枚举：喂一个原始 ID 进去，看官方折回成什么。
         *
         * 官方这些编解码器不会失败，越界的 ID 会按 `ByIdMap` 的策略折成一个合法值，
         * 再写出去就和读到的不一样。这里把输入和官方重新编码出来的字节都交给 Bamboo 比。
         */
        private void writeEnumFolds() throws Exception {
            for (EnumProbe probe : enumProbes()) {
                for (int raw : probe.raws()) {
                    RegistryFriendlyByteBuf input = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
                    input.writeVarInt(1);
                    input.writeVarInt(0);
                    input.writeVarInt(BuiltInRegistries.DATA_COMPONENT_TYPE.getId(probe.type()));
                    probe.writer().accept(input, raw);
                    byte[] inputBytes = new byte[input.readableBytes()];
                    input.getBytes(input.readerIndex(), inputBytes);

                    String error = null;
                    byte[] outputBytes = inputBytes;
                    try {
                        DataComponentPatch decoded = DataComponentPatch.STREAM_CODEC.decode(input);
                        if (input.readableBytes() != 0) {
                            error = input.readableBytes() + " trailing bytes";
                        } else {
                            RegistryFriendlyByteBuf again =
                                    new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
                            DataComponentPatch.STREAM_CODEC.encode(again, decoded);
                            outputBytes = new byte[again.readableBytes()];
                            again.readBytes(outputBytes);
                        }
                    } catch (Exception failure) {
                        error = String.valueOf(failure);
                    }

                    line("{\"type\":\"fold\",\"name\":"
                            + quote(probe.name() + " " + raw)
                            + ",\"input\":\""
                            + hex(inputBytes)
                            + "\",\"output\":\""
                            + hex(outputBytes)
                            + "\",\"error\":"
                            + (error == null ? "null" : quote(error))
                            + "}");
                }
            }
        }

        /** 读 Bamboo 写回来的字节，用官方代码解出补丁，和原来的比。 */
        private void verify() throws Exception {
            List<String> mismatches = new ArrayList<>();
            int checked = 0;
            String text;
            while (checked < cases.size() && (text = in.readLine()) != null) {
                int space = text.indexOf(' ');
                if (space < 0) {
                    mismatches.add("bad line from bamboo: " + text);
                    continue;
                }
                int index = Integer.parseInt(text.substring(0, space));
                byte[] bytes = unhex(text.substring(space + 1));
                Case one = cases.get(index);
                checked++;
                try {
                    RegistryFriendlyByteBuf buffer =
                            new RegistryFriendlyByteBuf(Unpooled.wrappedBuffer(bytes), registries);
                    DataComponentPatch decoded = DataComponentPatch.STREAM_CODEC.decode(buffer);
                    if (buffer.readableBytes() != 0) {
                        mismatches.add(one.name() + ": " + buffer.readableBytes() + " trailing bytes");
                        continue;
                    }
                    if (decoded.equals(one.patch())) {
                        continue;
                    }
                    // 有两处 equals 和字节各自会误报，两种判断有一种过就算一致：
                    // 组件谓词里的 AnyValueType 没有 equals，分别构造的两个实例永远不相等；
                    // block_state 和 enchantments 在官方内存里是哈希表，重新编码的键序会变。
                    RegistryFriendlyByteBuf again = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
                    DataComponentPatch.STREAM_CODEC.encode(again, decoded);
                    byte[] reencoded = new byte[again.readableBytes()];
                    again.readBytes(reencoded);
                    if (!Arrays.equals(reencoded, bytes)) {
                        mismatches.add(one.name()
                                + ": official read back " + decoded + " and wrote " + hex(reencoded)
                                + ", expected " + hex(bytes));
                    }
                } catch (Exception error) {
                    mismatches.add(one.name() + ": official cannot decode: " + error);
                }
            }

            verifyArchive(mismatches);

            StringBuilder list = new StringBuilder();
            for (String mismatch : mismatches) {
                if (list.length() > 0) {
                    list.append(',');
                }
                list.append(quote(mismatch));
            }
            line("{\"type\":\"verified\",\"checked\":" + checked + ",\"mismatches\":[" + list + "]}");
            log.println("verified " + checked + " cases, " + mismatches.size() + " mismatches");
        }

        /** 一道存档题：官方编出来的物品堆和原始补丁，用来解 Bamboo 写回的 NBT。 */
        private record ArchiveCase(String name, ItemStack stack, DataComponentPatch patch, Tag stackTag, Tag patchTag) {}

        /**
         * 同一批补丁再走存档 {@code Codec}。物品堆用 {@code ItemStack.CODEC}，补丁本身用
         * {@code DataComponentPatch.CODEC}。哈希是 {@code HashedStack.create}。
         * 编不出来的（未注册的内联 Holder）记成 skipped，不算差异。只剩瞬态组件的补丁
         * 编出来是空的 {@code components}，官方自己再解再编也会把这个字段省掉，同样跳过。
         *
         * Encode failures (an inline holder with no registry id) are skipped. A patch
         * whose only entries are transient encodes as an empty {@code components}
         * compound, and the official codec drops that field on the next encode, so
         * those tags are skipped too.
         */
        private void writeArchive() throws Exception {
            int index = 0;
            for (Case one : cases) {
                if (transientOnly(one.patch())) {
                    // 只含瞬态组件的补丁编出来是空的 components。再解一次瞬态就没了，
                    // 空字段也会被省掉，官方自己也往返不了这份标签。
                    //
                    // A transient-only patch encodes as an empty components compound.
                    // Decoding drops the component and the next encode omits the field,
                    // so the official codec does not round-trip that tag either.
                    line("{\"type\":\"skipped\",\"case\":"
                            + quote("archive " + one.name())
                            + ",\"error\":"
                            + quote("transient component does not round-trip through ItemStack.CODEC")
                            + "}");
                    continue;
                }
                try {
                    Holder<Item> holder = one.item() >= 0
                            ? BuiltInRegistries.ITEM.get(one.item()).orElseThrow()
                            : Items.STONE.builtInRegistryHolder();
                    if (holder == null) {
                        throw new IllegalStateException("missing item " + one.item());
                    }
                    ItemStack stack = new ItemStack(holder, 1, one.patch());
                    Tag stackTag = ItemStack.CODEC.encodeStart(ops, stack).getOrThrow();
                    Tag patchTag = DataComponentPatch.CODEC.encodeStart(ops, one.patch()).getOrThrow();
                    boolean skipHash = false;
                    String added = "[]";
                    String removed = "[]";
                    try {
                        HashedStack hashed = HashedStack.create(stack, this::hashComponent);
                        added = addedJson(hashed);
                        removed = removedJson(hashed);
                    } catch (RuntimeException error) {
                        skipHash = true;
                    }
                    archive.add(new ArchiveCase(one.name(), stack, one.patch(), stackTag, patchTag));
                    line("{\"type\":\"archive\",\"index\":"
                            + index
                            + ",\"name\":"
                            + quote(one.name())
                            + ",\"stack\":\""
                            + hex(writeTag(stackTag))
                            + "\",\"patch\":\""
                            + hex(writeTag(patchTag))
                            + "\",\"added\":"
                            + added
                            + ",\"removed\":"
                            + removed
                            + ",\"skip_hash\":"
                            + skipHash
                            + "}");
                    index++;
                } catch (Exception error) {
                    line("{\"type\":\"skipped\",\"case\":"
                            + quote("archive " + one.name())
                            + ",\"error\":"
                            + quote(String.valueOf(error.getMessage()))
                            + "}");
                }
            }
        }

        /** 去掉瞬态以后什么都不剩。空补丁不算。 */
        private static boolean transientOnly(DataComponentPatch patch) {
            if (patch.isEmpty()) {
                return false;
            }
            DataComponentPatch.SplitResult split = patch.split();
            for (DataComponentType<?> type : split.added().keySet()) {
                if (!type.isTransient()) {
                    return false;
                }
            }
            for (DataComponentType<?> type : split.removed()) {
                if (!type.isTransient()) {
                    return false;
                }
            }
            return true;
        }

        private int hashComponent(TypedDataComponent<?> component) {
            return component.encodeValue(hashOps).getOrThrow(IllegalStateException::new).asInt();
        }

        private String addedJson(HashedStack hashed) {
            if (!(hashed instanceof HashedStack.ActualItem actual)) {
                return "[]";
            }
            StringBuilder out = new StringBuilder("[");
            boolean first = true;
            for (var entry : actual.components().addedComponents().entrySet()) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append('[')
                        .append(BuiltInRegistries.DATA_COMPONENT_TYPE.getId(entry.getKey()))
                        .append(',')
                        .append(entry.getValue())
                        .append(']');
            }
            out.append(']');
            return out.toString();
        }

        private String removedJson(HashedStack hashed) {
            if (!(hashed instanceof HashedStack.ActualItem actual)) {
                return "[]";
            }
            StringBuilder out = new StringBuilder("[");
            boolean first = true;
            for (DataComponentType<?> type : actual.components().removedComponents()) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append(BuiltInRegistries.DATA_COMPONENT_TYPE.getId(type));
            }
            out.append(']');
            return out.toString();
        }

        /** Bamboo 写回的 NBT，官方再解一次，要回到同一份物品堆和同一份补丁。 */
        private void verifyArchive(List<String> mismatches) throws Exception {
            for (int index = 0; index < archive.size(); index++) {
                String text = in.readLine();
                ArchiveCase one = archive.get(index);
                if (text == null) {
                    mismatches.add(one.name() + ": missing archive reply");
                    return;
                }
                String[] parts = text.split(" ");
                if (parts.length != 4 || !"A".equals(parts[0])) {
                    mismatches.add(one.name() + ": bad archive reply " + text);
                    continue;
                }
                try {
                    CompoundTag stackTag = readCompound(parts[2]);
                    ItemStack decoded = ItemStack.CODEC.parse(ops, stackTag).getOrThrow();
                    if (!ItemStack.matches(decoded, one.stack())) {
                        Tag again = ItemStack.CODEC.encodeStart(ops, decoded).getOrThrow();
                        if (!again.equals(one.stackTag())) {
                            mismatches.add(one.name() + ": official stack readback differs");
                        }
                    }
                } catch (Exception error) {
                    mismatches.add(one.name() + ": official cannot decode stack: " + error);
                }
                try {
                    CompoundTag patchTag = readCompound(parts[3]);
                    DataComponentPatch decoded = DataComponentPatch.CODEC.parse(ops, patchTag).getOrThrow();
                    if (!decoded.equals(one.patch())) {
                        Tag again = DataComponentPatch.CODEC.encodeStart(ops, decoded).getOrThrow();
                        if (!again.equals(one.patchTag())) {
                            mismatches.add(one.name() + ": official patch readback differs");
                        }
                    }
                } catch (Exception error) {
                    mismatches.add(one.name() + ": official cannot decode patch: " + error);
                }
            }
        }

        private byte[] writeTag(Tag tag) {
            RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
            buffer.writeNbt(tag);
            byte[] bytes = new byte[buffer.readableBytes()];
            buffer.readBytes(bytes);
            return bytes;
        }

        private CompoundTag readCompound(String text) {
            byte[] bytes = unhex(text);
            io.netty.buffer.ByteBuf buffer = Unpooled.wrappedBuffer(bytes);
            CompoundTag tag = FriendlyByteBuf.readNbt(buffer);
            if (tag == null) {
                throw new IllegalStateException("missing compound");
            }
            if (buffer.isReadable()) {
                throw new IllegalStateException(buffer.readableBytes() + " trailing bytes");
            }
            return tag;
        }

        /** 一批 NBT 的两种打印，再加手写 SNBT 和非法输入。 */
        private void writeSnbt() throws Exception {
            int index = 0;
            for (Tag tag : snbtSamples()) {
                String compact = tag.toString();
                String pretty = new SnbtPrinterTagVisitor().visit(tag);
                Tag parsed = TagParser.create(NbtOps.INSTANCE).parseFully(compact);
                Tag prettyParsed = TagParser.create(NbtOps.INSTANCE).parseFully(pretty);
                line("{\"type\":\"snbt\",\"name\":"
                        + quote("sample " + index)
                        + ",\"source\":\""
                        + hex(writeTag(tag))
                        + "\",\"compact\":"
                        + quote(compact)
                        + ",\"pretty\":"
                        + quote(pretty)
                        + ",\"parsed\":\""
                        + hex(writeTag(parsed))
                        + "\",\"pretty_parsed\":\""
                        + hex(writeTag(prettyParsed))
                        + "\"}");
                index++;
            }
            for (String text : SNBT_INPUTS) {
                String error = null;
                String parsed = null;
                try {
                    Tag tag = TagParser.create(NbtOps.INSTANCE).parseFully(text);
                    parsed = hex(writeTag(tag));
                } catch (Exception failure) {
                    error = String.valueOf(failure.getMessage());
                }
                line("{\"type\":\"snbtInput\",\"text\":"
                        + quote(text)
                        + ",\"error\":"
                        + (error == null ? "null" : quote(error))
                        + ",\"parsed\":"
                        + (parsed == null ? "null" : quote(parsed))
                        + "}");
            }
        }

        private static List<Tag> snbtSamples() {
            List<Tag> tags = new ArrayList<>();
            tags.add(ByteTag.valueOf((byte) 1));
            tags.add(ByteTag.valueOf((byte) -1));
            tags.add(ShortTag.valueOf((short) 258));
            tags.add(IntTag.valueOf(-42));
            tags.add(LongTag.valueOf(1L));
            tags.add(FloatTag.valueOf(1.5f));
            tags.add(FloatTag.valueOf(Float.NaN));
            tags.add(FloatTag.valueOf(Float.POSITIVE_INFINITY));
            tags.add(FloatTag.valueOf(-0.0f));
            tags.add(DoubleTag.valueOf(-0.0d));
            tags.add(DoubleTag.valueOf(Double.NaN));
            tags.add(StringTag.valueOf("plain"));
            tags.add(StringTag.valueOf("a'b\"c"));
            tags.add(StringTag.valueOf("line\n\t"));
            tags.add(StringTag.valueOf("竹"));
            tags.add(new ByteArrayTag(new byte[] {1, -2}));
            tags.add(new IntArrayTag(new int[] {1, 2}));
            tags.add(new LongArrayTag(new long[] {1L, 2L}));
            ListTag list = new ListTag();
            list.add(IntTag.valueOf(1));
            list.add(IntTag.valueOf(2));
            tags.add(list);
            tags.add(new ListTag());
            tags.add(new CompoundTag());
            CompoundTag compound = new CompoundTag();
            compound.putString("b", "x");
            compound.putInt("a", 1);
            compound.put("nested", list.copy());
            compound.putByteArray("bytes", new byte[] {1, 2});
            tags.add(compound);
            CompoundTag ordered = new CompoundTag();
            ordered.putString("DataVersion", "1");
            ordered.putString("author", "bamboo");
            ordered.put("size", new ListTag());
            tags.add(ordered);
            return tags;
        }

        private void line(String text) throws Exception {
            out.write(text);
            out.write('\n');
        }
    }

    /** 一个按 ID 查表的枚举：装着它的组件、怎么写出一个完整的值、要试哪些原始 ID。 */
    private record EnumProbe(
            String name, DataComponentType<?> type, ObjIntConsumer<RegistryFriendlyByteBuf> writer, int[] raws) {}

    /** 默认要试的原始 ID：负数、第一个、最后一个、刚好越界、越界一格、很大的数。 */
    private static int[] raws(int count) {
        return new int[] {-1, 0, count - 1, count, count + 1, 10000};
    }

    /** 稀疏枚举：负数、表里的第一个和最后一个、表长、表长加一、很大的数。 */
    private static int[] sparseRaws(int[] ids) {
        return new int[] {-1, ids[0], ids[ids.length - 1], ids.length, ids.length + 1, 10000};
    }

    private static int[] packedFishIds() {
        int[] ids = new int[TropicalFish.Pattern.values().length];
        for (int index = 0; index < ids.length; index++) {
            ids[index] = TropicalFish.Pattern.values()[index].getPackedId();
        }
        return ids;
    }

    private static int[] rabbitIds() {
        int[] ids = new int[Rabbit.Variant.values().length];
        for (int index = 0; index < ids.length; index++) {
            ids[index] = Rabbit.Variant.values()[index].id();
        }
        return ids;
    }

    /** 单个 VarInt 就是整个组件值的那些枚举。 */
    private static EnumProbe single(String name, DataComponentType<?> type, int count) {
        return new EnumProbe(name, type, (buffer, raw) -> buffer.writeVarInt(raw), raws(count));
    }

    private static List<EnumProbe> enumProbes() {
        List<EnumProbe> probes = new ArrayList<>();
        probes.add(single("rarity", DataComponents.RARITY, Rarity.values().length));
        probes.add(single("dye_color", DataComponents.DYE, DyeColor.values().length));
        probes.add(single(
                "map_post_processing", DataComponents.MAP_POST_PROCESSING, MapPostProcessing.values().length));
        probes.add(single("fox_variant", DataComponents.FOX_VARIANT, Fox.Variant.values().length));
        probes.add(single("axolotl_variant", DataComponents.AXOLOTL_VARIANT, Axolotl.Variant.values().length));
        probes.add(single("salmon_size", DataComponents.SALMON_SIZE, Salmon.Variant.values().length));
        probes.add(single("parrot_variant", DataComponents.PARROT_VARIANT, Parrot.Variant.values().length));
        probes.add(single(
                "mooshroom_variant", DataComponents.MOOSHROOM_VARIANT, MushroomCow.Variant.values().length));
        probes.add(single("llama_variant", DataComponents.LLAMA_VARIANT, Llama.Variant.values().length));
        probes.add(single(
                "horse_variant",
                DataComponents.HORSE_VARIANT,
                net.minecraft.world.entity.animal.equine.Variant.values().length));

        probes.add(new EnumProbe(
                "tropical_fish_pattern",
                DataComponents.TROPICAL_FISH_PATTERN,
                (buffer, raw) -> buffer.writeVarInt(raw),
                sparseRaws(packedFishIds())));
        probes.add(new EnumProbe(
                "rabbit_variant",
                DataComponents.RABBIT_VARIANT,
                (buffer, raw) -> buffer.writeVarInt(raw),
                sparseRaws(rabbitIds())));

        // 挥手动画是「类型加时长」。
        probes.add(new EnumProbe(
                "swing_animation_type",
                DataComponents.ATTACK_ANIMATION,
                (buffer, raw) -> {
                    buffer.writeVarInt(raw);
                    buffer.writeVarInt(6);
                },
                raws(SwingAnimationType.values().length)));

        // 烟花之星是「形状加两个颜色列表加两个开关」。
        probes.add(new EnumProbe(
                "firework_shape",
                DataComponents.FIREWORK_EXPLOSION,
                (buffer, raw) -> {
                    buffer.writeVarInt(raw);
                    buffer.writeVarInt(0);
                    buffer.writeVarInt(0);
                    buffer.writeBoolean(false);
                    buffer.writeBoolean(false);
                },
                raws(FireworkExplosion.Shape.values().length)));

        // 食用动画埋在 consumable 中间，前后的字段都要凑齐。
        probes.add(new EnumProbe(
                "item_use_animation",
                DataComponents.CONSUMABLE,
                (buffer, raw) -> {
                    buffer.writeFloat(1.6F);
                    buffer.writeVarInt(raw);
                    buffer.writeVarInt(1);
                    buffer.writeBoolean(false);
                    buffer.writeVarInt(0);
                },
                raws(ItemUseAnimation.values().length)));

        // 装备槽位是 equippable 的第一个字段，后面还有一长串。
        probes.add(new EnumProbe(
                "equipment_slot",
                DataComponents.EQUIPPABLE,
                (buffer, raw) -> {
                    buffer.writeVarInt(raw);
                    buffer.writeVarInt(1);
                    buffer.writeBoolean(false);
                    buffer.writeBoolean(false);
                    buffer.writeBoolean(false);
                    for (int flag = 0; flag < 5; flag++) {
                        buffer.writeBoolean(false);
                    }
                    buffer.writeVarInt(1);
                },
                raws(EquipmentSlot.values().length)));

        // 属性修饰的槽位分组、运算方式和显示方式都在同一条 entry 里。
        probes.add(new EnumProbe(
                "equipment_slot_group",
                DataComponents.ATTRIBUTE_MODIFIERS,
                (buffer, raw) -> writeAttributeEntry(buffer, 0, raw, 0),
                raws(EquipmentSlotGroup.values().length)));
        probes.add(new EnumProbe(
                "attribute_operation",
                DataComponents.ATTRIBUTE_MODIFIERS,
                (buffer, raw) -> writeAttributeEntry(buffer, raw, 0, 0),
                raws(AttributeModifier.Operation.values().length)));
        // 显示方式折成 2（override）时后面还要跟一段文本组件，所以不试那个值，
        // 手写的 SNBT 里已经有带文本的 override。
        int displays = ItemAttributeModifiers.Display.Type.values().length;
        probes.add(new EnumProbe(
                "attribute_display_type",
                DataComponents.ATTRIBUTE_MODIFIERS,
                (buffer, raw) -> writeAttributeEntry(buffer, 0, 0, raw),
                new int[] {-1, 0, 1, displays, displays + 1, 10000}));
        return probes;
    }

    /** 一条 `attribute_modifiers`：属性、修饰符、槽位分组、显示方式。 */
    private static void writeAttributeEntry(
            RegistryFriendlyByteBuf buffer, int operation, int slotGroup, int display) {
        buffer.writeVarInt(1);
        buffer.writeVarInt(0);
        buffer.writeUtf("bamboo:probe");
        buffer.writeDouble(1.0);
        buffer.writeVarInt(operation);
        buffer.writeVarInt(slotGroup);
        buffer.writeVarInt(display);
    }

    private static String hex(byte[] bytes) {
        StringBuilder text = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            text.append(Character.forDigit((value >> 4) & 0xf, 16));
            text.append(Character.forDigit(value & 0xf, 16));
        }
        return text.toString();
    }

    private static byte[] unhex(String text) {
        byte[] bytes = new byte[text.length() / 2];
        for (int index = 0; index < bytes.length; index++) {
            int high = Character.digit(text.charAt(index * 2), 16);
            int low = Character.digit(text.charAt(index * 2 + 1), 16);
            bytes[index] = (byte) ((high << 4) | low);
        }
        return bytes;
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

    /** 手写的 SNBT。前一半合法，后一半非法，两边都要同样接受或拒绝。 */
    private static final String[] SNBT_INPUTS = {
        "1b",
        "1s",
        "1",
        "1l",
        "1L",
        "1.5f",
        "1.5d",
        "true",
        "false",
        "'ab'",
        "\"a\\nb\"",
        "{a:1,b:2s}",
        "[1,2,3]",
        "[B;1b,2b]",
        "[I;1,2]",
        "[L;1l,2l]",
        "{}",
        "[]",
        "  { a : 1 }  ",
        "0x10",
        "{a:1,a:2}",
        "{",
        "{a:}",
        "1x",
        "[B;1]",
        "",
    };

    /** 手写的组件补丁，按 `/give` 的组件语法。每一行至少覆盖一种组件类型。 */
    private static final String[] HAND_WRITTEN = {
        "{custom_data:{a:1,b:\"x\",c:[1,2,3],d:{e:1.5d}}}",
        "{max_stack_size:16}",
        "{max_damage:200,max_stack_size:1}",
        "{damage:5}",
        "{unbreakable:{}}",
        "{use_effects:{can_sprint:1b,interact_vibrations:0b,speed_multiplier:0.25f}}",
        "{custom_name:{text:\"Kitten\",color:\"gold\",bold:1b,italic:0b,underlined:1b,strikethrough:0b,"
                + "obfuscated:0b,insertion:\"x\",font:\"minecraft:default\",shadow_color:123,"
                + "click_event:{action:\"run_command\",command:\"/say hi\"},"
                + "hover_event:{action:\"show_text\",value:\"hi\"},extra:[{text:\"!\",color:\"#FF0000\"}]}}",
        "{custom_name:{translate:\"chat.type.text\",with:[\"a\",1,{text:\"b\"}],fallback:\"f\"}}",
        "{custom_name:{keybind:\"key.jump\"}}",
        "{custom_name:{score:{name:\"@s\",objective:\"o\"}}}",
        "{custom_name:{selector:\"@a\",separator:\", \"}}",
        "{custom_name:{nbt:\"Items\",block:\"0 0 0\",interpret:1b}}",
        "{custom_name:{object:\"atlas\",sprite:\"minecraft:x\"}}",
        "{minimum_attack_charge:0.5f}",
        "{damage_type:\"minecraft:arrow\"}",
        "{item_name:{translate:\"item.minecraft.stone\"}}",
        "{item_model:\"minecraft:stone\"}",
        "{lore:[{text:\"a\"},{text:\"b\",color:\"red\"}]}",
        "{rarity:\"epic\"}",
        "{enchantments:{\"minecraft:sharpness\":5,\"minecraft:unbreaking\":3}}",
        "{can_place_on:[{blocks:[\"minecraft:stone\",\"minecraft:dirt\"]}]}",
        "{can_place_on:[{blocks:\"#minecraft:planks\",state:{facing:\"north\",age:{min:\"1\",max:\"3\"}},"
                + "nbt:{a:1},components:{\"minecraft:damage\":5},predicates:{\"minecraft:custom_name\":{}}}]}",
        "{can_break:[{blocks:\"#minecraft:mineable/pickaxe\"}]}",
        "{attribute_modifiers:[{type:\"minecraft:attack_damage\",id:\"minecraft:base\",amount:3.0d,"
                + "operation:\"add_value\",slot:\"mainhand\",display:{type:\"override\",value:\"x\"}},"
                + "{type:\"minecraft:armor\",id:\"a:b\",amount:1.0d,operation:\"add_multiplied_base\","
                + "slot:\"any\",display:{type:\"hidden\"}},"
                + "{type:\"minecraft:armor_toughness\",id:\"a:c\",amount:0.5d,operation:\"add_multiplied_total\"}]}",
        "{custom_model_data:{floats:[1.0f,2.0f],flags:[1b,0b],strings:[\"a\"],colors:[16711680]}}",
        "{tooltip_display:{hide_tooltip:0b,hidden_components:[\"minecraft:damage\",\"minecraft:enchantments\"]}}",
        "{repair_cost:3}",
        "{enchantment_glint_override:1b}",
        "{intangible_projectile:{}}",
        "{food:{nutrition:4,saturation:0.3f,can_always_eat:1b}}",
        "{consumable:{consume_seconds:1.6f,animation:\"drink\",sound:\"minecraft:entity.generic.eat\","
                + "has_consume_particles:0b,on_consume_effects:["
                + "{type:\"apply_effects\",effects:[{id:\"minecraft:speed\",amplifier:1,duration:100,"
                + "ambient:0b,show_particles:1b,show_icon:1b}],probability:0.5f},"
                + "{type:\"remove_effects\",effects:\"minecraft:speed\"},"
                + "{type:\"clear_all_effects\"},"
                + "{type:\"teleport_randomly\",diameter:8.0f,directional_particles:0b},"
                + "{type:\"play_sound\",sound:\"minecraft:block.note_block.bell\"}]}}",
        "{use_remainder:{id:\"minecraft:glass_bottle\",count:1}}",
        "{use_cooldown:{seconds:1.5f,cooldown_group:\"minecraft:x\"}}",
        "{damage_resistant:{types:\"#minecraft:is_fire\"}}",
        "{tool:{rules:[{blocks:\"#minecraft:mineable/axe\",speed:6.0f,correct_for_drops:1b},"
                + "{blocks:\"minecraft:stone\"}],default_mining_speed:1.0f,damage_per_block:2,"
                + "can_destroy_blocks_in_creative:0b}}",
        "{weapon:{item_damage_per_attack:2,disable_blocking_for_seconds:5.0f}}",
        "{attack_range:{min_reach:0.0f,max_reach:3.0f,min_creative_reach:0.0f,max_creative_reach:5.0f,"
                + "hitbox_margin:0.3f,mob_factor:1.0f}}",
        "{enchantable:{value:15}}",
        "{equippable:{slot:\"head\",equip_sound:\"minecraft:item.armor.equip_iron\","
                + "asset_id:\"minecraft:iron\",camera_overlay:\"minecraft:misc/x\","
                + "allowed_entities:\"minecraft:player\",dispensable:0b,swappable:0b,damage_on_hurt:0b,"
                + "equip_on_interact:1b,can_be_sheared:1b,shearing_sound:\"minecraft:entity.sheep.shear\"}}",
        "{repairable:{items:[\"minecraft:iron_ingot\"]}}",
        "{glider:{}}",
        "{tooltip_style:\"minecraft:x\"}",
        "{death_protection:{death_effects:[{type:\"clear_all_effects\"}]}}",
        "{blocks_attacks:{block_delay_seconds:0.25f,disable_cooldown_scale:1.0f,damage_reductions:["
                + "{horizontal_blocking_angle:90.0f,type:\"#minecraft:is_explosion\",base:0.0f,factor:1.0f},"
                + "{base:1.0f,factor:0.0f}],item_damage:{threshold:1.0f,base:0.0f,factor:1.0f},"
                + "bypassed_by:\"#minecraft:bypasses_shield\",block_sound:\"minecraft:item.shield.block\","
                + "disabled_sound:\"minecraft:item.shield.break\"}}",
        "{piercing_weapon:{deals_knockback:0b,dismounts:1b,"
                + "sound:\"minecraft:entity.player.attack.strong\",hit_sound:\"minecraft:entity.player.hurt\"}}",
        "{kinetic_weapon:{contact_cooldown_ticks:10,delay_ticks:2,"
                + "dismount_conditions:{max_duration_ticks:5,min_speed:1.0f,min_relative_speed:2.0f},"
                + "knockback_conditions:{max_duration_ticks:5},damage_conditions:{max_duration_ticks:5},"
                + "forward_movement:0.5f,damage_multiplier:2.0f,"
                + "sound:\"minecraft:entity.player.attack.sweep\",hit_sound:\"minecraft:entity.player.hurt\"}}",
        "{attack_animation:{type:\"stab\",duration:10}}",
        "{interact_animation:{type:\"none\",duration:0}}",
        "{block_transformer:\"minecraft:axe\"}",
        "{villager_food:{nutrition:4}}",
        "{stored_enchantments:{\"minecraft:mending\":1}}",
        "{dye:\"red\"}",
        "{dyed_color:11546150}",
        "{map_id:7}",
        "{map_decorations:{x:{type:\"minecraft:player\",x:1.0d,z:2.0d,rotation:90.0f}}}",
        "{charged_projectiles:[{id:\"minecraft:arrow\",count:1},{id:\"minecraft:firework_rocket\",count:1}]}",
        "{bundle_contents:[{id:\"minecraft:stone\",count:5},{id:\"minecraft:bundle\",count:1,"
                + "components:{\"minecraft:bundle_contents\":[{id:\"minecraft:dirt\",count:2}]}}]}",
        "{potion_contents:{potion:\"minecraft:strength\",custom_color:16711680,custom_effects:["
                + "{id:\"minecraft:speed\",duration:200,amplifier:1,hidden_effect:{amplifier:0,duration:100,"
                + "ambient:0b,show_particles:1b,show_icon:1b}}],custom_name:\"x\"}}",
        "{potion_duration_scale:0.5f}",
        "{suspicious_stew_effects:[{id:\"minecraft:night_vision\",duration:100}]}",
        "{writable_book_content:{pages:[{raw:\"a\",filtered:\"b\"},\"c\"]}}",
        "{written_book_content:{title:{raw:\"t\"},author:\"me\",generation:1,"
                + "pages:[{raw:{text:\"p\"},filtered:{text:\"q\"}},\"plain\"],resolved:1b}}",
        "{trim:{material:\"minecraft:gold\",pattern:\"minecraft:dune\"}}",
        "{debug_stick_state:{\"minecraft:oak_log\":\"axis\"}}",
        "{entity_data:{id:\"minecraft:pig\",Health:10.0f}}",
        "{bucket_entity_data:{BucketVariantTag:1}}",
        "{block_entity_data:{id:\"minecraft:chest\",Lock:{}}}",
        "{instrument:\"minecraft:ponder_goat_horn\"}",
        "{provides_trim_material:\"minecraft:iron\"}",
        "{ominous_bottle_amplifier:3}",
        "{jukebox_playable:\"minecraft:cat\"}",
        "{provides_banner_patterns:[\"minecraft:globe\"]}",
        "{recipes:[\"minecraft:stone_sword\",\"minecraft:iron_sword\"]}",
        "{lodestone_tracker:{target:{dimension:\"minecraft:overworld\",pos:[1,2,3]},tracked:0b}}",
        "{lodestone_tracker:{tracked:1b}}",
        "{firework_explosion:{shape:\"star\",colors:[16711680,255],fade_colors:[65280],"
                + "has_trail:1b,has_twinkle:1b}}",
        "{fireworks:{flight_duration:2,explosions:[{shape:\"burst\",colors:[1]},{shape:\"creeper\"}]}}",
        "{profile:\"Kitten\"}",
        "{profile:{id:[I;1,2,3,4],name:\"Kitten\",properties:[{name:\"textures\",value:\"abc\",signature:\"s\"}],"
                + "texture:\"minecraft:skin\",model:\"slim\"}}",
        "{note_block_sound:\"minecraft:block.note_block.bell\"}",
        "{banner_patterns:[{pattern:\"minecraft:globe\",color:\"red\"},"
                + "{pattern:\"minecraft:creeper\",color:\"blue\"}]}",
        "{base_color:\"white\"}",
        "{pot_decorations:{back:\"minecraft:brick\",left:{id:\"minecraft:angler_pottery_sherd\"},"
                + "right:\"minecraft:brick\",front:\"minecraft:brick\"}}",
        "{pot_decorations:{back:\"minecraft:brick\"}}",
        "{container:[{slot:0,item:{id:\"minecraft:stone\",count:3}},{slot:5,item:\"minecraft:dirt\"}]}",
        "{block_state:{facing:\"north\",waterlogged:\"true\"}}",
        "{bees:[{entity_data:{id:\"minecraft:bee\"},ticks_in_hive:10,min_ticks_in_hive:600}]}",
        "{sulfur_cube_content:{id:\"minecraft:stone\"}}",
        "{lock:{items:\"minecraft:tripwire_hook\"}}",
        "{container_loot:{loot_table:\"minecraft:chests/simple_dungeon\",seed:42L}}",
        "{break_sound:\"minecraft:entity.item.break\"}",
        "{compostable:{layers:1}}",
        "{cooking_fuel:{burn_time:200,speed_multiplier:1.0f}}",
        "{brewing_fuel:{uses:20,speed_multiplier:1.0f}}",
        "{mob_visibility:{targeting_entity_types:\"#minecraft:skeletons\",visibility:0.5f}}",
        "{\"villager/variant\":\"minecraft:plains\"}",
        "{\"wolf/variant\":\"minecraft:pale\"}",
        "{\"wolf/sound_variant\":\"minecraft:big\"}",
        "{\"wolf/collar\":\"red\"}",
        "{\"fox/variant\":\"snow\"}",
        "{\"salmon/size\":\"large\"}",
        "{\"parrot/variant\":\"blue\"}",
        "{\"tropical_fish/pattern\":\"betty\"}",
        "{\"tropical_fish/base_color\":\"orange\"}",
        "{\"tropical_fish/pattern_color\":\"cyan\"}",
        "{\"mooshroom/variant\":\"brown\"}",
        "{\"rabbit/variant\":\"evil\"}",
        "{\"pig/variant\":\"minecraft:warm\"}",
        "{\"pig/sound_variant\":\"minecraft:mini\"}",
        "{\"cow/variant\":\"minecraft:cold\"}",
        "{\"cow/sound_variant\":\"minecraft:moody\"}",
        "{\"chicken/variant\":\"minecraft:warm\"}",
        "{\"chicken/sound_variant\":\"minecraft:picky\"}",
        "{\"zombie_nautilus/variant\":\"minecraft:warm\"}",
        "{\"frog/variant\":\"minecraft:cold\"}",
        "{\"horse/variant\":\"gray\"}",
        "{\"painting/variant\":\"minecraft:kebab\"}",
        "{\"llama/variant\":\"brown\"}",
        "{\"axolotl/variant\":\"blue\"}",
        "{\"cat/variant\":\"minecraft:jellie\"}",
        "{\"cat/sound_variant\":\"minecraft:royal\"}",
        "{\"cat/collar\":\"lime\"}",
        "{\"sheep/color\":\"pink\"}",
        "{\"shulker/color\":\"purple\"}",
        "{provides_pottery_pattern:\"minecraft:angler\"}",
        "{sign_text_front:{messages:[\"a\",{text:\"b\",color:\"red\"},\"c\",\"d\"],color:\"red\","
                + "has_glowing_text:1b}}",
        "{sign_text_back:{messages:[\"a\",\"b\",\"c\",\"d\"],filtered_messages:[\"x\",\"y\",\"z\",\"w\"]}}",
        "{waxed:{}}",
        "{\"cushion/color\":\"gray\"}",
    };

    private ComponentOracle() {}
}
