# M2 规格：聊天与进服序列

目标版本：Minecraft: Java Edition 26.3，协议 777。这份规格覆盖聊天、命令入口、加入和离开消息，以及玩家留在世界里之后进服序列还缺什么。区块发送和移动见同目录的 `chunks-movement.md`，这里只写和聊天、进服有关的部分。

| 事实来源 | 用途 |
|---|---|
| 反编译的 26.3 源码 `mc-src/net/minecraft/...` | 所有字段和行为 |
| `target/vanilla/generated/reports/packets.json` | 所有包 ID |
| `target/vanilla/generated/reports/registries.json` | `attribute`、`command_argument_type` 的 ID |
| `client.jar` 里的 `assets/minecraft/lang/en_us.json` | 翻译键的文本 |
| `authlib-10.0.77.jar`（`javap`） | `UserApiService.OFFLINE_PROPERTIES` 含 `CHAT_ALLOWED` |
| 官方 codec 实跑 | 写了几个临时小程序（没有收进仓库），只调用 `server-26.3.jar` 里的 `STREAM_CODEC`、`ChunkTrackingView`、注册表加载，没有启动服务端或客户端，也没有动仓库 |

本文的字节例子有两种标记。**[官方编码]** 是官方 codec 写出的字节。**[官方解码]** 是手写字节，已被官方 codec 解码成功，且没有剩余字节。

复核时把本文的字节例子从文档里抽出来，用官方 codec 另跑了一遍（同样没有启动服务端或客户端）。全部解码成功，没有剩余字节。包体例子重新编码后和原字节相同。手写键序的文本组件只做了解码，官方编码的键序本来就不同。

记号：

| 记号 | 含义 |
|---|---|
| `VarInt` / `VarLong` | 变长整数 |
| `Byte` / `Bool` | 1 字节，`Bool` 是 0 或 1 |
| `Short` `Int` `Long` `Float` `Double` | 定长，大端 |
| `UUID` | 16 字节，高 64 位在前 |
| `String(n)` | VarInt 字节长度 + UTF-8。`n` 是 UTF-16 码元上限，字节长度上限是 `3n`（`Utf8String.read`） |
| `Identifier` | `String(32767)` |
| `NBT` | 网络 NBT，见第 2 节 |
| `Opt<T>` | `Bool`，为 1 时后面跟 `T`（`ByteBufCodecs.optional`、`readNullable`） |
| `List<T>` | VarInt 元素数 + 元素 |

包 ID 大于 127 时占两个字节，比如 `update_attributes` 的 134 写成 `86 01`。帧格式不变：`VarInt 帧长 + VarInt 包 ID + body`，M2 没有压缩。

---

## 1. 结论

1. **聊天不需要任何签名。** Bamboo 的 `login` 里 `online_mode=false`、`enforces_secure_chat=false`，客户端不创建聊天会话，`chat` 包里签名为空，`chat_session_update` 不会发。
2. **`LastSeenMessages.Update` 可以解码后丢弃。** 只要服务端不发带签名的 `player_chat`，客户端发来的永远是 `00 00 00 00 01`，`chat_ack` 也永远不会发。
3. **玩家聊天用 `disguised_chat`。** body 是 `message NBT + VarInt 1 + name NBT + 00`。显示 `<Name> message`，尊重客户端的聊天设置，叙述也正确，所有接收者共用一份字节。`system_chat` 留给加入、离开和错误提示。
4. **服务端不处理 `chat` 时，发送者什么也看不到。** 客户端没有本地回显，也不会报错。
5. **`commands` 包可以不发。** 没收到时客户端的命令树是空根节点，和收到最小命令树完全一样。玩家输入 `/foo bar` 时客户端照样发 `chat_command`，文本是 `foo bar`。
6. **进服序列里省略的包没有一个会导致崩溃。** 有 5 个问题要处理：
   - 自己的皮肤外层和左手设置不生效，需要 `set_entity_data`。
   - 创造模式触及距离偏短，需要 `update_attributes`。
   - 统计界面一直转圈，需要回 `award_stats`。
   - 多人需要完整的 `player_info_update` 和 `player_info_remove`。
   - 区块要多发一圈，见第 9 节。
7. **“回头只看到草方块顶面”和光照无关。** 代码指向 Bamboo 少发了一圈区块：最外圈缺邻居，客户端不编译它，里面一圈朝外的侧面又被最外圈的方块数据剔除掉了。详见第 9 节。
8. **记忆条目 `bamboo-vanilla-verification` 需要更正。** 它写着 “26.3 has no `hasAllNeighbors`”，但 `SectionUpdateTracker.hasAllNeighbors` 存在，`LevelExtractor.extract` 用它决定 section 能否首次编译。

---

## 2. 文本组件的网络格式

来源：`ComponentSerialization.createCodec`，`ByteBufCodecs.fromCodecWithRegistriesTrusted`，`FriendlyByteBuf.writeNbt`，`NbtIo.writeAnyTag`，`ListTag.write`，`TranslatableContents.MAP_CODEC`，`Style.Serializer.MAP_CODEC`。

### 2.1 编码规则

- 写法是 `NbtIo.writeAnyTag`：1 字节 tag 类型，后面直接是内容。**根节点没有名字，也没有长度前缀。**
- 纯文本、没有样式、没有子组件时（`Component.tryCollapseToString`）写成 `TAG_String`。其他情况写成 `TAG_Compound`。
- 解码时也接受非空的 `TAG_List`，元素是组件，会拼接起来。
- 字符串是 Java Modified UTF-8：2 字节大端长度 + 内容。Bamboo 的 `mc_bamboo_nbt::Tag` 已实现。
- Compound 的键没有顺序要求。官方的顺序来自 HashMap，手写成其他顺序也能解码 **[官方解码]**。

Compound 的键。这里只列 M2 用得到的，不是全部：样式还有 `shadow_color`、`font`，内容类型还有 `keybind`、`score`、`selector`、`nbt`、`object`（`Style.Serializer.MAP_CODEC`、`ComponentSerialization.bootstrap`）。

| 键 | NBT 类型 | 含义 |
|---|---|---|
| `text` | String | 字面文本 |
| `translate` | String | 翻译键 |
| `fallback` | String | 可选，翻译键不存在时使用 |
| `with` | List | 可选，翻译参数。元素是字符串、数字、布尔或组件 |
| `extra` | List | 可选，子组件，**必须非空** |
| `color` | String | 颜色名或 `#RRGGBB` |
| `bold` `italic` `underlined` `strikethrough` `obfuscated` | Byte | 0 或 1 |
| `insertion` | String | Shift 点击插入的文本 |
| `click_event` | Compound | `{action:"suggest_command",command:"..."}` 等 |
| `hover_event` | Compound | `{action:"show_entity",id:"minecraft:player",uuid:[I;...],name:...}` 等 |

颜色名：`black` `dark_blue` `dark_green` `dark_aqua` `dark_red` `dark_purple` `gold` `gray` `dark_gray` `blue` `green` `aqua` `red` `light_purple` `yellow` `white`（`TextColor`）。

`type` 键可以不写。官方编码时不写，解码时有 `type` 就按它分发，没有就依次尝试各种内容（`StrictEither`、`FuzzyCodec`）。

样式从父组件向下继承（`Component.visit`、`TranslatableContents.visit`），所以 `translate` 上的 `color` 对字符串参数也生效。

### 2.2 NBT 列表的陷阱

`with` 和 `extra` 都是 NBT 列表，而 NBT 列表只有一个元素类型。官方的处理（`ListTag.write`、`identifyRawElementType`）：

- 元素类型全部相同时，写成这个类型。
- 类型不同时，元素类型写 `10`，非 compound 的元素包成 `{"": 值}`。读取时 `addAndUnwrap` 再解开。

Bamboo 的 `Tag::List` 要求元素类型相同。不需要实现包装，只要保持列表同类型即可：要么全是字符串，要么全是 compound，纯文本元素写成 `{text:"..."}`。这两种写法都已 **[官方解码]**。

### 2.3 例子

纯文本 `hi` **[官方编码]**：

```
08 00 02 68 69
```

纯文本 `竹子` **[官方编码]**：

```
08 00 06 e7 ab b9 e5 ad 90
```

带参数和颜色的翻译，SNBT 是 `{translate:"multiplayer.player.joined",with:["Kitten"],color:"yellow"}`，共 77 字节 **[官方解码]**：

```
0a                                      TAG_Compound，根没有名字
08 00 09 74 72 61 6e 73 6c 61 74 65     TAG_String "translate"
   00 19 6d 75 6c 74 69 70 6c 61 79 65 72 2e 70 6c 61 79 65 72 2e 6a 6f 69 6e 65 64
09 00 04 77 69 74 68                    TAG_List "with"
   08 00 00 00 01                       元素类型 String，1 个元素（Int 长度）
   00 06 4b 69 74 74 65 6e              "Kitten"
08 00 05 63 6f 6c 6f 72                 TAG_String "color"
   00 06 79 65 6c 6c 6f 77              "yellow"
00                                      TAG_End
```

参数带样式时全部写成 compound，SNBT 是 `{translate:"chat.type.text",with:[{text:"Kitten",color:"gold"},{text:"hello"}]}` **[官方解码]**：

