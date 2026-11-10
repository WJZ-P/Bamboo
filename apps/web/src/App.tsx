import styled, { ThemeProvider } from "styled-components";

import { Backdrop, Footer, NavBar } from "./components/layout";
import { LocaleProvider } from "./i18n";
import { Architecture } from "./sections/Architecture";
import { Features } from "./sections/Features";
import { Hero } from "./sections/Hero";
import { Performance } from "./sections/Performance";
import { Progress } from "./sections/Progress";
import { Verification } from "./sections/Verification";
import { GlobalStyle } from "./theme/GlobalStyle";
import { theme } from "./theme/theme";

const Page = styled.div`
  position: relative;
  display: flex;
  flex-direction: column;
  min-height: 100vh;
`;

const Main = styled.main`
  position: relative;
  z-index: 1;
`;

export function App() {
  return (
    <ThemeProvider theme={theme}>
      <GlobalStyle />
      <LocaleProvider>
        <Page>
          <Backdrop />
          <NavBar />
          <Hero />
          <Main>
            <Progress />
            <Features />
            <Architecture />
            <Performance />
            <Verification />
          </Main>
          <Footer />
        </Page>
      </LocaleProvider>
    </ThemeProvider>
  );
}
