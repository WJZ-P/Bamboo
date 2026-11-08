# M2 规格：玩家移动与区块跟随

目标版本：Minecraft: Java Edition 26.3，协议 777（`client.jar` 内 `version.json`：`id=26.3`，`protocol_version=777`，`world_version=5023`）。

| 事实来源 | 用途 |
|---|---|
| 反编译的 26.3 源码 `mc-src/net/minecraft/...` | 所有字段和行为 |
| `target/vanilla/generated/reports/packets.json` | 所有包 ID，已和 `crates/mc-bamboo-data/src/generated/packets.rs` 对过 |
| `client.jar` 字节码（`javap`） | 抽查 `ChunkTrackingView.isWithinDistance`、`ClientChunkCache.calculateStorageRange`、`PlayerChunkSender` 的常量和 `onChunkBatchReceivedByClient`、`ChunkBatchSizeCalculator` 的常量、`ClientPacketListener.handleChunkBatchFinished`，都和反编译结果一致 |
| Guava 33.6.0 字节码（`target/vanilla/libraries`） | `Comparators.least` 的结果按升序排列 |
| `client.jar` 和 `target/vanilla/versions/26.3/server-26.3.jar` 的 class 字节比较 | `ServerGamePacketListenerImpl`、`PlayerChunkSender`、`ChunkMap`、`ChunkTrackingView`、`PlayerList`、本文涉及的包类等 17 个类 SHA-1 完全相同，所以反编译的服务端逻辑就是官方服务端的逻辑 |

**本文没有运行过服务端或客户端。** 所有“客户端会怎样”都是读源码得出的，没有实机验证的内容在第 12 节列出。

分级用语：

- **必须**：不做就玩不了，或者客户端和服务端的状态会不一致。
- **建议**：不影响可玩，但成本很低，M2 顺手做。
- **以后**：反作弊、生存模式或性能相关，M2 不做。

---

## 0. 先看这里

1. **`accept_teleportation` 在 26.3 带着客户端的完整位置和朝向**（VarInt + 3 个 Double + 2 个 Float，共 33 字节以上）。M1 的 `PlayRequest::decode` 只读 id，也没有调用 `reader.finish()`，所以现在没出错。给这个分支加 `finish()` 而不读完字段，就会报 `TrailingBytes`。
2. **客户端收到 `player_position` 后只回 `accept_teleportation`，没有额外的移动包。** 官方服务端把确认包里的位置当作一次移动来处理。
3. **跟踪范围既不是方形，也不是 `view_distance` 圈。** 它是集合 `T(vd)`：沿坐标轴半径 `vd + 1`，四角是圆的。`vd = 8` 时有 329 个区块。M1 的 `chunks_around` 发 289 个，和 `T(8)` 相比缺 44 个、多 4 个。
4. **客户端只编译 8 个相邻区块列都到齐、且光照已启用的非空 section。** `T(vd)` 正好等于渲染范围 `R(vd)` 向外扩一圈。按 M1 的方形发送，`vd = 8` 时渲染范围内 257 个区块有 36 个永远编译不出来。
5. **加载画面要等摄像机所在的 section 编译完才关闭，所以出生点周围 3×3 必须最先到。** 第一批的配额是 9 个、最近优先，`pending` 里至少有 9 个区块时正好是 3×3（见 7.4）。
6. **客户端没有“等区块”的保护。** `ClientLevel.hasChunk` 恒为 `true`，走进还没收到的区块时没有碰撞，重力照常，玩家会掉下去。
7. **`set_chunk_cache_center` 必须先于新区块发送。** 超出客户端范围的区块会被丢弃，只打一行 WARN，服务端不会知道，这个区块也不会被重新请求。
8. **三个包的区块坐标编码不一样：** `set_chunk_cache_center` 是两个 VarInt，x 在前。`level_chunk_with_light` 是两个 Int，x 在前。`forget_level_chunk` 是一个 Long，**线上 z 在前**。
9. **`chunk_batch_received` 是客户端网络线程发出的**，意思是“批次收到了”，不是“主线程处理完了”。服务端没有确认超时，收不到确认就只是停发。
10. **区块发送不能等 `player_loaded`。** 客户端要先收到区块才会发 `player_loaded`。互相等待时不会永久卡死，但要等客户端 30 秒超时（8.3）才放行。
11. **记忆条目需要更正。** `bamboo-vanilla-verification` 写着 “26.3 has no `hasAllNeighbors`”，但这次读到 `net.minecraft.client.SectionUpdateTracker.hasAllNeighbors`，`LevelExtractor` 用它决定 section 能否首次编译（见 8.5）。同一条里 “`SectionCopy` treats unloaded neighbours as air” 仍然成立。

---

## 1. 编码约定

| 写法 | 含义 |
|---|---|
| VarInt | 协议的变长整数，负数固定 5 字节（没有 zigzag）。M1 的 `write_varint` / `Reader::varint` |
| Double | 8 字节 IEEE 754，大端 |
| Float | 4 字节 IEEE 754，大端 |
| Int / Long | 4 / 8 字节有符号整数，大端 |
| Byte / Unsigned Byte | 1 字节 |
| Boolean | 1 字节，0 或 1 |
| String(n) | VarInt 字节长度 + UTF-8，最多 n 个字符 |

未开启压缩时一帧是 `VarInt(包 ID 和内容的总长度) + VarInt(包 ID) + 内容`。本文的字节示例都是完整帧。压缩不在 M2 范围内。

---

## 2. Serverbound 包

### 2.0 四种移动包的公共部分

`flags`，Unsigned Byte（`readUnsignedByte`）。其余位被忽略。

| 位 | 掩码 | 字段 |
|---|---|---|
| 0 | 0x01 | `onGround` |
| 1 | 0x02 | `horizontalCollision` |

例：站在地上并撞到墙是 `0x03`，悬空是 `0x00`。

- `x`、`y`、`z` 是实体位置，也就是脚底。
- `yRot` 是水平朝向（yaw），`xRot` 是俯仰（pitch）。线上总是 `yRot` 在前。客户端的 `yRot` 不做归一化，可以是任意有限值。

客户端的发送规则（`LocalPlayer.sendPosition`，`sendChanges` 每个客户端 tick 调用一次）：

- `move` = 和上次发送位置的距离平方 > (2.0E-4)²，或距上次发送位置已满 20 tick。
- `rot` = `yRot` 或 `xRot` 和上次发送的不同。

| 条件 | 发送的包 |
|---|---|
| `move` 且 `rot` | `move_player_pos_rot` |
| 只有 `move` | `move_player_pos` |
| 只有 `rot` | `move_player_rot` |
| 都没有，但 `onGround` 或 `horizontalCollision` 变了 | `move_player_status_only` |
| 都没有 | 不发 |

所以每个客户端 tick 最多一个移动包，玩家静止时也至少每秒一个 `move_player_pos`。服务端不需要回应这些包，官方没有回显。

一个客户端 tick 内的发送顺序（`Minecraft.tick`）：

1. `gameMode.tick()` → `Connection.tick()` → `ClientPacketListener.tick()`，可能发 `player_loaded`。
2. `level.tickEntities()`，玩家物理在这里计算。
3. `player.sendChanges()`：`player_input`（按键变了才发）→ `player_command`（疾跑状态变了才发）→ 移动包。
4. `client_tick_end`。

### 2.1 `minecraft:move_player_pos`

**ID 30 (0x1E)，serverbound，Play。** 来源：`ServerboundMovePlayerPacket.Pos.read/write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | x | Double |
| 2 | y | Double |
| 3 | z | Double |
| 4 | flags | Unsigned Byte |

例：`x=8.5, y=-60.0, z=8.5, flags=0x01`

```text
1A 1E  40 21 00 00 00 00 00 00  C0 4E 00 00 00 00 00 00  40 21 00 00 00 00 00 00  01
```

服务端：按 5.3 处理。**必须**。

### 2.2 `minecraft:move_player_pos_rot`

**ID 31 (0x1F)，serverbound，Play。** 来源：`ServerboundMovePlayerPacket.PosRot.read/write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | x | Double |
| 2 | y | Double |
| 3 | z | Double |
| 4 | yRot | Float |
| 5 | xRot | Float |
| 6 | flags | Unsigned Byte |

