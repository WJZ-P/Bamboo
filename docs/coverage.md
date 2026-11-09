# M2 完整性审查

目标版本：Minecraft: Java Edition 26.3，协议 777。这份文档检查 `chunks-movement.md`、`entities.md`、`blocks-inventory.md`、`chat-join.md` 合起来够不够让两个真实客户端走动、互相看见、一起建造、聊天，并长时间保持连接。

| 标记 | 含义 |
|---|---|
| [读] | 读 26.3 反编译源码或 `client.jar` 字节码 |
| [跑] | 用官方 jar 里的 codec、注册表和 assets 实跑，没有启动服务端或客户端。程序是开发时的临时工具，没有收进仓库 |
| UNVERIFIED | 没有实机确认，第 9 节逐条列出 |

没有特别标记的客户端行为都是 [读]。

---

## 0. 结论

1. **69 个 serverbound 包全部分类完。** 21 个必须处理，2 个只要回最小回应，9 个真实客户端会发但可以丢弃，37 个在 M2 里不会发（第 1 节）。
2. **发现一处真正的卡死。** `player_command` 的 `START_FALL_FLYING` 原来标成“忽略”。客户端已经先在本地进入滑翔，这个状态只有服务端能清。不回应时玩家落地后走不了路（1.2，已写入 `entities.md` 第 10 节）。
3. **发现一处多人必现的问题。** 区块包的方块实体列表一直是空的。放箱子的人看得到，之后才收到这个区块的人看不到。84 个方块受影响（已写入 `blocks-inventory.md` 第 11 章）。
4. **客户端只有一处等待没有自己的超时：** 加载画面的 `WaitingForServer`，只能靠 `game_event` 13 离开。M1 已经在发（第 2 节）。
5. **规格之间有 13 处不一致，都已处理**（第 3 节）。其中进服顺序没有冲突，只是各写了一部分，第 4 节给出合并后的进服、离开和 tick 顺序。
6. **规格只写了线上协议，没有写线程之间的约定。** 心跳、写超时、队列上限、世界线程退出、加入和离开的先后，这些决定长时间在线能不能稳定（第 5、6 节）。

---

## 1. Serverbound 包分类

ID 全部来自 `packets.json` 的 `play.serverbound`，共 69 个，`tablecheck.js` 确认下面四张表不重不漏。`idcheck.js` 另外核对了四份规格里约 140 处包 ID 引用，报出的 10 处都是把章节号或相邻列当成了 ID，人工看过，没有真正的错误。

### 1.0 不解码的包不能断开连接

这条约定对下面所有“丢弃”都成立：

- `PlayRequest::decode` 的 `other => Ok(Self::Other(other))` 分支必须保留。未知 ID 也走这里。这个分支不调用 `reader.finish()`。
- `Other` 在连接任务里就地丢弃，不要转发给世界线程，也不要记日志。`client_tick_end` 每名玩家每秒约 20 个。
- 官方对其中几个包会断开（`cookie_response` 发 `multiplayer.disconnect.unexpected_query_response`，`configuration_acknowledged` 抛 `IllegalStateException`）。Bamboo 不照做，这些包正常客户端不会发。
- 只有 Bamboo 解码的包才可能因为内容畸形断开。帧层错误（`FrameError`）仍然断开。
- `set_creative_mode_slot` 不能调用 `finish()`（`blocks-inventory.md` 2.5）。

### 1.1 必须处理（23 个）