```
0a
08 00 09 "translate" 00 0e "chat.type.text"
09 00 04 "with" 0a 00 00 00 02          元素类型 Compound，2 个元素
   08 00 04 "text" 00 06 "Kitten"  08 00 05 "color" 00 04 "gold"  00
   08 00 04 "text" 00 05 "hello"   00
00
```

官方在这种情况下的写法是 `with:[{color:"gold",text:"Kitten"},"hello"]`，字符串元素在网络上是 `08 00 00 00 05 "hello" 00`，也就是 `{"":"hello"}` **[官方编码]**。

### 2.4 客户端解码失败的后果

下面这些写法都会让官方解码抛出 `DecoderException`：

| 写法 | 错误 |
|---|---|
| `{text:"x",extra:[]}` | `List must have contents` |
| `{text:"x",color:"bad"}` | `Invalid color name: bad` |

客户端走 `Connection.exceptionCaught`，以 `Internal Exception` 断线。`isSkippable()` 只在 `PacketEncoder` 里生效，对解码没有作用。`PacketDecoder.decode` 发现包末尾有多余字节时同样抛异常断线。

`with:[]` 和缺少 `with` 都能解码，但占位符不够时显示原始模板 `<%s> %s`。这两种情况都已用官方解码验证。

---

## 3. 聊天怎样到达服务端

来源：`ChatScreen.handleChatInput`，`ClientPacketListener.handleLogin`、`sendChat`、`sendCommand`，`SignedMessageChain.Encoder.UNSIGNED`，`ServerGamePacketListenerImpl.handleChat`，`SignedMessageChain.Decoder.unsigned`。

### 3.1 客户端

1. `handleLogin` 把 `chatSession` 设为 `null`，把 `signedMessageEncoder` 设为 `UNSIGNED`。只有 `packet.onlineMode()` 为真时才调用 `prepareKeyPair()`。Bamboo 发 `online_mode=false`，所以这个连接上永远没有密钥。
2. `handleLogin` 还会弹出一次 “Chat messages can't be verified” 提示（`serverData != null && !seenInsecureChatWarning && !enforcesSecureChat()`）。这是正常行为。
3. 回车时 `ChatScreen.keyPressed` 先检查 `commandSuggestions.hasAllowedInput()`。聊天可见性是 `HIDDEN` 或 `SYSTEM`，或者启动器、账号禁止聊天时，普通消息不会发出。离线启动器不受账号限制：`Minecraft` 构造函数在 `offlineDeveloperMode` 时用 `UserApiService.OFFLINE`，`fetchProperties` 抛 `AuthenticationException` 时回退到 `OFFLINE_PROPERTIES`，这两条路都得到含 `CHAT_ALLOWED` 的属性。正版账号的权限来自服务端（`MinecraftServicesUserApiService.fetchProperties`），账号被禁聊时这里同样拦截。
4. `handleChatInput` 对文本做 `trim`、`normalizeSpace`、截断到 256，空字符串不发。以 `/` 开头的走 `sendCommand`，其他走 `sendChat`。
5. `sendChat` 取当前时间和随机 salt，调用 `lastSeenMessages.generateAndApplyUpdate()`。`UNSIGNED.pack` 返回 `null`，所以 `signature` 是 `Optional.empty()`。
6. 发送 `ServerboundChatPacket`。**没有本地回显**，`addRecentChat` 只记录输入历史。

### 3.2 官方服务端

1. `unpackAndApplyLastSeen` 校验 `Update`，失败时断线，原因是 `multiplayer.disconnect.chat_validation_failed`。
2. `tryHandleChat`：
   - 含非法字符时断线，原因是 `multiplayer.disconnect.illegal_characters`。
   - 服务端记录的 `chatVisibility` 是 `HIDDEN` 时回一条红色的 `chat.disabled.options`。
3. `signedMessageDecoder` 初始是 `Decoder.unsigned`。没有强制安全聊天时返回 `PlayerChatMessage.unsigned(profileId, body.content())`。**客户端的 timestamp 和 salt 在这里被丢弃**，时间换成 `Instant.now()`，salt 是 0，lastSeen 是空。
4. `broadcastChatMessage` 用 `ChatType.bind(ChatType.CHAT, player)`，名字是 `player.getDisplayName()`。
5. `OutgoingChatMessage.create` 只在发送者是全零 UUID 时才选 `Disguised`。真实玩家的未签名消息走 `Player` 分支，所以**官方离线服务器发的是没有签名的 `player_chat`**，不是 `disguised_chat`。

### 3.3 `LastSeenMessages.Update`

来源：`LastSeenMessages.Update`，`LastSeenMessagesTracker.generateAndApplyUpdate`，`LastSeenMessagesValidator.applyUpdate`，`ByteBufCodecs.fixedBitSet`，`FriendlyByteBuf.writeFixedBitSet`。

| 字段 | 编码 | 含义 |
|---|---|---|
| `offset` | `VarInt` | 上次发送之后新处理的消息数，发送后清零 |
| `acknowledged` | 3 字节定长位集 | 20 位窗口，`ceil(20/8)=3` |
| `checksum` | `Byte` | 见下 |

位集按 `BitSet.toByteArray` 写：第 `i` 位在第 `i/8` 字节的第 `i%8` 位，低字节在前，不足 3 字节时补零。

`offset=3`，位 `{0,9,19}`，`checksum=0x5a` 的编码 **[官方编码]**：

```
03          offset
01 02 08    位 0 在 01；位 9 是字节 1 的第 1 位，即 02；位 19 是字节 2 的第 3 位，即 08
5a          checksum
```

校验和（`LastSeenMessages.computeChecksum`）：

```
c = 1                                       // int32，溢出回绕
for sig in lastSeen.entries:
    c = 31 * c + Arrays.hashCode(sig.bytes) // 字节按有符号值参与
b = (byte) c
return b == 0 ? 1 : b
```

空列表得到 `1`。一个全零签名时 `Arrays.hashCode` 是 `-394403839`，结果是 `32`（官方代码实跑）。服务端的 `verifyChecksum` 接受 `0`（`IGNORE_CHECKSUM`）或相等的值。

**对着 Bamboo，这 5 个字节永远是 `00 00 00 00 01`。** 客户端的窗口只由 `ChatListener` 在 `message.signature() != null` 时调用 `markMessageAsProcessed` 来填充。`system_chat`、`disguised_chat` 和未签名的 `player_chat` 都不会进窗口。

官方校验器对这个值返回空列表，通过。`offset=1` 时报 `Advanced last seen window by 1 messages, but expected at most 0`，校验和是 7 时报 `Checksum mismatch`（官方代码实跑）。

### 3.4 服务端忽略时会怎样

| 忽略什么 | 后果 |
|---|---|
| 整个 `chat` 包 | 发送者和其他人都看不到消息。没有报错，没有断线，客户端不等待回应 |
| 只忽略 `Update`（解码但不校验） | 没有影响。Bamboo 没发过签名消息，这些字段没有可以对照的状态 |
| 不解码 `Update`，也不检查包长 | 能工作，但放过了畸形包。建议完整解码并调用 `reader.finish()` |

---

## 4. Serverbound 包

### `minecraft:chat`

ID 9（`0x09`），serverbound，Play。来源：`ServerboundChatPacket.STREAM_CODEC`，`ServerGamePacketListenerImpl.handleChat`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `message` | `String(256)` | 聊天文本 |
| `timeStamp` | `Long` | 毫秒时间戳（`ByteBufCodecs.INSTANT`） |
| `salt` | `Long` | 随机值 |
| `signature` | `Opt<256 字节>` | 定长，没有长度前缀。对着 Bamboo 总是 `00` |
| `lastSeen.offset` | `VarInt` | 见 3.3 |
| `lastSeen.acknowledged` | 3 字节 | 见 3.3 |
| `lastSeen.checksum` | `Byte` | 见 3.3 |

离线客户端发送 `hi` 的 body，共 25 字节 **[官方编码]**：

```
02 68 69                     message
00 00 01 8b cf e5 68 00      timeStamp（示例值）
11 22 33 44 55 66 77 88      salt（示例值）
00                           signature 不存在
00                           offset
00 00 00                     acknowledged
01                           checksum
```

服务端要做的事：

1. 完整解码，然后 `reader.finish()`。任何解码错误都断开连接。
2. 逐字符检查 `StringUtil.isAllowedChatCharacter`：字符小于 32、等于 127 或等于 167（`§`）都不允许。发现后用 `{translate:"multiplayer.disconnect.illegal_characters"}` 断线。这一步不能省，`§` 能通过 NBT 解码。
3. 已解析 `client_information` 且可见性是 `HIDDEN` 时，回 `system_chat`：`{translate:"chat.disabled.options",color:"red"}`。正常客户端不会走到这里，见 3.1。
4. 把消息交给世界线程，按第 5.4 节广播给所有在线玩家，**包括发送者**。
5. 控制台记录 `<Name> message`。官方对未签名消息的日志是 `[Not Secure] <Name> message`（`PlayerList.broadcastChatMessage` 给 `logChatMessage` 传 `"Not Secure"`），Bamboo 不需要这个前缀。桌面控制台要显示的话，给 `ServerEvent` 加一项。
6. 限流可以以后再做。官方是 `TickThrottler(20, 20 * chat-spam-threshold-seconds)`，默认阈值 200（`DedicatedServerProperties`）：每条消息加 20，每 tick 减 1，`count >= 200` 且不是 OP 或单人房主时以 `disconnect.spam` 踢出（`detectRateSpam`）。

