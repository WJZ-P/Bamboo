# 僵尸与骷髅：现状摸底与分期方案

状态：**期 1／2 已合并；期 3 的实现、对拍范围见下方「期 3 现状」**。以下 §1–3 保留原始调研作为方案背景。原调研核对方式：读本工作树源码 + `javap -c -p`
（`target/vanilla/versions/26.3/server-26.3.jar`）。本轮没有跑 `cargo test` / oracle
（没有改代码，也没有启动真实服务端或客户端）。

**前提**：`Level::for_each_player` 已在 main 修好（`RegionLevel::for_each_player`
和并行算阶段的 `ReadOnlyWorld.players` 都能给出本岛玩家，按 `PlayerId` 升序）。
下面的方案以「它可用」为前提，不再把「实体看不见玩家」列为风险；生物跨岛查询的
边界仍如 `docs/mob-entity-design.md` 5.5 所述：岛间隔 ≥ 128 格，35 格的
`FOLLOW_RANGE` 和 128 格的消失判定都在本岛内。

**路径写法**：表格里写全路径；正文里的简写按所在 crate 解析——
`entity/*`、`world.rs`、`level.rs`、`gamerules.rs`、`dispense/*`、`item_use/*`、
`protocol/*` 属于 `mc-bamboo-blocks`（`protocol/*` 指 `mc-bamboo-protocol`）；
`player.rs`、`command/*`、`interaction*`、`entity_sync.rs`、`entity_tracking.rs`、
`region/*`、`global.rs`、`gamerules.rs`、`world_state.rs`、`drops.rs`、
`experience_pickup.rs`、`player_save.rs` 属于 `mc-bamboo-game`；
`generated/*`、`registries.rs`、`entity_types.rs` 属于 `mc-bamboo-data`；
`archive/entity.rs`、`chunk_content.rs` 分别是 `mc-bamboo-blocks` /
`mc-bamboo-storage`；`tools/mc-bamboo-vanilla/*` 用全路径。
引用的行号以本工作树此分支的当前内容为准。

---

## 1. 七条摸底结论（每条带代码位置）

### 1.1 玩家受伤与死亡：完全没有（已知差异，已复核）

| 项 | 现状 | 位置 |
|---|---|---|
| 血量 / 受伤 / 死亡 / 重生字段 | `Player` 没有；`Player::view` 的 `alive` 恒为 `true`，`invisible=false`、`armor_cover=0`，注释明确「Bamboo 还没有玩家生命和药水效果」 | `crates/mc-bamboo-game/src/player.rs:30-125`, `:292-310` |
| 同步血量 | `EntitySync::Metadata::of` 用常量 `PLAYER_HEALTH = 20.0`；配对时发 `EntityData::Health`，之后不会变 | `crates/mc-bamboo-game/src/entity_sync.rs:41`, `:77-91` |
| 摔落 | `player_move.rs` / `Motion` 只记录 `fall_distance`，不转成伤害 | `crates/mc-bamboo-game/src/player_move.rs`；`docs/mob-entity-design.md:289-290` |
| `/kill` | 选中玩家明确 `return Err(fail("command.failed"))`；非玩家生物走 `World::hurt_mob` / `Entity.kill` | `crates/mc-bamboo-game/src/command/kill.rs:39-46`, `:118-135` |
| `/damage` | 命令树里没有；`catalog.rs:1660` 的 `damage` 是 `/worldborder damage` 子命令 | `crates/mc-bamboo-game/src/command/catalog.rs:1660` |
| 客户端左键攻击包 | `serverbound::ATTACK`（ID 1）在 `PlayRequest::decode` 里没有分支，落到 `Other(other)`，游戏层忽略；只有右键 `INTERACT` 走 `on_interact_entity`（牛喂食/挤奶） | `crates/mc-bamboo-protocol/src/packet/play/request.rs:301-421`；`crates/mc-bamboo-game/src/world.rs:2472-2487`；`crates/mc-bamboo-game/src/interaction.rs:1010-1100` |
| 玩家自己的伤害入口 | 只有 oracle 场景注入（`world.hurt_mob` 打的是**生物**）；没有「打玩家」的 `Command::Damage`，玩家不在实体表里 | `crates/mc-bamboo-blocks/src/world.rs:2958-2983`；`crates/mc-bamboo-blocks/src/entity/ctx.rs:337-355`（注释预留 `Damage`）；`crates/mc-bamboo-game/src/interaction.rs:173-176` |
| 重生包 | `ClientCommandKind::PerformRespawn` 已解码但 `on_client_command` 只处理 `RequestStats`；`SetHealth` / `Respawn` / `PlayerCombat*` / `HurtAnimation` 包只有注册表 ID，没有 Rust 包结构 | `crates/mc-bamboo-game/src/chat.rs:200-205`；`crates/mc-bamboo-data/src/generated/packets.rs:95,111,135-137,154,177` |
| 存档 | `PlayerData` 已有 `Health` / `HurtTime` / `DeathTime` / `AbsorptionAmount` / `food*` 等键并可往返，但 `PlayerData::from_runtime_parts` 和 `restore_inventory` 都用官方默认值（血 20），不恢复 | `crates/mc-bamboo-storage/src/player_data.rs:65-100`, `:208-232`, `:305-347`, `:716-760`；`crates/mc-bamboo-game/src/player_save.rs:25-45`, `:86-110` |
| 游戏规则 | 注册表已有 `keep_inventory`(18) / `immediate_respawn`(17) / `natural_health_regeneration`(32) / `spawn_mobs`(47) / `spawn_monsters`(48) / `show_death_messages`(45) 等；游戏层 `GameRuleSet::block_snapshot()` 只把 14 条下发给方块逻辑，`keep_inventory` 没有任何读取点 | `crates/mc-bamboo-data/src/generated/gamerules.rs:17-59`, `:104`, `:133`；`crates/mc-bamboo-game/src/gamerules.rs:88-108` |

**官方字节码要点**（本轮新核对，供第 1 期照接）：
- `Player.hurtServer`（`Player.java` 1492 行起）：能力无敌 / `BYPASSES_INVULNERABILITY` / 创造模式；
  **难度缩放**：`scalesWithDifficulty` 为真时和平 → 0（直接不受伤）、简单 →
  `min(amount/2+1, amount)`、困难 → `amount*3/2`；再调 `LivingEntity.hurtServer`。
- `LivingEntity.hurtServer`：格挡 `applyItemBlocking`、冻结增伤、`DAMAGES_HELMET`、
  `NaN/Inf → Float.MAX`、无敌帧 `damageCooldownTime > 10` 时只吃「比 `lastHurt` 多出来的
  那部分」、完整受击时重置 20 刻冷却与 10 刻 `hurtTime`；`broadcastDamageEvent`、
  `markHurt`、默认击退、图腾、`die`。这套「受伤骨架」Bamboo 的生物侧已有同位实现
  （`living.rs:1083-1161`），可抽成共享逻辑，但玩家的 `actuallyHurt` 另有：
  `CombatRules.getDamageAfterAbsorb`（`2+toughness/4`、`clamp(armor−amount/f, armor*0.2, 20)`、
  再乘 `1−f/25`，其中 `f` 被 `EnchantmentHelper.modifyArmorEffectiveness` 夹到 [0,1]）→
  `getDamageAfterMagicAbsorb`（抗性效果/保护附魔，clamp 0..20）→ 吸收血量 → 饥饿消耗、
  `CombatTracker`、扣血、`ENTITY_DAMAGE` 游戏事件。Bamboo 没有「从 6 槽装备算护甲值/韧性/
  保护附魔等级」的运行时代码（只有装备同步 `entity_sync.rs:332` 和物品组件表）。
