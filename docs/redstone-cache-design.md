# 红石缓存第一版设计（已批准）

> 状态：**已批准**（2026-11-12），实现按本稿进行。对应 `docs/world-model-design.md` §9 和
> `docs/redstone-cache-experiment.md`。本文里所有「官方」结论都用 26.3
> `server-26.3.jar` 的 `javap -c -p` 字节码核对过，出处写在括号里。
>
> 批准时的一处改动：**Q2 改为把段哈希放进 `mc-bamboo-world::Chunk`**，不用常驻
> `Level` 包装。段哈希错一次就是误命中，必须看得见所有写方块的路径；只有
> `Chunk::set_block` 是唯一的写入口。照 `Chunk::randomly_ticking_sections`
> （`docs/performance-optimizations.md` 7d 节）的做法：每段存 128 位哈希、
> `set_block` 增量异或、`from_sections` 整段算一遍、调试构建里断言等于整段重算、
> 只在内存不进存档、量 `set_block` 新增开销。其余 Q1/Q3/Q4/Q5/Q6/Q7 按本稿的倾向。
> Q5 补充：方块实体休眠已合进 main（`8baec27`），漏斗休闲眠是下一轮，本轮不碰
> `hopper.rs`。Q7 补充：总开关默认关，放进 `bamboo.toml` 和 APP 设置页；
> 等差分测试和对拍证明逐位一致、量出收益后再决定要不要默认打开。
> 对拍补充：`oracle` 加 `--redstone-cache` 开关，打开时热点阈值为 0（第一次就录），
> 开与不开都要 10114 / 0 fail，开的那次另报重放命中次数。

## 0. 一句话

按「一次触发」缓存：第一次执行时录下**读了什么、写了什么、取了几个随机数**，
下次同一个触发到来、且守卫确认它这次会读到完全相同的状态时，不再逐方块解释，
直接按记录顺序把效果落地。命中不了就退回解释执行，不留半套效果。缓存只是缓存，
整个丢掉不影响正确性。

沿用实验里已经验证过的边界、键和录制层，不另起一套语义。用户 2026-11-12 的两条
决定都在本文里落地：活塞音高重放时**照样从世界随机源取数**；第一版必须有
**命中不了就停录**和**每 region 内存上限 + 最近使用淘汰**。

## 1. 范围：四种触发、按收益和风险排序的机器

### 1.1 触发的边界（沿用实验，不改定义）

`Level::observe` 上的 `Trigger` / `Observation::TriggerBegin` / `TriggerEnd` 已经是现成的
四种边界（`crates/mc-bamboo-blocks/src/level.rs`，驱动循环在 `interact.rs` 和
`world.rs`）：

| 触发 | 边界位置 | 第一版 |
| --- | --- | --- |
| 到期的方块计划刻 `ScheduledTick` | `interact.rs::run_block_ticks` | **启用**，纯信号电路全在这里 |
| 到期的流体计划刻 `ScheduledFluidTick` | `interact.rs::run_due_ticks` 的流体循环 | **纳入边界**，但无实测收益，按通用规则自然收敛 |
| 一个方块事件 `BlockEvent` | `world.rs::run_block_events` | **启用**，活塞门的伸缩事件在这里 |
| 方块实体 ticker 的一次 tick `BlockEntityTick` | `world.rs::tick_block_entities` | **启用**，只为改完相对量的移动活塞服务；漏斗/熔炉按 §1.2 的规则自然排除 |

一次触发 = 一条边界之间的全部读写，包括它引起的连锁邻居更新、延迟到
`end_held_reads` 才跑的比较器读取。四种都进同一个缓存引擎，不按触发种类写四套代码。

**不新增机器白名单。** 哪些触发能进缓存由 §3 的录制规则 + §4 的热点/停录决定：
读时间/碰实体/取随机数（且不是 §3.2 里可重放的取数动作）的不录（§4.2）；
录了长期不命中的按 §4.3 停录。这样熔炉阵列（进度单调增长）、漏斗链（读时间）
自动出局，不用硬编码类型名。

### 1.2 机器，按「收益 ÷ 风险」排序

数字全部来自 `docs/redstone-cache-experiment.md`（4000 刻、精确键、严格口径）。

| 优先级 | 机器 | 实测 | 风险 | 第一版 |
| --- | --- | --- | --- | --- |
| 1 | 中继器时钟、侦测器时钟、红石线阵列、比较器阵列、互质时钟汇合 | 命中 98.1%–99.8%，可省 96.0%–99.7% 的「算」，3–98 条记录、6 KiB–4.3 MiB，记录数 500 刻内平掉 | **最低**：零随机数、零实体、零时间读，全部是计划刻 | **做**，作为主验收 |
| 2 | 活塞门 | 严格口径 100% 不能缓存；处理完音高取数 + `lastTicked` 后乐观口径 96.2%–96.5%、可省约 47% + 方块事件那 51.5% 里的一部分 | **中**：要重放随机取数、要把绝对时刻改成相对量、要碰方块实体 tick | **做**（用户 11-12 指定），单独一套验收 |
| 3 | 漏斗链、物品分类机 | 严格口径 100% 不能缓存（漏斗每刻读 `game_time()`）；乐观口径 99% 命中但只省 21.7% / 28.1% 的「算」 | 低收益，且 `BlockEntities` 正由方块实体休眠改动 | **不做**，保持不可缓存；等休眠合并后按 §5.2 再评估 |
| 4 | 熔炉阵列 | 严格口径 16000 次可缓存但 **0% 命中**，4000 刻涨到 16000 条 / 约 19 MiB | —— | **明确不做**，它是「停录 + 内存上限」的反例验收用例：缓存内存不得随刻数线性增长 |
| 5 | 流体刻、观察者加活塞收割机、刷怪塔 | 实验没摆 | —— | 第一版不专门优化，也不专门禁止 |

第一版交付时，第 1 类必须有实测收益，第 2 类必须逐位一致，第 4 类必须被内存上限压住。

## 2. 键

### 2.1 用纯段哈希键（实验里的 `pure_section_key`）

```
key = hash128(
    Trigger,                                  // 种类 + 位置 + 方块/流体/方块实体类型
                                              // （BlockEvent 还带 param_a/param_b），不含刻号
    sort( (ChunkSection, section_hash128) ),  // 读集合涉及的每个 16³ 段，整段方块状态哈希
    [ (pos, kind, value_hash64) ... ],        // 所有「不在方块状态里」的读项，按调用顺序
)
```

