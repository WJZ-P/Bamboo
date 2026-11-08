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
import java.util.List;
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
import net.minecraft.world.entity.Entity;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.inventory.ContainerData;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.alchemy.PotionContents;
import net.minecraft.world.item.alchemy.Potions;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.AbstractFurnaceBlockEntity;
import net.minecraft.world.level.block.entity.BlastFurnaceBlockEntity;
import net.minecraft.world.level.block.entity.BrewingStandBlockEntity;
import net.minecraft.world.level.block.entity.FurnaceBlockEntity;
import net.minecraft.world.level.block.entity.SmokerBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
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
        runs.add(brew(17L, 410));

        JsonObject root = new JsonObject();
        root.add("runs", runs);
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
            entity.getRecipesToAwardAndPopExperience(level, Vec3.ZERO);
            run.addProperty("afterTake", seedOf(level.random));
            run.add("afterItems", slotsOf(entity));
        }
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

    private static final class MachineLevel extends ServerLevel {
        private BlockState state = Blocks.AIR.defaultBlockState();
        private RandomSource random;
        private int nextId = 1;

        private MachineLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        static MachineLevel create(long seed) {
            try {
                MachineLevel level = (MachineLevel) UNSAFE.allocateInstance(MachineLevel.class);
                level.state = Blocks.AIR.defaultBlockState();
                Field random = MachineLevel.class.getDeclaredField("random");
                random.setAccessible(true);
                random.set(level, RandomSource.create(seed));
                return level;
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
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
            return this.state;
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState blockState, int updateFlags, int updateLimit) {
            this.state = blockState;
            return true;
        }

        @Override
        public void blockEntityChanged(BlockPos pos) {
        }

        @Override
        public void updateNeighbourForOutputSignal(BlockPos pos, net.minecraft.world.level.block.Block changedBlock) {
        }

        @Override
        public void levelEvent(@Nullable Entity source, int type, BlockPos pos, int data) {
        }

        @Override
        public boolean addFreshEntity(Entity entity) {
            return true;
        }

        @Override
        public <T extends Entity> List<T> getEntities(EntityTypeTest<Entity, T> type, AABB box, Predicate<? super T> selector) {
            return List.of();
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
