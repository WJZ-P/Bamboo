# 村民：官方机制摸底与分期方案

> **状态：待主代理和用户拍板。** 本轮只读调研，不改生产代码、不跑 `cargo test` / oracle、
> 不启动真实服务端或客户端。所有官方行为都用 `javap -c -p` 读
> `/Users/wjz/Desktop/Github/Bamboo/target/vanilla/versions/26.3/server-26.3.jar`
> 的字节码（未混淆，类名 / 方法名是真名），数据文件用 `unzip -p` 从同一个 jar 里读；
> Bamboo 侧现状按 `origin/main b7baeb26` 的工作树源码定位。没有把反编译源码抄进仓库。
>
> **最重要的版本差异**：26.3 里**没有 `VillagerAi`，也没有 `Schedule` / `ScheduleBuilder`**。
> 大脑改成 `Brain.Provider` + `ActivityData` + 数据驱动的 `timeline/villager_schedule.json`；
> 交易改成 `trade_set` / `villager_trade` 数据包。下文按 26.3 的真实结构写，不按旧版印象。
>
> 路径简写：`brain/*` = `crates/mc-bamboo-blocks/src/entity/mob/brain/*`；
> `mob/*` = `crates/mc-bamboo-blocks/src/entity/mob/*`；`entity/*` =
> `crates/mc-bamboo-blocks/src/entity/*`；`game/*` = `crates/mc-bamboo-game/src/*`；
> `storage/*` = `crates/mc-bamboo-storage/src/*`；`worldgen/*` =
> `crates/mc-bamboo-worldgen/src/*`；`oracle/*` = `tools/mc-bamboo-vanilla/src/mob_oracle/*`、
> `tools/mc-bamboo-vanilla/java/MobOracle.java`。

---

## 0. 结论摘要

1. **POI 是整套机制的地基**：工作站、床、钟、避雷针、`isCloseToVillage`、
   `CatSpawner`、`WanderingTraderSpawner`、村庄判定全读 `PoiManager`。但它同时是
   **唯一需要新增一套「区块旁的第二份持久数据」** 的部件，和 `ChunkStore`、岛的拆合、
   存盘编排都要接线，所以必须单独成一期、先验收。
2. **交易在 26.3 是数据包，不是 Java 表**：`VillagerTrades` 只剩 `bootstrap` 注册键；
   运行时走 `VillagerProfession.getTrades(level)` → `trade_set/<prof>/level_N.json`
   → `tags/villager_trade/<prof>/level_N.json` → `villager_trade/**.json`。
   这带来一个独立的数据导出子任务（内置 68 个 `trade_set` + 391 个
   `villager_trade` + 73 个 `tags/villager_trade`，另有 `trade_rebalance`
   数据包的 81+10 个覆盖文件），也带来「是否支持 `trade_rebalance`
   数据包覆盖」的范围决定。
3. **大脑可以大量复用**：`brain/` 的记忆槽、传感器调度、活动 / 优先级 / 桶序、
   `Behavior` / `OneShot` / `DoNothing` / `GateBehavior`、`RunOne` 洗牌种子、`Brain` NBT
   存档、以及 `mob-oracle` 的结构表 / 逐刻记忆对拍都在。真正缺的是：
   8 个新 `Activity` 变体、`GlobalPos` 型记忆值、`schedule` 字段 +
   `updateActivityFromSchedule`（timeline / 环境属性）、约 40 个村民专用行为、
   以及把 `World`/`EntityCtx` 接到 POI 查询。
4. **期数建议 8 期**，每期都能单独对拍：① POI ② 数据导出 + 村民实体骨架
   ③ 大脑 / 日程 ④ 职业工作站 ⑤ 交易 ⑥ 八卦 / 繁殖 / 铁傀儡
   ⑦ 刷怪器与转化 ⑧ 结构实体与收尾。工作量粗估 **15–19 人周**
   （结构表先行可压到 13–16；不含袭击、不含客户端 UI 的完整体验打磨），详见 §5。
5. **两个必须先拍板的决定**（§7）：
   - 期 1 先做 POI（建议），还是先做「实体骨架 + 数据导出」；
   - `trade_rebalance` 数据包覆盖是否进首版范围（建议不进）。

---

## 1. 官方机制摘要

### 1.1 大脑（`Brain` / `ActivityData` / timeline）

**构造与活动表。** `Villager.BRAIN_PROVIDER`（`Villager` 静态块）=
`Brain.provider(sensorList, Villager::lambda$static$0)`：

- 九个传感器，注册顺序固定（`SensorType`）：
  `NEAREST_LIVING_ENTITIES`、`NEAREST_PLAYERS`、`NEAREST_ITEMS`、`NEAREST_BED`、
  `HURT_BY`、`VILLAGER_HOSTILES`、`VILLAGER_BABIES`、`SECONDARY_POIS`、`GOLEM_DETECTED`。
  各 `scanRate`：`SecondaryPoiSensor` 40、`GolemSensor` 200、`NearestBedSensor` 20、
  其余默认 20（`Sensor.<init>(int)` / `Sensor.<init>()`）。`GolemSensor.requires()` =
  `{MOBS, GOLEM_DETECTED_RECENTLY}`，命中给 599 刻的 `GOLEM_DETECTED_RECENTLY`。
  `VillagerHostilesSensor` 是 `NearestVisibleLivingEntitySensor` 子类，`isHostile` 用
  `ACCEPTABLE_DISTANCE_FROM_HOSTILES` 的 11 项表（drowned / husk / vex / zombie /
  zombie_villager 8 格，evoker / illusioner / ravager 12 格，vindicator / zoglin
  10 格，pillager 15 格），按 `distanceToSqr <= r²` 写 `NEAREST_HOSTILE`。
- `Brain$Provider.makeBrain(E, Brain.Packed)` 调 `ActivitySupplier.createActivities(E)`
  拿 `List<ActivityData<E>>`，再 `new Brain(memoryTypes=[], sensorTypes, activities,
  packed.memories, random)`。`Brain$Provider` 的 `memoryTypes` 在村民这里**是空表**，
  记忆只从 `Sensor.requires()` 和行为的 `getRequiredMemories()` 登记（Bamboo 的
  `Brain::new` 已经是这个语义，见 `brain/mod.rs` 的 `frog_activity_conditions_need_registered_memories_like_vanilla` 测试）。
- `lambda$static$0(Villager)` 按 `isBaby()` 分叉，速度一律 `0.5f`：
  | 活动 | 条件 | 行为包 |
  |---|---|---|
  | `PLAY`（幼年） | 无 | `VillagerGoalPackages.getPlayPackage(0.5f)` |
  | `WORK`（成年） | `JOB_SITE = VALUE_PRESENT` | `getWorkPackage(profession, 0.5f)` |
  | `CORE` | 无 | `getCorePackage(profession, 0.5f)` |
  | `MEET` | `MEETING_POINT = VALUE_PRESENT` | `getMeetPackage(0.5f)` |
  | `REST` | 无 | `getRestPackage(0.5f)` |
  | `IDLE` | 无 | `getIdlePackage(0.5f)` |
  | `PANIC` | 无 | `getPanicPackage(0.5f)` |
  | `PRE_RAID` | 无 | `getPreRaidPackage(0.5f)` |
  | `RAID` | 无 | `getRaidPackage(0.5f)` |
  | `HIDE` | 无 | `getHidePackage(0.5f)` |
  优先级用 `Pair.of(int, BehaviorControl)` 显式给出；`getCorePackage` 的
  `0..5` 段最密集（`Swim`、`InteractWithDoor`、`LookAtTargetSink`、
  `VillagerPanicTrigger`、`WakeUp`、`ReactToBell`、`SetRaidStatus`、
  `ValidateNearbyPoi(JOB_SITE/POTENTIAL_JOB_SITE)`、`MoveToTargetSink`、
  `PoiCompetitorScan`、`LookAndFollowTradingPlayerSink`、`GoToWantedItem`、
  `AcquirePoi` ×3、`GoToPotentialJobSite`、`YieldJobSite`、
  `AssignProfessionFromJobSite`、`ResetProfession`）。每个非 PANIC 包的末尾是
  优先级 99 的 `UpdateActivityFromSchedule.create()`。
  `getWorkPackage` 还有 `WorkAtComposter`、`WorkAtPoi`、`StrollAroundPoi`、
  `StrollToPoi`、`StrollToPoiList`、`HarvestFarmland`、`UseBonemeal`、
  `ShowTradesToPlayer`、`SetLookAndInteract`、`SetWalkTargetFromBlockMemory`、
  `GiveGiftToHero`；`getMeetPackage` 有 `SocializeAtBell`、`TradeWithVillager`。
  `getIdlePackage` 的末段是 `VillagerMakeLove`（闸门条件 `BREED_TARGET`）。
- `makeBrain(Brain.Packed)` → `BRAIN_PROVIDER.makeBrain(...)` + `registerBrainGoals(brain)`。
  `registerBrainGoals` 只做两件事：`brain.setSchedule(EnvironmentAttributes
  .VILLAGER_ACTIVITY / BABY_VILLAGER_ACTIVITY)` 和
  `brain.updateActivityFromSchedule(level.environmentAttributes(), level.getGameTime(), position())`。
  `ageBoundaryReached()`、`AssignProfessionFromJobSite`、`ResetProfession`、
  僵尸村民治疗完成都会 `Villager.refreshBrain(ServerLevel)`：
  `brain.stopAll` → `BRAIN_PROVIDER.makeBrain(this, brain.pack())` → `registerBrainGoals`。
  **换职业 / 长大要重建整个大脑，但记忆 `pack()` 带过去。**

**日程（26.3 的数据驱动形式）。** `data/minecraft/timeline/villager_schedule.json`：

```json
{"clock":"minecraft:overworld","period_ticks":24000,
 "tracks":{
  "minecraft:gameplay/villager_activity":{"keyframes":[
   {"ticks":10,"value":"minecraft:idle"},{"ticks":2000,"value":"minecraft:work"},
   {"ticks":9000,"value":"minecraft:meet"},{"ticks":11000,"value":"minecraft:idle"},
   {"ticks":12000,"value":"minecraft:rest"}]},
  "minecraft:gameplay/baby_villager_activity":{"keyframes":[
   {"ticks":10,"value":"minecraft:idle"},{"ticks":3000,"value":"minecraft:play"},
   {"ticks":6000,"value":"minecraft:idle"},{"ticks":10000,"value":"minecraft:play"},
   {"ticks":12000,"value":"minecraft:rest"}]}}}
```

`tags/timeline/universal.json` 只含 `minecraft:villager_schedule`；
`tags/timeline/in_overworld.json` = universal + `day` / `moon` / `early_game`。
`Timelines.bootstrap` 把两条 track 绑到 `EnvironmentAttributes.VILLAGER_ACTIVITY` /
`BABY_VILLAGER_ACTIVITY`（`AttributeTypes.ACTIVITY`，默认值 `Activity.IDLE`）；
`lambda$bootstrap$20/21` 的关键帧与 json 逐值一致。
`Brain.updateActivityFromSchedule(EnvironmentAttributeSystem, long, Vec3)`：
`gameTime - lastScheduleUpdate > 20` 才重算，取
`attrs.getValue(schedule, pos)`（无 schedule 时 `IDLE`），若不在
`activeActivities` 里就 `setActiveActivityIfPossible(activity)`（条件不满足回退
`useDefaultActivity` → `IDLE`）。`UpdateActivityFromSchedule.create()` 的触发器
就是每刻把这个调用打一遍，所以 **20 刻节流由 `Brain` 自己保证**。
`Villager.shouldRestock` 还用 `Timelines.OVERWORLD_DAY` 的
`Timeline.getPeriodCount(clockManager)` 取「第几个游戏日」。

**`Brain.tick` 顺序**（`Brain.tick`）：`forgetOutdatedMemories` → `tickSensors`
（按登记顺序）→ `startEachNonRunningBehavior`（按优先级 / 活动桶序）→
`tickEachRunningBehavior`。Bamboo `brain/mod.rs::Brain::tick` 已经逐字照做。

**可直接复用 / 需要新做** 的清单：