- `Player.dropEquipment`：`keepInventory` 为假 → `destroyVanishingCursedItems` + `Inventory.dropAll()`；
  `Player.getBaseExperienceReward`：`keepInventory || isSpectator ? 0 : min(level*7,100)`；
  `LivingEntity.dropExperience` 对玩家 `isAlwaysExperienceDropper=true`，`ExperienceOrb.award`。
  `ServerPlayer.die` 发 `PlayerCombatKill` + 死亡消息（受 `show_death_messages` 和队伍可见性约束）。
  `PlayerList.respawn` 重建 `ServerPlayer`、`restoreFrom(..., keepInventory)`（其中
  `transferInventoryXpAndScore` 搬物品栏/等级/进度/总经验/`Score`/`enchantmentSeed`），
  发 `Respawn`、`SetDefaultSpawnPosition`、`ChangeDifficulty`、`SetExperience` 等包。
  这些和 `docs/mob-entity-design.md:286-306` 的 5.7 节一致。
- `ServerPlayer.tickRegeneration`：和平 + `naturalHealthRegeneration` 时每 20 刻回 1 血；
  `FoodData.tick` 在 `ServerPlayer.doTick`，Bamboo 也还没有饥饿系统。

### 1.2 敌对生物需要的基础：难度、光照、阳光下燃烧、`Monster` 基类

| 项 | 现状 | 位置 |
|---|---|---|
| 难度状态 | `WorldState::difficulty`（默认 Easy）、`/difficulty` 查询/设置、进服 `ChangeDifficulty` 包、`level.dat` 读写都有；`GlobalView` 每刻把难度发布给每个岛。**但没有任何逻辑读它**（`World::difficulty_id()` 恒返回 NORMAL=2） | `crates/mc-bamboo-game/src/world_state.rs:29-49`, `:118-140`；`crates/mc-bamboo-game/src/global.rs:637-680`；`crates/mc-bamboo-game/src/command/world_settings.rs:22-67`；`crates/mc-bamboo-blocks/src/world.rs:96-99`, `:1811-1814` |
| 和平清除 | 官方入口是 `Mob.checkDespawn` 开头：难度 == PEACEFUL 且 `!EntityType.isAllowedInPeaceful()` 时 `discard()`（不是 `ServerLevel` 主动扫全表）。Bamboo 的 `check_despawn` 按牛写死，注释「难度不是和平，`isAllowedInPeaceful` 也为真，这一支不删」，且 `Animal.removeWhenFarAway=false` 不消失；没有类别化 `MobCategory`/128 格消失 | `crates/mc-bamboo-blocks/src/entity/mob/living.rs:112-139`；官方 `Mob.java` 1605 行起 |
| 光照 | `BlockView::brightness(sky/block)` 已有（真实光照逐格读区块表）；新世界时钟按官方从 0 刻走起来，`GlobalView::sky_darken` 每刻按官方 `Level.updateSkyBrightness` 从主世界时钟 + `SKY_LIGHT_LEVEL` 时间线 + 雨 / 雷暴算出，`World::max_local_raw_brightness` / `effective_sky_brightness` 读 `Level::sky_darken()`，并行 `SourceView` 的 `ViewFacts::sky_darken` 读同一份快照；`can_see_sky` / `is_raining_at` 在 `World` 精确路径已有；**并行 `SourceView` 仍未覆盖** `is_raining_at` / `level_random_*`，恒给缺省值 | `crates/mc-bamboo-blocks/src/sky.rs`；`crates/mc-bamboo-blocks/src/entity/ctx.rs`；`crates/mc-bamboo-blocks/src/entity/view.rs`；`crates/mc-bamboo-blocks/src/world.rs`；`crates/mc-bamboo-game/src/global.rs`, `:interaction.rs` |
| 阳光下燃烧 | `Mob.aiStep` 里 `EntityTypeTags.BURN_IN_DAYLIGHT` → `burnUndead()`；`isSunBurnTick` 要 `EnvironmentAttributes.MONSTERS_BURN`、`getLightLevelDependentMagicValue()>0.5`、`random.nextFloat()*30 < (magic−0.4)*2`、不在水/雨/粉雪、眼睛那格 `canSeeSky`；`burnUndead` 头盔可损坏则 `hurtAndBreak(nextInt(2))` 并挡住，否则 `igniteForSeconds(8)`。`LevelReader.getLightLevelDependentMagicValue` = `maxLocalRawBrightness/15` → `f/(4−3f)` → 按维度 `ambientLight` 插值。Bamboo 只有牛的 `walk_target_value` 算了类似的魔法值（`goals.rs:873-884`），没有 `burnUndead`；`generated/tags.rs:967` 已有 `burn_in_daylight` 标签（含 zombie/skeleton/husk/stray/drowned 等 ID） | 官方 `Mob.java` 884-1075 / `LevelReader.java` 203-234；`crates/mc-bamboo-blocks/src/entity/mob/goals.rs:873-884`；`crates/mc-bamboo-data/src/generated/tags.rs:967` |
| `Monster` 基类 | 官方：`xpReward=5`；`aiStep` 里 `magic>0.5` 时 `noActionTime += 2`；`getWalkTargetValue = −getPathfindingCostFromLightLevels`；`shouldDropExperience=true`；`shouldDropLoot` 读 `MOB_DROPS`；`createMonsterAttributes` 只加 `ATTACK_DAMAGE`；`isDarkEnoughToSpawn` 生成条件（sky brightness ≤ `nextInt(32)`、block light > 维度上限则拒绝、雷暴用 `getMaxLocalRawBrightness(pos,10)`、和 `monsterSpawnLightTest.sample(random)` 比较）。Bamboo 的 `Mob` 是「`LivingEntity → Mob → PathfinderMob → AgeableMob → Animal → Cow`」一条链，没有 `Monster` 层：`xp_reward` 写死在 `die()` 里为牛的 `1+nextInt(3)`、声音是 `CowSoundVariant`、`check_despawn` 按牛、`Malus::animal()`、`getWalkTargetValue` 是草方块+亮度加权 | `crates/mc-bamboo-blocks/src/entity/mob/mod.rs:1-5`, `:459-570`；`crates/mc-bamboo-blocks/src/entity/mob/living.rs:112-139`, `:1212-1255`；`crates/mc-bamboo-blocks/src/entity/mob/path.rs:41-63`；官方 `Monster.java` 全文 |

结论：**基础件（属性表、伤害骨架、寻路、光照查询、实体同步）大部分已经在了，
缺的是「把它们从『只支持牛』泛化」**：`MobType`/`MobPublic`/`Attributes`/`GoalSelector`/
`spawn`/`nbt`/`archive`/`entity_tracking::default_data`/`mob_loot_drops` 里到处是牛的硬编码
（见 1.5），以及把难度/天色/雨/环境属性接到 `BlockView` 的两种驱动上。

### 1.3 自然生成：完全没有，可以只靠 `/summon` 和刷怪蛋

