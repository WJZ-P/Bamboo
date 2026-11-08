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
import java.util.List;
import java.util.Optional;
import java.util.concurrent.ForkJoinPool;
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
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.nbt.TagParser;
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
import net.minecraft.util.Unit;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.CreativeModeTab;
import net.minecraft.world.item.CreativeModeTabs;
import net.minecraft.world.item.Instrument;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.JukeboxSong;
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

    /** 一道题：一个名字加一个组件补丁。 */
    private record Case(String name, DataComponentPatch patch) {}

    private static final class Oracle {
        private final RegistryAccess registries;
        private final RegistryOps<Tag> ops;
        private final Writer out;
        private final BufferedReader in;
        private final PrintStream log;
        private final List<Case> cases = new ArrayList<>();

        Oracle(RegistryAccess registries, Writer out, BufferedReader in, PrintStream log) {
            this.registries = registries;
            this.ops = registries.createSerializationContext(NbtOps.INSTANCE);
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
            line("{\"type\":\"end\",\"cases\":" + cases.size() + "}");
            out.flush();

            verify();
            out.flush();
        }

        /** 每个物品的默认组件，整份搬进一个补丁里。 */
        private void collectItemDefaults() {
            for (Item item : BuiltInRegistries.ITEM) {
                DataComponentPatch.Builder builder = DataComponentPatch.builder();
                for (TypedDataComponent<?> component : item.components()) {
                    builder.set(component);
                }
                DataComponentPatch patch = builder.build();
                if (!patch.isEmpty()) {
                    cases.add(new Case("defaults " + BuiltInRegistries.ITEM.getKey(item), patch));
                }
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
                    + ",\"bytes\":\""
                    + hex(bytes)
                    + "\"}");
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

        private void line(String text) throws Exception {
            out.write(text);
            out.write('\n');
        }
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