| 件 | 现状 | 结论 |
|---|---|---|
| `Brain` 骨架、记忆过期、`MemoryStatus`、`isEmptyCollection`、`checkMemory` | `brain/memory.rs` 已有 | 复用 |
| 行为遍历顺序（`TreeMap` + `HashMap` 桶序 + `LinkedHashSet`） | `brain/mod.rs::order_entries` 已有 | 复用 |
| `Behavior` / `OneShot` / `DoNothing` / `GateBehavior` / `RunOne` 洗牌种子 | `brain/behavior.rs`、`behaviors.rs` 已有 | 复用 |
| `Brain.Packed` NBT `{memories:{name:{value,ttl}}}` | `brain/nbt.rs` 已有 | 复用 |
| `Sensor` 的 `scanRate` + 随机起始延迟 | `brain/sensor.rs` 已有 | 复用 |
| `mob-oracle` 的 `activities\|running\|memories` 逐刻转储、结构表 | `oracle/brain.rs`、`oracle/water_mob.rs`、`java/MobOracle.java` 已有 | 复用，扩新类 |
| `Activity` 变体 | 只有 Core/Idle/Panic/LongJump/Ram/Swim/LaySpawn/Tongue/PlayDead/Fight | **新加** Work/Play/Rest/Meet/Raid/PreRaid/Hide/Celebrate（含 `java_hash` 测试） |
| `MemoryValue::GlobalPos` + 约 20 种 POI / 村庄记忆 | `MemoryType::COUNT = 32`，无 `GlobalPos` | **新加**（`HOME`、`JOB_SITE`、`POTENTIAL_JOB_SITE`、`MEETING_POINT`、`SECONDARY_JOB_SITE`、`NEAREST_BED`、`LAST_SLEPT`、`LAST_WOKEN`、`LAST_WORKED_AT_POI`、`HEARD_BELL_TIME`、`GOLEM_DETECTED_RECENTLY`、`VISIBLE_VILLAGER_BABIES`、`BREED_TARGET`、`INTERACTION_TARGET`、`NEAREST_HOSTILE`、`NEAREST_VISIBLE_WANTED_ITEM`…；已有 `BreedTarget` / `NearestVisibleWantedItem` 等要换值类型）；追加在枚举末尾保住既有下标 |
| 9 个传感器中已有的 `NearestLivingEntities` / `NearestPlayers` / `NearestItems` / `HurtBy` | `SensorKind` 已有 4 个 | 复用 |
| `NearestBed` / `VillagerHostiles` / `VillagerBabies` / `SecondaryPois` / `GolemDetected` | 无 | **新做**（含 `requires()`、`class_name()`、`scan_rate()`） |
| `schedule` 字段 + `updateActivityFromSchedule` + timeline / 环境属性 | 无 | **新做**；Bamboo 已有 `Global::sky_darken` 用 `day.json` 的 `sky_light_level` 关键帧，可照同一思路加一个「按主世界时钟查 `villager_schedule` 两条 track」的 `GlobalView` 字段，避免引入完整 `EnvironmentAttributeSystem` |
| 约 40 个村民专用行为 | 无 | **新做**；先按「结构表占位 + 少数真逻辑」分批（§4 期 3） |
| `Brain::refresh` / `pack → new`（换职业、长大） | `Brain::new` 有，`stop_all` 有 | **补一个** `rebuild(provider)` 或让村民侧重建 |
| `Cx` 能查 POI / 生成实体 / 开菜单 | `Cx` 只有 `me/mob/ctx/memories/time` | **扩接口**（`EntityCtx` 加只读 POI 视图、生成命令） |

### 1.2 兴趣点（POI）

**存储与索引。** `PoiManager extends SectionStorage<PoiSection, PoiSection.Packed>`，
`MAX_VILLAGE_DISTANCE = 6`、`VILLAGE_SECTION_SIZE = 1`。 Constructor 用
`RegionStorageInfo(levelId, dimension, "poi")` + `path.resolve("poi")` +
`DataFixTypes.POI_CHUNK`，即 **`poi/` 目录与 `region/` / `entities/` 平级**
（`ChunkMap` 构造函数）。

- 键是 `SectionPos.asLong(sectionX, sectionY, sectionZ)`（16³ 段），
  `SectionStorage.getKey(ChunkPos,int)` 按 `(chunk.x, y, chunk.z)` 打包；
  `get(long)` / `getOrLoad(long)` / `getOrCreate(long)` 惰性读盘。
- `PoiSection`：`Short2ObjectMap<PoiRecord> records`（键 =
  `SectionPos.sectionRelativePos(pos)` 的 16 位段内下标）+ `Map<Holder<PoiType>,
  Set<PoiRecord>> byType` + `Runnable setDirty` + `boolean isValid`。
  `PoiSection.getRecords(typePredicate, Occupancy)` 先按 `byType` 过滤，再按
  `Occupancy`（`HAS_SPACE` / `IS_OCCUPIED` / `ANY`）过滤。
- `PoiRecord`：`BlockPos pos`、`Holder<PoiType> poiType`、`int freeTickets`
  （初值 `PoiType.maxTickets`）；`acquireTicket()` 有票才 -1 并标脏，
  `releaseTicket()` 未达上限才 +1 并标脏，`hasSpace()` / `isOccupied()`。
  客户端值不能触发 panic：所有 `take` / `release` 入口都返回 `boolean` / `Optional`。
- `PoiType` 是 record：`Set<BlockState> matchingStates`、`int maxTickets`、
  `int validRange`；`PoiTypes.forState(BlockState)` 查静态 `TYPE_BY_STATE`，
  `hasPoi(state)`。注册的 21 种（括号是 `maxTickets, validRange`）：
  13 个工作站 `armorer`(blast_furnace)、`butcher`(smoker)、
  `cartographer`(cartography_table)、`cleric`(brewing_stand)、`farmer`(composter)、
  `fisherman`(barrel)、`fletcher`(fletching_table)、`leatherworker`(cauldrons)、
  `librarian`(lectern)、`mason`(stonecutter)、`shepherd`(loom)、
  `toolsmith`(smithing_table)、`weaponsmith`(grindstone) 都是 `(1,1)`；
  `home` = 所有颜色的床且 `PART=HEAD`，`(1,1)`；`meeting` = bell，`(32,6)`；
  `beehive` / `bee_nest` / `nether_portal` / `lodestone` / `lightning_rod` /
  `test_instance` 都是 `(0,1)`（0 票，只做存在性 / 距离查询）。
- `PoiManager` 查询族：`exists`、`getType`、`existsAtPosition`、`getCountInRange`、
  `getInSquare`（`radius/16 + 1` 的区块方环）、`getInRange`（欧氏距离）、
  `getInChunk`、`findAll(WithType/ClosestFirstWithType)`、`findClosest(WithType)`、
  `take(typePredicate, biPredicate, pos, range)`（`HAS_SPACE` 里取第一个并 `acquireTicket`）、
  `getRandom`（`Util.toShuffledList`）、`release`（`releaseTicket`）。
  `DistanceTracker`（`DynamicGraphMinFixedPoint`）维护
  `sectionsToVillage(SectionPos)`：有 `meeting` POI 的段是 0 级源，向 6 邻段扩散到
  `MAX_VILLAGE_DISTANCE`；`ServerLevel.isVillage(pos)` = `isCloseToVillage(pos,1)`，
  `isCloseToVillage(pos,sectionDistance)` 超过 6 直接假。
  `PoiManager.tick` = `SectionStorage.tick` + `distanceTracker.runAllUpdates()`；
  `setDirty` / `onSectionLoad` 都会把段喂给 `DistanceTracker`。

**方块登记 / 注销。** `Level.setBlock`（`Level.java:setBlock` 尾部）调
`updatePOIOnBlockStateChange(pos, oldState, newState)`；基类空实现，
`ServerLevel.updatePOIOnBlockStateChange` 先算 `PoiTypes.forState(old)` /
`forState(new)`，**两者相等就直接返回**，否则把 `pos.immutable()` 交给
`Optional.ifPresent`，经 `MinecraftServer.execute(Runnable)` 回主线程调
`PoiManager.remove(pos)` 和 `PoiManager.add(pos, holder)`（并发安全都在
`SectionStorage` 里）。另有 `PoiManager.checkConsistencyWithBlocks(SectionPos,
LevelChunkSection)`：段第一次加载 / 数据版本变化时 `refresh()` 逐格扫 16³ 个位置，
用 `PoiTypes.forState` 重建并保留 `freeTickets`（`isValid` 位控制只做一次）。
`ensureLoadedAndValid(LevelReader, pos, radius)` 在查询前预取周边区块的段。
避雷针的用途：`ServerLevel.findLightningRod(BlockPos)` 用 `findClosest`
（类型 `LIGHTNING_ROD`、128 格、`ANY`、且高度图 `WORLD_SURFACE - 1`），
被 `thunderHit` 相关的雷击逻辑调用。

**存档格式。** `SectionStorage.writeChunk` 生成的 NBT 根：

```
{
  "Sections": {
     "<minSectionY..maxSectionY 的十进制字符串>": PoiSection.Packed
  },
  "DataVersion": <当前版本 int>
}
PoiSection.Packed = { "Valid": bool(默认 false), "Records": [PoiRecord.Packed] }
PoiRecord.Packed  = { "pos": BlockPos(int array / codec), "type": "minecraft:xxx",
                      "free_tickets": int (默认 0) }
```

`PoiSection$Packed.CODEC` 的字段名由字节码确认为 `Valid`（`lenientOptionalFieldOf`
默认 false）、`Records`；`PoiRecord$Packed.CODEC` 为 `pos` / `type` /
`free_tickets`。`SimpleRegionStorage.write` 再补 `DataVersion`；文件落在
`poi/r.<X>.<Z>.mca`（`IOWorker` 的 Anvil 格式，和 `region/` 同格式不同目录）。
`SectionStorage` 的 `dirtyChunks` 按 `ChunkPos` 去重，`flush` / `flushAll` 才写盘。

**Bamboo 怎么放（建议）。**

- 新增 `mc-bamboo-world`（或 `mc-bamboo-storage`）里的 `PoiStore`，**按 `SectionPos`
  索引、按 `ChunkPos` 分脏**，与 `ChunkStore` 平级；不要塞进 `ChunkStore`
  内部（`ChunkStore::chunk_mut` 的 `ChunkRevision` 是区块包缓存的版本，POI 改动
  不该触发区块重发）。
- 数据落点：`storage/world_dir.rs::poi_dir()` 已经建好目录（`create_layout`
  和 `has_legacy_layout` 都认它），只需接一份 `SectionStorage` 风格的
  encode/decode 到 `mc-bamboo-nbt` + 已有的 anvil 写盘路径
  （`storage/anvil.rs`、`chunk_io.rs`）。`DataVersion` 用
  `mc_bamboo_storage::version::DATA_VERSION`（现有 chunk / entity 文件同一常量）。
- 方块登记：`ChunkStore::set_block`（`mc-bamboo-world/src/store.rs:447`）是唯一的
  方块写入口，在这里比对旧 / 新 `PoiTypes.forState`，产出 `PoiDelta` 交给
  `Region.poi`；`Region` 在 `interaction::send_effects` 的同一刻应用，避免照搬
  官方的 `MinecraftServer.execute` 异步队列带来的顺序不确定性（**这是一个明确
  的与官方差异，需在文档 / 对拍里标注**）。
- 拆合：`RegionColumn` / `RegionColumnParts`（`game/region/column.rs`）现在带
  `column / ticks / block_entities / redstone / containers / block_items /
  command_blocks / books / entities`，`IslandPart`（`game/region/island_split.rs:147`）
  有 25 类。**POI 应该成为 `RegionColumn` 的第十个字段**（按 `SectionPos.y`
  分组的 `Packed` 段），并新增 `IslandPart::Poi`，跟 `Chunks` 一起按区块切。
  理由：POI 是「方块状态的派生索引」，和 `block_entities` 一样必须和方块同批搬，
  否则拆岛后村民的 `HOME` 会指向另一个岛的段，而 `GlobalPos` 记忆只带坐标不带岛号。
  `DistanceTracker` 的缓存不必搬，重建即可（官方也只在加载时重算）。
- 存盘编排：`game/world.rs` 的 `SaveSession` / `host.save_loaded_dirty(&mut region)`
  现在只拍区块；`storage/chunk_content.rs` 只管 `region/` 与 `entities/`。
  建议让 `SaveSession` 同时收 `Region.poi.take_dirty_chunks()`，编码成 `poi/`
  的 `ChunkPos → Tag`，走同一个 IO 队列；卸载 / `/save-all` 的栅栏要一起等。
- 岛间隔 ≥128 格、视野不跨岛（`docs/island-implementation-plan.md` §0 第 3 条），
  所以 POI 查询的 48 格 / 128 格上限都在本岛内；但 `WanderingTraderSpawner`
  的 48 格查询、`findLightningRod` 的 128 格查询仍要显式拒绝跨岛结果。

### 1.3 职业与工作站

- `VillagerData` = record `(Holder<VillagerType> type, Holder<VillagerProfession>
  profession, int level)`；`MIN/MAX_VILLAGER_LEVEL` = 1 / 5，
  `NEXT_LEVEL_XP_THRESHOLDS = [0,10,70,150,250]`，
  `canLevelUp(level) = 1 <= level < 5`；`getMinXpPerLevel(level)` =
  `thresholds[level-1]`，`getMaxXpPerLevel(level)` = `thresholds[level]`。
  CODEC 键是 `type` / `profession` / `level`（`VillagerData.CODEC` 中 `level` 缺省 1）。
  `createDefaultVillagerData()` = `(plains, none, 1)`。
  `VillagerType.byBiome(Holder<Biome>)` 七种：desert / jungle / plains / savanna /
  snow / swamp / taiga；`finalizeVillagerType` 在 `Villager.finalizeSpawn` 里定品种。
