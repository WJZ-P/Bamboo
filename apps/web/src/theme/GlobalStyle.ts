import { createGlobalStyle } from "styled-components";

import fusionPixelUrl from "../assets/fonts/fusion-pixel-12px-proportional-zh_hans.otf.woff2?url";

export const GlobalStyle = createGlobalStyle`
  @font-face {
    font-family: "Fusion Pixel";
    src: url(${fusionPixelUrl}) format("woff2");
    font-display: swap;
  }

  *, *::before, *::after {
    box-sizing: border-box;
  }

  html {
    scroll-behavior: smooth;
    scroll-padding-top: calc(${({ theme }) => theme.layout.navHeight} + 16px);
  }

  @media (prefers-reduced-motion: reduce) {
    html {
      scroll-behavior: auto;
    }
  }

  body {
    margin: 0;
    min-width: 320px;
    background: ${({ theme }) => theme.color.page};
    color: ${({ theme }) => theme.color.text};
    font-family: ${({ theme }) => theme.font.family};
    font-size: ${({ theme }) => theme.font.body};
    line-height: 1.6;
    -webkit-font-smoothing: none;
    text-rendering: optimizeSpeed;
    overflow-x: hidden;
  }

  h1, h2, h3, h4, p, ul, ol, figure {
    margin: 0;
    padding: 0;
    font-weight: normal;
  }

  ul, ol {
    list-style: none;
  }

  a {
    color: inherit;
    text-decoration: none;
  }

  button {
    font: inherit;
    color: inherit;
  }

  img, canvas {
    image-rendering: pixelated;
  }

  ::selection {
    background: ${({ theme }) => theme.color.bamboo.light};
    color: ${({ theme }) => theme.color.black};
    text-shadow: none;
  }

  :focus-visible {
    outline: 3px solid ${({ theme }) => theme.color.gold};
    outline-offset: 2px;
  }

  ::-webkit-scrollbar {
    width: 14px;
    height: 14px;
  }

  ::-webkit-scrollbar-track {
    background: ${({ theme }) => theme.color.sunken};
  }

  ::-webkit-scrollbar-thumb {
    background: ${({ theme }) => theme.color.stone.light};
    box-shadow:
      inset 2px 2px 0 0 ${({ theme }) => theme.color.panel},
      inset -2px -2px 0 0 ${({ theme }) => theme.color.stone.dark};
  }
`;