- 段哈希算法照抄实验 `recording.rs::cell_hash` / `StableHasher`：每格
  `hash(pos,0,state) | hash(pos,1,state)<<64`，整段 4096 格异或，写方块时
  `^= cell_hash(pos, old) ^ cell_hash(pos, new)` 增量更新。跨进程可比，实验数字能对上。
- **段哈希只覆盖方块状态**。红石记忆、方块实体内容、容器槽位、计划刻查询、亮度、
  告示牌、游戏规则、天气、世界边界、高度图、实体接口结果等仍按精确 `(位置,类别,值)`
  进键；读顺序进键（读集合相同但顺序不同说明执行路径不同）。
- 选它的理由（实验「三种键的对比」）：多数机器只比精确键低 0–0.1 个百分点，
  键小 4–330 倍（红石线阵列 9 MB → 27 KB / 条）；代价是互质时钟汇合掉 6.5 个百分点、
  记录数 98 → 427，仍在内存上限内。第一版不做「每台机器一个哈希域」的细化。

### 2.2 键存在哪里、怎么消冲突

缓存引擎（`mc-bamboo-blocks` 新模块，见 §9）里放
`HashMap<u128, SmallVec<[Entry; 1]>>`：128 位键做一次索引，桶内用**完整键原料**
（段坐标 + 段哈希列表 + 全部非状态读项）逐项比对，128 位碰撞不会误命中。
`Entry` 另存可复核的守卫读项（§4.1）和可重放的效果序列（§3.2）。

缓存实例挂在 `mc-bamboo-game` 的 `Region` 上，一个 region 一份，正好对上
「每个 region 的缓存有内存上限」（设计稿 §9.3）。假世界（`tools/mc-bamboo-hit-rate`）也存一份，
差分测试用同一套引擎。

### 2.3 什么时候失效：**不做显式失效**

- 每次重放前守卫重算键（§4.1），任何世界变化都会让段哈希或某个读项不匹配 → 退回解释。
  不匹配**不删记录**（以后可能还命中），由 §4.3 的 LRU + 内存上限回收。
- 全局快照类状态（游戏规则、天气、世界边界、玩家位置/距离）以读项进键，规则一改自然失效。
- 区块卸载、region 拆合、存档读回：按设计稿 §9.2「缓存只是缓存」直接丢掉整份缓存或受影响段的记录；
  第一版可以先只丢整份（正确性不受影响，命中率短暂下降）。
- **段哈希由 `mc-bamboo-world::Chunk` 自己维护**（Q2 已定）：每段一个 128 位哈希，
  `Chunk::set_block` 里 `^= cell_hash(pos, 旧) ^ cell_hash(pos, 新)`，`from_sections`
  整段重算。因为所有写方块的路径（玩家放置、命令、存档读回、区块搬进搬出、方块逻辑）
  最后都过 `Chunk::set_block`，缓存拿到的哈希一定和真实方块状态一致。缓存不另维护
  一份，也不做显式失效。

## 3. 录什么、怎么重放

### 3.1 录制层：复用实验的 `Recording`，新增一套「运行时记录」

实验的 `Record` / `Read` / `Write` / `hit_rates` **保持不动**（`docs/redstone-cache-experiment.md`
的数字和历史结论不能变）。在它旁边新增运行时结构：

```
GuardRead { method: ReadMethod, pos: Option<BlockPos>, value_hash: u64 }
   // ReadMethod 是枚举，覆盖 Level / LevelSideState / LevelEntities / Observation 上
   // 的每一个读入口（约 34 个 K_* 加子标签），守卫按它调同一个底层方法重读、比对哈希。

ReplayEffect（按触发里的先后顺序）：
   BlockWrite      { pos, state, flags, applied }   // 底层写入，不跑邻居更新
   Redstone        { method, pos, value }           // 走 RedstoneMemory 的原始入口
   BlockEvent      { event, existed_before }        // push_block_event，existed_before 进守卫
   BlockEntity     { method, pos, boxed_facts }     // insert / remove / with_hopper 等
   Container       { method, pos, slots_after }     // with_container / with_slots / clear
   Sign            { pos, content }
   TickScheduled   { pos, kind, fluid, delay, priority, existed_before }
   Effect          (Effect)                         // 音效 / levelEvent / 方块事件包等对外效果
   RandomPitchSound{ sound, source, xyz, volume, scale, base, except_initiator }
```

关键的三条：

1. **写方块记最终值，不含中间的邻居更新过程**（实验 9.3 已定）。重放走
   `Level::write_block(pos, state, flags)`：`RegionLevel::write_block` 照常用
   `flags & UPDATE_CLIENTS` 登记广播、`ChunkStore::set_block` 照常登记光照检查，
   但不经过 `World::set_block` 的连锁。这样「套用省不掉写、光照、发包」照做，
   省掉的只有「算」。
2. **所有「去重式写入」把写入前的存在性记成读项**：`BlockTicks::schedule` 在
   `(pos, kind)` 已排着时跳过且**不递增 `next_order`**；`BlockEvents::push` 在
   `queued` 里已有同事件时跳过；`BlockEntities::insert` 在同一格已有 ticker 槽位时复用。
   录制时把这些「之前有没有」记进 `GuardRead`，守卫复核，重放仍调原始入口让去重和
   子序号自然发生（**不绕过**）。这是保证计划刻排序、`sub_tick_order`、ticker 顺序
   和解释执行逐位相同的关键，比实验里「键相同但效果不同为 0」更严。
   实验里 `TickQuery` 的结果已经进读集合，但 schedule 前的存在性没有，要补。
3. **方块实体 ticker 的内部递增 id、驱动临时拿放实体、取事件、开/关 ticker 阶段
   不进键也不进效果**（实验已验证：不排除它们的话周期性活塞门永远不命中）。
   重放 `insert` 时照常调 `BlockEntities::insert`，id 分配顺序因为调用序列相同而相同。

### 3.2 随机数：只把「可重放的取数动作」纳入第一版

官方字节码核对：