- 官方 `NaturalSpawner`：`ServerChunkCache.tickChunks` 每刻重新 `createState`（遍历全岛实体、
  按 `MobCategory` 计数、`PotentialCalculator`、`LocalMobCapCalculator`），
  `getFilteredSpawningCategories`（全局上限 `maxInstancesPerChunk * spawnableChunkCount / 289`，
  `MAGIC_NUMBER = 17²`），`spawnForChunk` 逐类别检查 `canSpawnForCategoryLocal`，
  `getRandomPosWithin` → `getRandomSpawnMobAt`（群系/结构的 `MobSpawnSettings.SpawnerData`
  权重表）→ `checkMonsterSpawnRules`（`Monster.isDarkEnoughToSpawn`）→ `isValidPositionForMob`
  → `finalizeSpawn`；候选点还要 `isRightDistanceToPlayerAndSpawnPoint`（离玩家 > 24 格、
  离世界出生点 > 24 格、区块能刷怪，常量 `MIN_SPAWN_DISTANCE=24`、
  `SPAWN_DISTANCE_CHUNK=8` / `SPAWN_DISTANCE_BLOCK=128`）。
  `MobCategory.MONSTER` 上限 70、`despawnDistance` 128、`noDespawn` 32。
  官方的刷怪权重表由 `BiomeDefaultFeatures.monsters` 等生成器代码给出（不是数据文件），
  Bamboo 的 `generated/worldgen.json` 里没有群系 `MobSpawnSettings`，`mc-bamboo-data`
  也没有 `spawn_settings` 表。
- Bamboo：**没有任何自然刷怪入口**（`interaction::run_random_ticks` 只跑官方随机刻/雷击，
  `run_island_simulation` 的阶段表里没有刷怪阶段）；`spawn.rs` 文件头明确「不做自然刷怪」；
  生成入口只有 `/summon minecraft:cow`（`command/summon.rs:101` 硬编码只放行 cow）
  和牛刷怪蛋（`item_use/mod.rs:59`）。
- 所以**僵尸骷髅可以先只靠 `/summon` + 刷怪蛋**：`SpawnReason` / `finalize_spawn` /
  `is_in_spawnable_bounds` / `spawn_y_offset` / `snap_to` 都是可复用的通用件；
  战利品表 `minecraft:entities/zombie`、`.../skeleton`（含 husk/stray/bogged/drowned）已在
  `generated/loot.rs` 里，`entity_type` 和 `spawn_egg` 物品 ID 也都齐了。
  自然生成单独一期，且要先补「群系刷怪权重表」的数据导出（oracle 可照
  `BiomeDefaultFeatures` 生成或 `javap` 导出常量），否则精确模式对拍无从谈起。
- 用户已定「按玩家的上限缺省开」：`docs/mob-entity-design.md:21-27, 112-116`；
  `PlayerGrid::players_within(x,z,radius,&players)` / `nearest_distance` 已实现并有单测
  （5.1 最后一行），可以作为 `spawnableChunkCount` 和「按玩家上限」的输入，
  不必照搬 `LocalMobCapCalculator`。

### 1.4 弹射物：完全没有，箭是一整块新实体

- `EntityKind` 只有 `FallingBlock / Tnt / Item / ExperienceOrb / Mob` 五种
  （`entity/mod.rs:199-220`）；`KindState` / `KindPublic` / `EntityStore::by_type` /
  `is_hard_colliding` 同构（`entity/store.rs:111-200`）。没有 `AbstractArrow`、
  没有 `Projectile`（owner / leftOwner / deflect）基类。
- `dispense/entity.rs:1-14` 明确写着「箭、蛋、雪球……走默认抛出，不生成实体」；
  `ItemUse` 里也没有弓。
- 官方 `AbstractArrow` 是一整套：`tick`（`inGroundTime` / `shouldFall` / `tickDespawn`）、
  `shoot` / `getMovementToShoot`、`stepMoveAndHit`（`clip` 方块与实体）、
  `onHitEntity`（`baseDamage × 速度` → `damageSources().arrow` → `hurtOrSimulate`、
  `EnchantmentHelper.modifyDamage`）、`onHitBlock`（插在方块上、触发木按钮/靶、
  `hitBlockEnchantmentEffects`）、`playerTouch` / `tryPickup`（DISALLOWED / ALLOWED /
  CREATIVE_ONLY，`Inventory.add` → `LivingEntity.take` 的 `take_item_entity` 包）、
  `piercing` / `pickupItemStack` / `firedFromWeapon` / NBT。
  `Projectile` 基类还有 `spawnProjectileUsingShoot`、`checkLeftOwner`、`deflect`。
- 已有可复用的件：`physics::sweep_movement` / `clip_collider`（`entity/ctx.rs` 的
  `BlockView::clip_collider`）、`EntityStore` 的分桶与 `spawn_snapshots`、
  `entity_tracking::TrackedEntity::pairing_packets/step`（`entity_types::FACTS` 里
  `minecraft:arrow` 已有 range 4 / interval 20 / track_deltas true / 0.5×0.5）、
  `Inventory.add` 与 `take_item_entity` 包（`drops.rs`、`experience_pickup.rs`）。
  缺的是：`EntityKind::Arrow` 变体、箭的元数据（`AbstractArrow.defineSynchedData`
  只定义 `ID_FLAGS`(BYTE) / `PIERCE_LEVEL`(BYTE) / `IN_GROUND`(BOOL)，箭不在
  `LivingEntity` 继承树上，index 要按 `ClassTreeIdRegistry` 单独核对；
  `protocol/.../entity.rs` 现在只有牛的 15/16/17/18/19 号元数据）、
  箭的存档 NBT、命中实体/方块后的命令落地（`Command::Damage` / `ChangeBlock` /
  `Spawn` 都还只是 `ctx.rs:337-355` 注释里的预留名字）。
- 骷髅射箭还依赖：`AbstractSkeleton.performRangedAttack` →
  `ProjectileUtil.getMobArrow`（`setBaseDamageFromMob`：`2*power + triangle(difficulty*0.11, 0.57425)`）
  → `Projectile.spawnProjectileUsingShoot`（速度 1.6、散布 `rangedAttackUncertainty =
  14 − 4*difficultyId`）→ 射击音效；`RangedBowAttackGoal(mob, 1.0, 20, 15.0f)` 负责保持距离、
  `seeTime`、拉弓计时（HARD 20 刻 / 其它 40 刻，由 `AbstractSkeleton.getHardAttackInterval` /
  `getAttackInterval` 给出）；`MeleeAttackGoal` 在没弓时替换（`reassessWeaponGoal`）。

### 1.5 寻路和目标：地基够用，目标系统要从零补一半

- `WalkNodeEvaluator` + A* + `Path` + `Navigation` + `EvaluatorFlags` 已经逐位对拍过
  （`path.rs`、`navigation.rs`，`docs/mob-entity-design.md:165-177`）。
  `EvaluatorFlags { can_pass_doors: true, can_open_doors: false, can_float, can_walk_over_fences }`
  已存在，`path.rs:926-932` 已能把 `DoorWoodClosed` 在 `can_open_doors` 时改成 `WalkableDoor`；
  `Malus::animal()` 是牛的（火焰 16 / −1），敌人要换成 `Monster.getWalkTargetValue =
  −getPathfindingCostFromLightLevels` 的权重。
