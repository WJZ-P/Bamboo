import { type ReactNode, useId, useState } from "react";
import styled from "styled-components";

import { raisedBevel } from "../../theme/bevel";

/**
 * 可以收起的大卡片：标题栏整条可点，箭头朝下是展开、朝右是收起。
 * 收起时内容用 `hidden` 藏起来，不卸载，表单里填了一半的值不会丢。
 *
 * A large collapsible card: the whole header bar is clickable; the arrow points down
 * when open and right when collapsed. Collapsed content is hidden, not unmounted, so
 * half-filled form values survive.
 */
type Props = {
  title: string;
  hint?: ReactNode;
  /** 标题右边的小标签，比如「2 项有误」。 */
  badge?: ReactNode;
  /** 为真时强制展开（比如里面有校验错误），用户收不起来。 */
  forceOpen?: boolean;
  defaultOpen?: boolean;
  children: ReactNode;
};

const Card = styled.section`
  border: 4px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.panel};
  color: ${({ theme }) => theme.color.panelText};
  ${({ theme }) => raisedBevel(theme.color.panelLight, theme.color.panelShade)}
`;

const Header = styled.button`
  display: flex;
  width: 100%;
  align-items: center;
  gap: 12px;
  padding: 16px 20px;
  border: none;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;

  &:hover {
    background: rgba(255, 255, 255, 0.18);
  }

  &:focus-visible {
    outline: 4px solid ${({ theme }) => theme.color.bamboo.base};
    outline-offset: -8px;
  }
`;

/** 像素三角：用边框拼出来，转 90 度表示收起。 */
const Arrow = styled.span<{ $open: boolean }>`
  flex: none;
  width: 0;
  height: 0;
  border-left: 7px solid transparent;
  border-right: 7px solid transparent;
  border-top: 10px solid ${({ theme }) => theme.color.panelText};
  transform: rotate(${({ $open }) => ($open ? "0deg" : "-90deg")});
`;

const Title = styled.h2`
  margin: 0;
  font-size: ${({ theme }) => theme.font.title};
  font-weight: 400;
  line-height: 1.25;
`;

const Badge = styled.span`
  margin-left: auto;
  padding: 2px 8px;
  background: ${({ theme }) => theme.color.redstone.base};
  color: ${({ theme }) => theme.color.panelLight};
`;

const Hint = styled.p`
  margin: -6px 20px 14px 46px;
  color: ${({ theme }) => theme.color.panelTextMuted};
  line-height: 1.6;
`;

const Body = styled.div`
  padding: 0 20px 18px;
`;

export function PixelCollapsible({
  title,
  hint,
  badge,
  forceOpen = false,
  defaultOpen = true,
  children,
}: Props) {
  const [open, setOpen] = useState(defaultOpen);
  const bodyId = useId();
  const shown = open || forceOpen;
  return (
    <Card>
      <Header
        type="button"
        aria-expanded={shown}
        aria-controls={bodyId}
        onClick={() => setOpen(!shown)}
      >
        <Arrow $open={shown} aria-hidden />
        <Title>{title}</Title>
        {badge && <Badge>{badge}</Badge>}
      </Header>
      {hint && <Hint>{hint}</Hint>}
      <Body id={bodyId} hidden={!shown}>
        {children}
      </Body>
    </Card>
  );
}

/** 卡片里的一个小分组：上面一条分隔线，标题比卡片标题小一号。 */
export const CollapsibleGroup = styled.div`
  padding-top: 16px;
  margin-top: 16px;
  border-top: 4px solid ${({ theme }) => theme.color.panelShade};
  box-shadow: inset 0 4px 0 0 ${({ theme }) => theme.color.panelLight};

  &:first-child {
    margin-top: 0;
    padding-top: 0;
    border-top: none;
    box-shadow: none;
  }
`;

export const GroupTitle = styled.h3`
  margin: 0 0 8px;
  font-size: ${({ theme }) => theme.font.nav};
  font-weight: 400;
`;
