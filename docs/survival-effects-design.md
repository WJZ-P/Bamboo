# 生存细节与药水效果：设计与分期

状态：**待主代理审（2026-11-20）**。审过之前不写实现代码。
依据：官方 26.3 `server-26.3.jar` 的 `javap -c -p` 字节码、`docs/mob-entity-design.md`
§6–8（玩家生命期 0/1a/1b/1c 已做）、仓库现有代码。不抄反编译源码，只列方法名和结论。

## 0. 范围与总原则

三块：A 伤害来源、B 吃东西与饥饿消耗、C 药水效果系统。
不做：喷溅 / 滞留药水、药箭（等弹射物）；信标、潮涌核心；怪物自带效果（等那些怪）。

- 官方 `LivingEntity.activeEffects` 是 `Maps.newHashMap()`（见 `LivingEntity.<init>`
  字节码），键 `Holder$Reference` 没有覆写 `hashCode` / `equals`，迭代顺序取决于
  对象身份哈希，**官方自己没有固定顺序**。Bamboo 固定按效果 ID 升序（`BTreeMap`），
  逐刻状态、存档列表和 `removeAllEffects` 的多包顺序都按这个确定顺序；对拍两侧
  比的是排序后的全字段，不放宽比较。
- 效果放在**通用生物层** `mc-bamboo-blocks::entity::mob`，`Mob` 和 `Player`
  共用同一份 `EffectMap` / `EffectInstance` / 每刻推进的纯函数；游戏层
  `mc-bamboo-game` 只负责把结果写回玩家状态、发包、命令、存档和物品使用流程。
  这样牛也能挂效果，`mob-oracle` 能用官方牛逐刻对拍同一份内核。
- 所有新增按玩家 / 按实体状态**只存相对倒计时**：效果 `duration`、氧气 `air_supply`、
  着火 `remaining_fire_ticks`、`ticks_frozen`、正在使用物品的 `use_item_remaining`、
  消耗来源的位移累计都不是岛时钟绝对时刻。整行随 `IslandPart::Players` /
  实体行搬走，**不改 `region/island_split.rs`，不新增 `IslandPart`**。
- 伤害唯一入口仍是 `player_hurt::hurt_player(region, id, DamageSource, amount,
  &mut Option<JavaRandom>) -> bool`；环境来源只构造 `DamageSource::environment(kind)`
  再调它，不复制减伤 / 死亡 / 发包链路。
- 客户端能传来的任何值（物品组件补丁、命令参数、使用时长、效果 ID / 时长 / 等级）
  都先经范围检查或返回 `None` / 报错，不允许 panic。

## 1. 三块的官方入口、Bamboo 落点

### A. 伤害来源

26.3 的 `minecraft:damage_type` 注册表有 50 项（`generated/registries.rs` 首行）。
`DamageKind` 现建 22 种，环境伤害里缺的是 `campfire`、`stalagmite`、
`falling_block/anvil/stalactite`、`lightning_bolt`、`wither`、`dragon_breath`、
`dry_out`、`outside_border`、`fly_into_wall`、`mace_smash`、`fireball`、`fireworks`、
`sonic_boom`、`spear`、`spit`、`thrown`、`trident`、`wind_charge`、`unattributed_fireball`、
`player_explosion`、`mob_projectile`、`indirect_magic`、`bad_respawn_point`、`ender_pearl`
这些要弹射物 / 别的实体的来源，**本轮不建模**；本轮按官方 `DamageSources` 只补玩家能
真正碰到的环境种类（`DamageKind` 已有的 `InWall/Cramming/Drown/OnFire/InFire/Lava/
HotFloor/Cactus/SweetBerryBush/Fall/Freeze/OutOfWorld/Starve` 全部接上触发点）。