例：`x=8.5, y=-60.0, z=8.5, yRot=90.0, xRot=-45.0, flags=0x03`

```text
22 1F  40 21 00 00 00 00 00 00  C0 4E 00 00 00 00 00 00  40 21 00 00 00 00 00 00  42 B4 00 00  C2 34 00 00  03
```

服务端：按 5.3 处理。**必须**。

### 2.3 `minecraft:move_player_rot`

**ID 32 (0x20)，serverbound，Play。** 来源：`ServerboundMovePlayerPacket.Rot.read/write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | yRot | Float |
| 2 | xRot | Float |
| 3 | flags | Unsigned Byte |

例：`yRot=90.0, xRot=-45.0, flags=0x00`

```text
0A 20  42 B4 00 00  C2 34 00 00  00
```

服务端：按 5.3 处理，位置保持不变。**必须**。

### 2.4 `minecraft:move_player_status_only`

**ID 33 (0x21)，serverbound，Play。** 来源：`ServerboundMovePlayerPacket.StatusOnly.read/write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | flags | Unsigned Byte |

例：`flags=0x01`：`02 21 01`

服务端：只更新 `on_ground`。**必须**解码（否则其他玩家看到的落地状态会错）。

### 2.5 `minecraft:accept_teleportation`

**ID 0 (0x00)，serverbound，Play。** 来源：`ServerboundAcceptTeleportationPacket.STREAM_CODEC`。发送方：`ClientPacketListener.handleMovePlayer`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | id | VarInt | `player_position` 里的 id |
| 2 | x | Double | 客户端应用传送之后的位置 |
| 3 | y | Double | |
| 4 | z | Double | |
| 5 | yRot | Float | |
| 6 | xRot | Float | |

例：`id=1, x=8.5, y=-60.0, z=8.5, yRot=0.0, xRot=0.0`

```text
22 00  01  40 21 00 00 00 00 00 00  C0 4E 00 00 00 00 00 00  40 21 00 00 00 00 00 00  00 00 00 00  00 00 00 00
```

服务端：按 5.2 处理。**建议**解码全部字段并走确认流程。M2 唯一的传送是进入世界那次，所以暂时不影响游玩，但一旦有 `/tp` 或回滚就变成**必须**。不处理时至少不能当错误。

### 2.6 `minecraft:player_loaded`

**ID 44 (0x2C)，serverbound，Play。** 来源：`ServerboundPlayerLoadedPacket`，`StreamCodec.unit`。内容为空。

帧：`01 2C`

官方：`handleAcceptPlayerLoad` → `markClientLoaded`，把 `clientLoadedTimeoutTimer` 清零。

服务端：不当作错误就够了。真实客户端发完这个包才开始发移动包和交互包（见 8.3），所以 M2 不需要用它做门限。

### 2.7 `minecraft:client_tick_end`

**ID 13 (0x0D)，serverbound，Play。** 来源：`ServerboundClientTickEndPacket`，`StreamCodec.unit`。内容为空。

帧：`01 0D`

发送时机：每个客户端 tick 的末尾，条件是 `level != null` 且未暂停。多人游戏的 `Minecraft.pause` 恒为 false（`pause` 要求 `hasSingleplayerServer()`）。**加载画面期间也会发**。

官方（`handleClientTickEnd`）：这一 tick 没收到移动就把 `knownMovement` 清零，然后清掉 `receivedMovementThisTick` 和 `receivedPositionThisTick`。

服务端：不当作错误就够了。每名玩家每秒约 20 个，不要记日志。只有实现 5.1 的 A2（**以后**）才需要它。

### 2.8 `minecraft:player_input`

**ID 43 (0x2B)，serverbound，Play。** 来源：`ServerboundPlayerInputPacket`，`Input.STREAM_CODEC`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | flags | Byte |

| 位 | 掩码 | 字段 |
|---|---|---|
| 0 | 0x01 | forward |
| 1 | 0x02 | backward |
| 2 | 0x04 | left |
| 3 | 0x08 | right |
| 4 | 0x10 | jump |
| 5 | 0x20 | shift |
| 6 | 0x40 | sprint |

例：前进并疾跑 `0x01 | 0x40 = 0x41`：`02 2B 41`。前进并潜行 `0x21`：`02 2B 21`。

发送时机：`LocalPlayer.sendChanges`，客户端已加载且按键状态和上次不同时才发。

官方（`handlePlayerInput`）：保存输入。已加载时重置空闲计时，并 `setShiftKeyDown(input.shift())`。

服务端：本领域可以忽略。`ServerboundPlayerCommandPacket.Action` 里没有潜行动作，**潜行状态主要靠 `shift` 位**，“玩家互相可见”要靠它显示潜行（`interact` 包的 `usingSecondaryAction` 也会写同一个状态，见 `handleInteract`）。疾跑走 `player_command` 的 `START_SPRINTING` / `STOP_SPRINTING`（格式见 2.11）。

### 2.9 `minecraft:chunk_batch_received`

**ID 11 (0x0B)，serverbound，Play。** 来源：`ServerboundChunkBatchReceivedPacket`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | desiredChunksPerTick | Float |

例：`3.5`：`05 0B 40 60 00 00`

服务端：按 7.3 处理。**必须**。

### 2.10 `minecraft:client_information`

**Configuration 状态 ID 0 (0x00)，Play 状态 ID 14 (0x0E)，serverbound。** 来源：`ClientInformation(FriendlyByteBuf)` / `write`，`ServerboundClientInformationPacket`。

| # | 字段 | 编码 | 说明 |
|---|---|---|---|
| 1 | language | String(16) | |
| 2 | viewDistance | Byte，有符号 | 客户端的渲染距离选项，下限 2，上限 16 或 32（`Options.renderDistance`） |
| 3 | chatVisibility | VarInt | 0 FULL，1 SYSTEM，2 HIDDEN |
| 4 | chatColors | Boolean | |
| 5 | modelCustomisation | Unsigned Byte | |
| 6 | mainHand | VarInt | 0 LEFT，1 RIGHT |
| 7 | textFilteringEnabled | Boolean | |
| 8 | allowsListing | Boolean | |
| 9 | particleStatus | VarInt | 0 ALL，1 DECREASED，2 MINIMAL |

例：`language="zh_cn", viewDistance=12, chatVisibility=0, chatColors=true, modelCustomisation=0x7F, mainHand=1, textFilteringEnabled=false, allowsListing=true, particleStatus=0`

```text
Configuration: 0F 00  05 7A 68 5F 63 6E  0C  00  01  7F  01  00  01  00
Play:          0F 0E  05 7A 68 5F 63 6E  0C  00  01  7F  01  00  01  00
```

发送时机：

- Configuration：`login_acknowledged` 之后紧跟着发，顺序是 `custom_payload(brand)`、`client_information`（`ClientHandshakePacketListenerImpl`）。
- Play：`Options.save()` → `broadcastOptions()`，内容和上次发送的不同才发（`broadcastClientInformation`）。比较基准 `remoteClientInformation` 初值是 null，所以进入 Play 后第一次 `save` 一定会发，哪怕内容和 Configuration 里那次相同。

服务端：保存 `viewDistance` 作为 `requested_view_distance`，下一 tick 由 6.2 生效。官方的默认值是 2（`ServerPlayer.requestedViewDistance` 字段初值，`ClientInformation.createDefault()`）。**建议**：不做时用服务端视距也能玩，只是多发区块。其他字段由各自的领域决定是否使用。

### 2.11 移动时还会收到的两个包

本文的移动处理不依赖它们，格式和处理都在 `entities.md`：

- `minecraft:player_command`（ID 42，serverbound）：疾跑状态变化时发，顺序见 2.0。
- `minecraft:player_abilities`（ID 40，serverbound）：创造模式切换飞行时发。官方只写 `abilities.flying`（`handlePlayerAbilities`）。

M1 的 `PlayRequest::decode` 把它们归入 `Other`，不会报错。

---

## 3. Clientbound 包

### 3.1 `minecraft:set_chunk_cache_center`

