import styled from "styled-components";

import { sunkenBorder } from "../../theme/bevel";

/** 向内凹陷的下拉框，斜角和输入框一致；`color-scheme` 让系统弹出层也用深色。 */
export const PixelSelect = styled.select`
  width: 100%;
  padding: 8px 10px;
  ${({ theme }) => sunkenBorder(theme.color.sunkenShade, theme.color.panelLight)}
  background: ${({ theme }) => theme.color.black};
  color: ${({ theme }) => theme.color.panelLight};
  outline: none;
  cursor: pointer;
  color-scheme: dark;

  &:focus {
    border-color: ${({ theme }) => theme.color.bamboo.light};
  }

  &:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
`;
