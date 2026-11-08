import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
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
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.HashedStack;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.Prediction;
import net.minecraft.world.Container;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.entity.player.Abilities;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.player.PlayerEquipment;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.inventory.ContainerSynchronizer;
import net.minecraft.world.inventory.DispenserMenu;
import net.minecraft.world.inventory.HopperMenu;
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.inventory.RemoteSlot;
import net.minecraft.world.inventory.ShulkerBoxMenu;
import net.minecraft.world.inventory.Slot;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import sun.misc.Unsafe;

/**
 * 容器菜单的 oracle：官方代码出题、给答案，tools/mc-bamboo-vanilla/src/menu.rs 用 mc-bamboo-game 的 Replay 跑同样的题来比较。
 *
 * <p>不启动服务端，也不打开真正的方块。每道题是官方的一个菜单（ChestMenu 三行和六行、HopperMenu、DispenserMenu、
 * ShulkerBoxMenu）配一个 SimpleContainer 和一名假玩家的 Inventory，再按 handleContainerClick 的顺序点一串随机点击。
 * 假玩家是 Unsafe.allocateInstance 出来的 ServerPlayer，只补了 level、inventory 和 abilities.instabuild；
 * 掉落的物品被 override 成空操作，因为 Bamboo 也没有物品实体。
 *
 * <p>客户端按“从不预测”的模型模拟，两边相同：每次点击的 changedSlots 是空的，carriedItem 恒为 {@link HashedStack#EMPTY}。
 * 每步之后写出菜单的全部槽位、光标和 stateId。菜单的 stateId 只有 ContainerSynchronizer 会推进，
 * 官方的那个是 ServerPlayer 里的匿名类，这里按它原样写了一个：sendInitialData 和 sendSlotChange 各调一次
 * {@code container.incrementStateId()}，sendCarriedChange 不调。sendAllDataToRemote 和 broadcastChanges 本身仍然是官方的。
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
 * 日志和异常在 stderr。物品用注册表 ID，空槽是 [0, 0]。
 */
public final class MenuOracle {
    /** 菜单的容器 ID。所有题都用 1，和 ServerPlayer.nextContainerCounter 的第一次相同。 */
    private static final int CONTAINER_ID = 1;
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final ContainerInput[] INPUTS = ContainerInput.values();
    private static final Unsafe UNSAFE = loadUnsafe();
    private static final Map<String, Field> FIELDS = new HashMap<>();

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
            boolean creative = random.nextInt(4) != 0;
            FakePlayer player = FakePlayer.create(creative);
            Inventory inventory = player.getInventory();

            for (int i = 0, count = random.nextInt(10); i < count; i++) {
                inventory.setItem(inventorySlot(random), randomStack(random));
            }

            Container container = new SimpleContainer(shape.size);
            for (int i = 0, count = random.nextInt(shape.size + 1); i < count; i++) {
                container.setItem(random.nextInt(shape.size), randomStack(random));
            }

            AbstractContainerMenu menu = shape.create(inventory, container);
            // 官方 ServerPlayer.initMenu：装同步器，sendAllDataToRemote 发出第一个 stateId。
            menu.setSynchronizer(new Sync());

            JsonObject line = new JsonObject();
            line.addProperty("type", "case");
            line.addProperty("id", id);
            line.addProperty("shape", shape.id);
            line.addProperty("creative", creative);
            line.add("container", items(container, shape.size));
            line.add("inventory", inventoryItems(inventory));
            line.add("open", outcome(menu));

