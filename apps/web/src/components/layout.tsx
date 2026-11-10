import { useEffect, useRef } from "react";
import styled, { css } from "styled-components";

import iconUrl from "../assets/icon.png";
import { REPO_URL } from "../data/project";
import { useLocale, type Text } from "../i18n";
import { textShadow } from "../theme/bevel";
import { PixelButton, PixelLink } from "./pixel";

// ---- 背景 / Backdrop ----

const Stone = styled.div`
  position: fixed;
  inset: -24px;
  z-index: 0;
  background-image: var(--tex-backdrop);
  background-size: 640px 640px;
  image-rendering: pixelated;
  transform: translate(calc(var(--mouse-x, 0) * -12px), calc(var(--mouse-y, 0) * -12px));
  transition: transform 0.2s ease-out;

  @media (prefers-reduced-motion: reduce) {
    transform: none;
  }
`;

const Shade = styled.div`
  position: fixed;
  inset: 0;
  z-index: 0;
  pointer-events: none;
  background:
    radial-gradient(circle at center, rgba(0, 0, 0, 0) 0%, rgba(0, 0, 0, 0.6) 100%),
    rgba(0, 0, 0, 0.52);
`;

/**
 * 整页的石头背景，跟着鼠标轻微移动，上面压一层暗角。
 * 鼠标位置直接写进 CSS 变量，不经过 React 状态，移动鼠标不会触发重渲染。
 *
 * The page-wide stone backdrop, drifting slightly with the mouse under a vignette. The mouse
 * position goes straight into CSS variables rather than React state, so moving the mouse
 * never re-renders anything.
 */
export function Backdrop() {
  const stone = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onMove = (event: MouseEvent) => {
      const style = stone.current?.style;
      style?.setProperty("--mouse-x", ((event.clientX / window.innerWidth - 0.5) * 2).toFixed(3));
      style?.setProperty("--mouse-y", ((event.clientY / window.innerHeight - 0.5) * 2).toFixed(3));
    };
    window.addEventListener("mousemove", onMove, { passive: true });
    return () => window.removeEventListener("mousemove", onMove);
  }, []);

  return (
    <>
      <Stone ref={stone} />
      <Shade />
    </>
  );
}

// ---- 钢板：导航栏和页脚共用 / The steel plate shared by the navigation bar and the footer ----

const steelPlate = (texture: string) => css`
  position: relative;
  background: ${({ theme }) => theme.color.steel};

  /* 顶部高光加底部阴影，做出钢板的厚度。 / A top highlight and a bottom shadow give the plate its thickness. */
  &::before {
    content: "";
    position: absolute;
    inset: 0;
    pointer-events: none;
    box-shadow:
      inset 0 4px 0 0 rgba(255, 255, 255, 0.1),
      inset 0 -4px 0 0 rgba(0, 0, 0, 0.4);
  }

  /* 极淡的一层材质噪点。 / A very faint layer of texture grain. */
  &::after {
    content: "";
    position: absolute;
    inset: 0;
    pointer-events: none;
    opacity: 0.12;
    background-image: var(${texture});
    background-size: 32px 32px;
    image-rendering: pixelated;
    mix-blend-mode: luminosity;
  }
`;

const Rivet = styled.span<{ $corner: "tl" | "tr" | "bl" | "br" }>`
  position: absolute;
  z-index: 1;
  width: 8px;
  height: 8px;
  background: #1a1b1e;
  border-bottom: 1px solid #444444;
  border-right: 1px solid #444444;
  ${({ $corner }) => ($corner.startsWith("t") ? "top: 8px;" : "bottom: 8px;")}
  ${({ $corner }) => ($corner.endsWith("l") ? "left: 8px;" : "right: 8px;")}
`;

function Rivets() {
  return (
    <>
      <Rivet $corner="tl" />
      <Rivet $corner="tr" />
      <Rivet $corner="bl" />
      <Rivet $corner="br" />
    </>
  );
}

// ---- 导航栏 / Navigation bar ----

const NavPlate = styled.nav`
  ${steelPlate("--tex-dirt")}
  position: sticky;
  top: 0;
  z-index: 30;
  height: ${({ theme }) => theme.layout.navHeight};
  border-bottom: 4px solid ${({ theme }) => theme.color.steelEdge};
`;

const NavContent = styled.div`
  position: relative;
  z-index: 2;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  height: 100%;
  padding: 0 28px;

  @media (max-width: 640px) {
    padding: 0 20px;
  }
`;

