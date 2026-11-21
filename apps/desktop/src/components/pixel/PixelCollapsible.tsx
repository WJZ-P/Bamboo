import { type ReactNode, useId, useState } from "react";
import styled from "styled-components";

import { raisedBevel } from "../../theme/bevel";

/**
 * 可以收起的大卡片。顶上是 Minecraft 风格的深色标题条，整条可点，右边的像素箭头朝下表示
 * 展开、朝右表示收起。内容区用 `grid-template-rows: 0fr ↔ 1fr` 做高度过渡，箭头同步旋转；
 * 系统要求减少动效时直接切换。收起时内容设成 `inert`，键盘焦点进不去，但不卸载，
 * 表单里填了一半的值不会丢。
 *
 * A large collapsible card. A dark Minecraft-style title bar sits on top; the whole bar
 * is clickable and the pixel arrow on the right points down when open, right when
 * collapsed. The body animates its height through `grid-template-rows: 0fr ↔ 1fr` while
 * the arrow rotates; with reduced motion requested it switches instantly. Collapsed
 * content is `inert` (no keyboard focus) but stays mounted, so half-filled form values
 * survive.
 */
type Props = {
  title: string;
  hint?: ReactNode;
  /** 标题条右边、箭头左边的小标签，比如「2 项有误」。 */
  badge?: ReactNode;
  /** 为真时强制展开（比如里面有校验错误），用户收不起来。 */
  forceOpen?: boolean;
  defaultOpen?: boolean;
  children: ReactNode;
};

const DURATION = "220ms";
const EASING = "cubic-bezier(0.2, 0, 0, 1)";

const Card = styled.section`
  border: 4px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.panel};
  color: ${({ theme }) => theme.color.panelText};
  ${({ theme }) => raisedBevel(theme.color.panelLight, theme.color.panelShade)}
`;

/** 深色标题条：石头灰底、上亮下暗的斜角，和 Minecraft 列表里的选中条一个味道。 */
const Header = styled.button`
  display: flex;
  width: 100%;
  align-items: center;
  gap: 12px;
  padding: 12px 18px;
  border: none;
  border-bottom: 4px solid ${({ theme }) => theme.color.black};
  background: ${({ theme }) => theme.color.stone.dark};
  color: ${({ theme }) => theme.color.text};
  font: inherit;
  text-align: left;
  cursor: pointer;
  ${({ theme }) => raisedBevel(theme.color.stone.light, theme.color.sunken)}
  transition: background ${DURATION} ${EASING};

  &:hover {
    background: ${({ theme }) => theme.color.stone.base};
  }

  &:focus-visible {
    outline: 4px solid ${({ theme }) => theme.color.bamboo.light};
    outline-offset: -8px;
  }

  &[aria-expanded="false"] {
    border-bottom-color: transparent;
  }
`;

const Title = styled.h2`
  margin: 0;
  font-size: ${({ theme }) => theme.font.title};
  font-weight: 400;
  line-height: 1.25;
  /* Minecraft 文字的硬投影。The hard drop shadow of Minecraft text. */
  text-shadow: 2px 2px 0 ${({ theme }) => theme.color.sunken};
`;

const Badge = styled.span`
  margin-left: auto;
  padding: 2px 8px;
  background: ${({ theme }) => theme.color.redstone.base};
  color: ${({ theme }) => theme.color.panelLight};
`;

/** 像素三角：边框拼出来，展开朝下、收起转成朝右。 */
const Arrow = styled.span<{ $open: boolean }>`
  flex: none;
  margin-left: auto;
  width: 0;
  height: 0;
  border-left: 8px solid transparent;
  border-right: 8px solid transparent;
  border-top: 10px solid ${({ theme }) => theme.color.text};
  filter: drop-shadow(2px 2px 0 ${({ theme }) => theme.color.sunken});
  transform: rotate(${({ $open }) => ($open ? "0deg" : "90deg")});
  transition: transform ${DURATION} ${EASING};

  ${Badge} + & {
    margin-left: 0;
  }
`;

/** 外层负责高度动画：0fr 收起，1fr 展开。The outer box animates the height. */
const Collapse = styled.div<{ $open: boolean }>`
  display: grid;
  grid-template-rows: ${({ $open }) => ($open ? "1fr" : "0fr")};
  transition: grid-template-rows ${DURATION} ${EASING};

  @media (prefers-reduced-motion: reduce) {
    transition: none;
  }
`;

const Clip = styled.div<{ $open: boolean }>`
  min-height: 0;
  overflow: hidden;
  opacity: ${({ $open }) => ($open ? 1 : 0)};
  transition: opacity ${DURATION} ${EASING};

  @media (prefers-reduced-motion: reduce) {
    transition: none;
  }
`;

const Body = styled.div`
  padding: 16px 20px 18px;
`;

const Hint = styled.p`
  margin: 0 0 16px;
  color: ${({ theme }) => theme.color.panelTextMuted};
  line-height: 1.6;
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
        <Title>{title}</Title>
        {badge && <Badge>{badge}</Badge>}
        <Arrow $open={shown} aria-hidden />
      </Header>
      <Collapse $open={shown}>
        <Clip $open={shown} id={bodyId} inert={!shown}>
          <Body>
            {hint && <Hint>{hint}</Hint>}
            {children}
          </Body>
        </Clip>
      </Collapse>
    </Card>
  );
}

/** 卡片里的一个小分组：上面一道带斜角的分隔线，标题比卡片标题小一号。 */
export const CollapsibleGroup = styled.div`
  padding-top: 16px;
  margin-top: 16px;
  border-top: 4px solid ${({ theme }) => theme.color.panelShade};
  box-shadow: inset 0 4px 0 0 ${({ theme }) => theme.color.panelLight};

  &:first-of-type {
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
