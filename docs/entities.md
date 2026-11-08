# M2 规格：玩家互相可见（实体同步）

非玩家实体（下落方块、点燃的 TNT）不在这份规格里。它们归世界线程所有，用 ID 引用，同步走 `add_entity`、位置增量和 `remove_entities`。玩家配对、元数据和移动包仍以本文为准。

Non-player entities (falling blocks and primed TNT) are outside this spec. The world thread owns them and refers to them by id. Sync uses `add_entity`, position deltas and `remove_entities`. Player pairing, metadata and movement packets still follow this document.

目标：Minecraft Java 26.3，协议 777。本文覆盖 Tab 列表、实体追踪、移动同步、元数据、挥手、潜行冲刺和手持装备。移动包解析、区块发送、方块交互和聊天在别的规格里，这里只写它们和本领域相接的部分。

证据标记：

- `[跑]`：用官方 `server-26.3.jar` 的类实际执行过，字节或行为是真实输出。harness 是开发时的临时程序，没有收进仓库。它们只调用编解码器和 `ServerEntity.sendChanges`，没有启动服务端或客户端。复核时另写了一个临时程序：把本文每种包的例子字节喂给官方解码器，全部解码成功且没有剩余字节（第 7.3 节的 trace 字节来自官方编码器，没有重复解码）；用各 200 万组随机值比对本文的 `pack_degrees`、`java_round` 和 LpVec3 算法，0 差异。
- `[读]`：读 26.3 反编译源码得到。所有客户端行为都属于这一类。
- `UNVERIFIED`：第 15 节逐条列出。

包 ID 全部来自 `target/vanilla/generated/reports/packets.json`，`mc_bamboo_data::packets::play` 里已有同名常量。

## 0. 和旧资料不同的地方

| # | 26.3 的事实 | 后果 |
|---|---|---|
| 1 | **没有 serverbound `swing`**。左键发 `punch`（46，空包体，没有 hand）。右键放方块时客户端只在本地挥手，除 `use_item_on` 外没有挥手相关的包 | 放方块成功后，服务端要自己给观察者发 `swing_animation`（第 9 节） |
| 2 | 挥手不走 `animate`。`animate` 的 action 0 是 `WAKE_UP` | 挥手用 `swing_animation`（123） |
| 3 | `move_entity_pos` / `move_entity_pos_rot` 在 ID 后多一个 VarInt `properties`，`onGround` 折叠进它的 bit0 | 第 7 节 |
| 4 | `move_entity_rot` 的 `onGround` 在两个角度**之前** | 第 7 节 |
| 5 | `entity_position_sync` 没有速度字段，位置是 `PositionPath`（带类型前缀） | 第 7 节 |
| 6 | `add_entity` 和 `set_entity_motion` 的速度是 LpVec3，不是 3 个 Short | 第 1.3 节 |
| 7 | 玩家元数据：15 = 主手（`HUMANOID_ARM`，序列化器 42），16 = 皮肤部件，17 = 吸收值，18 = 分数，19/20 = 肩上鹦鹉（`OPTIONAL_UNSIGNED_INT`） | 第 8 节 |
| 8 | `player_command` 没有潜行 action，枚举只有 0..6。潜行走 `player_input` 的 0x20 位。`interact` 的 `usingSecondaryAction` 也写同一个标志（`handleInteract`），M2 不处理 `interact` | 第 10 节 |
| 9 | `player_info_update` 的 bit6 是 `UPDATE_LIST_ORDER`，bit7 是 `UPDATE_HAT`，线上按 bit 从低到高写 | 第 3 节 |
| 10 | `teleport_entity` **不更新**客户端的 delta 基准（只有 `add_entity`、`move_entity_*`、`entity_position_sync` 更新） | 玩家永远不用它，大位移用 `entity_position_sync` |
| 11 | `set_equipment` 的槽位是枚举 ordinal，不是 `EquipmentSlot.id`。副手是 1 不是 5 | 第 11 节 |
| 12 | 客户端从不在本地设置皮肤部件和主手，只靠服务端回发 index 15、16 | 元数据必须也发给玩家自己 |
| 13 | 不要发给自己 `add_entity`：`ClientLevel.addEntity` 会先按 ID 移除旧实体，也就是把 LocalPlayer 删掉 | 配对时跳过自己 |
| 14 | `player_command` 的 `START_FALL_FLYING` 不能只忽略：客户端先在本地进入滑翔，这个状态只有服务端能清 | 回一个 `set_entity_data`，第 10 节 |

## 1. 公共编码

| 名称 | 线上格式 |
|---|---|
| VarInt / VarLong | 7 位一组，低位在前 |
| Byte / UByte | 1 字节 |
| Short / Int / Long | 大端补码，2 / 4 / 8 字节 |
| Float / Double | IEEE 754 大端，4 / 8 字节 |
| Bool | 1 字节，0 或 1 |
| UUID | 16 字节：高 64 位 Long，再低 64 位 Long（`FriendlyByteBuf.writeUUID`） |
| String(n) | VarInt 字节长度 + UTF-8，n 是字符数上限（`Utf8String`） |
| List | VarInt 个数 + 元素（`ByteBufCodecs.list`） |
| Component | NBT：1 字节 tag 类型 + 内容，没有根名字（`ComponentSerialization.TRUSTED_STREAM_CODEC`）。已有的 `write_text_component` 就是这个格式 |

帧格式和 M1 相同：`VarInt 长度 + VarInt 包ID + 包体`。ID 大于 127 时占两字节：`teleport_entity` 128 = `80 01`，`update_attributes` 134 = `86 01`。

### 1.1 Angle（Byte）

`Mth.packDegrees`：`(byte)floor(angle * 256.0f / 360.0f)`，全程 f32。解码 `byte * 360 / 256`，按有符号字节算。

```rust
fn pack_degrees(angle: f32) -> u8 {
    (angle * 256.0 / 360.0).floor() as i32 as u8
}
```

`[跑]` 0 → `00`，45 → `20`，90 → `40`，-90 → `c0`，180 → `80`，-180 → `80`，179.9 → `7f`，359.9 → `ff`，-0.1 → `ff`，1.4 → `00`，1.41 → `01`。向下取整，不是四舍五入。

### 1.2 定点 delta

`VecDeltaCodec`：`E(v) = Math.round(v * 4096.0)`，每轴 `delta = E(new) - E(base)`。注意是两个绝对坐标分别取整后相减，不是 `E(new - base)`。这样连续 delta 不会累积误差：客户端算 `(E(base) + delta) / 4096`。

- 精度 1/4096 格。
- 范围：每轴 i16，也就是 `[-32768, 32767]`，约 ±8 格。任一轴越界就不能用 delta，要发 `entity_position_sync`。

Java 的 `Math.round` 是 `.5` 向正无穷取整，Rust 的 `f64::round` 是 `.5` 远离零，负数结果不同（-2.5 → Java -2，Rust -3）。客户端用 `Math.round` 重算基准，所以这里要对齐：

```rust
fn java_round(v: f64) -> i64 {
    let floor = v.floor();
    (if v - floor >= 0.5 { floor + 1.0 } else { floor }) as i64
}
fn fixed(v: f64) -> i64 { java_round(v * 4096.0) }
```

例子 `[跑]`：基准 (8.5, 65.0, -3.25) 移到 (8.75, 65.0, -3.375)：x 34816 → 35840，delta +1024 = `04 00`；y 不变 `00 00`；z -13312 → -13824，delta -512 = `fe 00`。

### 1.3 LpVec3（低精度向量）

来源 `net.minecraft.network.LpVec3.write`，`Vec3.LP_STREAM_CODEC`。用在 `add_entity.movement` 和 `set_entity_motion`。

48 位缓冲区 `buffer`：

| 位 | 含义 |
|---|---|
| 0–1 | scale 的低 2 位 |
| 2 | 续位标记：scale 放不进 2 位时为 1，此时后面跟一个 VarInt |
| 3–17 | x，15 位 |
| 18–32 | y，15 位 |
| 33–47 | z，15 位 |

线上顺序是混合字节序：`buffer` 的第 0 字节、第 1 字节，然后 `(buffer >> 16)` 按 **Int 大端**写 4 字节，最后可选 VarInt `scale >> 2`。

编码步骤：