| 官方入口（`javap -c -p` 核对） | 结论 | Bamboo 落点 |
|---|---|---|
| `Entity.baseTick` / `Entity.lavaHurt` / `Entity.clearFire` / `igniteForTicks` / `getFireImmuneTicks` | `remainingFireTicks>0` 且 `%20==0` 且不在岩浆里 → `onFire` 1.0；`lavaHurt` → `lava` 4.0；雨中灭火；服务器侧每刻把没在火里的值设成 `-getFireImmuneTicks()` | `player_environment.rs`（新），`Player` 加 `remaining_fire_ticks: i32`；存档 `Fire`，元数据位 0 的着火标志 |
| `Entity.checkInsideBlocks` + `InsideBlockEffectApplier.StepBasedCollector.applyAndClear` | `fire` / `lava` / `cactus` / `sweet_berry_bush` / `magma_block` / `powder_snow` / `bubble_column` 等由方块自己登记效果；`magma` 只在不潜行且落地时 `hotFloor` 1.0；`cactus` 1.0；`sweet_berry_bush` 1.0（移动时 0.5s 减速另算）；`powder_snow` 累积 `ticks_frozen`，每 40 刻 `freeze` 1.0（简单 2 / 普通 1 / 困难 1，和平不掉血） | 同文件，复用 `player_move.rs` 现有的碰撞盒 / 方块查询和 `mc_bamboo_blocks::block_state` 的形状 / 标签查询；牛那边的对应逻辑在 `entity/mob/motion.rs`，两边共用一份纯判定函数 |
| `LivingEntity.baseTick` / `decreaseAirSupply` / `increaseAirSupply` / `shouldTakeDrowningDamage` | 眼睛在水里且不是气泡柱：`air = decreaseAirSupply(air)`（`OXYGEN_BONUS>0` 时按 `nextDouble() >= 1/(bonus+1)` 概率不减，取实体随机数）；`air <= -20` 时置 0、`entity_event` 67、`drown` 2.0；否则 `air = min(air+4, 300)`。创造 / 无敌能力跳过 | 同文件，`Player` 加 `air_supply: i32`（缺省 300）、`ticks_frozen: i32`；存档 `Air` / `TicksFrozen`；随机源用 `Player.random` |
| `LivingEntity.baseTick` 的 `isInWall` / 世界边界 / `y < minY-64` | 卡在方块里 `inWall` 1.0（每刻，实际受无敌帧节流）；`y < -128` → `fellOutOfWorld` 4.0（每刻） | 同文件；`player_move.rs` 已有 `enclosed_in_air` / 碰撞盒查询可复用 |
| `Entity.causeFallDamage` → `LivingEntity.causeFallDamage` → `calculateFallDamage` / `calculateFallPower` | `floor((fallDistance+1e-6 - SAFE_FALL_DISTANCE) * damage_multiplier * damageMultiplier)`；`>0` 才 `fall` 伤害并播放摔落音效 / 方块音效；`fallDistance` 在 `record_fall` 里累计、落地清零；水 / 梯子 / 蜘蛛网 / 药水「缓降」要单独处理 | `player_move.rs` 的落地分支接 `player_environment::on_land`；`safe_fall_distance` / `fall_damage_multiplier` 从效果属性修饰读（S1 的属性表）；`DamageKind::Fall` 已在 `DamageKind` 里 |
| `LivingEntity.hurtServer` 的 `isInvulnerableTo` / `FIRE_RESISTANCE`、 `getDamageAfterMagicAbsorb` 的 `RESISTANCE` | 抗火挡 `#is_fire`（`bypasses_effects` 的伤害例外）；抗性 `amount *= max(0, 25 - 5*(amplifier+1))/25`，`#bypasses_resistance` 的伤害例外 | `player_health.rs` / `mob/damage.rs` 的 `ArmorValues::reduce` 增加 `resistance_amplifier: Option<i32>` 参数，调用方从效果表读；牛那边 `living.rs::hurt_server` 同步接一份 |

官方伤害类型标签（`generated/tags.rs` 的 `minecraft:damage_type` 组）已有
`bypasses_armor` / `bypasses_effects` / `bypasses_resistance` / `is_fire` /
`bypasses_invulnerability` / `no_knockback` 等；`DamageKind` 现在是硬编码 match，
S3 顺手改成读生成表的 ID 集合，避免 26.3 的标签和手写清单漂移（会带单测改坏确认）。

### B. 吃东西与饥饿消耗

`mc-bamboo-protocol::components::ComponentValue` **已经有** `Food { nutrition,
saturation, can_always_eat }`、`Consumable { consume_seconds, animation, sound,
has_consume_particles, on_consume_effects: Vec<ConsumeEffect> }`、`UseRemainder`、
`PotionContents`、`SuspiciousStewEffects`、`UseCooldown`，线上和存档 NBT 双向编解码都齐，
`defaults::resolve(item, patch, kind)` 能给物品默认组件。**不需要新建组件类型**。