timestamp、salt、signature 和 lastSeen 都不使用。

### `minecraft:chat_command`

ID 7（`0x07`），serverbound，Play。来源：`ServerboundChatCommandPacket`，`ClientPacketListener.sendCommand`，`handleChatCommand`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `command` | `String(32767)` | 不含开头的 `/` |

`/foo bar` 的 body **[官方编码]**：

```
07 66 6f 6f 20 62 61 72
```

服务端要做的事：做和 `chat` 相同的非法字符检查（`tryHandleChat` 的 `isCommand=true` 分支不检查可见性），然后执行命令。不认识的命令按第 7.4 节回错误。不回应时玩家什么也看不到。

### `minecraft:chat_command_signed`

ID 8（`0x08`），serverbound，Play。来源：`ServerboundChatCommandSignedPacket`，`ArgumentSignatures`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `command` | `String(32767)` | 不含 `/` |
| `timeStamp` | `Long` | 毫秒 |
| `salt` | `Long` | |
| `argumentSignatures` | `VarInt` 个数（≤ 8），每项 `name: String(16)` + `signature: 256 字节` | |
| `lastSeen` | 同 `chat` | 5 字节 |

只有解析结果里存在 `SignedArgument` 类型的参数时才发（`sendCommand`）。26.3 只有 `MessageArgument`（`minecraft:message`，ID 20）实现了它。**命令树里不放 `minecraft:message` 就永远收不到这个包。**

如果放了，离线客户端的签名器返回 `null`，`ArgumentSignatures.signCommand` 把它过滤掉，所以个数是 0。`say hi` 的 body **[官方编码]**：

```
06 73 61 79 20 68 69         command
00 00 01 8b cf e5 68 00      timeStamp
11 22 33 44 55 66 77 88      salt
00                           argumentSignatures 个数
00 00 00 00 01               lastSeen
```

服务端要做的事：M2 不需要处理，归入 `Other`。以后加 `minecraft:message` 时按 `chat_command` 处理 `command` 即可。

### `minecraft:chat_ack`

ID 6（`0x06`），serverbound，Play。来源：`ServerboundChatAckPacket`，`ClientPacketListener.markMessageAsProcessed`、`sendChatAcknowledgement`。

| 字段 | 编码 |
|---|---|
| `offset` | `VarInt` |

只有 `offset > 0` 才发，发送时机是窗口累计超过 64 或进入 Configuration。没有签名消息时不会发生。服务端要做的事：不处理，归入 `Other`。

### `minecraft:chat_session_update`

ID 10（`0x0A`），serverbound，Play。来源：`ServerboundChatSessionUpdatePacket`，`RemoteChatSession.Data`，`ProfilePublicKey.Data`。

| 字段 | 编码 |
|---|---|
| `sessionId` | `UUID` |
| `expiresAt` | `Long` 毫秒 |
| `publicKey` | `VarInt` 长度（≤ 512）+ 字节 |
| `keySignature` | `VarInt` 长度（≤ 4096）+ 字节 |

只由 `setKeyPair` 发送，而密钥对只在 `online_mode=true` 时准备。服务端要做的事：不处理，归入 `Other`。

### `minecraft:client_information`

Configuration ID 0（`0x00`），Play ID 14（`0x0E`），serverbound。body 相同。来源：`ClientInformation`，`ServerboundClientInformationPacket`，`ClientHandshakePacketListenerImpl`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `language` | `String(16)` | |
| `viewDistance` | `Byte` | 客户端渲染距离 |
| `chatVisibility` | `VarInt` | 0 `FULL`，1 `SYSTEM`，2 `HIDDEN` |
| `chatColors` | `Bool` | |
| `modelCustomisation` | `Byte`（无符号） | 掩码：cape `0x01`，jacket `0x02`，left_sleeve `0x04`，right_sleeve `0x08`，left_pants `0x10`，right_pants `0x20`，hat `0x40` |
| `mainHand` | `VarInt` | 0 `LEFT`，1 `RIGHT` |
| `textFilteringEnabled` | `Bool` | |
| `allowsListing` | `Bool` | |
| `particleStatus` | `VarInt` | 0 `ALL`，1 `DECREASED`，2 `MINIMAL` |

`zh_cn`、视距 12、`FULL`、颜色开、全部外层、右手的 body **[官方编码]**：

```
05 7a 68 5f 63 6e  0c  00  01  7f  01  00  01  00
```

枚举越界不是解码错误：`chatVisibility`、`particleStatus` 取模回绕，`mainHand` 落到 `LEFT`（各枚举的 `ByIdMap.OutOfBoundsStrategy`）。

发送时机：

- Configuration：`login_acknowledged` 之后紧跟着发，顺序是 `custom_payload`（brand）、`client_information`（`ClientHandshakePacketListenerImpl`）。
- Play：`Options.save()` → `broadcastOptions()` → `broadcastClientInformation`，内容和上次不同才发。比较基准 `remoteClientInformation` 初值是 `null`，所以进入 Play 后第一次 `save` 一定会发，内容可能和 Configuration 里那次相同。服务端不能假设收到就是有变化。

服务端要做的事：两个状态都要解码，保存 `viewDistance`、`chatVisibility`、`modelCustomisation`、`mainHand`。现在 Configuration 里把它归入 `Other` 丢掉了。Play 里收到后：

- 外层或主手变化时发 `set_entity_data`，给自己和看得到这名玩家的人。
- 帽子位变化时广播 `player_info_update`，动作 `UPDATE_HAT`（`handleClientInformation`）。

### `minecraft:client_command`

ID 12（`0x0C`），serverbound，Play。来源：`ServerboundClientCommandPacket`，`handleClientCommand`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `action` | `VarInt` | 0 `PERFORM_RESPAWN`，1 `REQUEST_STATS`，2 `REQUEST_GAMERULE_VALUES` |

编码是 `writeEnum`，也就是序号的 VarInt。越界时官方解码失败并断开（`readEnum` 数组越界）。

服务端要做的事：`action=1` 时回 `award_stats`（第 8.4 节）。其他值 M2 忽略：`action=0` 只在死亡重生或看完通关字幕时发（`LocalPlayer.respawn`、`handleGameEvent` 的 `WIN_GAME`），Bamboo 两者都没有；`action=2` 由 `InWorldGameRulesScreen` 发，官方也只回给有 `COMMANDS_GAMEMASTER` 权限的玩家（`sendGameRuleValues`）。

### `minecraft:ping_request` 和 `minecraft:pong_response`

`ping_request`：ID 38（`0x26`），serverbound，Play。`pong_response`：ID 63（`0x3F`），clientbound，Play。来源：`ServerboundPingRequestPacket`，`ClientboundPongResponsePacket`，`ServerGamePacketListenerImpl.handlePingRequest`，`PingDebugMonitor`，`ClientPacketListener.tick`、`handlePongResponse`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `time` | `Long` | 客户端的 `Util.getMillis()`，服务端原样返回 |

两个包的 body 相同。例子 **[官方编码]**：

```
00 00 01 8b cf e5 68 00
```

只有打开调试界面的网络图表时才发（`DebugScreenOverlay.showNetworkCharts`），每个客户端 tick 一个。

服务端要做的事：原样回 `pong_response`。官方不经过主线程，在网络线程直接回，所以 Bamboo 在连接任务里回就行，不用进世界线程。不回时图表没有数据，没有其他影响。

### `minecraft:keep_alive`

clientbound：ID 45（`0x2D`），Play。serverbound：ID 28（`0x1C`），Play。M1 已实现。来源：`ClientboundKeepAlivePacket`，`ServerboundKeepAlivePacket`，`ServerCommonPacketListenerImpl.keepConnectionAlive`、`handleKeepAlive`，`ClientCommonPacketListenerImpl.handleKeepAlive`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `id` | `Long` | 服务端选的值，客户端原样返回。官方用毫秒时间，M1 用递增计数，都可以 |

`id=1` 的 body **[官方编码]**：

```
00 00 00 00 00 00 00 01
```

服务端要做的事：

1. 距上次发送满 15 秒时，上一个还没回应就断开，原因是 `disconnect.timeout`，否则发一个新的。
2. 收到回应：`id` 匹配时清除等待，用 `latency = (latency * 3 + rtt_ms) / 4` 更新延迟，初值 0。不匹配或没有在等时，官方同样以 `disconnect.timeout` 断开。

M2 的心跳留在连接任务里，不进世界线程，计时不能依赖写 socket 完成。原因和两侧的超时见 `coverage.md` 第 5 节。

---

## 5. Clientbound 聊天包

### `minecraft:system_chat`

ID 124（`0x7C`），clientbound，Play。来源：`ClientboundSystemChatPacket`，`ClientPacketListener.handleSystemChat`，`ChatListener.handleSystemMessage`、`handleOverlay`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `content` | `NBT` | 文本组件 |
| `overlay` | `Bool` | 1 时显示在物品栏上方的动作栏 |

加入消息的完整帧，帧长 79 **[官方解码]**：

```
4f                  帧长
7c                  包 ID
0a ... 00           content，2.3 的 77 字节
00                  overlay
```

官方写出的 body 只有键的顺序不同（`with`、`color`、`translate`），共 78 字节 **[官方编码]**。

### `minecraft:disguised_chat`

