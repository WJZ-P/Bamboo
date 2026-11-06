import { type KeyboardEvent, type PointerEvent, useRef } from "react";
import styled from "styled-components";

import { pressedBevel, raisedBevel, sunkenBorder } from "../../theme/bevel";

const Track = styled.div`
  position: relative;
  height: 24px;
  ${({ theme }) => sunkenBorder(theme.color.sunkenShade, theme.color.slot, 3)}
  background: ${({ theme }) => theme.color.stone.base};
  cursor: pointer;
  touch-action: none;

  &:focus-visible {
    outline: 2px solid ${({ theme }) => theme.color.gold};
    outline-offset: 2px;
  }
`;

const Fill = styled.div`
  position: absolute;
  inset: 0 auto 0 0;
  background: ${({ theme }) => theme.color.bamboo.base};
  box-shadow:
    inset 0 3px 0 0 ${({ theme }) => theme.color.bamboo.light},
    inset 0 -2px 0 0 ${({ theme }) => theme.color.bamboo.dark};
  pointer-events: none;
`;

const Thumb = styled.div`
  position: absolute;
  top: 50%;
  width: 20px;
  height: 30px;
  border: 3px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.panel};
  transform: translate(-50%, -50%);
  ${({ theme }) => raisedBevel(theme.color.panelLight, theme.color.panelShade, 3)}
  pointer-events: none;

  ${Track}:active & {
    ${({ theme }) => pressedBevel(theme.color.panelLight, theme.color.panelShade, 3)}
  }
`;

interface PixelSliderProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  label: string;
  onChange: (value: number) => void;
}

/** 像素风滑块。支持鼠标拖动，也支持方向键调节。 */
export function PixelSlider({ value, min, max, step = 1, label, onChange }: PixelSliderProps) {
  const trackRef = useRef<HTMLDivElement>(null);
  const percent = ((value - min) / (max - min)) * 100;

  const updateFromPointer = (clientX: number) => {
    const bounds = trackRef.current?.getBoundingClientRect();
    if (!bounds) {
      return;
    }
    const ratio = Math.min(1, Math.max(0, (clientX - bounds.left) / bounds.width));
    onChange(min + Math.round((ratio * (max - min)) / step) * step);
  };

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    updateFromPointer(event.clientX);
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      updateFromPointer(event.clientX);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const direction = { ArrowLeft: -1, ArrowDown: -1, ArrowRight: 1, ArrowUp: 1 }[event.key];
    if (direction) {
      event.preventDefault();
      onChange(Math.min(max, Math.max(min, value + direction * step)));
    }
  };

  return (
    <Track
      ref={trackRef}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onKeyDown={handleKeyDown}
    >
      <Fill style={{ width: `${percent}%` }} />
      <Thumb style={{ left: `${percent}%` }} />
    </Track>
  );
}
