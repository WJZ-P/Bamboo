# M2 规格：方块交互与创造模式物品栏

目标版本：Minecraft Java 26.3，协议 777。读者是实现 `mc-bamboo-protocol`、`mc-bamboo-world`、`mc-bamboo-server` 的 Rust 开发者，读完不需要再看 Java。

## 0. 约定与证据

- 行为结论来自反编译的官方源码，文中用“类.方法”标出处。有疑问的控制流用 `javap -c` 核对过。
- 包 ID 来自 `packets.json`，已生成到 `mc_bamboo_data::packets::play`。
- 标 **[实测字节]** 的十六进制不是手算的：它们由官方 `STREAM_CODEC` 编码得到。方法是写了两个只读程序，只加载 `target/vanilla` 里的官方 jar、调用 codec 和注册表，没有启动服务端或客户端，没有改动仓库。这是任务列出的三种来源之外的补充手段，结果都和源码一致。第 6、9、10 章的方块和物品数量也来自它们的输出。程序和输出是临时文件：
  - 这些只读程序是开发时的临时工具，没有收进仓库。
- 字节例子**不含帧长度和包 ID**。多字节整数和浮点数都是大端。
- 字段类型：`VarInt`、`VarLong`、`Byte`、`UByte`、`Short`(i16)、`Int`(i32)、`Long`(i64)、`Float`(f32)、`Double`(f64)、`Boolean`(1 字节，0/1)。
- 没能从来源确认的内容标 **UNVERIFIED**，汇总在第 12 章。

## 1. 要点速览

1. **每个 sequence 都要 ack，拒绝时也要。** 否则客户端的预测永远不结束，这些位置之后收到的方块更新也不会生效（第 4 章）。
2. **同一 tick 内的顺序：处理入站包 → 广播方块变化 → 发 `block_changed_ack`。** 方块更新必须先于 ack 到达。ack 每 tick 最多一个，取最大的 sequence。
3. **创造模式破坏是一步完成的：** 收到 `START_DESTROY_BLOCK` 就破坏。没有进度、没有掉落、不发 `block_destruction`。
4. `level_event` 2001（破坏粒子和声音）发给**发起者以外**的人。发起者已经在本地预测过。
5. 放置目标：被点方块可替换时放在点击位置，否则放在 `pos + face`。可替换集合就是已生成的 block tag `minecraft:replaceable`（30 个方块）。
6. `use_item_on` 只要通过前置检查，结束时一律把 `pos` 和 `pos + face` 的当前状态发给发起者，放置失败也发。
7. `Direction` 有两种编码：`player_action` 是 1 字节 `UByte`，`use_item_on` 是 `VarInt`。
8. 创造模式物品栏由客户端决定，服务端只镜像，不回显。服务端自己改了物品栏才发包。
9. `set_creative_mode_slot` 的组件每项都有长度前缀，可以不解析。帧在物品之后结束，读到 item id 后可以直接丢弃剩余字节。其他包里的 `ItemStack` 没有长度前缀。
10. `registries.json` 已有物品 ID。`BlockItem` 的 物品→方块 映射不在任何报告里，要扩展 `Extractor.java`。
11. 挥手现在走新包 `swing_animation`（123）。`animate` 只剩 WAKE_UP、CRITICAL_HIT、MAGIC_CRITICAL_HIT，不要用它。serverbound 的挥手包叫 `punch`（46），没有 `swing` 这个包。
12. 总用默认状态时，1141 种方块物品里放下时状态和官方一致的只有 292 种，再加上特定条件下碰巧一致的 41 种（第 10 章）。

实现优先级：

| 级别 | 内容 |
|---|---|
| P0，不做就不能玩 | `player_action`（破坏、Q、F），`use_item_on`，`use_item`（只 ack），`set_carried_item`，`set_creative_mode_slot`，`block_update`，`block_changed_ack`，`level_event`，`container_set_slot`，物品→方块映射，放置的实体阻挡检查 |
| P1，多人和手感 | `punch` + `swing_animation`，`set_equipment`，`pick_item_from_block` + `set_held_slot`，`player_input` 的 shift，第 7 章的表，区块里的方块实体列表（第 11 章），`update_attributes`（进服时发给自己一次，`chat-join.md` 8.4） |
| P2，锦上添花 | `section_blocks_update`，`sound`，`system_chat` 提示，`container_set_content` |

## 2. 基础编码

### 2.1 BlockPos

一个 `Long`，从高位到低位是 `x(26) | z(26) | y(12)`，都是有符号数的补码截断。来源：`BlockPos.asLong`、`BlockPos.of`、`FriendlyByteBuf.writeBlockPos`。[实测常量：水平 26 位，Y 12 位]

| 字段 | 位 | 掩码 | 解码 |
|---|---|---|---|
| x | 63..38 | `0x3FFFFFF` | `v >> 38` |
| z | 37..12 | `0x3FFFFFF` | `v << 26 >> 38` |
| y | 11..0 | `0xFFF` | `v << 52 >> 52` |

```rust
fn pack(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & 0x3FF_FFFF) << 38) | ((z as i64 & 0x3FF_FFFF) << 12) | (y as i64 & 0xFFF)
}
// v 是 i64，>> 是算术右移，所以符号位自动扩展。
fn unpack(v: i64) -> (i32, i32, i32) {
    ((v >> 38) as i32, (v << 52 >> 52) as i32, (v << 26 >> 38) as i32)
}
```

例子 (18, -61, -7) [实测字节]：

```text
x = 18        -> 0x12      << 38 = 0x0000_0480_0000_0000
z = -7        -> 0x3FFFFF9 << 12 = 0x0000_003F_FFFF_9000
y = -61       -> 0xFC3           = 0x0000_0000_0000_0FC3
合计 0x0000_04BF_FFFF_9FC3，线上 00 00 04 BF FF FF 9F C3
```

第二个例子 (-1, -64, 1) = `FF FF FF C0 00 00 1F C0`。[实测字节]

### 2.2 Direction

来源：`Direction` 枚举。线上的 id 是 `get3DDataValue()`。

| id | 名字 | 法向 (dx,dy,dz) |
|---|---|---|
| 0 | DOWN | 0,-1,0 |
| 1 | UP | 0,1,0 |
| 2 | NORTH | 0,0,-1 |
| 3 | SOUTH | 0,0,1 |
| 4 | WEST | -1,0,0 |
| 5 | EAST | 1,0,0 |

两种编码：

- `player_action`：`UByte`，解码是 `BY_3D_DATA[abs(v % 6)]`。
- `use_item_on`：`VarInt`，`Direction.STREAM_CODEC`，越界时取模回绕（`OutOfBoundsStrategy.WRAP`）。

### 2.3 InteractionHand

`VarInt`：0 `MAIN_HAND`，1 `OFF_HAND`，越界按 0。来源：`InteractionHand.STREAM_CODEC`。

### 2.4 SectionPos 和段内坐标

`SectionPos` 是一个 `Long`，从高位到低位是 `x(22) | z(22) | y(20)`，单位是区块段。来源：`SectionPos.asLong`。

```rust
fn pack_section(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & 0x3F_FFFF) << 42) | ((z as i64 & 0x3F_FFFF) << 20) | (y as i64 & 0xF_FFFF)
}
```

段内坐标是 12 位：`local_x << 8 | local_z << 4 | local_y`，每个分量是世界坐标 `& 15`。注意顺序是 x、z、y。来源：`SectionPos.sectionRelativePos`。

### 2.5 ItemStack 的两种线上格式

`ItemStack.OPTIONAL_STREAM_CODEC`（trusted）用于本文所有 clientbound 包。`ItemStack.OPTIONAL_UNTRUSTED_STREAM_CODEC`（delimited）只用于 serverbound 的 `set_creative_mode_slot`。两者的差别只有每个组件值前面的长度。来源：`ItemStack.createOptionalStreamCodec`、`DataComponentPatch.createStreamCodec`。

| 字段 | 编码 | 说明 |
|---|---|---|
| count | VarInt | `<= 0` 表示空物品，**后面什么都没有**。26.x 没有 present 布尔值 |
| item | VarInt | `minecraft:item` 注册表 ID，不加一 |
| added | VarInt | 增加或覆盖的组件数量 |
| removed | VarInt | 移除的组件数量 |
| added 项 × added | | trusted：`type VarInt` + 值。delimited：`type VarInt` + `len VarInt` + `len` 字节 |
| removed 项 × removed | VarInt | 组件类型 ID |

`type` 是 `minecraft:data_component_type` 注册表 ID。值的字节在两种格式里相同，都由该组件类型的 `streamCodec()` 写出。delimited 只是在外面包一层长度（`ByteBufCodecs.registryFriendlyLengthPrefixed`）。

例子 [实测字节]：

| 内容 | 字节 |
|---|---|
| 空物品（两种格式相同） | `00` |
| 64 个 stone（两种格式相同） | `40 01 00 00` |
| 2 个 stone，`max_stack_size`=16，移除 `rarity`，delimited | `02 01 01 01 01 01 10 0C` |
| 同一物品，trusted | `02 01 01 01 01 10 0C` |

第三行逐字段：count 2，item 1，added 1，removed 1，type 1（`max_stack_size`），len 1，值 `10`，removed type 12（`rarity`）。第四行只是去掉了 `len`。

**能否跳过组件：** 能，理由有三条。

- `set_creative_mode_slot` 的字段只有 slot 和物品，物品后面没有字段（`ServerboundSetCreativeModeSlotPacket.STREAM_CODEC`）。
- 帧有长度前缀，不解析剩余字节不会让流错位。
- 就算要解析，delimited 格式也不需要认识任何组件。

调用方不能再调 `Reader::finish()`，它会报 `TrailingBytes`。

**跳过的代价：** 服务端丢了组件，以后把这个槽发回客户端时物品就变样了。创造模式物品栏里的药水、附魔书、烟花等都带组件。所以建议把组件当不透明字节保留（第 8.3 节）。

把保留的 delimited 组件转成 trusted：逐项去掉 `len`，不需要懂内容。

**不要把别人发来的组件字节转发给其他玩家。** 服务端没有校验过这些字节，内容非法时其他玩家的客户端解码失败，会被断开。官方服务端解码再重新编码，所以没有这个问题。给别的玩家发 `set_equipment` 时用空组件（`added=0, removed=0`）。原样发回给本人只影响本人。

校验规则（`ItemStack.validatedStreamCodec`、`ItemStack.MAP_CODEC`）：物品必须是已注册 ID，`count` 在 1..=99。官方解码失败时断开连接，Bamboo 可以直接忽略这个包。`item` 为 0（`minecraft:air`）时 `ItemStack.isEmpty()` 为真，按空物品处理。

