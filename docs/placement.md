# 方块行为设计（放置状态、形状更新、开关）

M2 放下的方块总是默认状态，门和床放不了，邻居不会更新。这一步让方块的行为和官方服务端一致，
范围以“玩家盖房子时看到的东西都对”为准。

官方源码只能读，不能抄进仓库。下面的类名都是官方类名。
移植是用 Rust 重新实现逻辑，不是逐行翻译：名字用 Rust 的风格，注释用中文写清楚为什么这样做。

## 1. 范围

要做：

1. 放置：所有方块物品走官方的 `BlockItem.place` 流程。`getStateForPlacement`、`canBeReplaced`、
   `canSurvive`、`isUnobstructed`（用真实碰撞形状）、`setPlacedBy`（门、床、高植物的另一半）。
2. 形状更新：`updateShape`、`updateNeighbourShapes`、`updateIndirectNeighbourShapes`，
   按官方的 `CollectingNeighborUpdater` 顺序连锁。栅栏互连、楼梯转角、箱子合并、失去支撑的方块消失。
3. 破坏：`playerWillDestroy`、`removeBlock`。破坏门的一半，另一半也消失。
4. 右键：`useItemOn`、`useWithoutItem`。门、活板门、栅栏门、拉杆、中继器、比较器这些只改状态的开关都要做，
   往花盆放植物、蛋糕插蜡烛这种只改方块的也要做。箱子这类要开界面的方块只吃掉点击，界面不做。
5. 计划刻：只做“放不住就消失”这一类（甘蔗、仙人掌、竹子这些在 `updateShape` 里排一个刻，到时候再判定）。
6. 非红石的 `neighborChanged`：铁轨失去支撑、中继器和红石线失去支撑。

不做，也不许半做：

- 红石信号。世界里处处没有信号：`hasNeighborSignal` 是假，`getSignal` 是 0。
  `powered` 属性只有玩家直接切换的（拉杆）才会变。按钮按下后弹回需要计划刻，可以做，归第 5 条。
- 流体流动、方块下落、火蔓延、植物生长、随机刻、掉落物、音效（D4 不变）。
- 方块实体的内容：告示牌文字、箱子物品、旗帜图案、头颅皮肤。
- 物品组件带来的差异：`block_state` 组件、`block_entity_data`。
- 需要光照引擎的判定：`getRawBrightness` 一律按 15 算（开阔的白天）。这是已知偏差，要写进 README 的已知限制。
- 需要 `level.getRandom()` 的分支。官方用随机数的地方要列出来，oracle 把这些情况排除。

D3（拒绝两格方块、默认含水方块、PlaceOnWater）作废，改成官方逻辑。GameMaster 物品仍然放不了：
官方要 gamemaster 权限，Bamboo 没有权限系统。

## 2. crate

新 crate `crates/mc-bamboo-blocks`（包名 `mc-bamboo-blocks`），带 LICENSE，依赖 `mc-bamboo-core`、`mc-bamboo-data`，
不依赖 `world`、`protocol`、`tokio`。它是纯逻辑：给一个能读写方块的 `Level`，算出世界的变化。
`mc-bamboo-game` 给 `Level` 做适配，`interaction.rs` 只留够距检查、序列号、发包。

```text
crates/mc-bamboo-blocks/src/
  lib.rs
  state.rs       BlockState、Property、取值和改值
  shape.rs       Aabb、碰撞形状查询、是否被实体挡住
  level.rs       Level trait、Effect
  world.rs       World：setBlock、destroyBlock、邻居队列、计划刻
  context.rs     UseContext（UseOnContext）、PlaceContext（BlockPlaceContext）
  placement.rs   BlockItem.place 和各物品类
  interact.rs    use_item_on、destroy_block 两个入口
  ticks.rs       计划刻队列
  behaviour/
    mod.rs       Behaviour trait、class -> 实现的分派表
    <family>.rs  每个家族一个文件
```

### 2.1 BlockState

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockState(pub u32);