| ID | 包 | 客户端什么时候发 | 不处理的后果 | 规格 |
|---|---|---|---|---|
| 0 | `accept_teleportation` | `ClientPacketListener.handleMovePlayer`，每次 `player_position` 后立刻发 | M2 只有进服一次传送，暂时不影响。不解码时至少不能当错误 | `chunks-movement.md` 2.5、5.2 |
| 7 | `chat_command` | `ClientPacketListener.sendCommand` | 玩家什么也看不到 | `chat-join.md` 第 4 节、7.4 |
| 8 | `chat_command_signed` | `say`、`me`、`msg` 的 `minecraft:message` | 验参数签名，通过后发 `player_chat`。不解码就断线 | `chat-join.md` 第 4 节 |
| 15 | `command_suggestion` | 字面量和在线玩家名的 `ask_server` | 不回时补全一直转，下次请求会取消上一次 | `chat-join.md` 第 7 节 |
| 9 | `chat` | `ClientPacketListener.sendChat` | 所有人都看不到消息 | `chat-join.md` 第 4 节 |
| 11 | `chunk_batch_received` | `handleChunkBatchFinished`，每批一个 | 只发第一批 9 个就停 | `chunks-movement.md` 7.3 |
| 14 | `client_information` | Configuration 一次，之后 `Options.save` 触发 | 皮肤外层和主手错，视距不生效 | `entities.md` 第 10 节，`chat-join.md` 第 4 节 |
| 24 | `edit_book` | 关掉书与笔或署名，`BookEditScreen.saveChanges`、`BookSignScreen.saveChanges` | 写下的字关掉就没了，署名后还是书与笔 | `ServerGamePacketListenerImpl.handleEditBook` |
| 28 | `keep_alive` | 收到 clientbound `keep_alive` 时 | 最多 30 秒后断开 | `chat-join.md` 第 4 节，本文第 5 节 |
| 30 | `move_player_pos` | `LocalPlayer.sendPosition` | 区块不跟随，别人看不到移动 | `chunks-movement.md` 2.1 |
| 31 | `move_player_pos_rot` | 同上 | 同上 | `chunks-movement.md` 2.2 |
| 32 | `move_player_rot` | 同上 | 别人看不到转头 | `chunks-movement.md` 2.3 |
| 33 | `move_player_status_only` | 同上 | `onGround` 错 | `chunks-movement.md` 2.4 |
| 36 | `pick_item_from_block` | 中键，`MultiPlayerGameMode.handlePickItemFromBlock` | 客户端不预测，中键没有效果 | `blocks-inventory.md` 8.4 |
| 40 | `player_abilities` | `LocalPlayer.onUpdateAbilities`，切换飞行 | 飞行时按 shift 被算成蹲下 | `entities.md` 第 10 节 |
| 41 | `player_action` | 破坏、Q、F、松开使用键 | 不能破坏方块，Q 之后物品栏不一致 | `blocks-inventory.md` 3.1、第 5 章、8.3 |
| 42 | `player_command` | `sendIsSprintingIfNeeded` 等 | 别人看不到冲刺。**action 6 不回应会卡住，见 1.2** | `entities.md` 第 10 节 |
| 43 | `player_input` | `LocalPlayer.sendChanges`，按键变化时 | 别人看不到潜行，蹲下放方块判定错 | `entities.md` 第 10 节 |
| 46 | `punch` | `Minecraft.startAttack`、`continueAttack` | 别人看不到挥手 | `entities.md` 第 9 节 |
| 54 | `set_carried_item` | `MultiPlayerGameMode.ensureHasSentCarriedItem` | 放出来的方块不对 | `blocks-inventory.md` 3.1 |
| 57 | `set_creative_mode_slot` | `CreativeInventoryListener`，`handleCreativeModeItemDrop` | 服务端不知道手里有什么 | `blocks-inventory.md` 8.2、8.3 |
| 66 | `use_item_on` | `MultiPlayerGameMode.useItemOn` | 不能放方块，预测不结束 | `blocks-inventory.md` 3.1、第 6 章 |
| 67 | `use_item` | `MultiPlayerGameMode.useItem` | 预测不结束，这个 sequence 之前的方块更新不生效 | `blocks-inventory.md` 3.1、第 4 章 |

### 1.2 需要最小回应（2 个，另加 42 的 action 6）

| ID | 包 | 回什么 | 不回的后果 | 规格 |
|---|---|---|---|---|
| 12 | `client_command` | `action=1`（`StatsScreen.init` 发）回 `award_stats`，body `00`。其他 action 忽略 | 统计界面一直显示 `multiplayer.downloadingStats`，Done 能退出 | `chat-join.md` 第 4 节、8.4 |
| 38 | `ping_request` | 原样回 `pong_response`（ID 63） | 只有打开调试网络图表时才发。不回时图表没有数据，没有其他影响 | `chat-join.md` 第 4 节（本次新增） |

`player_command` action 6 `START_FALL_FLYING`：

- `LocalPlayer.aiStep` 先调 `tryToStartFallFlying`，本地置 index 0 的 `0x80`，再发包。
- `LivingEntity.travel` 之后走 `travelFallFlying`，不读 WASD。
- `updateFallFlying` 的清除分支要求 `!level().isClientSide()`，全部代码里客户端能走到的清除点只有 `travelFallFlying` 里的 `onClimbable()`。
- 回应：只给发送者发 `set_entity_data`，index 0 写服务端当前标志字节，`0x80` 为 0。实体 7 是 `07 00 00 00 ff` [跑]。

原规格里只有这一项被标成忽略，却会卡住客户端。同类的 `autoSpinAttackTicks`（`LivingEntity` 里也只有服务端清 flag）要激流三叉戟和水，M2 走不到。

### 1.3 真实客户端会发，可以丢弃（9 个）

| ID | 包 | 什么时候发 | 确认：丢弃后不卡、不失步、不断线 |
|---|---|---|---|
| 1 | `attack` | 左键打到其他玩家，`MultiPlayerGameMode.attack` | 发完立刻本地执行 `Player.attack`，不等回包。随后的 `punch` 让别人看到挥手。没有伤害和击退，属于范围外 |
| 3 | `bundle_item_selected` | 滚轮滚过收纳袋，`BundleMouseActions.toggleSelectedBundleItem` | 客户端先本地 `BundleItem.toggleSelectedItem`，没有回包 |
| 13 | `client_tick_end` | 每个客户端 tick | 官方只用来清 `receivedPositionThisTick` 和 `knownMovement`，见 `chunks-movement.md` 2.7 |
| 19 | `container_close` | 关创造模式界面，`LocalPlayer.closeContainer` | 同一个方法里 `clientSideCloseContainer`，不等回包 |
| 23 | `debug_subscription_request` | 打开 FPS 图表时集合变化，`ClientDebugSubscriber.tick` | 发完不等回包，服务端 tick 图表没有数据 |
| 26 | `interact` | 右键其他玩家，`MultiPlayerGameMode.interact` | 发完本地 `interactOn`，结果只决定本地要不要继续 `use_item`。官方在这里写潜行标志，M2 的潜行来自 `player_input` |
| 37 | `pick_item_from_entity` | 中键点其他玩家 | 客户端不预测。官方 `getPickResult` 对玩家是 null，也什么都不做 |
| 44 | `player_loaded` | `ClientPacketListener.notifyPlayerLoaded` | 官方只结束 60 tick 门限，没有回包，见 `chunks-movement.md` 2.6 |
| 51 | `seen_advancements` | 关进度界面，`AdvancementsScreen.removed` | 官方只处理 `OPENED_TAB`。没有 `update_advancements` 时选中页是 null，`OPENED_TAB` 不会发 |

