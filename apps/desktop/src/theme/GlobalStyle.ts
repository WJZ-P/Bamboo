import { createGlobalStyle } from "styled-components";

import fusionPixelUrl from "../assets/fonts/fusion-pixel-12px-proportional-zh_hans.otf.woff2?url";

export const GlobalStyle = createGlobalStyle`
  @font-face {
    font-family: "Fusion Pixel";
    src: url(${fusionPixelUrl}) format("woff2");
    font-display: block;
  }

  *, *::before, *::after {
    box-sizing: border-box;
  }

  html, body, #root {
    height: 100%;
    margin: 0;
  }

  body {
    overflow: hidden;
    background: ${({ theme }) => theme.color.page};
    color: ${({ theme }) => theme.color.text};
    font-family: ${({ theme }) => theme.font.family};
    font-size: ${({ theme }) => theme.font.body};
    line-height: 1.5;
    image-rendering: pixelated;
    -webkit-font-smoothing: none;
    user-select: none;
  }

  button, input {
    font: inherit;
    color: inherit;
  }

  img {
    image-rendering: pixelated;
  }

  ::-webkit-scrollbar {
    width: 12px;
    height: 12px;
  }

  ::-webkit-scrollbar-track {
    background: ${({ theme }) => theme.color.sunkenShade};
  }

  ::-webkit-scrollbar-thumb {
    background: ${({ theme }) => theme.color.stone.light};
    box-shadow:
      inset 2px 2px 0 0 ${({ theme }) => theme.color.panel},
      inset -2px -2px 0 0 ${({ theme }) => theme.color.stone.dark};
  }
`;
