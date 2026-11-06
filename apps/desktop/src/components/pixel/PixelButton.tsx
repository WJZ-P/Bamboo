import styled, { css } from "styled-components";

import { pressedBevel, raisedBevel } from "../../theme/bevel";

type ButtonVariant = "bamboo" | "stone" | "redstone";
type ButtonSize = "small" | "medium" | "large";

const SIZES = {
  small: css`
    padding: 4px 10px;
    border-width: 2px;
  `,
  medium: css`
    padding: 8px 18px;
    border-width: 2px;
  `,
  large: css`
    padding: 12px 32px;
    border-width: 4px;
    font-size: ${({ theme }) => theme.font.title};
  `,
};

/** Minecraft 风格的按钮，按下时斜角反转并下沉。 */
export const PixelButton = styled.button<{ $variant?: ButtonVariant; $size?: ButtonSize }>`
  ${({ $size = "medium" }) => SIZES[$size]}
  border-style: solid;
  border-color: ${({ theme }) => theme.color.black};
  background: ${({ theme, $variant = "stone" }) => theme.color[$variant].base};
  color: ${({ theme }) => theme.color.panelLight};
  text-shadow: 2px 2px 0 ${({ theme, $variant = "stone" }) => theme.color[$variant].shadow};
  cursor: pointer;
  ${({ theme, $variant = "stone", $size = "medium" }) =>
    raisedBevel(theme.color[$variant].light, theme.color[$variant].dark, $size === "large" ? 4 : 2)}

  &:hover:not(:disabled) {
    filter: brightness(1.15);
  }

  &:active:not(:disabled) {
    ${({ theme, $variant = "stone", $size = "medium" }) =>
      pressedBevel(
        theme.color[$variant].light,
        theme.color[$variant].dark,
        $size === "large" ? 4 : 2,
      )}
    transform: translateY(2px);
  }

  &:focus-visible {
    outline: 2px solid ${({ theme }) => theme.color.gold};
    outline-offset: 2px;
  }

  &:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
`;
