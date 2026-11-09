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
import net.minecraft.world.entity.player.PlayerEquipment;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.inventory.AbstractFurnaceMenu;
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
import net.minecraft.world.inventory.FurnaceMenu;
import net.minecraft.world.inventory.GrindstoneMenu;
import net.minecraft.world.inventory.HopperMenu;
import net.minecraft.world.inventory.InventoryMenu;
import net.minecraft.world.inventory.LoomMenu;
import net.minecraft.server.network.Filterable;
import net.minecraft.util.Mth;
import net.minecraft.world.inventory.LecternMenu;
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.inventory.RemoteSlot;
import net.minecraft.world.inventory.ShulkerBoxMenu;
import net.minecraft.world.inventory.SimpleContainerData;
import net.minecraft.world.inventory.Slot;
import net.minecraft.world.inventory.SmithingMenu;
import net.minecraft.world.inventory.SmokerMenu;
import net.minecraft.world.inventory.StonecutterMenu;
import net.minecraft.world.inventory.TransientCraftingContainer;
import net.minecraft.world.item.DyeColor;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.Enchantments;
import net.minecraft.world.item.enchantment.ItemEnchantments;
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
                write(componentCase(written));
                written++;
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
        GRINDSTONE(2, "grindstone", true, false);

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

    /** 假世界里没有方块，菜单也不看世界，只有 enabledFeatures 会被物品的点击覆盖逻辑问到。 */
    private static final class FakeLevel extends ServerLevel {
        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        /** 和 BlockOracle 一样跳过构造函数：真的 ServerLevel 要一个完整的服务端。 */
        static FakeLevel create() {
            return allocate(FakeLevel.class);
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
            set(Player.class, player, "inventory", new Inventory(player, new PlayerEquipment(player)));
            Abilities abilities = new Abilities();
            abilities.instabuild = creative;
            abilities.mayBuild = mayBuild;
            set(Player.class, player, "abilities", abilities);
            player.connection = SilentListener.create();
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
