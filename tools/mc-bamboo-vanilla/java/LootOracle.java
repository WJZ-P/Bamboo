import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Optional;
import java.util.Random;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
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
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.world.Container;
import net.minecraft.world.Difficulty;
import net.minecraft.world.RandomSequences;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.level.block.entity.ShulkerBoxBlockEntity;
import net.minecraft.core.component.DataComponentPatch;
import net.minecraft.world.item.component.ItemContainerContents;
import com.mojang.serialization.JsonOps;
import net.minecraft.network.chat.Component;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.AgeableMob;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntitySpawnRequest;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.animal.chicken.Chicken;
import net.minecraft.world.entity.animal.cow.MushroomCow;
import net.minecraft.world.entity.animal.frog.Frog;
import net.minecraft.world.entity.animal.sheep.Sheep;
import net.minecraft.world.entity.monster.cubemob.AbstractCubeMob;
import net.minecraft.world.entity.projectile.FishingHook;
import net.minecraft.world.entity.raid.Raider;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.item.equipment.trim.ArmorTrim;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.alchemy.PotionContents;
import net.minecraft.world.item.component.BlockItemStateProperties;
import net.minecraft.world.item.component.InstrumentComponent;
import net.minecraft.world.item.component.OminousBottleAmplifier;
import net.minecraft.world.item.component.SuspiciousStewEffects;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.Enchantments;
import net.minecraft.world.item.enchantment.ItemEnchantments;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.storage.loot.LootParams;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.level.storage.loot.parameters.LootContextParamSets;
import net.minecraft.world.level.storage.loot.parameters.LootContextParams;
import net.minecraft.world.phys.Vec3;
import sun.misc.Unsafe;

/**
 * 用官方战利品表给 Bamboo 出题。
 *
 * 方块题走 {@code LootTable.getRandomItems}，参数和 {@code Block.getDrops} 一样。
 * 箱子题走 {@code LootTable.fill} 的同一步骤（私有方法用反射调用），随机源留在手里，
 * 跑完再取一次 {@code nextLong}。实体题走 {@code LivingEntity.dropFromLootTable} 的参数，
 * 但不放 {@code LAST_DAMAGE_PLAYER}（假世界造不出玩家）。探索地图会去找结构，假世界没有区块，
 * 这种题单独标成排除。造不出的实体也标成排除。
 *
 * Asks the official loot tables. Block cases call {@code getRandomItems} the way
 * {@code Block.getDrops} does. Chest cases follow {@code fill} and then take one more
 * {@code nextLong}. Entity cases use the {@code LivingEntity.dropFromLootTable} parameters
 * without {@code LAST_DAMAGE_PLAYER}, because this fake level cannot construct a player.
 * Exploration maps and entities that cannot be constructed are reported as exclusions.
 */