**ID 96 (0x60)，clientbound，Play。** 来源：`ClientboundSetChunkCacheCenterPacket.write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | x | VarInt，区块坐标 |
| 2 | z | VarInt，区块坐标 |

例：`(1, 0)`：`03 60 01 00`。`(-1, 300)`：`08 60 FF FF FF FF 0F AC 02`。

服务端：按 6.3 发送。**必须**。M1 已有 `SetChunkCacheCenter`。

### 3.2 `minecraft:forget_level_chunk`

**ID 38 (0x26)，clientbound，Play。** 来源：`ClientboundForgetLevelChunkPacket.write`，`FriendlyByteBuf.writeChunkPos`，`ChunkPos.pack`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | pos | Long：`(z as u32 as u64) << 32 \| (x as u32 as u64)` |

低 32 位是 x，高 32 位是 z，按大端写出后**线上先是 z，后是 x**。可以等价地写成 `z` 的 Int 加上 `x` 的 Int。

例：`x=-3, z=5`：

```text
x & 0xFFFFFFFF         = 0xFFFFFFFD
(z & 0xFFFFFFFF) << 32 = 0x00000005_00000000
pack                   = 0x00000005_FFFFFFFD
帧：09 26  00 00 00 05  FF FF FF FD
```

`x=-3, z=-3`：`09 26 FF FF FF FD FF FF FF FD`

服务端：按 6.3 发送，不走批次。**必须**。M1 还没有这个包。

### 3.3 `minecraft:chunk_batch_start`

**ID 12 (0x0C)，clientbound，Play。** 来源：`ClientboundChunkBatchStartPacket`，`StreamCodec.unit`。内容为空。

帧：`01 0C`

服务端：按 7.2 发送。**必须**。M1 已有 `ChunkBatchStart`。

### 3.4 `minecraft:chunk_batch_finished`

**ID 11 (0x0B)，clientbound，Play。** 来源：`ClientboundChunkBatchFinishedPacket.write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | batchSize | VarInt，这批的区块数 |

例：`9`：`02 0B 09`。`300`：`03 0B AC 02`。

服务端：按 7.2 发送。**必须**。M1 已有 `ChunkBatchFinished`。

### 3.5 `minecraft:level_chunk_with_light`（M1 已实现）