- `GoalSelector` **只有一个实例**（`Mob.goals`），调度规则（隔刻、标志位锁 MOVE/LOOK/JUMP/TARGET
  顺序、`reducedTickDelay`）与官方一致（`goals.rs:254-292`）；`Flag::Target` 枚举值在，
  但没有任何目标使用它，也没有第二个 `targetSelector`。官方僵尸/骷髅都注册
  `HurtByTargetGoal` / `NearestAttackableTargetGoal` 到 `targetSelector`。
- `Goal` 枚举 8 种全是牛的（Float / Panic / Breed / Tempt / FollowParent / Stroll /
  LookAtPlayer / RandomLookAround）；`GoalSelector::cow()` 是唯一构造器。
  `MeleeAttackGoal`、`RangedBowAttackGoal`、`RestrictSunGoal`、`FleeSunGoal`、
  `AvoidEntityGoal<Wolf>`、`MoveThroughVillageGoal`、`BreakDoorGoal`、
  `ZombieAttackTurtleEggGoal`、`SpearUseGoal` 全部没有。
  官方优先级（字节码）：僵尸 goalSelector 4/2/3/6/7/8/8，targetSelector 1/2/3/3/5；
  骷髅 goalSelector 2/3/3/5/6/6 + 4 号弓或近战，targetSelector 1/2/3/3。
- 目标选择可以用 `for_each_player`（已修好）+ `EntityCtx::for_each_other_in` / `other`
  （`ctx.rs:305-335`），以及现成的 `living::has_line_of_sight`（`living.rs:1059-1078`）
  和 `goals.rs:684-712` 的 `targeting_test`（已实现潜行 0.8、隐身 `0.7*armor_cover`、
  视线、`range*visibility` 夹到至少 2 格）。**缺的是 `Mob.target` / `setTarget` /
  `lastHurtBy` 记忆 / `HurtByTargetGoal.setAlertOthers` / `TargetingConditions` 里
  `canBeSeenAsEnemy`（和平难度为假）和 `isAlliedTo`**。
  另外玩家不在实体表里（`EntityPublic` 只描述世界实体），敌人「打玩家」必须走新的
  「对玩家的伤害」接口，而不是 `EntityCtx::other`。
- 「村民 / 铁傀儡 / 海龟 / 狼」都还不是 Bamboo 的实体（`MobType::Cow` 一种），
  所以这几条 `NearestAttackableTargetGoal` 现在没有可选目标；建议先只注册
  `Player`（和 `HurtByTarget` 里对本类/同类敌人的反打），村民等按「以后有该实体再启用」
  处理，并在对拍场景里明确「场景内没有村民/铁傀儡」。
- 破门：`BreakDoorGoal` 要读 `mob_griefing`、难度谓词（HARD，`Zombie` 的
  `DOOR_BREAKING_PREDICATE`）、240 刻进度、门方块状态改写与音效；`EvaluatorFlags` 的
  `can_open_doors` 也要按 `canBreakDoors` 打开。`Zombie.finalizeSpawn` 里
  `BREAK_DOOR_CHANCE=0.1` 才给它 `setCanBreakDoors(true)`。

### 1.6 变体：建议「小僵尸与尸壳一起、其余后放」

| 变体 | 官方做法（字节码） | 依赖 | 建议 |
|---|---|---|---|
| 小僵尸 | `Zombie.DATA_BABY_ID`（布尔元数据，26.3 的 `SynchedEntityData.defineId` 走 `ClassTreeIdRegistry` 按类继承树分配 index，`Zombie.defineSynchedData` 另外还有 `DATA_SPECIAL_TYPE_ID`(INT) 和 `DATA_DROWNED_CONVERSION_ID`(BOOL)）；`setBaby` 加 `SPEED_MODIFIER_BABY`（+0.5 乘基础，临时修饰符）；`BABY_DIMENSIONS = scalable(0.49,0.98).withEyeHeight(0.775)`；`getBaseExperienceReward` 幼年 ×2.5；`getSpawnAsBabyOdds` 在 `finalizeSpawn` 抽 | 现有 `age` / `is_baby()` / `dimensions()` / 属性修饰符 / `xpReward` 泛化；`EntityData::Baby` 的 index 要按官方类树核对（Bamboo 现在把 16 写死给 `AgeableMob.DATA_BABY_ID`） | **一起做**：几乎全是已有机制的分支（但牛用 `age<0`、僵尸用 boolean，且元数据 index 不能照抄牛，见风险） |
| 尸壳 Husk | 只是 `isSunSensitive()=false` + 音效 + `doHurtTarget` 附加饥饿效果、尺寸 1.95（和僵尸同） | 需要「效果」系统（饥饿/药水）；`Mob.passthrough` 里有 `active_effects`，但没有运行时效果 | **一起做「不燃烧 + 音效 + 战利品表」**，把「附加饥饿效果」后放（Bamboo 无药水效果）；是否接受这个可见差异要拍板 |
| 溺尸转化 | `ConversionTracker`（`DrownedConversionTime` / `InWaterTime`），`Zombie.tick` 每刻推进，`convertsToWhenDrowning()` → `DROWNED`；`Drowned` 自己有水下寻路、三叉戟、游泳 MoveControl 等一整套新 AI | 新实体类型 + 水中 AI + 装备/物品 | **后放**：单独一期，和「水中生物」一起 |
| 僵尸增援 | `Zombie.hurtServer`：HARD 难度 + `nextFloat < SPAWN_REINFORCEMENTS_CHANCE 属性` + `isSpawningMonsters()` + 50 次尝试（7..40 格），生成同类 `REINFORCEMENT` 实体；`ZOMBIE_LEADER_CHANCE=0.05` 在 `finalizeSpawn` 给领头僵尸加属性 | `DamageSource` 带目标、`EntityType.create(REINFORCEMENT)`、按位置生成实体、难度与 `spawn_monsters` 规则 | **后放**：它会显著增加「生成实体」的命令落地与对拍场景数量；先把受伤/死亡链路做稳 |
| 流浪者 Stray | `Skeleton` 的 `freezingTracker`（细雪 140 刻 / 转化 300 刻）→ STRAY；`Stray.getArrow` 给箭加 `SLOWNESS 600 刻` | 细雪里的冻结（`Mob.ticks_frozen` 已有）、药水效果、箭 | **后放**：和效果系统/箭的药水箭一起 |
| 沼骸 Bogged | `AbstractSkeleton` 子类，覆盖 `createAttributes`（血量）、`getArrow`（毒箭）、`Shearable`；只在沼泽群系的刷怪权重表里出现 | 群系刷怪权重、药水箭、剪切交互 | **后放**：自然生成那一期之后再排 |
| 尸壳/沼骸/流浪者的生成条件 | 按群系/结构选（`BiomeDefaultFeatures` 的 `monsters(...)`、`swampSpawns`） | 自然生成 | 同上 |

「小僵尸」要注意：`AgeableMob` 用 `age<0` 表示幼年、`EntityData::Baby` 同步；
`Zombie` 直接用一个 boolean 元数据，`DATA_SPECIAL_TYPE_ID` 还是 int（僵尸村民类型用）。
26.3 的 `SynchedEntityData.defineId` 改用 `ClassTreeIdRegistry`：每个类从「父类
已用的最后一个 index + 1」开始分配，所以牛的 `CowVariant`/`CowSoundVariant`
（Bamboo 现写死 18/19）和僵尸自己的三个静态 `EntityDataAccessor` 不是同一套编号，
**不能假定僵尸的 `Baby` 也在 index 16**。期 2 开工前需要用一个小 oracle
（或 `javap` + 官方 `getNonDefaultValues()`）导出每种实体的元数据 index 与默认值，
再让 `entity_tracking::default_data` / `KindData::pairing_values` 按 `MobType` 分支。
泛化 `MobPublic` 时也不能把 `age` 当成通用幼年标志。