1. 每轴 `sanitize`：NaN 变 0，再钳到 ±1.7179869183e10。
2. `m = max(|x|, |y|, |z|)`。`m < 3.051944088384301e-5` 时只写一个字节 `00`，结束。
3. `scale = ceil(m)`（i64）。`partial = (scale & 3) != scale`。`markers = partial ? (scale & 3) | 4 : scale`。
4. `pack(v) = Math.round((v * 0.5 + 0.5) * 32766.0)`，v 是 `轴值 / scale`。这里的值非负，`f64::round` 结果相同。
5. `buffer = markers | pack(x/scale) << 3 | pack(y/scale) << 18 | pack(z/scale) << 33`。
6. 写 `buffer as u8`、`(buffer >> 8) as u8`、`((buffer >> 16) as i32).to_be_bytes()`。`partial` 时再写 VarInt `(scale >> 2) as i32`。

解码第一字节为 0 表示零向量。`markers` 不可能为 0，因为 `scale >= 1`，`scale & 3 == 0` 时必有续位。

手算 (0.5, 0, -0.25)：`scale = 1`，`markers = 1`；x = round(24574.5) = 24575 = `0x5FFF`；y = 16383 = `0x3FFF`；z = round(12287.25) = 12287 = `0x2FFF`。`buffer = 0x5FFE_FFFE_FFF9`，写出 `f9 ff` 和 `5f fe ff fe`。

`[跑]` 官方编码结果：

| 向量 | 字节 |
|---|---|
| (0, 0, 0) | `00` |
| (1e-5, 0, 0) | `00` |
| (0.5, 0, -0.25) | `f9 ff 5f fe ff fe` |
| (1.5, -2.25, 3.0) | `fb ff ff fc 40 02` |
| (4, 0, 0) | `f4 ff 7f fe ff ff 01` |
| (5, 0, 0) | `f5 ff 7f fe ff ff 01` |

### 1.4 ItemStack（`ItemStack.OPTIONAL_STREAM_CODEC`）

| 字段 | 编码 | 说明 |
|---|---|---|
| count | VarInt | 0 表示空，后面什么都没有 |
| item | VarInt | `minecraft:item` 的注册表 ID，不加 1（`ByteBufCodecs.holderRegistry`） |
| added | VarInt | 增加的组件数，M2 可以写 0 |
| removed | VarInt | 移除的组件数，M2 可以写 0 |
| 组件 | added 个 `VarInt 类型 + 值`，再 removed 个 `VarInt 类型` | |

服务端收到的 `set_creative_mode_slot` 用的是 `OPTIONAL_UNTRUSTED_STREAM_CODEC`：`added` 部分每个值前面多一个 VarInt 字节长度，`removed` 部分不变，其他相同（`DataComponentPatch.DELIMITED_STREAM_CODEC`）。

`[跑]` 组件补丁（`max_stack_size` 类型 ID 1，设为 5；`max_damage` 类型 ID 2，移除）：

| 形式 | 字节 | 含义 |
|---|---|---|
| 普通 | `01 01 01 05 02` | 增 1、减 1、类型 1、值 5、移除类型 2 |
| untrusted | `01 01 01 01 05 02` | 增 1、减 1、类型 1、**长度 1**、值 5、移除类型 2 |

### 1.5 Bundle

`ClientboundBundlePacket` 不是一个包，线上是：`bundle_delimiter`、内部的包、`bundle_delimiter`（`BundlerInfo.unbundlePacket`）。

- `bundle_delimiter`：ID 0，clientbound，空包体。帧是 `01 00`。
- 客户端收到第一个分隔符后缓存之后的包，收到第二个时把它们放进同一个主线程任务依次处理（`PacketBundlePacker`、`ClientPacketListener.handleBundlePacket`）。
- 内部最多 4096 个包，不能有 terminal 包。
- 漏掉第二个分隔符，后面的包全被吞掉。两个分隔符和内容要一次写入同一条出站队列。

Bundle 只保证这些包在客户端主线程的同一个任务里连续处理，不影响每个包的解码。

## 2. 玩家实体常量

| 项 | 值 | 来源 |
|---|---|---|
| `minecraft:entity_type` 里的 `minecraft:player` | **159**（VarInt `9f 01`） | `registries.json` |
| clientTrackingRange | 32 区块，即 512 格 | `EntityTypes.PLAYER` `[跑]` |
| updateInterval | 2 tick | 同上 `[跑]` |
| trackDeltas | false | 同上 `[跑]` |
| 站立尺寸 | 0.6 × 1.8，眼高 1.62 | `Avatar` |
| 潜行尺寸 | 0.6 × 1.5，眼高 1.27 | `Avatar.POSES` |

`mc-bamboo-data` 现在只生成同步注册表，没有 `entity_type`、`item`、`attribute` 的 ID，需要补。

实体 ID：官方从 1 递增，跳过 0（`ServerLevel.getNextEntityId`）。Bamboo 的 `NEXT_ENTITY_ID` 从 1 开始，符合要求。`add_entity` 里的 ID 必须和该玩家自己 `login` 包里的 `entity_id` 相同（官方两处都是 `Entity.getId()`，见 `PlayerList.placeNewPlayer`、`ClientboundAddEntityPacket`）。

## 3. Tab 列表

Tab 列表和实体追踪是两件事：**所有在线玩家**都在每个客户端的 PlayerInfo 表里，和距离无关。

### player_info_update

`minecraft:player_info_update`，ID 71，clientbound。来源 `ClientboundPlayerInfoUpdatePacket.write`、`entryCodec`、`Action`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | actions | 1 字节位集 | `writeEnumSet`，8 个枚举值刚好 1 字节，bit i = ordinal i |
| 2 | entries | List | 一个包里所有 entry 共用同一组 action |

每个 entry：`UUID`，然后**从低位到高位**写每个置位 action 的内容：

| bit | 掩码 | Action | 内容 |
|---|---|---|---|
| 0 | `0x01` | ADD_PLAYER | String(16) 名字；properties：VarInt 个数（≤16），每个是 String(64) name、String(32767) value、Bool 有签名、[String(1024) signature] |
| 1 | `0x02` | INITIALIZE_CHAT | Bool 有会话；有时：UUID sessionId、Long 过期时间（毫秒）、VarInt 长度 + 公钥字节（≤512）、VarInt 长度 + 签名字节（≤4096） |
| 2 | `0x04` | UPDATE_GAME_MODE | VarInt：0 生存、1 创造、2 冒险、3 旁观 |
| 3 | `0x08` | UPDATE_LISTED | Bool，是否显示在 Tab |
| 4 | `0x10` | UPDATE_LATENCY | VarInt 毫秒 |
| 5 | `0x20` | UPDATE_DISPLAY_NAME | Bool 有值；有时 Component |
| 6 | `0x40` | UPDATE_LIST_ORDER | VarInt |
| 7 | `0x80` | UPDATE_HAT | Bool |

官方在 `createPlayerInitializing` 里置全部 8 位（`0xFF`）。服务端填的值：

| 字段 | 值 |
|---|---|
| 名字 | 游戏名 |
| properties | 离线模式写 0 个 |
| chat session | 不存在，写 `00` |
| game mode | 当前模式 |
| listed | 恒为 true |
| latency | 见下 |
| display name | 不存在，写 `00`（`ServerPlayer.getTabListDisplayName` 返回 null） |
| list order | 0（`getTabListOrder`） |
| hat | `skin_parts & 0x40 != 0` |

例子 `[跑]`，手写字节交给官方解码器，再编码结果完全相同：

```
ff                                                 actions = 全部
01                                                 1 个 entry
06 9a 79 f4 44 e9 47 26 a5 be fc a9 0e 38 aa f5    UUID
05 53 74 65 76 65                                  "Steve"
00                                                 0 个 property
00                                                 没有 chat session
01                                                 创造
01                                                 listed
2a                                                 延迟 42
00                                                 没有 display name
00                                                 list order 0
01                                                 显示帽子
```

服务端要做的事：

| 时机 | 收件人 | 掩码 | 来源 |
|---|---|---|---|
| 玩家加入：先把**已有的所有玩家**发给新玩家 | 新玩家 | `0xFF` | `PlayerList.placeNewPlayer` |
| 紧接着把**新玩家**发给所有人，包括新玩家自己 | 全部 | `0xFF` | 同上 |
| 游戏模式变化 | 全部 | `0x04` | `ServerPlayerGameMode.changeGameModeForPlayer` |
| `client_information` 让帽子位变化 | 全部 | `0x80` | `ServerGamePacketListenerImpl.handleClientInformation` |
| 每 601 tick（`++counter > 600`）广播所有玩家延迟 | 全部 | `0x10` | `PlayerList.tick` |