## 3. 数据包

### 3.1 Serverbound

#### `minecraft:player_action`，serverbound，41 (0x29)

| 字段 | 编码 | 说明 |
|---|---|---|
| action | VarInt | 枚举序号，见下表，越界是畸形包 |
| pos | Long，BlockPos | 非方块动作是 (0,0,0) |
| face | UByte | Direction id，非方块动作是 0 |
| sequence | VarInt | 只有预测动作有真实值，其余是 0 |

来源：`ServerboundPlayerActionPacket.write`，处理在 `ServerGamePacketListenerImpl.handlePlayerAction`。例子 `START_DESTROY_BLOCK`，(8,-61,8)，UP，seq 1 [实测字节]：`00 00 00 02 00 00 00 8F C3 01 01`。

| 值 | action | 创造模式下何时发 | M2 处理 |
|---|---|---|---|
| 0 | START_DESTROY_BLOCK | 左键、按住时约每 6 tick 一次 | 第 5 章 |
| 1 | CHANGE_DESTROY_DIRECTION | 不发 | 只记 ack |
| 2 | ABORT_DESTROY_BLOCK | 不发 | 只记 ack |
| 3 | STOP_DESTROY_BLOCK | 不发 | 只记 ack |
| 4 | DROP_ALL_ITEMS | Ctrl+Q | 清空手持槽，见 8.3 |
| 5 | DROP_ITEM | Q | 手持槽 count 减 1，见 8.3 |
| 6 | RELEASE_USE_ITEM | 松开使用键 | 忽略 |
| 7 | SWAP_ITEM_WITH_OFFHAND | F | 交换主副手，见 8.3 |
| 8 | STAB | 手持矛攻击 | 忽略 |

“创造模式不发”的依据：`MultiPlayerGameMode.startDestroyBlock` 的 instabuild 分支不会设置 `isDestroying`，所以 `stopDestroyBlock` 不发包，也走不到 `continueDestroyBlock` 的进度分支。

`handlePlayerAction` 只对 0..=3 调用 `ackBlockChangesUpTo`，其余动作不 ack。

#### `minecraft:use_item_on`，serverbound，66 (0x42)

| 字段 | 编码 | 说明 |
|---|---|---|
| hand | VarInt | 2.3 |
| pos | Long，BlockPos | 被点击的方块 |
| face | **VarInt** | 被点击的面 |
| cursor_x | Float | 点击点减去 `pos.x`，通常 0..1 |
| cursor_y | Float | 同上 |
| cursor_z | Float | 同上 |
| inside | Boolean | 视线起点在方块内部。只有脚手架放置用到 |
| world_border_hit | Boolean | 命中世界边界。方块交互不读它 |
| sequence | VarInt | |

来源：`ServerboundUseItemOnPacket.STREAM_CODEC`、`BlockHitResult.STREAM_CODEC`，处理在 `handleUseItemOn`。服务端还原的绝对点击点是 `pos + cursor`（double）。

例子：主手，点 (8,-61,8) 顶面中心，seq 2 [实测字节]：

```text
00                          hand
00 00 02 00 00 00 8F C3     pos
01                          face = UP
3F 00 00 00                 0.5
3F 80 00 00                 1.0
3F 00 00 00                 0.5
00 00                       inside, world_border_hit
02                          sequence
```

处理：第 6 章。

#### `minecraft:use_item`，serverbound，67 (0x43)

| 字段 | 编码 | 说明 |
|---|---|---|
| hand | VarInt | |
| sequence | VarInt | 注意在 yaw 之前 |
| yaw | Float | `yRot` |
| pitch | Float | `xRot` |

来源：`ServerboundUseItemPacket.STREAM_CODEC`，处理在 `handleUseItem`。例子：副手，seq 3，yaw 90，pitch -15.5 [实测字节]：`01 03 42 B4 00 00 C1 78 00 00`。

处理：记 ack。M2 的物品没有“对空气使用”的效果，其余忽略。客户端只要手持非空就发这个包，不管物品有没有使用效果，每次都消耗一个 sequence（`Minecraft.startUseItem`、`MultiPlayerGameMode.useItem`）。官方 `handleUseItem` 还会用包里的角度修正玩家朝向。

一次右键最多产生 4 个包：`use_item_on(主手)`、`use_item(主手)`、`use_item_on(副手)`、`use_item(副手)`，每个有自己的 sequence。`use_item_on` 得到 `Success` 或 `Fail`，或者 `use_item` 得到 `Success` 时，客户端停止循环。来源：`Minecraft.startUseItem`。

#### `minecraft:set_carried_item`，serverbound，54 (0x36)

| 字段 | 编码 | 说明 |
|---|---|---|
| slot | Short | 快捷栏下标 0..=8 |

来源：`ServerboundSetCarriedItemPacket.write`。例子 slot 4 [实测字节]：`00 04`。

处理：范围内就更新 `selected`，范围外记日志并忽略（`handleSetCarriedItem`）。不回包。

客户端只在选中槽变化时发（`MultiPlayerGameMode.ensureHasSentCarriedItem`），它记录的初值是 0，所以进服后不选其他槽就不会发。服务端初值也必须是 0。`use_item_on`、`use_item`、攻击、丢弃之前客户端都会先调用它，所以这些包到达时 `selected` 已是最新。`startDestroyBlock` 不调用，但每个客户端 tick 也会调一次。

#### `minecraft:set_creative_mode_slot`，serverbound，57 (0x39)

| 字段 | 编码 | 说明 |
|---|---|---|
| slot | Short | `InventoryMenu` 槽位，见 8.1。负数表示丢弃，客户端用 -1 |
| stack | ItemStack delimited | 2.5 |

来源：`ServerboundSetCreativeModeSlotPacket.STREAM_CODEC`。例子 [实测字节]：

| 内容 | 字节 |
|---|---|
| 槽 36 放 64 个 stone | `00 24 40 01 00 00` |
| 清空槽 36 | `00 24 00` |
| 丢弃 1 个 stone | `FF FF 01 01 00 00` |
| 槽 37，带组件 | `00 25 02 01 01 01 01 01 10 0C` |

处理：第 8.3 节。

#### `minecraft:pick_item_from_block`，serverbound，36 (0x24)

| 字段 | 编码 | 说明 |
|---|---|---|
| pos | Long，BlockPos | |
| include_data | Boolean | Ctrl+中键。M2 忽略 |

来源：`ServerboundPickItemFromBlockPacket.STREAM_CODEC`。例子 [实测字节]：`00 00 02 00 00 00 8F C3 00`。

客户端不做任何预测，只发这个包，所以服务端不处理时中键就没有效果。处理：第 8.4 节。

#### `minecraft:punch`，serverbound，46 (0x2E)

没有字段。来源：`ServerboundPunchPacket.STREAM_CODEC`（`StreamCodec.unit`）。

客户端每次左键挥手时发：`startAttack` 发一次（破坏方块、打空气、打实体都发，手持矛时不发），按住并瞄准非空气方块时**每个客户端 tick** 都发（`continueAttack`）。

处理（`handlePunch`）：让其他玩家看到挥手，发 `swing_animation` 给追踪者，不发给本人。官方由 `LivingEntity.SwingState.startIfAble` 限流：上一次挥手后第 1..=`duration/2` tick 内再次请求会被拒绝，默认 duration 6，也就是第 1、2、3 tick 拒绝。M2 不用做这个限流，和 `entities.md` 第 9 节一致：按住左键时每秒向每个追踪者多发约 20 个包，多发不会出错，收到的客户端自己也走 `startIfAble`（`ClientPacketListener.handleSwingAnimation`）。以后作为性能项再做。

#### 其他相关的包

| 包 | ID | 编码 | 处理 |
|---|---|---|---|
| `minecraft:player_input` | 43 (0x2B) | 1 字节位掩码，`0x20` 是 shift | 更新 `sneaking`（`handlePlayerInput`），第 7 章要用 |
| `minecraft:container_close` | 19 (0x13) | VarInt 容器 ID | 关创造模式界面时发，id 是 0，忽略 |
| `minecraft:pick_item_from_entity` | 37 (0x25) | `entity VarInt`，`include_data Boolean` | 中键点其他玩家时发。`Entity.getPickResult` 默认 null，`Player` 没有覆盖，官方什么都不做，忽略 |

创造模式不发 `container_click`（18）。`CreativeModeInventoryScreen.slotClicked` 覆盖了父类方法，不走 `handleContainerInput`。

### 3.2 Clientbound

#### `minecraft:block_update`，clientbound，8 (0x08)

| 字段 | 编码 | 说明 |
|---|---|---|
| pos | Long，BlockPos | |
| state | VarInt | 方块状态 ID（`blocks.json` 的 `states[].id`） |

来源：`ClientboundBlockUpdatePacket.STREAM_CODEC`。例子 (8,-60,8) = stone(1) [实测字节]：`00 00 02 00 00 00 8F C4 01`。oak_log 默认状态 140：`00 00 02 00 00 00 8F C4 8C 01`。

状态 ID 非法时客户端抛异常（`idMapper` 的 `byIdOrThrow`）。方块在客户端未加载的区块里时无害：`EmptyLevelChunk.setBlockState` 返回 null，更新被丢弃。

#### `minecraft:section_blocks_update`，clientbound，86 (0x56)

| 字段 | 编码 | 说明 |
|---|---|---|
| section | Long，SectionPos | 2.4 |
| count | VarInt | 条目数 |
| entries × count | VarLong | `state_id << 12 \| local`，`local` 见 2.4 |

来源：`ClientboundSectionBlocksUpdatePacket.write`。

例子：段 (1,-4,-1)，把世界坐标 (18,-61,-7) 设成 stone(1)，把 (31,-49,-1) 设成 oak_slab 默认状态(15180)。这段字节交给官方 codec 解码后得到的正是这两个更新 [实测字节]：

```text
00 00 07 FF FF FF FF FC     SectionPos (1,-4,-1) = 0x7FFFFFFFFFC
02                          count
93 25                       (1 << 12) | 0x293，0x293 = x2 z9 y3
FF 9F D3 1D                 (15180 << 12) | 0xFFF
```

官方的选择规则（`ChunkHolder.broadcastChanges`）：同一段本 tick 只改了 1 个方块时发 `block_update`，2 个及以上发这个包。客户端两种都接受，M2 可以先只发 `block_update`。

#### `minecraft:block_changed_ack`，clientbound，4 (0x04)

| 字段 | 编码 | 说明 |
|---|---|---|
| sequence | VarInt | 确认到此为止的所有预测 |