ID 33（`0x21`），clientbound，Play。来源：`ClientboundDisguisedChatPacket`，`ChatType.Bound.STREAM_CODEC`，`ByteBufCodecs.holder`，`ChatListener.handleDisguisedChatMessage`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `message` | `NBT` | 消息内容 |
| `chatType` | `VarInt` | **注册表 ID + 1**。0 表示后面跟内联定义，Bamboo 不用 |
| `name` | `NBT` | 发送者名字，填入 `sender` 参数 |
| `targetName` | `Opt<NBT>` | `target` 参数，普通聊天不需要 |

`chat_type` 注册表的 ID（官方代码实跑，和 `SYNCED_REGISTRIES` 的顺序一致）：

| ID | 条目 | 线上的值 | 模板 |
|---|---|---|---|
| 0 | `minecraft:chat` | 1 | `<%s> %s` |
| 1 | `minecraft:emote_command` | 2 | `* %s %s` |
| 2 | `minecraft:msg_command_incoming` | 3 | `%s whispers to you: %s` |
| 3 | `minecraft:msg_command_outgoing` | 4 | |
| 4 | `minecraft:say_command` | 5 | `[%s] %s` |
| 5 | `minecraft:team_msg_command_incoming` | 6 | |
| 6 | `minecraft:team_msg_command_outgoing` | 7 | |

`Kitten` 说 `hi` 的完整帧，帧长 17 **[官方编码]**：

```
11                          帧长
21                          包 ID
08 00 02 68 69              message "hi"
01                          chatType：minecraft:chat
08 00 06 4b 69 74 74 65 6e  name "Kitten"
00                          targetName 不存在
```

官方解码后 `chatType.decorate(message)` 的文本是 `<Kitten> hi`，`decorateNarration` 的文本是 `Kitten says hi`。

### `minecraft:player_chat`

ID 66（`0x42`），clientbound，Play。来源：`ClientboundPlayerChatPacket`，`SignedMessageBody.Packed`，`FilterMask`，`ClientPacketListener.handlePlayerChat`。

| 字段 | 编码 | 说明 |
|---|---|---|
| `globalIndex` | `VarInt` | 每个连接从 0 开始，严格递增 |
| `sender` | `UUID` | 必须已在 `player_info_update` 里 |
| `index` | `VarInt` | 未签名时是 0 |
| `signature` | `Opt<256 字节>` | |
| `body.content` | `String(256)` | |
| `body.timeStamp` | `Long` | 毫秒 |
| `body.salt` | `Long` | |
| `body.lastSeen` | `VarInt` 个数（≤ 20），每项 `VarInt(缓存 ID + 1)`，为 0 时跟 256 字节 | |
| `unsignedContent` | `Opt<NBT>` | |
| `filterMask` | `VarInt` 类型 | 0 `PASS_THROUGH`，1 `FULLY_FILTERED`，2 `PARTIALLY_FILTERED`（后面跟 `VarInt` 字节数 + 位集） |
| `chatType` | 同 `disguised_chat` | holder、name、targetName |

官方离线服务器发的结构，共 52 字节 **[官方编码]**。这里的 `name` 简化成了纯文本。官方写的是 `player.getDisplayName()`，也就是第 6 节那个带 `click_event`、`hover_event`、`insertion` 的 compound，这时 body 共 248 字节（官方代码实跑）：

```
00                                                 globalIndex
db 80 81 6f ce 8f 33 40 b6 e5 6d ef cb 8c 28 ef    sender
00                                                 index
00                                                 signature 不存在
02 68 69                                           content
00 00 01 8b cf e5 68 00                            timeStamp
00 00 00 00 00 00 00 00                            salt
00                                                 lastSeen 个数
00                                                 unsignedContent 不存在
00                                                 filterMask
01  08 00 06 4b 69 74 74 65 6e  00                 chatType、name、targetName
```

**M2 不需要这个包。** 用它要注意：

- `globalIndex` 错了客户端断线，原因是 `multiplayer.disconnect.bad_chat_index`，而且每个接收者的值不同，不能共用字节。
- `sender` 不在玩家列表时显示 `chat.validation_error`。
- 客户端打开 “Only Show Secure Chat” 时不显示。

### 5.4 三种方式的比较

都能显示 `<Name> message`。来源：`ChatListener`，`Minecraft.computeChatAbilities`，`ChatRestriction`，`ChatAbilities`，`ChatComponent`，`GuiMessageTag`，`ChatTrustLevel`。

| | `system_chat` | `disguised_chat` | `player_chat`（无签名） |
|---|---|---|---|
| 依赖注册表 | 否 | `chat_type`，Bamboo 已同步 | `chat_type` |
| 谁拼 `<Name>` | 服务端写 `chat.type.text` | 客户端按 `ChatType` 拼 | 客户端 |
| 每个接收者字节相同 | 是 | 是 | 否，`globalIndex` 不同 |
| 可见性 `SYSTEM`（Commands Only） | **仍显示** | 不显示 | 不显示 |
| 可见性 `HIDDEN` | 不显示 | 不显示 | 不显示 |
| 启动器或账号禁止聊天 | **仍显示** | 不显示 | 不显示 |
| 聊天延迟选项 | 不排队 | 排队 | 排队 |
| 屏蔽列表 | 从文本猜 `<...>` 里的名字，`hideMatchedNames` 默认开 | 不检查 | 按 UUID 检查 |
| 叙述 | 读整句 | `Kitten says hi` | `Kitten says hi` |
| 左侧标记 | `systemSinglePlayer`，灰条 `0xD0D0D0` | `system`，同色灰条 | `chatNotSecure`，同色灰条 |
| “Only Show Secure Chat” | 不受影响 | 不受影响 | 不显示 |

**结论：玩家聊天用 `disguised_chat`。** 它是这三者里最简单的正确做法：比 `system_chat` 只多一个 VarInt 和一个名字，但遵守玩家的聊天设置。用 `system_chat` 发玩家聊天会绕过 “Commands Only” 和账号禁聊，也就是把用户关掉的内容硬塞给他。

M2 的广播做法：`message` 写玩家输入的文本（`TAG_String`），`chatType=1`，`name` 写玩家名（`TAG_String`），`targetName=00`。编码一次，把同一份帧放进每个已进入 Play 的玩家的发送队列（为什么不能用现在的玩家表，见第 6 节）。官方服务端只发给可见性是 `FULL` 的玩家（`acceptsChatMessages`），也不给 `HIDDEN` 的玩家发 `overlay=false` 的系统消息（`acceptsSystemMessages`）。客户端自己也会过滤，M2 可以全发。

`name` 以后可以换成带 `hover_event` 的 compound，结构见第 6 节。

---

## 6. 加入和离开消息

来源：`PlayerList.placeNewPlayer`、`broadcastSystemMessage`，`ServerGamePacketListenerImpl.removePlayerFromWorld`，`Player.getDisplayName`、`decorateDisplayNameComponent`，`Entity.createHoverEvent`。

| 事件 | 翻译键 | 英文文本 | 参数 | 样式 |
|---|---|---|---|---|
| 加入 | `multiplayer.player.joined` | `%s joined the game` | 显示名 | `color:"yellow"` |
| 改名后加入 | `multiplayer.player.joined.renamed` | `%s (formerly known as %s) joined the game` | 显示名、旧名 | `color:"yellow"` |
| 离开 | `multiplayer.player.left` | `%s left the game` | 显示名 | `color:"yellow"` |

颜色写在外层的 translate 组件上（`.withStyle(ChatFormatting.YELLOW)`），参数继承它。两者都用 `system_chat`，`overlay=false`。

官方的完整结构 **[官方编码]**：

```
{color:"yellow",translate:"multiplayer.player.joined",with:[{
  click_event:{action:"suggest_command",command:"/tell Kitten "},
  hover_event:{action:"show_entity",id:"minecraft:player",name:"Kitten",
               uuid:[I;-612335249,-829476032,-1226478097,-880006929]},
  insertion:"Kitten",
  text:"Kitten"}]}
```

`uuid` 是 `TAG_Int_Array`，4 个 Int，字节就是 UUID 的 16 字节（`0b 00 04 "uuid" 00 00 00 04 db 80 81 6f ...`）。

Bamboo 的 M2 用简化结构 `{translate:"multiplayer.player.joined",with:["Kitten"],color:"yellow"}`，字节见 2.3。少了点击填 `/tell`、悬停提示和 Shift 插入，显示文本相同。不要加 `click_event`，Bamboo 没有 `/tell`。

`multiplayer.player.joined.renamed` 依赖名字缓存，Bamboo 没有存档，不会用到。

时机：

- **加入者收不到自己的加入消息。** `broadcastSystemMessage` 遍历 `this.players`，而这时新玩家还没加入列表（`this.players.add(player)` 在后面）。
- 官方在 `placeNewPlayer` 里广播，也就是 Configuration 结束之后。Bamboo 的 `PlayerSession::join` 在 Login 阶段调用，**不能在那里发**。
- **广播目标是已进入 Play 的连接，不是 `Shared` 的玩家表。** `PlayerSession::join` 在 Login 阶段就写入这张表，这时连接还在 Configuration，给它发 Play 的包，客户端会按 Configuration 的包 ID 解析。官方的 `PlayerList.players` 只在 `placeNewPlayer` 里加入，所以聊天、加入离开消息和 `player_info_update` 都不会发到 Configuration 里的连接。
- 离开时的顺序：广播 `left`，`level.removePlayerImmediately`（其他人收到 `remove_entities`），从列表移除，广播 `player_info_remove`。
- 只有进入过 Play 的玩家才发离开消息。在 Configuration 里断开的玩家不发。现在的 `PlayerSession::drop` 无法区分这两种情况。

