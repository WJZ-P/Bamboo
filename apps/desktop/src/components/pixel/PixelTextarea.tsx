import styled from "styled-components";

import { sunkenBorder } from "../../theme/bevel";

/** 多行文本输入框，斜角、配色和 [PixelInput] 相同，高度可以往下拉。 */
export const PixelTextarea = styled.textarea`
  width: 100%;
  min-height: 84px;
  padding: 8px 10px;
  ${({ theme }) => sunkenBorder(theme.color.sunkenShade, theme.color.panelLight)}
  background: ${({ theme }) => theme.color.field};
  color: ${({ theme }) => theme.color.panelLight};
  line-height: 1.6;
  outline: none;
  resize: vertical;
  user-select: text;

  &::placeholder {
    color: ${({ theme }) => theme.color.panelShade};
  }

  &:focus {
    border-color: ${({ theme }) => theme.color.bamboo.light};
  }

  &[aria-invalid="true"] {
    border-color: ${({ theme }) => theme.color.redstone.light};
  }

  &:disabled {
    opacity: 0.5;
  }
`;