- `VillagerProfession` = record `(name, heldJobSite, acquirableJobSite,
  requestedItems, secondaryPoi, workSound, Int2ObjectMap<ResourceKey<TradeSet>>
  tradeSetsByLevel)`。`bootstrap` 给 15 种职业注册（`none` / `nitwit` 用
  `PoiType.NONE` 和 `ALL_ACQUIRABLE_JOBS`；其余 13 种各绑定一个 `PoiTypes.*`
  和 1–5 级的 `TradeSets.<PROF>_LEVEL_N`）。`getTrades(level)` 查表。
  `Villager.POI_MEMORIES` = `ImmutableMap.of(HOME→is(HOME), JOB_SITE→
  profession.heldJobSite(), POTENTIAL_JOB_SITE→ALL_ACQUIRABLE_JOBS,
  MEETING_POINT→is(MEETING))`，`releaseAllPois()` / `releasePoi(memoryType)`
  在死亡 / 记忆清除时 `PoiManager.release(globalPos.pos())`。
- **获取工作站**：`AcquirePoi.create(typePredicate, memoryType,
  [secondaryMemoryType,] jobSiteForBabyFlag, Optional<Byte> eventId, biPredicate)`。
  每 20 刻一次（`gameTime + 20 + nextInt(20)` 抖动），从
  `PoiManager.findAllClosestFirstWithType(..., HAS_SPACE, 半径)` 里按
  `JitteredLinearRetry`（最多 5 次尝试、间隔 20 刻的线性退避）挑一个，
  `take(...)` 扣票、写 `GlobalPos(dimension, pos)` 到目标记忆、发实体事件。
  `getCorePackage` 里三个 `AcquirePoi` 分别对应 `POTENTIAL_JOB_SITE`（职业可获取）、
  `HOME`、`MEETING_POINT`。
- **认领职业**：`AssignProfessionFromJobSite.create()` 的 OneShot 触发器要
  `POTENTIAL_JOB_SITE` 存在、`JOB_SITE` 已登记、且 `pos.closerToCenterThan(
  villager.position(), 2.0)`；然后 `erase(POTENTIAL_JOB_SITE)`、
  `set(JOB_SITE, globalPos)`、实体事件 14；若当前职业是 `none`，查
  `PoiManager.getType(pos)` 再遍历 `VILLAGER_PROFESSION` 注册表找第一个
  `heldJobSite().test(poiType)` 的职业，`setVillagerData(withProfession(...))`
  + `refreshBrain(level)`。
- **失业**：`ResetProfession.create()` 在 `JOB_SITE = VALUE_ABSENT` 且职业不是
  `none` / `nitwit` 且 `villagerXp == 0` 且 `level <= 1` 时改成 `none` 并
  `refreshBrain`。`Villager.setVillagerData` 在职业变化时把 `offers` 置 `null`
  （下次 `getOffers()` 会 `updateTrades` 重建）。
- **等级与经验**：`AbstractVillager.notifyTrade(offer)` → `offer.increaseUses()`
  + `rewardTradeXp(offer)`。`Villager.rewardTradeXp`：`villagerXp += offer.getXp()`、
  记 `lastTradedPlayer`、`shouldIncreaseLevel()`（`canLevelUp(level) &&
  villagerXp >= getMaxXpPerLevel(level)`）时 `increaseMerchantCareer`
  （`withLevel(level+1)` → `updateTrades` 追加该级交易、给 200 刻 REGENERATION）、
  经验球 `3 + random.nextInt(4)`（升级再 +5）发给交易玩家。
  `customServerAiStep` 下一刻对 `lastTradedPlayer` 发
  `ReputationEventType.TRADE` 和实体事件 14。
- **工作**：`WorkAtPoi`（`JOB_SITE=VALUE_PRESENT`、`LOOK_TARGET=REGISTERED`）每
  300 刻且 `level.random.nextInt(2) != 0` 才启动，要求同维度且
  `pos.closerToCenterThan(villager.position(), 1.73)`；`start` 写
  `LAST_WORKED_AT_POI`、把工作站设成 `BlockPosTracker` 的 LOOK_TARGET、
  `playWorkSound()`、`useWorkstation()`（基类空），**最后
  `if (villager.shouldRestock(level)) villager.restock()`** —— 这是「先要交易
  才能补货」这条依赖的来源。

### 1.4 交易

**数据来源（26.3）。** `VillagerProfession.getTrades(level)` →
`Villager.updateTrades(ServerLevel)` → `AbstractVillager.addOffersFromTradeSet(
level, offers, tradeSetKey)` → `addOffersFromItemListingsWithoutDuplicates(
LootContext, offers, HolderSet<VillagerTrade>, amount)`。

- `data/minecraft/trade_set/<prof>/level_N.json`（68 个 `.json` 文件；
  13 职业 × 5 级 + 流浪商人的 buying/common/uncommon），形如
  `{"amount":2,"random_sequence":"minecraft:trade_set/armorer/level_1",
    "trades":"#minecraft:armorer/level_1"}`；`TradeSet` 的字段是
  `trades / amount / allow_duplicates(默认 false) / random_sequence`，
  `calculateNumberOfTrades(LootContext)` 用 `ContextIntProvider` 求值。
- `data/minecraft/tags/villager_trade/<prof>/level_N.json` 把标签展开成
  `minecraft:<prof>/<level>/<name>` 条目（可嵌套其他标签，如
  `#minecraft:common_smith/level_1`）。
- `data/minecraft/villager_trade/**.json`（内置 391 个 `.json`；`trade_rebalance`
  数据包目录下另有 81 个同路径覆盖文件 + 10 个标签覆盖文件）是 `VillagerTrade`：
  字段 `wants` / `additionalWants`（`TradeCost`）、`gives`（`ItemStackTemplate`）、
  `max_uses`、`xp`、`reputation_discount`（→ `priceMultiplier`）、
  `merchant_predicate`（`LootItemCondition`，如 `villager/variant` 限制品种）、
  `given_item_modifier`（`LootItemFunction`，如 `enchant_with_levels`）、
  `double_trade_price_enchantments`。`VillagerTrade.getOffer(LootContext)`
  把这些求值成 `MerchantOffer`。`random_sequence` 走
  `mc-bamboo-loot::RandomSequences`（Bamboo 已实现，见 `Region.loot_sequences`）。
- `VillagerTrades` 类在 26.3 是 `bootstrap(BootstrapContext<VillagerTrade>)` 的
  **数据生成器**：只有常量 `ResourceKey` 和 `register(...)` / `villagerTypeHolderSet`
  / `villagerTypeRestriction` / `anyValidMap` / `discardItemIfItsNot`，
  运行时不读它的 `ItemListing` 数组。`TradeRebalanceVillagerTrades` 是数据包版本。
  `TradeSets` 是 68 个 `ResourceKey<TradeSet>` 的静态键注册表（3 个流浪商人 + 13 职业 × 5 级；`javap -p` 另有 `register(...)` / `resourceKey(...)` 方法的返回类型也含 `ResourceKey`，数原始字段声明即可得 68）。
- `AbstractVillager.getOffers()` 在 `offers == null` 时 `updateTrades(level)`
  （懒生成）；`addAdditionalSaveData` 写 `Offers`（`MerchantOffers.CODEC`）和
  `Inventory`；`Villager.addAdditionalSaveData` 另写 `VillagerData`、
  `VillagerDataFinalized`、`FoodLevel`、`Gossips`、`Xp`、`LastRestock`、
  `LastGossipDecay`、`RestocksToday`（8 个键，顺序固定）。

**`MerchantOffer`。** 字段 / CODEC 键：
`buy`(ItemCost A) / `buyB`(Optional<ItemCost>) / `sell`(ItemStack) /
`uses`(默认 0) / `maxUses`(默认 4) / `rewardExp`(默认 true) /
`specialPrice`(默认 0) / `demand`(默认 0) / `priceMultiplier`(默认 0) /
`xp`(默认 1)。

- `getModifiedCostCount(cost)` =
  `Mth.clamp(cost.count + Math.max(0, Mth.floor(cost.count * demand * priceMultiplier))
  + specialPriceDiff, 1, cost.itemStack().getMaxStackSize())`。
  `getCostA()` / `getCostB()` 用这个数 `copyWithCount`。
- `updateDemand()`：`demand += uses - (maxUses - uses)`（即 `demand += 2*uses - maxUses`）。
- `isOutOfStock() = uses >= maxUses`；`needsRestock() = uses > 0`；
  `increaseUses()` / `resetUses()` / `setToOutOfStock()`。
- `satisfiedBy(a,b)` / `take(a,b)` 做两槽支付，`assemble()` 复制结果堆。

**补货。** `Villager.restock()` = `updateDemand()` → 每个 offer `resetUses()` →
有交易玩家时 `updateSpecialPrices(player)` → `lastRestockGameTime = now` →
`numberOfRestocksToday++`。`needsToRestock()` = 任一 offer `needsRestock()`。
`allowedToRestock()` = `restocksToday == 0` 或（`restocksToday < 2` 且
`now > lastRestock + 2400`）。`shouldRestock(ServerLevel)`：

```
longOverdue = now > lastRestockGameTime + 12000
dayNumber   = Timelines.OVERWORLD_DAY.getPeriodCount(level.clockManager())
isNewDay    = (lastRestockCheckDay > 0) && (dayNumber > lastRestockCheckDay)
newCycle    = longOverdue || isNewDay
lastRestockCheckDay = dayNumber                      // 每刻都写
if (newCycle) { lastRestockGameTime = now; resetNumberOfRestocks(); }
return allowedToRestock() && needsToRestock()
```

`resetNumberOfRestocks()` = `catchUpDemand()`（`2 - restocksToday` 次
`resetUses + updateDemand`，再置 0）+ `restocksToday = 0`。
`WorkAtPoi.start` 是唯一的调用点。

**需求与价格 / 折扣。** `Villager.updateSpecialPrices(Player)` 先
`resetSpecialPrices()`，再：

- 声望折扣：`reputation = getPlayerReputation(player) =
  gossips.getReputation(player.getUUID(), t -> true)`（字节码
  `lambda$getPlayerReputation$0` 恒返回 1）；每条 offer
  `addToSpecialPriceDiff(-Mth.floor(reputation * offer.getPriceMultiplier()))`。
- 英雄效果：`MobEffects.HERO_OF_THE_VILLAGE` 存在时
  `factor = 0.3 + 0.0625 * amplifier`（`double`），每条 offer
  `addToSpecialPriceDiff(-Math.max(1, (int)Math.floor(factor * baseCostA.count)))`。
- 结束若正在交易且 offers 非空，给 `MerchantMenu` 发一次 `setOffers` 触发重算。
  `startTrading(player)` = `updateSpecialPrices` + `setTradingPlayer` +
  `openTradingScreen(player, getDisplayName(), villagerData.level())`。
  `setTradingPlayer(null)` 时 `stopTrading()`。
  治疗僵尸村民的声望 + 交易声望由 `onReputationEventFrom` 写入 `GossipContainer`。

**包与菜单。**

- `ClientboundMerchantOffersPacket`（clientbound id 53，
  `mc-bamboo-data/src/generated/packets.rs:121` 已有 `MERCHANT_OFFERS = 53`）：
  `writeContainerId(int)` + `MerchantOffers.STREAM_CODEC` +
  `villagerLevel(varint)` + `villagerXp(varint)` + `showProgress(bool)` +
  `canRestock(bool)`；构造时 `offers.copy()`。
  `MerchantOffer.STREAM_CODEC` 顺序：`ItemCost A` → `ItemStack result` →
  `Optional<ItemCost> B` → `isOutOfStock(bool)` → `uses(int)` → `maxUses(int)` →
  `xp(int)` → `specialPriceDiff(int)` → `priceMultiplier(float)` → `demand(int)`
  （`writeToStream` 前 80 字节已核对到 `specialPriceDiff`，余下按 `createFromStream` 对称）。
- `ServerboundSelectTradePacket`（serverbound id 52；`packets.rs:266` 已有
  `SELECT_TRADE = 52`）：只有一个 `varint item`。`PlayRequest::decode`
  （`mc-bamboo-protocol/src/packet/play/request.rs`）**目前没有这个分支**，
  会落进 `Other(52)` 被游戏层忽略。
- `OpenScreen`（`protocol/packet/play/menu.rs`）已实现；`mc-bamboo-data`
  的 `menu` 注册表里 `MERCHANT = 19`（`generated/registries.rs:987`）。
- `MerchantMenu`（`net.minecraft.world.inventory.MerchantMenu`）：3 个槽
  `PAYMENT1 / PAYMENT2 / RESULT` + 玩家 36 格；`slotsChanged` →
  `updateSellItem()`；`setSelectionHint(int)` 对应 `SELECT_TRADE`；
  `tryMoveItems(int)`、`moveFromInventoryToPaymentSlot`、`quickMoveStack`、
  `setOffers / setXp / setMerchantLevel / setCanRestock / canRestock`、
  `stillValid(player)`（`AbstractVillager.stillValid` 要求交易玩家相同、
  村民活着、4 格交互距离内）。Bamboo `game/menu.rs::Menu` 是「箱子 + 工作站」
  的通用结构（`Target` 只认 `BlockPos` / 双箱 / 末影箱），**需要新增一个
  `Target::Entity(EntityUid)` 或独立的 merchant 分支**。