---

## 7. `commands` 包和命令输入

### 7.1 `minecraft:commands`

ID 16（`0x10`），clientbound，Play。来源：`ClientboundCommandsPacket`，`Commands.sendCommands`，`ClientPacketListener.handleCommands`。

| 字段 | 编码 |
|---|---|
| `nodes` | `List<Node>` |
| `rootIndex` | `VarInt` |

Node（`Entry.write`、`readNode`）：

| 字段 | 编码 | 出现条件 |
|---|---|---|
| `flags` | `Byte` | 总有 |
| `children` | `VarInt` 个数 + `VarInt` 下标 | 总有 |
| `redirect` | `VarInt` | `flags & 0x08` |
| `name` | `String(32767)` | 类型是 literal 或 argument |
| `parser` | `VarInt`，`minecraft:command_argument_type` 的 ID | 类型是 argument |
| `properties` | 由 parser 决定 | 类型是 argument |
| `suggestion` | `Identifier` | 类型是 argument，且 `flags & 0x10`。literal 节点带这一位时不读这个字段（`ClientboundCommandsPacket.read`） |

`flags` 的位：

| 位 | 含义 |
|---|---|
| `0x03` | 类型：0 root，1 literal，2 argument |
| `0x04` | 可执行 |
| `0x08` | 有 redirect |
| `0x10` | 有自定义补全，只对 argument 节点有效 |
| `0x20` | restricted：节点要求权限（`COMMAND_NODE_INSPECTOR.isRestricted`）。手打命令不受影响，`suggestionsProvider` 总带 `ALLOW_RESTRICTED_COMMANDS`。它只影响 `sendUnattendedCommand`（点击事件、书、对话框触发的命令）：`verifyCommand` 用无权限的 provider 解析，失败时弹确认框。Bamboo 不用置这一位 |

解析器 ID（`registries.json`）：`brigadier:bool` 0，`brigadier:float` 1，`brigadier:double` 2，`brigadier:integer` 3，`brigadier:long` 4，`brigadier:string` 5，`minecraft:entity` 6，`minecraft:game_profile` 7，`minecraft:vec3` 10，`minecraft:message` 20，`minecraft:gamemode` 42。

### 7.2 最小合法命令树

一个 root 节点 **[官方编码]**，`NodeResolver.resolve` 和 `validateEntries` 都能通过：

```
05          帧长
10          包 ID
01          节点数
00 00       root：flags=0，没有子节点
00          rootIndex
```

一个可执行的 `/help` **[官方解码]**：

```
02                     节点数
00  01 01              root：1 个子节点，下标 1
05  00  04 68 65 6c 70 literal：flags=1|4，没有子节点，名字 "help"
00                     rootIndex
```

`/say <text>`，参数是贪婪字符串 **[官方编码]**：

```
03                                 节点数
00  01 01                          root，子节点 1
01  01 02  03 73 61 79             literal "say"，子节点 2
06  00  04 74 65 78 74  05  02     argument：flags=2|4，名字 "text"，parser brigadier:string，properties 2（greedy）
00                                 rootIndex
```

`brigadier:string` 的 properties 是 `VarInt`：0 单词，1 可引号短语，2 贪婪。文本参数用它，不用 `minecraft:message`，这样客户端不走 `chat_command_signed`。

### 7.3 没收到 `commands` 时

`ClientPacketListener.commands` 的初值是 `new CommandDispatcher()`，只有一个空根节点。这和收到 7.2 的最小树**完全等价**。Bamboo 还没有命令时可以不发。

玩家输入 `/foo bar` 时：

1. `CommandSuggestions.updateCommandInfo` 用空树解析，输入框上方显示解析错误，内容来自 `command.unknown.command` 和 `command.context.parse_error`。
2. 回车不受影响。只有 `HIDDEN` 时 `hasAllowedInput()` 才拦截命令。
3. `sendCommand("foo bar")` 解析后没有可签名参数（官方代码实跑：`signable args=0`），发 `chat_command`。
4. 客户端不做其他事。服务端不回应时玩家什么也看不到。

Tab 补全：`command_suggestion` 由 `ClientSuggestionProvider.customSuggestion` 发送。触发条件是节点的 suggestion 为 `minecraft:ask_server`（客户端不认识的标识符也回退到它，`SuggestionProviders.getProvider`），或者参数要补全一个客户端没有的注册表（`suggestRegistryElements` 的回退）。空树和上面的例子都不会触发。

### 7.4 未知命令的回应

官方在 `Commands.finishParsing` 里发两条，每条都经过 `CommandSourceStack.sendFailure` 包一层红色。空根节点下解析 `foo bar` 的结果：没有异常，`cursor=0`，走 `dispatcherUnknownCommand`（官方代码实跑）。

第一条 **[官方编码]**：

```
{color:"red",extra:[{translate:"command.unknown.command"}],text:""}
```

第二条 **[官方编码]**：

```
{color:"red",text:"",extra:[{
  click_event:{action:"suggest_command",command:"/foo bar"},
  color:"gray",text:"",
  extra:[{color:"red",text:"foo bar",underlined:1b},
         {color:"red",italic:1b,translate:"command.context.here"}]}]}
```

文本是 `Unknown or incomplete command. See below for error` 和 `foo bar<--[HERE]`。`cursor > 10` 时第二条开头还有 `...`，前面的部分显示最后 10 个字符，空树下 `cursor` 总是 0。

Bamboo 的 M2 只回第一条也可以，写成 `{translate:"command.unknown.command",color:"red"}` 显示效果相同。这个键的文本里提到了下一行，所以两条都回更完整。

---

## 8. 进服序列审计

### 8.1 官方顺序和 Bamboo 的对比

来源：`PlayerList.placeNewPlayer`、`sendPlayerPermissionLevel`、`sendLevelInfo`，`ServerRecipeBook.sendInitialRecipeBook`，`ChunkMap.applyChunkTrackingView`，`ServerPlayer.initInventoryMenu`、`doTick`、`tick`，`ServerEntity.sendDirtyEntityData`。

`play.rs` 的注释说顺序和官方一致。对比后确认：Bamboo 发的包是官方序列的子序列，相对顺序正确。

| # | 包 | ID | 官方时机 | Bamboo M1 |
|---|---|---|---|---|
| 1 | `login` | 50 | `placeNewPlayer` | 有，字段顺序已核对 |
| 2 | `change_difficulty` | 10 | | 无 |
| 3 | `player_abilities` | 65 | | 有 |
| 4 | `set_held_slot` | 107 | | 无 |
| 5 | `update_recipes` | 136 | | 无 |
| 6 | `entity_event` 24..28 | 34 | `sendPlayerPermissionLevel` | 无 |
| 7 | `commands` | 16 | 同上 | 无 |
| 8 | `recipe_book_settings` | 77 | `sendInitialRecipeBook` | 无 |
| 9 | `recipe_book_add` | 75 | 同上 | 无 |
| 10 | 记分板相关 | | `updateEntireScoreboard`，没有记分板时不发 | 不需要 |
| 11 | `system_chat` 加入消息 | 124 | 只发给其他玩家 | 无 |
| 12 | `player_position` | 73 | `teleport` | 有 |
| 13 | `server_data` | 88 | 非 transfer 连接 | 无 |
| 14 | `player_info_update` 已在线玩家 → 新玩家 | 71 | | 无 |
| 15 | `player_info_update` 新玩家 → 所有人 | 71 | | 只发给自己 |
| 16 | `initialize_border` | 44 | `sendLevelInfo` | 无 |
| 17 | `set_time` | 115 | 同上 | 有，只含主世界时钟 |
| 18 | `set_default_spawn_position` | 99 | 同上 | 无 |
| 19 | `game_event` 下雨 | 39 | 下雨时 | 不需要 |
| 20 | `game_event` 13 | 39 | 同上 | 有 |
| 21 | `ticking_state` | 130 | `updateJoiningPlayer` | 无 |
| 22 | `ticking_step` | 131 | 同上 | 无 |
| 23 | `set_chunk_cache_center` | 96 | `level.addNewPlayer` | 有 |
| 24 | `post_effects` | 83 | `sendPostEffects` | 无 |
| 25 | `container_set_content` | 18 | `initInventoryMenu` → `setSynchronizer` → `sendAllDataToRemote` | 无 |
| 之后 | `chunk_batch_*`、`level_chunk_with_light` | 12、46、11 | `PlayerChunkSender` | 有 |
| 之后 | `update_advancements` | 133 | 第一次 `advancements.flushDirty` | 无 |
| 之后 | `set_health` | 106 | `doTick` | 无 |
| 之后 | `set_experience` | 105 | `doTick` | 无 |
| 之后 | `set_entity_data`、`update_attributes` 给自己 | 101、134 | `sendDirtyEntityData` → `sendToTrackingPlayersAndSelf` | 无 |
| 之后 | `waypoint` | 141 | `ServerWaypointManager` | 无 |
| 周期 | `set_time`，时钟列表为空 | 115 | 每 20 tick，`forceGameTimeSynchronization` | 无 |