| 官方入口 | 结论 | Bamboo 落点 |
|---|---|---|
| `Player.startUsingItem` / `LivingEntity.updateUsingItem` / `Player.aiStep` / `Player.completeUsingItem` / `ServerPlayer.completeUsingItem` / `ItemStack.finishUsingItem` / `Consumable.startConsuming` / `canConsume` / `consumeTicks` | 右键可消耗物品时记 `useItem` / `useItemRemaining`；每刻 `--`，到 0 才 `completeUsingItem`；`consumeTicks = (int)(consumeSeconds*20)`（缺省 1.6s = 32 刻）；`canAlwaysEat=false` 时满饥饿且非创造不能开始；切物品 / 受伤 / 换手 / 潜行条件变化要 `releaseUsingItem` | `Player` 加 `use_item: Option<UsingItem { hand, item, remaining, total }>`（相对倒计时）；`player_consume.rs`（新）接 `interaction::on_use_item` 的 `use_item` 包、`PlayerAction::ReleaseUseItem`、`player_hurt` 的中断、`player_hurt::tick` 的每刻推进 |
| `Consumable.shouldEmitParticlesAndSounds` / `emitParticlesAndSounds` / `onConsume` | 每 4 刻在 `remaining <= total - (int)(total*0.21875)` 之后发吃 / 喝音效与粒子，取实体随机数；完成时 `FoodData.eat(nutrition, saturation)`、`use_remainder`（碗 / 桶 / 玻璃瓶）、`on_consume_effects`、减 1 个物品、`game_event` EAT/DRINK | 同文件，音效走 `player_hurt.rs` 已有的 `Sound` 发包路径；粒子 `level_particles` 逐项对拍字节；`FoodData.eat` 加进 `player_health.rs`（`food = min(food+n, 20)`，`saturation = min(sat + n*s*2, food)`） |
| `consume_effects/ApplyStatusEffectsConsumeEffect` / `ClearAllStatusEffectsConsumeEffect` / `RemoveStatusEffectsConsumeEffect` / `PlaySoundConsumeEffect` / `TeleportRandomlyConsumeEffect` | 按组件列表顺序应用；本轮做前 3 种 + 音效，随机传送（紫颂果）不做，明确记为已知差异 | `player_consume.rs` 调 S2 的效果施加 / 清除接口 |
| `ServerPlayer.checkMovementStatistics` / `jumpFromGround` / `Player.attack` / `Player.causeFoodExhaustion` | 游泳 / 水下 / 水中：水平或三维位移 `cm=round(dist*100)`，`exhaustion += 0.01*cm*0.01`；地面疾跑 `0.1*cm*0.01`、普通 / 潜行 `0.0`；跳跃疾跑 0.2 / 否则 0.05；攻击命中实体 0.1；`Abilities.invulnerable` 时全跳过 | `player_exhaustion.rs`（新）从 `player_move.rs` / `interaction::on_interact_entity` / 攻击路径取每刻位移和标志，统一调 `player_health::FoodData::add_exhaustion`（上限 40，创造跳过）；挖掘消耗按 `ServerPlayerGameMode` 的破坏成功路径补 0.005（字节码待 S5 核对） |
| `FoodData.eat` / `FoodData.tick` | `tick` 已经在 `player_health.rs`；`eat` 只在这里补 | `player_health.rs` |

### C. 药水效果系统