### 1.4 M2 里不会发（35 个）

权限来自启动目录的 `ops.json`：在名单里的玩家进服时收到对应的 `entity_event` 24..28，不在名单里的是 24。下面这张表是「没有权限时客户端为什么不发」。有了权限之后，`change_game_mode`、`set_game_rule` 和 `set_command_block` 会进来，已经单独处理；其余仍然归入 `Other` 并丢弃，原因写在表后面。

| ID | 包 | 为什么不会发 |
|---|---|---|
| 2 | `block_entity_tag_query` | F3+I 查询要 `COMMANDS_GAMEMASTER`（`KeyboardHandler.copyRecreateCommand`） |
| 4 | `change_difficulty` | `WorldOptionsScreen` 的按钮没有权限时 `active = false` |
| 5 | `change_game_mode` | F3+N、F3+F4 要 `GameModeCommand.PERMISSION_CHECK` |
| 6 | `chat_ack` | 签名消息累计 offset 之后会发；现在按官方前移已读窗口 |
| 10 | `chat_session_update` | 正版验证且客户端准备了密钥时会发；现在校验并广播 `INITIALIZE_CHAT` |
| 16 | `configuration_acknowledged` | 只回应 `start_configuration` |
| 17 | `container_button_click` | 附魔台、讲台、织布机、切石机界面，都要服务端打开 |
| 18 | `container_click` | `AbstractContainerScreen.slotClicked` 才发，创造模式界面覆盖了它 |
| 20 | `container_slot_state_changed` | 合成器界面 |
| 21 | `cookie_response` | 只回应 `cookie_request` |
| 22 | `custom_payload` | 原版只在 Configuration 发 brand（`ClientHandshakePacketListenerImpl`）。模组可能发，照样丢弃 |
| 25 | `entity_tag_query` | 同 `block_entity_tag_query` |
| 27 | `jigsaw_generate` | 拼图方块界面要 `canUseGameMasterBlocks` |
| 29 | `lock_difficulty` | 同 `change_difficulty` |
| 34 | `move_vehicle` | 要骑乘（`LocalPlayer.sendChanges`） |
| 35 | `paddle_boat` | 要控制船（`AbstractBoat`） |
| 39 | `place_recipe` | 配方书只在生存物品栏和容器界面。创造模式的 `InventoryScreen.init` 直接换成 `CreativeModeInventoryScreen` |
| 45 | `pong` | 只回应 clientbound `ping`，Bamboo 不发 |
| 47 | `recipe_book_change_settings` | 同 `place_recipe` |
| 48 | `recipe_book_seen_recipe` | 同 `place_recipe` |
| 49 | `rename_item` | 铁砧界面 |
| 50 | `resource_pack` | 只回应资源包推送（`DownloadedPackSource`） |
| 52 | `select_trade` | 交易界面 |
| 53 | `set_beacon` | 信标界面 |
| 55 | `set_command_block` | 界面要 `canUseGameMasterBlocks` |
| 56 | `set_command_minecart` | 要命令方块矿车实体 |
| 58 | `set_game_rule` | `InWorldGameRulesScreen` 的入口按钮没有权限时禁用（`WorldOptionsScreen.updateButton`） |
| 59 | `set_jigsaw_block` | 同 `jigsaw_generate` |
| 60 | `set_structure_block` | 同 `jigsaw_generate` |
| 61 | `set_test_block` | 同 `jigsaw_generate` |
| 62 | `sign_update` | 编辑界面只由 `open_sign_editor` 打开。`SignBlock.setPlacedBy` 和 `useWithoutItem` 的打开逻辑只在服务端。所以 M2 放告示牌不弹编辑框，也写不了字 |
| 63 | `spectator_action` | 旁观模式 |
| 64 | `teleport_to_entity` | 旁观模式 |
| 65 | `test_instance_block_action` | 同 `jigsaw_generate` |
| 68 | `custom_click_action` | 只来自 `custom` 点击事件和对话框，Bamboo 不发 |

有权限（`entity_event` 26 以上，游戏管理员是 26）之后，客户端会发的包：

| 包 | 现在 |
|---|---|
| `client_command` 的 `action=2`（`REQUEST_GAMERULE_VALUES`） | 等级至少 2 时回 `game_rule_values`，只含当前特性集启用的规则。不够等级只记日志 |
| `set_game_rule`（58） | 等级至少 2 时照 `ServerGamePacketListenerImpl.handleSetGameRule` 写入并给每个 OP 发普通系统消息。未知规则记日志，反序列化失败忽略 |
| `change_game_mode`（5） | 等级至少 2 时走 `GameModeCommand.setGameMode`。不够等级只记日志 |
| `set_command_block`（55） | 等级至少 2 且创造模式的 `instabuild`（`canUseGameMasterBlocks`）时能改模式、命令和条件。命令不执行 |
| `block_entity_tag_query`（2）、`entity_tag_query`（25） | F3+I 会发。还没有方块实体和实体的 NBT 查询回包 |
| `change_difficulty`（4）、`lock_difficulty`（29） | 按钮会亮。难度仍是固定值，这两个包丢弃 |
| `jigsaw_generate`（27）、`set_jigsaw_block`（59）、`set_structure_block`（60）、`set_test_block`（61）、`test_instance_block_action`（65） | 管理员在创造模式可以放下这些方块，没有对应的方块实体，编辑包丢弃 |
| `set_command_minecart`（56） | 没有命令方块矿车 |
| `spectator_action`（63）、`teleport_to_entity`（64） | F3+F4 能切到旁观，但旁观点选和传送还没做，包丢弃 |