### 8.2 逐包结论

读客户端代码后的结论。**以下所有包缺失都不会崩溃。**

| 包 | 客户端缺省状态 | 缺失的影响 | M2 |
|---|---|---|---|
| `change_difficulty` | `new ClientLevelData(Difficulty.NORMAL, ...)`，未锁定 | 只有 `WorldOptionsScreen` 等界面读取，显示 Normal | 不用补 |
| `set_held_slot` | `Inventory.selected` 是 0 | 无。服务端的初值也要是 0 | 不用补 |
| `update_recipes` | `ClientRecipeContainer(Map.of(), empty)`，`propertySet` 返回 `EMPTY` | 影响熔炉类、酿造台、锻造台的槽位过滤和切石机列表（`AbstractFurnaceMenu`、`BrewingStandMenu`、`SmithingMenu`、`StonecutterMenu`），还有 `CampfireBlock.useItemOn` 判断手持物能否放上营火。M2 不开容器，放普通方块不受影响 | 不用补 |
| `recipe_book_settings` | `RecipeBookSettings` 全 false | 无 | 不用补 |
| `recipe_book_add` | 配方书为空，`SessionSearchTrees.recipeSearch` 是空树 | 配方书为空 | 不用补 |
| `entity_event` 24..28 | `LocalPlayer.permissions = NO_PERMISSIONS`，等同事件 24 | 无 bug。F3+F4 切换模式和操作员物品栏不可用，这是非 OP 的正常状态 | 不用补 |
| `commands` | 空 `CommandDispatcher` | 无，见 7.3 | 不用补 |
| `server_data` | `ServerData` 保留服务器列表 ping 到的 motd 和图标（`handleServerData` 只负责覆盖） | 无 | 不用补 |
| `initialize_border` | `ClientLevel.worldBorder = new WorldBorder()`：中心 0,0，大小 59999968，`warningBlocks` 5，`warningTime` 15 | 无 | 不用补 |
| `set_default_spawn_position` | `ClientLevel` 构造时设为 `(8, 64, 8)` | 客户端代码里只找到指南针读取（`CompassAngleState`）。Bamboo 的出生点是 `(8.5, -60, 8.5)`，水平位置基本相同 | 不用补 |
| `ticking_state` / `ticking_step` | `TickRateManager`：20 tps，未冻结，0 步 | 无 | 不用补 |
| `post_effects` | 空 | 无 | 不用补 |
| `container_set_content` | 空物品栏，`stateId` 0 | 服务端物品栏也是空的时候两边一致。要给初始物品才需要 | 不用补 |
| `set_health` | 生命 20，饱食度 20，饱和度 5（`FoodData`） | 创造模式不显示（`Hud` 里 `canHurtPlayer` 为假） | 不用补 |
| `set_experience` | 全 0 | 创造模式不显示（`Hud` 里 `hasExperience` 为假） | 不用补 |
| `update_advancements` | `ClientAdvancements` 空树 | 进度界面为空 | 不用补 |
| `waypoint` | `ClientWaypointManager` 为空 | `hasWaypoints()` 为假，定位栏不出现 | 不用补 |
| 周期 `set_time` | 客户端自己累加 `gameTime` | Bamboo 的时钟 `rate=0`，时间静止，不受影响 | 世界循环后按需 |
| **`set_entity_data` 给自己** | `Avatar` 定义 `DATA_PLAYER_MODE_CUSTOMISATION=0`，`DATA_PLAYER_MAIN_HAND=RIGHT`。客户端没有本地赋值的代码 | **有 bug。** `isModelPartShown` 全部为 false，自己的帽子、外套、袖子、裤腿和披风不显示。左手玩家的 `getMainArm()` 仍是右手 | **要补** |
| **`update_attributes` 给自己** | 基础值 `block_interaction_range` 4.5，`entity_interaction_range` 3.0。创造模式的加成只在 `ServerPlayer.updatePlayerAttributes` 里 | **有差异。** 客户端的方块触及距离是 4.5 而不是 5.0，实体是 3.0 而不是 5.0 | **要补** |
| **`award_stats`** | `StatsScreen.isLoading = true` | **有 bug。** 打开统计界面后一直显示 `Retrieving statistics...`，Done 按钮能退出 | **要补** |
| **`player_info_update` 其他玩家** | `playerInfoMap` 里没有 | **有 bug。** `createEntityFromPacket` 找不到 `PlayerInfo`，记日志后丢弃这个玩家实体 | **要补** |
| **`player_info_remove`** | | **有 bug。** Tab 列表残留已离开的玩家 | **要补** |

### 8.3 不用补但可能用到的包

下表的包都是 clientbound，Play 状态。这里写明了最小内容，ID 大于 127 的按两字节 VarInt 写。

| 包 | ID | 字段（线上顺序） | 最小 body **[官方编码]** | 来源 |
|---|---|---|---|---|
| `change_difficulty` | 10 | `difficulty` VarInt（0 peaceful，1 easy，2 normal，3 hard）；`locked` Bool | `02 00` | `ClientboundChangeDifficultyPacket` |
| `set_held_slot` | 107 | `slot` VarInt，0..8，其他值被忽略 | `00` | `ClientboundSetHeldSlotPacket` |
| `update_recipes` | 136 | `itemSets` VarInt 个数；`stonecutterRecipes` VarInt 个数 | `00 00` | `ClientboundUpdateRecipesPacket` |
| `recipe_book_settings` | 77 | crafting、furnace、blast_furnace、smoker 依次各 `open` Bool、`filtering` Bool | 8 个 `00` | `RecipeBookSettings.STREAM_CODEC` |
| `recipe_book_add` | 75 | `entries` VarInt 个数；`replace` Bool | `00 01` | `ClientboundRecipeBookAddPacket` |
| `entity_event` | 34 | `entityId` Int；`eventId` Byte | `00 00 00 01 18`（手写；同格式的 `00 00 00 01 1c` 已 **[官方解码]**） | `ClientboundEntityEventPacket` |
| `server_data` | 88 | `motd` NBT；`icon` Opt<VarInt 长度 + PNG> | `08 00 0f "A Bamboo Server" 00` | `ClientboundServerDataPacket` |
| `initialize_border` | 44 | `centerX`、`centerZ`、`oldSize`、`newSize` Double；`lerpTime` VarLong；`absoluteMaxSize`、`warningBlocks`、`warningTime` VarInt | 见下 | `ClientboundInitializeBorderPacket` |
| `set_default_spawn_position` | 99 | `dimension` Identifier；`pos` Long（打包）；`yaw`、`pitch` Float | 见下 | `LevelData.RespawnData.STREAM_CODEC` |
| `ticking_state` | 130 | `tickRate` Float；`isFrozen` Bool | `41 a0 00 00 00` | `ClientboundTickingStatePacket` |
| `ticking_step` | 131 | `tickSteps` VarInt | `00` | `ClientboundTickingStepPacket` |
| `post_effects` | 83 | `List<Identifier>` | `00` | `ClientboundPostEffectsPacket` |
| `container_set_content` | 18 | `containerId` VarInt；`stateId` VarInt；`items` VarInt 个数 + 物品；`carried` 物品。空物品是 `VarInt 0` | `00 01 2e` + 46 个 `00` + `00` | `ClientboundContainerSetContentPacket` |
| `set_health` | 106 | `health` Float；`food` VarInt；`saturation` Float | `41 a0 00 00 14 40 a0 00 00` | `ClientboundSetHealthPacket` |
| `set_experience` | 105 | `progress` Float；`level` VarInt；`total` VarInt | `00 00 00 00 00 00` | `ClientboundSetExperiencePacket` |

`entity_event` 的权限事件：24 `ALL`，25 `MODERATORS`，26 `GAMEMASTERS`，27 `ADMINS`，28 `OWNERS`（`PlayerList.sendPlayerPermissionLevel`，`LocalPlayer.handleEntityEvent`）。

`initialize_border` 的字节。下面是 `new WorldBorder()` 的字段默认值，也就是客户端的初值，发这一份等于什么都没改 **[官方编码]**：

```
00 00 00 00 00 00 00 00      centerX 0.0
00 00 00 00 00 00 00 00      centerZ 0.0
41 8c 9c 37 00 00 00 00      oldSize 5.9999968E7
41 8c 9c 37 00 00 00 00      newSize
00                           lerpTime
f0 86 a7 0e                  absoluteMaxSize 29999984
05                           warningBlocks
0f                           warningTime 15
```

官方服务端发的不是这一份。`ServerLevel.getWorldBorder` 调 `applyInitialSettings`，用 `WorldBorder.Settings.DEFAULT` 把 `warningTime` 设成 300，最后两个字节是 `ac 02`（官方代码实跑）。边界不移动时 `getLerpSpeed()` 是 0，`Hud.extractVignette` 里 `warningTime` 不起作用，两种写法看不出区别。

`set_default_spawn_position` 在 `(8, -60, 8)` 的 body：

```
13 6d 69 6e 65 63 72 61 66 74 3a 6f 76 65 72 77 6f 72 6c 64    "minecraft:overworld"
00 00 02 00 00 00 8f c4                                        pos
00 00 00 00                                                    yaw
00 00 00 00                                                    pitch
```