| 官方入口 | 结论 | Bamboo 落点 |
|---|---|---|
| `MobEffectInstance` 构造 / `update` / `tickServer` / `tickDownDuration` / `downgradeToHiddenEffect` / `CODEC` | 字段 `effect`(注册表 ID)、`duration`(`-1` = 无限)、`amplifier` 夹到 0..255、`ambient`、`visible`、`showIcon`、`hiddenEffect`（叠加时把旧的低等级效果藏起来）；`tickServer` 先 `shouldApplyEffectTickThisTick(duration, amplifier)`、`applyEffectTick` 返回 false 就移除、再减时长、必要时降到隐藏效果 | `mc-bamboo-blocks/src/entity/mob/effects.rs`（新）：`EffectInstance`、`EffectMap`（按效果 ID 升序的 `BTreeMap`，顺序确定）、`add/update/remove/remove_all/tick` 全是纯函数；`Mob` 加 `effects: EffectMap`、`Player` 也加一个同类型字段 |
| `LivingEntity.tickEffects` / `onEffectAdded` / `onEffectUpdated` / `onEffectsRemoved` / `refreshDirtyAttributes` / `updateInvisibilityStatus` / `updateSynchronizedMobEffectParticles` / `areAllEffectsAmbient` | 每刻遍历 `activeEffects`：`tickServer` 返回 false 就移除并 `onEffectsRemoved`；`duration % 600 == 0` 时补一次 `onEffectUpdated`（发包）；效果增删要刷新属性修饰、隐身标志、实体数据里的粒子颜色列表和 ambient 布尔 | 同文件 `tick` 返回 `EffectEvents { added, updated, removed, attributes_changed, particles, ambient }`，游戏层按事件发包 / 同步元数据；牛的 `living::base_tick` 在 `tickEffects` 位置调用同一个函数 |
| `MobEffects` 静态表 / `MobEffect.addAttributeModifiers` / `createModifiers` / `AttributeTemplate.create` | speed `MOVEMENT_SPEED +0.2*(amp+1) ADD_MULTIPLIED_TOTAL`、slowness `-0.15`、haste / mining_fatigue `ATTACK_SPEED ±0.1`、strength `ATTACK_DAMAGE +3*(amp+1) ADD_VALUE`、weakness `-4`、jump_boost `JUMP_STRENGTH +0.1`、health_boost `MAX_HEALTH +4`、luck / unluck `LUCK ±1`、absorption 无修饰（只加吸收血量）、slow_falling / water_breathing / fire_resistance / resistance 无修饰但被别处读 | `effects.rs` 的 `effect_attribute_modifiers(id, amplifier) -> &[(attr_id, amount, Operation)]`，走 `mob/attributes.rs::Attribute::value` 的三步求值；玩家侧把同样的修饰算进 `PlayerHealth::hurt` 用的属性读数（抗性、抗火、最大血量、吸收上限、氧气加成、安全摔落距离） |
| `RegenerationMobEffect`(50>>amp)、`PoisonMobEffect`(25>>amp，`hp>1` 才 `magic` 1.0)、`WitherMobEffect`(40>>amp，`wither` 1.0)、`HungerMobEffect`(每刻 `0.005*(amp+1)` 消耗)、`SaturationMobEffect`(`FoodData.eat(amp+1,1.0)`)、`HealOrHarmMobEffect`(`4<<amp` 治疗 / `6<<amp` magic 伤害)、`AbsorptionMobEffect`(`onEffectStarted` 设 `4*(amp+1)` 吸收、`applyEffectTick` 只在吸收>0 时继续)、`InstantaneousMobEffect`(`duration>=1`) | 每刻持续效果的动作写成 `EffectTickAction` 枚举（治疗 / magic 伤害 / 加饥饿消耗 / 吃东西 / 无），由游戏层或 `living.rs` 施加；这样牛和玩家共用一份公式，伤害仍走各自 `hurt` 入口 | `effects.rs` 产出 `Vec<EffectTickAction>`；`player_effects.rs`（新）把动作写进玩家状态；`living.rs` 把动作写进牛 |
| `ServerPlayer.onEffectAdded/onEffectUpdated/onEffectsRemoved` / `PlayerList.sendActivePlayerEffects/sendActiveEffects` / `ServerEntity.sendPairingData` / `ServerChunkCache.sendToTrackingPlayersAndSelf` | 玩家自己的 `update_mob_effect`（新增时 `blend=true`，更新时 `false`）和 `remove_mob_effect` 只发给自己的连接；跟踪者由 `sendToTrackingPlayersAndSelf` 广播；进服 / 重生序列第 7 步按效果表顺序补发所有 `update_mob_effect`（`blend=false`）；非玩家实体的效果包走 `ServerEntity.sendPairingData` | `mc-bamboo-protocol/src/packet/play/entity.rs` 加 `UpdateMobEffect` / `RemoveMobEffect`（ID 135 / 79 已在 `generated/packets.rs`；线上格式 VarInt 实体 ID + 效果 Holder(VarInt) + VarInt amplifier + VarInt duration + byte flags(ambient 1 / visible 2 / showIcon 4 / blend 8)，移除包只有 ID + Holder）；`player_effects.rs` 发包、`player_respawn.rs` 的第 7 步接上、`entity_sync.rs` 加元数据里的效果粒子颜色 / ambient |
| `LivingEntity.addAdditionalSaveData` / `readAdditionalSaveData` 的 `active_effects`；`Player.addAdditionalSaveData` / `readAdditionalSaveData` | 玩家和实体都存 `active_effects`：每项 `id` / `duration` / `amplifier` / `ambient` / `show_particles` / `show_icon` / `hidden_effect`；`PlayerData` 已有 `preserved` 顶层键兜底 | `mc-bamboo-storage/src/player_data.rs` 加建模字段 `active_effects: Vec<EffectData>`；`player_save.rs::runtime_player_data` / `restore_inventory` 双向；`mob/nbt.rs` 把 `active_effects` 从 `PASSTHROUGH` 移出、按官方 codec 读写；`player-data` oracle 加非默认往返用例 |
| `EffectCommands.register/giveEffect/computeDurationInTicks/clearEffects/clearEffect` | `effect give <targets> <effect> [seconds 1..1000000 \\| infinite] [amplifier 0..255] [hideParticles bool]`：秒 `*20`，`-1` 无限，缺省 600 刻（瞬间效果缺省 1）、默认 `hideParticles=false`；`effect clear [targets] [effect]`；只对 `LivingEntity` 生效，反馈用 `CommandResponseTracker` 的单 / 多目标翻译键 | `command/effect.rs`（新）+ `command/catalog.rs` 注册 + `command/class.rs::SCOPES` 按现有规则归类（`effect` 改本岛玩家/实体状态，和 `/damage` 一样算 `World`，但仍走 `IslandFn`）；对非玩家实体走 `World::add_mob_effect` / `clear_mob_effects` |
| `PotionContents` / `Consumables` / `Foods` / `suspicious_stew_effects` / `use_remainder` 的物品默认组件；`PotionItem` / `MilkBucketItem` / `HoneyBottleItem` / `GoldenAppleItem` / `EnchantedGoldenAppleItem` / `SuspiciousStewItem` 的 `finishUsingItem` | 普通药水：`potion_contents` 的自定义 / 预设效果列表，瞬间效果用 `applyInstantaneousEffect`（按距离缩放的喷溅不做）；牛奶：`ClearAllStatusEffectsConsumeEffect`；金苹果：吸收 + 再生，附魔金苹果：伤害吸收 IV / 再生 II / 抗性 / 抗火；谜之炖菜：`suspicious_stew_effects` 的时长按基础 × 8（普通）之类，按组件里的 `duration_seconds` 算 | `player_consume.rs` 读 `defaults::resolve` 的 `Consumable.on_consume_effects` + 物品 ID 分支；`ConsumeEffect` 的枚举值按 `mc-bamboo-protocol` 已有的定义展开；原版物品默认组件表由 `generate` 导出，不手写常量 |

