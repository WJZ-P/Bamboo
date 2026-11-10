import { css } from "styled-components";

/**
 * 外凸的斜角：左上亮、右下暗，是所有像素按钮和面板的基础。
 *
 * A raised bevel: light on the top-left, dark on the bottom-right. Every pixel button and
 * panel builds on it.
 */
export const raisedBevel = (light: string, dark: string, size = 4) => css`
  box-shadow:
    inset ${size}px ${size}px 0 0 ${light},
    inset -${size}px -${size}px 0 0 ${dark};
`;

/**
 * 内凹的斜角：把外凸的明暗反过来，用在按下的按钮上。
 *
 * A pressed bevel: the raised one with light and dark swapped, used on pressed buttons.
 */
export const pressedBevel = (light: string, dark: string, size = 4) =>
  raisedBevel(dark, light, size);

/**
 * 内凹的边框：左上暗、右下亮，用在展示区和格子上。
 *
 * A sunken border: dark on the top-left, light on the bottom-right, used on displays and slots.
 */
export const sunkenBorder = (shade: string, light: string, size = 4) => css`
  border: ${size}px solid;
  border-color: ${shade} ${light} ${light} ${shade};
`;

/** 硬边的文字阴影。 / A hard-edged text shadow. */
export const textShadow = (color = "#000000", size = 2) => css`
  text-shadow: ${size}px ${size}px 0 ${color};
`;
