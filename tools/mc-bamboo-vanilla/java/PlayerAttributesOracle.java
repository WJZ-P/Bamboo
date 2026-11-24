import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Collection;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.Set;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.NonNullList;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundUpdateAttributesPacket;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.FluidTags;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityFluidInteraction;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.EquipmentSlotGroup;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.ai.attributes.Attribute;
import net.minecraft.world.entity.ai.attributes.AttributeInstance;
import net.minecraft.world.entity.ai.attributes.AttributeMap;
import net.minecraft.world.entity.ai.attributes.AttributeModifier;
import net.minecraft.world.entity.ai.attributes.Attributes;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.component.ItemAttributeModifiers;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.ItemEnchantments;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.Fluid;
import sun.misc.Unsafe;

/**
 * 用官方 26.3 的真实类对拍玩家属性系统、挖掘速度与属性同步包。
 *
 * <p>不启动服务端、不接受 EULA，加载官方世界注册表与数据包，直接调用：
 * <ul>
 *   <li>{@code Player.createAttributes()} + {@code AttributeMap} +
 *       {@code ItemStack.forEachModifier}（含 {@code EnchantmentHelper.forEachModifier}）+
 *       {@code MobEffect.addAttributeModifiers} + 官方疾跑 / 细雪 / 创造 / 潜行修饰符；</li>
 *   <li>真实 {@code Player.getDestroySpeed(BlockState)}（4000 条随机组合）；</li>
 *   <li>真实 {@code ClientboundUpdateAttributesPacket.STREAM_CODEC} 双向解码与快照比对。</li>
 * </ul>
 */
public final class PlayerAttributesOracle {
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final Unsafe UNSAFE = loadUnsafe();

    private static AttributeModifier CREATIVE_BLOCK_RANGE;
    private static AttributeModifier CREATIVE_ENTITY_RANGE;
    private static AttributeModifier CROUCH_WAYPOINT;
    private static AttributeModifier SPRINTING_SPEED;
    private static Identifier POWDER_SNOW_ID;

    private static Field F_ENTITY_ON_GROUND;
    private static Field F_ENTITY_FLUID_INTERACTION;
    private static Field F_LIVING_ATTRIBUTES;
    private static Field F_LIVING_ACTIVE_EFFECTS;
    private static Field F_PLAYER_INVENTORY;
    private static Field F_INVENTORY_ITEMS;
    private static Field F_INVENTORY_SELECTED;

    private static void initReflection() throws ReflectiveOperationException {
        CREATIVE_BLOCK_RANGE = staticField(ServerPlayer.class, "CREATIVE_BLOCK_INTERACTION_RANGE_MODIFIER");
        CREATIVE_ENTITY_RANGE = staticField(ServerPlayer.class, "CREATIVE_ENTITY_INTERACTION_RANGE_MODIFIER");
        CROUCH_WAYPOINT = staticField(ServerPlayer.class, "WAYPOINT_TRANSMIT_RANGE_CROUCH_MODIFIER");
        SPRINTING_SPEED = staticField(LivingEntity.class, "SPEED_MODIFIER_SPRINTING");
        POWDER_SNOW_ID = staticField(LivingEntity.class, "SPEED_MODIFIER_POWDER_SNOW_ID");

        F_ENTITY_ON_GROUND = declaredField(Entity.class, "onGround");
        F_ENTITY_FLUID_INTERACTION = declaredField(Entity.class, "fluidInteraction");
        F_LIVING_ATTRIBUTES = declaredField(LivingEntity.class, "attributes");
        F_LIVING_ACTIVE_EFFECTS = declaredField(LivingEntity.class, "activeEffects");
        F_PLAYER_INVENTORY = declaredField(Player.class, "inventory");
        F_INVENTORY_ITEMS = declaredField(Inventory.class, "items");
        F_INVENTORY_SELECTED = declaredField(Inventory.class, "selected");
    }