- `PistonBaseBlock.triggerEvent` 在分支开头 `getRandom()` 存局部变量，伸出取一次
  `nextFloat()*0.25F+0.6F`、收回取一次 `nextFloat()*0.15F+0.6F` 传给 `playSound`；
  Bamboo `piston.rs:92/128` 的写法（分开乘加，不融合）与之一致。
- `playSound` 之后官方还发 `gameEvent(BLOCK_ACTIVATE/BLOCK_DEACTIVATE)`，Bamboo 目前
  没有 gameEvent 层，不在本次范围。
- 发包阶段 `Effect::Sound { seed: None }` 由 `send_effects` 从
  `sound_seed_generator`（独立于 `Level.random`）取一个 `nextLong` 当包内 seed；
  重放发同样的 `Effect::Sound { seed: None }` 即可，取数次数一致。

用户 11-12 的决定落地为：把「取一个数 → 按公式算音高 → 发 `Effect::Sound`」
记成**一条动作** `RandomPitchSound`，重放时在效果序列的同一位置：

```text
let draw = world_random.next_float();          // 从世界随机源取，不用自己的随机源
let pitch = draw * scale + base;               // 分开乘加，scale/base 存在动作里
world.push_effect(Effect::Sound { …, pitch, seed: None });
```

于是取数的**次数（1）、时机（效果序列里的位置）、公式、后续种子推进**都和解释执行
一样；音高逐位一致（重放和「同样状态下解释执行」面对同一个随机源状态，取到的数相同）。
录制时 `Effect::Sound` 的 pitch / seed 不进效果哈希（实验 `recording.rs:1457` 已经这么做了，
注释写明「音高和 seed 由世界随机源决定，不是状态」）。

怎么让录制层看见这次取数、又不把整类触发打成「不可缓存」，按 §10 Q3 的答复：
给 `Level` 加一个**带默认实现**的 `next_random_float()`（现有实现者零改动），
`World` 加 `play_piston_sound(..., scale, base)` 包装取数、算 pitch、发效果并 `observe`，
活塞两处改调它；`Recording` 覆盖该方法记成 `RandomPitchSound` 而**不标** `Uncacheable::Random`。
其余随机路径（投掷器、发射器、掉落物、战利品表、音符盒 `playSeededSound` 的 seed）
保持现状：`Level::random()` 一被调用就标 `Uncacheable::Random` → 该触发不缓存。
第一版只有活塞音高这一类随机数是可缓存的，不扩大。

### 3.3 重放顺序

严格按 `ReplayEffect` 序列一条条落地，和录制时的调用顺序一致。计划刻用
`World::schedule_tick_with_priority` / `schedule_fluid_tick_with_delay`（delay 是录制时的
相对延迟，和 `BlockTicks::now` 相加后与解释执行相同）；方块事件用
`World::block_event` / `LevelSideState::push_block_event`；对外效果用
`World::push_effect` 收进本次操作的 `Vec<Effect>`，由 `mc-bamboo-game::send_effects`
照常发包。重放**不**触发邻居更新、**不**跑任何方块逻辑，因此不会二次产生计划刻、
二次发音效、二次取随机数。

## 4. 守卫和退回

### 4.1 重放前要确认什么（守卫阶段零副作用）

按记录里的 `GuardRead` 顺序，用同一个 `Level` 只读地重读一遍并比对哈希：
触发本身（这一格现在还是不是记录里的方块/流体/实体类型）、每个段的当前整段哈希、
全部非方块状态读项、计划刻查询（`has_scheduled_tick` / `will_tick_this_tick` 的结果）、
去重式写入前的存在性、`is_unobstructed`、实体接口结果。另外：

- **「范围里没有实体」可以进键**（设计稿 §9.3）：录制时查到是空的记进键，守卫再确认还是空的。
- 守卫**不**改世界、**不**取随机数、**不**排计划刻、**不**发效果。实现成一个只读的
  `Level` 包装（`ReplayGuard`），写方法一律不可用（返回「不允许」而不是 panic，
  符合「客户端发来的任何值都不能触发 panic」）。
- 跨 `World` 实例共享的一次性状态（§5.1 的「本刻 tick 过」表）守卫时按当时的
  `(pos, now)` 重读，不一致就退回，只会降低命中率，不会误命中。
- **跨触发边界的待办状态**：一次触发在 `TriggerEnd` 之后仍可能留着已排队、
  要等下一阶段才跑的东西（方块事件在 `run_block_events` 阶段、计划刻在下一刻、
  方块实体 ticker 在 `pending` 里）。这些是「果」不是「算」，逐位一致由 §3.1 的
  去重存在性读项 + 原始入口重放保证，不额外设守卫。

### 4.2 确认不了就退回解释执行，且不留半套效果

- 守卫**全部通过**才进入落地阶段；落地阶段只做 §3.3 的底层写入/排队/收效果，
  这些操作在守卫通过后的状态上不会再「因为逻辑分支」失败。`BlockWrite.applied=false`
  （越界写入）按记录跳过并保持原样，和解释执行一致。
- 守卫任一项不匹配 → **丢掉这次命中**，从**完全干净的状态**开始正常解释执行
  （`state.tick` / `trigger_event` / `entity.tick` 原样跑），守卫阶段没有任何已应用的副作用，
  所以不存在「半套效果」。
- 命中路径也不会「半套」：落地前先把整份效果序列检查一遍（长度上限、位置合法性、
  记录完整性），任何异常都退回解释，不边写边判。
- 录到 `Uncacheable`（时间/实体/非活塞随机数、或效果序列超过单条上限）的触发
  **不进缓存**，直接解释执行。
- **边界外的读写**：实验的 `Recording` 把两次 `TriggerBegin/End` 之间的读写
  （周期输入翻红石块、补箱子、驱动循环自己的读写）暂存成 `pending_*`，归到下一次触发。
  第一版必须把这段的**写**单独标记：本次触发开始时若 `pending_writes` 非空，
  这次触发不缓存（退回解释），因为那段写可能来自缓存键覆盖不到的外部操作。
  否则「机器输入翻转」和「电路对输入的反应」会被混成一条可重放记录，重放会把
  输入翻转也一起套用。实验的命中率统计不受影响（它们测的是录制，不是重放）。

### 4.3 热点判断、停止录制、内存上限（用户 11-12 指定的第一版必备）