---

## 2. 客户端在等什么

逐一查了 `LevelLoadTracker`、`ClientPacketListener`、`ClientCommonPacketListenerImpl`、所有 `client/gui/screens` 里的等待界面（`LoadingDotsWidget`、`LoadingDotsText`、`GenericWaitingScreen`、`ProgressScreen`、`WaitingForResponseScreen`）和带 `pending`、`waiting`、`isLoading` 状态的字段。

### 2.1 会挡住进度的等待

| # | 位置 | 等什么 | 等不到时 | 客户端超时 | Bamboo |
|---|---|---|---|---|---|
| 1 | `ConnectScreen`，`ClientHandshakePacketListenerImpl` | `login_finished` | 停在连接界面 | 30 秒读超时 | M1 已有 |
| 2 | `ClientConfigurationPacketListenerImpl` | `select_known_packs`、`finish_configuration` | 同上 | 同上 | M1 已有 |
| 3 | Configuration 结束到 `login` 之间 | `login` | 停在连接界面，还没有 `level` 和 `gameMode` | 同上 | M2 的 `login` 改由世界线程发，晚一个 tick 没有问题 |
| 4 | `LevelLoadTracker.WaitingForServer` | `game_event` 13 | 永远停在加载画面 | **没有**。这个状态没有 `tick`，`timeoutAfter` 不生效。只有心跳也断了才会读超时 | M1 已有，必须保留 |
| 5 | `LevelLoadTracker.WaitingForPlayerChunk` | 摄像机 section 编译完，也就是 `player_position`、出生点 3×3 区块和光照 | 加载画面 | 从 `handleLogin` 起 30 秒，之后放行 | `chunks-movement.md` 7.4、8.3 |
| 6 | `clientLoaded = false`，`LocalPlayer.tick`、`sendChanges` | 第 5 步结束 | 没有物理，不发移动包 | 同第 5 步 | 不需要处理 |
| 7 | `StatsScreen.isLoading` | `award_stats` | 一直显示等待文字，Done 能退出 | 没有 | 要回，1.2 |
| 8 | `BlockStatePredictionHandler` | `block_changed_ack` | 预测不结束，这些位置后面的 `block_update` 不生效 | 没有 | `blocks-inventory.md` 第 4 章 |
| 9 | 滑翔的本地状态 | `set_entity_data` 清 `0x80` | 走不了路 | 没有 | 要回，1.2 |

### 2.2 看起来在等，其实不挡

| 位置 | 等什么 | 结论 |
|---|---|---|
| `InWorldGameRulesScreen` | `game_rule_values` | 等级至少 2 时回包，界面能停下。没有权限时入口按钮禁用，服务端收到请求也只记日志 |
| `ClientSuggestionProvider` | `command_suggestions` | 只是没有补全 |
| `DebugQueryHandler` | `tag_query` | 回调不触发，没有权限时根本不发 |
| `DialogScreen` 的 `WAIT_FOR_RESPONSE` | 服务端动作 | 只有服务端发对话框才有 |
| `ServerReconfigScreen` | Configuration 流程 | 只有 `start_configuration` 才进 |
| `ProgressScreen` | 断开时的本地清理 | 多人游戏不等服务端（`Minecraft.disconnectWithProgressScreen`） |
| `AdvancementsScreen` | `update_advancements` | 没有等待，只显示空状态 |
| `chunk_batch_start`、`chunk_batch_finished` | | 客户端不要求批次，`chunks-movement.md` 7.5 |
| `SectionOcclusionGraph.sectionsWaitingForChunkLoads` | 区块 | 渲染层面等，区块到了继续，`chunks-movement.md` 8.4 |
| 吃东西等使用物品 | `entity_event` 9 | `updateUsingItem` 的完成分支要求服务端。收不到时动画一直持续，松手时 `releaseUsingItem` 结束。创造模式物品也不会变，M2 接受 |

---

## 3. 规格之间的矛盾