impl BlockState {
    pub fn block(self) -> &'static Block;
    pub fn block_id(self) -> usize;
    pub fn is(self, default_state_of_block: u32) -> bool;       // 同一个方块，参数是 blocks::STONE 这样的常量
    pub fn is_tagged(self, tag: BlockTag) -> bool;
    pub fn get<T: PropertyValue>(self, property: Property<T>) -> T;      // 没有这个属性时 panic，和官方一样
    pub fn try_get<T: PropertyValue>(self, property: Property<T>) -> Option<T>;
    pub fn has<T>(self, property: Property<T>) -> bool;
    pub fn with<T: PropertyValue>(self, property: Property<T>, value: T) -> Self;
    pub fn is_air(self) -> bool;
    pub fn is_face_sturdy(self, face: Direction, support: SupportType) -> bool;
    pub fn fluid(self) -> FluidState;
    // 其余事实见第 3 章
}
```

`Property<T>` 是属性名和值类型的组合，常量放在 `state.rs`：`FACING`、`HORIZONTAL_FACING`、`AXIS`、`HALF`、`WATERLOGGED`、`OPEN`……
名字和官方 `BlockStateProperties` 对应。属性值是 Rust 枚举、`bool` 或 `i32`，和字符串互转。
方块放置不是热路径，按名字线性查找足够，不要为这个做缓存。

`BlockTag` 是官方标签名的包装。标签成员在第一次用时建成位图，查询只做下标访问。

### 2.2 Level 和 World

```rust
/// 世界的存储和世界外的一切。`mc-bamboo-game` 和 oracle 各有一个实现。
pub trait Level {
    /// 世界外返回 `void_air`。
    fn block_state(&self, pos: BlockPos) -> BlockState;
    /// 写入一格，不做任何连锁。高度超出世界时什么也不做，返回 `false`。
    fn write_block(&mut self, pos: BlockPos, state: BlockState) -> bool;
    /// 这些盒子（世界坐标）和所有挡路的实体都没有正体积重叠。
    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool;
}
```

`World<'a>` 持有 `&'a mut dyn Level`、邻居队列、计划刻队列的引用和收集到的 `Effect`。
`Behaviour` 的方法都通过 `World` 读写。`World` 的方法和官方同名：

- `set_block(pos, state, flags) -> bool`、`set_block_limited(pos, state, flags, limit) -> bool`：`Level.setBlock` 加 `LevelChunk.setBlockState`
  里的 `affectNeighborsAfterRemoval`、`onPlace`。不要漏掉 `(blockChanged || newBlock instanceof BaseRailBlock)` 这种细节。
- `remove_block(pos) -> bool`：`Level.removeBlock`。
- `destroy_block(pos, limit) -> bool`：`Level.destroyBlock`，不掉落。
- `neighbor_shape_changed`、`update_neighbors_at`：`CollectingNeighborUpdater` 的完整移植，包括分层栈的顺序。
- `schedule_tick(pos, block, delay)`。
- `raw_brightness(pos) -> i32`：固定 15。
- `has_neighbor_signal(pos) -> bool`：固定 `false`，写成函数是为了以后有红石时只改一处。

`Effect` 现在只有一种：

```rust
pub enum Effect {
    /// `levelEvent(2001, pos, state)`。`except` 是不用发的玩家，也就是自己已经预测过的发起者。
    Destroyed { pos: BlockPos, state: BlockState, except_initiator: bool },
}
```

### 2.3 Behaviour

```rust
pub trait Behaviour: Sync {
    fn state_for_placement(&self, block: &'static Block, ctx: &PlaceContext, world: &World) -> Option<BlockState>;
    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool;
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool;
    fn update_shape(&self, state: BlockState, world: &mut World, pos: BlockPos, direction: Direction,
                    neighbor_pos: BlockPos, neighbor: BlockState) -> BlockState;
    fn update_indirect_neighbour_shapes(&self, state: BlockState, world: &mut World, pos: BlockPos, flags: u32, limit: u32);
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState);
    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos);
    fn neighbor_changed(&self, state: BlockState, world: &mut World, pos: BlockPos);
    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor);
    fn player_will_destroy(&self, state: BlockState, world: &mut World, pos: BlockPos, player: &Actor) -> BlockState;
    fn use_item_on(&self, state: BlockState, world: &mut World, ctx: &UseContext) -> InteractionResult;
    fn use_without_item(&self, state: BlockState, world: &mut World, ctx: &UseContext) -> InteractionResult;
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos);
    fn collision_boxes(&self, state: BlockState, world: &World, pos: BlockPos, context: CollisionContext) -> Boxes;
}
```

每个方法都有和 `BlockBehaviour` 相同的默认实现。Java 的继承用组合表达：子类的实现直接调用父类结构体的方法，
相当于 `super.xxx`。分派表是 `mc_bamboo_data::BlockClass` 上的 `match`，新版本多一个类时编译失败，这正是想要的。

`Actor` 是发起动作的玩家：位置、`yaw`、`pitch`、`secondary_use`（官方 `isSecondaryUseActive`）、主副手物品。
`InteractionResult` 和官方同构：`Success`、`Consume`、`Fail`、`Pass`、`TryEmptyHandInteraction`。

### 2.4 入口

```rust
/// `ServerPlayerGameMode.useItemOn`，去掉观察者分支。够距、序列号这些由调用方处理。
pub fn use_item_on(level: &mut dyn Level, ticks: &mut BlockTicks, actor: &Actor, hand: Hand, hit: &BlockHit) -> Outcome;
/// `ServerPlayerGameMode.destroyBlock` 里 `canDestroyBlock` 之后的部分。
pub fn destroy_block(level: &mut dyn Level, ticks: &mut BlockTicks, actor: &Actor, pos: BlockPos) -> Outcome;
/// 到期的计划刻。
pub fn run_scheduled_ticks(level: &mut dyn Level, ticks: &mut BlockTicks, now: i64) -> Outcome;