单 action 的例子 `[跑]`：`80 01 <uuid> 01`（帽子开）、`10 01 <uuid> 2a`（延迟 42）。

延迟：收到 keep-alive 回应时 `latency = (latency * 3 + rtt_ms) / 4`，整数运算，初值 0（`ServerCommonPacketListenerImpl.handleKeepAlive`）。M1 的心跳循环要记录发送时间。

客户端（`ClientPacketListener.handlePlayerInfoUpdate`）：只有带 ADD_PLAYER 的包才新建 `PlayerInfo`。其他 action 碰到未知 UUID 时只打警告并跳过。

皮肤：离线模式没有 `textures` 属性，客户端按 `DefaultPlayerSkin.get(uuid)` 用 UUID 哈希选默认皮肤。其他玩家的皮肤要求签名有效（`PlayerInfo.createSkinLookup`，`requireSecure`），所以别伪造 `textures`。

### player_info_remove

`minecraft:player_info_remove`，ID 70，clientbound。来源 `ClientboundPlayerInfoRemovePacket`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | profileIds | List of UUID |

玩家退出时广播给所有人，在 `remove_entities` **之后**（`PlayerList.remove`）。`[跑]` `01 <uuid>`。

## 4. 谁能看到谁

来源 `ChunkMap.TrackedEntity.updatePlayer`、`ChunkMap.isChunkTracked`、`ServerPlayer.broadcastToPlayer`。

```
visible(viewer, target) =
    viewer != target
    && dx*dx + dz*dz <= range*range        // 当前坐标差，f64，不看 Y
    && broadcast_to(viewer, target)
    && chunk_tracked(viewer, target 所在区块)

range = min(effective, view(viewer) * 16)
effective = entity_broadcast_range_percentage * 512 / 100     // 默认 100，即 512
view(viewer) = clamp(viewer 请求的视距, 2, 服务端视距)
```

- 范围取决于**观察者**的视距，所以不对称：A 看得到 B 时 B 不一定看得到 A。例：服务端视距 10、客户端 12 → 160 格；客户端 2 → 32 格。
- `broadcast_to`：观察者是旁观者时，`target.camera == target` 才可见；否则 target 是旁观者就不可见，其余都可见。M2 全员创造模式，恒为 true。
- `chunk_tracked`：target 的区块在观察者的 `ChunkTrackingView` 里，并且**已经发给客户端**（不在 `PlayerChunkSender.pendingChunks`）。这保证 `add_entity` 到达时区块已加载。target 的区块 = `(floor(x) >> 4, floor(z) >> 4)`。
- 没有滞后区间：条件一旦不成立立刻移除。

官方的重新判定时机 `[读]`：

| 时机 | 判定内容 |
|---|---|
| `ChunkMap.move(A)`，每个有效的移动包都调用 | A 对所有实体，所有玩家对 A |
| `ChunkMap.tick`，实体的 section 变化时 | 该实体对所有玩家；移动过 section 的玩家再对所有实体 |
| `ChunkMap.addEntity` / `removeEntity` | 新玩家的双向 |

客户端静止时也至少每 20 tick 发一次位置包（`LocalPlayer.sendPosition` 的 `positionReminder`），所以 pending 区块发完后最迟约 1 秒内配对。

Bamboo 建议：每 tick 对所有有序玩家对跑一次 `visible`，人少时 O(n²) 够用，协议上看不出差别。状态转换：

- `false → true`：`target.seen_by` 加入 viewer，发配对包（第 5 节）。
- `true → false`：移除，给 viewer 发 `remove_entities`（第 6 节）。

## 5. 把一名玩家介绍给客户端

来源 `ServerEntity.addPairing`、`sendPairingData`。

### 5.1 顺序要求

`player_info_update`（带 ADD_PLAYER）**必须先于** `add_entity`。

`ClientPacketListener.handleAddEntity` → `createEntityFromPacket`：类型是 `EntityTypes.PLAYER` 时，用 `add_entity` 的 UUID 查 `getPlayerInfo`，查不到就打 `Server attempted to add player prior to sending player info`，返回 null，再打 `Skipping Entity with id`，**实体不创建**。查到时用 `playerInfo.getProfile()` 构造 `RemotePlayer`，名字和皮肤都来自这份 profile。

实体没创建时，之后所有针对这个 ID 的包都因 `level.getEntity(id) == null` 被静默丢弃，玩家一直隐形，直到下一次重新配对。

官方靠 `placeNewPlayer` 的顺序保证：先发 info，再 `level.addNewPlayer`。Bamboo 要让 info 和实体包走**同一条每客户端 FIFO 队列**，连接任务和世界线程不能各写各的。

### 5.2 配对包

一个 bundle，顺序固定：

1. `add_entity`
2. `set_entity_data`：全部非默认值，没有就不发
3. `update_attributes`：官方总是发，M2 可省（第 12 节）
4. `set_equipment`：全部非空槽，都空就不发
5. `set_passengers` 等：M2 没有

### add_entity

`minecraft:add_entity`，ID 1，clientbound。来源 `ClientboundAddEntityPacket.write`、构造函数 `(Entity, ServerEntity)`。

| # | 字段 | 编码 | 玩家的取值 |
|---|---|---|---|
| 1 | id | VarInt | 实体 ID |
| 2 | uuid | UUID | 必须等于 ADD_PLAYER 的 profile ID |
| 3 | type | VarInt | 159 |
| 4 | x | Double | `base.x` |
| 5 | y | Double | `base.y` |
| 6 | z | Double | `base.z` |
| 7 | movement | LpVec3 | `last_sent_movement`，M2 恒为零，写 `00` |
| 8 | xRot | Byte Angle | `last_pitch` |
| 9 | yRot | Byte Angle | `last_yaw` |
| 10 | yHeadRot | Byte Angle | `last_head` |
| 11 | data | VarInt | 0 |

**位置和角度取“上次发出去的状态”，不是当前值。** `ServerEntity` 的 delta 基准每个实体只有一份，所有观察者共用。新观察者从 `base` 开始，下一个 delta 才接得上。Bamboo 也应该这样做，这样同一个移动包可以编码一次发给所有人。

`[跑]` 实体 7，(8.5, 65, -3.25)，yaw 和头 90°：

```
07                                                 id
06 9a 79 f4 44 e9 47 26 a5 be fc a9 0e 38 aa f5    uuid
9f 01                                              type = 159
40 21 00 00 00 00 00 00                            x
40 50 40 00 00 00 00 00                            y
c0 0a 00 00 00 00 00 00                            z
00                                                 movement
00 40 40                                           xRot、yRot、yHeadRot
00                                                 data
```

客户端（`LivingEntity.recreateFromPacket`）：把 delta 基准设成包里的坐标，`yBodyRot = yHeadRot = packet.yHeadRot`。

## 6. 移除

### remove_entities

`minecraft:remove_entities`，ID 78，clientbound。来源 `ClientboundRemoveEntitiesPacket`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | entityIds | VarInt 个数 + VarInt ID |

`[跑]` ID 7 和 300：`02 07 ac 02`。官方每次只放一个 ID，格式允许多个。

| 时机 | 动作 |
|---|---|
| `visible` 变 false | 给该 viewer 发 |
| 玩家退出 | 给所有 `seen_by` 发，然后广播 `player_info_remove`（`ChunkMap.removeEntity`、`PlayerList.remove`） |
| 退出玩家是观察者 | 从所有 `seen_by` 里删掉它 |

## 7. 持续同步（`ServerEntity.sendChanges`）

### 7.1 每个玩家的同步状态

这些状态每个实体一份，不是每个观察者一份。

| 字段 | 初值 | 官方字段 |
|---|---|---|
| `base` | 开始追踪时的位置 | `positionCodec.base` |
| `last_yaw`、`last_pitch`、`last_head` | 开始追踪时的 Angle | `lastSentYRot`、`lastSentXRot`、`lastSentYHeadRot` |
| `was_on_ground` | 开始追踪时的值 | `wasOnGround` |
| `tick_count` | 0 | `tickCount` |
| `teleport_delay` | 0 | `teleportDelay` |
| `ticks_since_cycle` | -1 | `SteppedInterpolationTracker.ticksSinceLastStep` |
| `seen_by` | 空 | `TrackedEntity.seenBy` |

### 7.2 算法

每个玩家每 tick 调用一次。下面已去掉乘骑（`isPassenger`、`wasRiding`）、矿车、箭、物品展示框的分支，也去掉了 `needsSync`。官方的周期条件是 `needsSync || interval.test(tickCount) || isDirty`，玩家起跳时 `needsSync` 为真（第 12 节）。Bamboo 没有服务端物理，不设这个标记：