每个触发点按 `(Trigger 的种类, pos)` 归一化（去掉 `BlockEvent` 的
`param_a/param_b` 等变化项）记一个 `Site`。第一版用「累计解释耗时 ≥ 2 ms」当热点，
7i 的实测说明这不够：小电路一次解释只要几微秒，守卫和录制的常驻开销比直接算还贵；
互质时钟汇合三条时钟在同一段里交错写，段哈希频繁失配，没命中时白付了查找、守卫、
录制，再照常算一遍。第二版改成**按触发点自己算账**（2026-11-13）：

| 规则 | 参数 | 目的 |
| --- | --- | --- |
| **确定性工作量** | `work = 实验 Record 的读项数 + 写项数 + 段数` | 不用墙上时间：同一台机器在任何 CPU、任何编译选项下都得到同一个数，单元测试可复现。命中省下的就是这份工作量，守卫（每段一次查哈希、每条守卫项一次重读）和录制包装（每个读写项多记一笔）也折成同一个单位 |
| **太便宜的不录** | 单次 `work < 65536` 的触发点不录；试探阶段用 `RecordMode::Count` 只累加一个计数器，连续 8 次都够不到门槛就把该点判为不划算 | 九台机器里最大的「小电路」触发 25732 项、最小的红石线阵列触发 145105 项，65536 落在两者之间。见 `performance-optimizations.md` 7i 的门槛表 |
| **按净收益留不留** | 每个触发点一个 128 次触发的窗口，账 = 命中省下的 `work` − 白付的守卫 `work` − 白付的录制 `work`；窗口结算为负、且该点已命中过至少 8 次就停掉 | 负收益的触发点不再继续占内存和查表。给 8 次命中的宽限，是避免刚录到第一条记录、还没热起来的点被首窗口误关 |
| **停录与冷却** | 停掉后 200 刻（10 秒）内该点完全不碰缓存，之后重新试探；连续不划算时冷却翻倍到上限 3200 刻 | 沿用第一版规则；试探窗口用完后也走同一条冷却路径 |
| **守卫先做便宜的** | 先逐段比 128 位哈希，失配立刻放弃这条记录，不再逐项重读 `GuardRead` | 段内交错写的机器失配早，省掉旧版最贵的逐项守卫 |
| **不缓存的点常驻开销≈0** | `Recording` 加 `RecordMode::{Off, Count, Full}`，`Off` 时每次转发只是一次 `Cell` 加载；`admits` 查一次表就直达解释执行，不建触发帧、不发守卫读项 | 小电路在开关打开时不再为录制包装付钱 |
| **每个 region 内存上限 + LRU** | 32 MiB / region；单条记录 ≤ 1 MiB；按 `last_used_tick` 最近使用淘汰，达到上限时降到 80% | 兜住「一直录但一直不命中」和 9.4「计数器占内存」两类 |
| **开关** | 总开关默认关；差分测试和 oracle 显式打开 | 上线前默认不影响任何现有行为 |

统计和 LRU 都在缓存引擎内部，不进存档、不进 `BlockEntities`、不进 `director_host.rs`。
`CacheConfig::immediate()` 是最坏情况诊断配置：第一次就录、不因净收益停录，方块对拍
和 7i 的参考表用它；正式配置和差分测试用 `CacheConfig::enabled()`。

## 5. 两个绝对时刻

官方都不把这两个字段存盘，读回来是初值（`PistonMovingBlockEntity.saveAdditional/loadAdditional`
只写 `blockState/facing/progress/extending/source`；`HopperBlockEntity.saveAdditional/loadAdditional`
只写 `TransferCooldown` 和物品），所以改成相对量不会改变存盘行为。

### 5.1 活塞 `lastTicked`：**本代理改**

字节码：`PistonMovingBlockEntity.tick` 开头 `lastTicked = level.getGameTime()`；
`PistonBaseBlock.checkIfExtend` 用 `level.getGameTime() == moving.getLastTicked()`
（Bamboo `piston.rs:212`），这个比较的语义是「这一格本刻已经 tick 过」，
它和 `progress(0)<0.5`、`isHandlingTick()` 用 `||` 短路串在一起。

推荐机制（**不碰 `BlockEntities` 表、不改实体字段语义、不新增时间读**）：

- `PistonMoving` 删掉 `last_ticked: i64` 字段，`tick` 里删掉
  `self.last_ticked = world.game_time();`（这正是实验里活塞门 602 次时间读的来源）。
- 在缓存引擎里放一张 `ticked_entities: HashMap<BlockPos, i64>`（键是位置，值是
  「最后一次 tick 的 gameTime」；查询时 `value == now` 才是真），**跨同一个游戏刻的
  多个 `World` 实例共享**：游戏循环里 `run_due_ticks`（`World::in_world_tick`）、
  `run_block_event_phase`、`run_block_entity_tick`（`World::new`）是三个不同的
  `World`，不能把表放在 `World` 里。每刻开头靠 `now` 换戳自然失效，不显式清空。
  `run_taken_block_entity` 每跑完一个 ticker 就写 `(pos, now)`。
- `piston.rs::check_if_extend` 的 `world.game_time() == entity.last_ticked()` 换成
  对这张表的 `entity_ticked_this_tick(pushed_pos, now)`。这个布尔是**相对量、
  可重复、能进键**（通过 `observe` 记成一条 `GuardRead`），不再含绝对时刻。
  读表不算读世界时间（`now` 取 `BlockTicks::now()`，不经过 `World::game_time()`，
  不触发 `Observation::TimeRead`）。
- 时序等价性（`ServerLevel.tick` 字节码：`blockEvents` 阶段之后才把
  `handlingTick` 置 0，随后才是 `tickBlockEntities`）：计划刻和方块事件阶段
  `handling_tick=true`，官方那个 `||` 已经在时间比较之前短路，中间这一项取什么
  都不影响；真正可能用到它的是方块实体 tick 阶段（`handling_tick=false`），
  那时表里的 `(pos, now)` 恰好只覆盖「本刻、且在 ticker 顺序里排在前面」的实体，
  与 `lastTicked == gameTime` 的语义一致。读档后官方字段是 0，表的初值也是空，
  两者同样没有「本刻 tick 过」的记录。
- **验收**：`javap` 逐项核对三个条件的短路顺序；`oracle java --only piston`、
  `machine java`、活塞门差分测试 0 不一致；`archive` 往返测试保持 `progress` 语义。