来源：`ClientboundBlockChangedAckPacket.write`。例子 300 [实测字节]：`AC 02`。

#### `minecraft:level_event`，clientbound，47 (0x2F)

| 字段 | 编码 | 说明 |
|---|---|---|
| type | Int | 破坏方块用 2001（`LevelEvent.PARTICLES_AND_SOUND_DESTROY_BLOCK`） |
| pos | Long，BlockPos | |
| data | Int | 2001 时是被破坏方块的状态 ID |
| global | Boolean | false |

来源：`ClientboundLevelEventPacket.write`。例子：破坏 (8,-61,8) 的 grass_block，状态 9 [实测字节]：`00 00 07 D1 00 00 02 00 00 00 8F C3 00 00 00 09 00`。

客户端收到 2001 时自己播放破坏音效和粒子，音效、音量、音调都由状态 ID 推出（`LevelEventHandler`）。所以破坏不需要 `sound` 包，也不需要声音表。

#### `minecraft:block_destruction`，clientbound，5 (0x05)

| 字段 | 编码 | 说明 |
|---|---|---|
| id | VarInt | 破坏者的实体 ID |
| pos | Long，BlockPos | |
| progress | UByte | 0..=9 显示裂纹，其他值清除。官方用 -1，线上是 `FF` |

来源：`ClientboundBlockDestructionPacket.write`、`ClientLevel.destroyBlockProgress`。例子 [实测字节]：`07 00 00 02 00 00 00 8F C3 FF`。

**创造模式下用不到。** `handleBlockBreakAction` 的 instabuild 分支在进度逻辑之前就返回了。

#### `minecraft:set_held_slot`，clientbound，107 (0x6B)

| 字段 | 编码 | 说明 |
|---|---|---|
| slot | VarInt | 0..=8，范围外客户端忽略 |

来源：`ClientboundSetHeldSlotPacket.STREAM_CODEC`、`ClientPacketListener.handleSetHeldSlot`。例子 3 [实测字节]：`03`。注意 serverbound 版本是 `Short`，这里是 `VarInt`。

客户端切换后会用 `set_carried_item` 回发同样的值，服务端重复设置即可。

#### `minecraft:container_set_slot`，clientbound，20 (0x14)

| 字段 | 编码 | 说明 |
|---|---|---|
| container_id | VarInt | 玩家物品栏是 0 |
| state_id | VarInt | 见 8.3 |
| slot | Short | `InventoryMenu` 槽位 |
| stack | ItemStack trusted | |

来源：`ClientboundContainerSetSlotPacket.write`、`FriendlyByteBuf.writeContainerId`（26.3 是 VarInt）。例子：槽 36，1 个 stone，state 2 [实测字节]：`00 02 00 24 01 01 00 00`。

#### `minecraft:container_set_content`，clientbound，18 (0x12)

| 字段 | 编码 | 说明 |
|---|---|---|
| container_id | VarInt | 0 |
| state_id | VarInt | |
| count | VarInt | 槽位数。玩家物品栏是 46，不能超过，客户端按下标写槽 |
| items × count | ItemStack trusted | 下标就是 `InventoryMenu` 槽位 |
| carried | ItemStack trusted | 光标上的物品，空物品是 `00` |

来源：`ClientboundContainerSetContentPacket.STREAM_CODEC`、`AbstractContainerMenu.initializeContents`。

例子：只有槽 36 有 64 个 stone，state 1 [实测字节]：`00 01 2E`，接 36 个 `00`，接 `40 01 00 00`，接 9 个 `00`，再接 carried `00`。全空时就是 `00 01 2E` 加 47 个 `00`。

#### 其他物品栏包（M2 不需要）

| 包 | ID | 字段 | 说明 |
|---|---|---|---|
| `minecraft:set_player_inventory` | 110 (0x6E) | `slot VarInt`，`stack` trusted | `slot` 是 `Inventory` 下标（0..=35、36..=39 盔甲、40 副手），不是菜单槽位。例子 [实测字节] `28 01 01 00 00` |
| `minecraft:set_cursor_item` | 98 (0x62) | `stack` trusted | 创造模式界面打开时客户端忽略（`handleSetCursorItem`）。例子 `00` |

#### `minecraft:swing_animation`，clientbound，123 (0x7B)

| 字段 | 编码 | 说明 |
|---|---|---|
| entity_id | VarInt | |
| hand | VarInt | 2.3 |
| type | VarInt | `SwingAnimationType`：0 NONE，1 WHACK，2 STAB |
| duration | VarInt | tick |

来源：`ClientboundSwingAnimationPacket.STREAM_CODEC`、`SwingAnimation.STREAM_CODEC`。默认值是 WHACK、6。物品可以用 `attack_animation`、`interact_animation` 组件覆盖，报告里只有 `*_spear` 的 `attack_animation` 不是默认值。例子 entity 5，主手，默认 [实测字节]：`05 00 01 06`。

#### `minecraft:set_equipment`，clientbound，104 (0x68)

| 字段 | 编码 | 说明 |
|---|---|---|
| entity_id | VarInt | |
| 重复：slot | Byte | 低 7 位是 `EquipmentSlot.ordinal()`，最高位 `0x80` 表示后面还有一项 |
| 重复：stack | ItemStack trusted | |

来源：`ClientboundSetEquipmentPacket.write`。列表不能为空：解码是 do-while，至少读一项，官方空列表时不发包。

**用 ordinal，不是 `EquipmentSlot.id`：** MAINHAND 0，OFFHAND 1，FEET 2，LEGS 3，CHEST 4，HEAD 5，BODY 6，SADDLE 7。

例子：实体 5，主手 1 个 stone，副手空 [实测字节]：`05 80 01 01 00 00 01 00`。

官方每 tick 比较装备变化，只发给追踪者，不发给本人（`LivingEntity.updatePlayersWithNewEquipment`）。开始追踪时发一次全部非空槽（`ServerEntity.sendPairingData`）。主副手刚好互换时官方发 `entity_event` 55 代替，Bamboo 直接发两个槽的 `set_equipment` 效果相同。

#### `minecraft:sound`，clientbound，119 (0x77)

只有放置音效要用，优先级低。

| 字段 | 编码 | 说明 |
|---|---|---|
| sound | VarInt | `sound_event` 注册表 ID **加一**。0 表示内联定义，M2 不用 |
| source | VarInt | `SoundSource` 序号，BLOCKS 是 4 |
| x, y, z | Int ×3 | `(int)(坐标 * 8.0)`，Java 强转，向零截断 |
| volume | Float | |
| pitch | Float | |
| seed | Long | 任意 |

来源：`ClientboundSoundPacket.write`、`ByteBufCodecs.holder`。

例子 `block.stone.place`（ID 1605），方块中心 (8.5,-59.5,8.5)，音量 1.0，音调 0.8，seed 123456789 [实测字节]：

`C6 0C 04 00 00 00 44 FF FF FE 24 00 00 00 44 3F 80 00 00 3F 4C CC CD 00 00 00 00 07 5B CD 15`

#### `minecraft:system_chat`，clientbound，124 (0x7C)

高度限制提示用它。26.3 的 `sendOverlayMessage` 走 `system_chat`，`overlay=true`，不是 `set_action_bar_text`（`ServerPlayer.sendSystemMessage`）。

| 字段 | 编码 | 说明 |
|---|---|---|
| content | NBT，无根名 | 文本组件 |
| overlay | Boolean | true 显示在快捷栏上方 |

例子 `{translate:"build.tooHigh", with:[319], color:"red"}`，overlay [实测字节]：

```text
0A
09 00 04 "with"       03 00 00 00 01  00 00 01 3F
08 00 05 "color"      00 03 "red"
08 00 09 "translate"  00 0D "build.tooHigh"
00
01
```

`with` 是 Int 列表。`mc-bamboo-nbt` 的 `Tag::Compound`、`Tag::List`、`Tag::Int` 够用。低于下限时键是 `build.tooLow`，参数是下限。

#### `minecraft:update_attributes`，clientbound，134 (0x86)

进服时给自己发一次，为了创造模式的 5 格触及距离。完整的两属性例子在 `chat-join.md` 8.4。

Bamboo 不发这个包时，客户端的 `block_interaction_range` 是基础值 4.5，因为 +0.5 的修饰符只在 `ServerPlayer.updatePlayerAttributes` 里加。客户端瞄准时用这个属性（`LocalPlayer.pick`）。

例子 [实测字节]：实体 5，属性 8（`block_interaction_range`），base 4.5，一个修饰符 `minecraft:creative_mode_block_range`，amount 0.5，op 0：

`05 01 08 40 12 00 00 00 00 00 00 01 23 <35 字节 UTF-8 id> 3F E0 00 00 00 00 00 00 00`

字段顺序：`entity VarInt`，`count VarInt`，每项 `attribute VarInt`（不加一），`base Double`，`modifier_count VarInt`，每个修饰符 `id String`、`amount Double`、`operation VarInt`。

## 4. 序号和客户端预测

来源：`BlockStatePredictionHandler`、`ClientLevel`、`MultiPlayerGameMode.startPrediction`、`ServerGamePacketListenerImpl.ackBlockChangesUpTo`。

### 4.1 客户端怎么工作

| 事件 | 客户端行为 |
|---|---|
| 开始预测 | `currentSequenceNr` 先加一，所以第一个值是 1。预测动作里每次成功的 `setBlock` 记录 `pos → (旧状态, sequence, 玩家位置)`。同一 pos 已有记录时只把 sequence 更新成当前值，旧状态不变 |
| 收到 `block_update` / `section_blocks_update` | `setServerVerifiedBlockState`：pos 有记录时**只改记录里的状态，世界不变**。没有记录时立刻写入世界，flags 19 |
| 收到 `block_changed_ack(n)` | `endPredictionsUpTo`：取出所有 `sequence <= n` 的记录。世界里的状态和记录不同就写入记录里的状态。如果玩家因此和方块重叠，并且 `lastTeleportSequence < n`，把玩家拉回记录的位置 |

三个包会开启预测：`player_action` 的破坏动作、`use_item_on`、`use_item`。

预测包括客户端本地跑的全部逻辑。放门时门的上半、放栅栏时邻居的连接状态，都是被记录的位置。

flags 19 = `UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE`。`UPDATE_KNOWN_SHAPE` 让 `Level.setBlock` 跳过邻居形状更新，所以服务端发来的方块更新不会让客户端重算邻居，邻居要靠服务端自己发。

### 4.2 收不到 ack 会怎样

