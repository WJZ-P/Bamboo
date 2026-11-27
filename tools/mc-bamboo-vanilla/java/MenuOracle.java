import com.google.common.hash.HashCode;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import io.netty.buffer.Unpooled;
import io.netty.channel.ChannelFutureListener;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Random;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.advancements.triggers.CriterionTrigger;
import net.minecraft.advancements.CriterionTriggerInstance;
import net.minecraft.server.PlayerAdvancements;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.resources.ResourceKey;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponentPatch;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.component.TypedDataComponent;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.HashedPatchMap;
import net.minecraft.network.HashedStack;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.Packet;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.BannerPatternTags;
import net.minecraft.tags.TagLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.util.HashOps;
import net.minecraft.util.Prediction;
import net.minecraft.world.Container;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.entity.player.Abilities;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.animal.nautilus.AbstractNautilus;
import net.minecraft.world.entity.animal.nautilus.Nautilus;
import net.minecraft.world.inventory.NautilusInventoryMenu;
import net.minecraft.world.entity.player.PlayerEquipment;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.inventory.AbstractFurnaceMenu;
import net.minecraft.world.inventory.AnvilMenu;
import net.minecraft.world.inventory.CartographyTableMenu;
import net.minecraft.world.inventory.BlastFurnaceMenu;
import net.minecraft.world.inventory.BrewingStandMenu;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.inventory.ContainerData;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.inventory.ContainerLevelAccess;
import net.minecraft.world.inventory.ContainerSynchronizer;
import net.minecraft.world.inventory.CrafterMenu;
import net.minecraft.world.inventory.CraftingMenu;
import net.minecraft.world.inventory.DataSlot;
import net.minecraft.world.inventory.DispenserMenu;
import net.minecraft.world.inventory.EnchantmentMenu;
import net.minecraft.world.inventory.FurnaceMenu;
import net.minecraft.world.inventory.GrindstoneMenu;
import net.minecraft.world.inventory.HopperMenu;
import net.minecraft.world.inventory.InventoryMenu;
import net.minecraft.world.inventory.LoomMenu;
import net.minecraft.server.network.Filterable;
import net.minecraft.util.Mth;
import net.minecraft.world.inventory.LecternMenu;
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.inventory.MerchantMenu;
import net.minecraft.world.inventory.RemoteSlot;
import net.minecraft.world.inventory.ShulkerBoxMenu;
import net.minecraft.world.inventory.SimpleContainerData;
import net.minecraft.world.inventory.Slot;
import net.minecraft.world.inventory.SmithingMenu;
import net.minecraft.world.inventory.SmokerMenu;
import net.minecraft.world.level.storage.loot.parameters.LootContextParams;
import net.minecraft.world.level.storage.loot.parameters.LootContextParamSets;
import net.minecraft.world.level.storage.loot.LootParams;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.inventory.StonecutterMenu;
import net.minecraft.world.inventory.TransientCraftingContainer;
import net.minecraft.world.item.DyeColor;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.trading.ItemCost;
import net.minecraft.world.item.trading.Merchant;
import net.minecraft.world.item.trading.MerchantOffer;
import net.minecraft.world.item.trading.MerchantOffers;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.Enchantments;
import net.minecraft.world.item.enchantment.ItemEnchantments;
import net.minecraft.world.item.ItemStackTemplate;
import net.minecraft.world.item.component.BundleContents;
import net.minecraft.world.item.component.WritableBookContent;
import net.minecraft.world.item.component.WrittenBookContent;
import net.minecraft.world.item.crafting.AbstractCookingRecipe;
import net.minecraft.world.item.crafting.BrewingRecipe;
import net.minecraft.world.item.crafting.Ingredient;
import net.minecraft.world.item.crafting.RecipeHolder;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.item.crafting.RecipeType;
import net.minecraft.world.item.crafting.ShapedRecipe;
import net.minecraft.world.item.crafting.ShapelessRecipe;
import net.minecraft.world.item.crafting.SingleItemRecipe;
import net.minecraft.world.item.crafting.SmithingRecipe;
import net.minecraft.world.level.block.entity.BannerPattern;
import net.minecraft.world.level.saveddata.maps.MapId;
import net.minecraft.world.level.saveddata.maps.MapItemSavedData;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.storage.PrimaryLevelData;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelSettings.DifficultySettings;
import net.minecraft.world.Difficulty;
import com.mojang.serialization.Lifecycle;
import net.minecraft.world.level.block.entity.BannerPatternLayers;
import net.minecraft.world.level.gamerules.GameRules;
import sun.misc.Unsafe;

/**
 * 容器菜单的 oracle：官方代码出题、给答案，tools/mc-bamboo-vanilla/src/menu.rs 用 mc-bamboo-game 的 Replay 跑同样的题来比较。
 *
 * <p>不启动服务端，也不打开真正的方块。箱子、漏斗、发射器和潜影盒配一个 SimpleContainer。
 * 工作台、玩家 2×2、合成器、切石机、锻造台、三种熔炉和酿造台用菜单自己的格子。合成器配一个
 * 3×3 的 CraftingContainer 和 10 个数据槽（九个禁用位加 triggered），结果格只读。
 * 讲台用一个只实现了书槽的 Container。
 * 随机题按“从不预测”的模型点。最后另有一道带组件的题，客户端按官方预测回传哈希。
 * 每步写出槽位、光标、stateId、data 槽和组件补丁。讲台还写页码。
 * 假玩家的掉落被 override 成空操作，菜单比较只看槽位，不看地上的物品实体。
 *
 * <p>运行（工作目录和 classpath 与 Probe.java 相同，Java 25 或更新）：
 *
 *     java -cp <target/vanilla 下全部 jar> MenuOracle.java [--seed S] [--cases N] [--case I]
 *
 *     --seed S   默认 1。参数相同时两次运行的输出逐字节相同。
 *     --cases N  题目总数，默认 200。
 *     --case I   只跑第 I 题，用来重现不一致的题。
 *
 * stdout 一行一个 JSON：header（版本、协议、种子）、case（题目和每一步的答案）、end。
 * 日志和异常在 stderr。物品是 [注册表 ID, 数量, 组件补丁十六进制]，空槽是 [0, 0, ""]。
 */
public final class MenuOracle {
    /** 菜单的容器 ID。所有题都用 1，和 ServerPlayer.nextContainerCounter 的第一次相同。 */
    private static final int CONTAINER_ID = 1;
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final ContainerInput[] INPUTS = ContainerInput.values();
    private static final Unsafe UNSAFE = loadUnsafe();
    private static final Map<String, Field> FIELDS = new HashMap<>();
    private static RecipeManager RECIPES;
    private static RegistryAccess.Frozen REGISTRIES;
    private static ReloadableServerRegistries.Holder LOOT_TABLES;
    private static GameRules RULES;
    private static FakeServer SERVER;
    private static final Map<RecipeType<?>, List<RecipeHolder<?>>> BY_TYPE = new HashMap<>();

