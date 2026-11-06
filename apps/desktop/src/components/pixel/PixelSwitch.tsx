import styled from "styled-components";

import { raisedBevel, sunkenBorder } from "../../theme/bevel";

const Root = styled.button`
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0;
  border: 0;
  background: none;
  color: inherit;
  cursor: pointer;

  &:focus-visible {
    outline: 2px solid ${({ theme }) => theme.color.gold};
    outline-offset: 2px;
  }
`;

const Rail = styled.span<{ $on: boolean }>`
  position: relative;
  width: 40px;
  height: 20px;
  ${({ theme }) => sunkenBorder(theme.color.sunkenShade, theme.color.slot, 2)}
  background: ${({ theme, $on }) => ($on ? theme.color.bamboo.base : theme.color.stone.dark)};
`;

const Knob = styled.span<{ $on: boolean }>`
  position: absolute;
  top: -2px;
  left: ${({ $on }) => ($on ? "18px" : "-2px")};
  width: 20px;
  height: 20px;
  border: 2px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.panel};
  ${({ theme }) => raisedBevel(theme.color.panelLight, theme.color.panelShade, 2)}
`;

interface PixelSwitchProps {
  checked: boolean;
  label: string;
  onChange: (checked: boolean) => void;
}

/** 像素风开关。 */
export function PixelSwitch({ checked, label, onChange }: PixelSwitchProps) {
  return (
    <Root type="button" role="switch" aria-checked={checked} onClick={() => onChange(!checked)}>
      <Rail $on={checked}>
        <Knob $on={checked} />
      </Rail>
      {label}
    </Root>
  );
}