            JsonArray steps = new JsonArray();
            for (int[] click : clickSequence(random, menu.slots.size())) {
                // handleContainerClick 去掉 containerId、stillValid 和槽位校验之后的顺序。
                menu.suppressRemoteUpdates();
                menu.clicked(click[0], click[1], INPUTS[click[2]], player);
                menu.setRemoteCarried(HashedStack.EMPTY);
                menu.resumeRemoteUpdates();
                menu.broadcastChanges();

                JsonObject step = new JsonObject();
                step.addProperty("slot", click[0]);
                step.addProperty("button", click[1]);
                step.addProperty("input", click[2]);
                step.add("outcome", outcome(menu));
                steps.add(step);
            }
            line.add("steps", steps);
            return line;
        }
    }

    /** 菜单的形状。名字和 Rust 的 ReplayShape 对应。 */
    private enum Shape {
        CHEST_3(27, "chest3"),
        CHEST_6(54, "chest6"),
        HOPPER(5, "hopper"),
        DISPENSER(9, "dispenser"),
        SHULKER(27, "shulker");

        final int size;
        final String id;

        Shape(int size, String id) {
            this.size = size;
            this.id = id;
        }

        AbstractContainerMenu create(Inventory inventory, Container container) {
            return switch (this) {
                case CHEST_3 -> new ChestMenu(MenuType.GENERIC_9x3, CONTAINER_ID, inventory, container, 3);
                case CHEST_6 -> new ChestMenu(MenuType.GENERIC_9x6, CONTAINER_ID, inventory, container, 6);
                case HOPPER -> new HopperMenu(CONTAINER_ID, inventory, container);
                case DISPENSER -> new DispenserMenu(CONTAINER_ID, inventory, container);
                case SHULKER -> new ShulkerBoxMenu(CONTAINER_ID, inventory, container);
            };
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
            if (random.nextInt(100) < 12) {
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
            items.add(pair(container.getItem(slot)));
        }
        return items;
    }

    /** 玩家的 41 格：Official Inventory.getItem 对 36 到 40 返回盔甲和副手。 */
    private static JsonArray inventoryItems(Inventory inventory) {
        JsonArray items = new JsonArray();
        for (int slot = 0; slot <= 40; slot++) {
            items.add(pair(inventory.getItem(slot)));
        }
        return items;
    }

    private static JsonObject outcome(AbstractContainerMenu menu) {
        JsonObject outcome = new JsonObject();
        outcome.addProperty("state", menu.getStateId());
        outcome.add("carried", pair(menu.getCarried()));
        JsonArray slots = new JsonArray();
        for (Slot slot : menu.slots) {
            slots.add(pair(slot.getItem()));
        }
        outcome.add("slots", slots);
        return outcome;
    }

    /** 物品写成 [注册表 ID, 数量]，空槽是 [0, 0]。 */
    private static JsonArray pair(ItemStack stack) {
        JsonArray pair = new JsonArray();
        pair.add(stack.isEmpty() ? 0 : BuiltInRegistries.ITEM.getId(stack.getItem()));
        pair.add(stack.getCount());
        return pair;
    }

    /**
     * 官方 ServerPlayer 的匿名 ContainerSynchronizer。我们只关心 stateId：
     * 发整包和发一个槽各推进一次，光标不变。
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
    }

    /** 假玩家。继承 ServerPlayer 的理由和 BlockOracle 一样，只补菜单用得到的字段。 */
    private static final class FakePlayer extends ServerPlayer {
        private FakePlayer() {
            super(null, null, null, null);
        }

        static FakePlayer create(boolean creative) {
            FakePlayer player = allocate(FakePlayer.class);
            set(Entity.class, player, "level", FakeLevel.create());
            set(Player.class, player, "inventory", new Inventory(player, new PlayerEquipment(player)));
            Abilities abilities = new Abilities();
            abilities.instabuild = creative;
            set(Player.class, player, "abilities", abilities);
            return player;
        }

        /** Bamboo 没有物品实体，丢掉的东西直接消失。 */
        @Override
        public ItemEntity drop(ItemStack itemStack, boolean thrownFromHand, Prediction prediction) {
            return null;
        }

        @Override
        public void awardStat(net.minecraft.stats.Stat<?> stat, int count) {
        }
    }
}
