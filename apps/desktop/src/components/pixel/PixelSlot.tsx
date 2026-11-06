import styled from "styled-components";

/** 物品栏格子，用来放玩家头像之类的图标。 */
export const PixelSlot = styled.div`
  display: inline-flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 52px;
  height: 52px;
  border: 2px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.slot};
  box-shadow:
    inset 2px 2px 0 0 ${({ theme }) => theme.color.sunkenShade},
    inset -2px -2px 0 0 ${({ theme }) => theme.color.panelLight};
`;