    private PlayerAttributesOracle() {
    }

    private static Unsafe loadUnsafe() {
        try {
            Field field = Unsafe.class.getDeclaredField("theUnsafe");
            field.setAccessible(true);
            return (Unsafe) field.get(null);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    @SuppressWarnings("unchecked")
    private static <T> T staticField(Class<?> owner, String name) throws ReflectiveOperationException {
        Field field = owner.getDeclaredField(name);
        field.setAccessible(true);
        return (T) field.get(null);
    }

    private static Field declaredField(Class<?> owner, String name) throws ReflectiveOperationException {
        Field field = owner.getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }

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
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        initReflection();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            RegistryAccess registries = loadRegistries(resources);
            new Oracle(registries, out).run();
        }
    }

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

    private static final class Oracle {
        private static final EquipmentSlot[] SLOTS = {
            EquipmentSlot.MAINHAND,
            EquipmentSlot.OFFHAND,
            EquipmentSlot.FEET,
            EquipmentSlot.LEGS,
            EquipmentSlot.CHEST,
            EquipmentSlot.HEAD,
        };

        private static final EquipmentSlotGroup[] SLOT_GROUPS = {
            EquipmentSlotGroup.ANY,
            EquipmentSlotGroup.MAINHAND,
            EquipmentSlotGroup.OFFHAND,
            EquipmentSlotGroup.HAND,
            EquipmentSlotGroup.FEET,
            EquipmentSlotGroup.LEGS,
            EquipmentSlotGroup.CHEST,
            EquipmentSlotGroup.HEAD,
            EquipmentSlotGroup.ARMOR,
            EquipmentSlotGroup.BODY,
            EquipmentSlotGroup.SADDLE,
        };

        private final RegistryAccess registries;
        private final Registry<Enchantment> enchantments;
        private final Writer out;
        private final BufferedReader in;

        private final List<Holder<Attribute>> playerAttributes;
        private final List<Item> itemPool;
        private final List<Holder.Reference<Enchantment>> enchantPool;
        private final List<Holder<MobEffect>> effectPool;
        private final List<Item> miningTools;
        private final List<Block> miningBlocks;

        Oracle(RegistryAccess registries, Writer out) {
            this.registries = registries;
            this.enchantments = registries.lookupOrThrow(Registries.ENCHANTMENT);
            this.out = out;
            this.in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));

            AttributeMap probe = new AttributeMap(Player.createAttributes().build());
            this.playerAttributes = new ArrayList<>();
            for (Holder.Reference<Attribute> holder : BuiltInRegistries.ATTRIBUTE.listElements().toList()) {
                if (probe.hasAttribute(holder)) {
                    this.playerAttributes.add(holder);
                }
            }
            this.playerAttributes.sort(Comparator.comparingInt(h -> BuiltInRegistries.ATTRIBUTE.getId(h.value())));

            this.itemPool = List.of(
                    Items.DIAMOND_PICKAXE,
                    Items.NETHERITE_PICKAXE,
                    Items.GOLDEN_PICKAXE,
                    Items.DIAMOND_SWORD,
                    Items.NETHERITE_AXE,
                    Items.TRIDENT,
                    Items.STICK,
                    Items.DIAMOND_HELMET,
                    Items.NETHERITE_CHESTPLATE,
                    Items.IRON_LEGGINGS,
                    Items.LEATHER_BOOTS,
                    Items.NETHERITE_BOOTS,
                    Items.TURTLE_HELMET,
                    Items.SHIELD
            );

            String[] enchantNames = {
                "minecraft:efficiency",
                "minecraft:aqua_affinity",
                "minecraft:depth_strider",
                "minecraft:swift_sneak",
                "minecraft:respiration",
                "minecraft:sweeping_edge",
                "minecraft:blast_protection",
                "minecraft:fire_protection",
                "minecraft:sharpness",
                "minecraft:unbreaking",
            };
            this.enchantPool = new ArrayList<>();
            for (String name : enchantNames) {
                this.enchantPool.add(enchantments.get(Identifier.parse(name)).orElseThrow());
            }

