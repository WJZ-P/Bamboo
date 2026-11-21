/**
 * 设置页上每一项的声明。
 *
 * 加一个服务端选项时，这里加一条，Rust 侧 `settings.rs` 的 `ServerSettings` 加一个字段，
 * 页面本身不用改。数值范围要和 `settings.rs` 里 `validate` 的边界对上：
 * 这边只负责提示，真正拦住非法值的是 Rust 那一侧。
 *
 * 官方设置写进 `server.properties`，Bamboo 优化设置写进 `bamboo.toml`；
 * 两个文件在「服务器目录」里，命令行版用同一套文件。所有选项都是服务端启动时
 * 读一次，现在没有改完立刻生效的选项。
 */

import type { ServerSettings } from "../api/bamboo";

/** 两个分区。每个分区下面再按用途分小组，页面把分区标题单独渲染。 */
export const SECTIONS = [
  {
    id: "official",
    title: "官方设置",
    hint: "写进 server.properties。键名、写法和默认值都照官方 26.3，从官方服务端迁过来的人可以直接沿用原来的文件。Bamboo 还没实现的官方键会原样保留，但现在不起作用。",
  },
  {
    id: "bamboo",
    title: "Bamboo 优化设置",
    hint: "写进 bamboo.toml。这些是 Bamboo 自己的项，官方服务端没有。精确模式不设实体线程数，和官方逐位一致；性能模式才用并行驱动。",
  },
] as const;

export type SectionId = (typeof SECTIONS)[number]["id"];

export const GROUPS = [
  { id: "basic", section: "official", title: "基本", hint: "玩家怎么找到这台服务器。" },
  {
    id: "world",
    section: "official",
    title: "世界生成",
    hint: "只对新建的世界生效：种子、世界类型和超平坦预设在建世界时写进存档，之后改这里不会改变已有的存档。",
  },
  { id: "network", section: "official", title: "网络", hint: "连接本身怎么传数据。" },
  {
    id: "auth",
    section: "official",
    title: "正版验证",
    hint: "要不要让 Mojang 确认每名玩家的账号。和代理转发互斥：代理已经替服务端验过了，两个只能开一个。",
  },
  {
    id: "forwarding",
    section: "bamboo",
    title: "代理转发",
    hint: "服务端藏在 BungeeCord 或 Velocity 后面时才需要改。开启后一定要用防火墙挡住外部直连，否则任何人都能冒充任意玩家进服。",
  },
  {
    id: "advanced",
    section: "bamboo",
    title: "连接护栏",
    hint: "挡住赖着不走的连接，一般不用改。",
  },
  {
    id: "performance",
    section: "bamboo",
    title: "性能",
    hint: "实体阶段和区块信箱的执行方式，以及世界生成两份缓存的预算和空闲释放。精确模式是默认值，所有对拍和测试都在这个模式下跑。",
  },
] as const;

export type GroupId = (typeof GROUPS)[number]["id"];

/** 值的类型是 `T` 的那些设置项。 */
type KeysOf<T> = {
  [K in keyof ServerSettings]: ServerSettings[K] extends T ? K : never;
}[keyof ServerSettings];

interface Common<K extends keyof ServerSettings> {
  key: K;
  label: string;
  group: GroupId;
  /** 这一项是什么，显示在控件下面。 */
  hint: string;
  /** 对应的配置文件里的键名。 */
  fileKey: string;
  /** 额外的校验，返回错误文字或者 null。整份草稿都给进来，可以跨项检查。 */
  validate?: (draft: ServerSettings) => string | null;
  /** 当前用不上这一项，灰掉但仍然显示。 */
  disabled?: (draft: ServerSettings) => boolean;
  /** 占满一整行。滑块、多选和长文本都需要。 */
  wide?: boolean;
}

// 每种控件各自一个类型，`control` 都是单个字面量：这样 switch 和 if 都能把类型收窄干净。
type Text<K extends keyof ServerSettings> = Common<K> & {
  maxLength?: number;
  /** 用多行文本框，JSON 这类长文本需要。 */
  multiline?: boolean;
};
type Numeric<K extends keyof ServerSettings> = Common<K> & {
  min: number;
  max: number;
  unit?: string;
};

