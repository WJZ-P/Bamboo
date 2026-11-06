import { css } from "styled-components";

/** 外凸的斜角：左上亮、右下暗，是所有像素按钮和面板的基础。 */
export const raisedBevel = (light: string, dark: string, size = 4) => css`
  box-shadow:
    inset ${size}px ${size}px 0 0 ${light},
    inset -${size}px -${size}px 0 0 ${dark};
`;

/** 内凹的斜角：把外凸的明暗反过来，用在按下的按钮上。 */
export const pressedBevel = (light: string, dark: string, size = 4) =>
  raisedBevel(dark, light, size);

/** 内凹的边框：左上暗、右下亮，用在输入框和展示区上。 */
export const sunkenBorder = (shade: string, light: string, size = 4) => css`
  border: ${size}px solid;
  border-color: ${shade} ${light} ${light} ${shade};
`;