| # | 话题 | 冲突 | 结论 | 处理 |
|---|---|---|---|---|
| 1 | `tests/join.rs` 的区块数 | `chat-join.md` 第 10 节：视距 1 时 25 个。`chunks-movement.md` 第 10 节：`T(2)` 49 个 | **49**。配置要先 clamp 到 2..=32（`ChunkMap.setServerViewDistance`），之后才算 `T` | 改了 `chat-join.md` |
| 2 | `set_held_slot`、`container_set_content` | `chat-join.md` 8.2：不用补。`blocks-inventory.md` 8.6：建议照发 | 不发。物品栏和选中槽在两边的初值相同 | 改了 `blocks-inventory.md` |
| 3 | `update_attributes` 给自己 | `chat-join.md`：要补。`blocks-inventory.md`：P2 可选 | 进服时发一次。配对时给别人不发 | 改了 `blocks-inventory.md`，优先级改成 P1 |
| 4 | `UPDATE_LATENCY` | `chat-join.md`：M2 可以不做。`entities.md`：每 601 tick 广播 | 建议做。心跳已经有往返时间 | 改了 `chat-join.md` |
| 5 | `punch` 限流 | `entities.md` 第 9 节：不用实现。`blocks-inventory.md` 3.1：应该做 | M2 不做。多发没有副作用，属于性能项 | 改了 `blocks-inventory.md` |
| 6 | F 换手 | `entities.md` 第 11 节：不处理。`blocks-inventory.md`：P0 | 处理，以 `blocks-inventory.md` 8.3 为准 | 改了 `entities.md` |
| 7 | 物品组件 | `entities.md`：只存 item 和 count，还写了转发方法。`blocks-inventory.md`：当不透明字节保留，**不要转发给别人** | 保留，只发回本人，别人写空组件。转发未校验的字节会让对方断线 | 改了 `entities.md` |
| 8 | `client_information` 的级别 | `chunks-movement.md`：建议。`entities.md`：必须 | 解码是必须，用视距是建议 | 改了 `chunks-movement.md` |
| 9 | `START_FALL_FLYING` | `entities.md`：忽略，别人看到普通站姿。代码：发送者卡住 | 回 `set_entity_data` | 改了 `entities.md` |
| 10 | 依赖方块实体渲染的方块 | `blocks-inventory.md` 第 11 章列了告示牌和床。代码和 assets：26.3 的床不是方块实体，告示牌有方块模型 | 受影响的是箱子、潜影盒、旗帜、头颅等 84 个 | 改了 `blocks-inventory.md` |
| 11 | `PlayRequest` 的文本字段 | `chat-join.md` 第 10 节：借用帧数据，加生命周期。世界线程要跨线程拿到它 | 用 `String`，不借用 | 改了 `chat-join.md` |
| 12 | 进服包序 | 三份规格各列一部分（`chat-join.md` 8.5，`chunks-movement.md` 8.3，`entities.md` 第 14 节）。相对顺序没有冲突 | 以 `chat-join.md` 8.5 为基准，4.1 是合并后的版本 | 其他两份加了指向 |
| 13 | 每 tick 顺序 | 三份规格各有一版，都只列自己的步骤 | 合并成 4.3 | 三份都加了指向 |

已核对、没有矛盾：包 ID，`ItemStack` 两种格式，`swing_animation`，`set_equipment` 的槽位，`player_info_update` 的 8 个动作，离开时 `remove_entities` 和 `player_info_remove` 的先后，`T` 和 `R` 的定义，`isChunkTracked` 的用法，`hasClientLoaded` 门槛不实现的结论，`entity_interaction_range` 13 和 `block_interaction_range` 8（`registries.json`）。

---

## 4. 合并后的顺序

### 4.1 进服

连接任务收到 `finish_configuration` 后切到 Play，把 `Join` 发给世界线程，然后只转发入站包。心跳从这一刻开始计时。

世界线程处理 `Join`，全部写进这名玩家的出站队列：

| # | 包 | 发给 | 说明 |
|---|---|---|---|
| 1 | `login` | 新玩家 | `chunkRadius` 写 clamp 后的服务端视距 |
| 2 | `player_abilities` | 新玩家 | |
| 3 | `player_position` | 新玩家 | id 1，记入 `awaiting_position` |
| 4 | `player_info_update` `0xFF` | 新玩家 | 所有已在线玩家。没有人时可以不发 |
| 5 | `player_info_update` `0xFF` | 所有人和新玩家 | 这一步把新玩家加入“已进入 Play”名单 |
| 6 | `set_entity_data` index 15、16，`update_attributes` | 新玩家 | 值来自 Configuration 的 `client_information` |
| 7 | `system_chat` 加入消息 | 其他已在线玩家 | 不发给新玩家 |
| 8 | `set_time` | 新玩家 | |
| 9 | `game_event` 13 | 新玩家 | |
| 10 | `set_chunk_cache_center` | 新玩家 | `T` 全部进 `pending` |

这一 tick 末尾发第一批区块（9 个）。后续的配对由第 5 步之后的 `visible` 判定完成，要等目标区块离开 `pending`（`entities.md` 第 4 节）。

M1 的 `send_join_packets` 在连接任务里写 socket，M2 不能这样：世界线程发的 `player_info_update` 可能抢在 `login` 前面。

### 4.2 离开

连接任务结束时先把 `Leave` 发给世界线程，然后才让 `PlayerSession` 销毁。世界线程按这个顺序处理（`ServerGamePacketListenerImpl.removePlayerFromWorld`，`PlayerList.remove`）：

1. 只有进入过 Play 的玩家才继续。在 Configuration 里断开的玩家世界线程不知道。
2. `system_chat` 离开消息，发给其他玩家。
3. `remove_entities`，发给这名玩家的 `seen_by`。
4. 把这名玩家从所有人的 `seen_by` 里删掉。
5. 从“已进入 Play”名单删掉。
6. `player_info_remove`，发给所有人。

`Leave` 必须先于同一 UUID 的下一次 `Join` 到达世界线程。所以 `Join` 和 `Leave` 走同一条全局队列，`PlayerSession` 必须在 `Leave` 发出之后才销毁，否则同名玩家重连时世界线程里还有旧实体。

### 4.3 每 tick