const Brand = styled.a`
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: ${({ theme }) => theme.font.heading};
  line-height: 1;
  letter-spacing: 3px;
  color: #ffffff;
  text-shadow:
    3px 3px 0 #000000,
    0 0 8px rgba(94, 172, 68, 0.35);

  img {
    width: 40px;
    height: 40px;
    filter: drop-shadow(2px 4px 0 rgba(0, 0, 0, 0.5));
    transition: transform 0.15s ease-out;
  }

  &:hover img {
    transform: rotate(-12deg) scale(1.1);
  }

  @media (max-width: 640px) {
    font-size: ${({ theme }) => theme.font.lead};
    letter-spacing: 2px;

    img {
      width: 32px;
      height: 32px;
    }
  }
`;

const NavLinks = styled.div`
  display: flex;
  align-items: center;
  gap: 22px;
  margin-left: auto;
  margin-right: 12px;

  @media (max-width: 1020px) {
    display: none;
  }
`;

const NavAnchor = styled.a`
  color: ${({ theme }) => theme.color.panel};
  ${textShadow()}

  &:hover {
    color: #ffffff;
    box-shadow: 0 4px 0 0 ${({ theme }) => theme.color.bamboo.light};
  }
`;

const NavActions = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
`;

export const NAV_SECTIONS: readonly { readonly id: string; readonly label: Text }[] = [
  { id: "progress", label: { zh: "进度", en: "Progress" } },
  { id: "features", label: { zh: "玩法", en: "Features" } },
  { id: "architecture", label: { zh: "架构", en: "Architecture" } },
  { id: "performance", label: { zh: "测量", en: "Measurements" } },
  { id: "verification", label: { zh: "验证", en: "Verification" } },
];

export function NavBar() {
  const { locale, toggle, t } = useLocale();
  return (
    <NavPlate aria-label={t({ zh: "页面导航", en: "Page navigation" })}>
      <Rivets />
      <NavContent>
        <Brand href="#top">
          <img src={iconUrl} alt="" />
          Bamboo
        </Brand>
        <NavLinks>
          {NAV_SECTIONS.map((section) => (
            <NavAnchor key={section.id} href={`#${section.id}`}>
              {t(section.label)}
            </NavAnchor>
          ))}
        </NavLinks>
        <NavActions>
          <PixelButton type="button" onClick={toggle} aria-label={t({ zh: "切换到英文", en: "Switch to Chinese" })}>
            {locale === "zh" ? "EN" : "中文"}
          </PixelButton>
          <PixelLink $tone="bamboo" href={REPO_URL} target="_blank" rel="noopener noreferrer">
            GitHub
          </PixelLink>
        </NavActions>
      </NavContent>
    </NavPlate>
  );
}

// ---- 页脚 / Footer ----

const FooterPlate = styled.footer`
  ${steelPlate("--tex-sand")}
  z-index: 1;
  margin-top: 96px;
  border-top: 4px solid ${({ theme }) => theme.color.steelEdge};
`;

const FooterContent = styled.div`
  position: relative;
  z-index: 2;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  max-width: ${({ theme }) => theme.layout.maxWidth};
  margin: 0 auto;
  padding: 28px 32px;
  text-align: center;
  color: ${({ theme }) => theme.color.textMuted};
  ${textShadow()}

  a {
    color: ${({ theme }) => theme.color.panel};
    box-shadow: 0 2px 0 0 ${({ theme }) => theme.color.bamboo.light};
  }

  a:hover {
    color: #ffffff;
  }
`;

export function Footer() {
  const { t } = useLocale();
  return (
    <FooterPlate>
      <Rivets />
      <FooterContent>
        <p>
          <a href={REPO_URL} target="_blank" rel="noopener noreferrer">
            Bamboo
          </a>
          {t({ zh: " · 以 AGPL-3.0 许可证开源", en: " · open source under AGPL-3.0" })}
        </p>
        <p>
          {t({
            zh: "Bamboo 不是 Minecraft 官方产品，未获 Mojang 或 Microsoft 批准，也与它们没有关联。",
            en: "Bamboo is not an official Minecraft product. It is not approved by or associated with Mojang or Microsoft.",
          })}
        </p>
        <p>
          {t({
            zh: "页面上的方块贴图全部由代码生成。字体是 Fusion Pixel（OFL-1.1）。",
            en: "Every block texture on this page is generated by code. The typeface is Fusion Pixel (OFL-1.1).",
          })}
        </p>
      </FooterContent>
    </FooterPlate>
  );
}