export type TextField = Text<KeysOf<string>> & { control: "text" };
export type PasswordField = Text<KeysOf<string>> & { control: "password" };
export type NumberField = Numeric<KeysOf<number>> & { control: "number" };
export type SliderField = Numeric<KeysOf<number>> & { control: "slider" };
export type ChoiceField = Common<KeysOf<ServerSettings["forwarding"]>> & {
  control: "choice";
  options: readonly { value: ServerSettings["forwarding"]; label: string }[];
};

/**
 * 值是「几个固定字符串之一」的设置项。纯 `string` 的项（种子、JSON 文本）
 * 不在里面：它们可以填任意文字，做不成下拉。
 */
type ChoiceKey = {
  [K in keyof ServerSettings]: ServerSettings[K] extends string
    ? string extends ServerSettings[K]
      ? never
      : K
    : never;
}[keyof ServerSettings];

/** 下拉框，选项的取值类型跟着字段走。 */
export type SelectField = {
  [K in ChoiceKey]: Common<K> & {
    control: "select";
    options: readonly { value: ServerSettings[K]; label: string }[];
  };
}[ChoiceKey];
export type ToggleField = Common<KeysOf<boolean>> & { control: "toggle" };

export type SettingField =
  | TextField
  | PasswordField
  | NumberField
  | SliderField
  | ChoiceField
  | SelectField
  | ToggleField;

/** 心跳必须留出余量：客户端 30 秒收不到任何字节就自己断开。 */
const MAX_KEEP_ALIVE_SECONDS = 29;
/** 帧长度上限是 2 MiB 差一字节，阈值到了这里就等于永远不压缩。 */
const MAX_COMPRESSION_THRESHOLD = 2_097_151;
/** 服务端能接受的实体线程数上限。 */
const MAX_ENTITY_THREADS = 256;
/** 服务端能接受的区块计算线程数上限，和 `MAX_CHUNK_WORKER_THREADS` 一致。 */
const MAX_CHUNK_WORKER_THREADS = 64;
const MAX_MAIL_BUDGET = 65_536;
/** 世界生成缓存三个键的上限，和服务端 `worldgen_cache` 里的 `MAX_*` 一致。 */
const MAX_TERRAIN_CACHE_MB = 4096;
const MAX_FEATURE_CACHE_MB = 1024;
const MAX_CACHE_IDLE_SECONDS = 3600;
/** 服务端能接受的岛 tick 线程数上限，和 `MAX_ISLAND_THREADS` 一致。 */
const MAX_ISLAND_THREADS = 64;