pub struct Outcome { pub result: InteractionResult, pub effects: Vec<Effect> }
```

`BlockHit` 是 `BlockHitResult`：`pos`、`face`、`location: Vec3`、`inside`。`location` 用 `f64::from(pos) + f64::from(cursor)` 算，
和官方解码时的精度一样。

`mc-bamboo-core` 补上 `Axis`、`Direction::opposite/axis/clockwise/counter_clockwise/from_yaw/ordered_by_nearest` 等。
官方的 `Mth.sin`、`Mth.cos` 是查表的，`orderedByNearest` 必须用同样的表，否则边界角度上和客户端预测不一致。

## 3. 生成的数据

全部从官方代码运行时读出，写进 `mc-bamboo-data` 的生成文件。

- `Block` 增加 `class: BlockClass`（`block.getClass().getSimpleName()`）和 `properties: &'static [PropertyDef]`。
  `PropertyDef { name, values }` 按官方 `StateDefinition` 的顺序。生成时校验状态 ID 就是属性值的笛卡尔积，最后一个属性变化最快，
  不成立就报错。
- `Item` 增加 `wall_block: Option<i32>`、`attachment: Option<Direction 的 id>`（`StandingAndWallBlockItem` 的两个字段）。
- `STATE_FACTS`：每个状态一个数，位的含义在 `mc-bamboo-data/src/lib.rs` 里用常量写清楚：
  `isFaceSturdy` 三种 `SupportType` 各 6 位，`isAir`、`canOcclude`、`isSolid`、`blocksMotion`、`liquid`、`canBeReplaced()`、
  `isRedstoneConductor`、`isCollisionShapeFullBlock`、`isSolidRender`、`isSignalSource`、`hasAnalogOutputSignal`。
  读取时用 `EmptyBlockGetter.INSTANCE` 和 `BlockPos.ZERO`。结果依赖位置或上下文的状态要在生成时列出来，单独处理。
- `STATE_FLUID`：流体种类、是否源头、等级。`LIGHT_DAMPENING`（`getLightDampening`）。
- 形状：`SHAPES: &[&[Aabb]]` 去重后的盒子表，`COLLISION_SHAPE`、`OUTLINE_SHAPE`、`SUPPORT_SHAPE`（`getBlockSupportShape`）
  是每个状态到表下标的映射。碰撞形状用 `CollisionContext.empty()` 读，`placementContext` 下不同的状态单独处理。
- 家族需要的构造参数：`BlockSetType.canOpenByHand`、花盆的内容和对应关系、蜡烛蛋糕的对应关系、藤蔓的 head/body、珊瑚的死活对应，
  以及移植中发现需要的其他字段。读私有字段用反射。

新增的生成结果都不写死版本相关的数字。`generate` 要的是官方 bundler `server.jar`。

## 4. 验证：官方代码做 oracle

这是这一步最重要的部分。方块行为的分支太多，靠手写用例测不全，所以让官方代码出题和给答案：

`tools/mc-bamboo-vanilla/java/BlockOracle.java` 在 JVM 里加载官方类（和 Extractor 一样，不启动服务端，不涉及 EULA），
用一个假的世界运行**官方的** `useItemOn`、`destroyBlock` 流程，把场景和结果一行一行写到 stdout。
`tools/mc-bamboo-vanilla/src/oracle.rs` 读这些行，用 `mc-bamboo-blocks` 在同样的场景里运行，比较结果。

```bash
cargo run --release -p mc-bamboo-vanilla -- oracle <java> [--cases N] [--seed S] [--only <方块或类名>]
```

假世界的要求：

- 继承 `ServerLevel`，用 `sun.misc.Unsafe.allocateInstance` 跳过构造函数，覆盖所有用到的方法，这样 `level instanceof ServerLevel`
  的分支和真实服务端一样走。玩家同理，继承 `ServerPlayer`。