## 2. 对拍场景清单（扩 `MobOracle.java` 的玩家场景）

沿用 `player_hurt_*` / `player_death_*` 的做法：Java 侧在假世界里对官方
`ServerPlayer` 直接调入口（反射设字段 / 调 `hurtServer` / `addEffect` / `startUsingItem` /
`finishUsingItem` / `aiStep` 等），逐刻输出 `playerStepJson` 扩展字段和
`RecordingListener` 抓到的包字节；Rust 侧在 `mc-bamboo-game` 的 `oracle-samples`
下加 `replay_*` 函数，走正式的 `Region` / `Player` / `player_*` 模块重放，
浮点和包字节按原始位比较，不留容差。原有全部场景保持 0 不一致。

| 子期 | 新增场景（前缀） | 逐刻比的状态 | 包字节 |
|---|---|---|---|
| S1/S2 | `effect_give_*`（含时长边界 1 / 600 / 无限、等级 0/1/255、隐藏粒子、叠加升级 / 降级 / 短时长、到期、600 刻同步）、`effect_clear_*`（单个 / 全部 / 不存在）、`effect_tick_regen*` / `poison*` / `wither*` / `hunger*` / `saturation*` / `instant_health*` / `instant_damage*` / `absorption*`、`effect_resistance_*`、`effect_fire_resistance_*`、`effect_mob_cow_*`（牛挂速度 / 再生，比属性和血量） | 效果表全字段（ID / duration / amplifier / ambient / visible / showIcon / hiddenEffect）、血量 / 吸收 / 饥饿四字段 / 属性读数 / 随机源种子 / `dead` | `update_mob_effect`、`remove_mob_effect`、`set_health`、`set_entity_data`（粒子 / ambient）、`damage_event`、`hurt_animation`、`update_attributes`、`entity_event` |
| S3 | `env_fall_*`（3 / 3.5 / 4 / 23 格、水、干草块、缓降）、`env_drown_*`（气泡柱、氧气回满、抗水下呼吸、`OXYGEN_BONUS` 概率边界）、`env_fire_*`（岩浆、`on_fire` 20 刻、火焰方块、抗火、雨灭火）、`env_block_*`（仙人掌 / 甜浆果 / 岩浆块 / 卡墙 / 细雪 40 刻冻伤按难度）、`env_void_*`（`y<-128`）、`env_fall_death_*`（摔落死亡消息） | `fall_distance`、`air_supply`、`remaining_fire_ticks`、`ticks_frozen`、血量 / 冷却 / 饥饿 / 随机源 / 位置 | `damage_event`、`hurt_animation`、`set_health`、`entity_event`(67)、音效、`set_entity_data` 着火位 |
| S4 | `consume_food_*`（面包 32 刻、金苹果、附魔金苹果、谜之炖菜、牛奶、药水、蜂蜜瓶、碗 / 桶 / 瓶剩余物、满饥饿 `canAlwaysEat`、中途松手 / 换物品 / 受伤打断、创造模式）、`consume_particles_*`（每 4 刻音效和粒子字节） | `use_item_remaining` / 手、四字段、效果表、物品栏、随机源 | `sound`、`level_particles`、`container_set_slot`、`update_mob_effect`、`remove_mob_effect`、`game_event`、`animate` |
| S5 | `exhaust_walk_*` / `sprint_*` / `jump_*` / `swim_*` / `attack_*` / `mine_*`（各含创造模式跳过、位移 0、四舍五入边界） | 饥饿四字段 / `tick_timer` / 随机源 | `set_health` |
| S6 | 若 S4 已覆盖物品效果来源，S6 只补 `effect_command_*`（命令反馈、多目标、非生物目标、非法参数）和存读档往返 | 同 S2 | 同上 + `system_chat` 反馈 |

