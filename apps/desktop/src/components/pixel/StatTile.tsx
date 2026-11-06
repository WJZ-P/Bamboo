import type { ReactNode } from "react";
import styled from "styled-components";

import { SunkenBox } from "./PixelPanel";

const Tile = styled(SunkenBox)`
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  padding: 10px 12px;
`;

const Label = styled.span`
  color: ${({ theme }) => theme.color.textMuted};
  font-size: ${({ theme }) => theme.font.small};
`;

const Value = styled.span`
  overflow: hidden;
  font-size: ${({ theme }) => theme.font.title};
  line-height: 1.25;
  text-overflow: ellipsis;
  text-shadow: 2px 2px 0 ${({ theme }) => theme.color.black};
  white-space: nowrap;
`;

/** 展示单个数字的小方块。 */
export function StatTile({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Tile>
      <Label>{label}</Label>
      <Value>{children}</Value>
    </Tile>
  );
}