- 预测的方块一直留着。破坏的方块消失，放置的方块出现，这就是幽灵方块。
- 记录没有超时：`serverVerifiedStates` 只在 `endPredictionsUpTo` 里删除，它的唯一调用者是 `handleBlockChangedAck`。
- 这些位置之后收到的 `block_update` 全部不生效，只会存进记录。玩家看到的世界和服务端永久不一致。
- 记录还会累积，每个预测过的位置占一项。
- 之后任何更大的 ack 都能一次清掉所有积压，把世界同步到记录里的状态。

### 4.3 服务端的义务

1. 三个包在处理时都要记 ack，无论结果如何。`handleUseItemOn`、`handleUseItem` 在开头记，`handlePlayerAction` 在破坏逻辑之后记。距离太远、高度越界、物品无效这些提前返回的路径都已经记过了。
2. 官方用一个 `i32` 字段累计（初值 -1）：`ack = max(sequence, ack)`，在连接 tick 里若 `> -1` 就发一个包并重置。sequence 为负时官方抛异常，客户端不会发这种值，Bamboo 可以按畸形包处理。
3. **方块更新先于 ack。** 官方顺序是（`MinecraftServer.processPacketsAndTick`、`tickChildren`、`Connection.tick`）：
   1. `processQueuedPackets`：所有入站包的 handler。
   2. `level.tick` 里的 `ServerChunkCache.broadcastChangedChunks`：区块广播。
   3. `tickConnection` 里的 `ServerGamePacketListenerImpl.tick`：发 ack。

   如果 ack 先到，客户端先回滚到旧状态，再被 `block_update` 改回，方块闪一下，还可能触发上面的拉回玩家。
4. 拒绝操作时不需要额外发 `block_update` 来“修复”预测。ack 本身就会回滚到记录的状态，前提是服务端的状态自发送 chunk 后没变，或者已经发过更新。

## 5. 创造模式破坏方块

来源：`ServerGamePacketListenerImpl.handlePlayerAction`、`ServerPlayerGameMode.handleBlockBreakAction`、`destroyAndAck`、`destroyBlock`。

### 5.1 客户端

`Minecraft.startAttack` → `MultiPlayerGameMode.startDestroyBlock`：`abilities.instabuild` 为真时在预测里调用 `destroyBlock(pos)`，本地播放音效和粒子，写入空气，然后发 `player_action(START_DESTROY_BLOCK)`。接着本地挥手，发 `punch`。

按住左键时 `continueDestroyBlock` 用 `destroyDelay = 5` 限速，每 6 tick 再发一次 `START_DESTROY_BLOCK`，每次是新的 sequence。

手持不能破坏方块的物品时客户端的 `destroyBlock` 不改世界，但**包照发**。

### 5.2 服务端流程

```text
on_player_action(START_DESTROY_BLOCK, pos, face, seq):
    defer: pending_ack = max(pending_ack, seq)            # 所有路径都要
    if !within_reach(player, pos, buffer = 1.0): return   # 不发任何包，靠 ack 回滚
    if pos.y > MAX_Y:
        send_to(player, block_update(pos, world[pos]))
        return
    state = world[pos]                                    # pos.y < MIN_Y 时是 void_air
    if state 是空气: return                               # 见下方说明
    if !can_destroy_in_creative(held_main_item) or is_game_master_block(state):
        send_to(player, block_update(pos, state))
        return
    send_nearby(except = player, radius = 64, level_event(2001, pos, state_id(state), false))
    world[pos] = 空气                                     # 含水方块在官方会留下水
    mark_changed(pos)                                     # 本 tick 结束时广播
```

说明：

- `within_reach` 见 6.1。
- 官方还会依次检查 spawn protection 和世界边界（`mayInteract`）。M2 没有这两样。
- 破坏空气：官方不提前返回，但 `removeBlock` 不产生变化，客户端也忽略空气的 2001 事件，效果等于什么都不做。
- 不能破坏的物品：`Item.canDestroyBlock`，`tool` 组件的 `can_destroy_blocks_in_creative` 为 false 时不能破坏。报告里有 9 个：`wooden_sword`、`stone_sword`、`copper_sword`、`golden_sword`、`iron_sword`、`diamond_sword`、`netherite_sword`、`mace`、`trident`。另外 `debug_stick` 覆盖了这个方法，恒为 false。手持矛时客户端走 `piercingAttack`，根本不发 `START_DESTROY_BLOCK`。
- `GameMasterBlock`（命令方块、结构方块、拼图方块、测试方块）要求 `canUseGameMasterBlocks`，即 instabuild 且有 gamemaster 权限。M2 没有权限系统，等于不能破坏。
- bedrock 没有特殊处理，创造模式能破坏。
- `player.preventsBlockDrops()` 在 instabuild 时为真，所以不掉落，也不扣耐久。

### 5.3 谁收到什么

| 包 | 发起者 | 其他玩家 |
|---|---|---|
| `level_event` 2001 | 不发，客户端已预测 | 发。同维度，`(x-px)²+(y-py)²+(z-pz)² < 64²`，x、y、z 是方块整数坐标，不加 0.5（`ServerLevel.levelEvent`、`PlayerList.broadcast`） |
| `block_update` 或 `section_blocks_update` | 发，tick 中段 | 发。所有已收到该区块的玩家（`ChunkHolder.broadcastChanges`） |
| `block_changed_ack` | 发，tick 末 | 不发 |
| `swing_animation` | 不发 | 追踪者，来自 `punch` |
| `block_destruction` | 不发 | 不发 |
| `sound` | 不发 | 不发，音效在 2001 里 |
| `light_update` | 只有该区块正好在本人追踪范围边缘时才发，见第 11 章 | 同左 |

“已收到该区块”在官方是 `ChunkMap.isChunkTracked`：区块在视距内，并且不在待发送队列里。队列里的区块发送时已包含新状态。

## 6. 创造模式放置方块

来源：`ServerGamePacketListenerImpl.handleUseItemOn`、`ServerPlayerGameMode.useItemOn`、`BlockItem.useOn`、`BlockItem.place`、`BlockPlaceContext`。

### 6.1 前置检查

按顺序。下面任一条返回时只有 ack，**不发** `block_update`：

1. 记 ack。
2. `within_reach(player, pos, 1.0)` 为假则返回。
3. 点击点偏差：三个轴的 `|cursor - 0.5|` 都 `< 1.0000001` 才继续，否则记日志并返回。也就是每个 `cursor` 分量必须在 (-0.5000001, 1.5000001) 内。官方写的是“都小于才继续”，所以 NaN 也被拒绝。Rust 要写成 `!(d.abs() < limit)`，写成 `d.abs() >= limit` 会放过 NaN。

这两条返回时发提示，但也不发 `block_update`：

4. `pos.y > MAX_Y`：提示 `build.tooHigh`，参数 319。
5. `pos.y < MIN_Y`：提示 `build.tooLow`，参数 -64。

再往后的路径结束时都发 6.5 的两个 `block_update`。

触及距离（`Player.isWithinBlockInteractionRange`）：

```rust
fn within_reach(eye: DVec3, pos: BlockPos, buffer: f64) -> bool {
    // 点到方块单位立方体的距离。
    let axis = |lo: f64, p: f64| (lo - p).max(p - (lo + 1.0)).max(0.0);
    let dx = axis(pos.x as f64, eye.x);
    let dy = axis(pos.y as f64, eye.y);
    let dz = axis(pos.z as f64, eye.z);
    let range = 5.0 + buffer; // 基础 4.5，创造模式 +0.5
    dx * dx + dy * dy + dz * dz < range * range
}
```

`eye = (x, y + eye_height, z)`。站立 1.62，蹲下 1.27，游泳和鞘翅 0.4（`Avatar.POSES`）。创造模式的 5.0 是服务端值，和客户端是否收到 `update_attributes` 无关，用 5.0 不会误拒。

`MIN_Y` = -64，`MAX_Y` = 319（`LevelHeightAccessor.getMaxY` = min_y + height - 1）。

### 6.2 方块是否先处理右键

官方 `ServerPlayerGameMode.useItemOn`：

```text
suppress = player.sneaking && (主手非空 || 副手非空)
if !suppress:
    r = 被点方块.useItemOn(手持物, ...)
    if r 消耗动作: return r
    if r 是 TRY_WITH_EMPTY_HAND && hand == MAIN_HAND:
        r = 被点方块.useWithoutItem(...)
        if r 消耗动作: return r
if 手持物为空: return PASS
继续物品逻辑
```

结论：不蹲下时右键箱子、门之类的方块**不会放置**。M2 需要一张表，见第 7 章。蹲下状态来自 `player_input`。

### 6.3 目标位置

`BlockPlaceContext` 的构造函数：

```text
clicked = hit.pos
relative = hit.pos + face
replace_clicked = can_be_replaced(world[clicked], item)
target = replace_clicked ? clicked : relative
can_place = replace_clicked || can_be_replaced(world[relative], item)
```

默认的替换规则（`BlockBehaviour.canBeReplaced`）：

```text
can_be_replaced(state, item) = state 所属方块在 #minecraft:replaceable 里
                               && item != 该方块对应的物品
```

第二个条件的效果：手持 short_grass 点 short_grass 时不替换，放到相邻位置。

`replaceable` 是方块属性，同一方块的所有状态都相同（已对全部方块核对）。这 30 个方块和 block tag `minecraft:replaceable` 完全相等：

`air`、`cave_air`、`void_air`、`water`、`lava`、`bubble_column`、`fire`、`soul_fire`、`light`、`structure_void`、`short_grass`、`tall_grass`、`fern`、`large_fern`、`dead_bush`、`bush`、`red_shrub`、`short_dry_grass`、`tall_dry_grass`、`seagrass`、`tall_seagrass`、`snow`、`vine`、`glow_lichen`、`resin_clump`、`warped_roots`、`crimson_roots`、`nether_sprouts`、`leaf_litter`、`hanging_roots`。

下面这些方块覆盖了 `canBeReplaced`：slab（101 个）、candle（17）、snow、vine、`glow_lichen`、`resin_clump`、`sculk_vein`、`pink_petals`、`wildflowers`、`leaf_litter`、`sea_pickle`、`turtle_egg`、`scaffolding`、`pitcher_crop`，以及 `kelp_plant`、`weeping_vines_plant`、`twisting_vines_plant`、`cave_vines_plant`。

手持**其他**物品时，只有 `sculk_vein` 和默认规则不同。`MultifaceBlock.canBeReplaced` 是 `!手持本物品 || hasAnyVacantFace(state)`，所以手持石头点 `sculk_vein` 会替换它，而它不在 tag 里。M2 要把这一条单独加进 `can_be_replaced`，否则服务端放到 `pos + face`，客户端预测的却是原位。其余覆盖只影响手持同种物品的情况。`pitcher_crop` 和四种 `*_plant` 不在 tag 里，覆盖后结果恒为 false，和默认规则相同。