**ID 46 (0x2E)，clientbound，Play。** 来源：`ClientboundLevelChunkWithLightPacket.STREAM_CODEC`，`ClientboundLevelChunkPacketData.STREAM_CODEC`，`ClientboundLightUpdatePacketData.STREAM_CODEC`，`LevelChunkSection.write`，`PalettedContainer.Data.write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | x | Int |
| 2 | z | Int |
| 3 | heightmaps | VarInt 条目数；每条：VarInt 类型 + VarInt 长度 + Long × 长度 |
| 4 | data | VarInt 字节长度（上限 2097152）+ 所有 section 依次写出 |
| 5 | blockEntities | VarInt 个数；每个：Byte packedXZ（`(x & 15) << 4 \| (z & 15)`）、Short y、VarInt 类型、可选 NBT（单字节 `0x00` 表示没有）。M1 恒为 0 个 |
| 6 | skyYMask | BitSet |
| 7 | blockYMask | BitSet |
| 8 | emptySkyYMask | BitSet |
| 9 | emptyBlockYMask | BitSet |
| 10 | skyUpdates | VarInt 个数；每个：VarInt 长度（上限 2048）+ 字节 |
| 11 | blockUpdates | 同上 |

- 发给客户端的高度图类型：1 `WORLD_SURFACE`、4 `MOTION_BLOCKING`、5 `MOTION_BLOCKING_NO_LEAVES`（`Heightmap.Types.sendToClient`）。
- section：Short `nonEmptyBlockCount`、Short `fluidCount`、方块 PalettedContainer、生物群系 PalettedContainer。
- PalettedContainer：Byte 位数、调色板、定长 Long 数组（**没有长度前缀**）。
  - 调色板按位数分三种（`Strategy`，各调色板的 `write`）：位数 0 是一个 VarInt 全局 ID。方块 4..=8 位、生物群系 1..=3 位是 VarInt 个数加每项一个 VarInt 全局 ID。更大的位数是全局调色板，不写内容。方块不能写 1..=3 位，官方这时用 4 位。
  - Long 的个数是 `ceil(条目数 / floor(64 / 位数))`，条目不跨 Long，位数 0 时为 0（`SimpleBitStorage`、`ZeroBitStorage`）。方块 4096 条，生物群系 64 条。
- BitSet（`ByteBufCodecs.BIT_SET`）：VarInt 字节数 + `BitSet.toByteArray()` 的结果，小端位序，末尾的零字节省略。例：第 25 位置位是 `04 00 00 00 02`。

M1 的 `encode_chunk_body` 和这张表逐项一致，M2 不用改格式。

服务端：按第 7 节的批次发送。**必须**。M2 改变的是发送时机和内容来源（见第 10 节）。

### 3.6 `minecraft:player_position`（M1 已实现）

**ID 73 (0x49)，clientbound，Play。** 来源：`ClientboundPlayerPositionPacket.STREAM_CODEC`，`PositionMoveRotation.STREAM_CODEC`，`Vec3.STREAM_CODEC`，`Relative.SET_STREAM_CODEC`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | id | VarInt |
| 2 | x | Double |
| 3 | y | Double |
| 4 | z | Double |
| 5 | deltaX | Double |
| 6 | deltaY | Double |
| 7 | deltaZ | Double |
| 8 | yRot | Float |
| 9 | xRot | Float |
| 10 | relatives | Int，位掩码 |

| 位 | 掩码 | `Relative` | 位 | 掩码 | `Relative` |
|---|---|---|---|---|---|
| 0 | 0x001 | `X` | 5 | 0x020 | `DELTA_X` |
| 1 | 0x002 | `Y` | 6 | 0x040 | `DELTA_Y` |
| 2 | 0x004 | `Z` | 7 | 0x080 | `DELTA_Z` |
| 3 | 0x008 | `Y_ROT` | 8 | 0x100 | `ROTATE_DELTA` |
| 4 | 0x010 | `X_ROT` | | | |

置位的字段是相对当前值的增量，未置位的是绝对值。例：`X | Z | Y_ROT` = `0x0D`，线上 `00 00 00 0D`。M2 只用绝对传送，`relatives = 0`，delta 全 0，和官方的 `teleport(x, y, z, yRot, xRot)` 相同。

例：`id=1, x=8.5, y=-60.0, z=8.5`，其余全 0，共 63 字节：

```text
3E 49  01
40 21 00 00 00 00 00 00  C0 4E 00 00 00 00 00 00  40 21 00 00 00 00 00 00
00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00
00 00 00 00  00 00 00 00
00 00 00 00
```

客户端：应用位置、速度、朝向，**立刻**回 `accept_teleportation`，然后调用 `BlockStatePredictionHandler.onTeleport` 和 `stopDestroyBlock`。这条路径不受加载状态影响。绝对传送会把客户端速度置零。

服务端：按 5.2 发送。M1 的 `PlayerPosition` 字段顺序正确。

### 3.7 `minecraft:set_chunk_cache_radius`

**ID 97 (0x61)，clientbound，Play。** 来源：`ClientboundSetChunkCacheRadiusPacket.write`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | radius | VarInt |

例：`8`：`02 61 08`

官方只在服务端运行时改视距时广播（`PlayerList.setViewDistance`）。进入世界时的半径由 `login` 的 `chunkRadius` 给出。**以后**：Bamboo 的视距目前是启动配置，用不到这个包。

### 3.8 `minecraft:game_event`，事件 13（M1 已实现）

**ID 39 (0x27)，clientbound，Play。** 来源：`ClientboundGameEventPacket.write`，`ClientboundGameEventPacket.LEVEL_CHUNKS_LOAD_START`。

| # | 字段 | 编码 |
|---|---|---|
| 1 | event | Unsigned Byte，13 |
| 2 | param | Float，0.0 |

帧：`06 27 0D 00 00 00 00`

**必须保留。** 客户端收不到它就永远卡在加载画面（见 8.3）。M1 的 `LevelChunksLoadStart` 已经在发。

---

## 4. 官方服务端一个 tick 的顺序

来源：`MinecraftServer.processPacketsAndTick`、`tickChildren`，`ServerChunkCache.tick`。

1. `packetProcessor.processQueuedPackets()`：执行网络线程排进来的所有包。`handleMovePlayer` 在这里运行，`ChunkMap.move` 也在这里运行，所以 `set_chunk_cache_center` 和 `forget_level_chunk` 在这一步就发出。
2. 所有玩家 `suspendFlushing`。
3. `level.tick` → `ServerChunkCache.tick` → `ChunkMap.tick()`：对每个玩家调用 `updateChunkTracking`，然后做实体同步。
4. `tickConnection` → `ServerGamePacketListenerImpl.tick`：`resetPosition`、`player.doTick()`、`tickCount++`、心跳。
5. `playerList.tick()` 等。
6. 最后是 “send chunks”：每个玩家 `chunkSender.sendNextChunks(player)`，然后 `resumeFlushing`。

Bamboo 世界线程的等价顺序：

1. 按到达顺序处理这一 tick 之前收到的所有包（同一连接内不能乱序）。
2. 对每个玩家更新区块跟踪（6.2、6.3）。
3. 其他系统（实体同步等）。
4. 对每个玩家发一批区块（7.2）。

官方在第 1 步就更新跟踪，Bamboo 放到第 2 步效果相同，因为区块都是在 tick 末尾才发。

这里只列区块相关的步骤。合并了实体和方块之后的完整顺序在 `coverage.md` 4.3。

---

## 5. 移动处理

### 5.1 官方的 `handleMovePlayer`

来源：`ServerGamePacketListenerImpl.handleMovePlayer`、`handlePlayerPositionChange`、`tickPlayer`。下表按执行顺序列出。

| 步骤 | 官方行为 | Bamboo |
|---|---|---|
| A1 | x/y/z 是 NaN，或 yRot/xRot 不是有限值（缺的字段按 0 算）→ 断开，`multiplayer.disconnect.invalid_player_movement`。**±Infinity 的位置不算非法**，后面被 clamp | **必须**。NaN 进入区块计算后结果不可预期 |
| A2 | 带位置的包，且 `receivedPositionThisTick` 已为 true → 同样断开。标记由 `client_tick_end` 清除，**不是服务端 tick**。正常客户端每个客户端 tick 最多一个 | **已做**。连接任务把 `client_tick_end` 转给世界线程，由它清除标记 |
| A3 | `player.wonGame`（看完终末之诗）→ 忽略 | 不需要 |
| A4 | `tickCount == 0` → `resetPosition` | 不需要 |
| A5 | `hasClientLoaded()` 为 false → 静默忽略。`clientLoadedTimeoutTimer` 初值 60，每 tick 减 1，收到 `player_loaded` 清零，所以最迟 60 tick 后放行 | **已做**。`player_loaded` 会进世界线程 |
| A6 | `updateAwaitingTeleport()` 为 true → **只更新朝向，丢弃位置**，结束 | **已做**。超过 20 tick 用新 id 重发 |
| B1 | x、z clamp 到 ±3.0E7，y clamp 到 ±2.0E7，yRot、xRot 做 `wrapDegrees` | **必须** |
| B2 | 乘客、睡眠的分支 | 不需要 |
| B3 | 速度检查：`d² - v² > 100 × deltaPackets`（滑翔 300）→ WARN `moved too quickly`，`teleport` 回当前位置，结束。`d²` 是到本 tick 开始时位置（`firstGood`）的距离平方，`v²` 是 `deltaMovement` 长度平方，`deltaPackets` 是本 tick 收到的移动包数，超过 5 按 1 算。由 gamerule `player_movement_check` 控制，默认开，**对创造模式也生效** | **已做**，但上限始终是 100：没有鞘翅 300，游戏规则关不掉，也不跑 `travel` |
| B4 | 根据 `onGround` 变化推断跳跃（`jumpFromGround`） | **已做**。只改下一次速度检查用的 `deltaMovement`，不结算饥饿 |
| B5 | 客户端说着地但服务端脚下没东西 → 最多每 200 tick 补发一次脚下的方块（`forceSendPlayerSupportBlocks`） | **以后**，依赖服务端物理 |
| B6 | `player.move(MoverType.PLAYER, delta)`：服务端用自己的碰撞模拟这段位移 | **已做**。方块碰撞和踏步走 `sweep_movement`，没有实体碰撞和世界边界 |
| B7 | `moved wrongly`：模拟结果和目标的距离平方 > 0.0625 → `fail = true`。**创造和旁观模式跳过这个判断**。Y 分量恒被置 0（条件 `yDist > -0.5 \|\| yDist < 0.5` 永远成立） | **已做** |
| B8 | 接受条件：`noPhysics`，或睡眠，或（`!fail` 或原位置已卡在方块里）且 `isEntityCollidingWithAnythingNew` 为 false。否则 `teleport` 回移动前的位置。**后一项对创造模式也生效** | **已做**，只比较方块。睡眠分支没有 |
| B9 | 接受：`absSnapTo` 写入位置和朝向。`absSnapRotationTo` 还会把 `xRot` clamp 到 ±90，A6 的只更新朝向走的也是它 | **必须** |
| B10 | 更新 `clientIsFloating`，`tickPlayer` 里连续悬空超过 80 tick（按重力缩放）就踢。`mayfly` 为 true 时恒为 false | **已做**。`allow-flight` 固定为 false。没有漂浮和鞘翅豁免 |
| B11 | `ServerChunkCache.move` → `ChunkMap.move` | **必须**，见第 6 节 |
| B12 | `setOnGroundWithMovement`、摔落伤害、`knownMovement`、统计、`lastGood` | `on_ground`、水平碰撞标记、摔落距离和 `lastGood` **已做**。伤害、统计和 `knownMovement` 没做 |

`rot`、`status_only` 包在官方也走完整流程，只是位置取当前值。

`wrapDegrees(a)`：`r = a % 360.0`，`r >= 180` 时减 360，`r < -180` 时加 360。Java 和 Rust 的浮点 `%` 都保留被除数的符号，可以直接照写。

### 5.2 传送和 `awaitingPositionFromClient`

状态（`ServerGamePacketListenerImpl`）：

| 字段 | 初值 | 含义 |
|---|---|---|
| `awaitingTeleport` | 0 | 最新一次传送的 id |
| `awaitingPositionFromClient` | null | 非 null 表示有传送没被确认，值是传送后的服务端位置。官方另一个写入点是起床（`handlePlayerCommand` 的 `STOP_SLEEPING`），M2 没有睡眠 |
| `awaitingTeleportTime` | 0 | 用于重发的 tick 计数 |

**发起传送**（`teleport`）：

1. `awaitingTeleportTime = tickCount`。
2. `awaitingTeleport` 先加 1，等于 `i32::MAX` 时归 0。所以第一次传送的 id 是 1，和 M1 的 `SPAWN_TELEPORT_ID` 一致。
3. 服务端立刻把玩家位置改成目标位置。
4. `awaitingPositionFromClient = 当前位置`。
5. 发 `player_position`。

**等待期间的移动包**（`updateAwaitingTeleport`）：

- 位置被丢弃，朝向照常更新。这是为了丢掉客户端在收到传送之前发出的旧位置。
- 距离上次发送已超过 20 tick 时，用**新的 id** 重发一次。只有收到移动包时才检查，不是定时器。

等待期间官方还会拒绝方块放置：`handleUseItemOn` 要求 `awaitingPositionFromClient == null`。

**收到 `accept_teleportation`**（`handleAcceptTeleportPacket`）：

| 情况 | 处理 |
|---|---|
| `id != awaitingTeleport` | 静默忽略。重发之后，旧 id 的确认就走这里，位置继续被挡住 |
| `id` 相等，但 `awaitingPositionFromClient` 为 null，或值非法（同 A1） | 断开，`invalid_player_movement`。包括重复确认，以及还没传送过就发 `id=0` |
| `id` 相等且合法 | 把玩家放到 `awaitingPositionFromClient`，清空它，再以包里的位置和朝向调用 `handlePlayerPositionChange(..., onGround=false, horizontalCollision=false)` |

确认包不设置 `receivedPositionThisTick`，也不受 `hasClientLoaded` 限制。

**Bamboo：** 进服传送和移动校验的拉回都走这套状态。等待确认时丢掉位置包，超过 20 tick 用新 id 重发。没有这道门，路上的旧位置包会把玩家拉回传送前的地方。

### 5.3 Bamboo 的处理流程

收到四种移动包时走 5.1 的顺序：非法值断开，同一客户端 tick 的第二个位置包断开，没加载完就忽略，等待传送确认时丢掉位置。然后做速度检查、方块碰撞和穿入新方块的检查。不通过就 `teleport` 回拉回点，通过才写入客户端给的位置。区块跟踪仍按第 6 节，用的是这次接受之后的位置。

收到 `accept_teleportation`：按上表处理。相等且合法时，先放到等待中的位置，再以包里的坐标走一遍 5.1，`on_ground` 和水平碰撞都是 false。

还没接上的官方分支：鞘翅、漂浮、激流、睡觉、骑乘、击退宽限、切维度、单人房主、`player_movement_check` 关闭、`LivingEntity.travel`、实体碰撞、世界边界、`Block.fallOn`、压力板和绊线的 `entityInside`、`forceSendPlayerSupportBlocks`、终末之诗。摔落距离只记录，不扣血。`allow-flight` 没有配置项，按官方默认 false。

区块坐标：`chunk_x = (x.floor() as i32) >> 4`，z 相同。来源：`Entity.setPosRaw`（`Mth.floor`）和 `SectionPos.blockToSectionCoord`。`>>` 是算术右移，所以 `x = -0.5` 落在区块 -1。clamp 之后的值都在 `i32` 范围内。

---

## 6. 区块跟踪

### 6.1 两个集合

来源：`ChunkTrackingView.isWithinDistance`（字节码已核对）。

```text
within(c, vd, x, z, b):
    dx = max(0, |x - c.x| - b)
    dz = max(0, |z - c.z| - b)
    return dx*dx + dz*dz < vd*vd      // 用 64 位整数