### 1.5 八卦（`GossipContainer`）与声望

- `GossipContainer`：`Map<UUID, EntityGossips>`；`EntityGossips` 是
  `Object2IntMap<GossipType>`。CODEC = `List<GossipEntry>`，
  每条 `{"Target": UUID, "Type": "major_negative", "Value": int}`。
- `GossipType`（`id, weight, max, decayPerDay, decayPerTransfer`）：
  | 类型 | weight | max | 日衰减 | 转移衰减 |
  |---|---:|---:|---:|---:|
  | `major_negative` | -5 | 100 | 10 | 10 |
  | `minor_negative` | -1 | 200 | 20 | 20 |
  | `minor_positive` | 1 | 25 | 1 | 5 |
  | `major_positive` | 5 | 20 | 0 | 20 |
  | `trading` | 1 | 25 | 2 | 20 |
- `add(uuid, type, value)` 用 `mergeValuesForAddition`：
  `sum = old + value`，`sum > type.max` 时取 `max(type.max, old)`；
  `EntityGossips.makeSureValueIsntTooLowOrTooHigh` 把 `> max` 夹到 `max`、
  `< 2`（注意是绝对值判据前的 `< 2`，字节码 `iconst_2`）删掉；整张表空了就删 UUID。
  `getReputation(uuid, predicate)` = `EntityGossips.weightedValue(predicate)` =
  `Σ(value * type.weight)`；村民用的是 `type -> true`（`getPlayerReputation`）。
  `decay()` 每类减 `decayPerDay`，`< 2` 删；`Villager.maybeDecayGossip()`
  每 24000 刻（`lastGossipDecayTime`）调一次。
  `transferFrom(other, random, max)`：按 `|weightedValue|` 做前缀和加权抽最多
  `max` 条，每条减 `decayPerTransfer` 再 `mergeValuesForTransfer = Math.max`。
  `Villager.gossip(level, other, time)` 要双方距上次八卦都 ≥1200 刻，
  `transferFrom(other.gossips, random, 10)`，然后 `spawnGolemIfNeeded(level,time,5)`；
  有转移且正在交易时 `updateSpecialPrices`。
- 声望事件 `ReputationEventType` → `Villager.onReputationEventFrom`：
  `ZOMBIE_VILLAGER_CURED` = `MAJOR_POSITIVE +20` 和 `MINOR_POSITIVE +25`；
  `TRADE` = `TRADING +2`；`VILLAGER_HURT` = `MINOR_NEGATIVE +25`；
  `VILLAGER_KILLED` = `MAJOR_NEGATIVE +25`。
  `ServerLevel.onReputationEvent(type, entity, handler)` 直接回调
  `handler.onReputationEventFrom(...)`；`Villager.die` 里
  `tellWitnessesThatIWasMurdered(killer)` 对附近目击者广播 `VILLAGER_KILLED`，
  `setLastHurtByMob` 广播 `VILLAGER_HURT`。
- **对铁傀儡生成的影响**：间接。铁傀儡走 `LAST_SLEPT` / `GOLEM_DETECTED_RECENTLY`
  / 恐慌这几条（下节），八卦不直接参与；但**八卦是价格折扣的唯一输入**，
  且 `gossip()` 是 `spawnGolemIfNeeded(...,5)` 的调用点之一。
  另外 `Villager.gossip` 末尾在有交易玩家且转移量 > 0 时重算折扣，
  所以声望传播会立刻改价格。
- `GossipContainer.getCountForType(type, predicate)` / `remove(type)` 供
  数据修复用；Bamboo 首版不必实现 `remove(uuid,type)` 之外的全部重载。

### 1.6 繁殖

- `Villager.canBreed()` = `foodLevel + countFoodPointsInInventory() >= 12`
  （`BREEDING_FOOD_THRESHOLD = 12`）且 `!isSleeping()` 且 `getAge() == 0`。
  `hungry()` = `foodLevel < 12`；`eatUntilFull()` 遍历 8 格 `SimpleContainer`，
  读 `DataComponents.VILLAGER_FOOD`（`VillagerFood{nutrition}`）逐个吃；
  `eatAndDigestFood()` = `eatUntilFull()` + `digestFood(12)`。
  `wantsToPickUp(level, stack)`：`#villager_picks_up` 标签、`VILLAGER_FOOD` 组件、
  或职业 `requestedItems`；`hasExcessFood()` = 库存 ≥24；
  `wantsMoreFood()` = 库存 <12；`hasFarmSeeds()` = `#villager_plantable_seeds`。
- `VillagerMakeLove`（`Behavior`，入口条件 `BREED_TARGET = VALUE_PRESENT` 和
  `NEAREST_VISIBLE_LIVING_ENTITIES = VALUE_PRESENT`，时长 350）：
  `isBreedingPossible` 要 `BehaviorUtils.targetIsValid(brain, BREED_TARGET, VILLAGER)`
  且双方 `canBreed()`；`start` 用 `BehaviorUtils.lockGazeAndWalkToEachOther(
  self, partner, 0.5f, 2)`、双方实体事件 18、`birthTimestamp = now + 275 +
  self.random.nextInt(50)`；`tick` 到点 `tryToGiveBirth`：
  `takeVacantBed(level, self)` = `PoiManager.take(is(HOME),
  (type,pos) -> canReach(self,pos,type), self.blockPosition(), 48)`（要路径可达，
  用 `PoiType.validRange()` 建路径）；没有空床 → 双方实体事件 13（愤怒粒子）；
  有床 → `breed(level, self, partner)`：父母 `setAge(6000)`、幼崽
  `setAge(-24000)`、`snapTo` 到母亲位置、`addFreshEntityWithPassengers`、
  实体事件 12（爱心）；失败则 `PoiManager.release(bedPos)` 归还床位；
  成功 `giveBedToChild(child, bedPos)` 设 `HOME = GlobalPos(dimension, bedPos)`。
- `Villager.getBreedOffspring(ServerLevel, AgeableMob)`：`d = random.nextDouble()`；
  `<0.5` 用出生地群系的 `VillagerType.byBiome`，`<0.75` 用母亲品种，
  否则用父亲品种。`finalizeSpawn(..., BREEDING, ...)` 把职业改成 `none`，
  再 `finalizeVillagerType`（按出生地群系）。

### 1.7 村庄相关生成

**主世界自定义刷怪器顺序**（`MinecraftServer.createLevels` 的
`ImmutableList.of(...)`，字节码常量段 39200–40700）：
`PhantomSpawner` → `PatrolSpawner` → **`CatSpawner`** → `VillageSiege` →
**`WanderingTraderSpawner(SavedDataStorage)`**。`ServerLevel.tickCustomSpawners(boolean)`
按这个顺序逐个 `CustomSpawner.tick(level, spawnEnemies)`，在 `ServerChunkCache.tickChunks`
的自然刷怪和逐区块随机刻之后。Bamboo 现在只接了前两个
（`game/world.rs:3447-3448` 的 `phantom_spawn::tick` / `patrol_spawn::tick`），
注释里已写明「之后的猫、村庄围攻、流浪商人还没有」。

**铁傀儡召唤（村民侧）。** `VillagerPanicTrigger.tick` 每
`gameTime % 100 == 0` 调 `spawnGolemIfNeeded(level, time, 3)`；
`Villager.gossip` 末尾调 `spawnGolemIfNeeded(level, time, 5)`。
`wantsToSpawnGolem(time)` = `golemSpawnConditionsMet(time)`（
`LAST_SLEPT` 记忆存在且 `time - lastSlept < 24000`，
`TIME_SINCE_SLEEPING_FOR_GOLEM_SPAWNING = 24000`）且
`!brain.hasMemoryValue(GOLEM_DETECTED_RECENTLY)`。
`spawnGolemIfNeeded`：AABB 外扩 10 格取 `Villager` 列表，筛出同样
`wantsToSpawnGolem(time)` 的，`limit(5)` 后 `size() >= requiredCount` 才
`SpawnUtil.trySpawnMob(EntityTypes.IRON_GOLEM, EntitySpawnReason.MOB_SUMMONED,
level, self.blockPosition(), 10, 8, 6, SpawnUtil.Strategy.LEGACY_IRON_GOLEM, false)`；
成功后对**未过滤的整张列表**清 `GOLEM_DETECTED_RECENTLY`（`GolemSensor` 下一轮重设）。
`GolemSensor.checkForNearbyGolem` 扫 `MOBS` 里有没有 `IRON_GOLEM`，
命中 `setMemoryWithExpiry(GOLEM_DETECTED_RECENTLY, true, 599)`。
**结论：铁傀儡生成要 POI（床 → LAST_SLEPT）+ 大脑（PANIC / 八卦 / GOLEM 传感器）
+ `IronGolem` 实体 + `SpawnUtil` 的 10×8×6 落点搜索，不需要袭击系统。**

**`CatSpawner`。** `TICK_DELAY = 1200`，构造时 `nextTick = 0`；每刻 -1，
到 0 重置 1200 并取 `ServerLevel.getRandomPlayer()`（没有就返回）。
落点 `player.blockPosition().offset(±(8+random.nextInt(24)), 0,
±(8+random.nextInt(24)))`（两次 `nextBoolean` 定正负），`hasChunksAt(x-10..x+10,
z-10..z+10)`，`SpawnPlacements.isSpawnPositionOk(CAT, level, pos)`：
- `level.isCloseToVillage(pos, 2)` → `spawnInVillage`：48 格内
  `getCountInRange(is(HOME), pos, 48, IS_OCCUPIED) > 4`
  且 48×8×48 盒内 `Cat` 数 < 5 才 `spawnCat(pos, level, false)`。
- 否则 `StructureManager.getStructureWithPieceAt(pos, StructureTags.CATS_SPAWN_IN)`
  有效 → `spawnInHut`：16×8×16 盒内没有猫才 `spawnCat(pos, level, true)`
  （`true` = `setPersistenceRequired()`）。
- `spawnCat`：`CAT.create(level, NATURAL)` → `finalizeSpawn(level,
  getCurrentDifficultyAt(pos), NATURAL, null)` → 可选持久化 →
  `snapTo(pos, 0, 0)` → `addFreshEntityWithPassengers`。
  **注意 `spawnInHut` 传的是 `true`（持久化），而 `spawnInVillage` 传 `false`；
  字节码里 `spawnCat(..., boolean persist)` 的第三参在两处分别是 `iconst_0` /
  `iconst_1`。**

**`WanderingTraderSpawner`。** 常量：
`DEFAULT_TICK_DELAY = 1200`、`DEFAULT_SPAWN_DELAY = 24000`、
`MIN_SPAWN_CHANCE = 25`、`MAX_SPAWN_CHANCE = 75`、
`SPAWN_CHANCE_INCREASE = 25`、`SPAWN_ONE_IN_X_CHANCE = 10`、
`NUMBER_OF_SPAWN_ATTEMPTS = 10`。`tick` 先查
`GameRules.SPAWN_WANDERING_TRADERS`（假直接返回）；每 1200 刻
`getTraderData()`（`SavedDataStorage.computeIfAbsent(WanderingTraderData.TYPE)`），
`spawnDelay -= 1200`，>0 返回；否则 `spawnDelay = 24000`、
`spawnChance = clamp(spawnChance + 25, 25, 75)`、
`if (random.nextInt(100) > spawnChance) return`（严格大于才跳过）、
`spawn(level)` 成功则 `spawnChance = 25`。
`spawn`：`getRandomPlayer()` 没有返回 true；`random.nextInt(10) != 0` 返回 false；
48 格内 `PoiManager.find(is(MEETING), pos->true, player.pos, 48, ANY)`
（没有则用玩家位置）；`findSpawnPositionNear(level, target, 48)` 用
`WANDERING_TRADER` 的 `SpawnPlacements` / `WORLD_SURFACE` 高度图试 10 次；
`hasEnoughSpace`（1×2×1 全空碰撞形状）；群系不能在
`#without_wandering_trader_spawns`；`WANDERING_TRADER.spawn(level, pos, EVENT)`；
两只 `TRADER_LLAMA` 各 `setLeashedTo(trader, true)`；
`trader.setDespawnDelay(48000)`、`setWanderTarget(target)`、
`setHomeTo(target, 16)`。
`WanderingTraderData` 存 `data/minecraft/wandering_trader.dat`，
CODEC 键 `spawn_delay`（默认 24000）/ `spawn_chance`（默认 25）。
`WanderingTrader` 自己的 `WanderToPositionGoal` / 48000 刻消失 / 交易表
（`trade_set/wandering_trader/{buying,common,uncommon}`，各 `amount` 见 json）
是另一个实体，建议单独放在期 7 的后半段。

### 1.8 转化