export const FIELDS: readonly SettingField[] = [
  {
    key: "host",
    group: "basic",
    fileKey: "server-ip",
    label: "监听地址",
    control: "text",
    hint: "0.0.0.0 对应官方默认的空 server-ip（监听所有网卡），127.0.0.1 只有本机能连。",
    validate: (draft) => (draft.host.trim().length > 0 ? null : "监听地址不能为空"),
  },
  {
    key: "port",
    group: "basic",
    fileKey: "server-port",
    label: "端口",
    control: "number",
    min: 1,
    max: 65535,
    hint: "客户端添加服务器时填的端口。",
  },
  {
    key: "levelName",
    group: "basic",
    fileKey: "level-name",
    label: "世界名字",
    control: "text",
    maxLength: 80,
    hint: "服务器目录里的世界目录名。改了之后服务端会去读写另一个世界目录，原来的存档不会自动搬过去。",
    validate: (draft) => {
      const name = draft.levelName.trim();
      if (name.length === 0) return "世界名字不能为空";
      if (name.includes("/") || name.includes("\\")) return "世界名字不能带路径分隔符";
      return null;
    },
  },
  {
    key: "motd",
    group: "basic",
    fileKey: "motd",
    label: "服务器介绍",
    control: "text",
    maxLength: 120,
    wide: true,
    hint: "服务器列表里显示在名字下面的那行字，对应官方的 motd。",
  },
  {
    key: "maxPlayers",
    group: "basic",
    fileKey: "max-players",
    label: "最大玩家数",
    control: "slider",
    min: 1,
    max: 100,
    wide: true,
    hint: "同时在线的玩家上限，服务器列表里也会显示，对应官方的 max-players。",
  },
  {
    key: "viewDistance",
    group: "basic",
    fileKey: "view-distance",
    label: "视距",
    control: "slider",
    min: 2,
    max: 32,
    unit: "区块",
    wide: true,
    hint: "客户端的渲染半径，对应官方的 view-distance。越大，玩家进服时要传输的区块越多。",
  },
  {
    key: "levelSeed",
    group: "world",
    fileKey: "level-seed",
    label: "世界种子",
    control: "text",
    wide: true,
    hint: "留空表示新建世界时随机取一个。数字按 Long.parseLong 解释，其它字符串按 Java 的 String.hashCode 算成种子，和官方一致。只对新建的世界生效，已有存档的种子以存档里的为准。",
  },
  {
    key: "levelType",
    group: "world",
    fileKey: "level-type",
    label: "世界类型",
    control: "select",
    options: [
      { value: "normal", label: "默认（噪声地形）" },
      { value: "flat", label: "超平坦" },
      { value: "largeBiomes", label: "巨型生物群系" },
      { value: "amplified", label: "放大化" },
    ],
    hint: "新世界用的世界预设，对应官方的 level-type。默认是官方同款噪声地形；选超平坦才能用下面的超平坦设置。只对新建的世界生效，已有存档按存档里的世界类型继续生成。",
  },
  {
    key: "generateStructures",
    group: "world",
    fileKey: "generate-structures",
    label: "生成结构",
    control: "toggle",
    wide: true,
    hint: "对应官方的 generate-structures，默认打开。关掉后新世界不生成村庄、要塞这些结构，只有地形和地物。只对新建的世界生效。",
  },
  {
    key: "generatorSettings",
    group: "world",
    fileKey: "generator-settings",
    label: "超平坦设置",
    control: "text",
    multiline: true,
    wide: true,
    disabled: (draft) => draft.levelType !== "flat",
    hint: "世界类型是超平坦时才生效，对应官方的 generator-settings。layers 是从世界底部往上叠的方块层，biome 是群系；官方默认是基岩 1 层、泥土 2 层、草方块 1 层、平原群系，留空或 {} 就是这套默认值。",
    validate: (draft) => {
      if (draft.levelType !== "flat") return null;
      const text = draft.generatorSettings.trim();
      if (text.length === 0) return null;
      try {
        const parsed: unknown = JSON.parse(text);
        if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
          return "超平坦设置必须是一个 JSON 对象";
        }
      } catch {
        return "超平坦设置不是合法的 JSON";
      }
      return null;
    },
  },
  {
    key: "compressionThreshold",
    group: "network",
    fileKey: "network-compression-threshold",
    label: "压缩阈值",
    control: "number",
    min: -1,
    max: MAX_COMPRESSION_THRESHOLD,
    unit: "字节",
    hint: "负载超过这么多字节的包才压缩，对应官方的 network-compression-threshold。-1 关闭压缩，0 压缩每一个包。",
  },
  {
    key: "onlineMode",
    group: "auth",
    fileKey: "online-mode",
    label: "正版验证",
    control: "toggle",
    wide: true,
    hint: "对应官方的 online-mode，默认打开。打开后登录过程会加密，每名玩家都要经过 Mojang 的 session server 确认，皮肤也由 Mojang 提供；服务端因此需要能访问外网。关掉才能用离线账号直连：既不加密也不验证，UUID 由玩家名算出来。打开了代理转发就不能再打开这一项。",
    validate: (draft) =>
      draft.onlineMode && draft.forwarding !== "off"
        ? "正版验证和代理转发不能同时打开：代理已经替服务端验过身份了"
        : null,
  },
  {
    key: "preventProxyConnections",
    group: "auth",
    fileKey: "prevent-proxy-connections",
    label: "禁止代理连接",
    control: "toggle",
    wide: true,
    disabled: (draft) => !draft.onlineMode,
    hint: "对应官方的 prevent-proxy-connections。打开后向 Mojang 提问时带上客户端的 IP，Mojang 会比对玩家登录时报备的那个，对不上就不认这次登录。玩家经过 VPN 或者代理进来时两边的 IP 不一样，会被挡住。",
  },
  {
    key: "enforceSecureProfile",
    group: "auth",
    fileKey: "enforce-secure-profile",
    label: "强制签名聊天",
    control: "toggle",
    wide: true,
    hint: "对应官方的 enforce-secure-profile，默认打开。正版验证也打开、并且取到了 Mojang 的公钥之后，没有签名的聊天和需要签名的命令会被拒绝。没开正版验证时这一项不起作用。",
  },
  {
    key: "forwarding",
    group: "forwarding",
    fileKey: "forwarding",
    label: "转发模式",
    control: "choice",
    wide: true,
    options: [
      { value: "off", label: "不转发" },
      { value: "legacy", label: "BungeeCord 旧式" },
      { value: "modern", label: "Velocity 现代" },
    ],
    hint: "玩家的 IP 和 UUID 由谁提供。不转发时由本服务端自己生成离线 UUID；旧式转发信任握手包里带的信息；现代转发会在登录时向代理索要并验证签名。开启转发就要把正版验证关掉。",
    validate: (draft) =>
      draft.onlineMode && draft.forwarding !== "off"
        ? "打开代理转发就要把正版验证关掉，服务端不能自己再验一次"
        : null,
  },
  {
    key: "forwardingSecret",
    group: "forwarding",
    fileKey: "forwarding_secret",
    label: "转发密钥",
    control: "password",
    wide: true,
    disabled: (draft) => draft.forwarding !== "modern",
    hint: "只有现代转发用得上，内容就是 Velocity 的 forwarding.secret 文件。",
    validate: (draft) =>
      draft.forwarding === "modern" && draft.forwardingSecret.length === 0
        ? "现代转发必须填写转发密钥"
        : null,
  },
  {
    key: "prePlayTimeoutSeconds",
    group: "advanced",
    fileKey: "pre_play_timeout_seconds",
    label: "进服时限",
    control: "number",
    min: 1,
    max: 3600,
    unit: "秒",
    hint: "一个连接从握手到真正进入游戏的总时限，超时就断开。官方没有这一项，它用来挡住连上来不说话的连接。",
  },
  {
    key: "maxPrePlayConnections",
    group: "advanced",
    fileKey: "max_pre_play_connections",
    label: "进服连接数上限",
    control: "number",
    min: 1,
    max: 1_000_000,
    hint: "同时处于握手、登录、配置阶段的连接上限，超出的连接直接丢弃。已经进入游戏的玩家不占名额。",
  },
  {
    key: "keepAliveSeconds",
    group: "advanced",
    fileKey: "keep_alive_seconds",
    label: "心跳间隔",
    control: "number",
    min: 1,
    max: MAX_KEEP_ALIVE_SECONDS,
    unit: "秒",
    hint: "服务端每隔这么久发一次心跳，上一次还没有回应就断开。客户端 30 秒收不到任何数据就会自己断开，所以这里必须小于 30 秒。",
  },
  {
    key: "entityThreads",
    group: "performance",
    fileKey: "entity_threads",
    label: "实体并行线程数（实验）",
    control: "number",
    min: 0,
    max: MAX_ENTITY_THREADS,
    wide: true,
    hint: "0 = 精确模式（默认）：实体逐个算完立刻落地，和官方逐位一致，所有 oracle、probe、单元测试都在这个模式下跑。≥ 1 = 性能模式：实体阶段改成「并行算、按 uid 升序串行落地」，3000 个掉落物加 300 个下落方块的场景里 8 线程整刻快 4.0 倍。性能模式相对官方有已知差异：物品合并少看一拍（最多晚一刻合并）、TNT 爆炸的击退晚一刻体现在物理上、漏斗吸走物品后物品自己这一刻的 age 和位置仍按未被吸走算；别的实体看到的也是这一刻开始时的样子。目前每刻新起线程，只用于实验，不建议在正式服开。",
  },
  {
    key: "redstoneCache",
    group: "performance",
    fileKey: "redstone_cache",
    label: "红石缓存（实验）",
    control: "toggle",
    wide: true,
    hint: "把反复执行、每次读到相同状态的一段红石电路记下来，下次直接套用结果。结果和官方逐位一致，打开后对大型红石电路有效；小型电路和容器机器收益很小。默认关闭，打开后每个 region 的缓存有 32 MiB 上限，命中不了的触发点会自动停止录制。",
  },
  {
    key: "chunkWorkerThreads",
    group: "performance",
    fileKey: "chunk_worker_threads",
    label: "区块计算线程数",
    control: "number",
    min: 0,
    max: MAX_CHUNK_WORKER_THREADS,
    hint: "区块的解析、生成和存盘序列化放到多少条常驻后台线程上做。0 = 在世界线程上做（默认）。区块内容和线程数无关，只有区块到达的时刻可能不同。",
  },
  {
    key: "worldgenTerrainCacheMb",
    group: "performance",
    fileKey: "worldgen_terrain_cache_mb",
    label: "地形缓存上限（MiB）",
    control: "number",
    min: 0,
    max: MAX_TERRAIN_CACHE_MB,
    hint: "世界生成时暂存的半成品地形最多占多少内存，默认 128。缓存只影响内存和跑图速度，不影响生成结果；周围区块都生成完的条目会立刻释放。0 = 不缓存（跑图明显变慢）。",
  },
  {
    key: "worldgenFeatureCacheMb",
    group: "performance",
    fileKey: "worldgen_feature_cache_mb",
    label: "地物缓存上限（MiB）",
    control: "number",
    min: 0,
    max: MAX_FEATURE_CACHE_MB,
    hint: "世界生成时暂存的地物（树、矿石等）结果最多占多少内存，默认 64。只影响内存和跑图速度，不影响生成结果。0 = 不缓存。",
  },
  {
    key: "worldgenCacheIdleSeconds",
    group: "performance",
    fileKey: "worldgen_cache_idle_seconds",
    label: "缓存空闲释放（秒）",
    control: "number",
    min: 0,
    max: MAX_CACHE_IDLE_SECONDS,
    hint: "区块计算队列空闲，且多久没有查询缓存后清空上面两份缓存，默认 30 秒。不会卸载区块。0 = 关闭空闲清理。",
  },
  {
    key: "deliveriesPerTick",
    group: "performance",
    fileKey: "deliveries_per_tick",
    label: "每刻交付区块上限",
    control: "number",
    min: 0,
    max: MAX_MAIL_BUDGET,
    hint: "Director 信箱每刻最多把多少个区块交给世界线程。上限是为了削进服时的尖峰；脚下 3×3 不受限制。0 表示不限（只用于对比测量），默认 64。",
  },
  {
    key: "reclaimsPerTick",
    group: "performance",
    fileKey: "reclaims_per_tick",
    label: "每刻收回区块上限",
    control: "number",
    min: 0,
    max: MAX_MAIL_BUDGET,
    hint: "Director 信箱每刻最多从世界线程收回多少个区块，默认 64。积压变大时按积压比例放宽，避免很多人同时高速移动时内存没有上界。0 表示不限（只用于对比测量）。",
  },
  {
    key: "islandThreads",
    group: "performance",
    fileKey: "island_threads",
    label: "岛的 tick 线程数",
    control: "number",
    min: 0,
    max: MAX_ISLAND_THREADS,
    hint: "世界的模拟（方块、实体、发区块）放到多少条 tick 线程上跑，和负责区块加载、全局状态的主控线程并行；每个岛一次占一条。默认是这台电脑逻辑核数的一半，夹在 2 到 8 之间（第一次启动时算好写进 bamboo.toml）。开着多岛时至少给 2 条：只有 1 条时一个过载的岛会拖住所有岛。0 = 精确模式：全部在一条线程上跑，和官方逐位一致，对拍和测试用。>= 1 时区块交付晚一刻。",
  },
  {
    key: "islands",
    group: "performance",
    fileKey: "islands",
    label: "多岛",
    control: "toggle",
    wide: true,
    hint: "默认开启：分区器把相距很远的玩家群拆成各自的岛，各岛并行跑，一个岛过载只拖慢它自己。跨岛的先后、传送和全局命令反馈会比精确模式晚几刻，天气的随机序列和官方不同。关闭 = 整个世界一个岛；关闭并且 tick 线程数设成 0 就是精确模式，和官方逐位一致，所有对拍和探针都显式选它。",
  },
];

/** 一项的错误提示，没有问题时返回 null。 */
export function fieldError(field: SettingField, draft: ServerSettings): string | null {
  if (field.control === "number" || field.control === "slider") {
    const value = draft[field.key];
    if (!Number.isInteger(value)) {
      return "请填一个整数";
    }
    if (value < field.min || value > field.max) {
      return `范围是 ${field.min} 到 ${field.max}`;
    }
  }
  return field.validate?.(draft) ?? null;
}

/** 默认值的说明文字。默认值由 Rust 侧给出，这里只管怎么显示。 */
export function describeDefault(field: SettingField, defaults: ServerSettings): string {
  if (field.control === "number" || field.control === "slider") {
    const value = defaults[field.key];
    return field.unit ? `${value} ${field.unit}` : String(value);
  }
  if (field.control === "choice" || field.control === "select") {
    const value = defaults[field.key];
    return field.options.find((option) => option.value === value)?.label ?? value;
  }
  if (field.control === "toggle") {
    return defaults[field.key] ? "开" : "关";
  }
  return defaults[field.key].length > 0 ? defaults[field.key] : "留空";
}
