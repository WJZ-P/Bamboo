/**
 * 全局设计变量。配色和桌面控制台（apps/desktop）一致：Minecraft 箱子 GUI 的灰色，主色取竹子的绿。
 *
 * Global design tokens. The colours match the desktop console in apps/desktop: the grey of a
 * Minecraft chest GUI, with bamboo green as the accent.
 */
export const theme = {
  color: {
    page: "#131313",
    text: "#e8e8e8",
    textMuted: "#a0a0a0",
    black: "#000000",

    /** 箱子 GUI 风格的浅色面板。 / Light panels in the chest GUI style. */
    panel: "#c6c6c6",
    panelLight: "#ffffff",
    panelShade: "#555555",
    panelText: "#2b2b2b",
    panelTextMuted: "#4f4f4f",

    /** 向内凹陷的深色区域。 / Dark sunken areas. */
    sunken: "#161616",
    sunkenShade: "#373737",
    sunkenLight: "#8b8b8b",

    /** 物品栏格子。 / Inventory slots. */
    slot: "#8b8b8b",

    /** 导航栏和页脚的钢板。 / The steel plate of the navigation bar and footer. */
    steel: "#282a2e",
    steelEdge: "#111111",

    bamboo: { base: "#3c8527", light: "#5eac44", dark: "#225313", shadow: "#1a3b11" },
    stone: { base: "#555555", light: "#777777", dark: "#333333", shadow: "#000000" },

    gold: "#ffcc33",
    diamond: "#7ec8e3",
    redstone: "#e0584f",
    amethyst: "#b79cf2",
    copper: "#e8915a",
  },
  /** 里程碑和步骤的状态色，总是和文字标签一起出现。 / Status colours, always shown with a text label. */
  status: {
    done: "#6be04a",
    active: "#ffcc33",
    partial: "#7ec8e3",
    planned: "#8a8a8a",
  },
  /**
   * 图表用色。series 是区分身份用的六个色，顺序固定，在深色底上通过了色觉障碍可分辨性检查；
   * 只有一个系列的图一律用 bar。
   *
   * Chart colours. `series` holds six identity colours in a fixed order, checked for
   * colour-vision-deficiency separation on the dark surface. Single-series charts use `bar`.
   */
  chart: {
    surface: "#161616",
    bar: "#4f9e3a",
    track: "#262626",
    grid: "#3a3a3a",
    series: ["#4f9e3a", "#9a7be0", "#b98a14", "#3fa0c4", "#d9544b", "#2aa58c"],
  },
  font: {
    family: `"Fusion Pixel", "Microsoft YaHei UI", "PingFang SC", monospace`,
    /**
     * 字体的原生点阵是 12px，取它的整数倍显示最清晰；18px 略虚，但读长段文字最舒服。
     *
     * The font's native grid is 12px, so integer multiples are the sharpest. 18px is slightly
     * soft but the most comfortable size for paragraphs.
     */
    caption: "12px",
    body: "18px",
    lead: "24px",
    heading: "36px",
    display: "72px",
  },
  layout: {
    maxWidth: "1160px",
    navHeight: "72px",
  },
} as const;

export type ThemeType = typeof theme;