- **村民 → 僵尸村民**：`Zombie.hurtServer` / `doHurtTarget` 的
  `killedEntity` 分支（`Zombie.java` 字节码 54174 起）：难度 HARD 必转，
  非 HARD 时 `random.nextBoolean()` 也要过；`convertVillagerToZombieVillager(
  ServerLevel, Villager)` 用 `ConversionParams.single(mob, true, true)` +
  `Villager.convertTo(ZOMBIE_VILLAGER, ...)`。Bamboo `mob/conversion.rs`
  已有 `ConversionTracker` + `Mob.convertTo` 的 `SINGLE` 分支，可复用。
- **僵尸村民生成**：`ZombieVillager.finalizeSpawn` →
  `initializeZombieVillagerData(random)`（随机 `VillagerType`、随机职业），
  以及自然生成的 5% 分支（`Zombie` 的 `ZombieGroupData`，见
  `mob/zombie_spawn.rs` 的现状）。
- **治疗**：`ZombieVillager.mobInteract`：主手是 `GOLDEN_APPLE` 且有
  `MobEffects.WEAKNESS` → `consume(1, player)` →
  `startConverting(player.getUUID(), 3600 + random.nextInt(2401))`。
  `startConverting` 记 `conversionStarter` / `villagerConversionTime`、
  设 `DATA_CONVERTING_ID = true`、移除 WEAKNESS、加 STRENGTH
  （时长 = 转化时长、等级 = `min(difficulty.id - 1, 0)`）、发实体事件 16。
  `tick`：每刻 `villagerConversionTime -= getConversionProgress()`，<=0 就
  `finishConversion`。`getConversionProgress` 默认 1；`random.nextFloat() < 0.01`
  时扫 `x±4,y±4,z±4`，最多 14 个
  `#speeds_up_zombie_villager_curing`（铁栅栏 / 床）方块，每格加速
  （`MAX_SPECIAL_BLOCKS_COUNT = 14`、`SPECIAL_BLOCK_RADIUS = 4`）。
  `finishConversion` → `convertTo(VILLAGER, CONVERSION)`；`AfterConversion`
  把装备搬到村民 8 格 `Inventory`（`getSlot(300 + slotIndex)`）、
  `setVillagerDataFinalized` / `setVillagerData`（职业、等级、品种）、
  `setGossips` / `setOffers(tradeOffers.copy())` / `setVillagerXp`、
  `finalizeSpawn(level, difficulty, CONVERSION, null)`、`refreshBrain(level)`、
  给治疗者发 `CuredZombieVillagerTrigger` 和
  `ReputationEventType.ZOMBIE_VILLAGER_CURED`、给新村民 200 刻 NAUSEA。
  存档键 `VillagerData` / `VillagerDataFinalized` / `Offers` / `Gossips` /
  `ConversionTime` / `ConversionPlayer` / `Xp`。
  `removeWhenFarAway` 在转化中或 `villagerXp != 0` 时为假（不消失）。
- **村民 → 女巫**：`Villager.thunderHit(ServerLevel, LightningBolt)` 在
  非 PEACEFUL 时 `convertTo(WITCH, ConversionParams.single(mob,false,false),
  after)`，`AfterConversion` 里 `finalizeSpawn(..., CONVERSION, null)` +
  清理；失败时 `super.thunderHit(...)`。Bamboo 有 `LightningBolt` 实体类型和
  雷击随机刻（`interaction::run_random_ticks`），但村民被劈的分支要新接。
- 与八卦的耦合：治疗完成 → `ServerLevel.onReputationEvent` →
  附近村民 `MAJOR_POSITIVE+20`、`MINOR_POSITIVE+25` → 价格折扣（§1.5）。

### 1.9 结构：村庄里生成的村民

- `StructureTemplate` 的 `entityInfoList: List<StructureEntityInfo{pos,
  blockPos, nbt}>` 来自 `.nbt` 的 `entities` 列表；
  真实模板 `minecraft:village/plains/villagers/unemployed` 带 1 个村民，
  `data/minecraft/structure/village/common/iron_golem.nbt` 带铁傀儡，
  猫模板在 `village/common/animals/cat_*.nbt`。
- `StructureTemplate.placeInWorld` 的 `placeEntities(ServerLevelAccessor,
  BlockPos, Mirror, Rotation, BlockPos pivot, BoundingBox, boolean
  finalizeEntities, ProblemReporter)`：
  逐实体 `blockPos = transform(entityInfo.blockPos, mirror, rotation, pivot)
  .offset(origin)`，落在 `boundingBox` 外的跳过；`tag = nbt.copy()`，
  `Pos` 覆盖成变换后 `Vec3` 的 `[3]double`；`remove("UUID")`；
  有 `block_pos` 时按 `BlockPos.CODEC` 改成三个 int；
  `createEntityIgnoreException`（`EntityType.loadEntityRecursive`）→
  `entity.rotate/mirror` 算 `yRot` → `addFreshEntityWithPassengers`；
  `finalizeEntities` 控制 `Mob.finalizeSpawn`（`SinglePoolElement.getSettings`
  为 false，所以村庄件会 `finalizeSpawn`，再按群系定 `VillagerType`）。
- **Bamboo 现状**：`worldgen/structure/jigsaw/place.rs` 的
  `place_entities()` 已经逐字照官方做了几何 + NBT 改写，产出
  `PlacedEntity{block_pos, x,y,z, y_rot, x_rot, raw_y_rot, tag}`，
  `place_piece()` 把它填进 `PlacedBlocks.entities`；单测
  `place_entities_matches_the_vanilla_transform` 用真实村民模板验证。
  **但是 `PlacedBlocks.entities` 没有任何下游消费方**：
  `worldgen/structure/place_step.rs::layout_place` 调 `place_piece` 后丢弃返回值；
  `game/worldgen.rs::GeneratedChunk` 只有 `block_entities:
  Vec<GeneratedBlockEntity>`，没有 `entities`；`game/region/column.rs`
  的 `ChunkEntities` 只在拆合 / 存盘里用。
  `docs/worldgen-structures-e2e.md:212` 明确写「生成期实体（村民、悦灵、
  守卫者、女巫 / 猫）不进假世界；只保证 `nextLong` / 方块写入序列一致，
  不比较实体本身」。
- 因此这一期的重点是**打通产出 → `GeneratedChunk.entities` → `Region` 实体表**
  的管线，并把 `PlacedEntity.tag` 交给 `mob/nbt.rs::read_mob` 建行
  （需要补 `VillagerData` / `Offers` / `Gossips` 等键），再跑 `finalizeSpawn`。

---

## 2. 依赖关系图

```
                       ┌──────────────────────────┐
                       │ 期 1  POI (PoiStore)      │
                       │ PoiTypes / 段索引 / 存档   │
                       │ 方块 set_block 登记注销    │
                       └────────────┬─────────────┘
                                    │
        ┌───────────────┬──────────┼───────────────┬──────────────────┐
        ▼               ▼          ▼               ▼                  ▼
   isCloseToVillage  CatSpawner  WTSP(find    床 / 工作站 / 钟 /   DistanceTracker
   / 村庄判定         (home POI)  meeting POI)  避雷针 (POI 查询)   (村庄距离)
        │               │          │               │
        │               │          │               ▼
        │               │          │       ┌───────────────────┐
        │               │          │       │ 期 3 大脑 / 日程    │
        │               │          │       │ GlobalPos 记忆、     │
        │               │          │       │ 9 传感器、活动表、   │
        │               │          │       │ timeline            │
        │               │          │       └─────────┬─────────┘
        │               │          │                 │
        │               │          │        ┌────────▼─────────┐
        │               │          │        │ 期 4 职业工作站     │
        │               │          │        │ AcquirePoi /       │
        │               │          │        │ Assign / Reset /   │
        │               │          │        │ 等级经验            │
        │               │          │        │ WorkAtPoi           │
        │               │          │        └────────┬──────────┘
        │               │          │                 │
        │               │          │        ┌────────▼──────────────┐
        │               │          │        │ 期 2 数据导出 + 骨架    │
        │               │          │        │ trade_set / villager_  │
        │               │          │        │ trade / tags → data    │
        │               │          │        │ MobType::Villager、     │
        │               │          │        │ VillagerData 同步/存档  │
        │               │          │        └────────┬──────────────┘
        │               │          │                 │
        │               │          │        ┌────────▼──────────┐
        │               │          │        │ 期 5 交易           │
        │               │          │        │ MerchantOffer、     │
        │               │          │        │ 菜单/包、补货、     │
        │               │          │        │ 需求、折扣          │
        │               │          │        └────────┬──────────┘
        │               │          │                 │
        │               │          │        ┌────────▼──────────┐
        │               │          │        │ 期 6 八卦 / 繁殖 /  │
        │               │          │        │ 铁傀儡              │
        │               │          │        │ GossipContainer、   │
        │               │          │        │ 声望→折扣、         │
        │               │          │        │ VillagerMakeLove、  │
        │               │          │        │ GolemSensor         │
        │               │          │        └────────┬──────────┘
        │               │          │                 │
        └───────────────┴──────────┴────────┬────────┴──────────────┐
                                           ▼                       ▼
                                ┌──────────────────┐   ┌─────────────────────┐
                                │ 期 7 刷怪器/转化  │   │ 期 8 结构实体       │
                                │ CatSpawner、WTSP、 │   │ PlacedEntity →      │
                                │ ZombieVillager、  │   │ GeneratedChunk →    │
                                │ 雷击变女巫        │   │ EntityStore         │
                                └──────────────────┘   └─────────────────────┘
```

**文字版的硬依赖**（每条都对应字节码里的调用）：

1. **POI → 一切位置查询**：`AcquirePoi`、`VillagerMakeLove.takeVacantBed`、
   `CatSpawner.spawnInVillage`、`WanderingTraderSpawner.spawn`、
   `ServerLevel.isCloseToVillage`、`findLightningRod` 都直接 `getPoiManager()`。
2. **POI → 工作站 → 职业**：`AssignProfessionFromJobSite` 读
   `PoiManager.getType(potentialJobSite.pos())` 才定职业；没有 POI 就没有职业。
3. **职业 → 交易表**：`Villager.updateTrades` 读
   `profession.getTrades(level)`，失业 / 换职业会 `offers = null`。
4. **交易 → 补货**：`WorkAtPoi.start` 里 `shouldRestock` → `restock`，
   而 `restock` 操作的是 `MerchantOffers` 的 `demand/uses/specialPrice`；
   没有 offer 结构就无从补货。
5. **八卦 → 折扣**：`updateSpecialPrices` 的 `getPlayerReputation` 读
   `GossipContainer`；`gossip()` 传播后立刻重算折扣。
6. **大脑 → 恐慌 / 聚会 → 铁傀儡**：`VillagerPanicTrigger.tick`（每 100 刻）
   和 `Villager.gossip` 是 `spawnGolemIfNeeded` 的两个入口；
   `LAST_SLEPT` 由 `SleepInBed`（REST 活动）写，`GOLEM_DETECTED_RECENTLY`
   由 `GolemSensor`（200 刻）写。
7. **繁殖 → POI 票 + 幼年实体**：空床来自 `PoiManager.take`，幼崽
   `getBreedOffspring` / `finalizeSpawn(BREEDING)` 要 `VillagerType.byBiome`。
8. **转化 → 村民 / 交易 / 八卦**：`ZombieVillager.finishConversion` 把
   `VillagerData / Offers / Gossips / Xp` 搬给新村民并 `refreshBrain`；
   治疗成功广播 `ZOMBIE_VILLAGER_CURED` 改声望。
9. **结构 → 实体骨架 + 存档**：`PlacedEntity.tag` 要能被
   `mob/nbt.rs::read_mob` 完整还原（含 `VillagerData` / `Offers` / `Brain`）。

**可以并行 / 提前做的**：数据导出（期 2 的前半）、`MerchantOffers` 的
纯数据结构与编码、包与 NBT 的编解码单测、`Activity` 变体与
`java_hash` 单测、POI 存档编解码单测，都不依赖运行时 POI 查询。

---

## 3. 建议的分期（每期都能单独对拍）

> 每期的「可交付」都是一条能单独跑 oracle 的纵向切片；不追求一期把村民「做完」。
> 期 1/2 的顺序可调，见 §7 决定 1。

### 期 1：POI 存储与方块登记（不含任何村民实体）

- `PoiStore`：`SectionPos → PoiSection`，`Short2ObjectMap` 风格的段内表 +
  按类型索引；`PoiType` 21 种 + `forState`；`add / remove / take / release /
  exists / getType / findClosest / findAllClosestFirstWithType / getCountInRange`。
- `poi/r.X.Z.mca` 编解码（`Sections` / `Valid` / `Records` / `pos` /
  `type` / `free_tickets` / `DataVersion`），接 `world_dir::poi_dir()`、
  `SaveSession`、`RegionColumn` / `IslandPart::Poi` 的拆合。
- `ChunkStore::set_block` 产 `PoiDelta`，`Region` 同刻应用；
  `checkConsistencyWithBlocks` 在区块加载时重建无效段。