`BlockPos` 打包（`BlockPos.asLong`）：`(x & 0x3FFFFFF) << 38 | (z & 0x3FFFFFF) << 12 | (y & 0xFFF)`。

```
x = 8    8 << 38        = 0x0000_0200_0000_0000
z = 8    8 << 12        = 0x0000_0000_0000_8000
y = -60  -60 & 0xFFF    = 0x0000_0000_0000_0FC4
合计                      0x0000_0200_0000_8FC4
```

### 8.4 要补的包

#### `minecraft:player_info_update`

ID 71（`0x47`），clientbound，Play。来源：`ClientboundPlayerInfoUpdatePacket`，`ClientPacketListener.handlePlayerInfoUpdate`。

| 字段 | 编码 |
|---|---|
| `actions` | 1 字节定长位集，位号是 `Action` 的序号（`writeEnumSet`） |
| `entries` | `VarInt` 个数，每项 `UUID`，后面按位号从小到大写各动作的字段 |

| 位 | 掩码 | 动作 | 字段 |
|---|---|---|---|
| 0 | `0x01` | `ADD_PLAYER` | `name: String(16)`；`properties: VarInt` 个数（≤ 16），每项 `String(64)`、`String(32767)`、`Opt<String(1024)>` |
| 1 | `0x02` | `INITIALIZE_CHAT` | `Opt<sessionId UUID, expiresAt Long, publicKey, keySignature>` |
| 2 | `0x04` | `UPDATE_GAME_MODE` | `VarInt` |
| 3 | `0x08` | `UPDATE_LISTED` | `Bool` |
| 4 | `0x10` | `UPDATE_LATENCY` | `VarInt` 毫秒 |
| 5 | `0x20` | `UPDATE_DISPLAY_NAME` | `Opt<NBT>` |
| 6 | `0x40` | `UPDATE_LIST_ORDER` | `VarInt` |
| 7 | `0x80` | `UPDATE_HAT` | `Bool` |

官方进服时置全部 8 位，也就是 `0xFF`（`createPlayerInitializing`），各字段的值见 `entities.md` 第 3 节。Bamboo 现在的 `0x0D` 也能工作，没写到的字段用 `PlayerInfo` 的默认值：延迟 0，`showHat=true`，`tabListOrder=0`。可见的差别有两处，都在 `PlayerTabOverlay`：延迟 0 总是画满格信号；进服前就关掉帽子的玩家，头像仍画帽子层。M2 按 `entities.md` 改成 `0xFF`。

`0x0D` 这一条记录的 body **[官方解码]**：

```
0d                                                 ADD_PLAYER | UPDATE_GAME_MODE | UPDATE_LISTED
01                                                 条目数
db 80 81 6f ce 8f 33 40 b6 e5 6d ef cb 8c 28 ef    UUID
06 4b 69 74 74 65 6e                               name
00                                                 properties 个数
01                                                 game mode：Creative
01                                                 listed
```

服务端要做的事：

- 新玩家进入时，先把所有已在线玩家发给他，再把他发给所有人，包括他自己（`placeNewPlayer` 的顺序）。
- 必须早于这名玩家的 `add_entity`。
- 要把 `AddPlayerInfo` 改成支持多条记录。
- 官方每 601 tick 广播一次 `UPDATE_LATENCY`（`PlayerList.tick`，条件是 `++sendAllPlayerInfoIn > 600`）。M2 建议做，和 `entities.md` 第 3 节一致：心跳已经有往返时间，不发时 Tab 列表的延迟一直是 0，总是画满格信号。

#### `minecraft:player_info_remove`

ID 70（`0x46`），clientbound，Play。来源：`ClientboundPlayerInfoRemovePacket`，`PlayerList.remove`。

| 字段 | 编码 |
|---|---|
| `profileIds` | `VarInt` 个数 + `UUID` |

```
01  db 80 81 6f ce 8f 33 40 b6 e5 6d ef cb 8c 28 ef
```

**[官方编码]** 服务端要做的事：玩家离开时广播给其他人，顺序见第 6 节。

#### `minecraft:set_entity_data`

ID 101（`0x65`），clientbound，Play。来源：`ClientboundSetEntityDataPacket`，`SynchedEntityData.DataValue`，`EntityDataSerializers`，`Avatar`。

| 字段 | 编码 |
|---|---|
| `entityId` | `VarInt` |
| 项目，重复 | `index: Byte`，`serializer: VarInt`，值 |
| 结束标记 | `Byte 0xFF` |

这里用到的两项（官方代码实跑）：

| 下标 | 字段 | serializer | 值 |
|---|---|---|---|
| 15 | `DATA_PLAYER_MAIN_HAND` | 42 `HUMANOID_ARM` | `VarInt`，0 左，1 右 |
| 16 | `DATA_PLAYER_MODE_CUSTOMISATION` | 0 `BYTE` | `Byte`，`client_information` 的掩码 |

实体 1、左手、全部外层的 body **[官方解码]**：

```
01          entityId
0f 2a 00    下标 15，serializer 42，LEFT
10 00 7f    下标 16，serializer 0，0x7F
ff          结束
```

服务端要做的事：进入 Play 后发一次给自己，值来自 Configuration 里的 `client_information`。Play 里设置变化时再发。看得到这名玩家的其他人也需要这两项，这部分属于实体同步。

#### `minecraft:update_attributes`

ID 134（`0x86 0x01`），clientbound，Play。来源：`ClientboundUpdateAttributesPacket`，`ServerPlayer.updatePlayerAttributes`，`ClientPacketListener.handleUpdateAttributes`。

| 字段 | 编码 |
|---|---|
| `entityId` | `VarInt` |
| `attributes` | `VarInt` 个数（≤ 128） |
| 每项 `attribute` | `VarInt`，`minecraft:attribute` 的 ID，**不加 1** |
| 每项 `base` | `Double` |
| 每项 `modifiers` | `VarInt` 个数，每项 `id: Identifier`、`amount: Double`、`operation: VarInt` |

`operation`：0 是 `ADD_VALUE`。

创造模式玩家（实体 1）的 body，共 113 字节 **[官方编码]**：

```
01                                     entityId
02                                     attributes 个数
08                                     minecraft:block_interaction_range
40 12 00 00 00 00 00 00                base 4.5
01                                     modifiers 个数
23 "minecraft:creative_mode_block_range"
3f e0 00 00 00 00 00 00                amount 0.5
00                                     ADD_VALUE
0d                                     minecraft:entity_interaction_range
40 08 00 00 00 00 00 00                base 3.0
01
24 "minecraft:creative_mode_entity_range"
40 00 00 00 00 00 00 00                amount 2.0
00
```

服务端要做的事：进入 Play 后发一次给自己。服务端校验触及距离时要用加成后的值：方块 5.0，实体 5.0。校验本身属于方块交互的规格。

#### `minecraft:award_stats`

ID 3（`0x03`），clientbound，Play。来源：`ClientboundAwardStatsPacket`，`ClientPacketListener.handleAwardStats`，`StatsScreen.onStatsUpdated`。

| 字段 | 编码 |
|---|---|
| `stats` | `VarInt` 个数，每项 `Stat` + `VarInt` 值 |

空统计的 body 是 `00` **[官方编码]**。服务端要做的事：收到 `client_command` 的 `action=1` 时回这个包。

### 8.5 M2 建议的进服顺序

只列要发的包。第 4、5、7 步要访问在线玩家表，应该放在世界线程里做。这里是所有规格的基准顺序，`chunks-movement.md` 8.3 和 `entities.md` 第 14 节只列各自关心的包。这 10 步全部由世界线程写入同一条出站队列，原因和离开顺序、合并后的 tick 顺序见 `coverage.md` 第 4 节。`set_held_slot` 和 `container_set_content` 不发（8.2）。

1. `login`
2. `player_abilities`
3. `player_position`
4. `player_info_update`：所有已在线玩家 → 新玩家
5. 把新玩家加入在线列表，`player_info_update`：新玩家 → 所有人，包括他自己
6. `set_entity_data`、`update_attributes`：给自己
7. `system_chat` 加入消息：只发给其他已在线玩家。官方在第 3 步之前发。这条消息发给的是别人，和新玩家的包之间没有依赖，放在第 5 步之后实现起来更简单
8. `set_time`
9. `game_event` 13
10. `set_chunk_cache_center`，然后按 `chunks-movement.md` 发区块

---

## 9. “回头只看到草方块顶面”

**结论：代码指向最外圈缺邻居，和光照无关。** 这是读代码和实跑 `ChunkTrackingView` 得出的，没有截图，也没有实机验证。

### 9.1 证据链

1. **Bamboo 发的范围。** `play.rs` 的 `chunks_around(SPAWN_CHUNK, view_distance)` 是边长 `2r+1` 的正方形。默认 `r=8`，也就是 `x, z ∈ [-8, 8]`，289 个区块，之后不再发。

2. **官方发的范围比渲染范围多一圈。** 两个集合都在 `ChunkTrackingView.isWithinDistance` 里：

   ```
   bufferRange = includeNeighbors ? 2 : 1
   dx = max(0, |x - cx| - bufferRange)
   dz = max(0, |z - cz| - bufferRange)
   return dx*dx + dz*dz < r*r
   ```

   服务端发送时用 `contains(x, z)`，也就是 `includeNeighbors=true`。客户端渲染时用 `isInViewDistance`，也就是 `false`（`SectionOcclusionGraph.isInViewDistance`）。实跑官方类的结果：

   | r | 官方发送 | 单轴最远 | 渲染集合 | 官方里缺邻居的 | Bamboo 正方形里缺邻居的 |
   |---|---|---|---|---|---|
   | 8 | 329 | 9 | 257 | 0 | 36 |
   | 10 | 473 | 11 | 385 | 0 | 44 |

