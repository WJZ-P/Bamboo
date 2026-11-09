/**
 * 设置页上每一项的声明。
 *
 * 加一个服务端选项时，这里加一条，Rust 侧 `settings.rs` 的 `ServerSettings` 加一个字段，
 * 页面本身不用改。数值范围要和 `settings.rs` 里 `validate` 的边界对上：
 * 这边只负责提示，真正拦住非法值的是 Rust 那一侧。
 *
 * 所有选项都是服务端启动时读一次，现在没有改完立刻生效的选项。以后要是有了，
 * 在这里加一个标记，再把页面顶部那句「要重启才生效」收窄到对应的项。
 */

import type { ServerSettings } from "../api/bamboo";

export const GROUPS = [
  { id: "basic", title: "基本", hint: "玩家怎么找到这台服务器。" },
  { id: "network", title: "网络", hint: "连接本身怎么传数据。" },
  {
    id: "auth",
    title: "正版验证",
    hint: "要不要让 Mojang 确认每名玩家的账号。关掉时谁都能随便填一个名字进服。和代理转发互斥：代理已经替服务端验过了，两个只能开一个。",
  },
  {
    id: "forwarding",
    title: "代理转发",
    hint: "服务端藏在 BungeeCord 或 Velocity 后面时才需要改。开启后一定要用防火墙挡住外部直连，否则任何人都能冒充任意玩家进服。",
  },
  { id: "advanced", title: "高级", hint: "挡住赖着不走的连接，一般不用改。" },
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
  /** 额外的校验，返回错误文字或者 null。整份草稿都给进来，可以跨项检查。 */
  validate?: (draft: ServerSettings) => string | null;
  /** 当前用不上这一项，灰掉但仍然显示。 */
  disabled?: (draft: ServerSettings) => boolean;
  /** 占满一整行。滑块、多选和长文本都需要。 */
  wide?: boolean;
}

// 每种控件各自一个类型，`control` 都是单个字面量：这样 switch 和 if 都能把类型收窄干净。
type Text<K extends keyof ServerSettings> = Common<K> & { maxLength?: number };
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
export type ToggleField = Common<KeysOf<boolean>> & { control: "toggle" };

export type SettingField =
  | TextField
  | PasswordField
  | NumberField
  | SliderField
  | ChoiceField
  | ToggleField;

/** 心跳必须留出余量：客户端 30 秒收不到任何字节就自己断开。 */
const MAX_KEEP_ALIVE_SECONDS = 29;
/** 帧长度上限是 2 MiB 差一字节，阈值到了这里就等于永远不压缩。 */
const MAX_COMPRESSION_THRESHOLD = 2_097_151;

export const FIELDS: readonly SettingField[] = [
  {
    key: "host",
    group: "basic",
    label: "监听地址",
    control: "text",
    hint: "127.0.0.1 只有本机能连，0.0.0.0 对局域网和外网开放。",
    validate: (draft) => (draft.host.trim().length > 0 ? null : "监听地址不能为空"),
  },
  {
    key: "port",
    group: "basic",
    label: "端口",
    control: "number",
    min: 1,
    max: 65535,
    hint: "客户端添加服务器时填的端口。",
  },
  {
    key: "motd",
    group: "basic",
    label: "服务器介绍",
    control: "text",
    maxLength: 120,
    wide: true,
    hint: "服务器列表里显示在名字下面的那行字，对应官方的 motd。",
  },
  {
    key: "maxPlayers",
    group: "basic",
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
    label: "视距",
    control: "slider",
    min: 2,
    max: 32,
    unit: "区块",
    wide: true,
    hint: "客户端的渲染半径，对应官方的 view-distance。越大，玩家进服时要传输的区块越多。",
  },
  {
    key: "compressionThreshold",
    group: "network",
    label: "压缩阈值",
    control: "number",
    min: -1,
    max: MAX_COMPRESSION_THRESHOLD,
    unit: "字节",
    hint: "负载超过这么多字节的包才压缩，对应官方的 network-compression-threshold。-1 关闭压缩，0 压缩每一个包。",
  },
  {
    key: "keepAliveSeconds",
    group: "network",
    label: "心跳间隔",
    control: "number",
    min: 1,
    max: MAX_KEEP_ALIVE_SECONDS,
    unit: "秒",
    hint: "服务端每隔这么久发一次心跳，上一次还没有回应就断开。客户端 30 秒收不到任何数据就会自己断开，所以这里必须小于 30 秒。",
  },
  {
    key: "onlineMode",
    group: "auth",
    label: "正版验证",
    control: "toggle",
    wide: true,
    hint: "对应官方的 online-mode。打开后登录过程会加密，每名玩家都要经过 Mojang 的 session server 确认，皮肤也由 Mojang 提供；服务端因此需要能访问外网。关掉时既不加密也不验证，UUID 由玩家名算出来。打开了代理转发就不能再打开这一项。",
    validate: (draft) =>
      draft.onlineMode && draft.forwarding !== "off"
        ? "正版验证和代理转发不能同时打开：代理已经替服务端验过身份了"
        : null,
  },
  {
    key: "preventProxyConnections",
    group: "auth",
    label: "禁止代理连接",
    control: "toggle",
    wide: true,
    disabled: (draft) => !draft.onlineMode,
    hint: "对应官方的 prevent-proxy-connections。打开后向 Mojang 提问时带上客户端的 IP，Mojang 会比对玩家登录时报备的那个，对不上就不认这次登录。玩家经过 VPN 或者代理进来时两边的 IP 不一样，会被挡住。",
  },
  {
    key: "enforceSecureProfile",
    group: "auth",
    label: "强制签名聊天",
    control: "toggle",
    wide: true,
    hint: "对应官方的 enforce-secure-profile，默认打开。正版验证也打开、并且取到了 Mojang 的公钥之后，没有签名的聊天和需要签名的命令会被拒绝。没开正版验证时这一项不起作用。",
  },
  {
    key: "forwarding",
    group: "forwarding",
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
    label: "进服连接数上限",
    control: "number",
    min: 1,
    max: 1_000_000,
    hint: "同时处于握手、登录、配置阶段的连接上限，超出的连接直接丢弃。已经进入游戏的玩家不占名额。",
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
  if (field.control === "choice") {
    const value = defaults[field.key];
    return field.options.find((option) => option.value === value)?.label ?? value;
  }
  if (field.control === "toggle") {
    return defaults[field.key] ? "开" : "关";
  }
  return defaults[field.key].length > 0 ? defaults[field.key] : "留空";
}