### 1.7 对拍：现有 `mob-oracle` 可以扩展，但要重构场景协议

现状（`tools/mc-bamboo-vanilla/java/MobOracle.java` 2036 行 +
`src/mob_oracle.rs` 1330 行）：Java 侧用 `FakeLevel extends ServerLevel`（反射注入
注册表/环境属性/玩家表，`getDifficulty()=NORMAL`、`getMaxLocalRawBrightness=15`）、
每头牛一个官方 `ServerEntity` 用 `StreamCodec` 把包编成「ID+包体」，
一行 JSON 一个 `case`，逐刻输出位置/速度/朝向/血量/落地/路径节点/运行中目标/
随机数种子；Rust 侧用 `MobLevel` 假世界重放，浮点按位比较，52 个场景 50640 刻
0 不一致，含改坏确认（`docs/mob-entity-design.md:173-176, 220-228`）。

要扩到敌对生物和玩家受伤，需要改这些绑死牛的地方：

| 位置 | 现在 | 要改成 |
|---|---|---|
| `Case.cows: Vec<CowSetup>` / `steps: Vec<Vec<CowStep>>` | 只有牛 | 场景里多种实体（牛/僵尸/骷髅/箭/玩家），步骤里按实体 ID 或类型分列；`CowStep` 的 `var`/`snd`/`age`/`love` 泛化成可选字段 |
| `CowStep` / `stepJson` / `compare` | 比 `goals`/`path`/`hp`/`nat`/`fire`/`seed` | 增加 `target`（uid）、`hurt_time`/`damage_cooldown`（无敌帧）、`in_ground`/`shake_time`/`pierce`（箭）、`is_baby`/`can_break_doors`/`converting`（僵尸）、玩家 `health`/`hurt_time`/`death_time`/`last_hurt`/`absorption`/`food`/`xp` |
| `MobLevel` 假世界 | 固定 NORMAL 难度、亮度 15、晴天、无 `sky_darken` | 场景参数带 `difficulty` / `sky_darken` / 逐格光照 / 雨 / 雷暴；Java `FakeLevel` 同样覆盖 `getSkyDarken` / `isRainingAt` / `getMaxLocalRawBrightness(pos,darken)` / `environmentAttributes` 的 `MONSTERS_BURN` 与 `NATURAL_MOB_SPAWNS` |
| `hurts: Vec<[i32;4]>` | 只能按「牛下标 + 伤害量 + 来源=玩家0」打生物 | 增加「对玩家造成伤害」的动作（Java 侧 `player.hurtServer(level, source, amount)`，Rust 侧走新的玩家伤害入口），以及箭命中（`AbstractArrow.onHitEntity` 的伤害量与 `DamageType.arrow`） |
| `packets` / `sync_flags` / `spawns` | 每个 `Sync` 只跟踪牛；`spawnJson` 只认 item/orb/cow | 箭/僵尸的配对包（元数据 index、属性包）、`SetHealth`/`DamageEvent`/`HurtAnimation`/`PlayerCombatKill`/`Respawn`/`SetExperience` 包；`spawnJson` 加 arrow 与怪物种类 |
| `MobOracle.java` 的 `play()` | 循环里 `for (Cow cow : new ArrayList<>(cows))` | 按实体 uid 顺序 tick（官方 `ServerLevel` 顺序），玩家也要 tick（`ServerPlayer.doTick`），`StubList` 要能接 `respawn` 或至少记录重建后的状态 |
| `entity_tracking::default_data` / `mob_non_default_data` | 硬编码牛的元数据 index | 按 `MobType` / `EntityKind` 分支（见 1.5/1.6） |

建议**每期都新增一组 `--only` 前缀的场景并保留原有 52 个牛场景不回归**：
第 1 期加 `player_hurt_*`（难度 4 档 × 护甲/附魔/无敌帧/击退/死亡/重生/`keepInventory`）；
第 2 期加 `zombie_*`（追玩家、攻击、燃烧、和平清除、破门、小僵尸）；
第 3 期加 `arrow_*` / `skeleton_*`（箭飞行/命中/插方块/拾取、保持距离射击、躲太阳）。
Java 侧 `Official`/`FakeLevel`/`Sync`/`Bytes` 的骨架可以复用，不必另起一个 oracle；
但「玩家死亡/重生」可能需要官方 `PlayerList` 的可运行占位（`StubList` 现在只够发统计），
这是第 1 期最大的对拍工程量。

---

## 2. 交回的分期（按依赖排序，每期能独立合并、有对拍）

工作量是按「一个熟悉现有生物代码的开发者」估的人日（含单测 + 对拍场景 + 改坏确认），
不含等待主代理/用户拍板的时间。公共前提：`Level::for_each_player` 已可用（见开头）。

### 期 0：规格与公共接缝（0.5–1 人日，可并入期 1）

- 拍板下面第 3 节的取舍；确定 `MobType` 泛化方式、玩家伤害接口的归属
  （建议放在 `mc-bamboo-game`，因为 `Player` 在那里；`mc-bamboo-blocks` 只提供
  「伤害计算」纯函数和 `DamageSource`/`DamageKind`）。
- 把 `DamageKind` 扩到 `Arrow` / `MobAttackNoAggro` / `Starve` / `Thorns` 等，
  `DamageSource` 补 `cause` / `direct` / `weapon` / `scales_with_difficulty` /
  `bypasses_cooldown` 等标签（现成 17 种只有牛用得到的）。
- `BlockView` 补 `difficulty()` / `sky_darken()` / `is_raining_at()` / `can_see_sky()` /
  `max_local_raw_brightness(pos,darken)` / `level_random_*`，并让 `SourceView`
  （并行）与 `World`（精确）给出同一份答案；`GlobalView` 已经发布难度/天气/时间。
- 对拍：不需要新 oracle；`cargo test -p mc-bamboo-blocks -p mc-bamboo-game` 保证
  两种驱动对同一场景读数一致（已有 `entity_view`/parallel 测试骨架）。

### 期 1：玩家生命与死亡（**排在僵尸骷髅之前**，6–9 人日，可拆 1a/1b）

用户补充点名要单独列一期，范围与分期如下：

**1a 受伤与减伤（3–4 人日）**
- `Player` 增 `health` / `absorption` / `hurt_time` / `hurt_duration` / `death_time` /
  `damage_cooldown` / `last_hurt` / `last_damage_source` / `last_hurt_by_*` /
  `invulnerable_time` / `food`（至少 `food_level` / `saturation` / `exhaustion` / `tick_timer`
  的缺省值，否则 `actuallyHurt` 的 `causeFoodExhaustion` 和 `tickRegeneration` 无法对齐）、
  `alive/dead` 派生，`Player::view` 填真实 `alive` / `armor_cover`。
