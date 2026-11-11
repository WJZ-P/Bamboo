import type { ReactNode } from "react";
import styled, { css } from "styled-components";

import { pressedBevel, raisedBevel, sunkenBorder, textShadow } from "../theme/bevel";
import type { ThemeType } from "../theme/theme";

type ButtonTone = "bamboo" | "stone";

interface ButtonStyleProps {
  readonly $tone?: ButtonTone;
  readonly $large?: boolean;
  readonly $pressed?: boolean;
}

const buttonStyle = css<ButtonStyleProps>`
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: ${({ $large }) => ($large ? "12px 28px" : "6px 14px")};
  border: ${({ $large }) => ($large ? 4 : 2)}px solid ${({ theme }) => theme.color.black};
  background: ${({ theme, $tone = "stone" }) => theme.color[$tone].base};
  color: #ffffff;
  font-size: ${({ theme, $large }) => ($large ? theme.font.lead : theme.font.body)};
  line-height: 1.2;
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
  ${({ theme, $tone = "stone", $large, $pressed }) =>
    ($pressed ? pressedBevel : raisedBevel)(theme.color[$tone].light, theme.color[$tone].dark, $large ? 4 : 2)}
  ${({ theme, $tone = "stone", $large }) => textShadow(theme.color[$tone].shadow, $large ? 2 : 1)}

  &:hover {
    filter: brightness(1.15);
  }

  &:active {
    transform: translateY(2px);
    ${({ theme, $tone = "stone", $large }) =>
      pressedBevel(theme.color[$tone].light, theme.color[$tone].dark, $large ? 4 : 2)}
  }
`;

/** 像素按钮。绿色是主要操作，灰色是次要操作。 / A pixel button: green for primary actions, grey for secondary ones. */
export const PixelButton = styled.button<ButtonStyleProps>`
  ${buttonStyle}
`;

/** 长得像按钮的链接。 / A link that looks like a button. */
export const PixelLink = styled.a<ButtonStyleProps>`
  ${buttonStyle}
`;

const GuiOuter = styled.div`
  background: ${({ theme }) => theme.color.panel};
  border: 4px solid ${({ theme }) => theme.color.black};
  padding: 4px;
  box-shadow: 12px 12px 0 0 rgba(0, 0, 0, 0.55);
`;

const GuiInner = styled.div`
  border: 4px solid;
  border-color: ${({ theme }) =>
    `${theme.color.panelLight} ${theme.color.panelShade} ${theme.color.panelShade} ${theme.color.panelLight}`};
  padding: 24px;
  color: ${({ theme }) => theme.color.panelText};

  @media (max-width: 640px) {
    padding: 14px;
  }
`;

/** 箱子界面那样的浅灰面板。 / A light grey panel in the style of a chest screen. */
export function GuiPanel({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <GuiOuter className={className}>
      <GuiInner>{children}</GuiInner>
    </GuiOuter>
  );
}

/** 面板里的标题，深灰字配白色硬阴影。 / A panel title: dark grey with a hard white shadow. */
export const GuiTitle = styled.h3`
  font-size: ${({ theme }) => theme.font.lead};
  line-height: 1.3;
  color: ${({ theme }) => theme.color.panelText};
  ${textShadow("#ffffff", 1)}
`;

/** 向内凹的深色展示区，底下铺一层压暗的石头。 / A dark sunken display over dimmed stone. */
export const Sunken = styled.div`
  ${({ theme }) => sunkenBorder(theme.color.sunkenShade, theme.color.panelLight)}
  background-color: ${({ theme }) => theme.color.sunken};
  background-image:
    linear-gradient(rgba(0, 0, 0, 0.8), rgba(0, 0, 0, 0.8)),
    var(--tex-stone);
  background-size: auto, 64px 64px;
  image-rendering: pixelated;
  color: ${({ theme }) => theme.color.text};
`;

/** 物品栏格子。 / An inventory slot. */
export const Slot = styled.div<{ $size?: number }>`
  display: flex;
  align-items: center;
  justify-content: center;
  width: ${({ $size = 48 }) => $size}px;
  height: ${({ $size = 48 }) => $size}px;
  flex: none;
  background: ${({ theme }) => theme.color.slot};
  ${({ theme }) => sunkenBorder(theme.color.sunkenShade, theme.color.panelLight, 2)}
`;

