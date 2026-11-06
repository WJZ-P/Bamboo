import { getCurrentWindow } from "@tauri-apps/api/window";
import styled from "styled-components";

import icon from "../../assets/icon.png";
import { useServer } from "../../state/ServerContext";
import { raisedBevel } from "../../theme/bevel";
import { StatusLamp } from "../pixel";

const Bar = styled.header`
  display: flex;
  flex: none;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding-left: 10px;
  border-bottom: 4px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.stone.dark};
  ${({ theme }) => raisedBevel(theme.color.stone.base, "#222222", 2)}
`;

const Logo = styled.img`
  width: 20px;
  height: 20px;
  pointer-events: none;
`;

const Name = styled.span`
  flex: 1;
  text-shadow: 2px 2px 0 ${({ theme }) => theme.color.black};
`;

const WindowButton = styled.button<{ $danger?: boolean }>`
  width: 44px;
  height: 100%;
  border: 0;
  border-left: 2px solid ${({ theme }) => theme.color.black};
  background: none;
  cursor: pointer;

  &:hover {
    background: ${({ theme, $danger }) =>
      $danger ? theme.color.redstone.base : theme.color.stone.base};
  }
`;

const appWindow = getCurrentWindow();

/** 自绘标题栏。`data-tauri-drag-region` 让这块区域可以拖动窗口。 */
export function TitleBar() {
  const { snapshot } = useServer();

  return (
    <Bar data-tauri-drag-region>
      <Logo src={icon} alt="" />
      <Name data-tauri-drag-region>Bamboo</Name>
      <StatusLamp $on={snapshot?.running ?? false} />
      <WindowButton type="button" aria-label="最小化" onClick={() => appWindow.minimize()}>
        _
      </WindowButton>
      <WindowButton type="button" aria-label="最大化" onClick={() => appWindow.toggleMaximize()}>
        □
      </WindowButton>
      <WindowButton $danger type="button" aria-label="关闭" onClick={() => appWindow.close()}>
        ×
      </WindowButton>
    </Bar>
  );
}