    private MenuOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        // 官方日志默认写 stdout。日志框架初始化时记下 System.out，要抢在这之前换掉。
        System.setOut(log);
        Options options = Options.parse(args);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        loadItemComponents();
        try (Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            new Oracle(options, out).run();
        }
    }

    /**
     * 物品的默认组件平时由加载数据包时绑定，ItemStack 的构造函数要求它已经发生。
     * 这里走和 WorldLoader 一样的资源加载流程，只做到绑定组件为止，不创建服务端也不加载维度。
     */
    private static void loadItemComponents() throws Exception {
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
                            Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                            ForkJoinPool.commonPool(), Runnable::run)
                    .get();
            managers.updateComponentsAndStaticRegistryTags();
            managers.getRecipeManager().finalizeRecipeLoading(FeatureFlags.DEFAULT_FLAGS);
            RECIPES = managers.getRecipeManager();
            REGISTRIES = layers.compositeAccess();
            LOOT_TABLES = managers.fullRegistries();
        }
        RULES = new GameRules(FeatureFlags.DEFAULT_FLAGS);
        SERVER = FakeServer.create(RECIPES, RULES);
        for (RecipeHolder<?> recipe : RECIPES.getRecipes()) {
            BY_TYPE.computeIfAbsent(recipe.value().getType(), key -> new ArrayList<>()).add(recipe);
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

    /** 不运行构造函数：真正的 ServerLevel 和 ServerPlayer 要一个完整的服务端才能构造。 */
    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static Field field(Class<?> owner, String name) {
        return FIELDS.computeIfAbsent(owner.getName() + '#' + name, key -> {
            try {
                Field field = owner.getDeclaredField(name);
                field.setAccessible(true);
                return field;
            } catch (NoSuchFieldException error) {
                throw new IllegalStateException(error);
            }
        });
    }

    private static void set(Class<?> owner, Object target, String name, Object value) {
        try {
            field(owner, name).set(target, value);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    private record Options(int cases, long seed, int only) {
        static Options parse(String[] args) {
            int cases = 200;
            long seed = 1;
            int only = -1;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--cases" -> cases = Integer.parseInt(args[++i]);
                    case "--seed" -> seed = Long.parseLong(args[++i]);
                    case "--case" -> only = Integer.parseInt(args[++i]);
                    default -> throw new IllegalArgumentException("unknown option " + args[i]);
                }
            }
            return new Options(cases, seed, only);
        }
    }

    private static final class Oracle {
        private final Options options;
        private final Writer out;

        Oracle(Options options, Writer out) {
            this.options = options;
            this.out = out;
        }

        void run() throws IOException {
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            header.addProperty("seed", options.seed());
            header.addProperty("cases", options.cases());
            write(header);

            int written = 0;
            for (int index = 0; index < options.cases(); index++) {
                if (options.only() >= 0 && index != options.only()) {
                    continue;
                }
                write(caseOf(index));
                written++;
            }
            if (options.only() < 0) {
                for (int index = 0; index < 12; index++) {
                    write(nautilusCase(written, index, options.seed()));
                    written++;
                }
                write(componentCase(written));
                written++;
                write(lootChestCase(written));
                written++;
                for (int index = 0; index < 24; index++) {
                    write(stationCase(written, index, options.seed()));
                    written++;
                }
                write(enchantSeedCase(written));
                written++;
                for (int index = 0; index < 6; index++) {
                    write(bundleCase(written, index));
                    written++;
                }
            }

            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            end.addProperty("cases", written);
            write(end);
        }

        private void write(JsonObject line) throws IOException {
            out.write(line.toString());
            out.write('\n');
        }

        private JsonObject caseOf(int id) {
            Random random = new Random(options.seed() * 1_000_003L + id);
            Shape shape = Shape.values()[random.nextInt(Shape.values().length)];
            if (shape == Shape.LECTERN) {
                return lecternCase(id, random);
            }
            if (shape == Shape.MERCHANT) {
                return merchantCase(id, random);
            }
            boolean creative = random.nextInt(4) != 0;
            FakePlayer player = FakePlayer.create(creative);
            Inventory inventory = player.getInventory();

            for (int i = 0, count = random.nextInt(10); i < count; i++) {
                inventory.setItem(inventorySlot(random), randomStack(random));
            }

            Container container = null;
            ItemStack[] grid = new ItemStack[shape.size];
            for (int i = 0; i < grid.length; i++) {
                grid[i] = ItemStack.EMPTY;
            }
            JsonArray data = new JsonArray();
            int[] machineData = new int[0];
            if (shape.workstation) {
                for (int i = 0, count = random.nextInt(grid.length + 1); i < count; i++) {
                    grid[random.nextInt(grid.length)] = randomStack(random);
                }
                if (random.nextInt(5) != 0) {
                    placeRecipe(shape, grid, random);
                }
                // 织布机和砂轮的随机物品栏放不进旗帜、染料和附魔，结果槽比不出图案和耐久。
                // Random inventory items never include banners, dyes, or enchantments, so the
                // result slot would not compare pattern layers or durability.
                decorateStation(shape, grid, inventory, random);
                if (shape.machineData) {
                    machineData = new int[4];
                    for (int i = 0; i < machineData.length; i++) {
                        machineData[i] = random.nextInt(400);
                        data.add(machineData[i]);
                    }
                } else if (shape == Shape.CRAFTER) {
                    // 九个禁用位和 triggered。真实世界里只有空格能禁用，这里故意让禁用格也可能
                    // 有东西，把 mayPlace 和结果格的行为一起比到。
                    machineData = new int[10];
                    for (int i = 0; i < 9; i++) {
                        machineData[i] = random.nextInt(3) == 0 ? 1 : 0;
                    }
                    machineData[9] = random.nextInt(2);
                    for (int value : machineData) {
                        data.add(value);
                    }
                }
            } else {
                container = new SimpleContainer(shape.size);
                for (int i = 0, count = random.nextInt(shape.size + 1); i < count; i++) {
                    container.setItem(random.nextInt(shape.size), randomStack(random));
                }
            }

            AbstractContainerMenu menu = shape.create(inventory, container, player);
            if (machineData.length > 0) {
                seedData(menu, machineData);
            }
            if (shape.workstation) {
                placeGrid(menu, shape, grid);
            }
            // 官方 ServerPlayer.initMenu：装同步器，sendAllDataToRemote 发出第一个 stateId。
            menu.setSynchronizer(new Sync());

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", shape.id);
            line.addProperty("creative", creative);
            line.add("container", shape.workstation ? stacks(grid) : items(container, shape.size));
            line.add("inventory", inventoryItems(inventory));
            line.add("data", data);
            if (shape.workstation) {
                JsonArray patches = gridPatches(grid);
                if (!patches.isEmpty()) {
                    line.add("patches", patches);
                }
            }
            line.add("open", outcome(menu));

            JsonArray steps = new JsonArray();
            for (int[] click : clickSequence(random, menu.slots.size())) {
                boolean menuButton = click.length > 3 && click[3] == 1;
                if (menuButton) {
                    // handleContainerButtonClick：不压住同步，接受了才再广播一次。
                    if (menu.clickMenuButton(player, click[1])) {
                        menu.broadcastChanges();
                    }
                } else {
                    // handleContainerClick 去掉 containerId、stillValid 和槽位校验之后的顺序。
                    menu.suppressRemoteUpdates();
                    menu.clicked(click[0], click[1], INPUTS[click[2]], player);
                    menu.setRemoteCarried(HashedStack.EMPTY);
                    menu.resumeRemoteUpdates();
                    menu.broadcastChanges();
                }

                JsonObject step = new JsonObject();
                step.addProperty("slot", click[0]);
                step.addProperty("button", click[1]);
                step.addProperty("input", click[2]);
                step.addProperty("menuButton", menuButton);
                step.add("outcome", outcome(menu));
                steps.add(step);
            }
            line.add("steps", steps);
            return line;
        }

        /**
         * 鹦鹉螺坐骑物品栏：官方 `NautilusInventoryMenu` 的两格（鞍、身体）
         * 和玩家 36 格。开局按题目的背包 / 装备填好，逐步点击后逐槽比。
         *
         * Nautilus mount inventory: vanilla `NautilusInventoryMenu`'s saddle /
         * body slots plus the player's 36 slots, with per-click slot comparison.
         */
        private JsonObject nautilusCase(int id, int variant, long baseSeed) {
            Random random = new Random(baseSeed * 9_000_019L + variant);
            boolean creative = variant % 3 == 0;
            FakePlayer player = FakePlayer.create(creative);
            Inventory inventory = player.getInventory();
            for (int i = 0, count = random.nextInt(10); i < count; i++) {
                inventory.setItem(inventorySlot(random), randomStack(random));
            }
            // 官方 `EntityTypes.NAUTILUS.create` 在这个假世界里需要注册表；
            // 直接调无参构造器，再手工放进一个能读写装备槽的世界。
            // `EntityTypes.NAUTILUS.create` needs a full fake level here, so
            // construct the mob directly and give it equipment through the
            // menu's own `createEquipmentSlotContainer`.
            Nautilus nautilus = new Nautilus(net.minecraft.world.entity.EntityTypes.NAUTILUS,
                    player.level());
            nautilus.setPos(0.5, 64.0, 0.5);
            // `TamableAnimal.tame` 要给玩家发进度；假服务器没有进度表，直接设
            // 驯服位和主人。菜单只关心 `canUseSlot` / `isTame()`。
            // `TamableAnimal.tame` awards an advancement; the fake server has
            // no advancement map, so set the tame flag and owner directly.
            nautilus.setTame(true, false);
            nautilus.setOwnerReference(net.minecraft.world.entity.EntityReference.of(player));
            if (variant % 2 == 0) {
                nautilus.setItemSlot(EquipmentSlot.SADDLE, new ItemStack(Items.SADDLE));
            }
            if (variant % 3 == 1) {
                nautilus.setItemSlot(EquipmentSlot.BODY,
                        new ItemStack(Items.IRON_NAUTILUS_ARMOR));
            }
            AbstractContainerMenu menu = new NautilusInventoryMenu(
                    CONTAINER_ID, inventory, new SimpleContainer(0), nautilus, 0);
            menu.setSynchronizer(new Sync());
            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", "nautilus_mount");
            line.addProperty("creative", creative);
            JsonArray mountItems = new JsonArray();
            if (variant % 2 == 0) {
                mountItems.add(idCount(new ItemStack(Items.SADDLE)));
            } else {
                mountItems.add(idCount(ItemStack.EMPTY));
            }
            if (variant % 3 == 1) {
                mountItems.add(idCount(new ItemStack(Items.IRON_NAUTILUS_ARMOR)));
            } else {
                mountItems.add(idCount(ItemStack.EMPTY));
            }
            line.add("container", mountItems);
            line.add("inventory", inventoryItems(inventory));
            line.add("data", new JsonArray());
            line.add("open", outcome(menu));
            JsonArray steps = new JsonArray();
            for (int[] click : clickSequence(random, menu.slots.size())) {
                menu.suppressRemoteUpdates();
                menu.clicked(click[0], click[1], INPUTS[click[2]], player);
                menu.setRemoteCarried(HashedStack.EMPTY);
                menu.resumeRemoteUpdates();
                menu.broadcastChanges();
                JsonObject step = new JsonObject();
                step.addProperty("slot", click[0]);
                step.addProperty("button", click[1]);
                step.addProperty("input", click[2]);
                step.addProperty("menuButton", false);
                step.add("outcome", outcome(menu));
                steps.add(step);
            }
            line.add("steps", steps);
            return line;
        }

        /**
         * 铁砧、制图台、附魔台的固定题。
         *
         * 铁砧的名字、附魔台的种子（进服时的 `enchantmentSeed`）和玩家的实体随机源都在题目里固定，
         * 这样官方和 Bamboo 的每槽、每个 stateId、每个 data 槽都能逐项比较。题目用
         * `ContainerLevelAccess.NULL`，所以制图台和附魔台的 `access.execute` 根本不跑。
         */
        private JsonObject stationCase(int id, int variant, long baseSeed) {
            Random random = new Random(baseSeed * 7_000_003L + variant);
            Shape shape = switch (variant % 3) {
                case 0 -> Shape.ANVIL;
                case 1 -> Shape.CARTOGRAPHY;
                default -> Shape.ENCHANTMENT;
            };
            boolean creative = variant % 2 == 0;
            int seed = variant * 31 + 17;
            // 固定题要覆盖不同等级（附魔/铁砧的等级判定）和不同书架数。
            // Fixed cases cover different levels and bookshelf counts.
            int xpLevel = (variant * 7 + 3) % 41;
            int bookshelves = shape == Shape.ENCHANTMENT ? (variant * 5) % 16 : 0;
            FakePlayer player = FakePlayer.create(seed, creative);
            set(Player.class, player, "experienceLevel", xpLevel);
            Inventory inventory = player.getInventory();
            ItemStack[] grid = new ItemStack[shape.size];
            for (int i = 0; i < grid.length; i++) {
                grid[i] = ItemStack.EMPTY;
            }
            if (shape == Shape.ANVIL) {
                grid[0] = anvilInput(variant);
                if (variant % 3 != 0) {
                    grid[1] = anvilAddition(variant);
                }
            } else if (shape == Shape.CARTOGRAPHY) {
                grid[0] = mapInput(variant);
                grid[1] = switch (variant % 4) {
                    case 0 -> new ItemStack(Items.PAPER, 1 + random.nextInt(4));
                    case 1 -> new ItemStack(Items.MAP, 1 + random.nextInt(4));
                    case 2 -> new ItemStack(Items.GLASS_PANE, 1 + random.nextInt(4));
                    default -> new ItemStack(Items.STICK, 1 + random.nextInt(4));
                };
            } else {
                grid[0] = variant % 2 == 0
                        ? new ItemStack(Items.BOOK, 1)
                        : new ItemStack(Items.DIAMOND_SWORD, 1);
                grid[1] = new ItemStack(Items.LAPIS_LAZULI, 1 + random.nextInt(8));
                inventory.setItem(random.nextInt(36), new ItemStack(Items.LAPIS_LAZULI, 3));
            }

            MapId mapId = new MapId(variant + 1);
            byte scale = (byte) (variant % 5);
            boolean locked = variant % 4 == 0;
            FakeLevel level = FakeLevel.create(seed, bookshelves, mapId, scale, locked);
            set(Entity.class, player, "level", level);
            ContainerLevelAccess access = ContainerLevelAccess.create(level, BlockPos.ZERO);
            AbstractContainerMenu menu = switch (shape) {
                case ANVIL -> new AnvilMenu(CONTAINER_ID, inventory, access);
                case CARTOGRAPHY -> new CartographyTableMenu(CONTAINER_ID, inventory, access);
                case ENCHANTMENT -> new EnchantmentMenu(CONTAINER_ID, inventory, access);
                default -> shape.create(inventory, null, player);
            };
            placeGrid(menu, shape, grid);
            menu.setSynchronizer(new Sync());

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", shape.id);
            line.addProperty("creative", creative);
            line.addProperty("seed", seed);
            line.addProperty("fixedWorld", true);
            line.addProperty("mapScale", scale);
            line.addProperty("mapLocked", locked);
            line.addProperty("bookshelves", bookshelves);
            line.addProperty("xpLevel", xpLevel);
            if (shape == Shape.ENCHANTMENT) {
                // `onEnchantmentPerformed` 每次成功都从玩家自己的随机源换种子；
                // 题目按点击顺序提前算出足够多的值。
                // Each successful enchant draws a new seed from the player's
                // random source; precompute enough values for the click sequence.
                net.minecraft.util.RandomSource playerRandom =
                        net.minecraft.util.RandomSource.create(seed);
                com.google.gson.JsonArray seeds = new com.google.gson.JsonArray();
                for (int i = 0; i < 32; i++) {
                    seeds.add(playerRandom.nextInt());
                }
                line.addProperty("nextSeed", seeds.get(0).getAsInt());
                line.add("nextSeeds", seeds);
            }
            line.add("container", stacks(grid));
            line.add("inventory", inventoryItems(inventory));
            line.add("data", dataSlots(menu));
            JsonArray patches = gridPatches(grid);
            if (!patches.isEmpty()) {
                line.add("patches", patches);
            }
            line.add("open", outcome(menu));

            JsonArray steps = new JsonArray();
            if (shape == Shape.ANVIL) {
                JsonObject rename = new JsonObject();
                rename.addProperty("slot", 0);
                rename.addProperty("button", 0);
                rename.addProperty("input", 0);
                rename.addProperty("menuButton", false);
                rename.addProperty("rename", anvilName(variant));
                ((AnvilMenu) menu).setItemName(anvilName(variant));
                rename.add("outcome", outcome(menu));
                steps.add(rename);
            }
            for (int[] click : clickSequence(random, menu.slots.size())) {
                boolean menuButton = click.length > 3 && click[3] == 1;
                if (menuButton) {
                    if (menu.clickMenuButton(player, click[1])) {
                        menu.broadcastChanges();
                    }
                } else {
                    menu.suppressRemoteUpdates();
                    menu.clicked(click[0], click[1], INPUTS[click[2]], player);
                    menu.setRemoteCarried(HashedStack.EMPTY);
                    menu.resumeRemoteUpdates();
                    menu.broadcastChanges();
                }
                JsonObject step = new JsonObject();
                step.addProperty("slot", click[0]);
                step.addProperty("button", click[1]);
                step.addProperty("input", click[2]);
                step.addProperty("menuButton", menuButton);
                step.add("outcome", outcome(menu));
                steps.add(step);
            }
            line.add("steps", steps);
            return line;
        }

        /**
         * 两次连续成功的附魔：官方每次 `onEnchantmentPerformed` 都换
         * `enchantmentSeed`，重开菜单的选项必须跟着新种子。断点放在
         * `data[SEED_SLOT]` 上，不换种子就会被对拍抓到。
         *
         * Two successful enchants in a row: each `onEnchantmentPerformed`
         * changes `enchantmentSeed`, so the next menu's options follow the
         * new seed. Reusing the old seed is caught by the data-slot compare.
         */
        private static JsonObject enchantSeedCase(int id) {
            int seed = 991;
            int xpLevel = 40;
            int bookshelves = 15;
            FakePlayer player = FakePlayer.create(seed, false);
            set(Player.class, player, "experienceLevel", xpLevel);
            Inventory inventory = player.getInventory();
            ItemStack[] grid = {new ItemStack(Items.DIAMOND_SWORD), new ItemStack(Items.LAPIS_LAZULI, 8)};
            MapId mapId = new MapId(id + 1);
            FakeLevel level = FakeLevel.create(seed, bookshelves, mapId, (byte) 0, false);
            set(Entity.class, player, "level", level);
            EnchantmentMenu menu = new EnchantmentMenu(
                    CONTAINER_ID, inventory, ContainerLevelAccess.create(level, BlockPos.ZERO));
            placeGrid(menu, Shape.ENCHANTMENT, grid);
            menu.setSynchronizer(new Sync());

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", Shape.ENCHANTMENT.id);
            line.addProperty("creative", false);
            line.addProperty("seed", seed);
            line.addProperty("fixedWorld", true);
            line.addProperty("mapScale", 0);
            line.addProperty("mapLocked", false);
            line.addProperty("bookshelves", bookshelves);
            line.addProperty("xpLevel", xpLevel);
            net.minecraft.util.RandomSource playerRandom =
                    net.minecraft.util.RandomSource.create(seed);
            JsonArray seeds = new JsonArray();
            for (int i = 0; i < 8; i++) {
                seeds.add(playerRandom.nextInt());
            }
            line.addProperty("nextSeed", seeds.get(0).getAsInt());
            line.add("nextSeeds", seeds);
            line.add("container", stacks(grid));
            line.add("inventory", inventoryItems(inventory));
            line.add("data", dataSlots(menu));
            line.add("open", outcome(menu));

            JsonArray steps = new JsonArray();
            for (int button : new int[] {0, 1}) {
                menu.clickMenuButton(player, button);
                menu.broadcastChanges();
                JsonObject step = new JsonObject();
                step.addProperty("slot", 0);
                step.addProperty("button", button);
                step.addProperty("input", 0);
                step.addProperty("menuButton", true);
                step.add("outcome", outcome(menu));
                steps.add(step);
            }
            line.add("steps", steps);
            return line;
        }

        private static String anvilName(int variant) {
            return switch (variant % 4) {
                case 0 -> "Blade";
                case 1 -> "";
                case 2 -> "A very long name that is still under fifty characters";
                default -> "a\u007fb";
            };
        }

        /** 后面的铁砧题把 repair_cost 抬到 40 以上，覆盖「过于昂贵」和创造模式放行。 */
        private static ItemStack anvilInput(int variant) {
            ItemStack stack = switch (variant % 4) {
                case 0 -> new ItemStack(Items.DIAMOND_SWORD, 1);
                case 1 -> new ItemStack(Items.IRON_PICKAXE, 1);
                case 2 -> new ItemStack(Items.BOOK, 1);
                default -> new ItemStack(Items.DIAMOND_CHESTPLATE, 1);
            };
            if (variant % 3 == 0) {
                stack.setDamageValue(20 + variant);
            }
            if (variant % 5 == 0) {
                ItemEnchantments.Mutable mutable = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
                mutable.set(
                        REGISTRIES.lookupOrThrow(Registries.ENCHANTMENT).getOrThrow(Enchantments.SHARPNESS),
                        1 + variant % 3);
                stack.set(DataComponents.ENCHANTMENTS, mutable.toImmutable());
            }
            if (variant >= 12) {
                stack.set(DataComponents.REPAIR_COST, 45 + variant % 5);
            }
            return stack;
        }

        private static ItemStack anvilAddition(int variant) {
            if (variant % 3 == 1) {
                ItemStack book = new ItemStack(Items.ENCHANTED_BOOK, 1);
                ItemEnchantments.Mutable mutable = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
                mutable.set(
                        REGISTRIES.lookupOrThrow(Registries.ENCHANTMENT).getOrThrow(Enchantments.SHARPNESS),
                        1 + variant % 2);
                book.set(DataComponents.STORED_ENCHANTMENTS, mutable.toImmutable());
                return book;
            }
            return switch (variant % 3) {
                case 0 -> new ItemStack(Items.DIAMOND, 1 + variant % 3);
                default -> new ItemStack(Items.IRON_PICKAXE, 1);
            };
        }

        private static ItemStack mapInput(int variant) {
            ItemStack map = new ItemStack(Items.FILLED_MAP, 1 + variant % 3);
            map.set(DataComponents.MAP_ID, new MapId(variant + 1));
            return map;
        }

        /**
         * 收纳袋的菜单点击。每次点击后比全部 27 + 36 个槽位、光标和 state_id；
         * 带组件的物品按官方 `DataComponentPatch.STREAM_CODEC` 逐字节写成十六进制。
         *
         * Bundle menu clicks. Every step compares all slots, the cursor and the state id;
         * component patches are the official network bytes in hex.
         */
        private JsonObject bundleCase(int id, int variant) {
            Shape shape = Shape.CHEST_3;
            FakePlayer player = FakePlayer.create(false);
            Container container = new SimpleContainer(shape.size);
            // 0：空收纳袋；1：装了 3 件；2：几乎装满的铁锭；3：选中格；4：嵌套收纳袋；5：满袋再装。
            ItemStack empty = new ItemStack(Items.BUNDLE);
            ItemStack filled = bundleWith(Items.STONE, 3, Items.DIRT, 2, Items.APPLE, 1);
            ItemStack nearlyFull = bundleWith(Items.IRON_INGOT, 62);
            ItemStack full = bundleWith(Items.IRON_INGOT, 64);
            // 内层的 bundle_contents 要真的带上内容，否则外层只占 1/16。
            ItemStack inner = bundleWith(Items.STONE, 16);
            ItemStack nested = bundleWith(inner);
            switch (variant) {
                case 0 -> {
                    container.setItem(0, empty);
                    container.setItem(1, new ItemStack(Items.STONE, 16));
                }
                case 1 -> {
                    container.setItem(0, filled);
                    container.setItem(1, new ItemStack(Items.DIRT, 4));
                }
                case 2 -> {
                    container.setItem(0, nearlyFull);
                    container.setItem(1, new ItemStack(Items.IRON_INGOT, 8));
                }
                case 3 -> {
                    container.setItem(0, filled);
                    container.setItem(2, new ItemStack(Items.STONE, 1));
                }
                case 4 -> {
                    container.setItem(0, empty);
                    container.setItem(1, nested);
                }
                default -> {
                    container.setItem(0, full);
                    container.setItem(1, new ItemStack(Items.STONE, 1));
                }
            }
            AbstractContainerMenu menu = shape.create(player.getInventory(), container, player);
            menu.setSynchronizer(new Sync());

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", shape.id);
            line.addProperty("creative", false);
            line.add("container", bundleItems(container));
            line.add("inventory", inventoryItems(player.getInventory()));
            JsonArray patches = new JsonArray();
            for (int slot = 0; slot < shape.size; slot++) {
                ItemStack stack = container.getItem(slot);
                if (!stack.isEmpty() && stack.has(DataComponents.BUNDLE_CONTENTS)) {
                    JsonObject patch = new JsonObject();
                    patch.addProperty("index", slot);
                    patch.addProperty("bytes", hex(patchBytes(
                            RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY),
                            stack.getComponentsPatch())));
                    patches.add(patch);
                }
            }
            line.add("patches", patches);
            line.add("open", outcome(menu));

            JsonArray steps = new JsonArray();
            switch (variant) {
                case 0 -> {
                    // 左键拿起收纳袋，左键点石头装进去，左键放回，空光标右键取出。
                    steps.add(bundleClick(menu, player, 0, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 1, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 0, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 0, 1, 0, -1, -1));
                }
                case 1 -> {
                    // 光标上有 4 个泥土，左键装进袋；空光标右键取出；再右键点空槽。
                    steps.add(bundleClick(menu, player, 1, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 0, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 0, 1, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 0, 0, 0, -1, -1));
                }
                case 2 -> {
                    // 62/64 再装 8 个只能装 2 个；左键把剩下的放回槽里。
                    steps.add(bundleClick(menu, player, 1, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 0, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 1, 0, 0, -1, -1));
                }
                case 3 -> {
                    // 选中第 2 件 → 把收纳袋拿到光标上 → 放到别的格 → 再取出，
                    // 取出的必须仍是选中的那一件（泥土）。
                    steps.add(bundleClick(menu, player, 0, 0, 0, 0, 1));
                    steps.add(bundleClick(menu, player, 0, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 2, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 2, 1, 0, -1, -1));
                }
                case 4 -> {
                    // 嵌套收纳袋：空格拿空袋 → 左键点嵌套袋装进去（+1/16）→ 右键取出。
                    steps.add(bundleClick(menu, player, 0, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 1, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 0, 1, 0, -1, -1));
                }
                default -> {
                    // 满袋再装：装不进去，播失败音效，袋和槽里的物品都不变。
                    steps.add(bundleClick(menu, player, 1, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 0, 0, 0, -1, -1));
                    steps.add(bundleClick(menu, player, 1, 1, 0, -1, -1));
                }
            }
            line.add("steps", steps);
            return line;
        }

        /** 造一个 `bundle_contents` 里装了给定物品的收纳袋。参数要么是成对的 (Item, Integer)，
         * 要么是一件带组件的 ItemStack（嵌套收纳袋）。 */
        private static ItemStack bundleWith(Object... entries) {
            java.util.List<ItemStackTemplate> items = new ArrayList<>();
            if (entries.length == 1 && entries[0] instanceof ItemStack stack) {
                items.add(ItemStackTemplate.fromNonEmptyStack(stack));
            } else {
                for (int index = 0; index < entries.length; index += 2) {
                    items.add(new ItemStackTemplate((Item) entries[index], (Integer) entries[index + 1]));
                }
            }
            ItemStack stack = new ItemStack(Items.BUNDLE);
            stack.set(DataComponents.BUNDLE_CONTENTS, new BundleContents(items));
            return stack;
        }

        /** 初始容器：带 `bundle_contents` 的格子写组件补丁，其余只写物品和数量。 */
        private static JsonArray bundleItems(Container container) {
            JsonArray items = new JsonArray();
            for (int slot = 0; slot < container.getContainerSize(); slot++) {
                ItemStack stack = container.getItem(slot);
                if (!stack.isEmpty() && stack.has(DataComponents.BUNDLE_CONTENTS)) {
                    JsonArray entry = new JsonArray();
                    entry.add(BuiltInRegistries.ITEM.getId(stack.getItem()));
                    entry.add(stack.getCount());
                    entry.add(patchHex(stack));
                    items.add(entry);
                } else {
                    items.add(idCount(stack));
                }
            }
            return items;
        }

        /** 可选地在点击前发一次 `ServerboundSelectBundleItemPacket` 的等价入口。 */
        private static JsonObject bundleClick(
                AbstractContainerMenu menu,
                FakePlayer player,
                int slot,
                int button,
                int input,
                int selectSlot,
                int selectIndex) {
            if (selectSlot >= 0) {
                menu.setSelectedBundleItemIndex(selectSlot, selectIndex);
            }
            menu.suppressRemoteUpdates();
            menu.clicked(slot, button, INPUTS[input], player);
            menu.setRemoteCarried(HashedStack.EMPTY);
            menu.resumeRemoteUpdates();
            menu.broadcastChanges();
            JsonObject step = new JsonObject();
            step.addProperty("slot", slot);
            step.addProperty("button", button);
            step.addProperty("input", input);
            if (selectSlot >= 0) {
                JsonArray select = new JsonArray();
                select.add(selectSlot);
                select.add(selectIndex);
                JsonArray selections = new JsonArray();
                selections.add(select);
                step.add("selectBundle", selections);
            }
            step.add("outcome", outcome(menu));
            return step;
        }

        /**
         * 打开一个带 `LootTable` 的箱子：官方 `createMenu` 在发初始槽位之前用
         * `LootTable.fill` 把内容填好。用非零种子固定随机源，Bamboo 那边在
         * `Replay::new` 收到的是同一份填好的内容。
         *
         * Open a chest with a `LootTable`: vanilla `createMenu` fills it before
         * sending the initial slots. A non-zero seed fixes the random source, and
         * Bamboo's `Replay` receives the same filled contents.
         */
        private JsonObject lootChestCase(int id) {
            Shape shape = Shape.CHEST_3;
            FakePlayer player = FakePlayer.create(false);
            net.minecraft.world.SimpleContainer container = new net.minecraft.world.SimpleContainer(shape.size);
            // 官方 `RandomizableContainerBlockEntity.createMenu` 在这里解包；本 oracle
            // 用同一张战利品表和种子调 `LootTable.fill`，把内容交给两边比较。
            // Vanilla `createMenu` unpacks here; this oracle fills the same table and
            // seed into the container so both sides compare the filled slots.
            LootTable table = LOOT_TABLES.getLootTable(
                    ResourceKey.create(Registries.LOOT_TABLE,
                            net.minecraft.resources.Identifier.parse("minecraft:chests/simple_dungeon")));
            LootParams params = new LootParams.Builder(player.level())
                    .withParameter(LootContextParams.ORIGIN, new Vec3(0.5, 0.5, 0.5))
                    .withLuck(0.0F)
                    .create(LootContextParamSets.CHEST);
            table.fill(container, params, 424242L);
            AbstractContainerMenu menu = shape.create(player.getInventory(), container, player);
            menu.setSynchronizer(new RecordingSync(component -> 0));

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", shape.id);
            line.addProperty("creative", false);
            line.addProperty("lootChest", true);
            line.add("container", items(container, shape.size));
            line.add("inventory", inventoryItems(player.getInventory()));
            line.add("open", outcome(menu));
            line.add("steps", new JsonArray());
            return line;
        }

        /**
         * 带组件的钻石剑：客户端预测后回传哈希。补发列表空，说明两边的哈希一致。
         *
         * A diamond sword with components. The client sends hashes after predicting.
         * An empty resend list means both hashes agree.
         */
        private JsonObject componentCase(int id) {
            RegistryAccess.Frozen access = RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);
            RegistryOps<HashCode> hashOps = access.createSerializationContext(HashOps.CRC32C_INSTANCE);
            HashedPatchMap.HashGenerator hasher =
                    component -> hashComponent(hashOps, component);
            Shape shape = Shape.CHEST_3;
            FakePlayer player = FakePlayer.create(false);
            Container container = new SimpleContainer(shape.size);
            ItemStack sword = new ItemStack(Items.DIAMOND_SWORD);
            sword.set(DataComponents.CUSTOM_NAME, Component.literal("oracle"));
            sword.set(DataComponents.DAMAGE, 3);
            sword.set(DataComponents.ENCHANTMENT_GLINT_OVERRIDE, true);
            if (sword.has(DataComponents.ATTRIBUTE_MODIFIERS)) {
                sword.remove(DataComponents.ATTRIBUTE_MODIFIERS);
            }
            container.setItem(0, sword);
            container.setItem(1, new ItemStack(Items.STONE, 16));

            RecordingSync sync = new RecordingSync(hasher);
            AbstractContainerMenu menu = shape.create(player.getInventory(), container, player);
            menu.setSynchronizer(sync);

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", shape.id);
            line.addProperty("creative", false);
            line.addProperty("predict", true);
            line.add("container", items(container, shape.size));
            line.add("inventory", inventoryItems(player.getInventory()));
            line.add("open", outcome(menu));
            JsonArray patches = new JsonArray();
            JsonObject patch = new JsonObject();
            patch.addProperty("index", 0);
            patch.addProperty("bytes", hex(patchBytes(access, sword.getComponentsPatch())));
            patches.add(patch);
            line.add("patches", patches);

            JsonArray steps = new JsonArray();
            steps.add(predictingClick(menu, 0, 0, 0, player, sync, hasher));
            steps.add(predictingClick(menu, 2, 0, 0, player, sync, hasher));
            steps.add(predictingClick(menu, 1, 0, 0, player, sync, hasher));
            line.add("steps", steps);
            return line;
        }

        private static int hashComponent(RegistryOps<HashCode> hashOps, TypedDataComponent<?> component) {
            return component.encodeValue(hashOps).getOrThrow(IllegalStateException::new).asInt();
        }

        private static JsonObject predictingClick(
                AbstractContainerMenu menu,
                int slot,
                int button,
                int input,
                FakePlayer player,
                RecordingSync sync,
                HashedPatchMap.HashGenerator hasher) {
            List<ItemStack> before = new ArrayList<>();
            for (Slot menuSlot : menu.slots) {
                before.add(menuSlot.getItem().copy());
            }
            menu.suppressRemoteUpdates();
            menu.clicked(slot, button, INPUTS[input], player);
            for (int index = 0; index < menu.slots.size(); index++) {
                ItemStack after = menu.slots.get(index).getItem();
                if (!ItemStack.matches(before.get(index), after)) {
                    menu.setRemoteSlotUnsafe(index, HashedStack.create(after, hasher));
                }
            }
            menu.setRemoteCarried(HashedStack.create(menu.getCarried(), hasher));
            menu.resumeRemoteUpdates();
            sync.reset();
            menu.broadcastChanges();

            JsonObject step = new JsonObject();
            step.addProperty("slot", slot);
            step.addProperty("button", button);
            step.addProperty("input", input);
            step.add("outcome", outcome(menu));
            JsonArray resent = new JsonArray();
            for (int resentSlot : sync.slots) {
                resent.add(resentSlot);
            }
            step.add("resent", resent);
            step.addProperty("carried_resent", sync.carried);
            return step;
        }

        private static byte[] patchBytes(RegistryAccess registries, DataComponentPatch patch) {
            RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), registries);
            DataComponentPatch.STREAM_CODEC.encode(buffer, patch);
            byte[] bytes = new byte[buffer.readableBytes()];
            buffer.readBytes(bytes);
            return bytes;
        }

        private static String hex(byte[] bytes) {
            StringBuilder text = new StringBuilder(bytes.length * 2);
            for (byte value : bytes) {
                text.append(Character.forDigit((value >> 4) & 0xf, 16));
                text.append(Character.forDigit(value & 0xf, 16));
            }
            return text.toString();
        }

        /**
         * 商人菜单：一组固定交易、把付款物品放进玩家背包，然后选交易 / 放物品 / 取结果 / Shift 点击。
         * 交易表里 `itemB == 0` 表示没有第二格付款，和 Rust 侧的 `[itemA, countA, itemB, countB, result, resultCount, maxUses]` 对齐。
         * 三种变体：付款够做 1 次、够做 3 次、背包快满（只留一个空格）。
         */
        private JsonObject merchantCase(int id, Random random) {
            boolean creative = random.nextInt(4) != 0;
            FakePlayer player = FakePlayer.create(creative);
            Inventory inventory = player.getInventory();

            int emerald = BuiltInRegistries.ITEM.getId(Items.EMERALD);
            int apple = BuiltInRegistries.ITEM.getId(Items.APPLE);
            int dirt = BuiltInRegistries.ITEM.getId(Items.DIRT);

            MerchantOffers offers = new MerchantOffers();
            offers.add(new MerchantOffer(
                    new ItemCost(Items.EMERALD, 1), Optional.empty(), new ItemStack(Items.APPLE),
                    0, 12, 1, 0.0f));

            FakeMerchant merchant = new FakeMerchant(offers);
            MerchantMenu menu = new MerchantMenu(CONTAINER_ID, inventory, merchant);

            int variant = id % 3;
            // 付款数量：1 次 / 3 次。背包快满时只留一个空格给结果。
            int emeralds = variant == 1 ? 3 : 1;
            inventory.setItem(9, new ItemStack(Items.EMERALD, emeralds));
            if (variant == 2) {
                for (int slot = 10; slot < 35; slot++) {
                    inventory.setItem(slot, new ItemStack(Items.DIRT, 64));
                }
            }
            // 先填背包再装同步器，`sendAllDataToRemote` 把填好的槽都记进 remoteSlots。
            menu.setSynchronizer(new Sync());

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", "merchant");
            line.addProperty("creative", creative);
            line.add("container", emptyContainer(3));
            line.add("inventory", inventoryItems(inventory));
            JsonArray offersJson = new JsonArray();
            offersJson.add(offerJson(emerald, 1, 0, 0, apple, 1, 12));
            line.add("offers", offersJson);
            line.add("open", outcome(menu));

            JsonArray steps = new JsonArray();
            // 选交易 0：自动把绿宝石填进付款格，结果格显示苹果。
            steps.add(tradeStep(menu, 0));
            // 结果格 Shift 取出：连做多笔，直到付款不够或背包放不下。
            steps.add(clickStep(menu, player, 2, 0, 1));
            line.add("steps", steps);
            return line;
        }

        private static JsonArray emptyContainer(int size) {
            JsonArray container = new JsonArray();
            for (int i = 0; i < size; i++) {
                JsonArray empty = new JsonArray();
                empty.add(0);
                empty.add(0);
                container.add(empty);
            }
            return container;
        }

        private static JsonArray offerJson(int a, int ca, int b, int cb, int result, int cr, int uses) {
            JsonArray offer = new JsonArray();
            offer.add(a);
            offer.add(ca);
            offer.add(b);
            offer.add(cb);
            offer.add(result);
            offer.add(cr);
            offer.add(uses);
            return offer;
        }

        private JsonObject tradeStep(MerchantMenu menu, int slot) {
            menu.setSelectionHint(slot);
            menu.tryMoveItems(slot);
            menu.broadcastChanges();
            JsonObject step = new JsonObject();
            step.addProperty("slot", 0);
            step.addProperty("button", 0);
            step.addProperty("input", 0);
            step.addProperty("selectTrade", slot);
            step.add("outcome", outcome(menu));
            return step;
        }

        private JsonObject clickStep(AbstractContainerMenu menu, FakePlayer player, int slot, int button, int input) {
            menu.clicked(slot, button, INPUTS[input], player);
            menu.broadcastChanges();
            JsonObject step = new JsonObject();
            step.addProperty("slot", slot);
            step.addProperty("button", button);
            step.addProperty("input", input);
            step.add("outcome", outcome(menu));
            return step;
        }

        /**
         * 官方 {@link LecternMenu} 的按钮。不创建 {@code LecternBlockEntity}：它的 {@code setPage} 会改方块，
         * {@code stillValid} 没有世界时永远是假。页码夹取和 {@code removeItemNoUpdate} 清页写在旁边的容器里。
         */
        private JsonObject lecternCase(int id, Random random) {
            boolean creative = random.nextInt(4) != 0;
            boolean mayBuild = random.nextInt(5) != 0;
            int pageCount = random.nextInt(6);
            boolean written = random.nextBoolean();
            FakePlayer player = FakePlayer.create(creative, mayBuild);
            LecternSlots lectern = new LecternSlots();
            lectern.setBook(bookStack(written, pageCount));
            AbstractContainerMenu menu = new LecternMenu(CONTAINER_ID, lectern, lectern.data);
            menu.setSynchronizer(new Sync());

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", "lectern");
            line.addProperty("creative", creative);
            line.addProperty("mayBuild", mayBuild);
            line.addProperty("pageCount", pageCount);
            line.add("container", items(lectern, 1));
            line.add("inventory", inventoryItems(player.getInventory()));
            line.add("open", outcome(menu, lectern));

            JsonArray steps = new JsonArray();
            int count = 2 + random.nextInt(18);
            for (int i = 0; i < count; i++) {
                int buttonId = lecternButton(random, pageCount);
                if (menu.stillValid(player) && menu.clickMenuButton(player, buttonId)) {
                    menu.broadcastChanges();
                }
                JsonObject step = new JsonObject();
                step.addProperty("slot", 0);
                step.addProperty("button", 0);
                step.addProperty("input", 0);
                step.addProperty("buttonId", buttonId);
                step.add("outcome", outcome(menu, lectern));
                steps.add(step);
            }
            line.add("steps", steps);
            return line;
        }
    }

    /** 菜单的形状。名字和 Rust 的 ReplayShape 对应。 */
    private enum Shape {
        CHEST_3(27, "chest3", false, false),
        CHEST_6(54, "chest6", false, false),
        HOPPER(5, "hopper", false, false),
        DISPENSER(9, "dispenser", false, false),
        SHULKER(27, "shulker", false, false),
        LECTERN(1, "lectern", false, false),
        INVENTORY(4, "inventory", true, false),
        CRAFTING(9, "crafting", true, false),
        CRAFTER(9, "crafter", true, false),
        STONECUTTER(1, "stonecutter", true, false),
        SMITHING(3, "smithing", true, false),
        FURNACE(3, "furnace", true, true),
        BLAST_FURNACE(3, "blast_furnace", true, true),
        SMOKER(3, "smoker", true, true),
        BREWING(5, "brewing", true, true),
        LOOM(3, "loom", true, false),
        GRINDSTONE(2, "grindstone", true, false),
        ANVIL(2, "anvil", true, false),
        CARTOGRAPHY(2, "cartography", true, false),
        ENCHANTMENT(2, "enchantment", true, false),
        MERCHANT(3, "merchant", false, false);

        final int size;
        final String id;
        final boolean workstation;
        final boolean machineData;

        Shape(int size, String id, boolean workstation, boolean machineData) {
            this.size = size;
            this.id = id;
            this.workstation = workstation;
            this.machineData = machineData;
        }

        int seedOffset() {
            return this == INVENTORY || this == CRAFTING ? 1 : 0;
        }

        int gridWidth() {
            return this == INVENTORY ? 2 : this == CRAFTING || this == CRAFTER ? 3 : 0;
        }

        AbstractContainerMenu create(Inventory inventory, Container container, FakePlayer player) {
            return switch (this) {
                case CHEST_3 -> new ChestMenu(MenuType.GENERIC_9x3, CONTAINER_ID, inventory, container, 3);
                case CHEST_6 -> new ChestMenu(MenuType.GENERIC_9x6, CONTAINER_ID, inventory, container, 6);
                case HOPPER -> new HopperMenu(CONTAINER_ID, inventory, container);
                case DISPENSER -> new DispenserMenu(CONTAINER_ID, inventory, container);
                case SHULKER -> new ShulkerBoxMenu(CONTAINER_ID, inventory, container);
                case LECTERN -> throw new IllegalStateException("lectern cases build their own menu");
                case MERCHANT -> throw new IllegalStateException("merchant cases build their own menu");
                case INVENTORY -> new InventoryMenu(inventory, true, player);
                case CRAFTING -> new CraftingMenu(CONTAINER_ID, inventory, ContainerLevelAccess.create(player.level(), BlockPos.ZERO));
                // 合成器的九格是 3x3 的 CraftingContainer，ContainerData 有 9 个禁用位和 1 个 triggered。
                case CRAFTER -> crafterMenu(inventory);
                case STONECUTTER -> new StonecutterMenu(CONTAINER_ID, inventory);
                case SMITHING -> new SmithingMenu(CONTAINER_ID, inventory);
                case FURNACE -> new FurnaceMenu(CONTAINER_ID, inventory);
                case BLAST_FURNACE -> new BlastFurnaceMenu(CONTAINER_ID, inventory);
                case SMOKER -> new SmokerMenu(CONTAINER_ID, inventory);
                case BREWING -> new BrewingStandMenu(CONTAINER_ID, inventory);
                case LOOM -> new LoomMenu(CONTAINER_ID, inventory);
                case GRINDSTONE -> new GrindstoneMenu(CONTAINER_ID, inventory);
                case ANVIL -> new AnvilMenu(CONTAINER_ID, inventory);
                case CARTOGRAPHY -> new CartographyTableMenu(CONTAINER_ID, inventory);
                case ENCHANTMENT -> new EnchantmentMenu(CONTAINER_ID, inventory);
            };
        }

        /** 把 TransientCraftingContainer 的 menu 指回真正的菜单，设置格子时不会空指针。 */
        private static AbstractContainerMenu crafterMenu(Inventory inventory) {
            CrafterMenu menu = new CrafterMenu(CONTAINER_ID, inventory,
                    new TransientCraftingContainer(null, 3, 3), new SimpleContainerData(10));
            set(TransientCraftingContainer.class, menu.getContainer(), "menu", menu);
            return menu;
        }
    }

    /**
     * 出题用的物品，包括只能叠一个的工具和放不进潜影盒的潜影盒。
     * 放在嵌套类里，第一次用到时才初始化：物品注册表要等 Bootstrap.bootStrap 之后才碰得。
     */
    private static final class Palette {
        static final Item[] ITEMS = {
            Items.STONE,
            Items.DIRT,
            Items.STICK,
            Items.OAK_LOG,
            Items.APPLE,
            Items.ENDER_PEARL,
            Items.SNOWBALL,
            Items.DIAMOND_SWORD,
            Items.SHULKER_BOX,
        };

        private Palette() {
        }
    }

    private static ItemStack randomStack(Random random) {
        Item item = Palette.ITEMS[random.nextInt(Palette.ITEMS.length)];
        int max = new ItemStack(item).getMaxStackSize();
        int count = random.nextInt(4) == 0 ? max : 1 + random.nextInt(max);
        return new ItemStack(item, count);
    }

    /** 玩家物品栏里随机一格：官方 Inventory 的 0 到 35 是快捷栏和背包，40 是副手。 */
    private static int inventorySlot(Random random) {
        int slot = random.nextInt(37);
        return slot == 36 ? 40 : slot;
    }

    /**
     * 一串随机点击，偶尔夹一段完整的拖动：开始、往几格里加、结束。
     * 其它点击的类型按大致的比例抽，槽位偶尔是 -999（丢出）和 -1（点在菜单外）。
     */
    private static List<int[]> clickSequence(Random random, int slots) {
        List<int[]> clicks = new ArrayList<>();
        int count = 2 + random.nextInt(18);
        while (clicks.size() < count) {
            int roll = random.nextInt(100);
            if (roll < 8) {
                clicks.add(new int[]{0, random.nextInt(12) - 1, 0, 1});
            } else if (roll < 20) {
                drag(random, slots, clicks);
            } else {
                clicks.add(single(random, slots));
            }
        }
        return clicks;
    }

    private static void drag(Random random, int slots, List<int[]> clicks) {
        int type = random.nextInt(3);
        clicks.add(new int[]{random.nextInt(slots), type << 2, 5});
        for (int i = 0, adds = 1 + random.nextInt(5); i < adds; i++) {
            clicks.add(new int[]{random.nextInt(slots), (type << 2) | 1, 5});
        }
        clicks.add(new int[]{random.nextInt(slots), (type << 2) | 2, 5});
    }

    private static int[] single(Random random, int slots) {
        int roll = random.nextInt(100);
        if (roll < 35) {
            return new int[]{outside(random, slots), random.nextInt(100) < 70 ? 0 : 1, 0};
        }
        if (roll < 55) {
            return new int[]{outside(random, slots), random.nextInt(100) < 70 ? 0 : 1, 1};
        }
        if (roll < 70) {
            int button = random.nextInt(10);
            return new int[]{random.nextInt(slots), button == 9 ? 40 : button, 2};
        }
        if (roll < 77) {
            return new int[]{random.nextInt(slots), random.nextInt(3), 3};
        }
        if (roll < 90) {
            return new int[]{random.nextInt(slots), random.nextInt(2), 4};
        }
        if (roll < 97) {
            return new int[]{random.nextInt(slots), random.nextInt(2), 6};
        }
        // 不完整的拖动，两边都该把它重置掉。
        return new int[]{random.nextInt(slots), random.nextInt(12), 5};
    }

    /** 拾取和快速移动除了菜单里的槽位，也可能点在菜单外（-1）或者把光标上的东西丢出去（-999）。 */
    private static int outside(Random random, int slots) {
        int roll = random.nextInt(100);
        if (roll < 6) {
            return -999;
        }
        if (roll < 10) {
            return -1;
        }
        return random.nextInt(slots);
    }

    /** 一方块的容器内容。 */
    private static JsonArray items(Container container, int size) {
        JsonArray items = new JsonArray();
        for (int slot = 0; slot < size; slot++) {
            items.add(idCount(container.getItem(slot)));
        }
        return items;
    }

    /** 玩家的 41 格：Official Inventory.getItem 对 36 到 40 返回盔甲和副手。 */
    private static JsonArray inventoryItems(Inventory inventory) {
        JsonArray items = new JsonArray();
        for (int slot = 0; slot <= 40; slot++) {
            items.add(idCount(inventory.getItem(slot)));
        }
        return items;
    }

    private static JsonObject outcome(AbstractContainerMenu menu) {
        return outcome(menu, null);
    }

    private static JsonObject outcome(AbstractContainerMenu menu, LecternSlots lectern) {
        JsonObject outcome = new JsonObject();
        outcome.addProperty("state", menu.getStateId());
        outcome.add("carried", pair(menu.getCarried()));
        JsonArray slots = new JsonArray();
        for (Slot slot : menu.slots) {
            slots.add(pair(slot.getItem()));
        }
        outcome.add("slots", slots);
        outcome.add("data", dataSlots(menu));
        if (lectern != null) {
            outcome.addProperty("page", lectern.page);
        }
        return outcome;
    }

    /** {@code LecternMenu} 的按钮。100 以上是跳页，3 是取书。 */
    private static int lecternButton(Random random, int pageCount) {
        int[] pool = {
            1, 2, 3, 0, 4, 99, 100, 101, 102, 104, 100 + pageCount, Integer.MAX_VALUE, Integer.MIN_VALUE, -50,
        };
        return pool[random.nextInt(pool.length)];
    }

    /** 讲台上的那一本。页数为 0 的成书仍带着组件，{@code hasBook} 才是真。 */
    private static ItemStack bookStack(boolean written, int pageCount) {
        ItemStack stack = new ItemStack(written ? Items.WRITTEN_BOOK : Items.WRITABLE_BOOK);
        if (written) {
            List<Filterable<Component>> pages = new ArrayList<>();
            for (int i = 0; i < pageCount; i++) {
                pages.add(Filterable.passThrough(Component.literal("p" + i)));
            }
            stack.set(
                    DataComponents.WRITTEN_BOOK_CONTENT,
                    new WrittenBookContent(Filterable.passThrough("Title"), "Author", 0, pages, true));
        } else if (pageCount > 0) {
            List<Filterable<String>> pages = new ArrayList<>();
            for (int i = 0; i < pageCount; i++) {
                pages.add(Filterable.passThrough("p" + i));
            }
            stack.set(DataComponents.WRITABLE_BOOK_CONTENT, new WritableBookContent(pages));
        }
        return stack;
    }

    /**
     * {@code LecternBlockEntity} 的书槽和页码。{@code set} 走 {@code Mth.clamp}，
     * {@code removeItemNoUpdate} 把页码和页数写成 0。{@code stillValid} 只看有没有书组件。
     */
    private static final class LecternSlots implements Container {
        private ItemStack book = ItemStack.EMPTY;
        private int page;
        private int pageCount;
        final PageData data = new PageData();

        void setBook(ItemStack book) {
            this.book = book;
            this.page = 0;
            this.pageCount = pageCountOf(book);
        }

        void setPage(int requested) {
            int next = Mth.clamp(requested, 0, this.pageCount - 1);
            if (next != this.page) {
                this.page = next;
            }
        }

        @Override
        public int getContainerSize() {
            return 1;
        }

        @Override
        public boolean isEmpty() {
            return this.book.isEmpty();
        }

        @Override
        public ItemStack getItem(int slot) {
            return slot == 0 ? this.book : ItemStack.EMPTY;
        }

        @Override
        public ItemStack removeItem(int slot, int count) {
            if (slot != 0) {
                return ItemStack.EMPTY;
            }
            ItemStack result = this.book.split(count);
            if (this.book.isEmpty()) {
                onRemove();
            }
            return result;
        }

        @Override
        public ItemStack removeItemNoUpdate(int slot) {
            if (slot != 0) {
                return ItemStack.EMPTY;
            }
            ItemStack previous = this.book;
            this.book = ItemStack.EMPTY;
            onRemove();
            return previous;
        }

        private void onRemove() {
            this.page = 0;
            this.pageCount = 0;
        }

        @Override
        public void setItem(int slot, ItemStack itemStack) {
        }

        @Override
        public int getMaxStackSize() {
            return 1;
        }

        @Override
        public void setChanged() {
        }

        @Override
        public boolean stillValid(Player player) {
            return hasBook();
        }

        @Override
        public boolean canPlaceItem(int slot, ItemStack itemStack) {
            return false;
        }

        @Override
        public void clearContent() {
        }

        private boolean hasBook() {
            return this.book.has(DataComponents.WRITABLE_BOOK_CONTENT) || this.book.has(DataComponents.WRITTEN_BOOK_CONTENT);
        }

        private final class PageData implements ContainerData {
            @Override
            public int get(int dataId) {
                return dataId == 0 ? LecternSlots.this.page : 0;
            }

            @Override
            public void set(int dataId, int value) {
                if (dataId == 0) {
                    LecternSlots.this.setPage(value);
                }
            }

            @Override
            public int getCount() {
                return 1;
            }
        }
    }

    private static int pageCountOf(ItemStack book) {
        WrittenBookContent written = book.get(DataComponents.WRITTEN_BOOK_CONTENT);
        if (written != null) {
            return written.pages().size();
        }
        WritableBookContent writable = book.get(DataComponents.WRITABLE_BOOK_CONTENT);
        return writable == null ? 0 : writable.pages().size();
    }

    /** 物品写成 [注册表 ID, 数量, 组件补丁十六进制]，空槽是 [0, 0, ""]。 */
    private static JsonArray pair(ItemStack stack) {
        JsonArray pair = new JsonArray();
        pair.add(stack.isEmpty() ? 0 : BuiltInRegistries.ITEM.getId(stack.getItem()));
        pair.add(stack.getCount());
        pair.add(stack.isEmpty() ? "" : patchHex(stack));
        return pair;
    }

    private static String patchHex(ItemStack stack) {
        RegistryFriendlyByteBuf buffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), REGISTRIES);
        DataComponentPatch.STREAM_CODEC.encode(buffer, stack.getComponentsPatch());
        byte[] bytes = new byte[buffer.readableBytes()];
        buffer.readBytes(bytes);
        StringBuilder builder = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            builder.append(Character.forDigit((value >> 4) & 0xF, 16));
            builder.append(Character.forDigit(value & 0xF, 16));
        }
        return builder.toString();
    }

    private static JsonArray stacks(ItemStack[] grid) {
        JsonArray items = new JsonArray();
        for (ItemStack stack : grid) {
            items.add(idCount(stack));
        }
        return items;
    }

    /** 开局材料只要物品和数量，组件是放进去之后才算出来的。 */
    private static JsonArray idCount(ItemStack stack) {
        JsonArray pair = new JsonArray();
        pair.add(stack.isEmpty() ? 0 : BuiltInRegistries.ITEM.getId(stack.getItem()));
        pair.add(stack.getCount());
        return pair;
    }

    private static void placeGrid(AbstractContainerMenu menu, Shape shape, ItemStack[] grid) {
        int offset = shape.seedOffset();
        for (int i = 0; i < grid.length; i++) {
            if (!grid[i].isEmpty()) {
                menu.getSlot(i + offset).set(grid[i].copy());
            }
        }
    }

    private static void seedData(AbstractContainerMenu menu, int[] values) {
        if (menu instanceof CrafterMenu) {
            // CrafterMenu 的 ContainerData 是构造时传进去的 SimpleContainerData，
            // 菜单自己的 setData 就能写到它。
            for (int i = 0; i < values.length; i++) {
                menu.setData(i, values[i]);
            }
            return;
        }
        try {
            ContainerData data = menu instanceof BrewingStandMenu
                    ? (ContainerData) field(BrewingStandMenu.class, "brewingStandData").get(menu)
                    : (ContainerData) field(AbstractFurnaceMenu.class, "data").get(menu);
            for (int i = 0; i < values.length; i++) {
                data.set(i, values[i]);
            }
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    @SuppressWarnings("unchecked")
    private static JsonArray dataSlots(AbstractContainerMenu menu) {
        try {
            List<DataSlot> slots = (List<DataSlot>) field(AbstractContainerMenu.class, "dataSlots").get(menu);
            JsonArray data = new JsonArray();
            for (DataSlot slot : slots) {
                data.add(slot.get());
            }
            return data;
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    /**
     * 织布机放上旗帜和染料，有时再放图案物品或已经有层数的旗帜。
     * 砂轮放上带耐久、附魔或诅咒的物品。补丁由 {@link #gridPatches} 带给 Rust。
     *
     * <p>Loom cases get a banner and a dye, sometimes a pattern item or a banner that already has
     * layers. Grindstone cases get damaged, enchanted, or cursed items. {@link #gridPatches} carries
     * the component patches to Rust.
     */
    private static void decorateStation(Shape shape, ItemStack[] grid, Inventory inventory, Random random) {
        if (shape == Shape.LOOM && random.nextInt(5) != 0) {
            grid[0] = loomBanner(random);
            Item dye = random.nextBoolean() ? Items.DYE.white() : Items.DYE.blue();
            grid[1] = new ItemStack(dye, 1 + random.nextInt(64));
            int pattern = random.nextInt(4);
            if (pattern == 1) {
                grid[2] = new ItemStack(Items.GLOBE_BANNER_PATTERN);
            } else if (pattern == 2) {
                grid[2] = new ItemStack(Items.CREEPER_BANNER_PATTERN);
            } else if (pattern == 3) {
                grid[2] = new ItemStack(Items.FLOWER_BANNER_PATTERN);
            }
            inventory.setItem(random.nextInt(36), new ItemStack(Items.BANNER.white(), 1 + random.nextInt(16)));
            inventory.setItem(random.nextInt(36), new ItemStack(Items.DYE.black(), 1 + random.nextInt(64)));
        } else if (shape == Shape.GRINDSTONE && random.nextInt(5) != 0) {
            grid[0] = grindInput(random);
            if (random.nextBoolean()) {
                grid[1] = grindInput(random);
            }
        }
    }

    private static ItemStack loomBanner(Random random) {
        Item item = random.nextBoolean() ? Items.BANNER.white() : Items.BANNER.orange();
        ItemStack banner = new ItemStack(item, 1 + random.nextInt(16));
        int layers = random.nextInt(8);
        if (layers > 0) {
            banner.set(DataComponents.BANNER_PATTERNS, bannerLayers(layers, random));
        }
        return banner;
    }

    private static BannerPatternLayers bannerLayers(int count, Random random) {
        HolderGetter<BannerPattern> getter = REGISTRIES.lookupOrThrow(Registries.BANNER_PATTERN);
        HolderSet<BannerPattern> patterns = getter.get(BannerPatternTags.NO_ITEM_REQUIRED).orElseThrow();
        BannerPatternLayers.Builder builder = new BannerPatternLayers.Builder();
        DyeColor[] colors = {
            DyeColor.WHITE, DyeColor.ORANGE, DyeColor.MAGENTA, DyeColor.LIGHT_BLUE,
            DyeColor.YELLOW, DyeColor.LIME, DyeColor.PINK, DyeColor.GRAY,
            DyeColor.LIGHT_GRAY, DyeColor.CYAN, DyeColor.PURPLE, DyeColor.BLUE,
            DyeColor.BROWN, DyeColor.GREEN, DyeColor.RED, DyeColor.BLACK
        };
        for (int i = 0; i < count; i++) {
            builder.add(patterns.get(i % patterns.size()), colors[random.nextInt(colors.length)]);
        }
        return builder.build();
    }

    private static ItemStack grindInput(Random random) {
        int kind = random.nextInt(5);
        if (kind == 0) {
            return new ItemStack(Items.STICK, 1);
        }
        if (kind == 1) {
            ItemStack book = new ItemStack(Items.ENCHANTED_BOOK);
            book.set(DataComponents.STORED_ENCHANTMENTS, grindEnchantments(random));
            return book;
        }
        ItemStack sword = new ItemStack(random.nextBoolean() ? Items.DIAMOND_SWORD : Items.IRON_SWORD);
        if (random.nextBoolean()) {
            sword.setDamageValue(random.nextInt(Math.max(1, sword.getMaxDamage())));
        }
        if (random.nextInt(3) != 0) {
            sword.set(DataComponents.ENCHANTMENTS, grindEnchantments(random));
        }
        if (random.nextInt(4) == 0) {
            sword.set(DataComponents.REPAIR_COST, random.nextInt(10));
        }
        return sword;
    }

    /**
     * 锋利、耐久，外加最多一条绑定诅咒。
     *
     * <p>Sharpness, Unbreaking, and at most one Binding Curse.
     */
    private static ItemEnchantments grindEnchantments(Random random) {
        Registry<Enchantment> registry = REGISTRIES.lookupOrThrow(Registries.ENCHANTMENT);
        ItemEnchantments.Mutable mutable = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
        mutable.set(registry.getOrThrow(Enchantments.SHARPNESS), 1 + random.nextInt(5));
        if (random.nextBoolean()) {
            mutable.set(registry.getOrThrow(Enchantments.UNBREAKING), 1 + random.nextInt(3));
        }
        if (random.nextBoolean()) {
            // 只放绑定诅咒。两件都带它时结果里也只有这一条，不和哈希顺序打架。
            // Binding curse only. Two cursed inputs still leave one entry, so hash order cannot diverge.
            mutable.set(registry.getOrThrow(Enchantments.BINDING_CURSE), 1);
        }
        return mutable.toImmutable();
    }

    /**
     * 工作台材料格上的组件补丁。下标是格子序号，合成格在菜单里还要再加 {@code seedOffset}。
     *
     * <p>Component patches on workstation ingredients. The index is the grid slot; crafting adds
     * {@code seedOffset} when the stack is placed in the menu.
     */
    private static JsonArray gridPatches(ItemStack[] grid) {
        JsonArray patches = new JsonArray();
        for (int i = 0; i < grid.length; i++) {
            if (grid[i].isEmpty() || grid[i].getComponentsPatch().isEmpty()) {
                continue;
            }
            JsonObject patch = new JsonObject();
            patch.addProperty("index", i);
            patch.addProperty("bytes", patchHex(grid[i]));
            patches.add(patch);
        }
        return patches;
    }

    private static void placeRecipe(Shape shape, ItemStack[] grid, Random random) {
        List<RecipeHolder<?>> recipes = recipesFor(shape);
        if (recipes.isEmpty()) {
            return;
        }
        for (int attempt = 0; attempt < 8; attempt++) {
            RecipeHolder<?> recipe = recipes.get(random.nextInt(recipes.size()));
            if (tryPlace(shape, grid, recipe.value())) {
                return;
            }
        }
    }

    private static List<RecipeHolder<?>> recipesFor(Shape shape) {
        RecipeType<?> type = switch (shape) {
            case INVENTORY, CRAFTING, CRAFTER -> RecipeType.CRAFTING;
            case STONECUTTER -> RecipeType.STONECUTTING;
            case SMITHING -> RecipeType.SMITHING;
            case FURNACE -> RecipeType.SMELTING;
            case BLAST_FURNACE -> RecipeType.BLASTING;
            case SMOKER -> RecipeType.SMOKING;
            case BREWING -> RecipeType.BREWING;
            default -> null;
        };
        if (type == null) {
            return List.of();
        }
        return BY_TYPE.getOrDefault(type, List.of());
    }

    private static boolean tryPlace(Shape shape, ItemStack[] grid, net.minecraft.world.item.crafting.Recipe<?> recipe) {
        ItemStack[] next = grid.clone();
        boolean placed = false;
        if (recipe instanceof ShapedRecipe shaped) {
            int width = shape.gridWidth();
            if (width == 0 || shaped.getWidth() > width || shaped.getHeight() > width) {
                return false;
            }
            List<Optional<Ingredient>> ingredients = shaped.getIngredients();
            int cursor = 0;
            for (int y = 0; y < shaped.getHeight(); y++) {
                for (int x = 0; x < shaped.getWidth(); x++) {
                    Optional<Ingredient> ingredient = ingredients.get(cursor++);
                    if (ingredient.isEmpty()) {
                        continue;
                    }
                    ItemStack stack = one(ingredient.get());
                    if (stack.isEmpty()) {
                        return false;
                    }
                    next[x + y * width] = stack;
                    placed = true;
                }
            }
        } else if (recipe instanceof ShapelessRecipe shapeless) {
            List<Ingredient> ingredients = shapelessIngredients(shapeless);
            if (ingredients.size() > grid.length) {
                return false;
            }
            for (int i = 0; i < ingredients.size(); i++) {
                ItemStack stack = one(ingredients.get(i));
                if (stack.isEmpty()) {
                    return false;
                }
                next[i] = stack;
                placed = true;
            }
        } else if (recipe instanceof SmithingRecipe smithing) {
            ItemStack base = one(smithing.baseIngredient());
            if (base.isEmpty()) {
                return false;
            }
            next[0] = smithing.templateIngredient().map(MenuOracle::one).orElse(ItemStack.EMPTY);
            next[1] = base;
            next[2] = smithing.additionIngredient().map(MenuOracle::one).orElse(ItemStack.EMPTY);
            placed = true;
        } else if (recipe instanceof BrewingRecipe brewing) {
            ItemStack input = one(brewing.getInput().ingredient());
            ItemStack reagent = one(brewing.getReagent().ingredient());
            if (input.isEmpty() || reagent.isEmpty()) {
                return false;
            }
            next[0] = input;
            next[3] = reagent;
            placed = true;
        } else if (recipe instanceof SingleItemRecipe single) {
            ItemStack stack = one(single.input());
            if (stack.isEmpty()) {
                return false;
            }
            next[0] = stack;
            placed = true;
        }
        if (!placed) {
            return false;
        }
        System.arraycopy(next, 0, grid, 0, grid.length);
        return true;
    }

    @SuppressWarnings("unchecked")
    private static List<Ingredient> shapelessIngredients(ShapelessRecipe recipe) {
        try {
            return (List<Ingredient>) field(ShapelessRecipe.class, "ingredients").get(recipe);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    private static ItemStack one(Ingredient ingredient) {
        return ingredient.items().findFirst().map(holder -> new ItemStack(holder.value())).orElse(ItemStack.EMPTY);
    }

    /**
     * 带哈希的同步器。补发的槽位记下来，和 Bamboo 的预测点击比。
     *
     * Records which slots were resent so the predicting click can be compared.
     */
    private static final class RecordingSync implements ContainerSynchronizer {
        final HashedPatchMap.HashGenerator hasher;
        final List<Integer> slots = new ArrayList<>();
        boolean carried;

        RecordingSync(HashedPatchMap.HashGenerator hasher) {
            this.hasher = hasher;
        }

        void reset() {
            slots.clear();
            carried = false;
        }

        @Override
        public void sendInitialData(AbstractContainerMenu container, List<ItemStack> slotItems, ItemStack carriedItem, int[] dataSlots) {
            container.incrementStateId();
        }

        @Override
        public void sendSlotChange(AbstractContainerMenu container, int slotIndex, ItemStack itemStack) {
            slots.add(slotIndex);
            container.incrementStateId();
        }

        @Override
        public void sendCarriedChange(AbstractContainerMenu container, ItemStack itemStack) {
            carried = true;
        }

        @Override
        public void sendDataChange(AbstractContainerMenu container, int id, int value) {
        }

        @Override
        public RemoteSlot createSlot() {
            return new RemoteSlot.Synchronized(hasher);
        }
    }

    /**
     * 官方 ServerPlayer 的匿名 ContainerSynchronizer。我们只关心 stateId：
     * 发整包和发一个槽各推进一次，光标和 data 槽不变。
     */
    private static final class Sync implements ContainerSynchronizer {
        @Override
        public void sendInitialData(AbstractContainerMenu container, List<ItemStack> slotItems, ItemStack carriedItem, int[] dataSlots) {
            container.incrementStateId();
        }

        @Override
        public void sendSlotChange(AbstractContainerMenu container, int slotIndex, ItemStack itemStack) {
            container.incrementStateId();
        }

        @Override
        public void sendCarriedChange(AbstractContainerMenu container, ItemStack itemStack) {
        }

        @Override
        public void sendDataChange(AbstractContainerMenu container, int id, int value) {
        }

        @Override
        public RemoteSlot createSlot() {
            // 客户端只回传 HashedStack.EMPTY，算哈希的回调永远不会被调用。
            return new RemoteSlot.Synchronized(component -> 0);
        }
    }

    /**
     * 假世界：方块、地图存档和随机数都存在内存里，够制图台、附魔台和铁砧的
     * `access.execute` 用。官方的 `Level.setBlock` 和 chunk 那一层整个跳过。
     */
    private static final class FakeLevel extends ServerLevel {
        private Map<BlockPos, BlockState> blocks;
        private Map<MapId, MapItemSavedData> maps;
        private int seed;
        private int nextMapId = 1000;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
            this.seed = 0;
        }

        static FakeLevel create() {
            FakeLevel level = allocate(FakeLevel.class);
            level.blocks = new HashMap<>();
            level.maps = new HashMap<>();
            // `BundleItem.playInsertSound` 等音效要 `level.random.nextFloat()`；
            // 收纳袋题也是从这一份固定随机源取音高。
            set(Level.class, level, "random", net.minecraft.util.RandomSource.create(4242L));
            set(Level.class, level, "soundSeedGenerator", net.minecraft.util.RandomSource.create(4243L));
            return level;
        }

        static FakeLevel create(int seed, int bookshelves, MapId mapId, byte scale, boolean locked) {
            FakeLevel level = allocate(FakeLevel.class);
            level.init(seed, bookshelves, mapId, scale, locked);
            return level;
        }

        private void init(int seed, int bookshelves, MapId mapId, byte scale, boolean locked) {
            this.blocks = new HashMap<>();
            this.maps = new HashMap<>();
            this.nextMapId = 1000;
            set(Level.class, this, "registryAccess", REGISTRIES);
            set(Level.class, this, "random", net.minecraft.util.RandomSource.create(seed));
            set(Level.class, this, "soundSeedGenerator", net.minecraft.util.RandomSource.create(seed + 1));
            set(Level.class, this, "dimension", net.minecraft.world.level.Level.OVERWORLD);
            LevelSettings settings = new LevelSettings(
                    "oracle", GameType.SURVIVAL, DifficultySettings.DEFAULT, true,
                    WorldDataConfiguration.DEFAULT);
            PrimaryLevelData data = new PrimaryLevelData(
                    settings, PrimaryLevelData.SpecialWorldProperty.NONE, Lifecycle.stable());
            data.setGameTime(1);
            set(Level.class, this, "levelData", data);
            set(Level.class, this, "isClientSide", false);
            set(Level.class, this, "isDebug", false);
            this.seed = seed;
            int placed = 0;
            for (int x = -2; x <= 2 && placed < bookshelves; x++) {
                for (int y = 0; y <= 1 && placed < bookshelves; y++) {
                    for (int z = -2; z <= 2 && placed < bookshelves; z++) {
                        if (Math.abs(x) != 2 && Math.abs(z) != 2) {
                            continue;
                        }
                        BlockPos at = BlockPos.ZERO.offset(x, y, z);
                        // 官方要求书架与附魔台之间的格子是空气。
                        // Official requires air between the shelf and the table.
                        blocks.put(BlockPos.ZERO.offset(x / 2, y, z / 2), Blocks.AIR.defaultBlockState());
                        blocks.put(at, Blocks.BOOKSHELF.defaultBlockState());
                        placed += 1;
                    }
                }
            }
            blocks.put(BlockPos.ZERO, Blocks.ANVIL.defaultBlockState());
            MapItemSavedData map = MapItemSavedData.createForClient(scale, locked, Level.OVERWORLD);
            maps.put(mapId, map);
        }

        @Override
        public int getNextEntityId() {
            // 假世界没有区块源，但实体构造函数要一个 ID；直接用官方的全局计数器。
            // The fake level has no chunk source; the entity constructor still
            // needs an id, so use vanilla's global counter directly.
            return 1000 + idCounter++;
        }

        private static int idCounter = 1;

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return blocks.getOrDefault(pos, Blocks.AIR.defaultBlockState());
        }

        @Override
        public boolean isInValidBounds(BlockPos pos) {
            return true;
        }

        @Override
        public net.minecraft.world.level.material.FluidState getFluidState(BlockPos pos) {
            return net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int limit) {
            blocks.put(pos, state);
            return true;
        }

        @Override
        public boolean removeBlock(BlockPos pos, boolean moving) {
            blocks.put(pos, Blocks.AIR.defaultBlockState());
            return true;
        }

        @Override
        public net.minecraft.world.level.chunk.LevelChunk getChunk(int x, int z) {
            return null;
        }

        @Override
        public MapItemSavedData getMapData(MapId id) {
            return maps.get(id);
        }

        @Override
        public void setMapData(MapId id, MapItemSavedData data) {
            maps.put(id, data);
        }

        @Override
        public MapId getFreeMapId() {
            return new MapId(nextMapId++);
        }

        @Override
        public long getGameTime() {
            return 1;
        }

        @Override
        public void playSeededSound(Entity entity, double x, double y, double z,
                net.minecraft.core.Holder<net.minecraft.sounds.SoundEvent> sound,
                net.minecraft.sounds.SoundSource source, float volume, float pitch, long seed) {}

        @Override
        public void levelEvent(Entity entity, int event, BlockPos pos, int data) {}

        @Override
        public FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public RecipeManager recipeAccess() {
            return RECIPES;
        }

        @Override
        public DedicatedServer getServer() {
            return SERVER;
        }

        @Override
        public GameRules getGameRules() {
            return RULES;
        }
    }

    /** 只有 getRecipeManager 和 getGameRules 会被菜单用到。 */
    private static final class FakeServer extends DedicatedServer {
        private RecipeManager recipes;
        private GameRules rules;

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        static FakeServer create(RecipeManager recipes, GameRules rules) {
            FakeServer server = allocate(FakeServer.class);
            server.recipes = recipes;
            server.rules = rules;
            return server;
        }

        @Override
        public RecipeManager getRecipeManager() {
            return recipes;
        }

        @Override
        public GameRules getGameRules() {
            return rules;
        }

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return LOOT_TABLES;
        }
    }

    /** 合成结果直接往 connection 发一个槽包。这里把包丢掉，state id 已经加过了。 */
    private static final class SilentListener extends ServerGamePacketListenerImpl {
        private SilentListener() {
            super(null, null, null, null);
        }

        static SilentListener create() {
            return allocate(SilentListener.class);
        }

        @Override
        public void send(Packet<?> packet) {}

        @Override
        public void send(Packet<?> packet, ChannelFutureListener listener) {}
    }

    /** 假商人。交易表固定，`notifyTrade` 记使用次数（影响售罄状态）。 */
    private static final class FakeMerchant implements Merchant {
        private final MerchantOffers offers;
        private Player tradingPlayer;

        FakeMerchant(MerchantOffers offers) {
            this.offers = offers;
        }

        @Override
        public void setTradingPlayer(Player player) {
            this.tradingPlayer = player;
        }

        @Override
        public Player getTradingPlayer() {
            return tradingPlayer;
        }

        @Override
        public MerchantOffers getOffers() {
            return offers;
        }

        @Override
        public void overrideOffers(MerchantOffers merchantOffers) {}

        @Override
        public void notifyTrade(MerchantOffer offer) {
            offer.increaseUses();
        }

        @Override
        public void notifyTradeUpdated(ItemStack stack) {}

        @Override
        public int getVillagerXp() {
            return 0;
        }

        @Override
        public void overrideXp(int xp) {}

        @Override
        public boolean showProgressBar() {
            return true;
        }

        @Override
        public SoundEvent getNotifyTradeSound() {
            return SoundEvents.VILLAGER_YES;
        }

        @Override
        public boolean isClientSide() {
            // `playTradeSound` 只在服务端把商人 cast 成 Entity 取坐标；假商人不是实体，
            // 让这一支按客户端跳过，菜单比较不看声音。
            return true;
        }

        @Override
        public boolean stillValid(Player player) {
            return true;
        }
    }

    /** 假玩家。继承 ServerPlayer 的理由和 BlockOracle 一样，只补菜单用得到的字段。 */
    private static final class FakePlayer extends ServerPlayer {
        private FakePlayer() {
            super(null, null, null, null);
        }

        static FakePlayer create(boolean creative) {
            return create(creative, true);
        }

        static FakePlayer create(boolean creative, boolean mayBuild) {
            FakePlayer player = allocate(FakePlayer.class);
            set(Entity.class, player, "level", FakeLevel.create());
            // `Entity.playSound` 读自己的位置；收纳袋音效的取数不受坐标影响。
            set(Entity.class, player, "position", new net.minecraft.world.phys.Vec3(0.5, 64.0, 0.5));
            set(Player.class, player, "inventory", new Inventory(player, new PlayerEquipment(player)));
            set(Player.class, player, "gameProfile",
                    new com.mojang.authlib.GameProfile(java.util.UUID.nameUUIDFromBytes(new byte[]{1}), "Oracle"));
            Abilities abilities = new Abilities();
            abilities.instabuild = creative;
            abilities.mayBuild = mayBuild;
            set(Player.class, player, "abilities", abilities);
            player.connection = SilentListener.create();
            return player;
        }

        /** 固定实体随机源和附魔种子，附魔台和铁砧的题才能重放。 */
        static FakePlayer create(int seed, boolean creative) {
            FakePlayer player = create(creative);
            set(Entity.class, player, "random", net.minecraft.util.RandomSource.create(seed));
            set(Player.class, player, "enchantmentSeed", seed);
            set(Player.class, player, "experienceLevel", 0);
            return player;
        }

        /** 菜单比较不看地上的物品，这里丢掉的东西不生成实体。 */

        @Override
        public ItemEntity drop(ItemStack itemStack, boolean thrownFromHand, Prediction prediction) {
            return null;
        }

        /**
         * `LoomMenu` 构造时用 `player.registryAccess()` 取旗帜图案。假世界没有注册表。
         *
         * <p>`LoomMenu` reads banner patterns from `player.registryAccess()` in its constructor.
         * The fake level has no registry.
         */
        @Override
        public RegistryAccess registryAccess() {
            return REGISTRIES;
        }

        @Override
        public boolean isCreative() {
            // `ArmorSlot.mayPickup` 会经 `ServerPlayer.gameMode` 查这个；
            // 假玩家没有 gameMode，用 abilities.instabuild 代替。
            // `ArmorSlot.mayPickup` reaches `ServerPlayer.gameMode`; the fake
            // player has none, so use abilities.instabuild instead.
            return getAbilities().instabuild;
        }

        @Override
        public void awardStat(net.minecraft.stats.Stat<?> stat, int count) {}

        @Override
        public int awardRecipes(Collection<RecipeHolder<?>> recipes) {
            return 0;
        }

        @Override
        public void triggerRecipeCrafted(RecipeHolder<?> recipe, List<ItemStack> itemStacks) {}

        @Override
        public void onEquipItem(EquipmentSlot slot, ItemStack oldStack, ItemStack stack) {}

        @Override
        public PlayerAdvancements getAdvancements() {
            return EmptyAdvancements.INSTANCE;
        }

        /** 铁砧取结果时会异步过滤名字，假玩家没有聊天过滤器。 */
        @Override
        public net.minecraft.server.network.TextFilter getTextFilter() {
            return net.minecraft.server.network.TextFilter.DUMMY;
        }
    }

    /** 成就触发器只问有没有监听者。没有的话官方就跳过，不会改菜单。 */
    private static final class EmptyAdvancements extends PlayerAdvancements {
        static final EmptyAdvancements INSTANCE = allocate(EmptyAdvancements.class);

        private EmptyAdvancements() {
            super(null, null, null, null, null);
        }

        @Override
        public <T extends CriterionTriggerInstance> Map<PlayerAdvancements.TriggerInstanceKey, T> getTriggerMapForType(
                CriterionTrigger<T> type) {
            return null;
        }
    }
}