备选（不采用，写清楚供比较）：让 `PistonMoving` 存 `ticks_since_last_tick: u8`，
由 `World::tick_block_entities` 在每刻开头广播「新的一刻」推进它。语义一样，
但要给 ticker 驱动加钩子、要能遍历在册的移动活塞，和方块实体休眠的改动面重叠。

### 5.2 漏斗 `tickedGameTime`：**两种方案都列出，倾向方案 H1**

字节码：`HopperBlockEntity.pushItemsTick` 里 `tickedGameTime = level.getGameTime()`；
唯一用途是 `setCooldown` 那一段 `destination.tickedGameTime >= source.tickedGameTime`
（Bamboo `hopper.rs::apply_hopper_cooldown`），语义同样是「目标漏斗本刻是否已 tick 过，
决定要不要少等一刻」；来源不是漏斗时官方传 `null`，结果恒为 `false`。

- **方案 H1（第一版采用）**：**不改漏斗**。漏斗的 `BlockEntityTick` 继续因为读
  `game_time()` 落在「不可缓存」，漏斗链/分类机/熔炉阵列按 §1.2 不在第一版收益范围。
  等方块实体休眠合进 main 之后，再按 §5.1 同款方案把 `tickedGameTime` 换成
  `world.entity_ticked_this_tick(...)` 布尔。优点：零冲突，休眠本身会大幅减少这些
  ticker 的运行次数，缓存对它们的边际收益本就只有两成多。
- **方案 H2（现在就做，不碰 `BlockEntities` 表）**：`HopperEntity` 删掉
  `ticked_game_time: i64`，`apply_hopper_cooldown` 的 `destination.ticked_game_time >= from`
  改成两个 `world.entity_ticked_this_tick(pos)` 的比较（来源不是漏斗 → `false`），
  复用 §5.1 那张跨 `World` 实例共享的 `ticked_entities`。它不碰 `BlockEntities` 的**表实现**，
  但会动 `hopper.rs`、`HopperEntity::ticked_game_time()` 公开方法，以及引用它的
  `recording.rs::entity_time_facts` 和 `archive/tests.rs`；与方块实体休眠子代理可能
  改到同一批文件。语义等价性要用 `machine java` 和漏斗相关 oracle 题验证。

两种方案都不改存档（两个字段官方都不存）。**第一版按 H1**，H2 作为休眠合并后
「要不要让漏斗类机器也进缓存」的备选，等主代理定。

## 6. 怎么证明逐位一致

### 6.1 差分测试（主要手段）

扩展 `tools/mc-bamboo-hit-rate` 的 `Machine::compare`：同一台机器造两份假世界，
一份把 `RedstoneCache` 打开、一份关掉，**用同一个输入序列逐刻跑几千刻**，逐刻比较：

| 比较项 | 怎么取 |
| --- | --- |
| 世界状态 | 现有 `BenchLevel::snapshot`：方块、容器、红石记忆的可见读、排队的计划刻 |
| 效果 / 发出的包 | 每刻 `Outcome.effects` 全量逐项比，含 `Effect::Sound` 的 `pitch.to_bits()`、
  `seed`、坐标位模式、顺序；`send_effects` 对 `seed: None` 取 `sound_seed_generator` 的
  次数也要比（假世界里补一个等价的计数器） |
| 随机数状态 | `LevelRandom` 的 `seed`、`rand_value`、`next_gaussian` 缓存；
  `sound_seed_generator` 的种子 |
| 计划刻队列内部 | `BlockTicks::pending()` / `pending_fluids()` 的完整排序键（含 `sub_tick_order`），
  现有 snapshot 只比到 `(pos, block, trigger_tick, priority)`，要补 |
| 方块事件、方块实体 | `BlockEvents` 队列、`BlockEntities` 的 ticker 顺序和内容 |

- 机器覆盖实验里那**九台**：`machines::all()` 原样用，跑 4000 刻（与实验对齐），
  另加一组 10000 刻长跑确认内存/停录不漂移。
- 断言：**0 刻不一致**。任何一项不等就把机器名、刻号、触发点、两份差异打印出来。
- 额外用例：故意注入守卫失败（在两次触发之间改一格无关方块 / 改游戏规则 /
  塞一个物品实体 / 手工排一条计划刻），确认缓存退回解释且结果仍与关闭缓存一致。
- **命中率与效果一致性回归**：把实验的 `hit_rates` 在打开缓存后重跑一遍，
  「键相同但效果不同」必须仍是 0；命中率不得低于实验数字的 −1 个百分点
  （段哈希键在互质时钟汇合上的 91.6% 也照比）。

### 6.2 方块对拍

`cargo run --release -p mc-bamboo-vanilla -- oracle java --weather clear` 照常全过
（实验基线 10114 cases / 0 fail）；`machine java` 10 runs / 0 mismatches。
另外在**打开缓存**的条件下再跑一遍同一条 oracle 命令：oracle 的假世界默认不带缓存，
所以要么给 oracle 的 `OracleLevel` 显式挂一份缓存再跑，要么把缓存做成
`Region`/假世界可选、并新增一个 `--cache` 开关让 oracle 走带缓存的驱动入口。
前者改动更小，倾向前者（具体见 §9 文件清单）。

### 6.3 官方行为核对清单

`javap -c -p` 已核对：活塞伸出/收回各取一次 `nextFloat` 与两个公式、
`PistonMovingBlockEntity.tick` 的 `lastTicked = getGameTime()` 与存档字段、
`HopperBlockEntity.pushItemsTick` 的 `tickedGameTime = getGameTime()` 与
`saveAdditional` 只写 `TransferCooldown`、`PistonBaseBlock.checkIfExtend` 的
`getGameTime() == getLastTicked()`。实现阶段若再动官方语义，同样先读字节码。

## 7. 怎么量收益

在 `tools/mc-bamboo-hit-rate` 新增一个 `cache_bench` 二进制：对实验里的机器
（至少中继器时钟、侦测器时钟、红石线阵列、比较器阵列、互质时钟汇合、活塞门、
熔炉阵列）分别造两份世界，一份开缓存、一份关，**只计 `run_scheduled_ticks`
（方块逻辑阶段）的墙上时间**，跑 10000 刻 × 若干轮取最小（沿用 `bin/bench.rs`
的取最小方法），输出每刻平均耗时和比值。