const HeadingWrap = styled.header`
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  margin-bottom: 36px;
  text-align: center;
`;

const HeadingTitle = styled.h2`
  font-size: ${({ theme }) => theme.font.heading};
  line-height: 1.2;
  letter-spacing: 2px;
  color: #ffffff;
  text-shadow:
    4px 4px 0 #000000,
    0 8px 16px rgba(0, 0, 0, 0.8);

  @media (max-width: 640px) {
    font-size: ${({ theme }) => theme.font.lead};
    text-shadow: 3px 3px 0 #000000;
  }
`;

const HeadingRule = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
  max-width: 760px;
  color: ${({ theme }) => theme.color.panel};
  ${textShadow()}

  &::before,
  &::after {
    content: "";
    flex: none;
    width: 48px;
    height: 4px;
    background: ${({ theme }) => theme.color.bamboo.light};
    box-shadow: 2px 2px 0 0 #000000;
  }

  @media (max-width: 640px) {
    &::before,
    &::after {
      width: 16px;
    }
  }
`;

/** 每个板块的标题：白色大字，下面一行说明夹在两条绿杠中间。 / A section heading: large white type over a caption between two green bars. */
export function SectionHeading({ title, caption }: { title: string; caption: string }) {
  return (
    <HeadingWrap>
      <HeadingTitle>{title}</HeadingTitle>
      <HeadingRule>
        <span>{caption}</span>
      </HeadingRule>
    </HeadingWrap>
  );
}

export const Section = styled.section`
  position: relative;
  width: 100%;
  max-width: ${({ theme }) => theme.layout.maxWidth};
  margin: 0 auto;
  padding: 72px 20px 24px;

  @media (max-width: 640px) {
    padding: 48px 12px 16px;
  }
`;

export type StatusKey = keyof ThemeType["status"];

const BadgeWrap = styled.span<{ $status: StatusKey }>`
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 2px 10px 2px 6px;
  border: 2px solid ${({ theme }) => theme.color.black};
  background: #1e1e1e;
  color: ${({ theme }) => theme.color.text};
  font-size: ${({ theme }) => theme.font.body};
  line-height: 1.3;
  white-space: nowrap;
  ${raisedBevel("#373737", "#101010", 2)}

  &::before {
    content: "";
    width: 12px;
    height: 12px;
    flex: none;
    background: ${({ theme, $status }) => theme.status[$status]};
    box-shadow:
      inset 2px 2px 0 0 rgba(255, 255, 255, 0.45),
      inset -2px -2px 0 0 rgba(0, 0, 0, 0.35);
    border: 2px solid #000000;
  }
`;

/** 状态徽章：一盏小灯加文字，状态不只靠颜色表达。 / A status badge: a small lamp plus a label, so the state never relies on colour alone. */
export function StatusBadge({ status, label }: { status: StatusKey; label: string }) {
  return <BadgeWrap $status={status}>{label}</BadgeWrap>;
}

/** 小标签，比如功能卡片上的关键词。 / A small tag, such as a keyword on a feature card. */
export const Tag = styled.span`
  display: inline-block;
  padding: 0 8px;
  border: 2px solid ${({ theme }) => theme.color.panelShade};
  background: rgba(255, 255, 255, 0.28);
  color: ${({ theme }) => theme.color.panelText};
  font-size: ${({ theme }) => theme.font.body};
  line-height: 1.5;
  white-space: nowrap;
`;

/** 面板顶上的一行：左边标题，右边一句说明或一个链接。 / The top row of a panel: a title on the left, a remark or a link on the right. */
export const PanelHead = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  justify-content: space-between;
  gap: 4px 16px;
  margin-bottom: 16px;

  a {
    color: ${({ theme }) => theme.color.bamboo.dark};
    box-shadow: 0 2px 0 0 ${({ theme }) => theme.color.bamboo.base};
  }

  a:hover {
    color: ${({ theme }) => theme.color.black};
  }
`;

/** 面板里的小卡片：半透明的浅色加黑边。 / A small card inside a panel: translucent light fill with a black edge. */
export const Card = styled.li`
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  border: 3px solid #000000;
  background: rgba(255, 255, 255, 0.18);
  ${raisedBevel("rgba(255, 255, 255, 0.38)", "rgba(0, 0, 0, 0.3)", 3)}
`;
