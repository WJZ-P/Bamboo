//! 配方的数据结构和官方数据包导出的那张表。
//!
//! 表在 [`crate::recipes`]，条目顺序就是官方注册表的顺序：查找时取第一条匹配的，先后由它决定。
//! 这里只放数据，匹配逻辑在 `mc-bamboo-recipes`。
//!
//! 物品组件在这个版本里还是不透明的字节。需要读组件的特殊配方只能记下结构，
//! 组件相关的判断留给以后，见 [`RecipeKind`] 各个变体的说明。

/// 一个配方原料：官方的 `Ingredient`，展开后的物品 ID。
///
/// 官方存的是 `HolderSet<Item>`，可以是一个物品列表，也可以是一个物品标签。
/// 标签已经展开成物品 ID，`tag` 只留下名字方便核对数据。
/// `items` 按 ID 升序排列、没有重复，查找用二分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ingredient {
    /// 原料本来是一个标签时才有，比如 `minecraft:planks`。
    pub tag: Option<&'static str>,
    pub items: &'static [i32],
}

impl Ingredient {
    /// 官方的 `Ingredient.test`：物品在展开后的列表里。
    pub fn test(&self, item: i32) -> bool {
        self.items.binary_search(&item).is_ok()
    }

    /// 官方的 `Ingredient.isEmpty`：一个物品都没有的原料不参与放置。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// 默认物品栈带 `max_damage` 和 `damage` 组件的物品。
///
/// 官方的 `RepairItemRecipe` 要求两个输入都有这两个组件；原版物品里有 `max_damage`
/// 的都同时有 `damage`，所以查这张表就够了。表按 ID 升序。
pub fn is_damageable(item: i32) -> bool {
    crate::recipes::DAMAGEABLE.binary_search(&item).is_ok()
}

/// 默认物品栈带 `dye` 组件的物品。染色、烟花之星、烟花之星褪色都要求染料物品有它。
pub fn has_dye_component(item: i32) -> bool {
    crate::recipes::DYED.binary_search(&item).is_ok()
}

/// 一个配方的产物，官方的 `ItemStackTemplate`。
///
/// `components` 是模板自带组件补丁，按官方 `DataComponentPatch.STREAM_CODEC`
/// 编码的字节，由 `mc-bamboo-protocol` 解码；空切片表示没有组件改动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    pub item: i32,
    pub count: i32,
    pub components: &'static [u8],
}

/// 酿造配方的原料，官方的 `PotionIngredient`。
///
/// 物品条件之外还可以要求药水内容。官方数据包用到的只有直接列出药水 ID 的写法，
/// 所以 `potions` 是药水注册表 ID，`None` 表示不限制药水。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionIngredient {
    pub item: Ingredient,
    /// 药水注册表 ID，见 [`crate::recipes::POTIONS`]。
    pub potions: Option<&'static [i32]>,
}

/// 烹饪类配方的种类，对应官方的四个 `RecipeType`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CookingKind {
    Smelting,
    Blasting,
    Smoking,
    Campfire,
}

/// `FireworkExplosion.Shape` 的序列化名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireworkShape {
    SmallBall,
    LargeBall,
    Star,
    Creeper,
    Burst,
}

impl FireworkShape {
    /// 官方的序列化名。
    pub const fn name(self) -> &'static str {
        match self {
            Self::SmallBall => "small_ball",
            Self::LargeBall => "large_ball",
            Self::Star => "star",
            Self::Creeper => "creeper",
            Self::Burst => "burst",
        }
    }
}

/// 一个配方。`id` 是官方的配方键，比如 `minecraft:oak_planks`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    pub id: &'static str,
    pub kind: RecipeKind,
}

impl Recipe {
    /// 配方类型，对应官方的 `Recipe.getType()`。
    pub fn recipe_type(&self) -> RecipeType {
        self.kind.recipe_type()
    }

    /// 这个配方固定给出的产物。酿造的输出需要组件，只有物品和数量在这里。
    ///
    /// 修复物品、纹饰、地图放大的产物要看输入才知道，返回 `None`。
    pub fn result(&self) -> Option<Stack> {
        self.kind.result()
    }
}