```
ticks_since_cycle += 1
if tick_count % 2 == 0 || metadata_dirty {            // updateInterval = 2
    yaw   = pack_degrees(player.yaw)
    pitch = pack_degrees(player.pitch)
    send_rot = yaw != last_yaw || pitch != last_pitch
    teleport_delay += 1

    path = if ticks_since_cycle > 0 { Stepped[(pos, ticks_since_cycle)] } else { Linear(pos) }
    ticks_since_cycle = 0

    send_pos = (pos - base).length_squared() >= 2^-17 || tick_count % 60 == 0

    packet =
        if teleport_delay > 400 || was_on_ground != on_ground {
            was_on_ground = on_ground
            teleport_delay = 0
            Sync
        } else if send_pos {
            match delta(base, pos) {                  // 任一轴越界返回 None
                None    => Sync                       // 这里不重置 teleport_delay
                Some(d) => if send_rot { PosRot(d) } else { Pos(d) }
            }
        } else if send_rot { Rot } else { None }

    if packet 不是 None { 发给 seen_by }
    if metadata_dirty { set_entity_data 发给 seen_by 和玩家自己 }
    if packet 带位置 { base = pos }                   // Sync、Pos、PosRot
    if packet 带角度 { last_yaw = yaw; last_pitch = pitch }   // Sync、PosRot、Rot

    head = pack_degrees(player.head_yaw)
    if head != last_head { rotate_head 发给 seen_by; last_head = head }
}
tick_count += 1
```

一个周期内的包序固定：移动包、`set_entity_data`、`update_attributes`、`rotate_head`。官方在 `needsSync` 周期里还有一个 `set_entity_motion`，排在移动包之前。

常量（`ServerEntity`）：`TOLERANCE_LEVEL_POSITION` = 2⁻¹⁷（字节码里是 double `7.62939453125E-6`）。它和距离的**平方**比较，所以实际阈值约 0.00276 格。`FORCED_POS_UPDATE_PERIOD` = 60。`FORCED_TELEPORT_PERIOD` = 400。

### 7.3 什么时候发哪个包

`[跑]` 下表来自用官方 `ServerEntity.sendChanges` 跑脚本化移动的真实输出（实体 ID 7，interval 2）：

| tick | 情况 | 发出的包 |
|---|---|---|
| 0 | 首个周期，强制位置 | `move_entity_pos 07 00 0000 0000 0000`（Linear） |
| 60、120、… | 静止，每 60 tick 强制 | `move_entity_pos 07 02 02 0000 0000 0000`（1 步，delta 为 0） |
| 126 | 走路，每周期 0.2 格 | `move_entity_pos 07 02 02 0333 0000 0000` |
| 132 | 只改 yaw，不移动 | `move_entity_rot 07 00 40 00` |
| 134 | 边走边转 | `move_entity_pos_rot 07 02 02 0333 0000 0000 4e 00` |
| 136 | 只移动 0.002 格，低于阈值 | 不发，`base` 不变 |
| 138、140 | `onGround` 翻转 | `entity_position_sync` |
| 142 | x 跳 10 格，超过 delta 范围 | `entity_position_sync` |
| 143 | 元数据变脏，触发额外周期 | `move_entity_pos 07 02 01 019a 0000 0000`（ticks = 1），然后 `set_entity_data`（含自己） |
| 940 | 距上次重置满 401 个周期 | `entity_position_sync` |

`ReviewCheck.java` 补了 `needsSync`（手动置位，模拟 `jumpFromGround`；另一次独立运行，tick 从 0 重新计）：

| tick | 情况 | 发出的包 |
|---|---|---|
| 5（奇数） | 起跳：`needsSync`，速度 (0, 0.42, 0)，`onGround` 变 false | `set_entity_motion`，然后 `entity_position_sync`（Stepped，ticks = 1），都只发观察者 |

结论：

- 全量同步的触发条件：`onGround` 变化（每次起跳和落地都发）、delta 越界、每 401 个周期。玩家每周期 2 tick，也就是约 802 tick（约 40 秒）一次，额外周期会让它提前。
- 静止玩家每 60 tick 还有一个 delta 为 0 的 `move_entity_pos`。
- 官方 `trackDeltas = false`，走路时不发 `set_entity_motion`，但每次起跳会发一个（第 12 节）。Bamboo 都不发。

### 7.4 Linear 和 Stepped

`ServerPlayer` 没有覆盖 `LivingEntity.createInterpolationHandler`，服务端用的是 `SteppedInterpolationTracker`（`[读]`）。所以除首周期外官方都发 **1 步的 Stepped**，ticks 是距上个周期的 tick 数，1 或 2（`[跑]`）。

例外 `[读]`：实体在方块上弹起时 `Entity.restituteMovementAfterCollisions` 置 `syncPosition`，`updateTracking` 会在周期之外先记一步，这时官方可能发 2 步的 Stepped。Bamboo 不模拟物理，不用管。

客户端两种都接受。`RemotePlayer` 用 `SteppedInterpolationHandler`，`interpolationSteps = updateInterval = 2`：Linear 按一步 2 tick 处理，Stepped 按包里的 ticks 处理。ticks = 2 时两者结果相同。积压时 `advance` 会加速追上。`[读]`

**Bamboo 跟官方一样：`ticks_since_cycle > 0` 用 1 步 Stepped，否则 Linear。** 多一个字节，换来和官方完全相同的客户端路径。

周期间隔保持 2 tick。每 tick 发 Linear 也能工作，但客户端按 2 tick 一步排队，会持续走追赶逻辑。

### move_entity_pos

`minecraft:move_entity_pos`，ID 54，clientbound。来源 `ClientboundMoveEntityPacket.Pos.write`、`VecDelta.write`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | entityId | VarInt | |
| 2 | properties | VarInt | `(onGround ? 1 : 0) \| stepCount << 1` |
| 3a | stepCount = 0 时 | Short xa、Short ya、Short za | Linear，第 1.2 节的 delta |
| 3b | stepCount = n 时 | n 组 `VarInt ticks, Short xa, Short ya, Short za` | Stepped，每步相对上一步。Bamboo 只用 n = 1，此时相对 `base` |

`[跑]` delta (1024, 0, -512)，onGround = true：

```
07 01 04 00 00 00 fe 00        Linear：properties = 1
07 03 02 04 00 00 00 fe 00     1 步：properties = 3，ticks = 2
```

客户端解码时要求 `stepCount <= 剩余字节 / 7`。

### move_entity_pos_rot

`minecraft:move_entity_pos_rot`，ID 55，clientbound。来源 `ClientboundMoveEntityPacket.PosRot.write`。

| # | 字段 | 编码 |
|---|---|---|
| 1–3 | 同 `move_entity_pos` | |
| 4 | yRot | Byte Angle |
| 5 | xRot | Byte Angle |

`[跑]` onGround = false，yRot `40`，xRot `e0`：`07 02 02 04 00 00 00 fe 00 40 e0`。

### move_entity_rot

`minecraft:move_entity_rot`，ID 57，clientbound。来源 `ClientboundMoveEntityPacket.Rot.write`。ID 不连续：56 是 `move_minecart_along_track`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | entityId | VarInt |
| 2 | onGround | Bool |
| 3 | yRot | Byte Angle |
| 4 | xRot | Byte Angle |

`[跑]` `07 01 40 e0`。

### entity_position_sync

`minecraft:entity_position_sync`，ID 35，clientbound。来源 `ClientboundEntityPositionSyncPacket.STREAM_CODEC`、`PositionPath.STREAM_CODEC`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | id | VarInt | |
| 2 | pathType | VarInt | 0 = LINEAR，1 = STEPPED |
| 3a | LINEAR | Double x、y、z | |
| 3b | STEPPED | VarInt n，n 组 `Double x, Double y, Double z, VarInt tickOffset` | n ≥ 1，空列表解码会抛异常。注意 ticks 在坐标**后面**，和 `VecDelta` 相反 |
| 4 | yRot | Float | 真实角度，不是 Byte |
| 5 | xRot | Float | |
| 6 | onGround | Bool | |

`[跑]` (8.5, 65, -3.25)，yaw 90，pitch 0，onGround = true：

```
07 00 <24 字节坐标> 42 b4 00 00 00 00 00 00 01       LINEAR
07 01 01 <24 字节坐标> 02 42 b4 00 00 00 00 00 00 01  STEPPED，1 步，ticks = 2
```