- 方块存在 `HashMap` 里，范围外是 `void_air`，没设置的位置是空气。
- `setBlock` 按官方 `Level.setBlock` 和 `LevelChunk.setBlockState` 写，保留 `onPlace`、`affectNeighborsAfterRemoval`、
  `updateNeighborsAt`、`updateNeighbourShapes`，用真实的 `CollectingNeighborUpdater`。
- `destroyBlock` 里 `dropResources` 恒为假，不算掉落。
- 信号相关的方法恒为 0，亮度恒为 15。随机数用固定种子，走到随机分支的类列在排除表里。
- `isUnobstructed` 用场景里玩家的碰撞盒做 `Shapes.joinIsNotEmpty`。
- 记录 `levelEvent(2001)` 和 `scheduleTick`。
- 标签要绑定，走 Extractor 的 `loadRegistries`。

场景要覆盖：

- 每个方块物品：平地、墙边、天花板下，点六个面，点击点在上下半、左右半，八个以上朝向和多个俯仰角，蹲和不蹲。
- 邻居：同家族方块的所有状态（楼梯的各种朝向、半、形状，栅栏、墙、玻璃板、箱子），实心方块、台阶、空气、水。
- 替换：点在草、雪、藤蔓、台阶、蜡烛、海泡菜这些能被替换或叠加的方块上。
- 破坏：门、床、高植物的两半，失去支撑的方块，形状更新的连锁。
- 右键：所有状态的方块，空手和拿着东西，蹲和不蹲，主手和副手。
- 挡路：玩家站在目标格、半格、一格外。
- 计划刻：方块放不住时运行 `tick`。

每一行是一个 JSON 对象，字段见 `BlockOracle.java` 顶部的注释。比较内容：`InteractionResult`、世界里所有变化的格子、
`levelEvent` 的列表。计划刻的列表也比较，但只比较第 1 章第 5 条范围内的。

输出按类汇总：每个类跑了多少个场景、错了多少个、前几个错误的完整场景。全部通过之前不算完成。
**第一次全绿要先故意改坏几处确认 oracle 真的能发现问题。**

排除表写在 `oracle.rs` 里，每一条都有原因。排除的内容要进最后的报告。

## 5. 分工

先做基础（数据、crate 骨架、oracle），再分家族并行移植，最后接进 `mc-bamboo-game`。

| 家族文件 | 类 |
|---|---|
| `building` | 柱子、台阶、楼梯、栅栏、栅栏门、墙、玻璃板、铁栏杆、门、活板门、床、锁链、地毯、带釉陶瓦、巨型蘑菇、雪草这些建筑方块 |
| `attached` | 火把、告示牌、悬挂告示牌、旗帜、头颅、梯子、拉杆、按钮、压力板、灯笼、末地烛、避雷针、钟、绊线钩、架子、雕纹书架 |
| `plants` | `BushBlock` 系列、高植物、作物、茎、蘑菇、珊瑚、荷叶、藤蔓、`GrowingPlant` 系列、竹子、仙人掌、甘蔗、紫颂、滴叶、垂根、`MultifaceBlock` 系列 |
| `machines` | 活塞、发射器、投掷器、侦测器、中继器、比较器、阳光探测器、漏斗、铁轨、红石线、红石火把、熔炉系列、箱子系列、木桶、末影箱、潜影盒、讲台、铁砧、砂轮、切石机、织布机、营火、蜂巢、南瓜灯、绊线、音符盒、命令方块系列 |
| `misc` | 雪层、蜡烛、蜡烛蛋糕、蛋糕、海泡菜、海龟蛋、脚手架、滴水石锥、紫晶簇、混凝土粉末、下落方块、炼药锅、堆肥桶、花盆、火、液体、耕地、土径、树叶、海绵、TNT、重生锚 |

每个家族负责这些类的全部行为（放置、形状更新、存活、破坏、右键、计划刻）。
官方的类名在 oracle 的汇总里能看到，有类不属于任何家族时归 `misc`。

## 6. 规矩

- 注释用中文，日志和错误信息用英文。`rustfmt`，`cargo clippy --workspace --all-targets -- -D warnings`，不加 `allow`。
- 工作区 `rust-version` 是 1.96，本机 rustc 更新，不要用 1.96 之后的 API。
- 不改 `crates/mc-bamboo/src/main.rs`，那里有用户没提交的改动。
- 不启动真实的 Minecraft 服务端或客户端，不接受 EULA。
- 官方代码不进仓库。
- 函数短小，一个函数一件事。不写没用的抽象。不写重复官方类名以外的废话注释。
- 不提交，不推送。