- 开缓存那一侧**包含守卫和录制的开销**，是真实成本，不剔除。
- 同时输出命中率、缓存字节数、停录触发点数、LRU 淘汰次数，确认熔炉阵列的
  字节数在停录后不再随刻数增长。
- 与实验「可省耗时」和设计稿 §9.4 对照：实验只量「算」，量不到写、光照、发包；
  这里量的是整个方块逻辑阶段，预期节省比例低于实验数字，差额就是省不掉的那部分。
- 与 `bench` 的钩子开销基准对照，确认常驻包装（若采用 §10 Q2 的方案）没有把
  未命中的路径拖慢超过 2%。
- 正式配置与最坏情况配置各跑一张表：`cache_bench 4000 5` 用 `CacheConfig::enabled()`，
  `cache_bench 4000 5 immediate` 用 `CacheConfig::immediate()`。**验收线是正式配置下
  九台机器没有一台比关着慢 3% 以上**，同时红石线阵列的收益保持在同一量级。

## 8. 风险和边界（写清楚不做什么）

- 缓存只在**世界刻里的四种触发**上生效；玩家右键、命令 `/setblock /fill`、
  随机刻、实体 tick 不进缓存（它们没有 `TriggerBegin/End` 边界）。
- 不做设计稿 §9.6 的网表编译，不做实验性红石线算法（`REDSTONE_EXPERIMENTS`），
  不做容器引用缓存 / 信号查询缓存（那是设计稿 §9.5 更便宜的独立优化）。
- 区块卸载/region 拆合时缓存直接丢，不做迁移；第一版不做持久化。
- 段哈希是 128 位 XOR，理论碰撞由「桶内完整键原料逐项复核」挡住。
- 「键相同但效果不同 0 次」是实验在九台机器上的**必要但不充分**证据；
  §3.1 第 2 条补的去重存在性读项、§6.1 的逐刻差分才是第一版正确性的裁判。

## 9. 要动的文件

**新增**

| 文件 | 内容 |
| --- | --- |
| `crates/mc-bamboo-blocks/src/cache.rs`（或 `cache/` 子模块） | 缓存引擎：键与 `Entry`、`TriggerStats`、热点/停录/LRU/内存上限、`ReplayGuard`、效果落地、`Cached` 这个 `Level` 包装 |
| `docs/redstone-cache-design.md` | 本文 |
| `tools/mc-bamboo-hit-rate/src/bin/cache_diff.rs` | 九台机器的开/关缓存逐刻差分 |
| `tools/mc-bamboo-hit-rate/src/bin/cache_bench.rs` | 收益测量 |

**改动**

| 文件 | 改动 | 是否会和别的子代理冲突 |
| --- | --- | --- |
| `crates/mc-bamboo-blocks/src/recording.rs` | 新增运行时记录类型（`GuardRead` / `ReplayEffect`）和产出路径；**保留**实验用的 `Record`/`Read`/`Write`/`hit_rates` 不动 | 否 |
| `crates/mc-bamboo-blocks/src/level.rs` | `Observation` 加变体（随机取数动作、去重存在性、本刻 tick 查询）；`Level` 加带默认实现的 `next_random_float`（若 §10 Q3 选 A）。`Trigger`/`Effect` 尽量不动 | 否 |
| `crates/mc-bamboo-blocks/src/world.rs` | `World` 在触发边界接缓存钩子；加 `play_piston_sound` 包装、`entity_ticked_this_tick` 查询（`ticked_entities` 表挂在缓存引擎上，跨 `World` 实例共享，不是 `World` 字段） | **可能**：方块实体休眠若也改 `tick_block_entities` 的驱动，需协调「每跑完一个 ticker 记一笔」的位置；不改 `BlockEntities` 表本身 |
| `crates/mc-bamboo-blocks/src/interact.rs` | 新增带 cache 的驱动入口；现有 `run_due_ticks` / `run_block_event_phase` / `run_block_entity_tick` / `run_scheduled_ticks` **原样转发到 cache 为空的路径**，所有现调用点（含 oracle）零改动 | 否 |
| `crates/mc-bamboo-blocks/src/behaviour/machines/piston_entity.rs` | 删 `last_ticked` 字段和 `tick` 里的时间读；`observed_facts` 去掉时刻项 | 否 |
| `crates/mc-bamboo-blocks/src/behaviour/machines/piston.rs` | 两处改调 `play_piston_sound`；`check_if_extend` 换成本刻 tick 查询 | 否 |
| `crates/mc-bamboo-blocks/src/hopper.rs` | **第一版不改**（方案 H1）。若主代理选 H2，才动 `ticked_game_time` 与 `apply_hopper_cooldown` | **是**：方块实体休眠正在改 `mc-bamboo-blocks::BlockEntities`，可能连带 `hopper.rs` |
| `crates/mc-bamboo-game/src/region.rs` | `Region` 挂一份 `RedstoneCache`（含 32 MiB 上限），`block_level()` 把它交给驱动入口 | 小：只加一个字段 |
| `crates/mc-bamboo-game/src/interaction.rs` | `run_block_ticks` / `run_block_events` / `run_block_entity_ticks` 改调带 cache 的入口 | 否 |
| `crates/mc-bamboo-world/src/chunk.rs` | 每段存 128 位段哈希，`set_block` 增量异或、`from_sections` 整段重算、调试断言、随机改方块测试、量 `set_block` 开销 | 否 |
| `crates/mc-bamboo-blocks/src/cache.rs` | 新增：`Cached` 包装只负责转发读写与录制/守卫，不再维护段哈希（Q2 已改） | 否 |
| `tools/mc-bamboo-hit-rate/src/world.rs` | `Machine` 加可选 `RedstoneCache` 和 `step_with_cache`；`snapshot` 补 `sub_tick_order`、方块事件队列、随机源完整状态 | 否 |
| `tools/mc-bamboo-vanilla/src/oracle/replay.rs` | 给 `OracleLevel` 挂一份缓存，提供开缓存跑 oracle 的入口 | 否 |
| `docs/world-model-design.md` §9.3 | 补「已实现」状态和指向本文的链接 | 否 |

