/** 全局设计变量。配色沿用 Minecraft 箱子 GUI 的灰色，主色取竹子的绿。 */
export const theme = {
  color: {
    page: "#131313",
    text: "#e8e8e8",
    textMuted: "#9a9a9a",
    black: "#000000",

    /** 箱子 GUI 风格的浅色面板。 */
    panel: "#c6c6c6",
    panelLight: "#ffffff",
    panelShade: "#555555",
    panelText: "#3f3f3f",
    panelTextMuted: "#5f5f5f",

    /** 向内凹陷的深色区域。 */
    sunken: "#101010",
    sunkenShade: "#373737",
    sunkenLight: "#8b8b8b",

    /** 物品栏格子。 */
    slot: "#8b8b8b",

    bamboo: { base: "#3c8527", light: "#5eac44", dark: "#225313", shadow: "#1a3b11" },
    stone: { base: "#555555", light: "#777777", dark: "#333333", shadow: "#000000" },
    redstone: { base: "#a32424", light: "#d14a4a", dark: "#6b1212", shadow: "#3f0000" },

    gold: "#ffcc33",
    online: "#6be04a",
    offline: "#8a8a8a",
  },
  /** 性能曲线。CPU 用竹子绿，整机用金色细线，内存用钻石青。 */
  chart: {
    grid: "#2b2b2b",
    cpu: "#5eac44",
    cpuFill: "rgba(94, 172, 68, 0.35)",
    system: "#ffcc33",
    memory: "#7ec8e3",
    memoryFill: "rgba(126, 200, 227, 0.3)",
  },
  log: {
    ERROR: "#ff6b6b",
    WARN: "#ffcc33",
    INFO: "#a7e38f",
    DEBUG: "#7ec8e3",
    TRACE: "#9a9a9a",
  },
  font: {
    family: `"Fusion Pixel", "Microsoft YaHei UI", monospace`,
    /** 字体的原生点阵是 12px，取它的整数倍显示最清晰。 */
    small: "12px",
    body: "12px",
    /** 侧边栏导航。不是 12 的整数倍，边缘会略虚，但大小最合适。 */
    nav: "18px",
    title: "24px",
    display: "36px",
  },
} as const;

export type ThemeType = typeof theme;