M2 不实现这些覆盖的后果：slab 不能合并，蜡烛等不能叠加。`snow` 只放默认状态时 `layers` 总是 1，手持其他方块时 `layers == 1` 可替换，和 tag 规则一致。

超出世界的位置：`Level.getBlockState` 返回 `void_air`，它可替换，所以 `can_place` 为真，之后 `Level.setBlock` 因 `isInValidBounds` 为假失败，结果 `FAIL`。Bamboo 直接判 Y 越界即可。

### 6.4 放置检查

`BlockItem.place` 的顺序：

1. `can_place` 为假 → `FAIL`。
2. `updatePlacementContext`（脚手架用）。
3. `getPlacementState`：调用 `getStateForPlacement`，再做 `canPlace` 检查。结果为 null → `FAIL`。
4. `placeBlock`：`setBlock(target, state, 11)`，失败 → `FAIL`。
5. 写入方块实体、`setPlacedBy`（门的上半、床的另一半在这里放）。
6. 音效、`gameEvent`。创造模式不扣物品：`ItemStack.consume` 判断 `hasInfiniteMaterials`，`useItemOn` 还会恢复 count。
7. 返回 `SUCCESS`，它的 `swingSource` 是 `PREDICTED`。

`BlockItem.canPlace` 有两个条件：

- `mustSurvive()` 为真时（默认）要求 `state.canSurvive(level, pos)`，这是各方块自己的规则。
- `level.isUnobstructed(state, pos, placementContext)`：**不能放在实体里**。

实体阻挡规则（`CollisionGetter.isUnobstructed`、`EntityGetter.isUnobstructed`）：

- 取放置状态的碰撞形状。形状为空的方块（花、火把、告示牌等）永远通过，所以这些方块可以放在玩家身上。
- 否则遍历与形状包围盒相交的实体，排除旁观者。`source` 传的是 null，所以**发起者自己也算**。
- 实体要 `blocksBuilding` 为真。所有 `LivingEntity` 都是，M2 里只有玩家。
- 碰撞盒与方块形状有正体积重叠就失败，边界接触不算。

玩家碰撞盒：`[x-0.3, x+0.3] × [y, y+h] × [z-0.3, z+0.3]`。`h` 站立 1.8，蹲下 1.5，游泳和鞘翅 0.6。蹲下的条件是 shift 且没在飞（`Player.getDesiredPose`）。

M2 不需要完整的形状数据，建议：碰撞形状为空 → 通过；否则按整个方块 `[tx, tx+1]` 检查。部分方块（slab 等）会比官方严格一点，客户端预测成功而服务端拒绝时，ack 会把方块回滚掉，后果可接受。反过来，59 个方块的默认碰撞高 1.5（栅栏、栅栏门、墙），这里又比官方宽松：玩家的脚在目标格顶面以上半格内时，官方拒绝，这里会放下。

```rust
fn blocks_placement(p: &Player, t: BlockPos) -> bool {
    let (x, y, z) = (t.x as f64, t.y as f64, t.z as f64);
    p.x - 0.3 < x + 1.0 && p.x + 0.3 > x
        && p.y < y + 1.0 && p.y + p.height() > y
        && p.z - 0.3 < z + 1.0 && p.z + 0.3 > z
}
```

**这个检查不能省。** 客户端预测失败时也会发 `use_item_on`，服务端不查的话方块会真的放进玩家身体里。

### 6.5 结束时的包

官方在 `handleUseItemOn` 末尾无条件执行（在 6.1 第 5 条之后的所有路径，javap 确认）：

```text
send_to(player, block_update(pos, world[pos]))
send_to(player, block_update(pos + face, world[pos + face]))
```

`pos + face` 超出世界时官方发的是 `void_air`，客户端写入失败，Bamboo 可以跳过这一条。

这两个包是立刻发的，方块实际写入后的区块广播还会让发起者再收到一次，重复无害。

### 6.6 高度限制提示

`useItemOn` 没有消耗动作，并且这次算放置尝试（手持非空且是 `BlockItem` 或带内容的 `BucketItem`）时：

- `face == UP && pos.y >= MAX_Y`：`build.tooHigh`。
- `face == DOWN && pos.y <= MIN_Y`：`build.tooLow`。

等待传送确认期间（`awaitingPositionFromClient != null`）官方不执行交互，直接发 `build.tooHigh`，这是它的怪癖。Bamboo 应该同样跳过交互，提示可以不发。

提示都是可选的，功能上不影响同步。

### 6.7 谁收到什么

| 包 | 发起者 | 其他玩家 |
|---|---|---|
| 两个直发的 `block_update` | 发，处理时立刻 | 不发 |
| 区块广播 | 发，tick 中段 | 发。方块有方块实体且 `getUpdatePacket` 非空时还发 `block_entity_data` |
| `block_changed_ack` | 发，tick 末 | 不发 |
| `sound` | 不发，本地已播放 | 距离 `< 16`，位置是方块中心（`level.playSound(player, ...)`） |
| `swing_animation` | 不发 | 追踪者 |

放置和破坏不同：**放置没有 level_event**，其他人听到声音只靠 `sound`。

音效参数：`volume = (sound_type.volume + 1) / 2`，`pitch = sound_type.pitch * 0.8`，声音是 `sound_type.place`，source 是 BLOCKS。范围 `SoundEvent.getRange`：`volume > 1` 时 `16 * volume`，否则 16。

挥手：`swingAndResetAttackStrength(hand, interact_animation, false)`，第三个参数表示不发给本人。官方这段代码有两个分支各调用一次（javap 确认），放置成功时追踪者实际收到两个相同的包。Bamboo 发一个即可。

### 6.8 物品类型的分支

`Item` 的类决定走哪条路。`Extractor` 导出时要带上这个类型：

| 类 | 数量 | M2 处理 |
|---|---|---|
| `BlockItem` | 1038 | 正常放置 |
| `StandingAndWallBlockItem`、`BannerItem`、`HangingSignItem`、`PlayerHeadItem` | 33+16+13+1 | 官方按视线在主方块和挂墙变体之间选（`StandingAndWallBlockItem.getPlacementState`）。用默认状态总是主方块 |
| `DoubleHighBlockItem` | 29 | 官方 `placeBlock` 先把上方一格清成空气或水，上半在 `setPlacedBy` 放。见第 10 章 |
| `PlaceOnWaterBlockItem`（`lily_pad`、`frogspawn`） | 2 | `useOn` 恒为 PASS，只有 `use` 能放到水面。**不要**在 `use_item_on` 里放置 |
| `GameMasterBlockItem` | 7 | 没有权限时 `getPlacementState` 返回 null，M2 一律拒绝 |
| `ScaffoldingBlockItem` | 1 | 有自己的目标位置算法，M2 按普通方块或拒绝 |
| `SolidBucketItem`（`powder_snow_bucket`） | 1 | 按普通方块。创造模式下物品不变成空桶（`BucketItem.getEmptySuccessItem`） |
| 其他物品（工具、桶、刷怪蛋、船等） | 517 | 服务端不处理。客户端可能预测方块变化（比如斧子去皮），ack 会回滚 |

## 7. 被方块吃掉的右键

不蹲下时右键这些方块不放置（6.2）。Bamboo 不实现这些方块的功能时，正确做法是**什么都不改，照常发 6.5 的两个 `block_update`，记 ack**。不这么做的话，右键箱子会在旁边放方块，而客户端预测的是不放。

客户端可能预测了变化（门打开），ack 会回滚。

以下结论针对创造模式、无 OP 权限、主手，来自各类的 `useWithoutItem` 和 `useItemOn`。

**主副手的区别：** `useWithoutItem` 只在主手时调用（6.2）。A、B 里靠它消耗的方块，客户端主手那一步已经停止循环，副手包一般不会到。到了就走放置，比如主手为空时点 `lectern`（`LecternBlock.useItemOn` 对空主手返回 PASS）。告示牌、`decorated_pot`、铜傀儡像在 `useItemOn` 里消耗，不看是哪只手。

**A. 总是消耗：**

- 菜单类：`#minecraft:shulker_boxes`、`#minecraft:anvil`，以及 `chest`、`trapped_chest`、`#minecraft:copper_chests`（8 种）、`ender_chest`、`barrel`、`furnace`、`smoker`、`blast_furnace`、`dispenser`、`dropper`、`hopper`、`crafter`、`crafting_table`、`cartography_table`、`smithing_table`、`loom`、`stonecutter`、`grindstone`、`enchanting_table`、`brewing_stand`、`beacon`。
- 切换类：`#minecraft:doors` 去掉 `iron_door`，`#minecraft:trapdoors` 去掉 `iron_trapdoor`，`#minecraft:fence_gates`，`#minecraft:buttons`，`lever`，`repeater`，`comparator`，`note_block`，`daylight_detector`。
- 其他：`#minecraft:beds`、`straw_bed`、`#minecraft:flower_pots`、`#minecraft:all_signs`、`lectern`、`cake`（创造模式 invulnerable，`canEat` 恒真）、17 种 `candle_cake`、`dragon_egg`、`light`、`decorated_pot`、`#minecraft:copper_golem_statues`（8 种，`useItemOn` 对斧子以外的物品都切换姿势并返回 SUCCESS，未打蜡的 4 种手持 `honeycomb` 时 PASS）。

两个例外不值得在 M2 处理：手持头颅类物品点 `note_block` 顶面时不消耗（`NoteBlock.useItemOn`），手持悬挂告示牌点悬挂告示牌底面时不消耗（`shouldTryToChainAnotherHangingSign`）。

告示牌的判定以客户端为准：客户端 `SignBlock.useItemOn` 总是消耗，不预测放置。官方服务端在告示牌不可编辑时可能返回 PASS，那是有方块实体数据时才有的分支。

**B. 看状态或点击位置：**

| 方块 | 消耗条件 |
|---|---|
| `redstone_wire` | 点状或十字状，且切换后状态有变化 |
| `jukebox` | `has_record=true` |
| `composter` | `level=8`，或手持物可堆肥 |
| `chiseled_bookshelf` | 点中书槽，只有 `facing` 指向的正面才有 |
| `respawn_anchor` | `charges>0`，或手持 `glowstone` 且 `charges<4` |
| `sweet_berry_bush` | `age>1` |
| `cave_vines`、`cave_vines_plant` | 有浆果 |
| `bell` | 点在合适的面和高度（`BellBlock.isProperHit`） |
| `#minecraft:wooden_shelves`（13 种） | 主手、手持非空且点中槽位，只有 `facing` 指向的正面才有 |

