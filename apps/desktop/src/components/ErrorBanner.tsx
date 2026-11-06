import styled from "styled-components";

import { useServer } from "../state/ServerContext";
import { raisedBevel } from "../theme/bevel";
import { PixelButton } from "./pixel";

const Banner = styled.div`
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 10px 8px 14px;
  border: 4px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.redstone.base};
  color: ${({ theme }) => theme.color.panelLight};
  text-shadow: 2px 2px 0 ${({ theme }) => theme.color.redstone.shadow};
  user-select: text;
  ${({ theme }) => raisedBevel(theme.color.redstone.light, theme.color.redstone.dark, 2)}
`;

/** 启动失败之类的错误提示，直到用户关闭才消失。 */
export function ErrorBanner() {
  const { error, dismissError } = useServer();
  if (!error) {
    return null;
  }

  return (
    <Banner role="alert">
      {error}
      <PixelButton type="button" $size="small" onClick={dismissError}>
        知道了
      </PixelButton>
    </Banner>
  );
}