**明确不碰**：`crates/mc-bamboo-game/src/director_host.rs`、`world.rs` 的存档部分、
`crates/mc-bamboo-blocks/src/archive/*`、`BlockEntities` 表的存储实现
（`block_entity.rs` 里的表结构）、`crates/mc-bamboo/src/main.rs`、
`docs/redstone-cache-experiment.md`（历史实验，数字不改）。

**避不开的写清楚**：`world.rs`（触发边界、`play_piston_sound`、本刻 tick 查询）和
`interaction.rs`（驱动入口）是方块逻辑的必经之路，缓存必须接进去；`region.rs` 是
「每个 region 一份缓存」的落点。这三处都只加字段/入口/方法，不改现有行为。

## 10. 主代理的拍板结果

| # | 问题 | 倾向 |
| --- | --- | --- |
| Q1（已定） | 缓存引擎放 `mc-bamboo-blocks`，实例由 `mc-bamboo-game::Region` 持有，通过**新增带 cache 的驱动入口**接入（现有入口零改动转发） | 同意 |
| Q2（已定，改） | 段哈希怎么维护 | **放进 `mc-bamboo-world::Chunk`**：每段存 128 位哈希，`Chunk::set_block` 增量异或、`from_sections` 整段算、调试断言整段重算、只在内存不进存档、量新增开销。不用常驻 `Level` 包装（它看不见游戏层直接写 `ChunkStore` 的路径：玩家放置、命令、存档读回、区块搬进搬出），也没有那次虚调用 |
| Q3（已定） | 随机取数怎么录 | **方案 A**：`Level` 加带默认实现的 `next_random_float` + `World::play_piston_sound` 包装 + 活塞两处改调用；录成「取一个数 → 算音高 → 发音效」，重放照样从世界随机源取 |
| Q4（已定） | `lastTicked` 相对化 | 跨 `World` 实例共享的 `ticked_entities` 表 + 布尔比较（§5.1）。验收：差分测试 + 方块对拍活塞用例全过 + `machine` 对拍里有活塞的机器全过 |
| Q5（已定） | 漏斗 | **H1**：第一版不缓存漏斗、不碰 `hopper.rs`。方块实体休眠已合进 main（`8baec27`），漏斗休闲眠是下一轮，到时一起改 `tickedGameTime` |
| Q6（已定） | 热点指标 | 以「触发点累计解释耗时 ≥ 2 ms」为主、频率兜底；本代理改 `docs/world-model-design.md` §9.3 的口径，段末注明日期与实验依据 |
| Q7（已定） | 参数 | 按本行初值；总开关默认关、放进 `bamboo.toml` 和 APP 设置页，与实体并行线程数放一起。等逐位一致和收益实测后再决定要不要默认打开 |

## 11. 批准后的实施顺序

1. 段哈希进 `Chunk`，加测试，单独一个提交；
2. 引擎、录制、守卫、重放，先只接计划刻和方块事件；
3. 差分测试：实验里九台机器，开、关缓存各跑几千刻，逐刻比较世界状态、效果、包、随机数状态；
4. 接方块实体 tick（活塞）、随机取数、`lastTicked`；
5. 热点判断、停止录制、内存上限、LRU，各自的测试；
6. 配置项和 APP 设置；
7. 量收益（照 §7），写进 `docs/performance-optimizations.md`。

每一步都单独提交，每一步差分测试都要过。

## 12. 第一版实现结果（2026-11-12）

按 §11 的顺序做完了第 1、2、3、5、6、7 步，**第 4 步（把活塞门接进缓存）
只做完了前置工作，活塞方块事件和方块实体 tick 仍退回解释执行**。逐项：

| 步骤 | 状态 | 验收 |
| --- | --- | --- |
| 1 段哈希进 `Chunk` | 完成 | 每段 128 位哈希、`set_block` 增量异或、读盘整段重算、debug 断言、随机改 4000 次测试；`+6.4 ns/写`，见 `performance-optimizations.md` 7h |
| 2 引擎、录制、守卫、重放 | 完成 | 计划刻和方块事件；白名单集中在 `cache::supported_kind` / `supported_write`；去重存在性进守卫；`Observation::Capture` 关掉守卫/重放的记账 |
| 3 九台机器差分 | 完成 | `cache_diff 4000`：九台全部 **0 刻不一致**（世界状态、效果、发包取数、随机源种子、计划刻队列）；实验的命中率数字和「键同效果不同 0 次」不变 |
| 4 活塞门接进缓存 | **部分** | `lastTicked` 已改成相对量（跨 `World` 实例的 `BlockTicks::ticked_entities`）；活塞音高已改成 `next_random_float` + `Observation::PistonPitch` 的可重放动作；但 `Write::BlockEntity` / `Write::Container` / `Write::Effect` 在实验录制层里只存哈希，还原不出 `PistonMoving`、槽位表和音效参数，所以活塞触发仍按白名单退回解释 |
| 5 热点/停录/LRU | 完成 | 5 个单元测试；热点以累计解释耗时为主、频率兜底；停录还要求该触发点的命中率低；32 MiB/region、按最近使用淘汰 |
| 6 配置项 | 完成 | `bamboo.toml` 的 `redstone_cache`（默认关）、`ServerConfig`、`WorldConfig`、APP 设置页；游戏 tick 的两段接 `run_*_with_cache` |
| 7 收益 | 完成 | `cache_bench`：红石线阵列 **36.7×**，小型电路和互质汇合变慢，见 `performance-optimizations.md` 7i |

**第 4 步剩下的具体工作**（下一轮）：给运行时录制层加带真实值的
`ReplayEffect::BlockEntityInsert(PistonMoving 的全部字段)`、
`ContainerSlots(Vec<ItemStack>)`、`Sound { pitch 由取数动作算 }`，并给
`K_BLOCK_ENTITY` / `K_SLOTS` / `K_LOOT_UNPACK` / `K_TICKER` / `K_ENTITY_TIME` /
`K_WORLD_BORDER` 各自写守卫重算（不能只用通用的 `Recheck`，实测过它会把
纯信号机器的命中率从 99% 打到 40%，因为读项的值形状对不上）。
然后扩 `cache::supported_kind` / `supported_write`，把 `Trigger::BlockEntityTick`
接进 `cache_run::cacheable_trigger`，再用 `cache_diff` 和活塞 oracle 题验收。
这一段不能靠放宽白名单糊过去：守卫漏一个读就是误命中，就是和官方不一致。