**每个子期至少 3 处故意改坏确认**（沿用 `--break-self-test <tag>` 的机制，改的是
Bamboo 重放侧的局部常量 / 分支，不改官方一侧、不删用例、不放宽比较），例如：
效果时长少减 1、抗性倍率改错、隐藏效果丢失；摔落伤害少算 1、`air+4` 改成 `+5`、
抗火没挡 `lava`；`consumeTicks` 少 1 刻、`FoodData.eat` 饱和倍率改错、剩余物没给碗；
疾跑消耗系数改成走路、跳跃消耗漏掉、挖掘没加；命令秒数漏乘 20、存档 `duration` 丢位。
每处都要打印「改坏后 N 处不一致」并记录。

`player-data` oracle 新增 `active_effects` 的非默认往返用例（含 `hidden_effect` 嵌套、
无限时长、ambient / showIcon 组合），和官方 `Player.readAdditionalSaveData` codec 对读；
`chunk-content` 的实体存档也补牛带效果的一例。

## 3. 分期与提交顺序

| 子期 | 内容 | 提交（英文一句话标题） | 依赖 |
|---|---|---|---|
| S1 | 生物层效果内核：`effects.rs`（`EffectInstance` / `EffectMap` / 属性修饰 / 每刻动作 / 叠加 / 到期）、`Mob.effects`、`living::base_tick` 接 `tickEffects`、牛的效果施加入口、牛存档 `active_effects` | `Add the shared mob effect core` | 无 |
| S2 | 玩家效果落地：`Player.effects`、`player_effects.rs`（每刻推进、动作施加、发包、重生补发、进服补发）、协议两个包、实体数据粒子 / ambient、`/effect give\\|clear`、玩家存档 `active_effects`、减伤接抗性与抗火 | `Wire player effects, packets, command and save data` | S1 |
| S3 | 伤害来源：新 `player_environment.rs` + `Player` 的氧气 / 着火 / 冻结 / 空气字段、存档、摔落 / 溺水 / 火 / 方块 / 细雪 / 虚空 / 卡墙触发、`DamageKind` 改用生成标签 | `Apply environment damage sources to players` | S2（抗火 / 抗性 / 缓降） |
| S4 | 吃东西基础：`Player.use_item`、`player_consume.rs`（右键开始 / 每刻 / 完成 / 中断 / 音效粒子 / `FoodData.eat` / 剩余物 / `consume_effects`）、`ReleaseUseItem`、牛奶 | `Implement food and drink consumption` | S1（效果动作）、S2（发包） |
| S5 | 饥饿消耗来源：`player_exhaustion.rs`，移动 / 疾跑 / 跳跃 / 游泳 / 攻击 / 挖掘 | `Add the remaining hunger exhaustion sources` | 无（可插在 S3 前，按工期调） |
| S6 | 物品效果来源：药水 / 金苹果 / 附魔金苹果 / 谜之炖菜的具体组件展开，收尾补差异文档 | `Apply item-based status effects on consumption` | S4 |