T(c, vd) = { (x, z) : within(c, vd, x, z, 2) }   // contains(x, z)，默认 includeNeighbors = true
R(c, vd) = { (x, z) : within(c, vd, x, z, 1) }   // isInViewDistance(x, z)
```

- `T` 是服务端跟踪的范围，也就是发给客户端的区块。
- `R` 是客户端的渲染范围（`SectionOcclusionGraph.isInViewDistance`，见 8.5）。
- `T` 正好是 `R` 的每个区块加上它的 8 个邻居。`vd = 2..=32` 全部用脚本验证过，两个集合完全相等。
- `T` 的包围盒是 `[c - vd - 1, c + vd + 1]`（`Positioned.minX` 等）。

玩家视距（`ChunkMap.getPlayerViewDistance`）：

```text
vd = clamp(requested_view_distance, 2, server_view_distance)
```

`server_view_distance` 本身被限制在 2..=32（`setServerViewDistance`）。**`vd` 不能超过 `login` 包里的 `view_distance`**，否则区块可能落到客户端范围外（见 8.1）。

`vd = 2` 的例子。`C` 是中心，`C` 和 `R` 组成 `R(2)`，再加上 `T` 组成 `T(2)`：

```text
.........
.TTTTTTT.
.TRRRRRT.
.TRRRRRT.
.TRRCRRT.
.TRRRRRT.
.TRRRRRT.
.TTTTTTT.
.........
```

| `vd` | \|T\| | \|R\| | 沿轴走一格：进入 = 离开 | 斜走一格：进入 = 离开 |
|---|---|---|---|---|
| 2 | 49 | 25 | 7 | 13 |
| 3 | 81 | 49 | 9 | 17 |
| 4 | 117 | 77 | 11 | 19 |
| 6 | 213 | 157 | 15 | 25 |
| 8 | 329 | 257 | 19 | 31 |
| 10 | 473 | 385 | 23 | 37 |
| 12 | 637 | 533 | 27 | 41 |
| 16 | 1057 | 921 | 35 | 53 |
| 32 | 3725 | 3461 | 67 | 99 |

### 6.2 什么时候更新

官方有两个入口：

- `ChunkMap.move(player)`：每次接受位置后调用。`SectionPos`（含 Y）变了才继续。
- `ChunkMap.tick()`：每 tick 对每个玩家调用一次。这个入口负责处理传送（`teleport` 本身不调用 `move`）和视距变化。

两条路都进 `updateChunkTracking`：`view.center == player.chunkPosition()` 且 `view.viewDistance == vd` 时什么都不做，否则应用新视图。所以只在 Y 方向移动不会产生任何包。

**Bamboo：每 tick 对每个玩家比较一次 `(center, vd)` 就够了。**

### 6.3 怎么更新

来源：`ChunkMap.applyChunkTrackingView`，`ChunkTrackingView.difference`，`PlayerChunkSender.dropChunk`。

顺序不能变：

1. 新视图的中心和旧视图不同（或旧视图为空）→ 发 `set_chunk_cache_center`。
2. 计算差集：
   - 对每个 `T(new) \ T(old)` 的区块：加入 `pending`。
   - 对每个 `T(old) \ T(new)` 的区块：如果在 `pending` 里，从 `pending` 删除，**不发包**。否则发 `forget_level_chunk`。
3. 保存新视图。

细节：

- 官方遍历包围盒的并集，x 外层、z 内层，都是升序，所以 `forget_level_chunk` 按这个顺序发出。
- 两个视图的包围盒不相交时，先全部离开，再全部进入。结果和差集相同。
- `forget_level_chunk` 立刻发送，不走批次，也不受确认数量限制。
- 官方的 `dropChunk` 还要求 `player.isAlive()`。M2 没有死亡，可以不管。
- 进入世界：旧视图为空，发 center，`T` 全部进 `pending`，没有 forget。
- 离开服务器：官方会对所有区块调用 `dropChunk`。玩家已经断开，Bamboo 不需要发。
- 同一个区块离开后再进入，会重新进 `pending` 并完整重发。官方没有迟滞（来回踩边界就来回发）。

### 6.4 跨区块的例子

玩家从 `(15.9, -60, 8)` 走到 `(16.1, -60, 8)`，区块从 `(0, 0)` 变成 `(1, 0)`，`vd = 2`，所有区块都已发送：

```text
o 旧中心，n 新中心，# 不变，- 离开，+ 进入

.........
.-######+
.-######+
.-######+
.-##on##+
.-######+
.-######+
.-######+
.........
```

- 进入：x = 4，z = -3..=3。
- 离开：x = -3，z = -3..=3。

发出的包：

```text
本 tick 处理移动时：
  set_chunk_cache_center(1, 0)          03 60 01 00
  forget_level_chunk(-3, -3)            09 26 FF FF FF FD FF FF FF FD
  forget_level_chunk(-3, -2) ... (-3, 3)
本 tick 末尾：
  chunk_batch_start
  level_chunk_with_light，按到 (1, 0) 的距离排序：
    (4, 0)  (4, ±1)  (4, ±2)  (4, ±3)   // 距离平方 9, 10, 13, 18
  chunk_batch_finished(n)               // 受配额限制，可能分成多批
