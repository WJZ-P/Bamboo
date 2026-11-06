import styled from "styled-components";

import { sunkenBorder } from "../../theme/bevel";

/** 向内凹陷的文本输入框。 */
export const PixelInput = styled.input`
  width: 100%;
  padding: 8px 10px;
  ${({ theme }) => sunkenBorder(theme.color.sunkenShade, theme.color.panelLight)}
  background: ${({ theme }) => theme.color.black};
  color: ${({ theme }) => theme.color.panelLight};
  outline: none;
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
`;