- `DistanceTracker` 先用「按 `meeting` 段做 BFS 到 6」的简化版，
  `isCloseToVillage(pos, sections)` 可被查询。
- **不引入村民、大脑、交易**。

### 期 2：数据导出 + 村民实体骨架

- 导出到 `mc-bamboo-data`：`trade_set` 68 个、`villager_trade` 391 个、
  `tags/villager_trade` 73 个（展开成按等级的静态条目表）、`PoiTypes` 的 `matchingStates`、
  `VillagerProfession` 的 `heldJobSite / acquirableJobSite / requestedItems /
  secondaryPoi / tradeSetsByLevel`、`VillagerType.byBiome`、`GossipType` 常量、
  `timeline/villager_schedule.json` 两条关键帧。
- `MobType::Villager`（+ `ZombieVillager` / `WanderingTrader` / `IronGolem` 的
  占位枚举即可），`VillagerData` 字段、`createAttributes`（移速 0.5）、
  碰撞箱（`EntityTypes.VILLAGER` = `MobCategory.MISC`、`sized(0.6, 1.95)`、
  `eyeHeight(1.62)`、`clientTrackingRange(10)`；幼年 `0.49×0.98`、眼高 0.63）、
  `DATA_VILLAGER_DATA` / `DATA_VILLAGER_DATA_FINALIZED` /
  `DATA_UNHAPPY_COUNTER` 三项元数据（`EntityData` 新增变体 +
  `entity_tracking::default_data_for_type` + 序列化器 ID 核对）。
- 存档 8 个键 + `Offers` / `Inventory` / `Brain`（复用 `brain/nbt.rs`）。
- `/summon minecraft:villager` 和刷怪蛋；`removeWhenFarAway = false`；
  `finalizeSpawn` 的群系品种。**村民此时没有大脑、没有职业、没有交易。**

**期 2 实际做法与偏差（2026-11-23，分支 `villager-2`）**

- 数据导出按计划完成，但 `PoiTypes.matchingStates` 这一项按任务书跳过（期 1 的
  `poi` 分支已经导出，本期不与它重复）。
- 生成器新增 `villager` 输出：`TRADE_SETS` / `VILLAGER_TRADES` /
  `VILLAGER_TRADE_TAGS` 保留官方原文，`TRADE_SET_ENTRIES` 把标签按官方顺序递归展开
  （去重、循环引回报错），另导出 15 个 `Profession`（工作站用「对每个 POI 类型求值」
  展开成 POI 名字表）、7 个品种、29 条 `VillagerType.byBiome`、5 个 `GossipType`、
  3 个声望常量、三个元数据 index / 序列化器、尺寸 / 眼高 / 移速 / 等级阈值和两条日程
  关键帧。生成时校验数量（68 / 391 / 73 / 15 / 7 / 29），每级交易表存在，展开条目都能
  在 `villager_trade` 里找到；`mc-bamboo-data` 另用单测钉住数量、阈值和关键帧。
- `MobType::Villager` 接进 `ALL`、注册表 ID、名字、尺寸、音效、刷怪蛋、翻译键、
  战利品表、`MobCategory::Misc`、`removeWhenFarAway = false`、`is_animal = false`
  （不是 `Animal`，不写 `InLove`），构造时能开门 / 浮水 / 寻路 48 格 / 捡物。
- 元数据新增 `EntityData::VillagerUnhappy`(18) / `VillagerData`(19) /
  `VillagerDataFinalized`(20)，序列化器 ID 由生成器从官方代码导出；游戏层只在非
  默认时发，回到默认时发默认值。
- 存档写官方 8 个键，`Offers` / `Inventory` 作为原文 `Tag` 原样保留（期 2 不解释），
  `Brain` 继续走通用 `brain/nbt.rs`。读档时有 `VillagerData` 或
  `VillagerDataFinalized` 为真就把 finalized 置真，等级至少 1。
- 死亡只掉空的 `entities/villager` 表，不掉经验（`xpReward` 为 0，不是 `Animal` 的
  `1 + nextInt(3)`）。
- 对拍新增 `mob-oracle --only villager`：10 个群系的生成品种 / finalized / 尺寸 / 血量、
  1 题存档往返（8 个键 + `Offers` / `Inventory` / `Brain` 逐键比）、2 题死亡；
  `--break-self-test villager_spawn` 和 `villager_save` 两处改坏都能报出不一致。
- **偏差**：期 2 没有大脑，所以构造期不取 9 个传感器的起始延迟；`finalizeSpawn` 里
  `Mob.finalizeSpawn` 的跟随距离加成和左撇子因此和官方随机源不同。期 3 接大脑后
  会自动对齐，生成场景只比 `VillagerData` / finalized / 尺寸 / 血量，不比随机源。
  僵尸村民和流浪商人没有放占位枚举：它们会让 `MobType::ALL` 和自然刷怪映射多出
  两种没有实现的类型，收益很小，留到期 7。

### 期 3：大脑与日程

- `Activity` 加 8 个变体 + `java_hash` 单测；`MemoryType` 追加约 20 种 +
  `MemoryValue::GlobalPos`（含 `Brain.Packed` 里有 codec 的那些）。
- `SensorKind` 加 5 种，`requires()` / `scan_rate()` / `class_name()` 照官方；
  `VillagerHostilesSensor` 的 `ACCEPTABLE_DISTANCE_FROM_HOSTILES` 表。
- `Brain` 加 `schedule: Option<ActivitySchedule>`、
  `update_activity_from_schedule(attrs, game_time, pos)`（20 刻节流）；
  `GlobalView` 加 `villager_activity / baby_villager_activity`（由主控按
  `Global.clock` 和 `villager_schedule.json` 关键帧算，最多旧一刻，
  与 `sky_darken` 同路径）。
- `VillagerGoalPackages` 的 11 个包**先挂完整结构表**，行为分两档：
  真实现 `Swim / LookAtTargetSink / MoveToTargetSink / DoNothing /
  RunOne / GateBehavior / UpdateActivityFromSchedule / VillagerCalmDown /
  SetWalkTargetAwayFrom / VillageBoundRandomStroll / VillagerPanicTrigger`；
  其余先 `Unavailable` 占位（保住优先级、活动、类名、进入条件）。
- `refreshBrain` / `pack → new`；`customServerAiStep` 跑 `Brain.tick`。
- **PRE_RAID / RAID / HIDE 三个活动表照挂、行为占位**（Bamboo 没有袭击系统）。

### 期 4：职业与工作站

- 真实现 `AcquirePoi`（含 `JitteredLinearRetry`）、`ValidateNearbyPoi`、
  `GoToPotentialJobSite`、`YieldJobSite`、`AssignProfessionFromJobSite`、
  `ResetProfession`、`PoiCompetitorScan`、`SecondaryPoiSensor`、
  `GoToWantedItem` / `wantsToPickUp` / `InventoryCarrier` 的拾取。
- `VillagerProfession` 运行时表 + `VillagerData.withProfession/withLevel`、
  `setVillagerData` 清 `offers`、`refreshBrain`。
- `WorkAtPoi` 的 `LAST_WORKED_AT_POI` / 300 刻冷却 / 1.73 格判定 /
  `playWorkSound`；`useWorkstation` 基类空实现照旧。
- 等级阈值 + `rewardTradeXp` 的骨架（经验球可以复用现有
  `ExperienceOrb`，升级的 `updateTrades` 等期 5 接真表）。
- `releaseAllPois` / `die` 的 POI 归还。

### 期 5：交易

- `TradeCost` / `ItemCost` / `ItemStackTemplate` / `VillagerTrade` /
  `TradeSet` / `MerchantOffers` / `MerchantOffer` 的 Rust 结构 + CODEC +
  STREAM_CODEC；`LootContext` 里 `random_sequence` 接
  `Region.loot_sequences`（已有）。
- `updateTrades` / `addOffersFromTradeSet` / 不重复挑选（
  `allow_duplicates = false` 时按 key 去重）、`getOffers()` 懒生成。
- `OpenScreen(menu::MERCHANT)` + `ClientboundMerchantOffersPacket` 的 Rust 包；
  `PlayRequest::SelectTrade { slot: i32 }` 解码；`Menu` 加 merchant 分支
  （3 槽 + `setSelectionHint` + `slotsChanged` + `quickMoveStack` +
  `stillValid` 的 4 格距离）。
- `notifyTrade` / `increaseUses` / `rewardTradeXp` / 升级追加交易 /
  `shouldRestock` / `allowedToRestock` / `restock` / `catchUpDemand` /
  `updateDemand` / `updateSpecialPrices`（声望部分先 returning 0，
  期 6 接真八卦；英雄效果若 `MobEffects` 已有 `HERO_OF_THE_VILLAGE` 就直接接）。
- **可以给玩家真实完成一次交易并拿到物品**（这是第一期的用户可见里程碑）。

### 期 6：八卦、繁殖、铁傀儡

- `GossipContainer` / `GossipType` / `EntityGossips`：add / decay /
  transferFrom / getReputation / CODEC（`Target/Type/Value`）。
- `ReputationEventType` 的四种事件、`ServerLevel.onReputationEvent` 的
  半径广播、`die` 的目击者、`setLastHurtByMob`；接上期 5 的折扣。
- `VillagerFood` 组件 + 8 格 `SimpleContainer` + `canBreed` /
  `eatUntilFull` / `wantsToPickUp`；`VillagerMakeLove` 全链路 +
  `getBreedOffspring` 的三种品种来源 + `finalizeSpawn(BREEDING)`。
- `SleepInBed`（REST）/ `LAST_SLEPT` / `LAST_WOKEN`、`GolemSensor`、
  `spawnGolemIfNeeded` 的 3 / 5 人数分支、`SpawnUtil` 的 10×8×6 落点、
  `MobType::IronGolem` 的最小实体（100 血、移速 0.25，攻击 / 目标可后补）。
- `VillagerPanicTrigger` 的每 100 刻分支、`Villager.gossip` 的 1200 刻冷却。

### 期 7：刷怪器与转化

- `CatSpawner`（接进 `game/patrol_spawn.rs::tick` 的同一个
  `tick_custom_spawners`，顺序在巡逻队之后）、`VillageSiege` 占位、
  `WanderingTraderSpawner` + `WanderingTraderData` 的
  `data/minecraft/wandering_trader.dat`（`game/world_state.rs` 的
  saved-data 路径已有 `world_border.dat` / `chunk_tickets.dat` 可照抄）。
- `WanderingTrader` 实体（48000 刻消失、`setWanderTarget` / `setHomeTo`、
  三张流浪商人 trade_set）、`TRADER_LLAMA` 的拴绳（如果拴绳还没做，先占位）。
- `ZombieVillager`：生成、金苹果 + 虚弱治疗、`getConversionProgress`
  的 1% 加速扫描、`finishConversion` 搬状态、声望广播；
  `Zombie` 杀死村民的 HARD / 非 HARD 分支；`Villager.thunderHit` 变女巫。
- `MobType::ZombieVillager` 的元数据 / 存档键。

### 期 8：结构实体与收尾

- `PlacedBlocks.entities` → `GeneratedChunk.entities` →
  `Region.entities` 的管线；`mob/nbt.rs::read_mob` 认村民键；
  生成时 `finalizeSpawn`（`STRUCTURE` 原因）。
- 村庄 `iron_golem.nbt` / `cat_*.nbt` 的实体一并落地；
  `PlacedEntity.tag` 里 `Rotation` / `yRot` 的处理与
  `place_entities` 单测保持一致。
- 补齐 `docs/coverage.md` / `README.md` 的村民条目、已知限制清单；
  全量 `cargo test` / `clippy --all-targets` / `mob-oracle` 回归。

---

## 4. 每期的对拍做法

> 通用前置：`cargo run -p mc-bamboo-vanilla -- generate <jar>` 已生成
> `target/vanilla`；oracle 命令格式见 `README.md`「升级游戏版本」一节
> （`cargo run --release -p mc-bamboo-vanilla -- oracle java --only <名字>`
> / `mob-oracle java --only <前缀> --break-self-test <名字>`）。
> 每期都要有**正向 0 不一致**和**至少一处改坏自检能被抓到**。

