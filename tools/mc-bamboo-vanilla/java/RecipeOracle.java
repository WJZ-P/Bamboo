import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Random;
import java.util.Set;
import java.util.TreeMap;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.advancements.predicates.MinMaxBounds;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.component.predicates.PotionsPredicate;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.alchemy.Potion;
import net.minecraft.world.item.alchemy.PotionContents;
import net.minecraft.world.item.alchemy.Potions;
import net.minecraft.world.item.crafting.AbstractCookingRecipe;
import net.minecraft.world.item.crafting.BannerDuplicateRecipe;
import net.minecraft.world.item.crafting.BlastingRecipe;
import net.minecraft.world.item.crafting.BookCloningRecipe;
import net.minecraft.world.item.crafting.BrewingRecipe;
import net.minecraft.world.item.crafting.CampfireCookingRecipe;
import net.minecraft.world.item.crafting.CraftingInput;
import net.minecraft.world.item.crafting.CraftingRecipe;
import net.minecraft.world.item.crafting.DecoratedPotRecipe;
import net.minecraft.world.item.crafting.DyeRecipe;
import net.minecraft.world.item.crafting.FireworkRocketRecipe;
import net.minecraft.world.item.crafting.FireworkStarFadeRecipe;
import net.minecraft.world.item.crafting.FireworkStarRecipe;
import net.minecraft.world.item.crafting.ImbueRecipe;
import net.minecraft.world.item.crafting.Ingredient;
import net.minecraft.world.item.crafting.MapExtendingRecipe;
import net.minecraft.world.item.crafting.PotionIngredient;
import net.minecraft.world.item.crafting.Recipe;
import net.minecraft.world.item.crafting.RecipeHolder;
import net.minecraft.world.item.crafting.RecipeInput;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.item.crafting.RecipeType;
import net.minecraft.world.item.crafting.RepairItemRecipe;
import net.minecraft.world.item.crafting.ShapedRecipe;
import net.minecraft.world.item.crafting.ShapelessRecipe;
import net.minecraft.world.item.crafting.ShieldDecorationRecipe;
import net.minecraft.world.item.crafting.SingleRecipeInput;
import net.minecraft.world.item.crafting.SmithingRecipe;
import net.minecraft.world.item.crafting.SmithingRecipeInput;
import net.minecraft.world.item.crafting.SmeltingRecipe;
import net.minecraft.world.item.crafting.SmokingRecipe;
import net.minecraft.world.item.crafting.StonecutterRecipe;
import net.minecraft.world.item.crafting.TransmuteRecipe;
import net.minecraft.world.item.crafting.BrewingInput;
import net.minecraft.world.level.Level;

/**
 * 让官方的 {@code RecipeManager} 对大量合成网格和单物品输入出题，一题一行 JSON 写到 stdout。
 *
 * 网格大部分从真实配方的原料变出来：平移、镜像、换成标签里的别的物品、缺一格、多一格、
 * 数量不同；少部分完全随机。每个输入都把该类型里所有匹配的配方按注册表顺序列出来，
 * 再把第一条匹配配方的产物（物品和数量）附上。
 *
 * 参数：{@code --cases <随机网格数>}、{@code --seed <种子>}。
 */
public final class RecipeOracle {
    private static final Gson GSON = new Gson();
    /** Minecraft 的日志会把 `System.out` 包一层，题目直接写原始的 stdout。 */
    private static final PrintStream OUT =
            new PrintStream(new FileOutputStream(FileDescriptor.out), false, StandardCharsets.UTF_8);
    private static final int DEFAULT_CASES = 6000;
    private static final long DEFAULT_SEED = 20261108L;

