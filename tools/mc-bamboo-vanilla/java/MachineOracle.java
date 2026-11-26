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
import net.minecraft.world.level.block.entity.FurnaceBlockEntity;
import net.minecraft.world.level.block.entity.SmokerBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
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

        JsonObject root = new JsonObject();
        root.add("runs", runs);
        root.add("beacons", beacons);
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
            if (this.fakePlayers == null || this.fakePlayers.isEmpty()) {
                return List.of();
            }
            List<T> found = new ArrayList<>();
            for (FakeBeaconPlayer p : this.fakePlayers) {
                T cast = type.tryCast(p);
                if (cast != null && p.getBoundingBox().intersects(box) && selector.test(cast)) {
                    found.add(cast);
                }
            }
            return found;
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