```

`vd = 8` 时形状是圆角的，进入的 19 个区块不全在 x = 10 这一列：

```text
(6, ±9)  (8, ±8)  (9, ±7)  (9, ±6)  (10, -5..=5)
```

### 6.5 给其他领域的接口

`isChunkTracked(player, x, z) = T.contains(x, z) && !pending.contains(x, z)`（`ChunkMap.isChunkTracked`）。

- 方块更新只发给这些玩家（`ChunkHolder.broadcastChanges` → `getPlayers(pos, false)`）。还在 `pending` 的区块不用发方块更新，发送时已经是最新内容。
- 实体只在其所在区块满足这个条件时才对玩家可见（`ChunkMap.TrackedEntity.updatePlayer`）。

所以 `pending` 必须能被这两个系统查询。

---

## 7. 区块发送：`PlayerChunkSender`

背景：TCP 的发送端如果比接收端快，数据会堆在 socket 缓冲区和进程内的队列里，内存和延迟都会涨。让接收方反过来限制发送方的机制叫背压（backpressure）。批次确认就是这里的背压。

来源：`PlayerChunkSender`（常量已用 `javap -constants` 核对）。

### 7.1 每名玩家的状态

| 字段 | 初值 |
|---|---|
| `pending` | 空集合 |
| `desired_chunks_per_tick` | 9.0 |
| `batch_quota` | 0.0 |
| `unacknowledged_batches` | 0 |
| `max_unacknowledged_batches` | 1 |

常量：`MIN_CHUNKS_PER_TICK = 0.01`，`MAX_CHUNKS_PER_TICK = 64.0`，确认后的未确认批次上限 10。官方的 `desired_chunks_per_tick` 和 `batch_quota` 都是 `float`，Bamboo 用 `f32`。

### 7.2 每 tick 发送一批

每个玩家每 tick 最多一批，在 tick 的最后。

```text
if unacked >= max_unacked: 结束
cap   = max(1.0, desired)
quota = min(quota + desired, cap)     // pending 为空时这一步也会执行
if quota < 1.0: 结束
if pending 为空: 结束
n = floor(quota)
chunks = pending 里离玩家最近的 n 个，按 dx*dx + dz*dz 升序
pending -= chunks
unacked += 1
发 chunk_batch_start
依次发 level_chunk_with_light
发 chunk_batch_finished(chunks.len)
quota -= chunks.len
```

- 距离用玩家**此刻**所在的区块，不是区块进入 `pending` 时的位置。
- 距离相同的区块顺序不重要，官方的顺序取决于 `LongOpenHashSet` 的迭代顺序。
- 官方在这一刻才构造区块包，所以内容一定是最新的。
- 官方的 `memoryConnection` 分支是单人游戏用的，Bamboo 不需要。
- 官方的 `getChunkToSend` 可能返回 null（区块还没准备好）。Bamboo 的平坦世界不会出现。

### 7.3 收到 `chunk_batch_received`

```text
unacked = max(0, unacked - 1)
desired = value 是 NaN ? 0.01 : clamp(value, 0.01, 64.0)
if unacked == 0: quota = 1.0
max_unacked = 10
```

### 7.4 推论

- **第一批最多 9 个。** 官方的 `pending` 只收已经准备好的区块（`ChunkMap.markChunkPendingToSend`），所以可能更少。Bamboo 的 `pending` 进世界时至少有 49 个，第一批一定是 9 个。距离平方是 0、1、1、1、1、2、2、2、2，正好是 3×3。这正是客户端关闭加载画面需要的（见 8.3）。
- **第一批之后一定要等确认。** 第一次确认之前 `max_unacked` 是 1。
- **确认之后最多有 10 批在路上。** 每批最多 64 个，所以未确认的区块上限是 640 个。
- **收不到确认：** 发送停止，区块留在 `pending`。官方没有超时，也不断开，连接存活只靠 keep-alive。`forget_level_chunk` 和 `set_chunk_cache_center` 不受影响。
- **速率：** `desired >= 1` 时每 tick 发 `floor(desired)` 个，配额被 `cap` 截断，小数部分不累积（3.5 就是每 tick 3 个）。`desired < 1` 时攒够 1.0 才发一个，比如 0.5 是每 2 tick 一个。
- **发送不看 `hasClientLoaded`。** 加载期间照常发。

推演：`pending` 足够多，第 3 tick 之前收到一次 `3.5` 的确认。

| tick | 动作 | 发出 | 之后 quota | unacked |
|---|---|---|---|---|
| 1 | quota 0 → 9 | 9 | 0 | 1 |
| 2 | 1 ≥ 1，停 | 0 | 0 | 1 |
| 3 | 确认后 quota = 1，再 → 3.5 | 3 | 0.5 | 1 |
| 4 | quota → 3.5 | 3 | 0.5 | 2 |
| 12 | 一直没有新确认 | 3 | 0.5 | 10 |
| 13 | 10 ≥ 10，停 | 0 | 0.5 | 10 |

### 7.5 客户端怎么算 `desiredChunksPerTick`

来源：`ChunkBatchSizeCalculator`，`ClientPacketListener.handleChunkBatchStart/Finished`。

```text
初值：agg = 2_000_000 ns/区块，w = 1
start：记下时间
finished(n)，n > 0 时：
    sample = clamp((现在 - start) / n, agg / 3, agg * 3)
    agg    = (agg * w + sample) / (w + 1)
    w      = min(49, w + 1)
desired = (float)(7_000_000 / agg)
```

- 初始值对应 3.5，不是服务端的起始值 9.0。
- 单次**样本**被限制在 `agg` 的 1/3 到 3 倍之间，再和旧值加权平均，所以变化是渐进的。第一次确认时 `w = 1`，`agg` 最多变成 2 倍或 2/3。在很快的连接上，前几次确认大约是 5.25、6.75、8.10、9.35、10.51、11.62。最慢的第一次是 1.75。
- `n = 0` 时不更新估算，但仍然发确认。
- 这两个 handler **没有** `ensureRunningOnSameThread`，直接在网络线程执行（`Connection.channelRead0` → `packet.handle`，`handleChunkBatchFinished` 的字节码开头就是计算）。所以样本只反映从收到 start 到收到 finished 的时间，也就是网络和解码，不包含主线程处理区块的时间。确认也不会等主线程，加载画面期间照样很快回来。
- 客户端对批次没有其他要求：`handleLevelChunkWithLight` 不检查是否在批次内。批次纯粹是限速协议。`batchSize` 写错只会让估算变差。

---

## 8. 客户端行为

### 8.1 `ClientChunkCache`：范围和环形存储

来源：`ClientChunkCache`、`ClientChunkCache.Storage`，`ClientLevel` 构造函数，`ClientPacketListener.handleLogin`。

```text
r = max(2, view) + 3                                 // calculateStorageRange
D = 2 * r + 1                                        // Storage.viewRange
index(x, z) = floorMod(z, D) * D + floorMod(x, D)    // 环形数组
inRange(x, z) = |x - cx| <= r && |z - cz| <= r       // 方形
```

- `view` 是 `login` 的 `chunkRadius`，或之后 `set_chunk_cache_radius` 的值。**和客户端自己的渲染距离无关。**
- `(cx, cz)` 初值是 `(0, 0)`，只有 `set_chunk_cache_center` 能改。客户端不会根据玩家位置自己移动它。

`T` 的半径是 `vd + 1`，`vd <= view`，所以至少有 2 圈余量：`vd = 8` 时 `T` 半径 9，`r = 11`，`D = 23`。这个余量让 center 晚到一点时不至于丢区块，但不能依赖它，传送一下就超了。

**收到区块时**（`replaceWithPacketData`）：

| 情况 | 结果 |
|---|---|
| 不在 `inRange` | WARN `Ignoring chunk since it's not in the view range: x, z`，返回 null，区块内容丢弃。服务端不会知道，这里会留下空洞，直到这个区块被再次发送 |
| 槽位空，或槽位里是别的区块 | 新建 `LevelChunk`，`Storage.replace`，旧区块走卸载流程 |
| 槽位里就是这个区块 | 原地更新。所以重发一个已加载的区块是安全的 |

不在范围内时光照部分仍然会执行：`handleLevelChunkWithLight` 无条件排队 `applyLightData`，这一列的光照被启用（`setLightEnabled(true)`），只跳过 `enableChunkLight`。

**中心移动时**：`updateViewCenter` 只写两个字段，不清理任何东西。超出范围的区块留在槽位里，`getChunk` 不再返回它，直到新区块落到同一个槽位把它替换掉。环形存储能自我恢复，但 `forget_level_chunk` 仍然要发，否则客户端会留着这些区块的实体和光照状态。

### 8.2 忘记区块

来源：`ClientPacketListener.handleForgetLevelChunk`、`queueLightRemoval`，`ClientChunkCache.drop`，`ClientLevel.unload`。