- 玩家 `hurtServer`：与生物共享 `living.rs:1083-1161` 的「无敌帧/冷却/事件/击退」骨架，
  玩家侧另接 `Player.hurtServer` 的难度缩放、`Player.actuallyHurt` 的护甲/附魔/抗性/吸收。
  护甲值从 6 槽装备的 `attribute_modifiers` 组件求和，保护/穿刺附魔从
  `EnchantmentHelper` 对应的效果组件读（精度取舍见第 3 节）。
- 出站包：`SetHealth` / `DamageEvent` / `HurtAnimation`（`EntityData::Health` 已有，
  但玩家自己的血量走 `set_health`，还要 `EntitySync::Metadata` 用真血量）、
  `SetExperience`（已有）、`EntityEvent` 的受伤动画；`EntitySync` 的 `metadata_dirty`。
- `/damage <target> <amount> [type] [at|by]` 命令（官方 `DamageCommand`）作为可注入
  入口，`/kill` 对玩家改走真实死亡流程；`serverbound::ATTACK` 至少能打非玩家实体
  （否则玩家无法反击，但完整 PVP/攻击冷却可后放，见第 3 节）。
- 存档：`PlayerData` 的 `Health`/`HurtTime`/`DeathTime`/`AbsorptionAmount`/`food*`
  与运行时双向接上（`from_runtime_parts` / `restore_inventory`），`oracle player-data`
  加用例。
- 对拍：扩 `mob-oracle` 为 `player_hurt_*` 场景（4 档难度 × 无甲/铁甲/保护 IV/
  连续两次伤害/击退/`/damage`/`/kill`），逐刻比血量、无敌帧、冷却、随机源、
  包字节；`StubList` 先不接重生。

**1b 死亡、掉落与重生（3–5 人日）**
- `Player.die` / `dropAllDeathLoot`：`keepInventory` 两分支、消失诅咒、
  `Inventory.dropAll()` 生成掉落物（已有 `spawn_at_location` / `GroundItem`）、
  `min(level*7,100)` 拆经验球（`orb::award` 已有）、清经验/进度/总经验/`Score`、
  死亡消息（`show_death_messages` + 队伍可见性）、`PlayerCombatKill` 包。
- 死亡界面与重生：`ClientCommandKind::PerformRespawn` → `PlayerList.respawn` 等价物
  （Bamboo 只有一个 `Region`，建议**原地重置 `Player` 的状态而不换对象**，
  保留 `EntitySync.seen_by` / `menus` / `player_grid` / `links`）；
  `immediate_respawn` 规则、`respawn` 已有 `RespawnData`（世界出生点/`spawnpoint`），
  发 `Respawn` / `SetDefaultSpawnPosition` / `ChangeDifficulty` / `SetExperience` /
  装备与属性包；`keepInventory` 时 `transferInventoryXpAndScore` 原样搬
  物品栏/经验/`Score`/`enchantmentSeed`。
- 和平回血与饥饿：`ServerPlayer.tickRegeneration` 的「每 20 刻 +1」、
  `FoodData.tick` 的基础分支（自然恢复/饥饿掉血）至少做到不影响现有创造模式行为。
- 对拍：`player_death_*` / `player_respawn_*` 场景（keepInventory 开/关、
  immediate_respawn、玩家等级经验、消失诅咒、死亡消息包），Java 侧需要让
  `StubList.respawn` 可用或等效地记录重建后的 `ServerPlayer` 字段。

**验收**：`mob-oracle` 原 52 场景 0 不一致 + 新增玩家场景 0 不一致；
`cargo test -p mc-bamboo-game`（含 `BAMBOO_TEST_LOADED=1` 多岛）；
`player-data` oracle；`/kill @s`、`/damage @s`、客户端重生流程在 `Harness`
上逐包对拍；改坏确认至少 5 处（难度缩放、护甲公式、无敌帧、经验奖励、
keepInventory）。

### 期 2：敌对生物地基 + 僵尸（5–7 人日）

- `MobType::{Zombie,Skeleton,...}`、`Mob` 状态按官方类分层（`Monster` 层）、
  `Attributes::zombie/skeleton`、`GoalSelector` 支持两个选择器（`goals` + `target_goals`）、
  `Goal` 新增 `MeleeAttack` / `HurtByTarget` / `NearestAttackableTarget` /
  `RestrictSun` / `FleeSun` / `WaterAvoidingRandomStroll`（牛的 `Stroll` 泛化）、
  `LookAtPlayer` 泛化到 8 格。
- 难度：`BlockView::difficulty` 接 `Region.global.difficulty`；`check_despawn`
  按 `MobCategory` 分类（和平清除、`MONSTER` 128 格 / 600 刻 / 1/800 消失、
  牛仍 `removeWhenFarAway=false`）；`isSpawningMonsters` 读 `spawn_mobs`+`spawn_monsters`。
- 光照/燃烧：`sky_darken` 已按官方 `Level.updateSkyBrightness` 从主世界时钟 +
  `SKY_LIGHT_LEVEL` + 天气算好，`SourceView` 同步；本轮直接读它实现
  `Monster.updateNoActionTime`、
  `BURN_IN_DAYLIGHT` 的 `burnUndead`（头盔耐久/点燃 8 秒）、`getWalkTargetValue`
  改成 `−成本`。`is_raining_at` / `can_see_sky` 也补到 `SourceView`。
- 攻击：`Mob.doHurtTarget`（ATTACK_DAMAGE + 附魔修正 + 额外击退）→
  **对玩家**走期 1 的伤害入口；`lastHurtBy` 记忆、`HurtByTargetGoal.setAlertOthers`、
  `TargetingConditions.canBeSeenAsEnemy`（和平为假）。
- 僵尸：`finalizeSpawn`（随机装备/小僵尸/破门概率/领头加成可先留接口）、
  `isSunSensitive`、`doHurtTarget` 的着火传染、`BreakDoorGoal`（含 `mob_griefing`、
  HARD 难度、`can_open_doors`）、小僵尸（`Baby` 元数据、+0.5 速度修饰符、
  0.49×0.98 尺寸、2.5 倍经验）、尸壳（不燃烧 + 音效 + 战利品表，不接饥饿效果）。
- 生成入口：`/summon minecraft:zombie|skeleton|husk`（NBT）、刷怪蛋
  （`item_use/mod.rs` 加分支）、`nbt.rs` / `archive/entity.rs::MODELED_ENTITIES` /
  `entity_tracking::default_data` / `interaction/level.rs::mob_loot_drops` 泛化；
  网络同步（元数据/属性/事件/战利品表）复用 `TrackedEntity`。
- 对拍：`zombie_*` 场景（平地追玩家近战、墙/台阶寻路、受伤无敌帧、
  白天燃烧/头盔挡住、夜晚不烧、和平清除、128 格消失、破门、小僵尸、
  玩家死亡/掉落经验球、HARD 增援先不做）、逐刻比位置/目标/路径/血量/着火/
  随机数/包字节。

### 期 3：箭实体与骷髅（6–8 人日）

- `EntityKind::Arrow`（`AbstractArrow` 的建模子集：位置/速度/owner/
  `baseDamage` / `crit` / `pierce` / `in_ground` / `shake_time` / `life` /
  `pickup` / `pickup_item` / `last_state`），`physics` 已有 `clip_collider` /
  `sweep_movement`；实体表 `by_type` / 元数据 / 存档 / 同步（箭的 `ID_FLAGS` /
  `PIERCE_LEVEL` / `IN_GROUND`）、`playerTouch` 拾取（`Inventory.add` +
  `take_item_entity` 包）、命中实体（`damageSources().arrow`、`baseDamage × 速度`、
  击退）与命中方块（插住、`lastState` 变化后掉落、按钮/靶可选后放）。