            this.effectPool = List.of(
                    MobEffects.SPEED,
                    MobEffects.SLOWNESS,
                    MobEffects.HASTE,
                    MobEffects.MINING_FATIGUE,
                    MobEffects.STRENGTH,
                    MobEffects.WEAKNESS,
                    MobEffects.JUMP_BOOST,
                    MobEffects.HEALTH_BOOST,
                    MobEffects.ABSORPTION,
                    MobEffects.LUCK,
                    MobEffects.UNLUCK,
                    MobEffects.WATER_BREATHING,
                    MobEffects.CONDUIT_POWER,
                    MobEffects.POISON
            );

            this.miningTools = List.of(
                    Items.AIR,
                    Items.STICK,
                    Items.WOODEN_PICKAXE,
                    Items.STONE_PICKAXE,
                    Items.IRON_PICKAXE,
                    Items.GOLDEN_PICKAXE,
                    Items.DIAMOND_PICKAXE,
                    Items.NETHERITE_PICKAXE,
                    Items.DIAMOND_AXE,
                    Items.DIAMOND_SHOVEL,
                    Items.DIAMOND_HOE,
                    Items.DIAMOND_SWORD,
                    Items.SHEARS
            );

            this.miningBlocks = List.of(
                    Blocks.STONE,
                    Blocks.DEEPSLATE,
                    Blocks.OBSIDIAN,
                    Blocks.OAK_LOG,
                    Blocks.DIRT,
                    Blocks.SAND,
                    Blocks.OAK_LEAVES,
                    Blocks.COBWEB,
                    Blocks.IRON_BLOCK,
                    Blocks.OAK_PLANKS,
                    Blocks.GLASS,
                    Blocks.GRASS_BLOCK
            );
        }

        void run() throws Exception {
            String line;
            while ((line = in.readLine()) != null) {
                if (line.isBlank()) {
                    continue;
                }
                JsonObject request = com.google.gson.JsonParser.parseString(line).getAsJsonObject();
                JsonObject response = new JsonObject();
                String command = request.get("command").getAsString();
                response.addProperty("command", command);
                try {
                    switch (command) {
                        case "attributes" -> handleAttributes(request, response);
                        case "mining-speed" -> handleMiningSpeed(request, response);
                        case "sync-packets" -> handleSyncPackets(request, response);
                        default -> response.addProperty("error", "unknown command: " + command);
                    }
                } catch (Exception error) {
                    error.printStackTrace(System.err);
                    response.addProperty("error", String.valueOf(error));
                }
                out.write(response.toString());
                out.write('\n');
                out.flush();
            }
        }

        private void handleAttributes(JsonObject request, JsonObject response) {
            int count = request.get("cases").getAsInt();
            long seed = request.get("seed").getAsLong();
            Random rng = new Random(seed);
            JsonArray cases = new JsonArray();

            AttributeModifier.Operation[] ops = AttributeModifier.Operation.values();
            double[] customAmounts = {-2.5, -0.5, -0.1, 0.0, 0.15, 0.25, 0.5, 1.0, 2.0, 4.5};

            for (int c = 0; c < count; c++) {
                int gameMode = rng.nextInt(4);
                boolean sprinting = rng.nextBoolean();
                boolean crouching = rng.nextBoolean();
                int[] frozenChoices = {0, 0, 1, 70, 140, 200};
                int ticksFrozen = frozenChoices[rng.nextInt(frozenChoices.length)];
                boolean onSolid = rng.nextBoolean();

                AttributeMap map = new AttributeMap(Player.createAttributes().build());
                JsonArray slotsJson = new JsonArray();

                for (int s = 0; s < SLOTS.length; s++) {
                    if (rng.nextInt(3) == 0) {
                        continue;
                    }
                    Item item = itemPool.get(rng.nextInt(itemPool.size()));
                    ItemStack stack = new ItemStack(item);
                    JsonObject slotObj = new JsonObject();
                    slotObj.addProperty("slot", s);
                    slotObj.addProperty("item", BuiltInRegistries.ITEM.getId(item));

                    JsonArray enchJson = new JsonArray();
                    if (rng.nextInt(3) != 0) {
                        ItemEnchantments.Mutable mutable = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
                        int enchCount = 1 + rng.nextInt(3);
                        Map<Integer, Integer> chosen = new HashMap<>();
                        for (int e = 0; e < enchCount; e++) {
                            Holder.Reference<Enchantment> holder = enchantPool.get(rng.nextInt(enchantPool.size()));
                            int level = 1 + rng.nextInt(5);
                            mutable.set(holder, level);
                            chosen.put(enchantments.getId(holder.value()), level);
                        }
                        stack.set(DataComponents.ENCHANTMENTS, mutable.toImmutable());
                        for (Map.Entry<Integer, Integer> entry : chosen.entrySet()) {
                            JsonArray pair = new JsonArray();
                            pair.add(entry.getKey());
                            pair.add(entry.getValue());
                            enchJson.add(pair);
                        }
                    }
                    slotObj.add("enchantments", enchJson);

                    if (rng.nextInt(3) == 0) {
                        ItemAttributeModifiers.Builder builder = ItemAttributeModifiers.builder();
                        JsonArray customJson = new JsonArray();
                        int modCount = 1 + rng.nextInt(3);
                        for (int m = 0; m < modCount; m++) {
                            Holder<Attribute> attr = playerAttributes.get(rng.nextInt(playerAttributes.size()));
                            Identifier modId = Identifier.parse("minecraft:oracle_custom_" + s + "_" + m);
                            double amount = customAmounts[rng.nextInt(customAmounts.length)];
                            AttributeModifier.Operation op = ops[rng.nextInt(ops.length)];
                            int groupIdx = rng.nextInt(SLOT_GROUPS.length);
                            builder.add(attr, new AttributeModifier(modId, amount, op), SLOT_GROUPS[groupIdx]);

                            JsonArray row = new JsonArray();
                            row.add(BuiltInRegistries.ATTRIBUTE.getId(attr.value()));
                            row.add(modId.toString());
                            row.add(Double.doubleToRawLongBits(amount));
                            row.add(op.ordinal());
                            row.add(groupIdx);
                            customJson.add(row);
                        }
                        stack.set(DataComponents.ATTRIBUTE_MODIFIERS, builder.build());
                        slotObj.add("customModifiers", customJson);
                    }

                    slotsJson.add(slotObj);
                    // 官方 LivingEntity.handleEquipmentChanges 调 stack.forEachModifier(slot, ...)
                    stack.forEachModifier(SLOTS[s], (holder, modifier) -> {
                        AttributeInstance inst = map.getInstance(holder);
                        if (inst != null) {
                            inst.removeModifier(modifier.id());
                            inst.addTransientModifier(modifier);
                        }
                    });
                }

                JsonArray effectsJson = new JsonArray();
                int effectCount = rng.nextInt(5);
                Map<Holder<MobEffect>, Integer> chosenEffects = new HashMap<>();
                for (int e = 0; e < effectCount; e++) {
                    Holder<MobEffect> effect = effectPool.get(rng.nextInt(effectPool.size()));
                    int amp = rng.nextInt(5);
                    chosenEffects.put(effect, amp);
                }
                for (Map.Entry<Holder<MobEffect>, Integer> entry : chosenEffects.entrySet()) {
                    entry.getKey().value().addAttributeModifiers(map, entry.getValue());
                    JsonArray pair = new JsonArray();
                    pair.add(BuiltInRegistries.MOB_EFFECT.getId(entry.getKey().value()));
                    pair.add(entry.getValue());
                    effectsJson.add(pair);
                }

                // 官方 ServerPlayer.updatePlayerAttributes / LivingEntity.setSprinting / tryAddFrost
                if (gameMode == 1) {
                    AttributeInstance blockReach = map.getInstance(Attributes.BLOCK_INTERACTION_RANGE);
                    if (blockReach != null) {
                        blockReach.addOrUpdateTransientModifier(CREATIVE_BLOCK_RANGE);
                    }
                    AttributeInstance entityReach = map.getInstance(Attributes.ENTITY_INTERACTION_RANGE);
                    if (entityReach != null) {
                        entityReach.addOrUpdateTransientModifier(CREATIVE_ENTITY_RANGE);
                    }
                }
                if (crouching) {
                    AttributeInstance waypoint = map.getInstance(Attributes.WAYPOINT_TRANSMIT_RANGE);
                    if (waypoint != null) {
                        waypoint.addOrUpdateTransientModifier(CROUCH_WAYPOINT);
                    }
                }
                if (sprinting) {
                    AttributeInstance speed = map.getInstance(Attributes.MOVEMENT_SPEED);
                    if (speed != null) {
                        speed.removeModifier(SPRINTING_SPEED.id());
                        speed.addTransientModifier(SPRINTING_SPEED);
                    }
                }
                if (onSolid && ticksFrozen > 0) {
                    AttributeInstance speed = map.getInstance(Attributes.MOVEMENT_SPEED);
                    if (speed != null) {
                        float percent = (float) Math.min(ticksFrozen, 140) / 140.0F;
                        float amount = -0.05F * percent;
                        speed.removeModifier(POWDER_SNOW_ID);
                        speed.addTransientModifier(new AttributeModifier(
                                POWDER_SNOW_ID, (double) amount, AttributeModifier.Operation.ADD_VALUE));
                    }
                }

                JsonObject row = new JsonObject();
                row.addProperty("gameMode", gameMode);
                row.addProperty("sprinting", sprinting);
                row.addProperty("crouching", crouching);
                row.addProperty("ticksFrozen", ticksFrozen);
                row.addProperty("onSolidForFrost", onSolid);
                row.add("slots", slotsJson);
                row.add("effects", effectsJson);
                row.add("expected", serializeAttributeMap(map));
                cases.add(row);
            }

            response.add("cases", cases);
        }

        private JsonArray serializeAttributeMap(AttributeMap map) {
            JsonArray out = new JsonArray();
            for (Holder<Attribute> holder : playerAttributes) {
                int id = BuiltInRegistries.ATTRIBUTE.getId(holder.value());
                double value = map.getValue(holder);
                AttributeInstance inst = map.getInstance(holder);
                List<AttributeModifier> mods = new ArrayList<>();
                if (inst != null) {
                    mods.addAll(inst.getModifiers());
                }
                mods.sort(Comparator.comparing(m -> m.id().toString()));
                JsonArray modsJson = new JsonArray();
                for (AttributeModifier m : mods) {
                    JsonArray entry = new JsonArray();
                    entry.add(m.id().toString());
                    entry.add(Double.doubleToRawLongBits(m.amount()));
                    entry.add(m.operation().ordinal());
                    modsJson.add(entry);
                }
                JsonArray row = new JsonArray();
                row.add(id);
                row.add(Double.doubleToRawLongBits(value));
                row.add(modsJson);
                out.add(row);
            }
            return out;
        }

        private void handleMiningSpeed(JsonObject request, JsonObject response) throws Exception {
            int count = request.get("cases").getAsInt();
            long seed = request.get("seed").getAsLong();
            Random rng = new Random(seed);

            Holder.Reference<Enchantment> efficiency =
                    enchantments.get(Identifier.parse("minecraft:efficiency")).orElseThrow();
            Holder.Reference<Enchantment> aquaAffinity =
                    enchantments.get(Identifier.parse("minecraft:aqua_affinity")).orElseThrow();

            // 构造一次真实 Player 实例，直接调用官方 Player.getDestroySpeed(BlockState)。
            Player fakePlayer = (Player) UNSAFE.allocateInstance(ServerPlayer.class);
            Inventory inventory = (Inventory) UNSAFE.allocateInstance(Inventory.class);
            NonNullList<ItemStack> items = NonNullList.withSize(36, ItemStack.EMPTY);
            F_INVENTORY_ITEMS.set(inventory, items);
            F_INVENTORY_SELECTED.setInt(inventory, 0);
            F_PLAYER_INVENTORY.set(fakePlayer, inventory);

            final boolean[] eyeInWaterBox = {false};
            EntityFluidInteraction fluidInteraction = new EntityFluidInteraction(Set.of()) {
                @Override
                public boolean isEyeInFluid(TagKey<Fluid> tag) {
                    return eyeInWaterBox[0] && tag == FluidTags.WATER;
                }
            };
            F_ENTITY_FLUID_INTERACTION.set(fakePlayer, fluidInteraction);

            JsonArray cases = new JsonArray();
            double[] breakSpeedBases = {1.0, 1.0, 1.0, 0.5, 1.5, 2.0, 0.0};

            for (int c = 0; c < count; c++) {
                Item tool = miningTools.get(rng.nextInt(miningTools.size()));
                Block block = miningBlocks.get(rng.nextInt(miningBlocks.size()));
                BlockState state = block.defaultBlockState();
                int effLevel = tool == Items.AIR ? 0 : rng.nextInt(6);
                boolean hasAqua = rng.nextBoolean();
                int hasteAmp = rng.nextInt(3) == 0 ? rng.nextInt(5) : -1;
                int conduitAmp = rng.nextInt(4) == 0 ? rng.nextInt(5) : -1;
                int fatigueAmp = rng.nextInt(3) == 0 ? rng.nextInt(5) : -1;
                boolean eyeInWater = rng.nextBoolean();
                boolean onGround = rng.nextBoolean();
                double breakSpeedBase = breakSpeedBases[rng.nextInt(breakSpeedBases.length)];

                ItemStack mainStack = tool == Items.AIR ? ItemStack.EMPTY : new ItemStack(tool);
                if (effLevel > 0) {
                    ItemEnchantments.Mutable m = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
                    m.set(efficiency, effLevel);
                    mainStack.set(DataComponents.ENCHANTMENTS, m.toImmutable());
                }
                ItemStack headStack = ItemStack.EMPTY;
                if (hasAqua) {
                    headStack = new ItemStack(Items.DIAMOND_HELMET);
                    ItemEnchantments.Mutable m = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
                    m.set(aquaAffinity, 1);
                    headStack.set(DataComponents.ENCHANTMENTS, m.toImmutable());
                }

                AttributeMap map = new AttributeMap(Player.createAttributes().build());
                map.getInstance(Attributes.BLOCK_BREAK_SPEED).setBaseValue(breakSpeedBase);
                mainStack.forEachModifier(EquipmentSlot.MAINHAND, (holder, mod) -> {
                    AttributeInstance inst = map.getInstance(holder);
                    if (inst != null) {
                        inst.removeModifier(mod.id());
                        inst.addTransientModifier(mod);
                    }
                });
                headStack.forEachModifier(EquipmentSlot.HEAD, (holder, mod) -> {
                    AttributeInstance inst = map.getInstance(holder);
                    if (inst != null) {
                        inst.removeModifier(mod.id());
                        inst.addTransientModifier(mod);
                    }
                });

                Map<Holder<MobEffect>, MobEffectInstance> active = new HashMap<>();
                if (hasteAmp >= 0) {
                    active.put(MobEffects.HASTE, new MobEffectInstance(MobEffects.HASTE, 200, hasteAmp));
                    MobEffects.HASTE.value().addAttributeModifiers(map, hasteAmp);
                }
                if (conduitAmp >= 0) {
                    active.put(MobEffects.CONDUIT_POWER,
                            new MobEffectInstance(MobEffects.CONDUIT_POWER, 200, conduitAmp));
                    MobEffects.CONDUIT_POWER.value().addAttributeModifiers(map, conduitAmp);
                }
                if (fatigueAmp >= 0) {
                    active.put(MobEffects.MINING_FATIGUE,
                            new MobEffectInstance(MobEffects.MINING_FATIGUE, 200, fatigueAmp));
                    MobEffects.MINING_FATIGUE.value().addAttributeModifiers(map, fatigueAmp);
                }

                items.set(0, mainStack);
                F_LIVING_ATTRIBUTES.set(fakePlayer, map);
                F_LIVING_ACTIVE_EFFECTS.set(fakePlayer, active);
                eyeInWaterBox[0] = eyeInWater;
                F_ENTITY_ON_GROUND.setBoolean(fakePlayer, onGround);

                float speed = fakePlayer.getDestroySpeed(state);

                JsonArray row = new JsonArray();
                row.add(BuiltInRegistries.ITEM.getId(tool));
                row.add(Block.getId(state));
                row.add(effLevel);
                row.add(hasAqua ? 1 : 0);
                row.add(hasteAmp);
                row.add(conduitAmp);
                row.add(fatigueAmp);
                row.add(eyeInWater ? 1 : 0);
                row.add(onGround ? 1 : 0);
                row.add(Double.doubleToRawLongBits(breakSpeedBase));
                row.add(Float.floatToRawIntBits(speed));
                cases.add(row);
            }

            response.add("cases", cases);
        }

        private void handleSyncPackets(JsonObject request, JsonObject response) {
            byte[] bambooFull = Base64.getDecoder().decode(request.get("full").getAsString());
            byte[] bambooSpeed = Base64.getDecoder().decode(request.get("speedDirty").getAsString());
            byte[] bambooComplex = Base64.getDecoder().decode(request.get("complexDirty").getAsString());

            // 1. 构造官方 AttributeMap（创造模式进服的真实 getSyncableAttributes()）。
            AttributeMap map = new AttributeMap(Player.createAttributes().build());
            map.getInstance(Attributes.BLOCK_INTERACTION_RANGE)
                    .addOrUpdateTransientModifier(CREATIVE_BLOCK_RANGE);
            map.getInstance(Attributes.ENTITY_INTERACTION_RANGE)
                    .addOrUpdateTransientModifier(CREATIVE_ENTITY_RANGE);
            ClientboundUpdateAttributesPacket vanillaFull =
                    new ClientboundUpdateAttributesPacket(42, map.getSyncableAttributes());
            map.getAttributesToSync().clear();

            // 2. 速度 II（amplifier=1）。
            MobEffects.SPEED.value().addAttributeModifiers(map, 1);
            ClientboundUpdateAttributesPacket vanillaSpeed =
                    new ClientboundUpdateAttributesPacket(42, map.getAttributesToSync());
            map.getAttributesToSync().clear();

            // 3. 效率 V 钻石镐 + 切回生存模式 + 疾跑。
            Holder.Reference<Enchantment> efficiency =
                    enchantments.get(Identifier.parse("minecraft:efficiency")).orElseThrow();
            ItemStack pickaxe = new ItemStack(Items.DIAMOND_PICKAXE);
            ItemEnchantments.Mutable m = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
            m.set(efficiency, 5);
            pickaxe.set(DataComponents.ENCHANTMENTS, m.toImmutable());
            pickaxe.forEachModifier(EquipmentSlot.MAINHAND, (holder, mod) -> {
                AttributeInstance inst = map.getInstance(holder);
                if (inst != null) {
                    inst.removeModifier(mod.id());
                    inst.addTransientModifier(mod);
                }
            });
            map.getInstance(Attributes.BLOCK_INTERACTION_RANGE)
                    .removeModifier(CREATIVE_BLOCK_RANGE);
            map.getInstance(Attributes.ENTITY_INTERACTION_RANGE)
                    .removeModifier(CREATIVE_ENTITY_RANGE);
            AttributeInstance speedInst = map.getInstance(Attributes.MOVEMENT_SPEED);
            speedInst.removeModifier(SPRINTING_SPEED.id());
            speedInst.addTransientModifier(SPRINTING_SPEED);
            ClientboundUpdateAttributesPacket vanillaComplex =
                    new ClientboundUpdateAttributesPacket(42, map.getAttributesToSync());

            response.add("full", comparePacket("full", bambooFull, vanillaFull));
            response.add("speedDirty", comparePacket("speedDirty", bambooSpeed, vanillaSpeed));
            response.add("complexDirty", comparePacket("complexDirty", bambooComplex, vanillaComplex));
        }

        private JsonObject comparePacket(
                String label, byte[] bambooBytes, ClientboundUpdateAttributesPacket vanillaPacket) {
            JsonObject result = new JsonObject();
            result.addProperty("label", label);
            JsonObject vanillaJson = packetToCanonicalJson(vanillaPacket);
            result.add("vanilla", vanillaJson);

            if (bambooBytes.length == 0) {
                result.addProperty("decodeError", "empty payload");
                result.addProperty("equal", false);
                return result;
            }
            RegistryFriendlyByteBuf buf = new RegistryFriendlyByteBuf(
                    Unpooled.wrappedBuffer(bambooBytes), registries);
            try {
                ClientboundUpdateAttributesPacket decoded =
                        ClientboundUpdateAttributesPacket.STREAM_CODEC.decode(buf);
                int trailing = buf.readableBytes();
                JsonObject bambooJson = packetToCanonicalJson(decoded);
                result.add("bamboo", bambooJson);
                result.addProperty("trailingBytes", trailing);
                result.addProperty("equal", trailing == 0 && bambooJson.equals(vanillaJson));
            } catch (Exception error) {
                result.addProperty("decodeError", String.valueOf(error));
                result.addProperty("equal", false);
            } finally {
                buf.release();
            }
            return result;
        }

        private JsonObject packetToCanonicalJson(ClientboundUpdateAttributesPacket packet) {
            // 官方 ClientboundUpdateAttributesPacket 的属性列表来自 HashMap，按属性 ID 排序后比较。
            JsonObject obj = new JsonObject();
            obj.addProperty("entityId", packet.getEntityId());
            List<ClientboundUpdateAttributesPacket.AttributeSnapshot> values =
                    new ArrayList<>(packet.getValues());
            values.sort(Comparator.comparingInt(s -> BuiltInRegistries.ATTRIBUTE.getId(s.attribute().value())));
            JsonArray arr = new JsonArray();
            for (ClientboundUpdateAttributesPacket.AttributeSnapshot snap : values) {
                JsonObject sObj = new JsonObject();
                sObj.addProperty("attribute", BuiltInRegistries.ATTRIBUTE.getId(snap.attribute().value()));
                sObj.addProperty("baseBits", Double.doubleToRawLongBits(snap.base()));
                List<AttributeModifier> mods = new ArrayList<>(snap.modifiers());
                mods.sort(Comparator.comparing(mod -> mod.id().toString()));
                JsonArray modsArr = new JsonArray();
                for (AttributeModifier mod : mods) {
                    JsonArray row = new JsonArray();
                    row.add(mod.id().toString());
                    row.add(Double.doubleToRawLongBits(mod.amount()));
                    row.add(mod.operation().ordinal());
                    modsArr.add(row);
                }
                sObj.add("modifiers", modsArr);
                arr.add(sObj);
            }
            obj.add("attributes", arr);
            return obj;
        }
    }
}