| 期 | 对拍做法 | 关键比什么 | 改坏自检 |
|---|---|---|---|
| 1 | 新 `poi-oracle`：Java 侧在假 `ServerLevel` 里放 / 拆床、钟、13 种工作站、避雷针，调 `PoiManager.add/remove/take/release/exists/findClosest/getCountInRange/sectionsToVillage`；Bamboo 侧跑同一序列。另加 `chunk-content` 风格的 NBT 往返：官方 `SectionStorage.writeChunk` 产出的根标签 vs Bamboo 编码，逐键逐值比 `Sections/<y>/Valid/Records[]/pos/type/free_tickets/DataVersion` | 段索引、`freeTickets` 扣还、`HAS_SPACE/IS_OCCUPIED/ANY` 过滤、村庄距离 BFS、存档字节 | 故意把 `free_tickets` 初值写成 0 / 漏一个 `meeting` 段 / 拆方块时只 add 不 remove |
| 1（拆合） | 现有 `island_split` 单测加 POI 断言：拆岛后两岛各自只含自己区块的段，并岛后无重复、无丢失；`poi/` 脏块集合跟 `region/` 脏块集合一致 | 段归属、脏标记 | 拆岛时不搬 `IslandPart::Poi` |
| 2 | `chunk-content` oracle 新建村民存档：`VillagerData/level/type/profession`、`Offers`、`Gossips`、`Xp`、`LastRestock`… 与官方 `saveWithFullMetadata` 逐键比；`entity_oracle` 比 `set_entity_data` 的三个元数据字节和 `entity_type` 注册表 ID；数据导出用「jar 内 json → 生成表」的逐文件哈希比对单测 | NBT 键顺序 / 默认值、元数据序列化器 ID、导出表完整性 | 把 `level` 缺省写成 0 / 漏 `trade_set` 的一个文件 |
| 3 | 扩 `mob-oracle --only villager_brain`：Java 侧用 `MobOracle.Brain.dump`（`getActiveActivities` / `getRunningBehaviors` / `brain.forEach(Visitor)`）每刻转储；Bamboo 侧复用 `oracle/brain.rs::dump` + `water_mob.rs::brain_table` 的结构表（传感器类名 / scanRate / requires、行为优先级 / 活动 / 类名、进入条件、退出清记忆）。场景：固定 `gameTime` 跨 10 / 2000 / 9000 / 11000 / 12000 五个关键帧，成年 / 幼年各一套，另跑 24000 刻长场景比 `lastScheduleUpdate` 的 20 刻节流 | 活动切换时刻、结构表逐字一致、记忆名 / ttl、随机源 | `--break-self-test villager_sensors`（延迟各 +1）、`villager_shuffle`（洗牌种子 +1）、把 `work` 关键帧改成 2001 |
| 4 | `--only villager_job`：场景 = 空地放 1 个工作站 + 1 个失业村民，跑 200–600 刻，逐刻比 `JOB_SITE / POTENTIAL_JOB_SITE` 记忆、`VillagerData.profession / level`、POI `free_tickets`、工作站被拆后 `ResetProfession` 的时机；再跑 `rewardTradeXp` 的纯函数用例（阈值 10/70/150/250） | `AcquirePoi` 的 20 刻抖动、`AssignProfession` 的 2 格判定、清 offers、refreshBrain 后记忆保留 | 故意把 `closerToCenterThan(...,2.0)` 改成 3.0 / 不 `release` POI |
| 5 | 新 `trade-oracle`（或扩 `menu` oracle，现有 `tools/mc-bamboo-vanilla/src/menu.rs`）：Java 侧构造 `Villager` + 指定 `VillagerData`，`getOffers()` 后导出 `MerchantOffers.CODEC` 的 SNBT 和 `ClientboundMerchantOffersPacket` 的字节；Bamboo 侧同种子跑 `updateTrades`，逐条比 `buy/buyB/sell/uses/maxUses/rewardExp/specialPrice/demand/priceMultiplier/xp`。再跑补货 / 需求 / 折扣的逐题用例：给定 `demand/uses/priceMultiplier/specialPriceDiff/reputation/amplifier` 算 `getCostA().count`；最后跑 `notifyTrade → restock → shouldRestock` 的时间线（含跨游戏日） | 交易表挑选取数（`random_sequence`）、offer 字段、包字节、价格公式、补货次数 | 把 `updateDemand` 写成 `demand += uses` / 漏英雄效果的 `max(1, …)` / 包里少写 `canRestock` |
| 6 | `mob-oracle --only villager_gossip / villager_breed / villager_golem`：八卦逐题比 `add/decay/transferFrom/getReputation` 的整数表和 CODEC（用 `GossipContainer.CODEC` 导出）；繁殖比 `canBreed`、275+nextInt(50) 倒计时、床位扣还、幼崽品种（三种来源）与 `Age`；铁傀儡比 `wantsToSpawnGolem` 的 `LAST_SLEPT < 24000`、`GOLEM_DETECTED_RECENTLY` 599 刻、3 / 5 人数门槛、成功后的生成实体与落点 | 加权抽样的随机源、`decay` 的 `<2` 删除、`VillagerMakeLove` 时长、生成原因 `MOB_SUMMONED` | `--break-self-test villager_gossip`（改 weight）、把 `24000` 改成 `12000`、漏 `giveBedToChild` |
| 7 | 扩现有 `--only patrol` 的框架做 `--only cat_spawner` / `wandering_trader`：长场景（几十万刻）比 `nextTick`、每次 `spawnDelay/spawnChance`、随机玩家偏移、生成实体的类型 / 坐标 / 持久化 / `despawnDelay` / `wanderTarget`，以及结束时世界随机源种子（现有 patrol oracle 已这么做）。转化用 `--only villager_conversion`：逐刻比 `villagerConversionTime`、1% 加速扫描取数、搬过去的 `VillagerData/Offers/Gossips/Xp`、声望变化；雷击用现有雷击随机刻场景加一个村民 | 刷怪器在主世界顺序（phantom→patrol→cat→siege→trader）里对世界随机源的取数位置、难度分支、存档键 | 把 CatSpawner 挪到巡逻队之前 / 改 `nextInt(10)` 门槛 / 漏 `ConversionPlayer` |
| 8 | 扩 `jigsaw` / `structure_e2e` oracle：Java 侧 `StructureTemplate.placeInWorld` 后导出 `level.getEntities(...)` 的 `saveWithoutId`；Bamboo 侧把 `PlacedEntity` 落地后导出 `mob/nbt.rs::write_mob`，按 `(实体类型, Pos, VillagerData, Offers, Brain)` 比对。现有 `place_entities_matches_the_vanilla_transform` 单测继续守几何 | 实体数量 / 位置 / 朝向 / 品种 / 是否被 `finalizeSpawn` | 丢掉 `PlacedBlocks.entities` / 不删 `UUID` |

**通用回归**：每期合入前跑 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、
`cargo test --workspace`（全量很慢，给 3600 秒）、`mob-oracle` 全量
（当前基线 1841 例 190745 刻 0 不一致），以及本工作树的
`tools/mc-bamboo-vanilla` 新 oracle 的 `--only` 子集。
精确 / 齐步模式的既有对拍要显式选模式（`docs/island-implementation-plan.md` §9 第 7 条），
不能依赖缺省。

---

## 5. 工作量估计

粗估以「一个熟悉本仓库的开发者」为单位，含实现 + 单测 + oracle 场景 +
改坏自检 + 文档；不含代码评审往返，不含袭击 / 劫掠 / 不祥之瓶系统，
不含客户端 UI 的完整体验打磨（村民交易界面本身的 3 槽交互要做，但
粒子 / 音效 / 进度条动画可后置）。

| 期 | 内容 | 主要新增文件 | 估计 |
|---|---|---|---|
| 1 | POI 存储、段索引、存档、方块登记、拆合 | `mc-bamboo-world/src/poi.rs`（或新 crate 模块）、`storage/poi_nbt.rs`、`game/region/poi.rs`、`IslandPart::Poi`、oracle | **2–3 周** |
| 2 | 数据导出 + 村民实体骨架 | `data/generated/trades.rs` / `villager.rs`、`mob/villager.rs`、`entity/nbt` 扩展、元数据 / 跟踪 / 存档接口 | **1.5–2 周** |
| 3 | 大脑与日程 | `brain/` 扩 Activity / MemoryType / SensorKind / Kind、timeline 视图、40 个行为的骨架 + 10 个真实现 | **3–4 周**（最大的一块） |
| 4 | 职业与工作站 | `mob/villager_job.rs`、`AcquirePoi` 等行为、POI 票与释放 | **1.5–2 周** |
| 5 | 交易 | `mc-bamboo-game/src/merchant.rs`、`protocol` 两个包、`menu/merchant.rs`、数据包求值 | **2–3 周** |
| 6 | 八卦 / 繁殖 / 铁傀儡 | `mob/gossip.rs`、`VillagerMakeLove`、`IronGolem` 最小实体 | **1.5–2 周** |
| 7 | 刷怪器与转化 | `game/cat_spawn.rs` / `wandering_trader.rs`、`ZombieVillager`、saved data | **2 周** |
| 8 | 结构实体与收尾 | `GeneratedChunk.entities` 管线、世界生成 → 实体表、文档 | **1 周** |
| | **合计** | | **约 15–19 人周** |

若把「行为先全占位、只跑结构表 + 日程」作为期 3 的验收线（真行为挪到
期 4/6 分批补），期 3 可压到 **1.5 周**，总量降到 **13–16 人周**，
代价是村民在期 3 结束时还不会走 / 睡 / 聚会。建议按「结构表先行」拆成
3a / 3b 两个可独立验收的小期。

**风险缓冲**：期 1 的存档 / 岛拆合、期 3 的行为数量、期 5 的
`LootContext` / `ItemStackTemplate` 求值（附魔、地图、药水等复杂
`given_item_modifier`）是三处最容易超估的地方，各留 0.5–1 周缓冲。

---

## 6. 与已有代码的接口点和风险

### 6.1 接口点清单

| 官方类 / 机制 | Bamboo 位置 | 要做什么 |
|---|---|---|
| `PoiManager` / `SectionStorage` | `mc-bamboo-world/src/store.rs::ChunkStore`、`storage/chunk_io.rs`、`storage/world_dir.rs:167` | 新建平级的 `PoiStore`；`poi_dir()` 已存在；`SaveSession` 要多收一份脏块；`RegionColumn` / `IslandPart` 加字段 |
| `Level.setBlock` → `updatePOIOnBlockStateChange` | `mc-bamboo-world/src/store.rs:447::ChunkStore::set_block` | 唯一写入口；产 `PoiDelta`，不要改 `ChunkStore` 的公开签名（可加一个 `poi_delta: &mut Vec<…>` 出参或返回 `PoiDelta`） |
| `ServerLevel.isCloseToVillage` / `sectionsToVillage` | `game/interaction.rs` 的 `BlockView` / `entity/ctx.rs::EntityCtx` | 加只读 `poi_count_in_range` / `sections_to_village`；并行 `SourceView`（`entity/view.rs`）也要给同一份快照，否则精确模式会分叉（现有 `is_raining_at` 的已知坑） |
| `Brain.Provider` / `ActivityData` | `brain/mod.rs`、`brain/behavior.rs`、`brain/behaviors.rs` | 复用；加 `Activity` 变体、`rebuild`、`schedule`；`Cx` 要能查 POI、开菜单、生成实体 |
| `MemoryModuleType(GlobalPos)` | `brain/memory.rs`（`COUNT=32`、`ALL`、`MemoryValue`） | 追加枚举项（**末尾追加**保住 `filled()` 顺序与存档下标）、加 `GlobalPos` 值和 codec 判定；`brain/nbt.rs` 已有 `{value, ttl}` 框架 |
| `timeline/villager_schedule.json` | `game/global.rs::Global::sky_darken` / `GlobalView`、`mc-bamboo-blocks/src/sky.rs` | 照 `sky_light_level` 关键帧的做法加两个 `Activity` 快照字段；**不要**引入完整 `EnvironmentAttributeSystem` |
| `VillagerData` / `EntityDataSerializers.VILLAGER_DATA` | `protocol/packet/play/entity.rs::EntityData`、`game/entity_sync.rs`、`game/entity_tracking.rs:999` | 新增元数据变体（要核 `VILLAGER_DATA` 的序列化器 ID 和 `Holder` 编码）、`default_data_for_type`、非默认数据同步 |
| `VillagerType.byBiome` | `mc-bamboo-data/src/generated/worldgen.json` 的群系表、`RegionLevel` 的 `biome_at` | 七个群系标签 → 品种的映射表导出 |
| `trade_set` / `villager_trade` | `mc-bamboo-data/src/generated/*`、`mc-bamboo-loot::RandomSequences` | 静态导出 + `random_sequence` 取数接 `Region.loot_sequences`（现有） |
| `MerchantOffers.CODEC` / `STREAM_CODEC` | `mc-bamboo-nbt`、`protocol/packet/play/menu.rs`、`mc-bamboo-protocol/src/item.rs::ItemStack` | 新结构 + 编解码；`ItemCost` / `ItemStackTemplate` 要接现有组件系统 |
| `OpenScreen` / `MERCHANT_OFFERS` / `SELECT_TRADE` | `protocol/packet/play/menu.rs:15`、`generated/packets.rs:121,266`、`protocol/packet/play/request.rs::decode`、`game/menu.rs` | 两个新包 + 一个请求分支 + `Menu::Target` 的实体变体（现有 `Target` 只有 `BlockPos`） |
| `Region.menus` | `game/region.rs:249`、`game/region/column.rs`（拆合时菜单跟着玩家） | 交易菜单指向实体而不是方块，拆岛时要按「玩家和他附近的区块总是一起搬」的既有不变量处理；实体 ID 在两个岛间由共享分配器保证唯一（`island-implementation-plan.md` §1 第 14 条） |
| `customSpawners` 顺序 | `game/world.rs:3447-3448`、`game/patrol_spawn.rs` | 幻翼 / 巡逻队之后按顺序加猫、围攻占位、流浪商人；**每岛一份计时器**是既有约定，与官方「每维度一份」有已知差异（`patrol_spawner` 注释已写明），要一并记录 |
| `WanderingTraderData` | `game/world_state.rs`、`storage/world_dir.rs::saved_data` | `data/minecraft/wandering_trader.dat`，照 `world_border.dat` / `chunk_tickets.dat` 的路径规则 |
| `Mob.convertTo` / `ConversionTracker` | `mob/conversion.rs` | 复用 `SINGLE` 分支；补 `CONVERSION` / `MOB_SUMMONED` / `BREEDING` / `EVENT` 四种 `EntitySpawnReason`（`mob/spawn.rs` 现在只有 4 种） |
| `StructureTemplate.placeEntities` | `worldgen/structure/jigsaw/place.rs:201` 的 `place_entities` / `PlacedEntity`、`worldgen/structure/place_step.rs:668`、`game/worldgen.rs::GeneratedChunk` | 产出已有，缺「`PlacedBlocks.entities` → `GeneratedChunk.entities` → `Region.entities`」管线 |
| `mob/nbt.rs` | `mob/nbt.rs:174::write_mob` / `:574::read_mob` / `:604::load_into` | 补 `VillagerData` / `Offers` / `Gossips` / `FoodLevel` / `Xp` / `LastRestock` / `LastGossipDecay` / `RestocksToday` / `VillagerDataFinalized`；`Brain` 键已支持 |
| `EntityStore` / `RegionColumn.entities` | `entity/store.rs:920::spawn`、`:979::spawn_row`、`game/region/column.rs:81` | 结构实体落地走 `spawn_row`，避免逐只重算空间索引 |
| oracle | `oracle/brain.rs`、`oracle/water_mob.rs::brain_table`、`java/MobOracle.java:1490` | 复用 `activities\|running\|memories` 转储与结构表；新增 `villager_*` 场景 |