客户端（`handleEntityPositionSync`）：先把 delta 基准设成终点。实体在 ticking 且距离平方不超过 4096（64 格）时走插值，否则直接 `snapTo`。

### rotate_head

`minecraft:rotate_head`，ID 85，clientbound。来源 `ClientboundRotateHeadPacket.write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | entityId | VarInt |
| 2 | yHeadRot | Byte Angle |

玩家的 `head_yaw` 就是 `yaw`：`Player.aiStep` 每 tick 执行 `yHeadRot = getYRot()`。yaw 变了必须发这个包，否则别人看到身体转了头没转。身体朝向由客户端算，线上没有。客户端用 3 步插值（`lerpHeadTo(yaw, 3)`）。

官方的头比身体晚一个周期发，因为 `aiStep` 跑在 `sendChanges` 之后。Bamboo 让两者同周期发即可。

### teleport_entity

`minecraft:teleport_entity`，ID 128，clientbound。来源 `ClientboundTeleportEntityPacket`、`PositionMoveRotation.STREAM_CODEC`、`Relative`。**M2 不发。**

| # | 字段 | 编码 |
|---|---|---|
| 1 | id | VarInt |
| 2–4 | x、y、z | Double |
| 5–7 | dx、dy、dz | Double |
| 8 | yRot | Float |
| 9 | xRot | Float |
| 10 | relatives | Int 位掩码：X 1、Y 2、Z 4、Y_ROT 8、X_ROT 0x10、DELTA_X 0x20、DELTA_Y 0x40、DELTA_Z 0x80、ROTATE_DELTA 0x100 |
| 11 | onGround | Bool |

`[跑]` 总长 62 字节。

官方只在实体没有 updateInterval 或 `SERVER_AND_CLIENT` 移动模式时发给观察者（`Entity.sendTeleportTransitionToPlayers`），玩家两者都不是。服务器传送玩家时，给玩家本人发 `player_position`，观察者靠下一周期的普通同步，超过 delta 范围时自然走 `entity_position_sync`。

`handleTeleportEntity` 不更新 delta 基准，之后的 `move_entity_*` 会按旧基准算，玩家会跳回去。

## 8. 元数据

### set_entity_data

`minecraft:set_entity_data`，ID 101，clientbound。来源 `ClientboundSetEntityDataPacket`、`SynchedEntityData.DataValue.write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | id | VarInt |
| 2 | items | 重复：`UByte index, VarInt serializerId, 值` |
| 3 | 结束符 | UByte `0xFF` |

没有个数前缀，靠 `0xFF` 结束，所以 index 上限是 254。

`[跑]` 潜行 + 冲刺 + 蹲姿 + 20 血 + 左手 + 全部皮肤部件：

```
07
00 00 0a          index 0，BYTE，0x02 | 0x08
06 14 05          index 6，POSE (20)，CROUCHING
09 03 41 a0 00 00 index 9，FLOAT，20.0
0f 2a 00          index 15，HUMANOID_ARM (42)，LEFT
10 00 7f          index 16，BYTE，0x7f
ff
```

类型写错后果很重：`SynchedEntityData.assignValue` 发现序列化器和该 index 不符时抛 `IllegalStateException`，index 超范围时数组越界，都会走 `onPacketError`，客户端断开连接（`disconnect.packetError`）。

### 序列化器 ID

注册顺序不等于字段声明顺序，以 `EntityDataSerializers` 的 static 块为准。`[跑]`

| ID | 名称 | 线上值 |
|---|---|---|
| 0 | BYTE | 1 字节 |
| 1 | INT | **VarInt**，不是 Int |
| 2 | LONG | VarLong |
| 3 | FLOAT | Float |
| 4 | STRING | String(32767) |
| 5 | COMPONENT | Component |
| 6 | OPTIONAL_COMPONENT | Bool + [Component] |
| 7 | ITEM_STACK | 第 1.4 节 |
| 8 | BOOLEAN | Bool |
| 11 | OPTIONAL_BLOCK_POS | Bool + [BlockPos，`FriendlyByteBuf.writeBlockPos`]，玩家默认为空 |
| 17 | PARTICLES | List of 粒子 |
| 19 | OPTIONAL_UNSIGNED_INT | VarInt，值加 1，0 表示空 |
| 20 | POSE | VarInt |
| 42 | HUMANOID_ARM | VarInt：0 左、1 右 |

其余：9 ROTATIONS、10 BLOCK_POS、12 DIRECTION、13 OPTIONAL_LIVING_ENTITY_REFERENCE、14 BLOCK_STATE、15 OPTIONAL_BLOCK_STATE、16 PARTICLE、18 VILLAGER_DATA、21 CAT_VARIANT、22 CAT_SOUND_VARIANT、23 COW_VARIANT、24 COW_SOUND_VARIANT、25 WOLF_VARIANT、26 WOLF_SOUND_VARIANT、27 FROG_VARIANT、28 PIG_VARIANT、29 PIG_SOUND_VARIANT、30 CHICKEN_VARIANT、31 CHICKEN_SOUND_VARIANT、32 ZOMBIE_NAUTILUS_VARIANT、33 OPTIONAL_GLOBAL_POS、34 PAINTING_VARIANT、35 SNIFFER_STATE、36 ARMADILLO_STATE、37 COPPER_GOLEM_STATE、38 WEATHERING_COPPER_STATE、39 VECTOR3、40 QUATERNION、41 RESOLVABLE_PROFILE、43 DYE_COLOR。

### 玩家的元数据序号

继承链 `Entity → LivingEntity → Avatar → Player`，每层接着上一层编号（`ClassTreeIdRegistry`）。`ServerPlayer` 没有新增。`[跑]` 反射读取了每个 accessor 的 ID 和序列化器。

| index | 官方字段 | 序列化器 | 默认值 | 所属类 |
|---|---|---|---|---|
| 0 | DATA_SHARED_FLAGS_ID | 0 BYTE | 0 | Entity |
| 1 | DATA_AIR_SUPPLY_ID | 1 INT | 300 | Entity |
| 2 | DATA_CUSTOM_NAME | 6 OPTIONAL_COMPONENT | 空 | Entity |
| 3 | DATA_CUSTOM_NAME_VISIBLE | 8 BOOLEAN | false | Entity |
| 4 | DATA_SILENT | 8 BOOLEAN | false | Entity |
| 5 | DATA_NO_GRAVITY | 8 BOOLEAN | false | Entity |
| 6 | DATA_POSE | 20 POSE | STANDING | Entity |
| 7 | DATA_TICKS_FROZEN | 1 INT | 0 | Entity |
| 8 | DATA_LIVING_ENTITY_FLAGS | 0 BYTE | 0 | LivingEntity |
| 9 | DATA_HEALTH_ID | 3 FLOAT | 1.0（构造函数立刻设成 20.0） | LivingEntity |
| 10 | DATA_EFFECT_PARTICLES | 17 PARTICLES | 空 | LivingEntity |
| 11 | DATA_EFFECT_AMBIENCE_ID | 8 BOOLEAN | false | LivingEntity |
| 12 | DATA_ARROW_COUNT_ID | 1 INT | 0 | LivingEntity |
| 13 | DATA_STINGER_COUNT_ID | 1 INT | 0 | LivingEntity |
| 14 | SLEEPING_POS_ID | 11 OPTIONAL_BLOCK_POS | 空 | LivingEntity |
| 15 | DATA_PLAYER_MAIN_HAND | 42 HUMANOID_ARM | RIGHT (1) | Avatar |
| 16 | DATA_PLAYER_MODE_CUSTOMISATION | 0 BYTE | 0 | Avatar |
| 17 | DATA_PLAYER_ABSORPTION_ID | 3 FLOAT | 0 | Player |
| 18 | DATA_SCORE_ID | 1 INT | 0 | Player |
| 19 | DATA_SHOULDER_PARROT_LEFT | 19 OPTIONAL_UNSIGNED_INT | 空 | Player |
| 20 | DATA_SHOULDER_PARROT_RIGHT | 19 OPTIONAL_UNSIGNED_INT | 空 | Player |

index 0 的位（`Entity.FLAG_*`）：

| 位 | 掩码 | 含义 |
|---|---|---|
| 0 | `0x01` | 着火 |
| 1 | `0x02` | **按住潜行键**（`isShiftKeyDown`，名牌变暗） |
| 2 | `0x04` | 未使用 |
| 3 | `0x08` | **冲刺** |
| 4 | `0x10` | 游泳 |
| 5 | `0x20` | 隐身 |
| 6 | `0x40` | 发光 |
| 7 | `0x80` | 滑翔 |