1. `drop(pos)`：只有在 `inRange` 内且槽位里就是这个区块时才执行。清空槽位，`chunkCount` 减 1，把区块记入 `removedLoadedChunks`，所有 section 标记为空，然后 `level.unload`：清除方块实体、关闭光照、`entityStorage.stopTicking(pos)`。
2. `debugSubscriber.dropChunk`。
3. `queueLightRemoval`：无论第 1 步是否执行都会排队。关闭这列的光照，清除两种光照数据。

- 忘记一个没发过的区块没有副作用。
- `set_chunk_cache_center` 在 forget 之前发。走一格时，被忘记的区块离新中心最多 `vd + 2` 圈，而 `r >= vd + 3`，都在 `inRange` 内。一次跨 3 格及以上（比如传送）才可能超出范围，这时第 1 步不执行，区块留在槽位里等被覆盖。
- `stopTicking` 只让这个区块里的普通实体停止 tick，**不会移除实体**。`Player.isAlwaysTicking()` 为 true，其他玩家不受影响。移除实体靠服务端的实体同步。
- 区块之后再发过来就是普通的新区块。

### 8.3 进入世界：加载画面

来源：`LevelLoadTracker`，`ClientPacketListener.handleLogin`、`handleGameEvent`、`tick`、`notifyPlayerLoaded`，`LevelRenderer`，`LocalPlayer.tick`、`sendChanges`。

`handleLogin` 把 `clientLoaded` 置为 false，然后打开 `LevelLoadingScreen`。

| 状态 | 怎么进入 | 怎么离开 |
|---|---|---|
| `WaitingForServer` | 收到 `login`。`timeoutAfter = 现在 + 30 秒` | **只有**收到 `game_event` 13。这个状态没有 tick 逻辑，30 秒超时在这里不生效 |
| `WaitingForPlayerChunk` | 收到 `game_event` 13 | 以下任一：超过 `timeoutAfter`（WARN `Timed out while waiting for the client to load chunks, letting the player into the world anyway`）；摄像机 y 超出建筑高度；玩家是旁观者或已死亡；`playerSectionReady` 被置位 |
| `ClientLevelReady` | 上一状态判定就绪 | `现在 >= readyAt + closeDelay`。连接服务器时 `closeDelay = 0`（`ConnectScreen` 用 `new LevelLoadTracker()`） |

就绪后 `ClientPacketListener.tick` 发 `player_loaded`，把 `clientLoaded` 置为 true，画面关闭。

`playerSectionReady` 由渲染线程的回调设置：摄像机所在 section 的 mesh 不是 `UNCOMPILED`，且淡入进度 >= 0.3（`LevelRenderer.isSectionCompiledAndVisible`）。

- 这个 section 非空时，首次编译要求 8 个相邻区块列齐全（见 8.5）。
- 这个 section 全是空气时，`SectionOcclusionGraph.runUpdates` 直接把 mesh 从 `UNCOMPILED` 改成 `EMPTY`，只需要本区块，不需要邻居。

Bamboo 的出生点：脚底 y = -60，眼高 1.62（`Avatar.DEFAULT_EYE_HEIGHT`），摄像机在方块 y = -59，属于 section y = -4（方块 -64..=-49），里面有地表，是非空 section。**所以出生点周围 3×3 必须到齐，否则要等 30 秒超时。** 用 7.2 的算法自然满足。

`clientLoaded` 为 false 时：

- `LocalPlayer.tick` 不执行，没有物理。
- `LocalPlayer.sendChanges` 不执行，不发 `player_input` 和移动包。
- 仍然会发：`accept_teleportation`、`chunk_batch_received`、`client_tick_end`、`keep_alive` 回应。

进入世界后第一个 tick 的包序是 `player_loaded`、（按键变了才有的 `player_input`）、移动包、`client_tick_end`。`LocalPlayer.xLast` 等字段没有初值，所以第一次 `sendPosition` 一定判定 `move`（除非出生点就在原点附近）。M1 的 yaw、pitch 都是 0，第一个移动包是 `move_player_pos`。

进服包序。这里只列和加载画面有关的包，M2 要发的完整顺序以 `chat-join.md` 8.5 为准：

```text
S→C  login, player_abilities, player_position(1), player_info_update, set_time,
     game_event(13), set_chunk_cache_center(0, 0)
S→C  chunk_batch_start, 9 × level_chunk_with_light, chunk_batch_finished(9)
C→S  accept_teleportation(1, ...), client_tick_end × N, chunk_batch_received(d)
S→C  后续批次
C→S  player_loaded, move_player_pos, client_tick_end, ...
```

### 8.4 进入世界后：周围区块还没到

26.3 客户端没有任何“等区块”的保护，服务端必须跟上。

- `LocalPlayer.tick` 只检查 `hasClientLoaded`，不看区块。
- `ClientLevel.hasChunk` 恒为 `true`。所以 `LivingEntity.travelInAir` 里“区块没加载就把 Y 速度固定在 -0.1”的分支在客户端走不到，**重力照常**。
- 碰撞：`Level.getChunkForCollisions` → `getChunk(x, z, FULL, false)` 返回 null，`BlockCollisions` 跳过这个区块，**没有碰撞**。
- 方块读取：`getChunk` 返回 `emptyChunk`（`EmptyLevelChunk`），方块都是 `VOID_AIR`。
- 移动包继续发，服务端看到的是玩家在掉落。

所以不飞行的玩家走进没收到的区块会掉下去。飞行的玩家只是飞进空白。

渲染上，`SectionOcclusionGraph.runUpdates` 遇到未加载的区块就停止扩散，把 section 记入 `sectionsWaitingForChunkLoads`，区块到了再继续。

### 8.5 渲染范围和 8 邻居规则

来源：`LevelExtractor`，`SectionUpdateTracker.hasAllNeighbors`，`SectionOcclusionGraph`，`Options.getEffectiveRenderDistance`。

- 客户端的渲染距离：`min(渲染距离选项, 服务端半径)`。
- 可见性扩散的边界是 `ChunkTrackingView.isInViewDistance`，也就是以**摄像机所在 section**为中心的 `R(vd)`。
- 可见且需要编译的 section，只有满足以下任一条件才会被编译：
  - 已经编译过（`sectionMesh != UNCOMPILED`）；
  - `hasAllNeighbors`：周围 8 个区块列都有（`getChunk(..., false) != null`）且 `lightOnInColumn` 为 true。
- `lightOnInColumn` 要等区块包里的光照被应用：`applyLightData` 在光照队列里执行，每帧处理一部分（`ClientLevel.pollLightUpdates`）。
- 未编译的 section 的 `facesCanSeeEachother` 恒为 false，`smartCull` 开启时（默认开启）可见性扩散穿不过去。
- 除了超出建筑高度的情况，实体只在所在 section 已编译且可见时才渲染（`LevelExtractor.isEntityVisible`）。所以站在 `T` 最外圈的玩家，即使已经发了添加实体的包，也可能看不到。

这就是服务端用 `T` 而不是 `R` 的原因：`T` 把 `R` 里每个区块的 8 邻居都带上了。少发一圈，渲染范围就实际缩小一圈。

---

## 9. Bamboo 分级清单

### 必须

| 项 | 章节 |
|---|---|
| 解码四种移动包和 `chunk_batch_received` | 2.1–2.4、2.9 |
| 不把 `accept_teleportation`、`player_loaded`、`client_tick_end`、`player_input` 当错误 | 2.5–2.8 |
| 拒绝 NaN 和非有限朝向，clamp 位置，`wrapDegrees` 朝向，pitch clamp 到 ±90 | 5.1 A1、B1、B9 |
| 保存位置、朝向、`on_ground` | 5.3 |
| 用 `T(vd)` 计算跟踪范围，`vd` 不超过 `login` 的半径 | 6.1 |
| 每 tick 比较 `(center, vd)`，按 center → forget → pending 的顺序更新 | 6.2、6.3 |
| `forget_level_chunk`，线上 z 在前。不发的话，客户端只能等环形存储被覆盖才清理，实体和光照状态会残留 | 3.2、8.1 |
| 完整移植 `PlayerChunkSender`：初值、配额、确认、最近优先。客户端本身不要求批次，但没有限速时，慢客户端会让服务端的发送队列无限增长 | 第 7 节 |
| 区块发送不等 `player_loaded` | 7.4 |
| 发送时取区块的当前内容 | 第 10 节 |
| 继续发 `game_event` 13 和 `set_chunk_cache_center` | 3.8、8.3 |