/// 官方的 `RecipeType`。数字是注册表 ID，和客户端配方书无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecipeType {
    Crafting,
    Smelting,
    Blasting,
    Smoking,
    CampfireCooking,
    Stonecutting,
    Smithing,
    Brewing,
}

/// 官方的 `RecipeSerializer` 分出来的配方种类。
///
/// 需要物品组件的特殊配方只做了结构检查：
/// 装饰罐、烟花火箭、烟花之星、淡色烟花之星、染色、浸染、转化是完整判断；
/// 旗帜复制、书复制、地图放大、修复物品、盾牌装饰读不到组件，见各变体的注释。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecipeKind {
    /// `ShapedRecipe`。`pattern` 已经按 `ShapedRecipePattern` 裁掉空行空列，
    /// `width * height == pattern.len()`，`None` 的格子必须空着。
    Shaped {
        width: u8,
        height: u8,
        pattern: &'static [Option<Ingredient>],
        result: Stack,
    },
    /// `ShapelessRecipe`。
    Shapeless {
        ingredients: &'static [Ingredient],
        result: Stack,
    },
    /// `TransmuteRecipe`（羊毛染色、床、箱子、陶罐……）。
    /// `result_item == None` 表示产物沿用输入物品（地图复制）。
    Transmute {
        input: Ingredient,
        material: Ingredient,
        min_material: i32,
        max_material: i32,
        result_item: Option<i32>,
        result_count: i32,
        result_components: &'static [u8],
        add_material_count: bool,
    },
    /// `DyeRecipe`：皮革盔甲染色。
    Dye {
        target: Ingredient,
        dye: Ingredient,
        result: Stack,
    },
    /// `ImbueRecipe`：药箭，3x3 摆满，中心是药水。
    Imbue {
        source: Ingredient,
        material: Ingredient,
        result: Stack,
    },
    /// 烧炼、高炉、烟熏炉、营火。
    Cooking {
        kind: CookingKind,
        ingredient: Ingredient,
        result: Stack,
        experience: f32,
        time: i32,
    },
    /// `StonecutterRecipe`：同一输入可能有多个，按注册表顺序。
    Stonecutting {
        ingredient: Ingredient,
        result: Stack,
    },
    /// `SmithingTransformRecipe`：下界合金升级之类。
    SmithingTransform {
        template: Option<Ingredient>,
        base: Ingredient,
        addition: Option<Ingredient>,
        result: Stack,
    },
    /// `SmithingTrimRecipe`：产物要写纹饰组件，这里只记图案名。
    SmithingTrim {
        template: Ingredient,
        base: Ingredient,
        addition: Ingredient,
        pattern: &'static str,
    },
    /// `BrewingRecipe`。
    Brewing {
        input: PotionIngredient,
        reagent: PotionIngredient,
        result: Stack,
    },
    /// `FireworkRocketRecipe`：一个壳、最多三个燃料、若干烟花之星。
    FireworkRocket {
        shell: Ingredient,
        fuel: Ingredient,
        star: Ingredient,
        result: Stack,
    },
    /// `FireworkStarRecipe`。`shapes` 是形状 → 决定形状的原料。
    FireworkStar {
        shapes: &'static [(FireworkShape, Ingredient)],
        trail: Ingredient,
        twinkle: Ingredient,
        fuel: Ingredient,
        dye: Ingredient,
        result: Stack,
    },
    /// `FireworkStarFadeRecipe`：给烟花之星加褪色。
    FireworkStarFade {
        target: Ingredient,
        dye: Ingredient,
        result: Stack,
    },
    /// `MapExtendingRecipe`：地图书扩展。官方先找带 `MAP_ID` 的地图，再查存档的缩放等级。
    /// 原版 `filled_map` 没有默认的 `MAP_ID`，默认物品栈永远匹配不上，结果恒为 false。
    /// `result_item == None` 表示沿用输入的地图。
    MapExtending {
        map: Ingredient,
        material: Ingredient,
        result_item: Option<i32>,
        result_count: i32,
        result_components: &'static [u8],
    },
    /// `BannerDuplicateRecipe`：两个同色旗帜，一个带图案一个不带。
    /// 原版旗帜的 `BANNER_PATTERNS` 默认是空图案，所以默认物品栈永远凑不齐“带图案的源”，
    /// 匹配结果恒为 false；这里仍照官方结构写，等组件能读以后就是完整实现。
    BannerDuplicate { banner: Ingredient, result: Stack },
    /// `BookCloningRecipe`：成书复制。原版成书的 `WRITTEN_BOOK_CONTENT` 没有默认值，
    /// 默认物品栈永远找不到源，匹配结果恒为 false；结构照官方保留。
    BookCloning {
        source: Ingredient,
        material: Ingredient,
        min_generation: i32,
        max_generation: i32,
        result: Stack,
    },
    /// `ShieldDecorationRecipe`：盾牌加旗帜图案。官方只要求另一格是空图案的盾牌，
    /// 默认物品栈的 `BANNER_PATTERNS` 都是空的，所以匹配可以完整做到。
    ShieldDecoration {
        banner: Ingredient,
        target: Ingredient,
        result: Stack,
    },
    /// `RepairItemRecipe`：两个同名、数量 1、带 `max_damage` 和 `damage` 的物品。
    /// 用 [`is_damageable`] 判断，默认物品栈上可以完整匹配。
    RepairItem,
    /// `DecoratedPotRecipe`：四块陶片放在田字格里。
    DecoratedPot {
        back: Ingredient,
        left: Ingredient,
        right: Ingredient,
        front: Ingredient,
        result: Stack,
    },
}