**已验证**：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets`
0 警告、`cargo test --workspace` 全过、
`oracle java --weather clear` 与 `oracle java --weather clear --redstone-cache`
都是 10114 cases / 0 fail（开的那次 2364 triggers / 20 replays）、
`machine java` 10 runs / 0 mismatches、九台机器差分 0 刻不一致。

## 13. 第二版：只在划算的地方生效（2026-11-13）

§12 的第 7 步暴露了「开着缓存反而更慢」的机器，见 `performance-optimizations.md` 7i。
按 §4.3 的新规则改造了缓存引擎和录制包装：

| 改动 | 落点 |
| --- | --- |
| 确定性工作量、门槛、窗口净收益账、冷却、三态门控 | `cache::RedstoneCache`（`admits` / `begin_probe` / `note_probe_work` / `try_replay` / `record`） |
| 守卫先比段哈希，失配立刻放弃 | `cache::RedstoneCache::try_replay` |
| `RecordMode::{Off, Count, Full}`，`Off`/`Count` 只碰 `Cell` | `level::Observation::RecordMode`、`recording::Recording`、`world::World` |
| 不划算的点直达解释执行，不建触发帧 | `cache_run::run_cached_trigger`、`interact.rs` 的带缓存驱动入口 |
| 正式配置与最坏情况配置都能量 | `cache_bench` / `cache_diff` 的第三个/第二个参数 `immediate` |

**验收**：正式配置下九台机器开/关对比最慢 0.99×（比较器阵列，慢 0.8%），
红石线阵列 39.75×，见 `performance-optimizations.md` 7i；
`cache_diff 4000`（正式与 `immediate` 两种配置）九台全部 0 刻不一致；
`oracle java --weather clear` 开、关都是 10114 / 0 fail；
单元测试覆盖「便宜的触发点不被录」「净收益为负会停」「冷却后会重试」「段哈希
失配不再逐项重读」，全部只用确定性计数，不依赖机器快慢。

## 14. 第三版：活塞门接进缓存（2026-11-13）

按 §13 末尾的四步计划做完了第 12 节剩下的第 4 步。

| 改动 | 落点 |
| --- | --- |
| 带真实值的 `ReplayEffect`：`PistonMoving` 全部字段、`RestorePistonMoving`、`RemoveBlockEntity`、`ContainerSlots(Vec<ItemStack>)`、`ClearContainer`、`ForgetBlockEntityMemory`、`UnpackLootTable`、`NoteBlockEntityTick`、`PistonSound` | `cache::ReplayEffect`；`World::replay_*` 一律走 `insert` / `remove` / `with_slots` / `clear_container` / `unpack_loot_table` / `push_effect` 原始入口 |
| 活塞音高重放照样从世界随机源取一个 `next_random_float()`，按 `scale`/`base` 算 pitch | `World::replay_piston_sound`（设计稿 §3.2） |
| `ReadMethod` 给 `K_BLOCK_ENTITY` / `K_ENTITY_TIME` / `K_SLOTS` / `K_LOOT_UNPACK` / `K_TICKER` / `K_WORLD_BORDER` / `K_REDSTONE` 子标签各加重算 | `cache_run::guard_value`；`Level::loot_table_pending` 是新增的只读查询（游戏层覆盖） |
| 守卫数量核对改成 `guarded_reads` 逐项计数，漏一个读就整条作废 | `recording::RuntimeFrame` / `finish_record` |
| ticker 驱动的临时拿放、insert 侧的实体内容是「果」不是「因」，不进运行时守卫 | `recording::read_entity_driver`（实验 `Record` 仍照旧记两笔，历史数字不变） |
| 方块实体 tick 接进缓存驱动，游戏循环该段也走 `run_block_entity_tick_with_cache` | `World::tick_block_entities_cached`、`interact.rs`、`mc-bamboo-game::interaction` |
| 重放还原 tick 后实体状态但保留 ticker 编号和顺序 | `BlockEntities::replace_in_place`、`Observation::PistonTickState` |
| 没有 tick 后实体状态可还原的 ticker（熔炉、漏斗、活塞 `finalTick`）不进缓存 | `recording::finish_record` 的 `piston_tick_replayable` 检查 |

**关键的两处正确性细节**：守卫阶段实体还在表里，驱动先抄下触发前的实体可观察状态
（`TakenEntityFacts`）再取出，避免把「解释执行后的状态」误当成输入；重放不跑方块逻辑，
所以要按录下的 tick 后状态把 `PistonMoving` 原地写回，并补上 `BlockTicks` 那笔
「本刻 tick 过」的记录。`Write::Effect` 只有伴随 `Observation::PistonPitch` 的
`seed: None` 音效才放行，别的对外效果一律作废记录。

**验收**：`cache_diff 4000`（正式与 `immediate`）九台全部 **0 刻不一致**；
`immediate` 下活塞门 702 次触发、19 条记录、48 次重放（其余 200 次是移除自身
实体的 `finalTick` 或没有可还原实体状态的 ticker，已明确退回解释，见下），
`cache_diff` 现在会断言活塞门在 `immediate` 下确实重放过（防止白名单悄悄退回解释）；
`oracle java --only piston --weather clear` 开、关都是 10028 cases / 0 fail
（开的那次 10009 triggers / 125 replays）；
`oracle java --weather clear` 开、关都是 **10114 / 0 fail**；
`machine java` 10 runs / 0 mismatches；`cargo test --workspace` 全过、
`cargo fmt --all --check`、`cargo clippy --workspace --all-targets` 0 警告。
正式配置下九台机器仍没有一台比关着慢 3% 以上（重测最慢 0.97×，见 7i 的验收线），
红石线阵列 33–40×（不同负载下）。

**过程中修掉的两个坑**（都不是靠放宽白名单）：
1. 给带缓存的 `run_scheduled_ticks_with_cache` 多调了一次 `tick_crafting_clocks`，
   合成器 9 道 oracle 题会挂；已删掉，和原路径逐行对齐。
2. 守卫漏掉 `K_REDSTONE` 的十来个子标签时纯信号机器看不出问题（它们不读这些项），
   扩白名单前逐个补齐；带副作用（take/clear/torch）的子标签守卫阶段一律返回 `None`，
   让那种记录退回解释，而不是猜一个值。