### 建议

| 项 | 理由 |
|---|---|
| 完整解码 `accept_teleportation`，实现传送确认门限（`awaiting_position` + id） | M2 暂时用不上，但有任何服务端传送时就是**必须** |
| 用 `client_information` 的视距，`vd = clamp(requested, 2, server)` | 客户端渲染距离小时少发区块。解码这个包本身是**必须**，皮肤外层和主手要用（`entities.md` 第 10 节） |
| 超过 20 tick 未确认时重发传送 | 只有 Bamboo 主动传送时才用得上，几行代码 |
| 配置 `view_distance` 限制在 2..=32 | 官方是 clamp，不是报错。不做的话 Rust 的 `clamp` 可能 panic，见第 10 节 |

### 以后

| 项 | 依赖 |
|---|---|
| 速度检查（B3）和回滚 | 对创造模式也生效，加的时候要先有 5.2 的完整流程 |
| 同一客户端 tick 两个位置包就断开（A2） | `client_tick_end` |
| `hasClientLoaded` 门限和 60 tick 超时（A5） | 只防改过的客户端 |
| 服务端碰撞、防穿墙（B6、B8）、补发脚下方块（B5） | 方块碰撞形状 |
| 悬空踢出、摔落伤害、统计 | 生存模式 |
| `knownMovement` | 弹射物初速、拴绳、横扫攻击判定才用 |
| `set_chunk_cache_radius` | 运行时改视距 |
| 合并每 tick 的写入（官方的 `suspendFlushing`） | 性能 |
| 减小区块包：M1 每个满亮度 section 都写 2048 字节的天空光 | 性能 |
| 重发区块时带上方块光 | 服务端方块光计算，见第 10 节 |

### 不需要

载具、睡眠、`wonGame`、`memoryConnection` 分支、`debugSynchronizers`、路径点（waypoint）。

---

## 10. 对 M1 代码的影响

| 位置 | 变化 |
|---|---|
| `mc-bamboo-protocol/src/codec.rs` | `Reader` 要加 `f32`、`f64`、`i8` |
| `packet/play.rs` `PlayRequest` | 加四种移动包和 `ChunkBatchReceived`，`AcceptTeleportation` 带全部字段（**建议**项）。带浮点后要去掉 `Eq` 派生 |
| `packet/play.rs` | 新增 `ForgetLevelChunk` |
| `packet/configuration.rs` `ConfigurationRequest` | 解码 `client_information`（**必须**，见 `entities.md` 第 10 节），把视距带进 Play。现在归入 `Other` |
| `connection/play.rs` | `send_chunks`、`chunks_around`、`CHUNKS_PER_BATCH`、`CHUNK_SEND_TIMEOUT` 和 `ConnectionError::ChunkSendTimeout` 都被第 6、7 节取代 |
| `connection/play.rs` | `SPAWN_TELEPORT_ID` 要进入玩家状态，下一次传送 id 从 2 开始 |
| `server.rs` `flat_chunk_body` | 只能用于没改过的区块。被放置或破坏过方块的区块要在发送时重新编码，否则跟随玩家发出去的是旧内容 |
| `chunk_encoding.rs` `encode_chunk` | `block_mask`、`empty_block_mask` 写死为 0，重发的区块不带方块光。`readSectionList` 跳过两个掩码都没置位的 section，`replaceWithPacketData` 和 `enableChunkLight` 不调用 `propagateLightSources`，forget 时 `queueLightRemoval` 又清掉了旧数据。所以玩家放过发光方块的区块，走远再回来后没有方块光，直到附近再触发 `checkBlock`。平坦世界本身没有发光方块，M2 可以接受（第 12 节）。`fluid_count` 和方块实体写死为 0 的问题见 `blocks-inventory.md` 第 11 节 |
| `config.rs` | `view_distance` 没有任何校验，`tests/join.rs` 用的是 1。官方 clamp 到 2..=32（`ChunkMap.setServerViewDistance`）。必须先处理配置，再算 `vd`：`server_view_distance` 小于 2 时，Rust 的 `clamp(2, server)` 因为 min 大于 max 而 panic |
| `tests/join.rs` | `VIEW_DISTANCE = 1`、`CHUNKS = 9` 不再成立：`T(2)` 是 49 个 |
| `tests/common/mod.rs` `read_chunks` | 必须回 `chunk_batch_received`，否则读完第一批 9 个就会一直等下去 |
| 世界线程的出站队列 | 同一连接的包必须保持顺序，center 不能被区块超过。不能在世界线程上阻塞写 socket，否则一个慢客户端会拖慢所有人。队列大小和写超时见 `coverage.md` 第 5 节 |

---

## 11. 测试向量

### `T` 和 `R` 的边界

| 集合 | 属于 | 不属于 |
|---|---|---|
| `T((0,0), 8)` | (9,0) (9,2) (9,3) (9,5) (8,7) (7,7) | (9,6) (8,8) (10,0) (9,9) |
| `R((0,0), 8)` | (8,0) (8,1) (8,4) (7,6) | (8,5) (7,7) (9,0) |

### 集合大小

见 6.1 的表。

### 差集

- `T((0,0),2)` → `T((1,0),2)`：进入 `(4, -3..=3)`，离开 `(-3, -3..=3)`。
- `T((0,0),8)` → `T((1,0),8)`：进入 19 个，见 6.4。

### M1 对照（`vd = 8`）

方形 17×17 = 289 个。缺少 `T` 的 44 个，多出 `T` 之外的 4 个。`R` 的 257 个区块里有 36 个缺邻居。

### `PlayerChunkSender`

见 7.4 的表。另外：

- 确认值 NaN → `desired = 0.01`。
- 确认值 1000 → `desired = 64`。
- 确认值 0.001 → `desired = 0.01`。
- 没有未确认批次时收到确认：`unacked` 保持 0。
- `pending` 里的区块离开视图：不发 forget。
- 已发送的区块离开视图：发 forget。

### 朝向

| 输入 | 结果 |
|---|---|
| `yRot` 190 | -170 |
| `yRot` -190 | 170 |
| `yRot` 180 | -180 |
| `yRot` 540 | -180 |
| `xRot` 120 | 90 |
| `xRot` 200 | -90（`wrapDegrees` 先得 -160） |

### 解码

第 2 节的字节示例都由脚本生成，可以直接做单元测试。特别注意：

- `accept_teleportation` 的完整帧是 35 字节。
- `flags` 高位有值时不能报错。
- `forget_level_chunk(-3, 5)` 是 `00 00 00 05 FF FF FF FD`。

---

## 12. UNVERIFIED

没有运行过服务端或客户端（任务要求），下面这些都只有源码依据，没有实机观察：

1. **UNVERIFIED：走进未收到的区块时玩家会掉落。** 依据是 `ClientLevel.hasChunk`、`travelInAir` 和 `BlockCollisions`，没有实机观察。
2. **UNVERIFIED：缺一圈时的实际画面。** 编译规则来自源码，但肉眼看到的效果没有看过。
3. **UNVERIFIED：3×3 到齐后加载画面多久关闭。** 还取决于客户端的编译速度和 `chunkSectionFadeInTime` 选项。
4. **UNVERIFIED：`7_000_000` 的含义。** 公式来自字节码。把它理解成“每 tick 花 7 ms 接收区块”是推测，源码里没有说明。
5. **UNVERIFIED：M1 “回头只看到草方块顶面”的原因。** 可能的解释：M1 的外圈缺邻居，不被编译。内侧已编译的区块和外圈相邻的侧面，因为外圈的方块数据存在而被剔除，所以从外面看没有侧面。这和 8.5 的规则吻合，但没有截图，也没有实机验证。M2 用 `T` 发送之后，这个边缘始终在玩家的渲染距离之外。
6. **UNVERIFIED：放过发光方块的区块重发后的观感。** 第 10 节的代码路径来自源码，实际画面没有看过，也不确定相邻区块的 `checkBlock` 能恢复多少光照。