index 6 的 Pose（`Pose`，VarInt）：STANDING 0、FALL_FLYING 1、SLEEPING 2、SWIMMING 3、SPIN_ATTACK 4、CROUCHING 5、DYING 7。

index 8 的位（`LivingEntity`）：`0x01` 正在使用物品，`0x02` 用副手，`0x04` 旋转攻击。M2 不用。

index 16 的位（`PlayerModelPart`）：`0x01` 披风、`0x02` 夹克、`0x04` 左袖、`0x08` 右袖、`0x10` 左裤腿、`0x20` 右裤腿、`0x40` 帽子。**服务端默认 0，也就是全部隐藏。** 客户端默认全开（`Options.modelParts`），上报 `0x7f`。

### 何时发什么

| 场景 | 内容 | 收件人 |
|---|---|---|
| 配对 | 全部非默认值，index 升序（`getNonDefaultValues`）。一般是 9 = 20.0、16 = 客户端上报值，再加上此刻非默认的 0、6、15 | 新观察者 |
| 玩家进服后第一个周期 | 官方靠 `isDirty` 发出 9 和 16（以及非默认的 15） | 观察者和**玩家自己** |
| 值变化 | 只发变化的 index | 观察者和**玩家自己**（`sendToTrackingPlayersAndSelf`） |

`metadata_dirty` 会让当 tick 进入周期（第 7.2 节）。没有移动包时也照发。

发给自己不能省：`DATA_PLAYER_MODE_CUSTOMISATION` 和 `DATA_PLAYER_MAIN_HAND` 在客户端只由这个包写入。M1 没发，所以现在玩家的 F5 和背包模型应该看不到外层皮肤（读代码推断，UNVERIFIED-1）。

`RemotePlayer.updatePlayerPose` 是空方法：其他玩家的姿势完全由 index 6 决定，客户端不会根据潜行位自己推算。所以潜行要同时改 index 0 和 index 6。

## 9. 挥手

没有 `swing` 包。M2 处理两条路径：

| 触发 | 客户端发什么 | 服务端广播 |
|---|---|---|
| 左键（打空气、挖方块、打实体，按住挖掘时持续发。`Minecraft.startAttack`、`continueAttack`） | `punch` | `swing_animation`，MAIN_HAND，物品的 attack 动画，**只发观察者** |
| 右键放方块成功 | `use_item_on`，没有挥手包。客户端本地挥手（`Minecraft.startUseItem`） | `swing_animation`，`use_item_on` 里的 hand，**只发观察者** |

放方块的来源：`BlockItem.useOn` 返回 `InteractionResult.SUCCESS`，它的 `swingSource` 是 `PREDICTED`，`handleUseItemOn` 传 `sendToSwingingEntity = false`。结果是 `SERVER_ONLY` 时才发给自己，M2 碰不到。

官方的 `handleUseItemOn` 有两个 `swingAndResetAttackStrength` 调用点，字节码确认都在。`SwingState.startIfAble` 在 `ticks == 0` 时不拦截（`[跑]`：同一 tick 第二次调用返回 true），所以会发两次。Bamboo 发一次就够，客户端的 `swing` 重新开始同一段动画。

`SwingState` 还会丢弃动画前半段里的重复挥手（`[跑]`：duration 6 时，开始后第 1 到 3 tick 返回 false，第 4 tick 返回 true）。Bamboo 不用实现：客户端自己也跑 `startIfAble`，多发的包会被忽略。

动画值：默认 `SwingAnimation.DEFAULT` = (WHACK, 6)。物品没有自定义 `attack_animation` / `interact_animation` 组件时都是它。官方里只有长矛这类设了 STAB（`Item.Properties.spear`）。M2 恒发 (WHACK, 6)。

其他 `swing_animation` 来源 M2 都不做：`use_item`（`handleUseItem`）、`interact`（`handleInteract`）、丢物品（`LivingEntity.drop`，`player_action` 4、5）。手持带 `piercing_weapon` 组件的物品（长矛）时客户端不发 `punch`，改发 `player_action` 8 `STAB`（`MultiPlayerGameMode.piercingAttack`），官方在 `PiercingWeapon.attack` 里挥手。所以 M2 里别人看不到长矛的挥手。

### punch

`minecraft:punch`，ID 46，serverbound。来源 `ServerboundPunchPacket`、`ServerGamePacketListenerImpl.handlePunch`。

包体为空。服务端：给 `seen_by` 发 `swing_animation`。

### swing_animation

`minecraft:swing_animation`，ID 123，clientbound。来源 `ClientboundSwingAnimationPacket`、`SwingAnimation.STREAM_CODEC`、`LivingEntity.swing`。

| # | 字段 | 编码 | 值 |
|---|---|---|---|
| 1 | entityId | VarInt | |
| 2 | hand | VarInt | 0 主手、1 副手 |
| 3 | type | VarInt | 0 NONE、1 WHACK、2 STAB |
| 4 | duration | VarInt | tick |

`[跑]` 主手默认：`07 00 01 06`。完整帧：`05 7b 07 00 01 06`。副手 STAB 10：`07 01 02 0a`。

### animate

`minecraft:animate`，ID 2，clientbound。来源 `ClientboundAnimatePacket`、`ClientPacketListener.handleAnimate`。**M2 不发。**

| # | 字段 | 编码 | 值 |
|---|---|---|---|
| 1 | id | VarInt | |
| 2 | action | UByte | 0 WAKE_UP、1 CRITICAL_HIT、2 MAGIC_CRITICAL_HIT |

把旧协议的“0 = 挥主手”发过来，客户端会执行 `stopSleepInBed`。

## 10. 潜行和冲刺

客户端每 tick（`LocalPlayer.sendChanges`）依次发：`player_input`（仅变化时）、`player_command`（冲刺状态变化时）、`move_player_*`。

### player_input：潜行

`minecraft:player_input`，ID 43，serverbound。来源 `ServerboundPlayerInputPacket`、`Input.STREAM_CODEC`、`handlePlayerInput`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | flags | Byte |

| 掩码 | 按键 |
|---|---|
| `0x01` | forward |
| `0x02` | backward |
| `0x04` | left |
| `0x08` | right |
| `0x10` | jump |
| `0x20` | **shift** |
| `0x40` | sprint |

`[跑]` 前进 + 潜行 + 冲刺键：`61`。

服务端：

1. `shift = flags & 0x20 != 0`，写进 index 0 的 `0x02` 位。
2. 更新姿势（`Player.getDesiredPose`）：`shift && !flying` 时 CROUCHING，否则 STANDING。创造飞行时按 shift 是下降，姿势不变，但 `0x02` 位照样置 1。

`0x40` 只是按键状态，不代表正在冲刺。服务端只把它存进 `lastClientInput`，用于进度条件（`PlayerPredicate`）。

简化：官方的 `updatePlayerPose` 还要检查碰撞（低矮空间强制蹲下或爬行）和睡觉、游泳、滑翔。M2 只做上面这条规则。

### player_command：冲刺

`minecraft:player_command`，ID 42，serverbound。来源 `ServerboundPlayerCommandPacket`、`handlePlayerCommand`、`LocalPlayer.sendIsSprintingIfNeeded`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | id | VarInt | 玩家自己的实体 ID，官方服务端不读 |
| 2 | action | VarInt | 枚举 ordinal |
| 3 | data | VarInt | 只有 START_RIDING_JUMP 用，其余 0 |

| action | 名称 | M2 处理 |
|---|---|---|
| 0 | STOP_SLEEPING | 忽略 |
| 1 | START_SPRINTING | 置 index 0 的 `0x08` |
| 2 | STOP_SPRINTING | 清 index 0 的 `0x08` |
| 3 | START_RIDING_JUMP | 忽略 |
| 4 | STOP_RIDING_JUMP | 忽略 |
| 5 | OPEN_INVENTORY | 忽略 |
| 6 | START_FALL_FLYING | **回 `set_entity_data`**，见下 |

`[跑]` 实体 7 开始冲刺：`07 01 00`。开始滑翔：`07 06 00`。

#### START_FALL_FLYING 的最小回应

这个 action 不能只忽略。来源：`LocalPlayer.aiStep`，`Player.tryToStartFallFlying`、`startFallFlying`，`LivingEntity.travel`、`travelFallFlying`、`updateFallFlying`、`stopFallFlying`，`ServerGamePacketListenerImpl.handlePlayerCommand`。`[读]`