| 阶段 | 内容 | 约束 | 来源 |
|---|---|---|---|
| 0 | 处理 `Leave`，再处理 `Join` | 4.2 | |
| 1 | 按到达顺序处理入站包。同一连接不能乱序。方块改动写入世界，直发包进队列（`use_item_on` 的两个 `block_update`、`level_event`、`set_held_slot`、`container_set_slot`、聊天广播、`award_stats`、滑翔回应） | | `chunks-movement.md` 第 4 节，`blocks-inventory.md` 第 11 章 |
| 2 | 派生状态：`head_yaw = yaw`，姿势，元数据脏标记 | | `entities.md` 第 13 节 |
| 3 | 区块跟踪：比较 `(center, vd)`，发 `set_chunk_cache_center`，发 `forget_level_chunk`，更新 `pending` | center 先于 forget 和区块 | `chunks-movement.md` 6.3 |
| 4 | 广播方块变化给 `isChunkTracked` 的玩家 | 先于第 7 阶段 | `blocks-inventory.md` 4.3 |
| 5 | `visible` 配对和解除，然后每名玩家 `send_changes` | 配对用 `pending` 的当前状态 | `entities.md` 第 4、7 节 |
| 6 | 比较装备，发 `set_equipment` | | `entities.md` 第 11 节 |
| 7 | 每名玩家 `block_changed_ack`，取最大 sequence | 晚于第 4 阶段 | `blocks-inventory.md` 4.3 |
| 8 | 每 601 tick 广播 `UPDATE_LATENCY` | | `entities.md` 第 3 节 |
| 9 | 每名玩家发一批区块 | 最后一步，内容取当前世界 | `chunks-movement.md` 7.2 |

官方的等价顺序：`processQueuedPackets`（含 `ChunkMap.move`）、`broadcastChangedChunks`、`ChunkMap.tick`、`tickConnection`（ack、`doTick`）、`playerList.tick`、`sendNextChunks`（`MinecraftServer.tickChildren`，`ServerChunkCache.tick`）。第 3 阶段排在第 4 阶段前面和官方相同，因为官方在处理移动包时就更新了跟踪。

tick 循环：20 tick 每秒，按绝对时间排下一次，不要在工作完成后再 sleep 50 ms。官方落后不多时连续跑 tick 追上，落后超过约 2 秒时打 `Can't keep up!`，把 `nextTickTimeNanos` 往后推，跳过这段时间（`MinecraftServer.runServer`）。Bamboo 照做就行，协议上只有 `Stepped` 的 `ticks` 字段和 tick 有关，它数的是实际跑过的 tick。

---

## 5. 长时间在线和超时

### 5.1 两侧的超时

| 谁 | 机制 | 值 | 触发后 | 来源 | 对 Bamboo 的要求 |
|---|---|---|---|---|---|
| 客户端 | `ReadTimeoutHandler` | 30 秒没收到任何字节 | `disconnect.timeout` | `Connection.connect`、`exceptionCaught` | 没有其他玩家时，世界里唯一的流量就是心跳（M1 的时钟静止，也不发周期 `set_time`）。心跳间隔必须小于 30 秒，余量是 15 秒 |
| 客户端 | 对端关闭 | | `disconnect.endOfStream` | `Connection.channelInactive` | 断开前先发 `disconnect`，否则玩家只看到这个原因 |
| 客户端 | 心跳回应延后 | 主线程卡在事件轮询超过 200 ms 时不回，留到恢复，最多 1 分钟 | 恢复后补发，过期丢弃 | `handleKeepAlive`、`sendDeferredPackets`，`RenderSystem.isFrozenAtPollEvents` | 超过服务端期限时会被踢，这和官方相同 |
| 客户端 | 加载画面 | 30 秒 | 放行玩家 | `LevelLoadTracker` | 2.1 第 5 项 |
| 官方服务端 | `ReadTimeoutHandler` | 30 秒没收到字节 | 断开 | `ServerConnectionListener` | 不需要单独实现，心跳已覆盖 |
| 官方服务端 | 心跳 | 每 15 秒，上一次没回就断开 | `disconnect.timeout` | `ServerCommonPacketListenerImpl.keepConnectionAlive` | M1 相同，`keep_alive_interval` 15 秒 |
| 官方服务端 | 登录 | 600 tick | `multiplayer.disconnect.slow_login` | `ServerLoginPacketListenerImpl.tick` 的 `MAX_TICKS_BEFORE_LOGIN` | `pre_play_timeout` 30 秒覆盖 Login 和 Configuration，时长相同。在 Login 阶段到点时也发同一个翻译键，别的阶段官方没有时限，直接断开 |
| Bamboo M1 | `CHUNK_SEND_TIMEOUT` | 60 秒 | 断开 | `connection/play.rs` | 删除。批次确认取代它 |

客户端正常时每秒至少发 20 个 `client_tick_end`，所以读超时只在客户端真的挂了时才有意义。

没有其他客户端看门狗：收不到 `move_entity_*` 时远程玩家只是停在原地，收不到区块时玩家掉下去，都不会断开。

### 5.2 M2 要守住的规则