每做完一个子期交回一次报告（改了什么、命令和结果、改坏确认三处、疑点）。
最后一次交回跑全量验收：`cargo fmt --check`、`cargo clippy --workspace --all-targets`
0 警告、`cargo test -p mc-bamboo-game`（缺省 + `BAMBOO_TEST_LOADED=1`）、
`cargo test -p mc-bamboo-blocks -p mc-bamboo-server`、`mob-oracle` 全量、
`player-data`、探针两种配置各一次（端口 ≥ 27000）。中间子期只跑相关项 +
`mob-oracle --only <新前缀>` + 原有 `player_hurt` / `player_death` / `player_regen` 前缀，
确认原有 0 不一致，节省全量编译时间。

## 4. 与并行分支的协调（写在报告里）

- `zombie` 分支可能在通用 `LivingEntity` 层加最小的「着火」状态。本方案着火字段和接口：
  - `Mob` 沿用**已有**的 `Entity.remaining_fire_ticks`（`entity/mod.rs::Entity` 上的
    `remaining_fire_ticks`，`mob/nbt.rs` 已存 `Fire`，`living::base_tick` 已有
    `%20==0 → onFire 1.0`）；不新增字段、不新增接口。
  - `Player` 新增 `remaining_fire_ticks: i32`，存档 `PlayerData.fire`（已有），
    对外接口 `player_environment::{ignite_for_ticks, clear_fire, remaining_fire_ticks,
    is_on_fire, tick_environment}`；元数据位 0 的着火标志由 `entity_sync` 读它。
  - 合并时若 `zombie` 往 `Mob` 加了同名字段，删掉重复、保留 `Entity` 上那份即可；
    两边都调 `Entity.baseTick` 语义，不冲突。
- `day-night` 分支改天色：本方案不读 `sky_darken`，不碰时钟。
- `island-borrow` 改命令范围：`/effect` 只按 `command/class.rs::SCOPES` 现有规则
  加一行归类，不改分类逻辑。默认建议 `World`（和 `/damage` 一样可选实体目标），
  但执行仍是 `IslandFn`，只改本岛玩家 / 实体。

## 5. 已知取舍（先记下，实现时逐条对照）

- 属性修饰只做 `MobEffects` 静态表里有修饰的效果，和玩家受伤 / 挖掘 / 移动里真正
  读得到的属性（速度、攻击伤害、攻击速度、跳跃、最大血量、吸收上限、氧气加成、
  安全摔落距离、摔落倍率、幸运）。`scale` / `gravity` / 隐身 / 发光 / 漂浮 /
  缓降的物理副作用本轮只做「效果存在 → 属性 / 伤害 / 元数据」这条线，移动物理的
  完整改写留下一轮，文档里列成已知差异。
- 喷溅 / 滞留 / 药箭 / 信标 / 潮涌 / 怪附带效果不做；`TeleportRandomlyConsumeEffect`
  （紫颂果）不做；`onMobHurt` / `onMobRemoved` 等带额外实体效果（侵袭、渗浆、
  盘丝、风爆）不做。
- 效果增删发给跟踪者的广播按官方 `sendToTrackingPlayersAndSelf` 的收件人集合实现；
  假世界只比玩家自己那份连接字节，跟踪者那份由 `entity_sync` 的既有路径覆盖，
  在报告里明说。
- 客户端传来的物品组件补丁格式非法时按 `defaults::resolve` 已有行为返回 `None`，
  不 panic；命令参数越界由命令图解析器拒绝。