1. 穿着鞘翅在空中按跳跃时，客户端**先在本地**置 index 0 的 `0x80`，再发这个包。
2. 之后 `LivingEntity.travel` 走 `travelFallFlying`。这条路径不读 WASD，落地后也不会退出。
3. 客户端清不掉这一位：`updateFallFlying` 的清除分支要求 `!level().isClientSide()`。本地唯一的清除点是 `travelFallFlying` 里的 `onClimbable()`。

所以服务端不回应时，玩家落地后仍在滑翔，走不了路，只有爬上梯子或重连才能恢复。创造模式物品栏里拿件鞘翅就能走到这一步。

官方的拒绝路径是 `tryToStartFallFlying` 失败时调 `stopFallFlying`，让 index 0 发回客户端。M2 不做滑翔，所以总是走这条路径：

收到 action 6 时，立刻**只给发送者**发 `set_entity_data`，index 0 写服务端当前的标志字节，`0x80` 为 0。不用改服务端状态，也不用发给观察者。

`[跑]` 实体 7，没有潜行和冲刺：`07 00 00 00 ff`，完整帧 `06 65 07 00 00 00 ff`。正在冲刺：`07 00 00 08 ff`。

效果：滑翔只持续一个往返时间，鞘翅在 M2 不可用，但玩家不会卡住。要支持滑翔时，服务端要保存 `0x80`，并在 `canGlide` 不成立时清掉它（`updateFallFlying`）。

官方的冲刺还会加 `movement_speed` 修饰符（`LivingEntity.setSprinting`），再用 `update_attributes` 同步。`RemotePlayer` 的位置来自插值，不跑移动物理（`noPhysics = true`，`aiStep` 不调 `travel`），所以对别人的画面没有影响。

`hasClientLoaded()` 门槛：`handlePlayerInput`（`setLastClientInput` 不受限，`setShiftKeyDown` 受限）和 `handlePlayerCommand` 都检查它。**进服时它是 false**：`ServerGamePacketListenerImpl` 的构造函数调 `restartClientLoadTimerAfterRespawn`，把 `clientLoadedTimeoutTimer` 设成 60。收到 `player_loaded`（serverbound，ID 44，`handleAcceptPlayerLoad`）时清零，否则 60 个玩家 tick 后放行。死亡等待重生时和重生后也是这样。

M2 不用实现这个门槛：客户端的 `LocalPlayer.sendChanges` 也要等自己的 `hasClientLoaded()`，它在发 `player_loaded` 时才置真（`ClientPacketListener.notifyPlayerLoaded`），所以 `player_input`、`player_command` 总在 `player_loaded` 之后到达。

### player_abilities：飞行状态

`minecraft:player_abilities`，ID 40，serverbound。来源 `ServerboundPlayerAbilitiesPacket`、`handlePlayerAbilities`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | flags | Byte，`0x02` = 正在飞行 |

`flying = (flags & 0x02 != 0) && may_fly`。只用于上面的姿势判断。

### client_information：视距、皮肤部件、主手

`minecraft:client_information`，Configuration 状态 ID 0，Play 状态 ID 14，serverbound。来源 `ClientInformation.write`、`ServerConfigurationPacketListenerImpl.handleClientInformation`、`ServerGamePacketListenerImpl.handleClientInformation`、`ServerPlayer.updateOptions`。

| # | 字段 | 编码 | 用途 |
|---|---|---|---|
| 1 | language | String(16) | |
| 2 | viewDistance | Byte | 第 4 节的 `view(viewer)` |
| 3 | chatVisibility | VarInt | 聊天规格 |
| 4 | chatColors | Bool | |
| 5 | modelCustomisation | UByte | 写入 index 16 |
| 6 | mainHand | VarInt | 写入 index 15 |
| 7 | textFilteringEnabled | Bool | |
| 8 | allowsListing | Bool | |
| 9 | particleStatus | VarInt | |

`[跑]` `05 7a 68 5f 63 6e 0c 00 01 7f 01 00 01 00`（zh_cn，视距 12，全部皮肤部件，右手）。

客户端在登录确认后立刻发一次（`ClientHandshakePacketListenerImpl`，Configuration 状态），之后只在选项变化时于 Play 状态发（`broadcastClientInformation`）。**M1 把 Configuration 里的这个包当 `Other` 丢了，M2 必须解码并保存。**

Play 状态收到时：更新视距和元数据。帽子位（`0x40`）变化时再广播 `UPDATE_HAT`。

## 11. 装备

### set_equipment

`minecraft:set_equipment`，ID 104，clientbound。来源 `ClientboundSetEquipmentPacket.write`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | entity | VarInt | |
| 2 | slots | 重复：`Byte slot, ItemStack` | 不是最后一项时 slot 或上 `0x80`。没有个数前缀，至少一项 |

槽位是 `EquipmentSlot` 的 **ordinal**，低 7 位：

| ordinal | 槽 | `EquipmentSlot.id`（不要用） |
|---|---|---|
| 0 | MAINHAND | 0 |
| 1 | OFFHAND | 5 |
| 2 | FEET | 1 |
| 3 | LEGS | 2 |
| 4 | CHEST | 3 |
| 5 | HEAD | 4 |
| 6 | BODY | 6 |
| 7 | SADDLE | 7 |

`[跑]` 主手 64 个石头（物品 ID 1）、清空副手、头戴钻石头盔（物品 ID 1084）：

```
07                 entity
80 40 01 00 00     MAINHAND | 0x80，count 64，item 1，0 增 0 减
81 00              OFFHAND | 0x80，count 0 = 空
05 01 bc 08 00 00  HEAD，无续位，count 1，item 1084，0 增 0 减
```

服务端（`LivingEntity.detectEquipmentUpdates`，每 tick）：比较各槽和上次发出的值，有变化就只发变化的槽，按 ordinal 排序，**只发 `seen_by`，不发自己**。变成空也要发，count 写 0。配对时发全部非空槽。

主手是快捷栏当前选中格（`PlayerEquipment.get`），所以切换 `set_carried_item` 也会触发。比较用 `ItemStack.matches`：物品、数量、组件都相同才算没变。

### set_carried_item

`minecraft:set_carried_item`，ID 54，serverbound。来源 `ServerboundSetCarriedItemPacket`、`handleSetCarriedItem`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | slot | Short | 快捷栏下标 0..8。越界时官方只打警告，不断开 |

服务端：保存选中格。主手随之变化，下一次装备比较会发 `set_equipment`。`[跑]` 第 3 格：`00 03`。

### set_creative_mode_slot

`minecraft:set_creative_mode_slot`，ID 57，serverbound。来源 `ServerboundSetCreativeModeSlotPacket`、`handleSetCreativeModeSlot`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | slotNum | Short | `InventoryMenu` 的编号，1..45 有效。小于 0 表示丢出物品，M2 忽略 |
| 2 | itemStack | ItemStack，第 1.4 节的 untrusted 形式 | count 0 表示清空槽位 |

`[跑]` 快捷栏第 0 格放 1 个石头：`00 24 01 01 00 00`（slot 36、count 1、item 1、0 增 0 减）。

`InventoryMenu` 编号：1–4 合成格、5 HEAD、6 CHEST、7 LEGS、8 FEET、9–35 背包、36–44 快捷栏（格 = slot - 36）、45 OFFHAND。

服务端：`has_infinite_materials`（`abilities.instabuild`，M2 恒为 true）时写入槽位。官方还要求 `count <= maxStackSize`，否则忽略。这个上限在物品组件里，M2 没有物品数据，不做这个检查。

M2 简化：组件按 `blocks-inventory.md` 2.5、8.3 当不透明字节保留，只用于发回本人（F 换手、中键）。发给观察者的 `set_equipment` 一律写空组件，带组件的物品别人看到的是基础外观，比如没有附魔光效。不要把这些字节转发给其他玩家：服务端没有校验过内容，非法字节会让对方解码失败并断线。比较装备变化时看 `(item, count)` 就够了。

### 主副手交换

按 F 交换主副手发 `player_action`（ID 41）action 7 `SWAP_ITEM_WITH_OFFHAND`（`Minecraft.handleKeybinds`）。客户端不做预测，要等服务端交换后回发槽位（`handlePlayerAction`）。处理按 `blocks-inventory.md` 8.3：服务端交换两个槽，给本人发 `container_set_slot`，下一次装备比较给观察者发 `set_equipment`。

### entity_event（交换主副手）

`minecraft:entity_event`，ID 34，clientbound。来源 `ClientboundEntityEventPacket`、`LivingEntity.handleHandSwap`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | entityId | **Int**，不是 VarInt |
| 2 | eventId | Byte |

