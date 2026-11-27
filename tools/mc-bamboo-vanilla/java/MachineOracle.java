import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import io.netty.buffer.Unpooled;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.UUID;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.ForkJoinPool;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Predicate;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponentPatch;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.monster.zombie.Drowned;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.inventory.BeaconMenu;
import net.minecraft.world.inventory.ContainerData;
import net.minecraft.world.inventory.ContainerLevelAccess;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.alchemy.PotionContents;
import net.minecraft.world.item.alchemy.Potions;
import net.minecraft.world.item.crafting.CampfireCookingRecipe;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.item.crafting.RecipeType;
import net.minecraft.world.item.crafting.SingleRecipeInput;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.CampfireBlock;
import net.minecraft.world.level.block.entity.AbstractFurnaceBlockEntity;
import net.minecraft.world.level.block.entity.BeaconBeamOwner;
import net.minecraft.world.level.block.entity.BeaconBlockEntity;
import net.minecraft.world.level.block.entity.BlastFurnaceBlockEntity;
import net.minecraft.world.level.block.entity.BrewingStandBlockEntity;
import net.minecraft.world.level.block.entity.CampfireBlockEntity;
import net.minecraft.world.level.block.entity.ConduitBlockEntity;
import net.minecraft.world.level.block.entity.FurnaceBlockEntity;
import net.minecraft.world.level.block.entity.SmokerBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.level.storage.TagValueInput;
import net.minecraft.world.level.storage.ValueInput;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.util.ProblemReporter;
import net.minecraft.core.Holder;
import org.jspecify.annotations.Nullable;
import sun.misc.Unsafe;

/**
 * 熔炉和酿造台的方块实体在假世界里逐刻跑，记下进度、燃料、物品、方块状态和世界随机源。
 * 取出产物时调用官方 {@code getRecipesToAwardAndPopExperience}，经验球不留下，但随机数照抽。
 */
public final class MachineOracle {
    private static final BlockPos POS = BlockPos.ZERO;
    private static RecipeManager RECIPES;
    private static RegistryAccess REGISTRIES;
    private static ReloadableServerResources MANAGERS;
    private static FakeServer SERVER;
    private static final Unsafe UNSAFE = loadUnsafe();