3. **客户端不编译缺邻居的 section。** `LevelExtractor.extract` 遍历可见 section，只有脏的才考虑编译，而且必须满足下面两个条件之一：

   - section 已经有 mesh，也就是不是 `CompiledSectionMesh.UNCOMPILED`。
   - `SectionUpdateTracker.hasAllNeighbors` 为真，也就是周围 8 个区块都存在，且 `lightOnInColumn` 为真。

   这是唯一产生编译任务的地方，`LevelRenderer.compileSections` 只消费它的结果。

   所以 `|x|=8` 或 `|z|=8` 这一圈的地面 section 永远是 `UNCOMPILED`，不绘制。它们上方的空 section 由 `SectionOcclusionGraph.runUpdates` 直接标成 `EMPTY`，不影响遍历。

4. **第 7 圈朝外的侧面被第 8 圈剔除。** 第 7 圈有全部邻居，可以编译。`RenderRegionCache` 复制邻居数据时，第 8 圈是已加载的区块，方块是实心的。`ModelBlockRenderer.shouldRenderFace` 调用 `Block.shouldRenderFace`。邻居是完整方块，遮挡形状盖满了这个面，所以返回 false，不生成这个面。

5. **合起来看到的画面。** 玩家走过第 7 圈的边缘，踏上第 8 圈。第 8 圈有方块数据，能碰撞，但不绘制。回头看时，世界的边缘只有第 7 圈的草方块顶面，下面没有泥土和基岩的侧面，世界像一张没有厚度的纸。这和用户的描述一致。

6. **再往外。** 踏出第 8 圈进入未加载区块后，`runUpdates` 把摄像机所在 section 放进 `sectionsWaitingForChunkLoads` 就结束了，遍历不会开始，所有地形消失。`BlockCollisions` 对未加载区块没有碰撞，`ClientLevel.hasChunk` 又总是返回 true，所以重力照常，玩家开始下落。摄像机低于世界底部时 `initializeQueueForFullUpdate` 走 `cameraSection == null` 分支，从最底层 section 开始，地形重新出现。

这个缺口从进服那一刻就存在，不是走动造成的。走动只是让玩家能看到边缘。

### 9.2 排除的方向

| 方向 | 结论 |
|---|---|
| 光照掩码和数据 | 没问题。`LightData` 的 26 层和 `readSectionList` 的 `getLightSectionCount()` 对应，第 0 层是 `minSectionY - 1`。`Dark` 进 `emptySkyYMask`，`Full` 和 `Partial` 进 `skyYMask`，顺序一致。`partial_layer` 的下标和半字节顺序和 `DataLayer` 相同。方块光掩码全空时 `setLightEnabled` 仍会调用 |
| 光照会不会让面消失 | 不会。光照只影响亮度，面是否生成只看 `shouldRenderFace`。全黑的面是黑色，不是消失 |
| `lightOnInColumn` | `applyLightData` 末尾 `setLightEnabled(pos, true)`，Bamboo 的所有区块都走这里。出生点能正常渲染也说明它是真 |
| 省略的进服包 | 第 8 节的包没有一个进入渲染路径 |
| `set_chunk_cache_center` | 只改 `Storage.viewCenterX/Z`。范围是 `max(2, r) + 3 = 11`，中心不动时 ±8 的区块都在范围内。Bamboo 也不发 `forget_level_chunk`，没有区块被丢弃 |
| 邻居未加载时的侧面 | `SectionCopy` 对 `EmptyLevelChunk` 返回空气，侧面会生成。但这只对第 8 圈朝外的面成立，而第 8 圈根本不编译 |
| 方块和高度图 | `LevelChunkSection.read`、`Heightmap.Types`（1、4、5）和 Bamboo 的写法一致，`Probe` 已用官方代码解码过 |

### 9.3 修复

按 `ChunkTrackingView.contains(x, z)` 发送区块，也就是 `includeNeighbors=true` 的范围。`login` 里的 `chunkRadius` 仍然写 `r`，不加 1。

`chunks-movement.md` 的 `T(c, vd)` 就是这个集合。M2 的区块跟随做好后，缺邻居的外圈始终在渲染距离之外。

只把正方形半径改成 `r+1` 也能消除这个现象，但和官方的集合不同，每次多发一些角落的区块。

还有一个相关影响：`LevelExtractor.isEntityVisible` 要求实体所在 section 不是 `UNCOMPILED`（`LevelRenderer.isSectionCompiledAndVisible`）。其他玩家站在未编译的 section 里时不显示，修复后不再发生。

---

## 10. Bamboo 改动清单

`mc-bamboo-protocol`：

- `PlayRequest` 增加 `Chat`、`ChatCommand`、`ClientInformation`、`ClientCommand`。解码结果要跨线程交给世界线程，文本字段用 `String`，不要借用帧数据。完整的解码清单见 `coverage.md` 第 7 节。
- `ConfigurationRequest` 增加 `ClientInformation`。
- `chat` 解码时检查 `String(256)` 和 `reader.finish()`。现有的 `Reader::string` 已按 UTF-16 码元计数，和 `Utf8String.read` 一致。
- 把 `packet/mod.rs` 的 `write_text_component` 扩展成可以写 `Tag` 的文本组件。纯文本仍是 `TAG_String`。不要生成空的 `extra`。`with` 保持元素类型相同。
- 新增 `SystemChat`、`DisguisedChat`、`PlayerInfoRemove`、`SetEntityData`、`UpdateAttributes`、`AwardStats`，把 `AddPlayerInfo` 改成多条记录，动作位改成 `0xFF`（第 8.4 节）。
- `Disconnect` 要能带翻译组件。现在的 `reason: &str` 只能写纯文本。

`mc-bamboo-data`：

- `mc-bamboo-vanilla` 的生成器增加 `chat_type` 模块，给 `minecraft:chat` 等条目生成 ID，写包时加 1。不要在代码里手写 `1`。
- 增加 `attribute` 的 `block_interaction_range`、`entity_interaction_range`。

`mc-bamboo-server`：

- 把 `client_information` 的内容从 Configuration 带进 Play。
- 加入消息放在进入 Play 时，不放在 `PlayerSession::join`。离开消息只对进入过 Play 的玩家发。
- 新增一份“已进入 Play 的玩家”名单给广播用。`Shared` 的玩家表从 Login 开始就有人，不能直接用（第 6 节）。
- 聊天广播：编码一次，同一份帧发给这份名单上的所有玩家。
- `ServerEvent` 增加聊天事件，桌面控制台可以显示。
- 区块集合按第 9.3 节改。`config.rs` 里 `view_distance` 的注释也要改。

测试：

- `crates/mc-bamboo-server/tests/join.rs` 的 `CHUNKS` 和 `tools/mc-bamboo-vanilla/java/Probe.java` 的 `EXPECTED_CHUNKS` 都假定了正方形，要一起改。`join.rs` 的视距是 1，要先按 `chunks-movement.md` 第 10 节把配置 clamp 到 2..=32，实际视距是 2，集合是 `T(2)`，共 49 个，不是 9 个。`read_chunks` 也要回 `chunk_batch_received`。
- `Probe` 可以增加：解码 `disguised_chat` 后断言 `decorate` 的文本是 `<Kitten> hi`。

---

## 11. UNVERIFIED

1. **没有运行真实客户端。** 所有客户端行为来自读代码。官方 codec 实跑只验证了字节格式和 `ChunkTrackingView` 的集合，不验证界面。
2. **第 9 节的诊断没有实机确认。** 用户的截图还没有拿到。代码路径是确定的，但无法确认用户看到的就是这个画面。修好之后要让用户走到边缘再看一次。
3. **加入者收不到自己的加入消息。** 这是从 `placeNewPlayer` 的语句顺序推出的，没有实机观察。
4. **5.4 表里的灰条。** 读到了 `GuiMessageTag` 的颜色和 `handleTag` 的绘制调用，没有确认鼠标悬停时提示文字的显示条件。
5. **雾是否遮住出生点能看到的边缘。** 第 7 圈的边缘离出生点约 120 格，没有查雾的计算，所以不确定玩家在出生点能不能直接看到边缘。
6. **`update_attributes` 的实际手感。** 触及距离的差异来自属性基础值和 `ServerPlayer` 里的修饰符，没有实机比较。
7. **`player_chat` 的 `body.lastSeen` 缓存 ID。** 只抄录了格式。M2 不用这个包，没有验证 `MessageSignatureCache` 的行为。
8. **第 8.2 节的“不会崩溃”。** 这是沿着每个包的处理函数和缺省值读出来的，没有逐个打开依赖这些包的界面（配方书、进度、统计）。M1 实机只说明进服和走动不崩溃。
9. **用户启动器的实际路径。** `CHAT_ALLOWED` 的结论来自 `Minecraft` 构造函数和 authlib 字节码。用户的启动器走 `offlineDeveloperMode`、`fetchProperties` 失败回退，还是正版账号权限，没有确认。