M2 只放默认状态，所以 B 组实际是这样：

- `redstone_wire`：默认四向都是 `none`，就是点状，切换后变十字，条件总成立。**按 A 处理。**
- `chiseled_bookshelf`、`*_shelf`：默认朝北，只有点北面才消耗。`bell`：默认 `attachment=floor`、朝北，只有点南北两面、高度不超过 0.8124 才消耗。
- `jukebox`、`composter`、`respawn_anchor`、`sweet_berry_bush`、`cave_vines`：默认状态都不满足状态条件，只剩表里写的手持物条件（`composter` 的可堆肥物品、`respawn_anchor` 的 `glowstone`）。

按不消耗处理也不会掉线，只是方块出现在客户端没预测的位置。

**C. 不要放进表里：**

- `*_fence`：`FenceBlock.useWithoutItem` 在客户端恒为 PASS，服务端只有附近有拴绳生物时才消耗。M2 没有生物，所以在栅栏上放方块是正常的。
- `iron_door`、`iron_trapdoor`：`BlockSetType.IRON.canOpenByHand` 为 false，返回 PASS。
- `command_block` 系列、`jigsaw`、`structure_block`、`test_block`、`test_instance_block`：没有权限时 PASS（`structure_block` 经 `StructureBlockEntity.usedBy`，它先判断 `canUseGameMasterBlocks`）。
- 只对特定物品响应的方块，手持其他方块时不消耗：

| 方块 | 只有这些手持物才消耗 |
|---|---|
| `pumpkin` | `shears` |
| `tnt` | `flint_and_steel`、`fire_charge` |
| `campfire`、`soul_campfire` | 能烤的物品（方块物品里只有 `kelp`、`potato`），或 `#douses_campfires` 且 `lit` |
| `bee_nest`、`beehive` | `honey_level>=5` 时的 `shears`、`glass_bottle` |
| `cauldron` | `water_bucket`、`lava_bucket`、`powder_snow_bucket`、水瓶。M2 放不出别的 `*_cauldron` |
| `vault` | 手持非空且 `vault_state=active`，默认 `inactive` |
| `candle` 系列 | 空手且 `lit` |
| `redstone_ore`、`deepslate_redstone_ore` | 手持方块物品且 `can_place` 时 PASS，其余 SUCCESS。`can_place` 为假时本来也放不下，所以对放置没有影响 |

所用的 tag 都已在 `mc_bamboo_data::tags` 里。`#minecraft:doors`、`#minecraft:flower_pots`、`#minecraft:all_signs` 的成员已和覆盖 `useWithoutItem` 的类比对过，完全一致。

## 8. 创造模式物品栏

### 8.1 槽位

`InventoryMenu`（容器 ID 0）共 46 个槽。`set_creative_mode_slot`、`container_set_slot` 用菜单槽位。来源：`InventoryMenu` 常量和构造函数、`AbstractContainerMenu.addStandardInventorySlots`。

| 菜单槽位 | 内容 | `Inventory` 下标 |
|---|---|---|
| 0 | 合成结果 | 无 |
| 1..=4 | 2×2 合成格 | 无 |
| 5 | 头 | 39 |
| 6 | 胸 | 38 |
| 7 | 腿 | 37 |
| 8 | 脚 | 36 |
| 9..=35 | 背包 | 9..=35（相同） |
| 36..=44 | **快捷栏** | 0..=8 |
| 45 | 副手 | 40 |

手持物：主手 = `slots[36 + selected]`，副手 = `slots[45]`。`selected` 是 0..=8，和 `set_carried_item`、`set_held_slot` 同一含义。

### 8.2 客户端什么时候发包

创造模式界面完全在本地操作，光标上的物品服务端不知道。`CreativeModeInventoryScreen` 打开时在 `inventoryMenu` 上挂 `CreativeInventoryListener`：只要本地检测到某个槽变了，就发 `set_creative_mode_slot(槽位, 新物品)`。

| 玩家操作 | 发什么 |
|---|---|
| 打开界面（E） | 通常不发。`CreativeModeInventoryScreen.init` 的 `addSlotListener` 会立刻 `broadcastChanges`，上次比较之后变过的槽（中键、F、Q、`container_set_slot` 造成的）各发一个 `set_creative_mode_slot`，内容是客户端当前物品，按 8.3 正常写入 |
| 点物品分类里的物品拿到光标上 | 什么也不发 |
| 把光标上的物品放进快捷栏 | `set_creative_mode_slot(36+i, 物品)` |
| 在 Inventory 标签页里移动物品 | 每个变化的槽一个，范围 5..=45 |
| 在物品分类界面悬停按数字键 | `set_creative_mode_slot(36+i, 满组物品)` |
| Shift 点销毁槽 | 对 0..=45 每槽发一个空物品，槽 0 被服务端忽略 |
| 物品丢到界面外 | `set_creative_mode_slot(-1, 物品)` |
| 关闭界面 | `container_close(0)` |
| 快捷栏恢复（`X` + 数字，不需要打开界面） | 9 个 `set_creative_mode_slot(36+i, ...)` |
| 手持可穿戴物品右键（`equippable.swappable` 为真，比如盔甲、鞘翅） | 只有 `use_item`。客户端本地写入装备槽（`Item.use` → `Equippable.swapWithEquipmentSlot`），**没有** `set_creative_mode_slot`。服务端不处理时本人看到穿上了，别人看不到，直到下次打开物品栏时 `broadcastChanges` 补发这个槽。M2 接受，不会卡住也不会断线 |
| 滚轮或数字键换手持槽 | `set_carried_item`，下一次客户端 tick 或操作前发 |
| 中键 | `pick_item_from_block`，见 8.4 |
| Q / Ctrl+Q | `player_action(DROP_ITEM / DROP_ALL_ITEMS)`，客户端已本地扣掉 |
| F | `player_action(SWAP_ITEM_WITH_OFFHAND)`，**客户端不预测** |

`set_creative_mode_slot` 只在 `hasInfiniteMaterials()` 时发。也就是说客户端的 `abilities.instabuild` 必须为真，这正是 M1 的 `PlayerAbilities.instant_build`。同样依赖它的还有打开创造模式界面（`InventoryScreen.init` 里的判断）。

### 8.3 服务端状态和规则

```rust
struct PlayerInventory {
    slots: [Stack; 46],   // 下标是菜单槽位
    selected: u8,         // 初值 0
    state_id: i32,        // 初值 0
}

struct Stack {
    item: i32,            // 0 且 count 0 表示空
    count: i32,
    added: Vec<(i32, Vec<u8>)>,   // 组件值，不透明
    removed: Vec<i32>,
}
```

`set_creative_mode_slot`（`handleSetCreativeModeSlot`）：

```text
if !infinite_materials: return
valid_slot = 1 <= slot <= 45
valid_data = stack 为空 || count <= max_stack_size(stack)
if valid_slot && valid_data: slots[slot] = stack         # 不回包
elif slot < 0 && valid_data: 丢弃                        # M2 没有物品实体，物品消失
# 其余情况（slot 0、slot > 45、count 超限）静默忽略
```

- **不回包。** 官方写入后用 `setRemoteSlot` 记下，`broadcastChanges` 就发现不了差异，所以这里没有任何响应。
- `max_stack_size` 来自报告（1、16、64 三种值，第 9 章）。物品可以用组件改上限，这里只看默认值的话会误拒带 `max_stack_size` 组件的大堆叠。正常游玩拿不到这种物品。简单做法是只校验 `count` 在 1..=99。

服务端主动改变时必须发包：

| 情况 | 操作 | 发给本人 |
|---|---|---|
| F 换手 | 交换 `slots[36+selected]` 和 `slots[45]` | 有变化的槽各一个 `container_set_slot`，通常两个。官方在玩家 tick 的 `broadcastChanges` 里发 |
| Q | 手持槽 `count -= 1`，到 0 变空 | 不发（`ServerPlayer.drop` 用 `setRemoteSlot` 抑制） |
| Ctrl+Q | 手持槽清空 | 不发 |
| 中键 | 见 8.4 | `set_held_slot` 和变化的槽 |

Q 和 Ctrl+Q 必须镜像，否则服务端以为手里还有东西。

丢出物品时官方还会给追踪者发一次 `swing_animation`（主手、默认动画，不发本人，走 `startIfAble` 限流）。Q、Ctrl+Q 和 `set_creative_mode_slot` 的丢弃分支都走 `LivingEntity.drop`，丢的物品为空时不发。这个包不影响同步，M2 想让别人看到挥手就照发。

`state_id`：官方每发一次 `container_set_slot` / `container_set_content` 就 `(id + 1) & 32767`。客户端只保存它，在 `container_click` 里回传。创造模式不发 `container_click`，所以 M2 的值无所谓，建议照做。

物品放置不改物品栏（6.4）。

### 8.4 中键选取

`handlePickItemFromBlock`、`tryPickItem`、`Inventory.addAndPickItem`、`pickSlot`、`getSuitableHotbarSlot`：

```text
on pick_item_from_block(pos, _):
    if !within_reach(player, pos, 1.0): return
    if !is_loaded(pos): return                    # Level.isLoaded：Y 越界或区块未加载都算未加载
    item = block_to_item(world[pos])              # 第 9 章
    if item 是空气: return                        # 水、火、传送门等
    stack = Stack { item, count: 1, 无组件 }
    i = 在 Inventory 0..=35 里找第一个 item 和组件都相同的下标
    if i 存在:
        if i < 9: selected = i
        else:
            h = suitable_hotbar_slot()
            selected = h
            交换 Inventory[h] 和 Inventory[i]
    else:
        h = suitable_hotbar_slot()
        selected = h
        if Inventory[h] 非空:
            f = 0..=35 里第一个空槽
            if f 存在: Inventory[f] = Inventory[h]
        Inventory[h] = stack
    send(set_held_slot(selected))
    对每个变化的槽 send(container_set_slot(0, state_id, 菜单槽位, stack))

suitable_hotbar_slot():
    for k in 0..9: i = (selected + k) % 9; 空槽就返回 i
    for k in 0..9: i = (selected + k) % 9; 没附魔就返回 i
    return selected
```

注意：没有空槽放旧物品时，旧物品被覆盖（`addAndPickItem` 就是这样）。

`Inventory` 下标 `i` 换算菜单槽位：`i < 9` 时是 `36 + i`，否则是 `i`。

`block_to_item` 用 `Block.asItem()`，它包含挂墙变体（`Item.BY_BLOCK`）。少数方块覆盖了 `getCloneItemStack`，比如 `FlowerPotBlock`、`PistonHeadBlock`、`AbstractBannerBlock`、`DecoratedPotBlock`，它们的结果和 `asItem()` 可能不同。M2 忽略这些差别，中键拿到的物品最多不带组件或是空。