    private MachineOracle() {
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
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        loadRecipes();

        JsonArray runs = new JsonArray();
        runs.add(furnace("furnace", new FurnaceBlockEntity(POS, Blocks.FURNACE.defaultBlockState()),
                Blocks.FURNACE.defaultBlockState(), 11L, 210, true,
                new ItemStack(Items.RAW_IRON, 8), new ItemStack(Items.COAL, 8), ItemStack.EMPTY));
        runs.add(furnace("blast_furnace", new BlastFurnaceBlockEntity(POS, Blocks.BLAST_FURNACE.defaultBlockState()),
                Blocks.BLAST_FURNACE.defaultBlockState(), 12L, 110, true,
                new ItemStack(Items.RAW_IRON, 4), new ItemStack(Items.COAL, 4), ItemStack.EMPTY));
        runs.add(furnace("smoker", new SmokerBlockEntity(POS, Blocks.SMOKER.defaultBlockState()),
                Blocks.SMOKER.defaultBlockState(), 13L, 110, true,
                new ItemStack(Items.POTATO, 4), new ItemStack(Items.COAL, 4), ItemStack.EMPTY));
        runs.add(furnace("lava_bucket", new FurnaceBlockEntity(POS, Blocks.FURNACE.defaultBlockState()),
                Blocks.FURNACE.defaultBlockState(), 14L, 210, true,
                new ItemStack(Items.RAW_IRON, 1), new ItemStack(Items.LAVA_BUCKET), ItemStack.EMPTY));
        runs.add(furnace("no_recipe", new FurnaceBlockEntity(POS, Blocks.FURNACE.defaultBlockState()),
                Blocks.FURNACE.defaultBlockState(), 15L, 4, true,
                new ItemStack(Items.STICK), new ItemStack(Items.COAL), ItemStack.EMPTY,
                new int[] {0, 0, 40, 200}));
        runs.add(furnace("cool_down", new FurnaceBlockEntity(POS, Blocks.FURNACE.defaultBlockState()),
                Blocks.FURNACE.defaultBlockState(), 16L, 6, true,
                ItemStack.EMPTY, ItemStack.EMPTY, ItemStack.EMPTY,
                new int[] {0, 0, 10, 200}));
        // 取出多种配方经验：floor 与小数部分按 `nextFloat` 进位，以及拆成多颗球。
        runs.add(furnaceXp("xp_fraction_low", 21L,
                new String[] {"minecraft:baked_potato", "minecraft:glass"}, new int[] {3, 2}));
        runs.add(furnaceXp("xp_fraction_high", 22L,
                new String[] {"minecraft:charcoal", "minecraft:cooked_beef"}, new int[] {7, 5}));
        runs.add(furnaceXp("xp_whole", 23L,
                new String[] {"minecraft:gold_ingot_from_smelting_gold_ore"}, new int[] {10}));
        runs.add(furnaceXp("xp_many_orb_values", 24L,
                new String[] {"minecraft:gold_ingot_from_smelting_gold_ore",
                        "minecraft:baked_potato", "minecraft:glass"}, new int[] {2500, 31, 17}));
        // 砂轮取出结果：`getExperienceAmount` 的一次 nextInt + `ExperienceOrb.award` 的拆分。
        runs.add(grindstoneXp("grindstone_xp_small", 25L, 1, 0));
        runs.add(grindstoneXp("grindstone_xp_big", 26L, 3, 2));
        runs.add(grindstoneXp("grindstone_xp_none", 27L, 0, 0));
        runs.add(grindstoneXp("grindstone_xp_curse", 28L, 2, 1));
        runs.add(brew(17L, 410));
        // 进度 598，第 2 刻烧完并弹出。熄灭从 5 往回减。木棍没有配方，到点把原物品弹出去。
        runs.add(campfire(18L, true, 4, Items.POTATO, 598, 600));
        runs.add(campfire(19L, false, 4, Items.POTATO, 5, 600));
        runs.add(campfire(20L, true, 2, Items.STICK, 0, 1));

        JsonArray beacons = new JsonArray();
        beacons.add(beaconTier1To4());
        beacons.add(beaconGlassAndBlock());
        beacons.add(beaconPaymentTier4Regen());
        beacons.add(beaconPaymentTier4Level2AndDeactivate());
        beacons.add(beaconPaymentTier2RangeAndDuration());
        beacons.add(beaconSaveRoundtrip());

        JsonArray conduits = new JsonArray();
        conduits.add(conduitFrame16());
        conduits.add(conduitFrame42());
        conduits.add(conduitDryDeactivate());
        conduits.add(conduitPlayerEffects());
        conduits.add(conduitDrownedAttack());
        conduits.add(conduitSaveRoundtrip());

        JsonObject root = new JsonObject();
        root.add("runs", runs);
        root.add("beacons", beacons);
        root.add("conduits", conduits);
        try (Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8))) {
            out.write(root.toString());
            out.write('\n');
        }
    }

    private static JsonObject furnace(String kind, AbstractFurnaceBlockEntity entity, BlockState state, long seed,
            int ticks, boolean take, ItemStack input, ItemStack fuel, ItemStack result) throws Exception {
        return furnace(kind, entity, state, seed, ticks, take, input, fuel, result, new int[] {0, 0, 0, 0});
    }

    private static JsonObject furnace(String kind, AbstractFurnaceBlockEntity entity, BlockState state, long seed,
            int ticks, boolean take, ItemStack input, ItemStack fuel, ItemStack result, int[] data)
            throws Exception {
        MachineLevel level = MachineLevel.create(seed);
        level.state = state;
        entity.setLevel(level);
        entity.setItem(0, input.copy());
        entity.setItem(1, fuel.copy());
        entity.setItem(2, result.copy());
        ContainerData access = dataOf(entity);
        for (int id = 0; id < data.length; id++) {
            access.set(id, data[id]);
        }
        JsonObject run = header(kind, seed, state, slotsOf(entity), dataOfValues(access));
        JsonArray frames = new JsonArray();
        for (int tick = 0; tick < ticks; tick++) {
            AbstractFurnaceBlockEntity.serverTick(level, POS, level.getBlockState(POS), entity);
            frames.add(frame(level, slotsOf(entity), dataOfValues(access)));
        }
        run.add("ticks", frames);
        if (take) {
            run.addProperty("beforeTake", seedOf(level.random));
            level.fresh.clear();
            entity.getRecipesToAwardAndPopExperience(level, Vec3.ZERO);
            run.addProperty("afterTake", seedOf(level.random));
            run.add("afterItems", slotsOf(entity));
            run.add("afterOrbs", orbArray(level.fresh));
        }
        return run;
    }

    /** 把 `addFreshEntity` 收下的经验球写成 `[value, count, x, y, z]`（坐标是原始位）。 */
    private static JsonArray orbArray(List<Entity> entities) {
        JsonArray out = new JsonArray();
        for (Entity entity : entities) {
            if (!(entity instanceof net.minecraft.world.entity.ExperienceOrb orb)) {
                continue;
            }
            JsonArray row = new JsonArray();
            row.add(orb.getValue());
            try {
                Field count = net.minecraft.world.entity.ExperienceOrb.class.getDeclaredField("count");
                count.setAccessible(true);
                row.add(count.getInt(orb));
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
            row.add(Double.doubleToRawLongBits(orb.position().x));
            row.add(Double.doubleToRawLongBits(orb.position().y));
            row.add(Double.doubleToRawLongBits(orb.position().z));
            out.add(row);
        }
        return out;
    }

    /**
     * 一条一条地往 `recipesUsed` 里塞配方再取经验：官方的
     * `Reference2IntOpenHashMap` 迭代顺序由对象哈希决定，不可复现；分成多次调用后
     * 两边都按题目给的顺序取数，`createExperience` 的 floor / 小数进位和拆球都能逐项比。
     *
     * Seed one recipe at a time: vanilla's `Reference2IntOpenHashMap` iterates
     * in identity-hash order, which is not reproducible. Splitting into one
     * call per recipe puts both sides in the case's order.
     */
    private static JsonObject furnaceXp(String kind, long seed, String[] recipes, int[] counts)
            throws Exception {
        MachineLevel level = MachineLevel.create(seed);
        BlockState state = Blocks.FURNACE.defaultBlockState();
        level.state = state;
        AbstractFurnaceBlockEntity entity = new FurnaceBlockEntity(POS, state);
        entity.setLevel(level);
        Field used = AbstractFurnaceBlockEntity.class.getDeclaredField("recipesUsed");
        used.setAccessible(true);
        JsonObject run = header(kind, seed, state, slotsOf(entity), dataOfValues(dataOf(entity)));
        run.add("ticks", new JsonArray());
        run.addProperty("beforeTake", seedOf(level.random));
        level.fresh.clear();
        JsonArray listed = new JsonArray();
        for (int i = 0; i < recipes.length; i++) {
            final String recipeName = recipes[i];
            var key = net.minecraft.resources.ResourceKey.create(
                    net.minecraft.core.registries.Registries.RECIPE,
                    net.minecraft.resources.Identifier.parse(recipeName));
            var holder = RECIPES.byKey(key).orElseThrow(() -> new IllegalStateException(recipeName));
            ((it.unimi.dsi.fastutil.objects.Reference2IntOpenHashMap<?>) used.get(entity)).clear();
            for (int n = 0; n < counts[i]; n++) {
                entity.setRecipeUsed(holder);
            }
            entity.getRecipesToAwardAndPopExperience(level, Vec3.ZERO);
            JsonArray row = new JsonArray();
            row.add(recipeName);
            row.add(counts[i]);
            listed.add(row);
        }
        run.addProperty("afterTake", seedOf(level.random));
        run.add("afterItems", slotsOf(entity));
        run.add("afterOrbs", orbArray(level.fresh));
        run.add("recipes", listed);
        return run;
    }

    /**
     * 砂轮取出结果的经验：把两件带附魔的物品放进 `repairSlots`，调用官方结果槽的
     * `getExperienceAmount(level)`，再按官方 `onTake` 调 `ExperienceOrb.award`。
     *
     * Grindstone take XP: put enchanted items into `repairSlots`, call the
     * official result slot's `getExperienceAmount(level)`, then
     * `ExperienceOrb.award` as `onTake` does.
     */
    private static JsonObject grindstoneXp(String kind, long seed, int sharpness, int mending)
            throws Exception {
        MachineLevel level = MachineLevel.create(seed);
        net.minecraft.world.entity.player.Inventory inventory =
                new net.minecraft.world.entity.player.Inventory(null, null);
        var menu = new net.minecraft.world.inventory.GrindstoneMenu(1, inventory);
        net.minecraft.world.item.enchantment.ItemEnchantments.Mutable enchants =
                new net.minecraft.world.item.enchantment.ItemEnchantments.Mutable(
                        net.minecraft.world.item.enchantment.ItemEnchantments.EMPTY);
        if (sharpness > 0) {
            enchants.set(REGISTRIES.lookupOrThrow(net.minecraft.core.registries.Registries.ENCHANTMENT)
                    .getOrThrow(net.minecraft.world.item.enchantment.Enchantments.SHARPNESS), sharpness);
        }
        if (mending > 0) {
            enchants.set(REGISTRIES.lookupOrThrow(net.minecraft.core.registries.Registries.ENCHANTMENT)
                    .getOrThrow(net.minecraft.world.item.enchantment.Enchantments.MENDING), mending);
        }
        ItemStack first = new ItemStack(Items.DIAMOND_SWORD);
        first.set(net.minecraft.core.component.DataComponents.ENCHANTMENTS, enchants.toImmutable());
        menu.getSlot(0).set(first);
        menu.getSlot(1).set(new ItemStack(Items.DIAMOND_SWORD));
        JsonObject run = header(kind, seed, Blocks.GRINDSTONE.defaultBlockState(),
                new JsonArray(), new JsonArray());
        run.add("ticks", new JsonArray());
        // 必须在调用 `getExperienceAmount` 之前记种子：那一次 `nextInt` 会取数。
        // Record the seed before `getExperienceAmount`; its `nextInt` draws.
        run.addProperty("beforeTake", seedOf(level.random));
        var slot = menu.getSlot(2);
        var getFromItem = slot.getClass().getDeclaredMethod("getExperienceFromItem", ItemStack.class);
        getFromItem.setAccessible(true);
        int sum = 0;
        for (int i = 0; i < 2; i++) {
            sum += (Integer) getFromItem.invoke(slot, menu.getSlot(i).getItem());
        }
        int reward = 0;
        if (sum > 0) {
            var getAmount = slot.getClass().getDeclaredMethod("getExperienceAmount",
                    net.minecraft.world.level.Level.class);
            getAmount.setAccessible(true);
            reward = (Integer) getAmount.invoke(slot, level);
        }

        run.addProperty("xpSumInput", sum);
        run.addProperty("xpReward", reward);
        level.fresh.clear();
        net.minecraft.world.entity.ExperienceOrb.award(level, Vec3.atCenterOf(POS), reward);
        run.addProperty("afterTake", seedOf(level.random));
        run.add("afterOrbs", orbArray(level.fresh));
        return run;
    }

    private static JsonObject brew(long seed, int ticks) throws Exception {
        MachineLevel level = MachineLevel.create(seed);
        BlockState state = Blocks.BREWING_STAND.defaultBlockState();
        level.state = state;
        BrewingStandBlockEntity entity = new BrewingStandBlockEntity(POS, state);
        entity.setLevel(level);
        entity.setItem(0, PotionContents.createItemStack(Items.POTION, Potions.WATER));
        entity.setItem(3, new ItemStack(Items.NETHER_WART));
        entity.setItem(4, new ItemStack(Items.BLAZE_POWDER));
        ContainerData access = dataOf(entity);
        JsonObject run = header("brewing", seed, state, slotsOf(entity), dataOfValues(access));
        JsonArray frames = new JsonArray();
        for (int tick = 0; tick < ticks; tick++) {
            BrewingStandBlockEntity.serverTick(level, POS, level.getBlockState(POS), entity);
            frames.add(frame(level, slotsOf(entity), dataOfValues(access)));
        }
        run.add("ticks", frames);
        return run;
    }

    private static JsonObject campfire(long seed, boolean lit, int ticks, Item item, int progress, int time)
            throws Exception {
        BlockState state = Blocks.CAMPFIRE.defaultBlockState();
        if (!lit) {
            state = state.setValue(CampfireBlock.LIT, false);
        }
        MachineLevel level = MachineLevel.create(seed);
        level.state = state;
        CampfireBlockEntity entity = new CampfireBlockEntity(POS, state);
        entity.setLevel(level);
        entity.getItems().set(0, new ItemStack(item));
        int[] cookingProgress = ints(entity, "cookingProgress");
        int[] cookingTime = ints(entity, "cookingTime");
        cookingProgress[0] = progress;
        cookingTime[0] = time;
        JsonObject run = header("campfire", seed, state, campfireSlots(entity), campfireData(cookingProgress, cookingTime));
        JsonArray frames = new JsonArray();
        RecipeManager.CachedCheck<SingleRecipeInput, CampfireCookingRecipe> check =
                RecipeManager.createCheck(RecipeType.CAMPFIRE_COOKING);
        for (int tick = 0; tick < ticks; tick++) {
            if (level.state.getValue(CampfireBlock.LIT)) {
                CampfireBlockEntity.cookTick(level, POS, level.state, entity, check);
            } else {
                CampfireBlockEntity.cooldownTick(level, POS, level.state, entity);
            }
            frames.add(frame(level, campfireSlots(entity), campfireData(cookingProgress, cookingTime)));
        }
        run.add("ticks", frames);
        return run;
    }

    private static final BlockPos BEACON_POS = new BlockPos(0, 64, 0);

    private static void buildPyramid(MachineLevel level, BlockPos pos, int layers, BlockState block) {
        for (int layer = 1; layer <= layers; layer++) {
            int cy = pos.getY() - layer;
            for (int cx = pos.getX() - layer; cx <= pos.getX() + layer; cx++) {
                for (int cz = pos.getZ() - layer; cz <= pos.getZ() + layer; cz++) {
                    level.putBlock(new BlockPos(cx, cy, cz), block);
                }
            }
        }
    }

    @SuppressWarnings("unchecked")
    private static List<BeaconBeamOwner.Section> rawBeamSectionsOf(BeaconBlockEntity entity) throws Exception {
        Field field = BeaconBlockEntity.class.getDeclaredField("beamSections");
        field.setAccessible(true);
        return (List<BeaconBeamOwner.Section>) field.get(entity);
    }

    private static JsonArray sectionsJson(List<BeaconBeamOwner.Section> sections) {
        JsonArray out = new JsonArray();
        for (BeaconBeamOwner.Section section : sections) {
            JsonArray pair = new JsonArray();
            pair.add(section.getColor());
            pair.add(section.getHeight());
            out.add(pair);
        }
        return out;
    }

    private static JsonObject beaconFrame(MachineLevel level, BeaconBlockEntity entity) throws Exception {
        JsonObject frame = new JsonObject();
        frame.addProperty("gameTime", level.gameTime);
        frame.add("data", dataOfValues(dataOf(entity)));
        frame.add("beamSections", sectionsJson(entity.getBeamSections()));
        frame.add("rawBeamSections", sectionsJson(rawBeamSectionsOf(entity)));
        JsonArray sounds = new JsonArray();
        for (String s : level.sounds) {
            sounds.add(s);
        }
        level.sounds.clear();
        frame.add("sounds", sounds);
        JsonArray effects = new JsonArray();
        for (int[] row : level.receivedEffects) {
            JsonArray item = new JsonArray();
            for (int v : row) {
                item.add(v);
            }
            effects.add(item);
        }
        level.receivedEffects.clear();
        frame.add("effects", effects);
        return frame;
    }

    private static JsonObject paymentCheck(
            MachineLevel level,
            BeaconBlockEntity entity,
            ItemStack payment,
            Optional<Holder<MobEffect>> primary,
            Optional<Holder<MobEffect>> secondary) throws Exception {
        BeaconMenu menu = new BeaconMenu(
                1,
                new SimpleContainer(36),
                dataOf(entity),
                ContainerLevelAccess.create(level, BEACON_POS));
        if (!payment.isEmpty()) {
            menu.getSlot(0).set(payment.copy());
        }
        level.sounds.clear();
        boolean ok = menu.updateEffects(primary, secondary);
        JsonObject row = new JsonObject();
        row.addProperty("hasPayment", !payment.isEmpty());
        row.addProperty("primary", primary.map(h -> BuiltInRegistries.MOB_EFFECT.asHolderIdMap().getId(h)).orElse(-1));
        row.addProperty("secondary", secondary.map(h -> BuiltInRegistries.MOB_EFFECT.asHolderIdMap().getId(h)).orElse(-1));
        row.addProperty("ok", ok);
        row.addProperty("remainingPayment", menu.getSlot(0).getItem().getCount());
        row.add("data", dataOfValues(dataOf(entity)));
        JsonArray sounds = new JsonArray();
        for (String s : level.sounds) {
            sounds.add(s);
        }
        level.sounds.clear();
        row.add("sounds", sounds);
        return row;
    }

    private static JsonObject beaconTier1To4() throws Exception {
        MachineLevel level = MachineLevel.createMultiBlock(31L);
        level.putBlock(BEACON_POS, Blocks.BEACON.defaultBlockState());
        BeaconBlockEntity entity = new BeaconBlockEntity(BEACON_POS, Blocks.BEACON.defaultBlockState());
        entity.setLevel(level);
        JsonArray ticks = new JsonArray();
        for (int t = 0; t <= 325; t++) {
            if (t == 1) {
                buildPyramid(level, BEACON_POS, 1, Blocks.IRON_BLOCK.defaultBlockState());
            } else if (t == 85) {
                buildPyramid(level, BEACON_POS, 2, Blocks.GOLD_BLOCK.defaultBlockState());
            } else if (t == 165) {
                buildPyramid(level, BEACON_POS, 3, Blocks.DIAMOND_BLOCK.defaultBlockState());
            } else if (t == 245) {
                buildPyramid(level, BEACON_POS, 4, Blocks.NETHERITE_BLOCK.defaultBlockState());
            }
            level.gameTime = t;
            BeaconBlockEntity.tick(level, BEACON_POS, Blocks.BEACON.defaultBlockState(), entity);
            ticks.add(beaconFrame(level, entity));
        }
        JsonObject out = new JsonObject();
        out.addProperty("kind", "beacon_tier1_to_4");
        out.add("ticks", ticks);
        return out;
    }

    private static BlockState blockByName(String name) {
        return BuiltInRegistries.BLOCK
                .getValue(net.minecraft.resources.Identifier.parse(name))
                .defaultBlockState();
    }

    private static JsonObject beaconGlassAndBlock() throws Exception {
        MachineLevel level = MachineLevel.createMultiBlock(32L);
        level.putBlock(BEACON_POS, Blocks.BEACON.defaultBlockState());
        buildPyramid(level, BEACON_POS, 4, Blocks.IRON_BLOCK.defaultBlockState());
        level.putBlock(new BlockPos(0, 65, 0), blockByName("minecraft:red_stained_glass"));
        level.putBlock(new BlockPos(0, 66, 0), blockByName("minecraft:red_stained_glass_pane"));
        level.putBlock(new BlockPos(0, 67, 0), blockByName("minecraft:blue_stained_glass"));
        level.putBlock(new BlockPos(0, 68, 0), Blocks.BEDROCK.defaultBlockState());
        level.putBlock(new BlockPos(0, 78, 0), blockByName("minecraft:green_stained_glass"));
        BeaconBlockEntity entity = new BeaconBlockEntity(BEACON_POS, Blocks.BEACON.defaultBlockState());
        entity.setLevel(level);
        JsonArray ticks = new JsonArray();
        for (int t = 0; t <= 245; t++) {
            if (t == 86) {
                level.putBlock(new BlockPos(0, 70, 0), Blocks.STONE.defaultBlockState());
            } else if (t == 166) {
                level.putBlock(new BlockPos(0, 70, 0), Blocks.AIR.defaultBlockState());
            }
            level.gameTime = t;
            BeaconBlockEntity.tick(level, BEACON_POS, Blocks.BEACON.defaultBlockState(), entity);
            ticks.add(beaconFrame(level, entity));
        }
        JsonObject out = new JsonObject();
        out.addProperty("kind", "beacon_glass_and_block");
        out.add("ticks", ticks);
        return out;
    }

    private static JsonObject beaconPaymentTier4Regen() throws Exception {
        MachineLevel level = MachineLevel.createMultiBlock(33L);
        level.putBlock(BEACON_POS, Blocks.BEACON.defaultBlockState());
        buildPyramid(level, BEACON_POS, 4, Blocks.IRON_BLOCK.defaultBlockState());
        level.fakePlayers.add(FakeBeaconPlayer.create(level, 1, 5.0, 64.0, 0.0, false));
        level.fakePlayers.add(FakeBeaconPlayer.create(level, 2, 50.0, 200.0, 0.0, false));
        level.fakePlayers.add(FakeBeaconPlayer.create(level, 3, 52.0, 64.0, 0.0, false));
        level.fakePlayers.add(FakeBeaconPlayer.create(level, 4, 2.0, 64.0, 0.0, true));
        BeaconBlockEntity entity = new BeaconBlockEntity(BEACON_POS, Blocks.BEACON.defaultBlockState());
        entity.setLevel(level);
        JsonArray ticks = new JsonArray();
        JsonArray payments = new JsonArray();
        for (int t = 0; t <= 165; t++) {
            level.gameTime = t;
            BeaconBlockEntity.tick(level, BEACON_POS, Blocks.BEACON.defaultBlockState(), entity);
            ticks.add(beaconFrame(level, entity));
            if (t == 81) {
                // 4 层已激活：先测无付款、非法主效果 REGENERATION、非法不同 1-3 层副效果，最后合法付款。
                payments.add(paymentCheck(level, entity, ItemStack.EMPTY,
                        Optional.of(MobEffects.SPEED), Optional.empty()));
                payments.add(paymentCheck(level, entity, new ItemStack(Items.EMERALD, 1),
                        Optional.of(MobEffects.REGENERATION), Optional.empty()));
                payments.add(paymentCheck(level, entity, new ItemStack(Items.EMERALD, 1),
                        Optional.of(MobEffects.SPEED), Optional.of(MobEffects.HASTE)));
                payments.add(paymentCheck(level, entity, new ItemStack(Items.EMERALD, 1),
                        Optional.of(MobEffects.SPEED), Optional.of(MobEffects.REGENERATION)));
            }
        }
        JsonObject out = new JsonObject();
        out.addProperty("kind", "beacon_payment_tier4_regen");
        out.add("payments", payments);
        out.add("ticks", ticks);
        return out;
    }

    private static JsonObject beaconPaymentTier4Level2AndDeactivate() throws Exception {
        MachineLevel level = MachineLevel.createMultiBlock(34L);
        level.putBlock(BEACON_POS, Blocks.BEACON.defaultBlockState());
        buildPyramid(level, BEACON_POS, 4, Blocks.EMERALD_BLOCK.defaultBlockState());
        level.fakePlayers.add(FakeBeaconPlayer.create(level, 1, 10.0, 64.0, 0.0, false));
        BeaconBlockEntity entity = new BeaconBlockEntity(BEACON_POS, Blocks.BEACON.defaultBlockState());
        entity.setLevel(level);
        JsonArray ticks = new JsonArray();
        JsonArray payments = new JsonArray();
        for (int t = 0; t <= 245; t++) {
            if (t == 165) {
                level.putBlock(new BlockPos(0, 63, 0), Blocks.AIR.defaultBlockState());
            }
            level.gameTime = t;
            BeaconBlockEntity.tick(level, BEACON_POS, Blocks.BEACON.defaultBlockState(), entity);
            ticks.add(beaconFrame(level, entity));
            if (t == 81) {
                payments.add(paymentCheck(level, entity, new ItemStack(Items.DIAMOND, 1),
                        Optional.of(MobEffects.HASTE), Optional.of(MobEffects.HASTE)));
            }
        }
        JsonObject out = new JsonObject();
        out.addProperty("kind", "beacon_payment_tier4_level2_and_deactivate");
        out.add("payments", payments);
        out.add("ticks", ticks);
        return out;
    }

    private static JsonObject beaconPaymentTier2RangeAndDuration() throws Exception {
        MachineLevel level = MachineLevel.createMultiBlock(35L);
        level.putBlock(BEACON_POS, Blocks.BEACON.defaultBlockState());
        buildPyramid(level, BEACON_POS, 2, Blocks.IRON_BLOCK.defaultBlockState());
        level.fakePlayers.add(FakeBeaconPlayer.create(level, 1, 30.0, 64.0, 0.0, false));
        level.fakePlayers.add(FakeBeaconPlayer.create(level, 2, 32.0, 64.0, 0.0, false));
        BeaconBlockEntity entity = new BeaconBlockEntity(BEACON_POS, Blocks.BEACON.defaultBlockState());
        entity.setLevel(level);
        JsonArray ticks = new JsonArray();
        JsonArray payments = new JsonArray();
        for (int t = 0; t <= 165; t++) {
            level.gameTime = t;
            BeaconBlockEntity.tick(level, BEACON_POS, Blocks.BEACON.defaultBlockState(), entity);
            ticks.add(beaconFrame(level, entity));
            if (t == 81) {
                payments.add(paymentCheck(level, entity, new ItemStack(Items.IRON_INGOT, 1),
                        Optional.of(MobEffects.STRENGTH), Optional.empty()));
                payments.add(paymentCheck(level, entity, new ItemStack(Items.IRON_INGOT, 1),
                        Optional.of(MobEffects.RESISTANCE), Optional.of(MobEffects.RESISTANCE)));
                payments.add(paymentCheck(level, entity, new ItemStack(Items.IRON_INGOT, 1),
                        Optional.of(MobEffects.RESISTANCE), Optional.empty()));
            }
        }
        JsonObject out = new JsonObject();
        out.addProperty("kind", "beacon_payment_tier2_range_and_duration");
        out.add("payments", payments);
        out.add("ticks", ticks);
        return out;
    }

    private static JsonObject beaconSaveFields(CompoundTag tag) {
        JsonObject obj = new JsonObject();
        tag.getString("primary_effect").ifPresent(v -> obj.addProperty("primary_effect", v));
        tag.getString("secondary_effect").ifPresent(v -> obj.addProperty("secondary_effect", v));
        tag.getInt("Levels").ifPresent(v -> obj.addProperty("Levels", v));
        return obj;
    }

    private static JsonObject beaconSaveRoundtrip() throws Exception {
        MachineLevel level = MachineLevel.createMultiBlock(36L);
        BeaconBlockEntity valid = new BeaconBlockEntity(BEACON_POS, Blocks.BEACON.defaultBlockState());
        valid.setLevel(level);
        ContainerData access = dataOf(valid);
        access.set(0, 4);
        access.set(1, BeaconMenu.encodeEffect(MobEffects.SPEED));
        access.set(2, BeaconMenu.encodeEffect(MobEffects.REGENERATION));
        CompoundTag savedValid = valid.saveCustomOnly(REGISTRIES);

        BeaconBlockEntity reloaded = new BeaconBlockEntity(BEACON_POS, Blocks.BEACON.defaultBlockState());
        reloaded.setLevel(level);
        ValueInput inValid = TagValueInput.create(ProblemReporter.DISCARDING, REGISTRIES, savedValid);
        reloaded.loadCustomOnly(inValid);
        CompoundTag resavedValid = reloaded.saveCustomOnly(REGISTRIES);

        // 非法效果过滤 + Levels 不读盘验证。
        CompoundTag invalidRaw = new CompoundTag();
        invalidRaw.putString("primary_effect", "minecraft:poison");
        invalidRaw.putString("secondary_effect", "minecraft:haste");
        invalidRaw.putInt("Levels", 4);
        BeaconBlockEntity filtered = new BeaconBlockEntity(BEACON_POS, Blocks.BEACON.defaultBlockState());
        filtered.setLevel(level);
        filtered.loadCustomOnly(TagValueInput.create(ProblemReporter.DISCARDING, REGISTRIES, invalidRaw));
        CompoundTag savedFiltered = filtered.saveCustomOnly(REGISTRIES);

        JsonObject out = new JsonObject();
        out.addProperty("kind", "beacon_save_roundtrip");
        out.add("savedValid", beaconSaveFields(savedValid));
        out.add("resavedValid", beaconSaveFields(resavedValid));
        out.add("savedFiltered", beaconSaveFields(savedFiltered));
        out.add("ticks", new JsonArray());
        return out;
    }

    private static final BlockPos CONDUIT_POS = new BlockPos(0, 64, 0);
    /** `ConduitBlockEntity.updateShape` 的 42 格坐标表，按官方遍历序。 */
    private static final int[][] CONDUIT_FRAME = conduitFrameOffsets();

    private static int[][] conduitFrameOffsets() {
        int[][] out = new int[42][];
        int index = 0;
        for (int i = -2; i <= 2; i++) {
            for (int j = -2; j <= 2; j++) {
                for (int k = -2; k <= 2; k++) {
                    int ax = Math.abs(i), ay = Math.abs(j), az = Math.abs(k);
                    if (ax <= 1 && ay <= 1 && az <= 1) {
                        continue;
                    }
                    boolean plane = (i == 0 && (ay == 2 || az == 2))
                            || (j == 0 && (ax == 2 || az == 2))
                            || (k == 0 && (ax == 2 || ay == 2));
                    if (plane) {
                        out[index++] = new int[] {i, j, k};
                    }
                }
            }
        }
        if (index != 42) {
            throw new IllegalStateException("conduit frame has " + index + " cells");
        }
        return out;
    }

    /** 假世界铺满水，并把潮涌核心所在格换成含水的 conduit。 */
    private static void conduitWater(MachineLevel level) {
        for (int i = -2; i <= 2; i++) {
            for (int j = -2; j <= 2; j++) {
                for (int k = -2; k <= 2; k++) {
                    level.putBlock(CONDUIT_POS.offset(i, j, k), Blocks.WATER.defaultBlockState());
                }
            }
        }
        level.putBlock(CONDUIT_POS, Blocks.CONDUIT.defaultBlockState());
    }

    private static void conduitFrame(MachineLevel level, int count, BlockState block) {
        for (int n = 0; n < count; n++) {
            int[] offset = CONDUIT_FRAME[n];
            level.putBlock(CONDUIT_POS.offset(offset[0], offset[1], offset[2]), block);
        }
    }

    /** 官方 `ConduitBlockEntity.serverTick` 跑一刻，记下反射出来的状态。 */
    private static JsonObject conduitFrame(MachineLevel level, ConduitBlockEntity entity) throws Exception {
        JsonObject frame = new JsonObject();
        frame.addProperty("gameTime", level.gameTime);
        java.lang.reflect.Field active = ConduitBlockEntity.class.getDeclaredField("isActive");
        active.setAccessible(true);
        java.lang.reflect.Field hunting = ConduitBlockEntity.class.getDeclaredField("isHunting");
        hunting.setAccessible(true);
        java.lang.reflect.Field effectBlocks = ConduitBlockEntity.class.getDeclaredField("effectBlocks");
        effectBlocks.setAccessible(true);
        java.lang.reflect.Field target = ConduitBlockEntity.class.getDeclaredField("destroyTarget");
        target.setAccessible(true);
        java.lang.reflect.Field tickCount = ConduitBlockEntity.class.getDeclaredField("tickCount");
        tickCount.setAccessible(true);
        java.lang.reflect.Field ambient = ConduitBlockEntity.class.getDeclaredField("nextAmbientSoundActivation");
        ambient.setAccessible(true);
        frame.addProperty("active", active.getBoolean(entity));
        frame.addProperty("hunting", hunting.getBoolean(entity));
        @SuppressWarnings("unchecked")
        List<BlockPos> blocks = (List<BlockPos>) effectBlocks.get(entity);
        frame.addProperty("blocks", blocks.size());
        Object reference = target.get(entity);
        frame.addProperty("target", reference == null ? "" : uuidString(reference));
        frame.addProperty("tickCount", tickCount.getInt(entity));
        frame.addProperty("nextAmbient", ambient.getLong(entity));
        JsonArray sounds = new JsonArray();
        for (String s : level.sounds) {
            sounds.add(s);
        }
        level.sounds.clear();
        frame.add("sounds", sounds);
        JsonArray effects = new JsonArray();
        for (int[] row : level.receivedEffects) {
            JsonArray item = new JsonArray();
            for (int v : row) {
                item.add(v);
            }
            effects.add(item);
        }
        level.receivedEffects.clear();
        frame.add("effects", effects);
        frame.addProperty("seed", seedOf(level.random));
        // 目标生物受伤后的血量：只统计真正有 `entityData` 的溺尸。
        JsonArray health = new JsonArray();
        for (Entity e : level.fakeEntities) {
            if (e instanceof FakeConduitDrowned drowned) {
                health.add(drowned.getHealth());
            }
        }
        frame.add("health", health);
        JsonArray hurts = new JsonArray();
        for (String hurt : level.hurts) {
            hurts.add(hurt);
        }
        level.hurts.clear();
        frame.add("hurts", hurts);
        frame.add("blockUpdates", level.blockUpdates);
        level.blockUpdates = new JsonArray();
        return frame;
    }

    /** `EntityReference` 只有 `getUUID()` 是公开的，用反射读出来。 */
    private static String uuidString(Object reference) throws Exception {
        return reference.getClass().getMethod("getUUID").invoke(reference).toString();
    }

    private static ConduitBlockEntity freshConduit(MachineLevel level) {
        ConduitBlockEntity entity = new ConduitBlockEntity(CONDUIT_POS, Blocks.CONDUIT.defaultBlockState());
        entity.setLevel(level);
        return entity;
    }

    private static JsonObject conduitRun(String kind, long seed, int ticks, int frameCount,
            String playerMode, String drownedMode) throws Exception {
        MachineLevel level = MachineLevel.createMultiBlock(seed);
        conduitWater(level);
        conduitFrame(level, frameCount, Blocks.PRISMARINE.defaultBlockState());
        if (playerMode != null) {
            // 玩家 1 在半径内且身体在水里；玩家 2 是范围外的对照组。
            FakeConduitPlayer near = FakeConduitPlayer.create(level, 1, 3.5, 64.0, 0.0, true);
            // 半径 `42/7*16 = 96`，所以 x=140 才是范围外的对照组。
            FakeConduitPlayer far = FakeConduitPlayer.create(level, 2, 140.0, 64.0, 0.0, false);
            level.fakeEntities.add(near);
            level.fakeEntities.add(far);
            if ("dry".equals(playerMode)) {
                near.inWater = false;
            }
            if ("dry_far".equals(playerMode)) {
                near.inWater = false;
                far.inWater = false;
            }
        }
        if (drownedMode != null) {
            level.fakeEntities.add(FakeConduitDrowned.create(level, 2.5, 64.0, 0.0));
            if ("two".equals(drownedMode)) {
                level.fakeEntities.add(FakeConduitDrowned.create(level, -2.5, 64.0, 1.0));
            }
        }
        ConduitBlockEntity entity = freshConduit(level);
        JsonArray frames = new JsonArray();
        for (int t = 0; t <= ticks; t++) {
            level.gameTime = t;
            if (t == 40 && "conduit_dry_deactivate".equals(kind)) {
                // 拆掉 3×3×3 里的一格水，下一次重扫应清空框架并失效。
                level.putBlock(CONDUIT_POS.offset(1, 1, 1), Blocks.AIR.defaultBlockState());
            }
            if (t == 80 && "conduit_dry_deactivate".equals(kind)) {
                // 补回水，第 80 刻重新激活。
                level.putBlock(CONDUIT_POS.offset(1, 1, 1), Blocks.WATER.defaultBlockState());
            }
            ConduitBlockEntity.serverTick(level, CONDUIT_POS, level.getBlockState(CONDUIT_POS), entity);
            frames.add(conduitFrame(level, entity));
        }
        JsonObject out = new JsonObject();
        out.addProperty("kind", kind);
        out.addProperty("seed", seed);
        out.add("ticks", frames);
        return out;
    }

    private static JsonObject conduitFrame16() throws Exception {
        return conduitRun("conduit_frame_16", 41L, 82, 16, null, null);
    }

    private static JsonObject conduitFrame42() throws Exception {
        return conduitRun("conduit_frame_42", 42L, 82, 42, null, null);
    }

    private static JsonObject conduitDryDeactivate() throws Exception {
        return conduitRun("conduit_dry_deactivate", 43L, 122, 42, null, null);
    }

    private static JsonObject conduitPlayerEffects() throws Exception {
        return conduitRun("conduit_player_effects", 44L, 82, 42, "wet", null);
    }

    private static JsonObject conduitDrownedAttack() throws Exception {
        return conduitRun("conduit_drowned_attack", 45L, 82, 42, null, "one");
    }

    /** 用官方 `EntityReference.store/read` 做 `Target` 的 `saveCustomOnly` → `loadCustomOnly` 往返。 */
    private static JsonObject conduitSaveRoundtrip() throws Exception {
        MachineLevel level = MachineLevel.createMultiBlock(46L);
        ConduitBlockEntity loaded = freshConduit(level);
        UUID uuid = UUID.fromString("01020304-0506-0708-090a-0b0c0d0e0f10");
        CompoundTag raw = new CompoundTag();
        raw.putIntArray("Target", new int[] {0x01020304, 0x05060708, 0x090a0b0c, 0x0d0e0f10});
        loaded.loadCustomOnly(TagValueInput.create(ProblemReporter.DISCARDING, REGISTRIES, raw));
        CompoundTag saved = loaded.saveCustomOnly(REGISTRIES);
        ConduitBlockEntity empty = freshConduit(level);
        CompoundTag emptySaved = empty.saveCustomOnly(REGISTRIES);
        JsonObject out = new JsonObject();
        out.addProperty("kind", "conduit_save_roundtrip");
        out.addProperty("uuid", uuid.toString());
        out.add("saved", jsonOf(saved));
        out.add("emptySaved", jsonOf(emptySaved));
        out.add("ticks", new JsonArray());
        return out;
    }

    private static JsonObject jsonOf(CompoundTag tag) {
        JsonObject out = new JsonObject();
        tag.getIntArray("Target").ifPresent(words -> {
            JsonArray array = new JsonArray();
            for (int word : words) {
                array.add(word);
            }
            out.add("Target", array);
        });
        return out;
    }

    /** 身体接触水的假玩家；`isInWater()` 由夹具显式给出，不跑 `baseTick`。 */
    private static final class FakeConduitPlayer extends Player {
        private MachineLevel ownerLevel;
        private int uid;
        private boolean inWater;

        private FakeConduitPlayer() {
            super(null, null);
        }

        static FakeConduitPlayer create(MachineLevel level, int uid, double x, double y, double z,
                boolean inWater) {
            FakeConduitPlayer p = allocate(FakeConduitPlayer.class);
            p.ownerLevel = level;
            p.uid = uid;
            p.inWater = inWater;
            try {
                java.lang.reflect.Field posField = Entity.class.getDeclaredField("position");
                posField.setAccessible(true);
                posField.set(p, new Vec3(x, y, z));
                java.lang.reflect.Field levelField = Entity.class.getDeclaredField("level");
                levelField.setAccessible(true);
                levelField.set(p, level);
                java.lang.reflect.Field bp = Entity.class.getDeclaredField("blockPosition");
                bp.setAccessible(true);
                bp.set(p, BlockPos.containing(x, y, z));
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
            p.setUUID(UUID.randomUUID());
            p.setBoundingBox(new AABB(x - 0.3, y, z - 0.3, x + 0.3, y + 1.8, z + 0.3));
            return p;
        }

        @Override
        public GameType gameMode() {
            return GameType.SURVIVAL;
        }

        @Override
        public boolean isSpectator() {
            return false;
        }

        @Override
        public boolean isInWater() {
            return this.inWater;
        }

        @Override
        public boolean addEffect(MobEffectInstance instance, @Nullable Entity source) {
            int id = BuiltInRegistries.MOB_EFFECT.asHolderIdMap().getId(instance.getEffect());
            this.ownerLevel.receivedEffects.add(new int[] {
                this.uid,
                id,
                instance.getDuration(),
                instance.getAmplifier(),
                instance.isAmbient() ? 1 : 0,
                instance.isVisible() ? 1 : 0,
                instance.showIcon() ? 1 : 0
            });
            return true;
        }
    }

    /** 水里、无敌（不真跑官方受伤链）的溺尸，记录 4 点魔法伤害。 */
    private static final class FakeConduitDrowned extends Drowned {
        private MachineLevel ownerLevel;
        private float health = 20.0F;

        private FakeConduitDrowned() {
            super(EntityTypes.DROWNED, null);
        }

        static FakeConduitDrowned create(MachineLevel level, double x, double y, double z) {
            FakeConduitDrowned d = allocate(FakeConduitDrowned.class);
            d.ownerLevel = level;
            d.health = 20.0F;
            try {
                java.lang.reflect.Field levelField = Entity.class.getDeclaredField("level");
                levelField.setAccessible(true);
                levelField.set(d, level);
                java.lang.reflect.Field posField = Entity.class.getDeclaredField("position");
                posField.setAccessible(true);
                posField.set(d, new Vec3(x, y, z));
                java.lang.reflect.Field randomField = Entity.class.getDeclaredField("random");
                randomField.setAccessible(true);
                // 新实体的随机源从 `level.entitySeeds` 取，和现有夹具一致。
                randomField.set(d, RandomSource.create(level.entitySeeds.nextLong()));
                java.lang.reflect.Field waterField = Entity.class.getDeclaredField("wasTouchingWater");
                waterField.setAccessible(true);
                waterField.set(d, true);
                java.lang.reflect.Field bp = Entity.class.getDeclaredField("blockPosition");
                bp.setAccessible(true);
                bp.set(d, BlockPos.containing(x, y, z));
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
            d.setUUID(UUID.fromString("11111111-2222-3333-4444-555555555555"));
            d.setBoundingBox(new AABB(x - 0.3, y, z - 0.3, x + 0.3, y + 1.95, z + 0.3));
            return d;
        }

        @Override
        public float getHealth() {
            return this.health;
        }

        @Override
        public void setHealth(float value) {
            this.health = value;
        }

        @Override
        public boolean isAlive() {
            return this.health > 0.0F;
        }

        @Override
        public boolean isDeadOrDying() {
            return this.health <= 0.0F;
        }

        @Override
        public boolean hurtServer(ServerLevel level,
                net.minecraft.world.damagesource.DamageSource source, float amount) {
            // 只记录伤害量和类型；官方的后续音效 / 战利品表在假世界里没有注册表支持。
            this.ownerLevel.hurts.add(source.type().msgId() + ":" + Float.toString(amount));
            this.health -= amount;
            if (this.health <= 0.0F) {
                // `Entity.discard()` 只把 `removalReason` 置真，不走假实体没初始化的 `remove` 链。
                try {
                    java.lang.reflect.Field removal = Entity.class.getDeclaredField("removalReason");
                    removal.setAccessible(true);
                    removal.set(this, Entity.RemovalReason.KILLED);
                } catch (ReflectiveOperationException error) {
                    throw new IllegalStateException(error);
                }
            }
            return true;
        }
    }

    private static int[] ints(CampfireBlockEntity entity, String name) throws Exception {
        Field field = CampfireBlockEntity.class.getDeclaredField(name);
        field.setAccessible(true);
        return (int[]) field.get(entity);
    }

    private static JsonArray campfireSlots(CampfireBlockEntity entity) {
        JsonArray slots = new JsonArray();
        for (ItemStack stack : entity.getItems()) {
            slots.add(stack(stack));
        }
        return slots;
    }

    private static JsonArray campfireData(int[] progress, int[] time) {
        JsonArray values = new JsonArray();
        for (int value : progress) {
            values.add(value);
        }
        for (int value : time) {
            values.add(value);
        }
        return values;
    }

    private static JsonObject header(String kind, long seed, BlockState state, JsonArray slots, JsonArray data) {
        JsonObject run = new JsonObject();
        run.addProperty("kind", kind);
        run.addProperty("seed", seed);
        run.addProperty("block", Block.BLOCK_STATE_REGISTRY.getId(state));
        run.add("slots", slots);
        run.add("data", data);
        return run;
    }

    private static JsonObject frame(MachineLevel level, JsonArray items, JsonArray data) {
        JsonObject frame = new JsonObject();
        frame.add("data", data);
        frame.addProperty("block", Block.BLOCK_STATE_REGISTRY.getId(level.state));
        frame.add("items", items);
        frame.addProperty("seed", seedOf(level.random));
        return frame;
    }

    private static JsonArray slotsOf(net.minecraft.world.Container container) {
        JsonArray slots = new JsonArray();
        for (int slot = 0; slot < container.getContainerSize(); slot++) {
            slots.add(stack(container.getItem(slot)));
        }
        return slots;
    }

    private static JsonArray dataOfValues(ContainerData data) {
        JsonArray values = new JsonArray();
        for (int id = 0; id < data.getCount(); id++) {
            values.add(data.get(id));
        }
        return values;
    }

    private static JsonArray stack(ItemStack item) {
        JsonArray triple = new JsonArray();
        triple.add(item.isEmpty() ? 0 : BuiltInRegistries.ITEM.getId(item.getItem()));
        triple.add(item.getCount());
        triple.add(item.isEmpty() ? "" : patchHex(item));
        return triple;
    }

    private static String patchHex(ItemStack item) {
        RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), REGISTRIES);
        DataComponentPatch.STREAM_CODEC.encode(buffer, item.getComponentsPatch());
        byte[] bytes = new byte[buffer.readableBytes()];
        buffer.readBytes(bytes);
        StringBuilder builder = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            builder.append(Character.forDigit((value >> 4) & 0xF, 16));
            builder.append(Character.forDigit(value & 0xF, 16));
        }
        return builder.toString();
    }

    private static ContainerData dataOf(Object entity) {
        Class<?> type = entity.getClass();
        while (type != null) {
            try {
                Field field = type.getDeclaredField("dataAccess");
                field.setAccessible(true);
                return (ContainerData) field.get(entity);
            } catch (NoSuchFieldException ignored) {
                type = type.getSuperclass();
            } catch (IllegalAccessException error) {
                throw new IllegalStateException(error);
            }
        }
        throw new IllegalStateException("no dataAccess on " + entity.getClass().getName());
    }

    private static long seedOf(RandomSource random) {
        try {
            Field field = LegacyRandomSource.class.getDeclaredField("seed");
            field.setAccessible(true);
            return ((AtomicLong) field.get(random)).get();
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static void loadRecipes() throws Exception {
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
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
                    .loadResources(resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                            Commands.CommandSelection.DEDICATED, net.minecraft.server.permissions.LevelBasedPermissionSet.GAMEMASTER,
                            ForkJoinPool.commonPool(), Runnable::run)
                    .get();
            managers.updateComponentsAndStaticRegistryTags();
            managers.getRecipeManager().finalizeRecipeLoading(FeatureFlags.DEFAULT_FLAGS);
            RECIPES = managers.getRecipeManager();
            REGISTRIES = layers.compositeAccess();
            MANAGERS = managers;
            SERVER = FakeServer.create();
        }
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

    private static final class FakeBeaconPlayer extends Player {
        private MachineLevel ownerLevel;
        private int uid;
        private boolean spectator;

        private FakeBeaconPlayer() {
            super(null, null);
        }

        static FakeBeaconPlayer create(MachineLevel level, int uid, double x, double y, double z, boolean spectator) {
            FakeBeaconPlayer p = allocate(FakeBeaconPlayer.class);
            p.ownerLevel = level;
            p.uid = uid;
            p.spectator = spectator;
            try {
                Field posField = Entity.class.getDeclaredField("position");
                posField.setAccessible(true);
                posField.set(p, new Vec3(x, y, z));
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
            p.setBoundingBox(new AABB(x - 0.3, y, z - 0.3, x + 0.3, y + 1.8, z + 0.3));
            return p;
        }

        @Override
        public GameType gameMode() {
            return this.spectator ? GameType.SPECTATOR : GameType.SURVIVAL;
        }

        @Override
        public boolean isSpectator() {
            return this.spectator;
        }

        @Override
        public boolean addEffect(MobEffectInstance instance, @Nullable Entity source) {
            int id = BuiltInRegistries.MOB_EFFECT.asHolderIdMap().getId(instance.getEffect());
            this.ownerLevel.receivedEffects.add(new int[] {
                this.uid,
                id,
                instance.getDuration(),
                instance.getAmplifier(),
                instance.isAmbient() ? 1 : 0,
                instance.isVisible() ? 1 : 0,
                instance.showIcon() ? 1 : 0
            });
            return true;
        }
    }

    private static final class MachineLevel extends ServerLevel {
        private BlockState state = Blocks.AIR.defaultBlockState();
        private Map<BlockPos, BlockState> blocks;
        private long gameTime;
        private List<String> sounds;
        private List<int[]> receivedEffects;
        private List<FakeBeaconPlayer> fakePlayers;
        /** 潮涌核心场景里的全部实体（假玩家 + 溺尸），按世界里的实体遍历序。 */
        private List<Entity> fakeEntities;
        /** 新实体的随机源种子，和 `MobOracle` 一样用 `ENTITY_RANDOM_SEED` 起步。 */
        private RandomSource entitySeeds;
        /** 受伤记录（伤害类型 + 数值）。 */
        private List<String> hurts;
        /** `sendBlockUpdated(pos, state, state, 2)` 的调用次数和位置。 */
        private JsonArray blockUpdates;
        private RandomSource random;
        private int nextId = 1;
        /** `addFreshEntity` 收下的新实体，取出成品后用来对拍经验球。 */
        private List<Entity> fresh = new ArrayList<>();

        private MachineLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        static MachineLevel create(long seed) {
            try {
                MachineLevel level = (MachineLevel) UNSAFE.allocateInstance(MachineLevel.class);
                level.state = Blocks.AIR.defaultBlockState();
                level.blocks = null;
                level.gameTime = 0L;
                level.sounds = new ArrayList<>();
                level.receivedEffects = new ArrayList<>();
                level.fakePlayers = new ArrayList<>();
                level.fakeEntities = new ArrayList<>();
                level.entitySeeds = RandomSource.create(0x0DDB1A5E5BAD5EEDL);
                level.hurts = new ArrayList<>();
                level.blockUpdates = new JsonArray();
                Field random = MachineLevel.class.getDeclaredField("random");
                random.setAccessible(true);
                random.set(level, RandomSource.create(seed));
                level.fresh = new ArrayList<>();
                return level;
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
        }

        static MachineLevel createMultiBlock(long seed) {
            MachineLevel level = create(seed);
            level.blocks = new HashMap<>();
            return level;
        }

        void putBlock(BlockPos pos, BlockState blockState) {
            if (blockState.isAir()) {
                this.blocks.remove(pos);
            } else {
                this.blocks.put(pos.immutable(), blockState);
            }
        }

        @Override
        public boolean isClientSide() {
            return false;
        }

        @Override
        public long getGameTime() {
            return this.gameTime;
        }

        @Override
        public int getMinY() {
            return -64;
        }

        @Override
        public int getHeight() {
            return 384;
        }

        @Override
        public int getHeight(Heightmap.Types type, int x, int z) {
            if (this.blocks == null) {
                return 0;
            }
            int highest = Integer.MIN_VALUE;
            for (Map.Entry<BlockPos, BlockState> entry : this.blocks.entrySet()) {
                BlockPos p = entry.getKey();
                if (p.getX() == x && p.getZ() == z && !entry.getValue().isAir()) {
                    if (p.getY() > highest) {
                        highest = p.getY();
                    }
                }
            }
            return highest == Integer.MIN_VALUE ? getMinY() : highest + 1;
        }

        @Override
        public void playSound(@Nullable Entity source, BlockPos pos, SoundEvent sound, SoundSource category,
                float volume, float pitch) {
            this.sounds.add(BuiltInRegistries.SOUND_EVENT.getKey(sound).toString());
        }

        @Override
        public void playSound(@Nullable Entity source, double x, double y, double z, SoundEvent sound,
                SoundSource category, float volume, float pitch) {
            this.sounds.add(BuiltInRegistries.SOUND_EVENT.getKey(sound).toString());
        }

        @Override
        public FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public RecipeManager recipeAccess() {
            return RECIPES;
        }

        @Override
        public MinecraftServer getServer() {
            return SERVER;
        }

        @Override
        public int getNextEntityId() {
            return this.nextId++;
        }

        @Override
        public RandomSource getRandom() {
            return this.random;
        }

        @Override
        public net.minecraft.world.damagesource.DamageSources damageSources() {
            return new DamageSources(REGISTRIES);
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return this.getBlockState(pos).getFluidState();
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            if (this.blocks != null) {
                return this.blocks.getOrDefault(pos, Blocks.AIR.defaultBlockState());
            }
            return this.state;
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState blockState, int updateFlags, int updateLimit) {
            if (this.blocks != null) {
                putBlock(pos, blockState);
            } else {
                this.state = blockState;
            }
            return true;
        }

        @Override
        public void blockEntityChanged(BlockPos pos) {
        }

        @Override
        public void sendBlockUpdated(BlockPos pos, BlockState oldState, BlockState newState, int flags) {
            this.blockUpdates.add(pos.getX() + "," + pos.getY() + "," + pos.getZ() + "," + flags);
        }

        @Override
        public void gameEvent(net.minecraft.core.Holder<net.minecraft.world.level.gameevent.GameEvent> event, Vec3 pos,
                net.minecraft.world.level.gameevent.GameEvent.Context context) {
        }

        @Override
        public void updateNeighbourForOutputSignal(BlockPos pos, net.minecraft.world.level.block.Block changedBlock) {
        }

        @Override
        public void levelEvent(@Nullable Entity source, int type, BlockPos pos, int data) {
        }

        @Override
        public boolean addFreshEntity(Entity entity) {
            fresh.add(entity);
            return true;
        }

        @Override
        public <T extends Entity> List<T> getEntities(EntityTypeTest<Entity, T> type, AABB box, Predicate<? super T> selector) {
            List<Entity> source = this.fakeEntities != null && !this.fakeEntities.isEmpty()
                    ? this.fakeEntities : new ArrayList<>(this.fakePlayers);
            List<T> found = new ArrayList<>();
            for (Entity e : source) {
                T cast = type.tryCast(e);
                if (cast != null && !e.isRemoved() && e.getBoundingBox().intersects(box)
                        && selector.test(cast)) {
                    found.add(cast);
                }
            }
            return found;
        }

        @Override
        public Entity getEntity(UUID uuid) {
            if (this.fakeEntities != null) {
                for (Entity e : this.fakeEntities) {
                    if (e.getUUID().equals(uuid) && !e.isRemoved()) {
                        return e;
                    }
                }
            }
            return null;
        }

        @Override
        public Entity getEntityInAnyDimension(UUID uuid) {
            Entity found = this.getEntity(uuid);
            // 只在没有目标时才补一次假的跨维度查找：所有实体都在本维度里。
            return found != null ? found : null;
        }

        @Override
        protected LevelEntityGetter<Entity> getEntities() {
            final List<Entity> source = this.fakeEntities == null ? List.of() : this.fakeEntities;
            return new LevelEntityGetter<>() {
                @Override public Entity get(int id) { return null; }
                @Override public Entity get(UUID uuid) {
                    for (Entity e : source) {
                        if (e.getUUID().equals(uuid) && !e.isRemoved()) return e;
                    }
                    return null;
                }
                @Override public Iterable<Entity> getAll() { return source; }
                @Override public <U extends Entity> void get(EntityTypeTest<Entity, U> type,
                        net.minecraft.util.AbortableIterationConsumer<U> consumer) {}
                @Override public void get(AABB box, java.util.function.Consumer<Entity> output) {}
                @Override public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AABB box,
                        net.minecraft.util.AbortableIterationConsumer<U> consumer) {}
            };
        }

        @Override
        public boolean noCollision(@Nullable Entity entity, AABB box, boolean alwaysCollideWithFluids) {
            return true;
        }
    }

    /** 燃料公式要问 `level.getServer().reloadableRegistries()`，这里只把那一项接上。 */
    private static final class FakeServer extends DedicatedServer {
        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        static FakeServer create() {
            return (FakeServer) allocate(FakeServer.class);
        }

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return MANAGERS.fullRegistries();
        }
    }

    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
            throw new IllegalStateException(error);
        }
    }
}