- 骷髅：`RangedBowAttackGoal`（保持距离、`seeTime`、HARD 20 刻 / 其它 40 刻射击间隔）、
  `MeleeAttackGoal` 降级、`performRangedAttack`（`getMobArrow` /
  `spawnProjectileUsingShoot` / 射击音效 / `rangedAttackUncertainty`）、
  `RestrictSunGoal` + `FleeSunGoal` + `AvoidEntityGoal<Wolf>`（狼未实现，后放）、
  战利品表 `minecraft:entities/skeleton`。
- 对拍：`arrow_*`（飞行/重力/命中实体与方块/插住后拾取/消失）、
  `skeleton_*`（8–15 格保持距离射击、近身切近战、躲太阳、不同难度射击间隔），
  逐刻比箭的位置速度/插地状态/玩家血量/包字节。

#### 期 3 现状

- `EntityKind::Arrow` / `KindState::Arrow` / `KindPublic::Arrow` 共用已有实体表，精确驱动在当刻落地伤害，并行驱动仍用已有的意图落地阶段。箭的存档、跟踪、震颤后拾取与两类命中已接入。拾取复用已有的玩家接触阶段；箭与方块重叠时，只有插地标志从假变真才触发震颤回调，首刻不触发震颤。
- 骷髅共用 Monster 基础、目标选择器、移动控制、装备和战利品；无弓走近战。日晒规避和拉弓状态在生物行里，箭的归属 UID 和相对计时在箭实体行里，岛拆合没有增加新的 Part。
- `mob-oracle --only projectile` 包含普通箭命中玩家、插墙与拆墙、飞行时被新方块包住，以及 NORMAL/HARD 骷髅射击。比较逐刻轨迹、箭的随机源、插地与震颤、骷髅拉弓与随机源、玩家状态和相关包字节。`--break-self-test projectile` 同时改变直射箭仰角并卸掉骷髅弓。
- `loot-oracle --entity-table minecraft:entities/skeleton` 单独比较骷髅死亡战利品表的物品和随机流；不覆盖装备槽掉落及经验球。
- 未建模和可能不一致的边界见 README「箭和骷髅的已知限制」。期 4 未做。

### 期 4：自然生成（4–6 人日，单独一项）

- 导出/生成群系 `MobSpawnSettings`（`BiomeDefaultFeatures` 的权重、组大小、
  `spawn_cost`）到 `mc-bamboo-data`；`MobCategory` 上限表；
  `NaturalSpawner.createState` 的每刻计数、全局上限 `max*spawnableChunks/289`、
  `spawn_monsters` / `spawn_mobs` 规则、按玩家上限（**默认开**，开关放全局配置，
  精确模式保留官方全局上限）、候选位置的光照/方块/距离条件、
  `finalizeSpawn` 与组生成（含小僵尸、尸壳/流浪者/沼骸按群系）。
- 接入 `run_island_simulation` 的一个新阶段（官方在 `chunkSource.tickChunks`
  的 `tickSpawningChunk` 里，和随机刻/雷击相邻），打乱刷怪区块列表用岛自己的
  随机源；`docs/multi-region-tick-design.md:58` 已记录官方只打乱这个列表。
- 对拍：新建 `natural-spawn` oracle 或 `mob-oracle` 的 `spawn_*` 场景
  （4 档难度 × 白天/夜晚/雷暴、几个群系固定刷怪表、玩家离 24/32/128 格、
  按玩家上限开/关），比「每刻生成了什么、什么位置、实体随机源状态」。
  注意官方区块列表按哈希顺序打乱，Bamboo 按坐标顺序，这是已有差异，
  对拍要固定/注入顺序或只比集合与计数。

### 期 5（可后放，不属于本轮计划）：变体与增强

溺尸转化 + 水中 AI、僵尸增援（HARD）、流浪者细雪转化 + 迟缓箭、
沼骸毒箭 + 剪切、村民/铁傀儡/海龟/狼目标、玩家完整 PVP 与攻击冷却、
药水效果系统、三叉戟/长矛（`SpearUseGoal`）。**这些都不阻塞期 1–4 的合并**。

### 依赖关系

```
期 0（规格+接缝）
  └→ 期 1a 玩家受伤 ─→ 期 1b 死亡/重生
                          ├→ 期 2 僵尸（需要「打玩家」）
                          │     └→ 期 4 自然生成（需要僵尸/骷髅实体与 finalizeSpawn）
                          └→ 期 3 箭+骷髅（需要玩家受伤 + 期 2 的 Monster 地基）
```
期 2 和期 3 都依赖期 1 的玩家伤害入口；期 3 还依赖期 2 已泛化好的
`MobType` / 双目标选择器 / 同步路径，所以不建议和期 2 并行开工。

---

## 3. 需要拍板的取舍

1. **玩家死亡的建模方式**：官方是「重建 `ServerPlayer`」；Bamboo 建议**原地重置
   `Player` 状态**并保留 `EntitySync.seen_by` / `menus` / `player_grid` / `links`，
   否则重生会重发配对包、丢菜单和玩家网格登记（`IslandPart::Players` 等按玩家分的
   七张表都要同步）。要确认接受这个「实现上的等价变形」，以及是否需要发完整的
   `Respawn` 包序列（默认建议：发，逐包对拍）。
2. **护甲/附魔减伤的精度**：`CombatRules.getDamageAfterAbsorb` 的
   `modifyArmorEffectiveness`（武器上的「破甲」附魔效果）和
   `getDamageAfterMagicAbsorb` 的保护附魔/抗性效果，需要 `EnchantmentHelper` 的
   一大片效果组件。**选项 A**：第一期只做「护甲值/韧性 + 保护附魔等级」的静态求和，
   明确把「武器破甲」「抗性效果」「格挡」「图腾」列为已知差异；
   **选项 B**：一次做全（会额外 2–4 人日并牵动效果系统）。倾向 A，
   对拍场景里用无附魔武器/无效果，把 B 留到效果系统那一轮。
3. **饥饿系统范围**：官方 `actuallyHurt` 要 `causeFoodExhaustion`、
   `ServerPlayer.doTick` 要 `FoodData.tick`、和平回血要 `saturation`。
   是否第一期就做完整的饥饿/自然回血/饥饿掉血，还是只建字段并按官方默认值
   不 tick（对拍场景里不触发）？倾向后者先落地，把饥饿单独列一项。
4. **`serverbound::ATTACK`（左键打实体）**：现在完全没接。是本期补上
   （玩家能打怪、和敌对生物形成闭环，但要做攻击冷却/暴击/附魔/挥手包），
   还是先用 `/damage` + `/kill` + oracle 注入，把左键攻击单列一期？倾向
   「期 1 只做到能对非玩家实体造成一次 `player_attack` 伤害，冷却/暴击后放」，
   否则僵尸骷髅做出来后玩家没有任何反击手段。
5. **Husk 的饥饿效果 / Stray 的迟缓箭 / Bogged 的毒箭**：这些都要药水效果系统。
   是否允许「本体行为一致但附加效果缺失」的可见差异先合并？倾向允许，
   并在 `docs/mob-entity-design.md` 的「未做」清单里逐条写明。