public final class LootOracle {
    private static final PrintStream OUT =
            new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8);
    private static final Unsafe UNSAFE = loadUnsafe();
    private static final String[] TOOLS = {
        "",
        "minecraft:wooden_pickaxe",
        "minecraft:stone_pickaxe",
        "minecraft:iron_pickaxe",
        "minecraft:golden_pickaxe",
        "minecraft:diamond_pickaxe",
        "minecraft:netherite_pickaxe",
        "minecraft:wooden_axe",
        "minecraft:stone_axe",
        "minecraft:iron_axe",
        "minecraft:golden_axe",
        "minecraft:diamond_axe",
        "minecraft:netherite_axe",
        "minecraft:wooden_shovel",
        "minecraft:stone_shovel",
        "minecraft:iron_shovel",
        "minecraft:golden_shovel",
        "minecraft:diamond_shovel",
        "minecraft:netherite_shovel",
        "minecraft:wooden_hoe",
        "minecraft:stone_hoe",
        "minecraft:iron_hoe",
        "minecraft:golden_hoe",
        "minecraft:diamond_hoe",
        "minecraft:netherite_hoe",
        "minecraft:shears"
    };
    private static final float[] RADII = {0.5F, 1.0F, 1.5F, 2.0F, 3.0F, 4.0F, 8.0F};
    private static final int[] CHEST_SIZES = {9, 27, 54};

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Throwable error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        System.setOut(log);
        int cases = 4000;
        long seed = 20261108L;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--cases" -> cases = Integer.parseInt(args[++i]);
                case "--seed" -> seed = Long.parseLong(args[++i]);
                default -> throw new IllegalArgumentException("usage: LootOracle [--cases N] [--seed S]");
            }
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            World world = World.load(resources);
            Random random = new Random(seed);
            int blockCases = Math.max(cases, 0);
            int chestCases = Math.max(cases / 5, 1);
            int entityCount = Math.max(cases / 5, 1);
            int excluded = 0;
            excluded += blockCases(world, random, blockCases);
            excluded += chestCases(world, random, chestCases);
            excluded += entityCases(world, random, entityCount);
            int blockEntityCases = blockEntityCases(world, random);
            excluded += blockEntityCases;
            JsonObject summary = new JsonObject();
            summary.addProperty("kind", "summary");
            summary.addProperty("cases", blockCases + chestCases + entityCount + 1);
            summary.addProperty("excluded", excluded);
            OUT.println(summary);
        }
    }

    private static int blockCases(World world, Random random, int cases) {
        List<Block> blocks = new ArrayList<>();
        for (Block block : BuiltInRegistries.BLOCK) {
            if (block.getLootTable().isPresent()) {
                blocks.add(block);
            }
        }
        int excluded = 0;
        for (int i = 0; i < cases; i++) {
            Block block = blocks.get(random.nextInt(blocks.size()));
            List<BlockState> states = block.getStateDefinition().getPossibleStates();
            BlockState state = states.get(random.nextInt(states.size()));
            String toolName = TOOLS[random.nextInt(TOOLS.length)];
            int silk = toolName.isEmpty() ? 0 : random.nextInt(2);
            int fortune = toolName.isEmpty() ? 0 : random.nextInt(4);
            boolean explode = random.nextBoolean();
            Float radius = explode ? RADII[random.nextInt(RADII.length)] : null;
            long worldSeed = random.nextLong();
            int x = random.nextInt(32) - 16;
            int y = random.nextInt(64) + 1;
            int z = random.nextInt(32) - 16;
            try {
                CaseResult result = world.blockDrops(state, x, y, z, toolName, silk, fortune, radius, worldSeed);
                JsonObject line = new JsonObject();
                line.addProperty("kind", "block");
                line.addProperty("block", BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString());
                line.addProperty("state", Block.getId(state));
                line.addProperty("x", x);
                line.addProperty("y", y);
                line.addProperty("z", z);
                line.addProperty("toolItem", result.toolItem);
                line.addProperty("toolCount", result.toolCount);
                line.add("enchants", result.enchants);
                if (radius != null) {
                    line.addProperty("explosion", radius);
                }
                line.addProperty("worldSeed", Long.toString(worldSeed));
                line.add("items", result.items);
                line.addProperty("nextLong", Long.toString(result.nextLong));
                OUT.println(line);
            } catch (Throwable error) {
                excluded++;
                printExcluded("block", block.getLootTable().get().identifier().toString(), error);
            }
        }
        return excluded;
    }

    /**
     * 一个带 `BLOCK_ENTITY` 的方块掉落题：装了钻石、改过名的潜影盒。
     * 官方 `Block.dropResources` 把方块实体交给 `copy_components`。
     *
     * A block drop with a `BLOCK_ENTITY`: a named shulker box with diamonds.
     * Vanilla `Block.dropResources` passes the block entity to `copy_components`.
     */
    private static int blockEntityCases(World world, Random random) {
        try {
            JsonObject line = world.blockEntityDrop(random.nextLong());
            OUT.println(line);
            return 0;
        } catch (Throwable error) {
            printExcluded("blockEntity", "minecraft:blocks/shulker_box", error);
            return 1;
        }
    }

    private static int chestCases(World world, Random random, int cases) {
        List<ResourceKey<LootTable>> tables = world.containerTables();
        if (tables.isEmpty()) {
            return 0;
        }
        int excluded = 0;
        for (int i = 0; i < cases; i++) {
            ResourceKey<LootTable> key = tables.get(random.nextInt(tables.size()));
            long lootSeed = random.nextLong();
            if (lootSeed == 0L) {
                lootSeed = 1L;
            }
            int slots = CHEST_SIZES[random.nextInt(CHEST_SIZES.length)];
            int x = random.nextInt(32) - 16;
            int y = random.nextInt(64) + 1;
            int z = random.nextInt(32) - 16;
            float luck = random.nextBoolean() ? 0.0F : random.nextInt(5);
            try {
                CaseResult result = world.chestFill(key, lootSeed, slots, x, y, z, luck);
                JsonObject line = new JsonObject();
                line.addProperty("kind", "chest");
                line.addProperty("table", key.identifier().toString());
                line.addProperty("seed", Long.toString(lootSeed));
                line.addProperty("slots", slots);
                line.addProperty("x", x);
                line.addProperty("y", y);
                line.addProperty("z", z);
                line.addProperty("luck", luck);
                line.add("items", result.items);
                line.addProperty("nextLong", Long.toString(result.nextLong));
                OUT.println(line);
            } catch (Throwable error) {
                excluded++;
                printExcluded("chest", key.identifier().toString(), error);
            }
        }
        return excluded;
    }

    /**
     * 实体死亡掉落。参数照 {@code LivingEntity.dropFromLootTable}，种子照 {@code getLootTableSeed}，
     * 非零时盖过随机序列。不放 {@code LAST_DAMAGE_PLAYER}。
     *
     * Entity death drops, matching {@code LivingEntity.dropFromLootTable}. A non-zero loot seed
     * replaces the random sequence. {@code LAST_DAMAGE_PLAYER} is left unset.
     */
    private static int entityCases(World world, Random random, int cases) {
        List<ResourceKey<LootTable>> tables = world.entityTables();
        if (tables.isEmpty()) {
            return 0;
        }
        int excluded = 0;
        for (int i = 0; i < cases; i++) {
            ResourceKey<LootTable> key = tables.get(random.nextInt(tables.size()));
            long worldSeed = random.nextLong();
            // 四分之一用实体默认的种子 0（走随机序列），其余用非零种子盖过序列。
            // One quarter uses the default seed 0 (the random sequence). The rest use a non-zero seed.
            long lootSeed;
            if (random.nextInt(4) == 0) {
                lootSeed = 0L;
            } else {
                lootSeed = random.nextLong();
                if (lootSeed == 0L) {
                    lootSeed = 1L;
                }
            }
            int x = random.nextInt(32) - 16;
            int y = random.nextInt(64) + 1;
            int z = random.nextInt(32) - 16;
            boolean onGround = random.nextBoolean();
            boolean onFire = random.nextBoolean();
            boolean baby = random.nextBoolean();
            boolean sheared = random.nextBoolean();
            int tick = random.nextInt(40);
            boolean withAttacker = random.nextBoolean();
            int harm = random.nextInt(8);
            try {
                CaseResult result = world.entityDrops(
                        key, worldSeed, lootSeed, x, y, z, onGround, onFire, baby, sheared, tick, withAttacker, harm);
                JsonObject line = new JsonObject();
                line.addProperty("kind", "entity");
                line.addProperty("table", key.identifier().toString());
                line.addProperty("worldSeed", Long.toString(worldSeed));
                line.addProperty("seed", Long.toString(lootSeed));
                line.add("victim", result.victim);
                if (result.attacker != null) {
                    line.add("attacker", result.attacker);
                }
                if (result.directAttacker != null) {
                    line.add("directAttacker", result.directAttacker);
                }
                line.addProperty("damageType", result.damageType);
                line.addProperty("direct", result.direct);
                line.add("items", result.items);
                line.addProperty("nextLong", Long.toString(result.nextLong));
                OUT.println(line);
            } catch (Throwable error) {
                excluded++;
                printExcluded("entity", key.identifier().toString(), error);
            }
        }
        return excluded;
    }

    private static void printExcluded(String kind, String table, Throwable error) {
        JsonObject line = new JsonObject();
        line.addProperty("kind", "excluded");
        line.addProperty("where", kind);
        line.addProperty("table", table);
        line.addProperty("reason", trace(error));
        OUT.println(line);
    }

    private static String trace(Throwable error) {
        StringBuilder text = new StringBuilder();
        Throwable current = error;
        for (int depth = 0; current != null && depth < 4; depth++) {
            if (depth > 0) {
                text.append(" | caused by ");
            }
            text.append(current.getClass().getSimpleName());
            if (current.getMessage() != null) {
                text.append(": ").append(current.getMessage());
            }
            StackTraceElement[] stack = current.getStackTrace();
            int shown = Math.min(stack.length, 6);
            for (int i = 0; i < shown; i++) {
                text.append(" @ ").append(stack[i].getClassName()).append('.').append(stack[i].getMethodName());
            }
            current = current.getCause();
        }
        String value = text.toString();
        return value.length() > 1500 ? value.substring(0, 1500) : value;
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
    private static <T> T allocate(Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static void setField(Class<?> owner, Object target, String name, Object value) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            field.set(target, value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static final class CaseResult {
        JsonArray items = new JsonArray();
        JsonArray enchants = new JsonArray();
        int toolItem;
        int toolCount;
        long nextLong;
        JsonObject victim;
        JsonObject attacker;
        JsonObject directAttacker;
        int damageType;
        boolean direct;
    }

    private static final class World {
        final Holder<DimensionType> overworld;
        final Holder<Biome> plains;
        final DamageSources damageSources;
        final RegistryAccess registries;
        final ReloadableServerRegistries.Holder lootRegistries;
        final Holder<Enchantment> silkTouch;
        final Holder<Enchantment> fortune;
        final FakeServer server;
        final FakeLevel level;
        final Method shuffleAndSplit;
        final Method availableSlots;
        final Field randomSequence;

        private World(
                Holder<DimensionType> overworld,
                Holder<Biome> plains,
                DamageSources damageSources,
                RegistryAccess registries,
                ReloadableServerRegistries.Holder lootRegistries,
                Holder<Enchantment> silkTouch,
                Holder<Enchantment> fortune,
                FakeServer server,
                FakeLevel level
        ) throws ReflectiveOperationException {
            this.overworld = overworld;
            this.plains = plains;
            this.damageSources = damageSources;
            this.registries = registries;
            this.lootRegistries = lootRegistries;
            this.silkTouch = silkTouch;
            this.fortune = fortune;
            this.server = server;
            this.level = level;
            this.shuffleAndSplit = LootTable.class.getDeclaredMethod(
                    "shuffleAndSplitItems", it.unimi.dsi.fastutil.objects.ObjectArrayList.class, int.class, RandomSource.class);
            this.shuffleAndSplit.setAccessible(true);
            this.availableSlots = LootTable.class.getDeclaredMethod("getAvailableSlots", Container.class, RandomSource.class);
            this.availableSlots.setAccessible(true);
            this.randomSequence = LootTable.class.getDeclaredField("randomSequence");
            this.randomSequence.setAccessible(true);
        }

        static World load(CloseableResourceManager resources) throws Exception {
            LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
            RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
            RegistryAccess.Frozen worldGen = RegistryDataLoader
                    .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                    .join();
            LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, worldGen);
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
            RegistryAccess registries = (RegistryAccess) managers.fullRegistries().lookup();
            Holder<DimensionType> overworld =
                    registries.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            Holder<Biome> plains = registries.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
            Registry<Enchantment> enchantments = registries.lookupOrThrow(Registries.ENCHANTMENT);
            FakeServer server = allocate(FakeServer.class);
            server.registries = managers.fullRegistries();
            server.recipes = managers.getRecipeManager();
            FakeLevel level = allocate(FakeLevel.class);
            level.server = server;
            level.plains = plains;
            setField(Level.class, level, "dimensionTypeRegistration", overworld);
            setField(Level.class, level, "dimension", Level.OVERWORLD);
            setField(Level.class, level, "registryAccess", registries);
            setField(Level.class, level, "damageSources", new DamageSources(registries));
            setField(ServerLevel.class, level, "server", server);
            return new World(
                    overworld,
                    plains,
                    new DamageSources(registries),
                    registries,
                    managers.fullRegistries(),
                    enchantments.getOrThrow(Enchantments.SILK_TOUCH),
                    enchantments.getOrThrow(Enchantments.FORTUNE),
                    server,
                    level);
        }

        /** 一个装了钻石、改过名的潜影盒，和 `Block.dropResources` 一样的 BLOCK 参数。 */
        JsonObject blockEntityDrop(long worldSeed) throws Exception {
            server.sequences = new RandomSequences();
            server.worldSeed = worldSeed;
            RandomSource levelRandom = RandomSource.create(worldSeed);
            setField(Level.class, level, "random", levelRandom);
            BlockState state = Blocks.SHULKER_BOX.defaultBlockState();
            ShulkerBoxBlockEntity chest = new ShulkerBoxBlockEntity(BlockPos.ZERO, state);
            chest.setItem(0, new ItemStack(Items.DIAMOND, 7));
            // `collectImplicitComponents` 读的是方块实体的 `name` 字段，不是组件表。
            // `collectImplicitComponents` reads the block entity's `name` field.
            setField(net.minecraft.world.level.block.entity.BaseContainerBlockEntity.class,
                    chest, "name", Component.literal("oracle box"));
            int x = 3;
            int y = 4;
            int z = 5;
            LootParams params = new LootParams.Builder(level)
                    .withParameter(LootContextParams.BLOCK_STATE, state)
                    .withParameter(LootContextParams.ORIGIN, new Vec3(x + 0.5, y + 0.5, z + 0.5))
                    .withOptionalParameter(LootContextParams.BLOCK_ENTITY, chest)
                    .withParameter(LootContextParams.TOOL, ItemStack.EMPTY)
                    .create(LootContextParamSets.BLOCK);
            LootTable table = lootRegistries.getLootTable(state.getBlock().getLootTable().orElseThrow());
            List<ItemStack> drops = table.getRandomItems(params);
            Optional<Identifier> sequence = sequenceOf(table);
            long nextLong = sequence.isPresent()
                    ? server.getRandomSequence(sequence.get()).nextLong()
                    : levelRandom.nextLong();
            JsonObject line = new JsonObject();
            line.addProperty("kind", "blockEntity");
            line.addProperty("block", "minecraft:shulker_box");
            line.addProperty("state", Block.getId(state));
            line.addProperty("x", x);
            line.addProperty("y", y);
            line.addProperty("z", z);
            line.addProperty("toolItem", 0);
            line.addProperty("toolCount", 0);
            line.add("enchants", new JsonArray());
            line.addProperty("worldSeed", Long.toString(worldSeed));
            JsonArray items = new JsonArray();
            for (ItemStack drop : drops) {
                items.add(stackJson(drop));
            }
            line.add("items", items);
            line.addProperty("nextLong", Long.toString(nextLong));
            // 方块实体组件的 codec JSON，Bamboo 侧要转成同样的组件再求值。
            JsonObject componentJson = new JsonObject();
            ItemContainerContents contents = ItemContainerContents.fromItems(
                    java.util.stream.IntStream.range(0, chest.getContainerSize())
                            .mapToObj(chest::getItem)
                            .toList());
            componentJson.add("minecraft:container",
                    ItemContainerContents.CODEC.encodeStart(JsonOps.INSTANCE, contents)
                            .result().orElseThrow());
            componentJson.addProperty("minecraft:custom_name", "oracle box");
            line.add("blockEntity", componentJson);
            return line;
        }

        List<ResourceKey<LootTable>> containerTables() {
            Registry<LootTable> registry = registries.lookupOrThrow(Registries.LOOT_TABLE);
            List<ResourceKey<LootTable>> keys = new ArrayList<>();
            for (ResourceKey<LootTable> key : registry.registryKeySet()) {
                String path = key.identifier().getPath();
                if (path.startsWith("blocks/") || path.startsWith("entities/")) {
                    continue;
                }
                keys.add(key);
            }
            keys.sort(Comparator.comparing(key -> key.identifier().toString()));
            return keys;
        }

        List<ResourceKey<LootTable>> entityTables() {
            Registry<LootTable> registry = registries.lookupOrThrow(Registries.LOOT_TABLE);
            List<ResourceKey<LootTable>> keys = new ArrayList<>();
            for (ResourceKey<LootTable> key : registry.registryKeySet()) {
                if (key.identifier().getPath().startsWith("entities/")) {
                    keys.add(key);
                }
            }
            keys.sort(Comparator.comparing(key -> key.identifier().toString()));
            return keys;
        }

        EntityType<?> typeFor(ResourceKey<LootTable> key) {
            for (EntityType<?> type : BuiltInRegistries.ENTITY_TYPE) {
                Optional<ResourceKey<LootTable>> loot = type.getDefaultLootTable();
                if (loot.isPresent() && loot.get().equals(key)) {
                    return type;
                }
            }
            String[] parts = key.identifier().getPath().split("/");
            if (parts.length < 2) {
                return null;
            }
            return entityType(parts[1]);
        }

        EntityType<?> entityType(String path) {
            for (EntityType<?> type : BuiltInRegistries.ENTITY_TYPE) {
                Identifier id = BuiltInRegistries.ENTITY_TYPE.getKey(type);
                if (id != null && id.getPath().equals(path)) {
                    return type;
                }
            }
            return null;
        }

        /**
         * 先用一次性随机源把实体造出来，再换成这题的世界随机源，避免构造函数把战利品的取数吃掉。
         *
         * Entities are constructed against a throwaway random source, then the case's world
         * random is installed, so constructors cannot consume the loot stream.
         */
        CaseResult entityDrops(
                ResourceKey<LootTable> key,
                long worldSeed,
                long lootSeed,
                int x,
                int y,
                int z,
                boolean onGround,
                boolean onFire,
                boolean baby,
                boolean sheared,
                int tick,
                boolean withAttacker,
                int harm
        ) throws Exception {
            EntityType<?> type = typeFor(key);
            if (type == null) {
                throw new IllegalStateException("entity setup: no entity type for " + key.identifier());
            }
            setField(Level.class, level, "random", RandomSource.create(1L));
            Entity victim;
            try {
                victim = type.create(level, new EntitySpawnRequest(EntitySpawnReason.TRIGGERED, true));
            } catch (Throwable error) {
                throw new IllegalStateException("entity setup: " + error, error);
            }
            if (victim == null) {
                throw new IllegalStateException("entity setup: create returned null for " + key.identifier());
            }
            double px = x + 0.5;
            double py = y;
            double pz = z + 0.5;
            try {
                victim.setPos(px, py, pz);
                victim.setOnGround(onGround);
                victim.tickCount = tick;
                if (onFire) {
                    victim.setRemainingFireTicks(40);
                }
                if (baby && victim instanceof AgeableMob ageable) {
                    ageable.setBaby(true);
                }
                if (sheared && victim instanceof Sheep sheep) {
                    sheep.setSheared(true);
                }
            } catch (Throwable error) {
                throw new IllegalStateException("entity setup: " + error, error);
            }
            LivingEntity attacker = null;
            if (withAttacker) {
                try {
                    EntityType<?> zombie = entityType("zombie");
                    Entity created = zombie == null
                            ? null
                            : zombie.create(level, new EntitySpawnRequest(EntitySpawnReason.TRIGGERED, true));
                    if (created instanceof LivingEntity living) {
                        living.setPos(px + 1.0, py, pz);
                        attacker = living;
                    }
                } catch (Throwable error) {
                    throw new IllegalStateException("entity setup: " + error, error);
                }
            }
            server.sequences = new RandomSequences();
            server.worldSeed = worldSeed;
            setField(Level.class, level, "random", RandomSource.create(worldSeed));
            DamageSource source = harmSource(attacker, harm);
            JsonObject victimJson = entityJson(victim);
            JsonObject attackerJson = source.getEntity() == null ? null : entityJson(source.getEntity());
            JsonObject directJson = source.getDirectEntity() == null ? null : entityJson(source.getDirectEntity());
            LootParams params = new LootParams.Builder(level)
                    .withParameter(LootContextParams.THIS_ENTITY, victim)
                    .withParameter(LootContextParams.ORIGIN, victim.position())
                    .withParameter(LootContextParams.DAMAGE_SOURCE, source)
                    .withOptionalParameter(LootContextParams.ATTACKING_ENTITY, source.getEntity())
                    .withOptionalParameter(LootContextParams.DIRECT_ATTACKING_ENTITY, source.getDirectEntity())
                    .create(LootContextParamSets.ENTITY);
            LootTable table = lootRegistries.getLootTable(key);
            net.minecraft.world.level.storage.loot.LootContext context = new net.minecraft.world.level.storage.loot.LootContext.Builder(params)
                    .withOptionalRandomSeed(lootSeed)
                    .create(sequenceOf(table));
            List<ItemStack> drops = new ArrayList<>();
            table.getRandomItems(context, drops::add);
            CaseResult result = new CaseResult();
            result.victim = victimJson;
            result.attacker = attackerJson;
            result.directAttacker = directJson;
            result.damageType = registries.lookupOrThrow(Registries.DAMAGE_TYPE).getId(source.type());
            result.direct = source.isDirect();
            for (ItemStack drop : drops) {
                result.items.add(stackJson(drop));
            }
            result.nextLong = context.getRandom().nextLong();
            return result;
        }

        private DamageSource harmSource(LivingEntity attacker, int harm) {
            DamageSources sources = level.damageSources();
            return switch (harm) {
                case 0 -> sources.generic();
                case 1 -> sources.cactus();
                case 2 -> sources.magic();
                case 3 -> sources.genericKill();
                case 4 -> sources.fall();
                case 5 -> sources.freeze();
                case 6 -> sources.lava();
                default -> attacker == null ? sources.generic() : sources.mobAttack(attacker);
            };
        }

        private JsonObject entityJson(Entity entity) {
            boolean living = entity instanceof LivingEntity;
            boolean fallFlying = living && ((LivingEntity) entity).isFallFlying();
            JsonObject json = new JsonObject();
            json.addProperty("type", BuiltInRegistries.ENTITY_TYPE.getId(entity.getType()));
            json.addProperty("living", living);
            json.addProperty("onGround", entity.onGround());
            json.addProperty("onFire", entity.isOnFire());
            json.addProperty("crouching", entity.isCrouching());
            json.addProperty("sprinting", entity.isSprinting());
            json.addProperty("swimming", entity.isSwimming());
            json.addProperty("flying", fallFlying);
            json.addProperty("fallFlying", fallFlying);
            json.addProperty("baby", living && ((LivingEntity) entity).isBaby());
            json.addProperty("inWater", entity.isInWater());
            json.addProperty("x", entity.getX());
            json.addProperty("y", entity.getY());
            json.addProperty("z", entity.getZ());
            Vec3 movement = entity.getKnownMovement();
            json.addProperty("mx", movement.x);
            json.addProperty("my", movement.y);
            json.addProperty("mz", movement.z);
            json.addProperty("fall", entity.fallDistance);
            json.addProperty("tick", entity.tickCount);
            BlockPos on = entity.getOnPos();
            json.addProperty("onX", on.getX());
            json.addProperty("onY", on.getY());
            json.addProperty("onZ", on.getZ());
            BlockPos below = entity.getBlockPosBelowThatAffectsMyMovement();
            json.addProperty("belowX", below.getX());
            json.addProperty("belowY", below.getY());
            json.addProperty("belowZ", below.getZ());
            json.addProperty("sheep", entity instanceof Sheep);
            json.addProperty("sheared", entity instanceof Sheep sheep && sheep.isSheared());
            // 掉落条件按组件原值比较。`create` 不跑 `finalizeSpawn`，所以这里是构造后的默认值。
            // Drop conditions compare component values. `create` does not run `finalizeSpawn`,
            // so these are the defaults left by the constructor.
            JsonObject components = new JsonObject();
            if (entity instanceof Sheep sheep) {
                components.addProperty("minecraft:sheep/color", sheep.getColor().getSerializedName());
            }
            if (entity instanceof Chicken chicken) {
                components.addProperty("minecraft:chicken/variant", holderId(chicken.getVariant()));
            }
            if (entity instanceof Frog frog) {
                components.addProperty("minecraft:frog/variant", holderId(frog.getVariant()));
            }
            if (entity instanceof MushroomCow mooshroom) {
                components.addProperty("minecraft:mooshroom/variant", mooshroom.getVariant().getSerializedName());
            }
            json.add("components", components);
            json.addProperty("raider", entity instanceof Raider);
            json.addProperty("captain", entity instanceof Raider raider && raider.isCaptain());
            boolean hasRaid = false;
            if (entity instanceof Raider raider) {
                try {
                    hasRaid = raider.hasRaid();
                } catch (Throwable ignored) {
                    hasRaid = false;
                }
            }
            json.addProperty("hasRaid", hasRaid);
            json.addProperty("fishingHook", entity instanceof FishingHook);
            json.addProperty("openWater", entity instanceof FishingHook hook && hook.isOpenWaterFishing());
            if (entity instanceof AbstractCubeMob cube) {
                json.addProperty("cubeSize", cube.getSize());
            }
            if (living) {
                JsonObject equipment = new JsonObject();
                LivingEntity mob = (LivingEntity) entity;
                for (EquipmentSlot slot : EquipmentSlot.VALUES) {
                    ItemStack stack = mob.getItemBySlot(slot);
                    if (!stack.isEmpty()) {
                        equipment.add(slot.getSerializedName(), stackJson(stack));
                    }
                }
                if (equipment.size() > 0) {
                    json.add("equipment", equipment);
                }
            }
            return json;
        }

        CaseResult blockDrops(
                BlockState state, int x, int y, int z, String toolName, int silk, int fortuneLevel, Float radius, long worldSeed
        ) throws Exception {
            ItemStack tool = tool(toolName, silk, fortuneLevel);
            server.sequences = new RandomSequences();
            server.worldSeed = worldSeed;
            RandomSource levelRandom = RandomSource.create(worldSeed);
            setField(Level.class, level, "random", levelRandom);
            LootParams.Builder builder = new LootParams.Builder(level)
                    .withParameter(LootContextParams.BLOCK_STATE, state)
                    .withParameter(LootContextParams.ORIGIN, new Vec3(x + 0.5, y + 0.5, z + 0.5))
                    .withParameter(LootContextParams.TOOL, tool);
            if (radius != null) {
                builder.withParameter(LootContextParams.EXPLOSION_RADIUS, radius);
            }
            LootParams params = builder.create(LootContextParamSets.BLOCK);
            LootTable table = lootRegistries.getLootTable(state.getBlock().getLootTable().orElseThrow());
            List<ItemStack> drops = table.getRandomItems(params);
            CaseResult result = new CaseResult();
            result.toolItem = BuiltInRegistries.ITEM.getId(tool.getItem());
            result.toolCount = tool.getCount();
            result.enchants = enchantList(tool);
            for (ItemStack drop : drops) {
                result.items.add(stackJson(drop));
            }
            Optional<Identifier> sequence = sequenceOf(table);
            result.nextLong = sequence.isPresent() ? server.getRandomSequence(sequence.get()).nextLong() : levelRandom.nextLong();
            return result;
        }

        @SuppressWarnings("unchecked")
        CaseResult chestFill(ResourceKey<LootTable> key, long lootSeed, int slots, int x, int y, int z, float luck)
                throws Exception {
            LootTable table = lootRegistries.getLootTable(key);
            LootParams params = new LootParams.Builder(level)
                    .withParameter(LootContextParams.ORIGIN, new Vec3(x + 0.5, y + 0.5, z + 0.5))
                    .withLuck(luck)
                    .create(LootContextParamSets.CHEST);
            // 非零种子时官方 `withOptionalRandomSource` 盖过 `random_sequence`，和 `fill` 一样。
            RandomSource random = RandomSource.create(lootSeed);
            it.unimi.dsi.fastutil.objects.ObjectArrayList<ItemStack> stacks = table.getRandomItems(params, random);
            SimpleContainer container = new SimpleContainer(slots);
            List<Integer> available = (List<Integer>) availableSlots.invoke(table, container, random);
            shuffleAndSplit.invoke(table, stacks, available.size(), random);
            for (ItemStack stack : stacks) {
                if (available.isEmpty()) {
                    break;
                }
                int slot = available.remove(available.size() - 1);
                container.setItem(slot, stack.isEmpty() ? ItemStack.EMPTY : stack);
            }
            CaseResult result = new CaseResult();
            for (int slot = 0; slot < slots; slot++) {
                result.items.add(stackJson(container.getItem(slot)));
            }
            result.nextLong = random.nextLong();
            return result;
        }

        ItemStack tool(String name, int silk, int fortuneLevel) {
            if (name.isEmpty()) {
                return ItemStack.EMPTY;
            }
            Item item = BuiltInRegistries.ITEM.getValue(Identifier.parse(name));
            ItemStack stack = new ItemStack(item);
            if (silk > 0) {
                stack.enchant(silkTouch, silk);
            }
            if (fortuneLevel > 0) {
                stack.enchant(fortune, fortuneLevel);
            }
            return stack;
        }

        @SuppressWarnings("unchecked")
        Optional<Identifier> sequenceOf(LootTable table) throws IllegalAccessException {
            return (Optional<Identifier>) randomSequence.get(table);
        }
    }

    private static JsonArray enchantList(ItemStack tool) {
        JsonArray list = new JsonArray();
        ItemEnchantments enchantments = tool.get(DataComponents.ENCHANTMENTS);
        if (enchantments == null) {
            return list;
        }
        List<Holder<Enchantment>> keys = new ArrayList<>(enchantments.keySet());
        keys.sort(Comparator.comparing(holder -> holderId(holder)));
        for (Holder<Enchantment> holder : keys) {
            int level = enchantments.getLevel(holder);
            if (level <= 0) {
                continue;
            }
            JsonObject entry = new JsonObject();
            entry.addProperty("name", holderId(holder));
            entry.addProperty("level", level);
            list.add(entry);
        }
        return list;
    }

    private static JsonObject stackJson(ItemStack stack) {
        JsonObject json = new JsonObject();
        json.addProperty("item", BuiltInRegistries.ITEM.getId(stack.getItem()));
        json.addProperty("count", stack.getCount());
        json.add("components", changedComponents(stack));
        // 物品原型上的组件。Bamboo 记下的是函数写出的绝对值，比对前要扣掉和原型相同的部分。
        // Components on a fresh stack. Bamboo stores the absolute value a function wrote,
        // so the comparison drops anything that already matches this baseline.
        json.add("baseline", stack.isEmpty() ? new JsonObject() : componentsOf(new ItemStack(stack.getItem())));
        return json;
    }

    /** 物品原型上本来就有的组件（比如默认物品名）不算战利品函数写进去的。 */
    private static JsonObject changedComponents(ItemStack stack) {
        if (stack.isEmpty()) {
            return new JsonObject();
        }
        JsonObject current = componentsOf(stack);
        JsonObject baseline = componentsOf(new ItemStack(stack.getItem()));
        JsonObject extra = new JsonObject();
        for (String key : current.keySet()) {
            if (!baseline.has(key) || !baseline.get(key).equals(current.get(key))) {
                extra.add(key, current.get(key));
            }
        }
        return extra;
    }

    /** 只写出战利品函数会改的那几个组件，形状和 Bamboo 记下的 JSON 对齐。 */
    private static JsonObject componentsOf(ItemStack stack) {
        JsonObject components = new JsonObject();
        Integer damage = stack.get(DataComponents.DAMAGE);
        if (damage != null && damage != 0) {
            components.addProperty("minecraft:damage", damage);
        }
        addEnchantments(components, "minecraft:enchantments", stack.get(DataComponents.ENCHANTMENTS));
        addEnchantments(components, "minecraft:stored_enchantments", stack.get(DataComponents.STORED_ENCHANTMENTS));
        BlockItemStateProperties blockState = stack.get(DataComponents.BLOCK_STATE);
        if (blockState != null && !blockState.properties().isEmpty()) {
            JsonObject properties = new JsonObject();
            blockState.properties().entrySet().stream()
                    .sorted(java.util.Map.Entry.comparingByKey())
                    .forEach(entry -> properties.addProperty(entry.getKey(), entry.getValue()));
            components.add("minecraft:block_state", properties);
        }
        PotionContents potion = stack.get(DataComponents.POTION_CONTENTS);
        if (potion != null && potion.potion().isPresent()) {
            JsonObject contents = new JsonObject();
            contents.addProperty("potion", holderId(potion.potion().get()));
            components.add("minecraft:potion_contents", contents);
        }
        SuspiciousStewEffects stew = stack.get(DataComponents.SUSPICIOUS_STEW_EFFECTS);
        if (stew != null && !stew.effects().isEmpty()) {
            JsonArray effects = new JsonArray();
            for (SuspiciousStewEffects.Entry entry : stew.effects()) {
                JsonObject effect = new JsonObject();
                effect.addProperty("id", holderId(entry.effect()));
                effect.addProperty("duration", entry.duration());
                effects.add(effect);
            }
            components.add("minecraft:suspicious_stew_effects", effects);
        }
        OminousBottleAmplifier amplifier = stack.get(DataComponents.OMINOUS_BOTTLE_AMPLIFIER);
        if (amplifier != null && amplifier.value() != 0) {
            components.addProperty("minecraft:ominous_bottle_amplifier", amplifier.value());
        }
        InstrumentComponent instrument = stack.get(DataComponents.INSTRUMENT);
        if (instrument != null) {
            components.addProperty("minecraft:instrument", holderId(instrument.instrument()));
        }
        ItemContainerContents container = stack.get(DataComponents.CONTAINER);
        if (container != null && container.nonEmptyItems().iterator().hasNext()) {
            components.add("minecraft:container",
                    ItemContainerContents.CODEC.encodeStart(JsonOps.INSTANCE, container)
                            .result().orElseThrow());
        }
        Integer trade = stack.get(DataComponents.ADDITIONAL_TRADE_COST);
        if (trade != null) {
            components.addProperty("minecraft:additional_trade_cost", trade);
        }
        net.minecraft.network.chat.Component customName = stack.get(DataComponents.CUSTOM_NAME);
        if (customName != null) {
            components.addProperty("minecraft:custom_name", customName.getString());
        }
        net.minecraft.network.chat.Component itemName = stack.get(DataComponents.ITEM_NAME);
        if (itemName != null) {
            components.addProperty("minecraft:item_name", itemName.getString());
        }
        ArmorTrim trim = stack.get(DataComponents.TRIM);
        if (trim != null) {
            JsonObject value = new JsonObject();
            value.addProperty("material", holderId(trim.material()));
            value.addProperty("pattern", holderId(trim.pattern()));
            components.add("minecraft:trim", value);
        }
        return components;
    }

    private static void addEnchantments(JsonObject components, String name, ItemEnchantments enchantments) {
        if (enchantments == null || enchantments.isEmpty()) {
            return;
        }
        JsonObject map = new JsonObject();
        List<Holder<Enchantment>> keys = new ArrayList<>(enchantments.keySet());
        keys.sort(Comparator.comparing(LootOracle::holderId));
        for (Holder<Enchantment> holder : keys) {
            int level = enchantments.getLevel(holder);
            if (level > 0) {
                map.addProperty(holderId(holder), level);
            }
        }
        if (map.size() > 0) {
            components.add(name, map);
        }
    }

    private static String holderId(Holder<?> holder) {
        return holder.unwrapKey().map(key -> key.identifier().toString()).orElse("unknown");
    }

    private static final class FakeServer extends DedicatedServer {
        ReloadableServerRegistries.Holder registries;
        RecipeManager recipes;
        RandomSequences sequences = new RandomSequences();
        long worldSeed;

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        @Override
        public RandomSource getRandomSequence(Identifier key) {
            return sequences.get(key, worldSeed);
        }

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return registries;
        }

        @Override
        public RecipeManager getRecipeManager() {
            return recipes;
        }
    }

    /**
     * 构造函数不跑。战利品求值碰到的世界查询在这里给出和空超平坦一样的答案：
     * 空气、平原、不下雨、亮度 15、看得到天。
     */
    private static final class FakeLevel extends ServerLevel {
        FakeServer server;
        Holder<Biome> plains;
        int nextEntityId = 1;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        @Override
        public net.minecraft.server.MinecraftServer getServer() {
            return server;
        }

        @Override
        public FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return Blocks.AIR.defaultBlockState();
        }

        @Override
        public boolean isLoaded(BlockPos pos) {
            // 没有区块源。空世界里每个位置都算已加载，和 Rust 的 FlatLevel 一样。
            // There is no chunk source. Every position counts as loaded, matching Rust's FlatLevel.
            return true;
        }

        @Override
        public Holder<Biome> getBiome(BlockPos pos) {
            return plains;
        }

        @Override
        public boolean canSeeSky(BlockPos pos) {
            return true;
        }

        @Override
        public int getMaxLocalRawBrightness(BlockPos pos) {
            return 15;
        }

        @Override
        public int getMaxLocalRawBrightness(BlockPos pos, int skyDarkening) {
            return 15;
        }

        @Override
        public boolean isRaining() {
            return false;
        }

        @Override
        public boolean isThundering() {
            return false;
        }

        @Override
        public Difficulty getDifficulty() {
            return Difficulty.NORMAL;
        }

        @Override
        public int getNextEntityId() {
            // `allocateInstance` 不跑字段初始化，0 不是合法的实体 id。
            // allocateInstance skips field initializers, and 0 is not a valid entity id.
            if (nextEntityId <= 0) {
                nextEntityId = 1;
            }
            int id = nextEntityId;
            nextEntityId++;
            return id;
        }
    }
}