1. **心跳留在连接任务里。** 官方的 `handleKeepAlive` 和 `handlePingRequest` 都没有 `ensureRunningOnSameThread`，在网络线程执行。放进世界线程的话，一次卡顿就会让所有人超时。
2. **心跳计时不能依赖 socket 写完成。** M1 在 `select!` 分支里 `connection.write(...).await`，客户端不读数据时这一步会一直挂起，超时判断也跟着停，玩家永远不会被踢。M2 把帧放进出站队列就算发出，等待计时从这一刻开始。这样不读数据的客户端最多 30 秒被发现。
3. **所有写 socket 的路径都要有时限。** M1 的 `disconnect` 只给 `finish()` 加了 `SHUTDOWN_LINGER`，`connection.write` 没有。对方不读时断开流程本身会挂住。
4. **出站队列的上限按字节算，并容得下区块批次的峰值。** 平坦区块约 54.4 KB（由 `encode_chunk` 的结构算出：25 层天空光各 2050 字节，加约 3.2 KB）。`T(8)` 共约 18 MB，未确认的上限 10 批 × 64 个约 35 MB。按帧数设一个小上限会误踢刚进服的玩家。按第 2 条，队列就算不设上限，也只会增长到心跳超时为止。
5. **`keep_alive_interval` 要校验。** 配置值大于等于 30 秒时客户端会先超时。
6. **断开要带原因。** 心跳超时、`id` 不匹配时官方发 `disconnect.timeout`。M1 的 `id` 不匹配分支直接返回错误，客户端只看到 `disconnect.endOfStream`。
7. **世界线程退出时连接任务要跟着结束。** 连接任务独立回心跳，世界线程 panic 后客户端会一直留在一个不动的世界里。世界线程结束时要取消全部连接。`Chunk::locate` 的 `assert!` 就是一个现成的 panic 来源（`blocks-inventory.md` 第 11 章）。
8. **关闭服务端的 `disconnect` 走同一条出站队列。** 这样已经排队的 bundle 不会被切开。`PacketBundlePacker.verifyNonTerminalPacket` 遇到 bundle 里的 `disconnect` 会抛异常。

### 5.3 长时间运行不会出问题的部分

已经看过，不需要处理：`sequence` 和 `state_id` 增长很慢。`chat` 不累积任何窗口（`lastSeen` 恒为空）。`pending` 受 `T` 限制。`T` 的坐标在 clamp 后都在 `i32` 内。客户端自己累加 `gameTime`，不收周期 `set_time` 也没有影响（Bamboo 的时钟 `rate=0`）。

---

## 6. 还缺什么

“已写入”表示这次已经补进规格。其余是实现时要做的决定。

### 6.1 必须

| # | 缺口 | 位置 |
|---|---|---|
| G1 | `START_FALL_FLYING` 的回应 | 已写入 `entities.md` 第 10 节 |
| G2 | 区块包的方块实体列表，或者拒绝放置那 84 个方块 | 已写入 `blocks-inventory.md` 第 11 章。**需要决定** |
| G3 | 线程结构：连接任务读帧、解码，就地处理 `keep_alive`、`ping_request`、`Other`，其余发给世界线程。写任务串行写出站队列。世界线程只写队列，不碰 socket。M1 的 `Connection` 要拆成读写两半 | 本文 4.1、5.2 |
| G4 | 心跳独立于世界线程和 socket 写 | 本文 5.2 第 1、2 条 |
| G5 | 写超时、队列上限 | 本文 5.2 第 3、4 条 |
| G6 | 世界线程退出时结束所有连接 | 本文 5.2 第 7 条 |
| G7 | `Join`、`Leave` 走同一条全局队列，`PlayerSession` 晚于 `Leave` 销毁 | 本文 4.2 |
| G8 | 配置先 clamp 到 2..=32，`login` 和 `T` 用同一个值 | `chunks-movement.md` 第 10 节 |
| G9 | 世界线程持有区块表：没改过的区块用 `flat_chunk_body`，改过的用自己的 `Chunk`，查询时回退到平坦模板。改动常驻内存直到关服 | `chunks-movement.md` 第 10 节，`blocks-inventory.md` 第 11 章 |

### 6.2 建议

| # | 缺口 | 说明 |
|---|---|---|
| G10 | 同名重连 | M1 拒绝新连接（`JoinRefusal::AlreadyOnline`）。官方踢掉旧连接，等它断开后放行（`disconnectAllPlayersWithProfile`，`WAITING_FOR_DUPE_DISCONNECT`，原因 `multiplayer.disconnect.duplicate_login`）。断网或机器宕机时 TCP 不会马上关闭，旧连接要等心跳超时，最多 30 秒，这段时间重连会被拒绝。**需要决定** |
| G11 | `pong_response` | 已写入 `chat-join.md` 第 4 节 |
| G12 | `UPDATE_LATENCY` | `entities.md` 第 3 节，往返时间从连接任务传给世界线程，用原子变量就够 |
| G13 | 断开原因用翻译键 | `Disconnect` 要能写翻译组件（`chat-join.md` 第 10 节）。`disconnect.timeout`、`multiplayer.disconnect.invalid_player_movement`、`multiplayer.disconnect.illegal_characters` 都要用 |
| G14 | 入站队列满时的处理 | 连接任务不能因为入站队列满了而停止读，否则心跳回应也读不到。`Other` 丢弃之后，正常玩家每 tick 只有几个小包，队列满了说明客户端异常或世界线程卡住，直接断开这名玩家 |

### 6.3 M2 接受的限制

这些都不会卡住或断线，建议写进 README：

| 现象 | 原因 |
|---|---|
| 鞘翅不能用 | `START_FALL_FLYING` 被拒绝 |
| 攻击其他玩家没有效果，只有挥手 | `attack` 丢弃 |
| 右键穿盔甲别人看不到，打开物品栏后才同步 | `blocks-inventory.md` 8.2 |
| 听不到别人的脚步声 | 官方由服务端的 `Entity.move` 产生声音发给其他人，`Player.playSound` 排除自己。Bamboo 没有服务端物理 |
| 食物吃不完，松手结束 | 2.2 |
| 右键成书打不开 | 没有 `open_book`。写字和署名已经按 `handleEditBook` 做了，槽位会同步回去 |
| 丢出的物品会留在地上 | 物品实体已经生成，5 分钟后消失 |
| 重连回到出生点，物品栏清空 | 不存档。出生点被方块占了时玩家会卡在方块里，创造模式可以飞出来 |
| 掉进虚空一直往下掉 | 官方由服务端扣血。客户端 `LivingEntity.onBelowWorld` 调 `Entity.hurt`，它只在 `ServerLevel` 上生效。可以飞回来 |

