import styled from "styled-components";

/** 方形状态灯，亮起时带一圈像素光晕。 */
export const StatusLamp = styled.span<{ $on: boolean }>`
  display: inline-block;
  flex: none;
  width: 12px;
  height: 12px;
  border: 2px solid ${({ theme }) => theme.color.black};
  background: ${({ theme, $on }) => ($on ? theme.color.online : theme.color.offline)};
  box-shadow: ${({ $on }) =>
    $on
      ? "inset 2px 2px 0 0 #c9ffb8, 0 0 0 2px rgba(107, 224, 74, 0.25)"
      : "inset 2px 2px 0 0 #b5b5b5"};
`;