    public static void main(String[] args) throws Exception {
        int cases = DEFAULT_CASES;
        long seed = DEFAULT_SEED;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--cases" -> cases = Integer.parseInt(args[++i]);
                case "--seed" -> seed = Long.parseLong(args[++i]);
                default -> throw new IllegalArgumentException("unknown argument " + args[i]);
            }
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));

        try (CloseableResourceManager resources =
                new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected())) {
            new Runner(Official.load(resources), new Random(seed), cases).run();
        }

        OUT.flush();
        System.exit(0);
    }

    private static final class Official {
        final RecipeManager recipes;

        private Official(RecipeManager recipes) {
            this.recipes = recipes;
        }

        /** 和世界加载一样的流程：标签、物品组件、配方都用真的。 */
        static Official load(CloseableResourceManager resources) throws Exception {
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
            return new Official(managers.getRecipeManager());
        }
    }

    private enum Mutation {
        NONE,
        DROP,
        ADD,
        WRONG_ITEM,
        WRONG_COUNT,
    }

    private static final class Runner {
        private final RecipeManager manager;
        private final Random random;
        private final int randomGrids;

        private final List<RecipeHolder<?>> crafting = new ArrayList<>();
        private final List<RecipeHolder<?>> smelting = new ArrayList<>();
        private final List<RecipeHolder<?>> blasting = new ArrayList<>();
        private final List<RecipeHolder<?>> smoking = new ArrayList<>();
        private final List<RecipeHolder<?>> campfire = new ArrayList<>();
        private final List<RecipeHolder<?>> stonecutting = new ArrayList<>();
        private final List<RecipeHolder<?>> smithing = new ArrayList<>();
        private final List<RecipeHolder<?>> brewing = new ArrayList<>();

        private final List<ShapedRecipe> shaped = new ArrayList<>();
        private final List<ShapelessRecipe> shapeless = new ArrayList<>();

        private final List<Item> pool = new ArrayList<>();
        private final List<Item> allItems = new ArrayList<>();
        private final TreeMap<String, Integer> emitted = new TreeMap<>();
        private final TreeMap<String, Integer> skipped = new TreeMap<>();

        Runner(Official official, Random random, int randomGrids) {
            this.manager = official.recipes;
            this.random = random;
            this.randomGrids = randomGrids;
            collectRecipes();
        }

        private void collectRecipes() {
            Set<Item> poolItems = new HashSet<>();
            for (RecipeHolder<?> holder : manager.getRecipes()) {
                Recipe<?> recipe = holder.value();
                if (recipe instanceof CraftingRecipe) {
                    crafting.add(holder);
                }
                if (recipe instanceof ShapedRecipe shapedRecipe) {
                    shaped.add(shapedRecipe);
                    for (Optional<Ingredient> ingredient : shapedRecipe.getIngredients()) {
                        ingredient.ifPresent(i -> collectItems(i, poolItems));
                    }
                } else if (recipe instanceof ShapelessRecipe shapelessRecipe) {
                    shapeless.add(shapelessRecipe);
                    for (Ingredient ingredient : shapelessRecipe.placementInfo().ingredients()) {
                        collectItems(ingredient, poolItems);
                    }
                }
                if (recipe instanceof AbstractCookingRecipe cooking) {
                    if (cooking instanceof SmeltingRecipe) {
                        smelting.add(holder);
                    } else if (cooking instanceof BlastingRecipe) {
                        blasting.add(holder);
                    } else if (cooking instanceof SmokingRecipe) {
                        smoking.add(holder);
                    } else if (cooking instanceof CampfireCookingRecipe) {
                        campfire.add(holder);
                    }
                    collectItems(cooking.input(), poolItems);
                }
                if (recipe instanceof StonecutterRecipe stonecutter) {
                    stonecutting.add(holder);
                    collectItems(stonecutter.input(), poolItems);
                }
                if (recipe instanceof SmithingRecipe simpleSmithing) {
                    smithing.add(holder);
                    collectItems(simpleSmithing.baseIngredient(), poolItems);
                    simpleSmithing.templateIngredient().ifPresent(i -> collectItems(i, poolItems));
                    simpleSmithing.additionIngredient().ifPresent(i -> collectItems(i, poolItems));
                }
                if (recipe instanceof BrewingRecipe brewingRecipe) {
                    brewing.add(holder);
                    collectItems(brewingRecipe.getInput().ingredient(), poolItems);
                    collectItems(brewingRecipe.getReagent().ingredient(), poolItems);
                }
            }
            pool.addAll(poolItems);
            for (Item item : BuiltInRegistries.ITEM) {
                if (item != Items.AIR) {
                    allItems.add(item);
                }
            }
        }

        private void collectItems(Ingredient ingredient, Set<Item> target) {
            ingredient.items().forEach(holder -> {
                if (holder.value() != Items.AIR) {
                    target.add(holder.value());
                }
            });
        }

        void run() {
            emptyCases();
            shapedCases();
            shapelessCases();
            specialCases();
            cookingCases();
            stonecuttingCases();
            smithingCases();
            brewingCases();
            randomGridCases();
            randomStackCases();
            JsonObject summary = new JsonObject();
            summary.addProperty("kind", "summary");
            summary.addProperty("cases", emitted.values().stream().mapToInt(Integer::intValue).sum());
            summary.addProperty("skipped", skipped.values().stream().mapToInt(Integer::intValue).sum());
            JsonObject byKind = new JsonObject();
            emitted.forEach(byKind::addProperty);
            summary.add("emitted", byKind);
            JsonObject skipReasons = new JsonObject();
            skipped.forEach(skipReasons::addProperty);
            summary.add("skipReasons", skipReasons);
            OUT.println(GSON.toJson(summary));
        }

        // ---------------------------------------------------------------- 合成网格

        private void emptyCases() {
            emitCrafting(Collections.nCopies(9, ItemStack.EMPTY), 3, 3);
            for (RecipeType<?> type : cookingTypes()) {
                emitStackCase("cooking", cookingName(type), type, cookingHandles(type), ItemStack.EMPTY);
            }
            emitStackCase("stonecutting", "stonecutting", stonecuttingType(), stonecutting, ItemStack.EMPTY);
            emitSmithing(ItemStack.EMPTY, ItemStack.EMPTY, ItemStack.EMPTY);
            emitBrewing(ItemStack.EMPTY, ItemStack.EMPTY);
        }

        private void shapedCases() {
            for (ShapedRecipe recipe : shaped) {
                List<Optional<Ingredient>> pattern = recipe.getIngredients();
                int width = recipe.getWidth();
                int height = recipe.getHeight();
                int grid = width <= 2 && height <= 2 ? 2 : 3;
                shaped(recipe, pattern, width, height, grid, grid, random.nextBoolean(), Mutation.NONE);
                Mutation[] mutations = {Mutation.DROP, Mutation.ADD, Mutation.WRONG_ITEM};
                for (Mutation mutation : mutations) {
                    shaped(recipe, pattern, width, height, grid, grid, random.nextBoolean(), mutation);
                }
                shaped(recipe, pattern, width, height, grid, grid, random.nextBoolean(), Mutation.WRONG_COUNT);
            }
        }

        private void shaped(ShapedRecipe recipe, List<Optional<Ingredient>> pattern, int width, int height,
                int gridWidth, int gridHeight, boolean mirror, Mutation mutation) {
            try {
                List<ItemStack> slots =
                        new ArrayList<>(Collections.nCopies(gridWidth * gridHeight, ItemStack.EMPTY));
                List<Integer> present = new ArrayList<>();
                for (int i = 0; i < pattern.size(); i++) {
                    if (pattern.get(i).isPresent()) {
                        present.add(i);
                    }
                }
                if (present.isEmpty()) {
                    return;
                }
                int special = present.get(random.nextInt(present.size()));
                int dx = random.nextInt(gridWidth - width + 1);
                int dy = random.nextInt(gridHeight - height + 1);
                for (int y = 0; y < height; y++) {
                    for (int x = 0; x < width; x++) {
                        int sourceX = mirror ? width - 1 - x : x;
                        int index = sourceX + y * width;
                        Optional<Ingredient> maybe = pattern.get(index);
                        if (maybe.isEmpty() || (mutation == Mutation.DROP && index == special)) {
                            continue;
                        }
                        Ingredient ingredient = maybe.get();
                        Item item = pickItem(ingredient);
                        int count = 1;
                        if (mutation == Mutation.WRONG_COUNT && index == special) {
                            count = 2 + random.nextInt(3);
                        } else if (random.nextInt(6) == 0) {
                            count = 1 + random.nextInt(3);
                        }
                        if (mutation == Mutation.WRONG_ITEM && index == special) {
                            item = pickWrongItem(ingredient);
                        }
                        slots.set((dy + y) * gridWidth + (dx + x), new ItemStack(item, count));
                    }
                }
                if (mutation == Mutation.ADD) {
                    int empty = randomEmpty(slots);
                    if (empty >= 0) {
                        slots.set(empty, new ItemStack(pickAnyItem(), 1 + random.nextInt(2)));
                    }
                }
                emitCrafting(slots, gridWidth, gridHeight);
            } catch (RuntimeException error) {
                skip("crafting", error);
            }
        }

        private void shapelessCases() {
            for (ShapelessRecipe recipe : shapeless) {
                List<Ingredient> ingredients = recipe.placementInfo().ingredients();
                if (ingredients.size() > 9) {
                    skip("shapeless-too-large", new IllegalStateException("" + ingredients.size()));
                    continue;
                }
                for (Mutation mutation : new Mutation[] {
                        Mutation.NONE, Mutation.DROP, Mutation.ADD, Mutation.WRONG_ITEM, Mutation.WRONG_COUNT}) {
                    shapeless(recipe, ingredients, mutation);
                }
            }
        }

        private void shapeless(ShapelessRecipe recipe, List<Ingredient> ingredients, Mutation mutation) {
            try {
                int size = ingredients.size();
                int grid = size <= 4 ? 2 : 3;
                List<Integer> positions = new ArrayList<>();
                for (int i = 0; i < grid * grid; i++) {
                    positions.add(i);
                }
                Collections.shuffle(positions, random);
                List<ItemStack> slots = new ArrayList<>(Collections.nCopies(grid * grid, ItemStack.EMPTY));
                int special = random.nextInt(size);
                for (int i = 0; i < size; i++) {
                    if (mutation == Mutation.DROP && i == special) {
                        continue;
                    }
                    Ingredient ingredient = ingredients.get(i);
                    Item item = mutation == Mutation.WRONG_ITEM && i == special
                            ? pickWrongItem(ingredient)
                            : pickItem(ingredient);
                    int count = mutation == Mutation.WRONG_COUNT && i == special
                            ? 2 + random.nextInt(3)
                            : 1;
                    slots.set(positions.get(i), new ItemStack(item, count));
                }
                if (mutation == Mutation.ADD) {
                    if (size == positions.size()) {
                        return;
                    }
                    slots.set(positions.get(size), new ItemStack(pickAnyItem(), 1));
                }
                emitCrafting(slots, grid, grid);
            } catch (RuntimeException error) {
                skip("crafting", error);
            }
        }

        // ---------------------------------------------------------------- 特殊配方

        /** 特殊配方的标准摆法和几种坏摆法，原料从配方自己的字段里取。 */
        private void specialCases() {
            for (RecipeHolder<?> holder : crafting) {
                Recipe<?> recipe = holder.value();
                try {
                    if (recipe instanceof TransmuteRecipe transmute) {
                        transmuteCases(transmute);
                    } else if (recipe instanceof DyeRecipe dye) {
                        dyeCases(dye);
                    } else if (recipe instanceof ImbueRecipe imbue) {
                        imbueCases(imbue);
                    } else if (recipe instanceof FireworkRocketRecipe rocket) {
                        fireworkRocketCases(rocket);
                    } else if (recipe instanceof FireworkStarRecipe star) {
                        fireworkStarCases(star);
                    } else if (recipe instanceof FireworkStarFadeRecipe fade) {
                        fireworkStarFadeCases(fade);
                    } else if (recipe instanceof MapExtendingRecipe map) {
                        mapExtendingCases(map);
                    } else if (recipe instanceof BannerDuplicateRecipe banner) {
                        bannerDuplicateCases(banner);
                    } else if (recipe instanceof BookCloningRecipe book) {
                        bookCloningCases(book);
                    } else if (recipe instanceof ShieldDecorationRecipe shield) {
                        shieldDecorationCases(shield);
                    } else if (recipe instanceof RepairItemRecipe) {
                        repairItemCases();
                    } else if (recipe instanceof DecoratedPotRecipe pot) {
                        decoratedPotCases(pot);
                    }
                } catch (ReflectiveOperationException | RuntimeException error) {
                    RuntimeException failure = error instanceof RuntimeException runtime
                            ? runtime : new IllegalStateException(error);
                    skip("crafting", failure);
                }
            }
        }

        private void transmuteCases(TransmuteRecipe recipe) throws ReflectiveOperationException {
            Ingredient input = ingredient(TransmuteRecipe.class, "input", recipe);
            Ingredient material = ingredient(TransmuteRecipe.class, "material", recipe);
            MinMaxBounds.Ints bounds =
                    (MinMaxBounds.Ints) readField(TransmuteRecipe.class, "materialCount", recipe);
            int min = Math.max(bounds.min().orElse(1), 0);
            int max = Math.min(bounds.max().orElse(8), 8);
            ItemStack source = new ItemStack(firstItem(input));
            ItemStack part = new ItemStack(firstItem(material));
            for (int count = min; count <= max; count++) {
                List<ItemStack> slots = new ArrayList<>();
                slots.add(source);
                for (int i = 0; i < count; i++) {
                    slots.add(part);
                }
                emitSlots(slots);
            }
            if (min > 0) {
                List<ItemStack> slots = new ArrayList<>();
                slots.add(source);
                for (int i = 1; i < min; i++) {
                    slots.add(part);
                }
                emitSlots(slots);
            }
            if (max < 8) {
                List<ItemStack> slots = new ArrayList<>();
                slots.add(source);
                for (int i = 0; i <= max; i++) {
                    slots.add(part);
                }
                emitSlots(slots);
            }
            emitSmall(source, part, new ItemStack(pickWrongItem(material)));
            emitSmall(source, source, part);
            emitSmall(part, part);
        }

        private void dyeCases(DyeRecipe recipe) throws ReflectiveOperationException {
            Ingredient target = ingredient(DyeRecipe.class, "target", recipe);
            Ingredient dye = ingredient(DyeRecipe.class, "dye", recipe);
            ItemStack targetStack = new ItemStack(firstItem(target));
            ItemStack dyeStack = new ItemStack(firstItem(dye));
            emitSmall(targetStack, dyeStack);
            emitSmall(targetStack, dyeStack, dyeStack);
            emitSmall(targetStack, new ItemStack(pickWrongItem(dye)));
            emitSmall(dyeStack, dyeStack);
            emitSmall(targetStack, targetStack);
        }

        private void imbueCases(ImbueRecipe recipe) throws ReflectiveOperationException {
            Ingredient source = ingredient(ImbueRecipe.class, "source", recipe);
            Ingredient material = ingredient(ImbueRecipe.class, "material", recipe);
            ItemStack sourceStack = new ItemStack(firstItem(source));
            ItemStack materialStack = new ItemStack(firstItem(material));
            List<ItemStack> slots = new ArrayList<>(Collections.nCopies(9, materialStack));
            slots.set(4, sourceStack);
            emitCrafting(slots, 3, 3);
            slots.set(0, new ItemStack(pickWrongItem(material)));
            emitCrafting(slots, 3, 3);
            slots.set(0, materialStack);
            slots.set(4, materialStack);
            slots.set(8, sourceStack);
            emitCrafting(slots, 3, 3);
            emitSmall(sourceStack);
        }

        private void fireworkRocketCases(FireworkRocketRecipe recipe) throws ReflectiveOperationException {
            ItemStack shell = new ItemStack(firstItem(ingredient(FireworkRocketRecipe.class, "shell", recipe)));
            ItemStack fuel = new ItemStack(firstItem(ingredient(FireworkRocketRecipe.class, "fuel", recipe)));
            ItemStack star = new ItemStack(firstItem(ingredient(FireworkRocketRecipe.class, "star", recipe)));
            emitSmall(shell, fuel);
            emitSmall(shell, fuel, fuel);
            emitSmall(shell, fuel, fuel, fuel);
            emitSmall(shell, fuel, star);
            emitSmall(shell, fuel, fuel, fuel, star);
            emitSmall(fuel, fuel);
            emitSmall(shell, shell, fuel);
            emitSmall(shell, new ItemStack(pickWrongItem(ingredient(FireworkRocketRecipe.class, "fuel", recipe))));
        }

        @SuppressWarnings("unchecked")
        private void fireworkStarCases(FireworkStarRecipe recipe) throws ReflectiveOperationException {
            Map<Object, Ingredient> shapes =
                    (Map<Object, Ingredient>) readField(FireworkStarRecipe.class, "shapes", recipe);
            Ingredient shape = shapes.values().iterator().next();
            ItemStack shapeStack = new ItemStack(firstItem(shape));
            ItemStack trail = new ItemStack(firstItem(ingredient(FireworkStarRecipe.class, "trail", recipe)));
            ItemStack twinkle = new ItemStack(firstItem(ingredient(FireworkStarRecipe.class, "twinkle", recipe)));
            Ingredient dyeIngredient = ingredient(FireworkStarRecipe.class, "dye", recipe);
            ItemStack dye = new ItemStack(firstItem(dyeIngredient));
            ItemStack fuel = new ItemStack(firstItem(ingredient(FireworkStarRecipe.class, "fuel", recipe)));
            emitSmall(shapeStack, dye, fuel);
            emitSmall(shapeStack, dye, fuel, trail, twinkle);
            emitSmall(shapeStack, dye);
            emitSmall(dye, fuel);
            emitSmall(shapeStack, new ItemStack(pickWrongItem(dyeIngredient)));
        }

        private void fireworkStarFadeCases(FireworkStarFadeRecipe recipe) throws ReflectiveOperationException {
            Ingredient dye = ingredient(FireworkStarFadeRecipe.class, "dye", recipe);
            ItemStack target = new ItemStack(firstItem(ingredient(FireworkStarFadeRecipe.class, "target", recipe)));
            ItemStack dyeStack = new ItemStack(firstItem(dye));
            emitSmall(target, dyeStack);
            emitSmall(target, dyeStack, dyeStack);
            emitSmall(dyeStack, dyeStack);
            emitSmall(target, new ItemStack(pickWrongItem(dye)));
        }

        private void mapExtendingCases(MapExtendingRecipe recipe) throws ReflectiveOperationException {
            ItemStack map = new ItemStack(firstItem(ingredient(MapExtendingRecipe.class, "map", recipe)));
            ItemStack material =
                    new ItemStack(firstItem(ingredient(MapExtendingRecipe.class, "material", recipe)));
            List<ItemStack> slots = new ArrayList<>(Collections.nCopies(9, material));
            slots.set(4, map);
            emitCrafting(slots, 3, 3);
            emitSmall(map, material);
            emitSmall(material, material);
        }

        private void bannerDuplicateCases(BannerDuplicateRecipe recipe) throws ReflectiveOperationException {
            Ingredient banner = ingredient(BannerDuplicateRecipe.class, "banner", recipe);
            ItemStack bannerStack = new ItemStack(firstItem(banner));
            emitSmall(bannerStack, bannerStack);
            emitSmall(bannerStack, new ItemStack(pickWrongItem(banner)));
            emitSmall(bannerStack);
        }

        private void bookCloningCases(BookCloningRecipe recipe) throws ReflectiveOperationException {
            ItemStack source =
                    new ItemStack(firstItem(ingredient(BookCloningRecipe.class, "source", recipe)));
            ItemStack material =
                    new ItemStack(firstItem(ingredient(BookCloningRecipe.class, "material", recipe)));
            emitSmall(source, material);
            emitSmall(source, material, material);
            emitSmall(material, material);
        }

        private void shieldDecorationCases(ShieldDecorationRecipe recipe) throws ReflectiveOperationException {
            Ingredient banner = ingredient(ShieldDecorationRecipe.class, "banner", recipe);
            Ingredient target = ingredient(ShieldDecorationRecipe.class, "target", recipe);
            ItemStack bannerStack = new ItemStack(firstItem(banner));
            ItemStack targetStack = new ItemStack(firstItem(target));
            emitSmall(bannerStack, targetStack);
            emitSmall(targetStack, bannerStack);
            emitSmall(bannerStack, bannerStack);
            emitSmall(targetStack, targetStack);
            emitSmall(bannerStack, new ItemStack(pickWrongItem(target)));
        }

        private void repairItemCases() {
            ItemStack pickaxe = new ItemStack(Items.DIAMOND_PICKAXE);
            ItemStack sword = new ItemStack(Items.DIAMOND_SWORD);
            emitSmall(pickaxe, pickaxe);
            emitSmall(pickaxe.copyWithCount(2), pickaxe);
            emitSmall(pickaxe, sword);
            emitSmall(pickaxe, new ItemStack(Items.STICK));
            emitSmall(pickaxe);
        }

        private void decoratedPotCases(DecoratedPotRecipe recipe) throws ReflectiveOperationException {
            ItemStack back = new ItemStack(firstItem(ingredient(DecoratedPotRecipe.class, "backPattern", recipe)));
            ItemStack left = new ItemStack(firstItem(ingredient(DecoratedPotRecipe.class, "leftPattern", recipe)));
            ItemStack right = new ItemStack(firstItem(ingredient(DecoratedPotRecipe.class, "rightPattern", recipe)));
            ItemStack front = new ItemStack(firstItem(ingredient(DecoratedPotRecipe.class, "frontPattern", recipe)));
            List<ItemStack> slots = new ArrayList<>(Collections.nCopies(9, ItemStack.EMPTY));
            slots.set(1, back);
            slots.set(3, left);
            slots.set(5, right);
            slots.set(7, front);
            emitCrafting(slots, 3, 3);
            slots.set(7, ItemStack.EMPTY);
            emitCrafting(slots, 3, 3);
            slots.set(7, front);
            slots.set(4, back);
            emitCrafting(slots, 3, 3);
        }

        /** 从前面开始摆，四格以内用 2x2，不然用 3x3。 */
        private void emitSmall(ItemStack... stacks) {
            List<ItemStack> slots = new ArrayList<>(List.of(stacks));
            emitSlots(slots);
        }

        private void emitSlots(List<ItemStack> stacks) {
            int grid = stacks.size() <= 4 ? 2 : 3;
            List<ItemStack> slots = new ArrayList<>(Collections.nCopies(grid * grid, ItemStack.EMPTY));
            for (int i = 0; i < stacks.size(); i++) {
                slots.set(i, stacks.get(i));
            }
            emitCrafting(slots, grid, grid);
        }

        private static Object readField(Class<?> owner, String name, Object target)
                throws ReflectiveOperationException {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            return field.get(target);
        }

        private static Ingredient ingredient(Class<?> owner, String name, Object target)
                throws ReflectiveOperationException {
            return (Ingredient) readField(owner, name, target);
        }

        private void randomGridCases() {
            for (int i = 0; i < randomGrids; i++) {
                try {
                    int grid = random.nextBoolean() ? 2 : 3;
                    List<ItemStack> slots = new ArrayList<>(grid * grid);
                    for (int slot = 0; slot < grid * grid; slot++) {
                        if (random.nextInt(100) < 45) {
                            slots.add(ItemStack.EMPTY);
                        } else {
                            slots.add(new ItemStack(pickAnyItem(), 1 + random.nextInt(4)));
                        }
                    }
                    emitCrafting(slots, grid, grid);
                } catch (RuntimeException error) {
                    skip("crafting", error);
                }
            }
        }

        // ---------------------------------------------------------------- 单物品和别的类型

        private void cookingCases() {
            for (RecipeHolder<?> holder : smelting) {
                cookingItems(holder);
            }
            for (RecipeHolder<?> holder : blasting) {
                cookingItems(holder);
            }
            for (RecipeHolder<?> holder : smoking) {
                cookingItems(holder);
            }
            for (RecipeHolder<?> holder : campfire) {
                cookingItems(holder);
            }
        }

        private void cookingItems(RecipeHolder<?> holder) {
            AbstractCookingRecipe recipe = (AbstractCookingRecipe) holder.value();
            RecipeType<?> type = recipe.getType();
            String name = cookingName(type);
            List<RecipeHolder<?>> handles = cookingHandles(type);
            for (Holder<Item> item : recipe.input().items().toList()) {
                try {
                    emitStackCase("cooking", name, type, handles,
                            new ItemStack(item.value(), 1 + random.nextInt(3)));
                } catch (RuntimeException error) {
                    skip("cooking", error);
                }
            }
        }

        private void stonecuttingCases() {
            for (RecipeHolder<?> holder : stonecutting) {
                StonecutterRecipe recipe = (StonecutterRecipe) holder.value();
                for (Holder<Item> item : recipe.input().items().toList()) {
                    try {
                        emitStackCase("stonecutting", "stonecutting", stonecuttingType(), stonecutting,
                                new ItemStack(item.value(), 1 + random.nextInt(3)));
                    } catch (RuntimeException error) {
                        skip("stonecutting", error);
                    }
                }
            }
        }

        private void smithingCases() {
            for (RecipeHolder<?> holder : smithing) {
                SmithingRecipe recipe = (SmithingRecipe) holder.value();
                ItemStack base = new ItemStack(firstItem(recipe.baseIngredient()));
                ItemStack template = recipe.templateIngredient()
                        .map(ingredient -> new ItemStack(firstItem(ingredient)))
                        .orElse(ItemStack.EMPTY);
                ItemStack addition = recipe.additionIngredient()
                        .map(ingredient -> new ItemStack(firstItem(ingredient)))
                        .orElse(ItemStack.EMPTY);
                try {
                    emitSmithing(template, base, addition);
                    if (!template.isEmpty()) {
                        emitSmithing(ItemStack.EMPTY, base, addition);
                    }
                    if (!addition.isEmpty()) {
                        emitSmithing(template, base, ItemStack.EMPTY);
                    }
                } catch (RuntimeException error) {
                    skip("smithing", error);
                }
            }
        }

        private void brewingCases() {
            for (RecipeHolder<?> holder : brewing) {
                BrewingRecipe recipe = (BrewingRecipe) holder.value();
                List<Holder<Potion>> inputPotions = potions(recipe.getInput());
                List<Holder<Potion>> reagentPotions = potions(recipe.getReagent());
                ItemStack input = new ItemStack(firstItem(recipe.getInput().ingredient()));
                ItemStack reagent = new ItemStack(firstItem(recipe.getReagent().ingredient()));
                try {
                    if (inputPotions.isEmpty()) {
                        emitBrewing(input, reagent);
                    } else {
                        for (Holder<Potion> potion : limit(inputPotions, 6)) {
                            emitBrewing(withPotion(input, potion), reagent);
                        }
                    }
                    if (!reagentPotions.isEmpty()) {
                        for (Holder<Potion> potion : limit(reagentPotions, 6)) {
                            emitBrewing(withPotion(input, inputPotions.isEmpty() ? null : inputPotions.get(0)),
                                    withPotion(reagent, potion));
                        }
                    }
                    if (!inputPotions.isEmpty()) {
                        Holder<Potion> wrong = pickWrongPotion(inputPotions);
                        if (wrong != null) {
                            emitBrewing(withPotion(input, wrong), reagent);
                        }
                    }
                } catch (RuntimeException error) {
                    skip("brewing", error);
                }
            }
        }

        private List<Holder<Potion>> potions(PotionIngredient ingredient) {
            Optional<PotionsPredicate> predicate = ingredient.potions();
            if (predicate.isEmpty()) {
                return List.of();
            }
            if (predicate.get().effects().isPresent()) {
                throw new IllegalStateException("potion effect predicates are not supported");
            }
            return predicate.get().potions().map(set -> set.stream().toList()).orElse(List.of());
        }

        private Holder<Potion> pickWrongPotion(List<Holder<Potion>> accepted) {
            for (Holder<Potion> potion : List.of(Potions.WATER, Potions.MUNDANE, Potions.THICK, Potions.AWKWARD)) {
                if (!accepted.contains(potion)) {
                    return potion;
                }
            }
            return null;
        }

        private ItemStack withPotion(ItemStack stack, Holder<Potion> potion) {
            ItemStack copy = stack.copy();
            if (potion != null) {
                copy.set(DataComponents.POTION_CONTENTS, new PotionContents(potion));
            }
            return copy;
        }

        private void randomStackCases() {
            for (RecipeType<?> type : cookingTypes()) {
                for (int i = 0; i < 150; i++) {
                    emitStackCase("cooking", cookingName(type), type, cookingHandles(type),
                            new ItemStack(pickAnyItem(), 1 + random.nextInt(3)));
                }
            }
            for (int i = 0; i < 300; i++) {
                emitStackCase("stonecutting", "stonecutting", stonecuttingType(), stonecutting,
                        new ItemStack(pickAnyItem(), 1 + random.nextInt(3)));
            }
            for (int i = 0; i < 200; i++) {
                emitSmithing(new ItemStack(pickAnyItem()), new ItemStack(pickAnyItem()),
                        new ItemStack(pickAnyItem()));
            }
            for (int i = 0; i < 300; i++) {
                emitBrewing(new ItemStack(pickAnyItem()), new ItemStack(pickAnyItem()));
            }
        }

        // ---------------------------------------------------------------- 出题和答案

        private void emitCrafting(List<ItemStack> slots, int width, int height) {
            CraftingInput input = CraftingInput.of(width, height, slots);
            JsonObject object = new JsonObject();
            object.addProperty("kind", "crafting");
            object.addProperty("width", width);
            object.addProperty("height", height);
            JsonArray array = new JsonArray();
            for (ItemStack slot : slots) {
                array.add(stackJson(slot));
            }
            object.add("slots", array);
            answer(object, craftingType(), input, crafting);
            emit(object, "crafting");
        }

        private void emitStackCase(String kind, String name, RecipeType<?> type, List<RecipeHolder<?>> handles,
                ItemStack input) {
            JsonObject object = new JsonObject();
            object.addProperty("kind", kind);
            object.addProperty("type", name);
            object.add("input", stackJson(input));
            answer(object, type, new SingleRecipeInput(input), handles);
            emit(object, kind);
        }

        private void emitSmithing(ItemStack template, ItemStack base, ItemStack addition) {
            SmithingRecipeInput input = new SmithingRecipeInput(template, base, addition);
            JsonObject object = new JsonObject();
            object.addProperty("kind", "smithing");
            object.add("template", stackJson(template));
            object.add("base", stackJson(base));
            object.add("addition", stackJson(addition));
            answer(object, smithingType(), input, smithing);
            emit(object, "smithing");
        }

        private void emitBrewing(ItemStack input, ItemStack reagent) {
            BrewingInput brewingInput = new BrewingInput(input, reagent);
            JsonObject object = new JsonObject();
            object.addProperty("kind", "brewing");
            object.add("input", stackJson(input));
            object.add("reagent", stackJson(reagent));
            answer(object, brewingType(), brewingInput, brewing);
            emit(object, "brewing");
        }

        private void answer(JsonObject object, RecipeType<?> type, RecipeInput input,
                List<RecipeHolder<?>> handles) {
            JsonArray matches = new JsonArray();
            for (RecipeHolder<?> holder : handles) {
                if (matches(holder, input)) {
                    matches.add(holder.id().identifier().toString());
                }
            }
            object.add("matches", matches);
            RecipeHolder<?> first = first(type, input);
            if (first == null) {
                object.add("first", JsonNull.INSTANCE);
                object.add("result", JsonNull.INSTANCE);
                return;
            }
            object.addProperty("first", first.id().identifier().toString());
            try {
                object.add("result", stackJson(assemble(first, input)));
            } catch (RuntimeException error) {
                object.add("result", JsonNull.INSTANCE);
                object.addProperty("assembleError", error.getClass().getSimpleName() + ": " + error.getMessage());
            }
        }

        @SuppressWarnings({"unchecked", "rawtypes"})
        private boolean matches(RecipeHolder<?> holder, RecipeInput input) {
            return ((Recipe) holder.value()).matches(input, (Level) null);
        }

        @SuppressWarnings({"unchecked", "rawtypes"})
        private RecipeHolder<?> first(RecipeType<?> type, RecipeInput input) {
            Optional<RecipeHolder<?>> result =
                    ((RecipeManager) manager).getRecipeFor((RecipeType) type, input, (Level) null);
            return result.orElse(null);
        }

        @SuppressWarnings({"unchecked", "rawtypes"})
        private ItemStack assemble(RecipeHolder<?> holder, RecipeInput input) {
            return ((Recipe) holder.value()).assemble(input);
        }

        private JsonElement stackJson(ItemStack stack) {
            if (stack.isEmpty()) {
                return JsonNull.INSTANCE;
            }
            JsonObject object = new JsonObject();
            object.addProperty("item", BuiltInRegistries.ITEM.getId(stack.getItem()));
            object.addProperty("count", stack.getCount());
            PotionContents contents = stack.get(DataComponents.POTION_CONTENTS);
            if (contents != null && contents.potion().isPresent()) {
                object.addProperty("potion", BuiltInRegistries.POTION.getId(contents.potion().get().value()));
            }
            return object;
        }

        private void emit(JsonObject object, String kind) {
            emitted.merge(kind, 1, Integer::sum);
            OUT.println(GSON.toJson(object));
        }

        private void skip(String kind, RuntimeException error) {
            String reason = error.getClass().getSimpleName() + ": " + error.getMessage();
            skipped.merge(kind + "/" + reason, 1, Integer::sum);
        }

        // ---------------------------------------------------------------- 小工具

        private RecipeType<?> craftingType() {
            return crafting.get(0).value().getType();
        }

        private RecipeType<?> smithingType() {
            return smithing.get(0).value().getType();
        }

        private RecipeType<?> brewingType() {
            return brewing.get(0).value().getType();
        }

        private RecipeType<?> stonecuttingType() {
            return stonecutting.get(0).value().getType();
        }

        private List<RecipeType<?>> cookingTypes() {
            return List.of(smelting.get(0).value().getType(), blasting.get(0).value().getType(),
                    smoking.get(0).value().getType(), campfire.get(0).value().getType());
        }

        private List<RecipeHolder<?>> cookingHandles(RecipeType<?> type) {
            if (type == smelting.get(0).value().getType()) {
                return smelting;
            }
            if (type == blasting.get(0).value().getType()) {
                return blasting;
            }
            if (type == smoking.get(0).value().getType()) {
                return smoking;
            }
            return campfire;
        }

        private String cookingName(RecipeType<?> type) {
            if (type == smelting.get(0).value().getType()) {
                return "smelting";
            }
            if (type == blasting.get(0).value().getType()) {
                return "blasting";
            }
            if (type == smoking.get(0).value().getType()) {
                return "smoking";
            }
            return "campfire_cooking";
        }

        private Item pickItem(Ingredient ingredient) {
            List<Holder<Item>> items = ingredient.items().toList();
            return items.get(random.nextInt(items.size())).value();
        }

        private Item firstItem(Ingredient ingredient) {
            return ingredient.items().findFirst().orElseThrow().value();
        }

        private Item pickWrongItem(Ingredient ingredient) {
            for (int attempt = 0; attempt < 20; attempt++) {
                Item item = pickAnyItem();
                if (!ingredient.test(new ItemStack(item))) {
                    return item;
                }
            }
            return pickAnyItem();
        }

        private Item pickAnyItem() {
            if (random.nextInt(5) != 0 && !pool.isEmpty()) {
                return pool.get(random.nextInt(pool.size()));
            }
            return allItems.get(random.nextInt(allItems.size()));
        }

        private int randomEmpty(List<ItemStack> slots) {
            List<Integer> empty = new ArrayList<>();
            for (int i = 0; i < slots.size(); i++) {
                if (slots.get(i).isEmpty()) {
                    empty.add(i);
                }
            }
            return empty.isEmpty() ? -1 : empty.get(random.nextInt(empty.size()));
        }

        private static <T> List<T> limit(List<T> values, int max) {
            return values.size() <= max ? values : values.subList(0, max);
        }
    }
}