### 8.5 其他玩家看到的手持物

主手、副手、盔甲的变化要发 `set_equipment` 给追踪者，组件留空。来源是 `selected` 变化或对应槽变化。建议在 tick 末比较 `(item, count)`，和官方的 `detectEquipmentUpdates` 对应。新追踪者发一次全部非空槽。

### 8.6 进服时

官方 `PlayerList.placeNewPlayer`：`player_abilities` 之后发 `set_held_slot(0)`，再往后 `initInventoryMenu` 发 `container_set_content(0, 1, 46 个物品, 空)`。

M2 没有存档，物品栏总是空的。客户端的 `inventoryMenu` 初始也是空的，所以这两个包不是必需的（**UNVERIFIED**，没有实机验证）。M2 不发，和 `chat-join.md` 8.2 一致。要发的话位置按上面的官方顺序，只有 50 字节。

## 9. 需要生成的数据

| 数据 | 来源 | 现状 | 用途 |
|---|---|---|---|
| 物品名 ↔ ID | `registries.json` 的 `minecraft:item` | **已有**，1658 项，`protocol_id` 和运行时 ID 全部一致。还没生成到 `mc-bamboo-data` | 解码 `ItemStack` |
| 物品 → 方块 | 无报告，要 `Extractor.java` | **缺** | 放置 |
| 物品类型 | `Extractor.java`：`item.getClass()` | **缺** | 6.8 |
| 方块 → 物品 | `Extractor.java`：`block.asItem()` | **缺** | 中键 |
| 方块 → 默认状态 | `extracted.json` | 已有（`blocks::*`），只是常量，需要能按 ID 查的表 | 放置 |
| 状态 → 方块 | `blocks.json` | 缺。每个方块的状态 ID 是连续的，且按方块 ID 递增（已核对），存每个方块的起始状态 ID 后二分查找 | 判定替换、第 7 章 |
| 可替换 | block tag `minecraft:replaceable` | **已有** | 6.3 |
| 碰撞形状是否为空 | `Extractor.java` | **缺** | 6.4 |
| `max_stack_size` | `reports/minecraft/components/item/*.json` | 可读，1658 个文件 | 校验 |
| 创造模式不能破坏 | 同上，`minecraft:tool.can_destroy_blocks_in_creative`，再加 `debug_stick` | 可读 | 5.2 |
| 放置音效 | `Extractor.java`：`state.getSoundType()` | 缺，优先级低。126 种组合 | `sound` |
| 方块实体类型 | `Extractor.java`：遍历 `BuiltInRegistries.BLOCK_ENTITY_TYPE`，取 `type.isValid(state)` 为真的类型。`newBlockEntity` 不能用，部分类型的构造函数要绑定组件。类型 ID 在 `registries.json` 的 `minecraft:block_entity_type`，49 项 | 缺，P1 | 第 11 章 |

`blocks.json` 在 26.3 只有 `properties` 和 `states`，没有 `definition`，看不出方块类。

报告里的物品和方块名字只有 14 个物品不同，所以不能用名字相等代替映射：

`redstone→redstone_wire`、`string→tripwire`、`wheat_seeds→wheat`、`powder_snow_bucket→powder_snow`、`cocoa_beans→cocoa`、`pumpkin_seeds→pumpkin_stem`、`melon_seeds→melon_stem`、`carrot→carrots`、`potato→potatoes`、`torchflower_seeds→torchflower_crop`、`pitcher_pod→pitcher_crop`、`beetroot_seeds→beetroots`、`sweet_berries→sweet_berry_bush`、`glow_berries→cave_vines`。

反过来，除 `air` 外有 77 个方块没有物品（`water`、`fire`、`cave_air`、`void_air`、`potted_*`、`*_candle_cake` 等）。同一物品也可能对应多个方块（63 个物品有挂墙变体）。

`Extractor.java` 增加的导出（已在 scratch 里用相同的 API 跑通过，`Bootstrap.bootStrap()` 之后就能用，不需要绑定组件）：

```java
JsonArray items = new JsonArray();
for (Item item : BuiltInRegistries.ITEM) {
    JsonObject object = new JsonObject();
    object.addProperty("name", BuiltInRegistries.ITEM.getKey(item).toString());
    object.addProperty("kind", item.getClass().getSimpleName());
    if (item instanceof BlockItem blockItem) {
        object.addProperty("block", BuiltInRegistries.BLOCK.getKey(blockItem.getBlock()).toString());
    }
    items.add(object);
}

JsonArray blocks = new JsonArray();
for (Block block : BuiltInRegistries.BLOCK) {
    BlockState state = block.defaultBlockState();
    JsonObject object = new JsonObject();
    object.addProperty("name", BuiltInRegistries.BLOCK.getKey(block).toString());
    object.addProperty("item", BuiltInRegistries.ITEM.getKey(block.asItem()).toString());
    object.addProperty("collision", !state
        .getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO, CollisionContext.placementContext(null))
        .isEmpty());
    blocks.add(object);
}
```

这里要用 `CollisionContext.placementContext(null)`，和 `BlockItem.canPlace` 走的是同一种上下文，已用官方 jar 跑过全部方块。它和 `CollisionContext.empty()` 的结果只有 `scaffolding` 不同：放置时碰撞为空，可以放在玩家身上（`ScaffoldingBlock.getCollisionShape` 先判断 `isPlacement`）。

放置上下文下默认状态的碰撞统计：253 个方块为空，454 个是完整方块，579 个是部分形状。有 152 个方块的碰撞随状态变化（按 `CollisionContext.empty()` 统计），M2 只放默认状态，所以只看默认状态够用。

## 10. 只用默认状态的限制

`Block.getStateForPlacement` 默认返回默认状态，但很多方块覆盖了它。总用默认状态时结果如下。下表数量是**方块物品**数，总数 1141，按方块类统计，前 14 行互不重叠，最后 3 行和前面有重叠。

| 类别 | 数量 | 官方怎么决定 | 默认状态的结果 |
|---|---|---|---|
| 原木、柱子类（含 `iron_chain` 等链、`creaking_heart`） | 72 | `axis` = 点击面的轴 | 永远竖直 |
| 台阶 | 101 | 点击高度决定 top/bottom，同种台阶合并成 double | 永远 bottom，不能合并 |
| 楼梯 | 97 | 朝向、上下、邻居决定 `shape` | 永远朝北、正放、直形 |
| 连接类（栅栏、铁栏、墙、红石线、绊线、紫颂植物、巨型蘑菇方块） | 78 | 按四周邻居算连接 | 栅栏等不连接，墙只有中柱 |
| 门 | 22 | 朝向、铰链，上半在 `setPlacedBy` 放 | 只有下半，永远朝北 |
| 其他双格（`tall_grass`、`large_fern`、`sunflower`、`lilac`、`rose_bush`、`peony`、`pitcher_plant`、`small_dripleaf`） | 8 | 上半在 `setPlacedBy` 放。`DoubleHighBlockItem.placeBlock` 只是先把上方一格清成空气或水 | 只有下半 |
| 床 | 17 | 朝向，另一半在 `setPlacedBy` 放 | 只有床尾，永远朝北 |
| 水平朝向（活板门、栅栏门、箱子、熔炉、釉陶、南瓜灯、砧、架子、铜傀儡像等） | 107 | 玩家朝向或点击面 | 永远是默认朝向，活板门永远在下半格 |
| 六向朝向（发射器、活塞、观察者、漏斗、末地烛、避雷针、紫晶簇、潜影盒、命令方块） | 40 | 视线方向或点击面 | 永远是默认朝向 |
| 主方块/挂墙二选一（火把、告示牌、旗帜、头颅、珊瑚扇、悬挂告示牌） | 63 | 按视线选，直立的还要算 `rotation` | 永远是主方块，贴墙放会悬空 |
| 附着方块（按钮、拉杆、砂轮、梯子、藤蔓、发光地衣、灯笼、绊线钩、可可、钟等） | 37 | 找能附着的面，找不到就放置失败 | 不看支撑，属性是默认值 |
| 可堆叠（蜡烛、雪、海泡菜、海龟蛋、花簇、落叶） | 23 | 点同种方块时数量加一 | 不会增加数量 |
| 铁轨 | 4 | 朝向决定 `shape`，放下后邻居再调整 | 永远南北向，不转弯 |
| 树叶 | 14 | `persistent=true` | `persistent=false`，M2 没有腐烂所以看不出差别 |
| 默认状态含水（珊瑚、海泡菜、海带、海草、潮涌核心） | 24 | 按位置的流体决定，海草和海带不在水里时放置失败 | **在空气里放出含水方块**，见第 11 章 |
| 需要支撑（`canSurvive`） | 277 | 不满足时不放置 | 悬空，比如石头上的花、空中的地毯 |
| 有方块实体 | 139 | 创建方块实体并写入组件 | 见第 11 章 |

全局性的限制，不限于上表：

- **不做邻居更新。** 官方 `setBlock` 的 flags 里有 `UPDATE_NEIGHBORS`，还会更新邻居形状。客户端预测时也更新邻居，ack 后被回滚成服务端状态，所以放在栅栏旁的栅栏不会连接。
- **不做物理。** 沙子、砂砾、混凝土粉末、铁砧不下落。没有红石。
- **两格方块破坏时只破坏点中的那一半。** 官方靠 `updateShape` 删除另一半（`DoorBlock`、`DoublePlantBlock`、`AbstractBedBlock`）。创造模式破坏门和高植物的上半、床尾时，`playerWillDestroy` 还会直接删掉另一半，并多发一个 2001（`DoublePlantBlock.preventDropFromBottomPart`、`AbstractBedBlock.playerWillDestroy`）。
- **物品的 `block_state` 组件被忽略。** 报告里 12 个物品默认带它（`light`、`bee_nest`、`beehive`、`test_block`、8 种铜傀儡像），值都等于方块默认状态，所以只有玩家改过的物品才有差别。

放下时状态和官方一致的集合。这里只比较放下那一刻的方块状态，不管之后的物理和方块实体：