---

## 7. 实现清单

### 7.1 `PlayRequest` 最终解码列表

解码结果要 `Send + 'static`，要去掉 `Copy` 和 `Eq`。

| 变体 | 包 | 在哪处理 |
|---|---|---|
| `KeepAlive` | 28 | 连接任务 |
| `PingRequest` | 38 | 连接任务 |
| `AcceptTeleportation`、`MovePlayer`（4 种） | 0、30..33 | 世界线程 |
| `ChunkBatchReceived` | 11 | 世界线程 |
| `Chat`、`ChatCommand` | 9、7 | 世界线程 |
| `ClientInformation`、`ClientCommand` | 14、12 | 世界线程 |
| `PlayerInput`、`PlayerCommand`、`PlayerAbilities` | 43、42、40 | 世界线程 |
| `Punch`、`SetCarriedItem`、`SetCreativeModeSlot` | 46、54、57 | 世界线程 |
| `PlayerAction`、`UseItemOn`、`UseItem`、`PickItemFromBlock` | 41、66、67、36 | 世界线程 |
| `Other` | 其余 | 连接任务丢弃 |

`Reader` 要加 `i8`、`i16`、`i32`、`f32`、`f64`、`bool`。现有的是 `varint`、`u8`、`u16`、`i64`、`uuid`、`count`、`string`、`rest`、`finish`。

`ConfigurationRequest` 加 `ClientInformation`。

### 7.2 要生成的数据

| 数据 | 来源 | 用途 |
|---|---|---|
| `item`、`entity_type`、`attribute`、`block_entity_type`、`chat_type` 的 ID | `registries.json`，`chat_type` 走生成器 | 各规格 |
| `sound_event` | `registries.json` | 放置音效，P2 |
| 物品 → 方块、物品类型、方块 → 物品、碰撞是否为空 | 扩展 `Extractor.java`（`blocks-inventory.md` 第 9 章） | 放置、中键 |
| 方块 → 方块实体类型 | 扩展 `Extractor.java` | 区块列表 |
| 状态 → 方块 | `blocks.json` | 替换规则、第 7 章的表 |
| `can_destroy_blocks_in_creative` | `reports/minecraft/components/item` | 破坏 |

---

## 8. 两个客户端的验收

先跑 `blocks-inventory.md` 第 13 章。下面是跨规格的部分，用两个不同名字的客户端 A、B：

1. A 进服，B 进服：A 收到黄色加入消息，B 收不到自己的。Tab 列表各有两人。出生点互相看得到。
2. A 走到 300 格外再回来：区块跟随，没有空洞，回来时 B 重新出现。
3. A 潜行、冲刺、跳、飞：B 看到的动作和名牌正确。
4. A 手持物品切换，换到副手：B 看到的手持物正确。
5. A 放箱子和旗帜，B 从视距外走来：都看得到。
6. 同一格同时破坏和放置：两边最后一致，没有幽灵方块。
7. 穿上鞘翅，空中按跳跃：玩家不卡住，落地能走。
8. 打开统计界面：不转圈。打开进度界面：显示空状态。打开调试网络图表：有数据。
9. 聊天含 emoji 和中文，输入 `/foo`：聊天显示正确，`/foo` 有红色错误。
10. 让两个客户端静止 10 分钟：都不断开。Tab 里的延迟有变化。
11. 强杀 A 的进程：B 马上看到离开消息，实体消失，A 能立刻重连。
12. 暂停 A 的进程（调试器）超过 30 秒：服务端踢掉 A，B 不受影响，世界线程不卡。恢复后 A 重连的结果按 G10 的决定。
13. 关闭服务端：两个客户端都看到关闭原因。

---

## 9. UNVERIFIED

按任务要求没有运行服务端和客户端，下面都没有实机确认。

1. **`START_FALL_FLYING` 卡住的实际表现。** 控制流来自源码，`travelFallFlying` 在地面上的手感没有看过。
2. **84 个方块看不见。** 依据是 `replaceWithPacketData`、`SectionCompiler` 和 assets 里的模型。模型统计只看有没有 `elements`，以及类型是否注册了渲染器。这个结论没有实机看过，右键触发补建后 section 是否重新编译也没有查。
3. **`isFrozenAtPollEvents` 什么时候成立。** 字节码是“轮询事件超过 200 ms”。拖动窗口是否触发它，26.3 换到 SDL 之后在 Windows 上的行为没有确认。
4. **平坦区块的字节数。** 54.4 KB 是从 `encode_chunk` 的结构算的，没有运行 Bamboo 量过。
5. **`set_entity_data` 回写 index 0 时和客户端本地标志的竞争。** 官方也是同样的做法，这里只读了代码。比如回应路上客户端刚好开始冲刺，这一位会被覆盖一个往返时间。
6. **第 6.3 节的限制。** 全部来自读代码，脚步声只读了 `Entity.move` 到 `Player.playSound` 的路径，没有抓包。
7. **1.3 的 `attack`。** 只确认了客户端不等回包。本地 `Player.attack` 的音效和粒子是否和官方有差别没有比较。