impl RecipeKind {
    /// 对应官方的 `Recipe.getType()`。
    pub fn recipe_type(&self) -> RecipeType {
        match self {
            Self::Shaped { .. }
            | Self::Shapeless { .. }
            | Self::Transmute { .. }
            | Self::Dye { .. }
            | Self::Imbue { .. }
            | Self::FireworkRocket { .. }
            | Self::FireworkStar { .. }
            | Self::FireworkStarFade { .. }
            | Self::MapExtending { .. }
            | Self::BannerDuplicate { .. }
            | Self::BookCloning { .. }
            | Self::ShieldDecoration { .. }
            | Self::RepairItem
            | Self::DecoratedPot { .. } => RecipeType::Crafting,
            Self::Cooking { kind, .. } => match kind {
                CookingKind::Smelting => RecipeType::Smelting,
                CookingKind::Blasting => RecipeType::Blasting,
                CookingKind::Smoking => RecipeType::Smoking,
                CookingKind::Campfire => RecipeType::CampfireCooking,
            },
            Self::Stonecutting { .. } => RecipeType::Stonecutting,
            Self::SmithingTransform { .. } | Self::SmithingTrim { .. } => RecipeType::Smithing,
            Self::Brewing { .. } => RecipeType::Brewing,
        }
    }

    /// 不看输入就能确定的产物。
    pub fn result(&self) -> Option<Stack> {
        match *self {
            Self::Shaped { result, .. }
            | Self::Shapeless { result, .. }
            | Self::Dye { result, .. }
            | Self::Imbue { result, .. }
            | Self::Cooking { result, .. }
            | Self::Stonecutting { result, .. }
            | Self::SmithingTransform { result, .. }
            | Self::Brewing { result, .. }
            | Self::FireworkRocket { result, .. }
            | Self::FireworkStar { result, .. }
            | Self::FireworkStarFade { result, .. }
            | Self::BannerDuplicate { result, .. }
            | Self::BookCloning { result, .. }
            | Self::ShieldDecoration { result, .. }
            | Self::DecoratedPot { result, .. } => Some(result),
            Self::Transmute { .. }
            | Self::MapExtending { .. }
            | Self::SmithingTrim { .. }
            | Self::RepairItem => None,
        }
    }
}