- 292 种：类是 `BlockItem`，方块没覆盖 `getStateForPlacement` 和 `canSurvive`。其中 278 种是实心完整方块，例如石头、泥土、木板、羊毛、混凝土、矿石。这 292 种里有 6 种官方之后会下落（`sand`、`red_sand`、`gravel`、`suspicious_sand`、`suspicious_gravel`、`dragon_egg`），10 种带方块实体（`spawner`、`enchanting_table`、`beacon` 等，见第 11 章）。
- 再加 41 种，要满足括号里的条件才一致，没有水和红石是共同前提：`grass_block`、`podzol`、`mycelium`（上方不是 `#minecraft:snow`）、`farmland`、`dirt_path`（上方没有实心方块，否则官方放 `dirt`）、`note_block`（官方按上下方块选 `instrument`，Bamboo 恒为 `harp`，只影响声音）、5 种珊瑚块（状态一致，但官方 60 到 99 tick 后会因为没水变成死珊瑚）、16 种混凝土粉末、8 种铜格栅、`redstone_lamp`、`barrier`、`heavy_core`、`mangrove_roots`、`sculk_sensor`、`sculk_shrieker`。

客户端总会收敛到服务端状态：`block_update` 先到，ack 时写入世界。所以这些错误只表现为方块变样，不会掉线也不会幽灵方块。

建议在 README 写明：

> M2 放置方块时总是使用方块的默认状态，不计算朝向、连接和支撑，不做邻居更新。普通实心方块完全正确，楼梯、台阶、原木、门等能放但外观不对。

需要决定的一点：下面几类即使放出来也明显是错的，可以在 M2 直接拒绝，只回 ack。这样方块在 ack 时消失，比留下半截好。

| 拒绝 | 数量 | 理由 |
|---|---|---|
| 两格方块 | 47 | 门、床、高植物，只有半截 |
| 默认状态含水 | 24 | 破坏 `fluid_count` 假设 |
| `PlaceOnWaterBlockItem` | 2 | 必须拒绝 |
| `GameMasterBlockItem` | 7 | 必须拒绝 |

后面如果想低成本改善，只需查状态表的两类收益最高：`axis`（72 种，一行逻辑）、台阶的 top/bottom。其余不建议在 M2 做。

## 11. 对接现有代码的注意点

- `Chunk::locate` 对越界 Y 用 `assert!`。`pos` 来自客户端，**必须先检查 Y**，否则一个包就能让世界线程 panic。`Chunk::block` 和 `set_block` 都会触发。`pos + face` 也要检查。
- `Shared.flat_chunk_body` 是所有区块共用的字节。改过的区块要用自己的编码结果，否则玩家走远再回来，改动就不见了。`encode_chunk` 已经从 `Chunk` 重新计算高度图和天空光。
- 区块 `Section::set_block` 已经维护 `non_air_blocks`。`cave_air`、`void_air` 在官方也算空气，但玩家放不出来。
- `chunk_encoding::write_section` 写死 `fluid_count = 0`。放置默认含水的方块会让这个值不对。客户端只把它用于 `EntityFluidInteraction` 的快速跳过，不影响渲染。
- `encode_chunk_body` 写死方块实体数量 0。这个问题多人时必然遇到，见下面的“区块里的方块实体列表”。
- 光照：`LevelChunk.setBlockState` 在客户端也会调用 `checkBlock`，客户端的 `ClientChunkCache` 有自己的 `LevelLightEngine`，每帧 `runLightUpdates`（`ClientLevel.update`）。所以代码上 `block_update` 不需要配 `light_update`（49）。官方也只把这个包发给“这个区块正好在追踪范围边缘”的玩家（`ChunkHolder.broadcastChanges` 的 `getPlayers(pos, true)`、`ChunkMap.isChunkOnTrackedBorder`），视距内部的玩家本来就靠自己算。实际观感 **UNVERIFIED**。`light.rs` 把所有非空气方块当作不透明，这只影响区块重发时的初始数据。
- `Reader` 还缺 `i16`、`i32`、`f32`、`bool`。`rest()` 可以用来取走组件字节。
- `PlayRequest` 派生了 `Copy` 和 `Eq`。加入 `f32` 字段要去掉 `Eq`，加入带 `Vec` 的物品要去掉 `Copy`。
- ID 大于 127 的包（`update_attributes` 134）用两字节 VarInt，`append_frame` 走 `write_varint` 没有问题。
- 触及检查依赖服务端的玩家位置，移动处理要先做好。官方传的 `buffer` 是 1.0，比客户端能瞄到的距离多一格。源码没有写这个余量的用途，但它能容忍服务端的位置略微滞后。
- `hasClientLoaded`：官方在收到 `player_loaded` 或 60 tick 超时之前忽略这些包，且不 ack。客户端加载完成前不能操作，Bamboo 不需要这个门槛。

### 区块里的方块实体列表

来源：`LevelChunk.setBlockState`、`replaceWithPacketData`、`getBlockEntity`，`ClientboundLevelChunkPacketData.BlockEntityInfo`，`SectionCompiler.compile`，`SectionCopy`，`BlockEntityRenderers`。

`block_update` 时客户端自己创建方块实体。收到区块时，客户端先 `clearAllBlockEntities`，然后只创建列表里的。渲染走 `SectionCompiler` → `RenderSectionRegion.getBlockEntity` → `SectionCopy`，只看已存在的方块实体，不会补建。

所以列表为空时，放这些方块的人当场看得到，**之后才收到这个区块的人看不到**。第二名玩家从远处走过来就是这种情况，放的人走远再回来也一样（**UNVERIFIED**，未实机确认）。


| 结果 | 方块数 | 内容 |
|---|---|---|
| 完全看不见，模型只有 particle 贴图 | 84（另有 3 个没有物品：`moving_piston`、`end_portal`、`end_gateway`） | 箱子 9、`trapped_chest`、`ender_chest`、潜影盒 17、旗帜 32、头颅 14（两者都含挂墙变体）、`conduit`、`decorated_pot`、铜傀儡像 8 |
| 方块本身可见，少动态部分 | 78 | 告示牌 26、悬挂告示牌 26、架子 13、`bell`、`lectern`、`campfire`、`enchanting_table`、`beacon` 等。方块实体默认没有内容，所以 M2 里基本看不出差别 |
| 没有方块实体渲染器，不受影响 | 26 | `furnace`、`barrel`、`hopper`、`dispenser`、`crafter` 等 |

26.3 的床不是方块实体（`AbstractBedBlock` 继承 `HorizontalDirectionalBlock`，模型是烘焙的），不受影响。

修法：`Chunk` 记下有方块实体的位置，在区块包里写条目。`ClientboundLevelChunkPacketData.BlockEntityInfo`：

| 字段 | 编码 | 说明 |
|---|---|---|
| `packedXZ` | `Byte` | `(x & 15) << 4 \| (z & 15)` |
| `y` | `Short` | 世界坐标，不是段内坐标 |
| `type` | `VarInt` | `minecraft:block_entity_type` 的 ID，不加一，必须是合法 ID，否则解码失败 |
| `tag` | NBT | `00` 表示没有，M2 总是写 `00` |

例子：区块内 (3, -60, 5) 的箱子（类型 ID 1）[实测字节，解码后重新编码相同]：`35 FF C4 01 00`。

`tag` 为空时客户端靠 `getBlockEntity(pos, IMMEDIATE)` 按方块状态创建默认方块实体，不比较 `type`。

不想做这个列表时，就在 M2 拒绝放置上面第一行的方块，只回 ack，并在 README 写明。

建议的 tick 结构。这里只列方块相关的步骤，合并了区块和实体之后的完整顺序在 `coverage.md` 4.3：

```text
1. 按到达顺序处理所有入站包。方块改动写入世界并记入 changed，直发包进各连接的发送队列。
2. 按段广播 changed：1 个改动发 block_update，多个发 section_blocks_update，发给所有已收到该区块的玩家。
3. 物品栏差异：set_equipment 发给追踪者。
4. 每个 pending_ack >= 0 的玩家发 block_changed_ack，重置为 -1。
```

每个连接的发送队列必须保持先进先出，第 2 步才能先于第 4 步到达。

## 12. UNVERIFIED

下面都没有实机验证。本次任务禁止运行服务端和客户端，所有结论来自读代码和离线 codec 编码。

| 项目 | 为什么不确定 | 怎么确认 |
|---|---|---|
| 不发 `container_set_content`、`set_held_slot` 时一切正常 | 代码看不出依赖，没有实测 | 进服后操作物品栏 |
| 不发 `light_update` 时光照看起来正确 | 客户端确实重算，官方对视距内部也不发这个包。但起点是 Bamboo 简化的天空光数据，视距边缘的区块官方又是发的 | 挖坑、搭顶，在视距边缘也试一次 |
| 靠方块实体渲染的方块在收到区块后看不见 | 从 `replaceWithPacketData`、`SectionCompiler` 和模型文件推出，没有看到现象。`Level.getBlockEntity` 是 `IMMEDIATE`，右键等逻辑访问时会补建，补建后这个 section 是否重新编译没有查 | 一名玩家放箱子，另一名玩家从视距外走过来 |
| 官方发两个 `swing_animation` 时客户端的表现 | javap 确认发两次，`SwingState.startIfAble` 在同一 tick 允许重新开始，看不出可见差异，没有观察过 | 不影响 Bamboo，只发一个 |
| 蹲下时 `pose` 变化的时机 | 实体阻挡用 1.5 还是 1.8，取决于服务端何时更新姿态。Bamboo 用 `player_input` 的 shift 近似，可能差一 tick。飞行时按 shift 官方仍是站立姿态，近似时会算成蹲下 | 影响很小 |

## 13. 验收用例

用真实 26.3 客户端，两个账号：

1. 破坏 grass_block：本人有声音，方块消失不闪回。另一名玩家看到粒子、听到声音、看到手臂挥动。
2. 按住左键连破：约每 0.3 秒一个，不出现幽灵方块。
3. 手持剑破坏：方块不消失，也不闪烁。
4. 放石头：在草地上方、在 short_grass 上（替换）、在另一方块侧面。放完不闪。
5. 把方块放进自己或另一名玩家身体：不放置。花可以放。
6. Y=319 顶面放置：不放置，有提示（如已实现）。破坏 bedrock，Y=-64。
7. 创造模式物品栏：放入、移动、清空、快捷栏恢复。换槽后放的方块就是手里的方块。
8. 中键：已在快捷栏的切过去，不在的加入。服务端后续放置用的是新物品。
9. F 换手：主副手交换，服务端放置使用新手持物。副手放置。
10. 放下药水后 F 换手：物品不变样（验证组件保留）。
11. 右键箱子不放方块。蹲下右键箱子放方块。在栅栏上放方块。
12. 放木头：放置瞬间朝向可能变，ack 后变成竖直（已知限制）。
13. 另一名玩家切换手持物时，这边能看到（`set_equipment`）。
14. 破坏再走远回来：改动还在。
15. A 放箱子和旗帜，B 从视距外走过来：两样都看得到（验证区块里的方块实体列表）。