主手和副手正好互换时，官方发事件 55 代替 `set_equipment`，客户端在本地交换两只手。M2 不用：直接发两个槽的 `set_equipment`，结果相同。

## 12. M2 不发的包

| 包 | 为什么可以不发 |
|---|---|
| `update_attributes`（134） | 见下 |
| `set_entity_motion`（103） | 玩家 `trackDeltas = false`，官方只在 `needsSync`、`syncVelocity`（击退等）或滑翔时发。普通走路不发，但**每次起跳都发**：`handlePlayerPositionChange` 调 `jumpFromGround`，后者置 `needsSync`，下一次 `sendChanges` 把速度发给观察者（第 7.3 节 `[跑]`）。客户端里已确认的用途是滑翔时的身体朝向（`AvatarRenderer.extractFlightData`）和 bob（`AbstractClientPlayer`）。bob 只在 `onGround` 时读速度，所以省掉起跳这一个包没有已知影响 |
| `teleport_entity`（128） | 第 7 节 |
| `animate`（2） | 只有起床和暴击粒子 |
| `entity_event`（34） | 第 11 节 |
| `set_passengers`、`set_entity_link` | 没有载具和拴绳 |
| `waypoint`（141） | 定位栏。不发时 `ClientWaypointManager.hasWaypoints()` 为 false |

滑翔不做：`START_FALL_FLYING` 按第 10 节拒绝，别人看到的是普通站姿。

### set_entity_motion

`minecraft:set_entity_motion`，ID 103，clientbound。来源 `ClientboundSetEntityMotionPacket`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | id | VarInt |
| 2 | movement | LpVec3 |

`[跑]` (0.5, 0, -0.25)：`07 f9 ff 5f fe ff fe`。

### update_attributes

`minecraft:update_attributes`，ID 134，clientbound。来源 `ClientboundUpdateAttributesPacket.STREAM_CODEC`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | entityId | VarInt |
| 2 | attributes | VarInt 个数（≤128），每个：VarInt `minecraft:attribute` ID、Double base、List of 修饰符 |

修饰符：String 标识符、Double amount、VarInt operation（0 add_value、1 add_multiplied_base、2 add_multiplied_total）。

对其他玩家可省：不发时 `RemotePlayer` 用 `Player.createAttributes` 的默认值，M2 没有改属性的东西。

对玩家**自己**有一个可见差异：创造模式的触及距离加成完全由服务端下发。`ServerPlayer.updatePlayerAttributes` 给 `block_interaction_range` 加 `minecraft:creative_mode_block_range` +0.5（add_value），给 `entity_interaction_range` 加 `minecraft:creative_mode_entity_range` +2.0，客户端代码里没有这两个常量。不发时创造模式的方块触及是 4.5 格而不是 5 格。要对齐官方，进服后给自己发一次：

`[跑]` 属性 ID 8（`block_interaction_range`，base 4.5，1 个修饰符）：

```
07 01
08
40 12 00 00 00 00 00 00
01
23 "minecraft:creative_mode_block_range"（35 字节）
3f e0 00 00 00 00 00 00
00
```

`entity_interaction_range` 的属性 ID 是 13，base 3.0。这件事属于方块交互，这里只提示。

## 13. Bamboo 要改的地方

现有代码和本规格的差距：

1. `configuration.rs` 解码 `client_information`，保存视距、皮肤部件、主手。Play 状态同样处理 ID 14。
2. `AddPlayerInfo` 改成多 entry、掩码 `0xFF`。现在只支持自己，掩码 `0x0D`（官方解码器确认可解，`[跑]`）。
3. `PlayRequest` 增加 `punch`、`player_input`、`player_command`、`player_abilities`、`client_information`、`set_carried_item`、`set_creative_mode_slot`。
4. 心跳循环记录 RTT，算 `latency`。
5. `mc-bamboo-data` 增加 `entity_type`、`item`、`attribute` 的 ID。
6. 世界线程持有第 7.1 节的状态。所有发给客户端的包走每客户端一条 FIFO 队列。

队列不能丢包：delta 基准是所有观察者共用的，漏掉一个 `move_entity_*` 的客户端会一直偏移，直到下一次全量同步。写不进去时断开连接，不要丢弃。上限要按字节算，并容得下区块批次的峰值，心跳不能依赖写 socket 完成，这两条见 `coverage.md` 第 5 节。

世界线程每 tick 的建议顺序（官方顺序不同，但协议上没有依赖）。这里只列实体相关的步骤，合并了区块和方块之后的完整顺序在 `coverage.md` 4.3：

1. 处理入站包：移动、输入、命令、设置、物品。
2. 更新派生状态：`head_yaw = yaw`，姿势，装备差异发 `set_equipment`。
3. 对所有有序玩家对跑 `visible`，配对或解除。
4. 对每个玩家跑 `send_changes`。
5. 每 601 tick 广播延迟。

官方的顺序供参考（`MinecraftServer.tickChildren`）：`level.tick`（其中 `ChunkMap.tick` 调 `sendChanges`）、`tickConnection`（`ServerPlayer.doTick`：姿势、头、装备）、`playerList.tick`。

## 14. 时序例子

A 在线，B 在 A 附近加入，都是创造模式：

```
B 登录、Configuration（client_information 已保存）
→ B  login、abilities、player_position 等（M1 已有）
→ B  player_info_update(0xFF, [A])                 已有玩家发给新玩家
→ A  player_info_update(0xFF, [B])                 新玩家发给所有人
→ B  player_info_update(0xFF, [B])
→ B  区块
世界线程：visible(A, B) 成立，A 已有 B 的区块
→ A  bundle{ add_entity(B), set_entity_data(B: 9, 16) }
第一个周期：
→ A,B  set_entity_data(B: 9, 16)                   观察者和自己
B 的区块发完后：visible(B, A) 成立
→ B  bundle{ add_entity(A), set_entity_data(A), set_equipment(A) }

B 走路：
→ A  每 2 tick move_entity_pos / move_entity_pos_rot，转头时再加 rotate_head
B 起跳：
→ A  entity_position_sync                          官方在它之前还有 set_entity_motion，Bamboo 不发
B 按住潜行：
← B  player_input(0x20)
→ A,B  set_entity_data(B: 0 = 0x02, 6 = CROUCHING)
B 左键：
← B  punch
→ A  swing_animation(B, 主手, WHACK, 6)
B 放方块：
← B  use_item_on
→ A  swing_animation(B, hand, WHACK, 6)，另加方块更新（方块规格）
B 退出：
→ A  remove_entities([B])
→ A  player_info_remove([B])
```

玩家退出和加入时官方还会广播系统消息，在聊天规格里。

“第一个周期”那一行是官方的行为。如果已经按 `chat-join.md` 8.5 第 6 步在进服时把 index 15、16 发给了自己，新观察者又从配对包拿到全部非默认值，这一行可以省掉。多发一次也没有影响。

## 15. UNVERIFIED

本任务不运行客户端，下列内容没有实机确认：

1. **所有客户端可见效果**：画面表现（插值平滑度、名牌变暗、F5 外层皮肤）都来自读 `net.minecraft.client.*` 反编译源码。线上格式已用官方 codec 验证，客户端解码用的是同一批类。M1 里“F5 看不到外层皮肤”是由 `isModelPartShown` 和默认值 0 推断的，没有截图确认。
2. **`Trace.java` 和 `ReviewCheck.java` 用的是 `Marker` 实体，不是 `ServerPlayer`**：造一个真实的 `ServerPlayer` 需要 Level。`rotate_head`（`Marker` 的头部朝向恒为 0）、`update_attributes`、滑翔时的速度这些分支没有执行到，只读了代码。`needsSync` 是手动置位的，`jumpFromGround` 触发它这一段也是读的。除 `needsSync` 外，移动包选择的路径是相同的。
3. **官方放方块时线上是否真的有两个 `swing_animation`**：字节码有两个调用点，`SwingState` 同一 tick 不拦截已经跑过，但没有抓真实流量。Bamboo 发一次，不依赖这一点。
4. **`set_equipment`、`set_creative_mode_slot` 例子里的物品**：harness 绑定的是空组件表（完整组件要加载数据包），不影响字节，因为线上只写 patch。
5. **Linear 和 1 步 Stepped 在 ticks = 2 时等价**：由 `SteppedInterpolationHandler.addSteps` 推断。Bamboo 用和官方相同的形式，所以不依赖这个结论。