6. **自然刷怪的「按玩家上限」具体口径**：用户已定默认开，但
   `spawnableChunkCount` 是照官方 `getNaturalSpawnChunkCount`（票等级 ≤ 某值）
   还是用 `PlayerGrid::players_within` 按半径算出的区块数？两者在多人多岛时数值不同。
   倾向「精确模式用官方全局上限，非精确模式用 PlayerGrid 的玩家半径上限」
   （和设计稿 4.6 一致），需要确认半径与开关名字。
7. **天色/白天燃烧的触发方式（已定）**：新世界时钟按官方从 0 刻走起来，
   `sky_darken` 由 `GlobalView` 每刻按官方 `Level.updateSkyBrightness` 从
   主世界时钟 + `SKY_LIGHT_LEVEL` 时间线 + 雨 / 雷暴算出，精确模式的 `World`
   和并行 `SourceView` 读同一份值；`clock-oracle` 逐刻与官方类对拍，
   方块 oracle 另有 `--clock-ticks 18000` 的夜晚 / 雷暴场景。
8. **村民/铁傀儡/海龟/狼目标**：这些实体都还没有。是现在就把
   `NearestAttackableTargetGoal` 写成「类型可配置、缺的目标跳过」，还是等对应
   生物实现时再加？倾向前者（目标系统本来就要泛化），但对拍场景里明确
   「无此类实体」，不假装覆盖。

---

## 4. 岛拆分 / 合并时新增状态怎么跟着搬

`crates/mc-bamboo-game/src/region/island_split.rs` 的 `IslandPart` 现在 25 类
（`ALL: [Self; 25]`，`:147-233`），拆岛时按区块或按玩家整批搬
（`:338-533`），合并时 `merge_rebased` 逐类并（`:637-727`），
时钟平移走 `ClockParts::entities → EntityStore::rebase`（`docs/mob-entity-design.md:176`）。

| 新增状态 | 归属 | 拆合处理 |
|---|---|---|
| 玩家血量/无敌帧/冷却/饥饿/死亡状态/死亡界面等待重生 | `Player` 结构体 | **不加新 `IslandPart`**：随 `IslandPart::Players` 整行搬走（`:478-479` 的 `pick(IslandPart::Players)` / `split_players`）。重生若原地重置，不要重建 `Player` 对象；若重建，`Menus`/`PlayerGrid`/`Links`/`PeerIps`/`LastAction` 和 `EntitySync.seen_by` 必须一起保留，否则要新增 Part 或重发配对包 |
| 生物目标 uid / `lastHurtBy` / 仇恨计时 / 破门进度 / 转化计时 / 小僵尸标志 / 箭的 owner uid / 插地状态 | `Mob` / `EntityKind::Arrow` 的实体行（`EntityStore`） | 随 `IslandPart::Entities`（`:423-437`）搬；`SpawnSnapshots` / `DepartedMobEvents` / `EntityTracking` 三个已有 Part 分别管新生成快照、离场事件、配对。按设计稿 4.7，跨实体引用存 uid，查不到当作没有——岛拆开后目标在另一岛时正好走这条降级 |
| 绝对时刻字段（`Mob.last_damage_stamp`、`Navigation.last_timeout_check` / `time_last_recompute`，新增的箭/转化/重生计时若存绝对时刻） | 实体行 / `Player` | 要么统一存**相对岛时钟的倒计时**（推荐，天然免平移），要么登记进 `ClockParts::entities`（`region.rs:188-199`）走 `EntityStore::rebase`；玩家侧若存绝对时刻，需要在 `Region::rebase_clock` 里加一类 `players`（当前 `ClockParts` 没有这个类） |
| 难度 / 天色 / 天气 / 游戏规则（`keepInventory`、`spawn_mobs`、`spawn_monsters`、`mob_griefing`） | 主控 `Global` 的每刻快照 `GlobalView`（`global.rs:637-680`） | **不加新 Part**：权威在 `Global`，拆岛时 `global: self.global.clone()`（`:526`），合并留吸收方。规则变化经主控扇出，不存在「两个岛难度不同」的状态 |
| 按玩家的自然刷怪上限（`SpawnState` 的类别计数、`PotentialCalculator`、`LocalMobCapCalculator`） | 建议**每刻从本岛实体表 + `PlayerGrid` 重算**（官方 `createState` 本来就是每刻重算，`ServerChunkCache.tickChunks:807-817`） | 不持久化就不需要新 `IslandPart`；若为了性能改成增量维护，就必须按区块拆/并（像 `TickLevels` / `BlockTicks` 那样 `split_off(chunks)` / `absorb`），并在 `IslandPart::ALL` 加一类、补 `split_off_island_parts` 的改坏确认测试。设计稿 4.6「按玩家算上限，计数增量维护」暗示后一种，倾向第一版先重算、验证后再优化 |
| 刷怪用的随机源消费（`level.random` 的 `Util.shuffle`、候选位置 `nextInt`） | `Region.random` / `IslandRandom` | 已有 `IslandIdentity` 的四个随机源（`:124-146`）随岛拆合并由 `derive_island_random` / `replace_random` 管理；新增「刷怪专用随机序列」要加进 `IslandIdentity` 并在 `split_off_island_restoring` 里还原，否则「拆开再并回」会重放同一串刷怪随机数。建议先复用 `Region.random`，不新增第五个源 |
| 玩家死亡掉落的物品/经验球 | `GroundItem` / `ExperienceOrb` 实体 | 随 `IslandPart::Entities` 和它所在区块走；`EntityTracking` 的配对跟着实体（`entity_tracking.rs:73-99`） |

一句话：**新增的按实体 / 按玩家状态都落在已有的 `Entities` / `Players` 两类里，
难度与规则是全局快照，唯一可能需要扩 `IslandPart` 清单的是「增量维护的刷怪计数表」
和「玩家侧的绝对时刻」**；本方案建议两者都先用「重算 / 相对倒计时」规避，
从而 25 类清单保持不变，只改 `ClockParts`（若要）和 `IslandIdentity`（若要）。
每期合并前跑 `world/island_split_tests.rs` 的「故意漏掉一类」改坏确认。

---

## 5. 本轮验证方式（如实）

- 只读源码：`grep` / `read_file`，未修改任何已跟踪文件（本文件与
  `.agent-notes.md` 是新增/忽略文件，未 `git add`、未 commit）。
- 官方行为：`javap -c -p` 对 26.3 未混淆 jar 的
  `Monster` / `Zombie` / `AbstractSkeleton` / `Skeleton` / `Husk` / `Stray` / `Bogged` /
  `Drowned` / `Mob` / `LivingEntity` / `Player` / `ServerPlayer` / `PlayerList` /
  `CombatRules` / `AbstractArrow` / `ProjectileUtil` / `RangedBowAttackGoal` /
  `BreakDoorGoal` / `TargetingConditions` / `LevelReader` / `Level` / `ServerLevel` /
  `ServerChunkCache` / `NaturalSpawner` / `NaturalSpawner$SpawnState` / `MobCategory` /
  `BiomeDefaultFeatures` / `EntityType` 等类。没有抄反编译源码进仓库。
- 未运行 `cargo check` / `cargo test` / oracle（没有代码改动）；
  未启动真实 Minecraft 服务端或客户端，未接受 EULA。
