import styled from "styled-components";

import { raisedBevel, sunkenBorder } from "../../theme/bevel";

/** 箱子 GUI 风格的浅色面板。 */
export const PixelPanel = styled.section`
  padding: 18px 20px;
  border: 4px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.panel};
  color: ${({ theme }) => theme.color.panelText};
  ${({ theme }) => raisedBevel(theme.color.panelLight, theme.color.panelShade)}
`;

export const PanelTitle = styled.h2`
  margin: 0 0 12px;
  color: ${({ theme }) => theme.color.panelText};
  font-size: ${({ theme }) => theme.font.title};
  font-weight: 400;
  line-height: 1.25;
`;

/** 向内凹陷的深色展示区，用来放日志和数据。 */
export const SunkenBox = styled.div`
  ${({ theme }) => sunkenBorder(theme.color.sunkenShade, theme.color.panelLight)}
  background: ${({ theme }) => theme.color.sunken};
  color: ${({ theme }) => theme.color.text};
`;