### 6.2 风险

1. **POI 是第二份按区块持久化的数据，岛拆合最容易出错。**
   `RegionColumn` 现在是「玩家和他附近的区块总是一起搬」的不变量撑着的
   （`game/region/column.rs` 文件头）。POI 段按 `SectionPos.y` 分组，
   一个区块有 `WORLD_HEIGHT/16` 层；拆岛时漏一层就会出现「床在 A 岛、
   POI 在 B 岛」。建议把 `Poi` 做成 `RegionColumn` 的一等字段并加
   `IslandPart::Poi` 的断言测试，而不是放在 `Region` 上单独按坐标切。
2. **方块登记的时序与官方不同。** 官方 `updatePOIOnBlockStateChange`
   把 add/remove 丢进 `MinecraftServer.execute` 异步回主线程；Bamboo
   单世界线程 + 多岛并行，若照搬会出现「同一刻里村民先查 POI、方块后登记」
   的竞态。建议同刻顺序应用，并在 oracle 场景里显式验证顺序差异不影响
   结果（或标注为已知差异）。
3. **并行 `SourceView` 的只读快照。** `docs/mob-entity-design.md` 已记
   `is_raining_at` / `level_random_*` 在并行路径给缺省值；POI 查询若只接
   `EntityCtx` 的精确路径，精确 / 分离驱动两种模式下村民行为会分叉。
   必须让 `ViewFacts` 带上 POI 快照（或至少带上「本岛 POI 表的只读句柄」）。
4. **`GlobalPos` 记忆的存档与跨岛。** `Brain.Packed` 只存带 codec 的记忆；
   `HOME/JOB_SITE/MEETING_POINT` 是 `GlobalPos(dimension, BlockPos)`，
   存档要写维度键。Bamboo 只有主世界，但岛拆合后 `GlobalPos` 不带岛号，
   村民的 `HOME` 可能指向另一个岛的床；这和「岛间隔 ≥128 格、POI 查询
   48/128 格上限在本岛内」一起约束，仍要在 `releasePoi` / 查询里拒绝跨岛结果。
5. **行为数量与 `Kind` 枚举膨胀。** `brain/behaviors.rs` 现在 1185 行，
   40 个新行为会让 `Kind` 的 `required()` / `class()` / `trigger()` /
   `table_name()` 匹配表变成维护热点。建议给村民行为单独一个
   `Kind::Villager(villager::Behavior)` 分支（照 `Kind::Goat` / `Kind::Frog`
   的既有做法），公共骨架不动。
6. **交易数据的复杂性。** 391 个内置 `villager_trade` 里有
   `given_item_modifier`（附魔、地图、`filtered`、`discard`）、
   `merchant_predicate`（`villager/variant` 等）、`double_trade_price_enchantments`、
   两槽 `wants`。首版若全量求值，会牵出 `LootItemFunction` / `LootItemCondition`
   的一大批未实现分支。建议首期把内置表导全、把「简单 give/wants」
   的交易跑通，复杂的 modifier 先按官方取数次数占位、在已知限制里列清楚。
7. **`trade_rebalance` 数据包覆盖。** jar 里 `data/minecraft/datapacks/
   trade_rebalance/` 有另一套 `villager_trade`（81 个）/ `tags`（10 个）。
   Bamboo 没有数据包系统，是否要支持「启用数据包时换表」是范围决定
   （§7 决定 2）。另外 `VillagerTrades` 类声明的 `ResourceKey` 常量（289 个）
   与内置 `villager_trade/*.json`（391 个）数量不一致：部分 json 由
   `TradeRebalanceVillagerTrades` 或多次 `register` 产生，本轮没有逐个对齐
   键名与文件。实现期应以「jar 内 json 全量导出 + 逐文件哈希单测」为准，
   不要拿 Java 常量清单当完整性依据。
8. **袭击 / 劫掠系统不存在。** `getPreRaidPackage` / `getRaidPackage` /
   `getHidePackage` / `SetRaidStatus` / `ReactToBell` 的部分分支、
   `GiveGiftToHero`（英雄特效送礼）、`CelebrateVillagersSurvivedRaid` 都依赖
   `Raid`。现有 `pillager.rs` / `raider.rs` 的做法是「目标占位、`canUse` 恒假、
   保住取数次数」，村民侧照抄即可；但**结构表里的类名与优先级必须照挂**，
   否则 `mob-oracle` 的结构表比对会失败。
9. **玩家 UUID / 声望。** `GossipContainer` 用玩家 UUID 做键。Bamboo 的
   `PlayerId` 是运行时编号，存档 / 声望要用 `Player.uuid`（
   `player_data.rs` 已有 UUID 往返）；离线模式下 UUID 的生成规则要和官方一致，
   否则重启后折扣丢失。
10. **`MerchantMenu` 与现有 `Menu` 模型不同。** `Menu.target` 只认方块，
    `RegionColumn` 拆合时菜单按区块搬。交易菜单指向实体，
    跨岛 / 村民死亡 / 玩家离开 4 格时的 `stillValid` 要单独处理，
    别硬塞进 `Target::Single(BlockPos)`。
11. **自定义刷怪器「每岛一份」的既有差异。** `region.rs:273` 注释已说明
    `patrol_spawner` 每岛一份、拆岛时两边各带自己的计时器，
    与官方「每主世界一份」不同。猫 / 流浪商人也照此做时，
    多岛下的生成频率会随岛数翻倍；要么沿用并标注，要么把计时器放到
    `Global` 用快照下发（改动更大）。建议先沿用既有约定并在文档里记。
12. **世界随机源取数顺序。** 新增刷怪器会插进 `run_island_simulation`
    的随机刻之后、幻翼 / 巡逻队附近，**改变后续所有 `level.random` 取数**。
    现有 `mob-oracle` 的 1841 例全量回归会立刻暴露；必须按顺序插入，
    并给「关闭新刷怪器」的配置 / 场景开关，方便逐期对拍不互相污染。

---

## 7. 需要主代理 / 用户拍板的决定

1. **期 1 与期 2 的先后。** 建议 **POI 先行**：工作站、床、钟、避雷针、
   猫、流浪商人、`isCloseToVillage` 全压在 `PoiManager` 上，先做能让后面
   七期都在真实数据上验收；代价是期 1 结束时用户看不到村民。
   备选是「先做实体骨架 + 数据导出（`/summon` 能出一只静止村民）」，
   能让进度更可见，但随后几期都要用假 POI 夹具，后期返工。
2. **`trade_rebalance` 数据包覆盖是否进首版。** 建议 **不进**：
   只导出 jar 里的内置 `trade_set` / `villager_trade` / 标签（68 + 391 + 73 个），
   把数据包系统留到单独一期。若用户要求「`/datapack enable trade_rebalance`
   能换交易表」，需要在期 2 就引入数据包加载与注册表覆盖模型，工作量
   +1–2 周且影响 `mc-bamboo-data` 的整个生成流程。
3. **袭击相关活动表的处理。** 建议结构表照挂、行为占 `Unavailable`
   （与现有 `Raider` 目标占位一致），不实现 `Raid`。若要求
   PRE_RAID / RAID / HIDE 真跑，需要先做袭击系统，超出本方案范围。
4. **期 3 是否拆成 3a（结构表 + 日程）/ 3b（真行为）。** 建议拆，
   每半期都能单独对拍，也把最大的一块风险摊开。
5. **`WanderingTrader` / `IronGolem` / `Cat` 的实体完成度。**
   建议首版是「能被生成、能存档、能被看见、有正确属性与消失计时」的
   最小实体，攻击 AI / 驯服 / 拴绳 / 铁傀儡目标表后补；否则期 6/7
   会被无关系统拖长。

---

## 8. 本轮的验证方式

- `javap -c -p` 读了（转储在 `/tmp/vj_*.txt`，不提交）：
  `Brain` / `Brain$Provider` / `ActivityData` / `Activity` / `MemoryModuleType` /
  `SensorType` / `EnvironmentAttributes` / `Timelines` / `Timeline` /
  `AttributeTrack` / `UpdateActivityFromSchedule` / `VillagerGoalPackages` /
  `Villager` / `AbstractVillager` / `VillagerData` / `VillagerType` /
  `VillagerProfession` / `PoiManager(+Occupancy/DistanceTracker)` /
  `PoiSection(+Packed)` / `PoiRecord(+Packed)` / `PoiType` / `PoiTypes` /
  `SectionStorage` / `SimpleRegionStorage` / `ServerLevel` / `ChunkMap` /
  `Level` / `MinecraftServer` / `ServerChunkCache` / `VillagerTrades` /
  `VillagerTrade(+Builder)` / `TradeSet` / `TradeSets` / `MerchantOffer` /
  `MerchantOffers` / `ItemCost` / `TradeCost` / `Merchant` / `MerchantMenu` /
  `ClientboundMerchantOffersPacket` / `ServerboundSelectTradePacket` /
  `GossipContainer(+EntityGossips/GossipEntry)` / `GossipType` /
  `VillagerMakeLove` / `VillagerFood` / `AcquirePoi` /
  `AssignProfessionFromJobSite` / `ResetProfession` / `WorkAtPoi` /
  `VillagerPanicTrigger` / `VillagerCalmDown` / `GolemSensor` /
  `VillagerHostilesSensor` / `VillagerBabiesSensor` / `SecondaryPoiSensor` /
  `NearestBedSensor` / `CatSpawner` / `WanderingTraderSpawner` /
  `WanderingTrader` / `WanderingTraderData` / `ZombieVillager` / `Zombie` /
  `IronGolem` / `StructureTemplate` / `StructureProcessor`。
- `unzip -p` 读了：`timeline/villager_schedule.json`、`tags/timeline/*`、
  `timeline/day.json`、`trade_set/<prof>/level_*.json`、
  `tags/villager_trade/<prof>/level_*.json`、`villager_trade/**.json`、
  `datapacks/trade_rebalance/...`、`structure/village/...` 清单。
- 源码只读核对：Bamboo 的 `brain/*`、`mob/mod.rs` / `nbt.rs` /
  `spawn.rs` / `conversion.rs`、`entity/store.rs`、`entity_tracking.rs`、
  `entity_sync.rs`、`menu.rs`、`protocol/packet/play/{entity,menu,request}.rs`、
  `game/{region,world,global,worldgen,worldgen_structures,patrol_spawn}.rs`、
  `game/region/{column,island_split}.rs`、`storage/{world_dir,chunk_content,chunk_nbt}.rs`、
  `worldgen/structure/{place_step,jigsaw/place}.rs`、`data/generated/*`、
  `tools/mc-bamboo-vanilla/{mob_oracle/brain.rs,mob_oracle/water_mob.rs,java/MobOracle.java}`、
  `docs/{worldgen-structures-e2e,island-implementation-plan,mob-entity-design}.md`。
- **没有跑** `cargo test` / `cargo clippy` / oracle（本轮不改代码，
  没有可验证的新行为）；没有启动真实服务端或客户端，没有接受 EULA。
