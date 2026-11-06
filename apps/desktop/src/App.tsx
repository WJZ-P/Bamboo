import { useState } from "react";
import styled, { ThemeProvider } from "styled-components";

import { ErrorBanner } from "./components/ErrorBanner";
import { type PageId, Sidebar } from "./components/layout/Sidebar";
import { TitleBar } from "./components/layout/TitleBar";
import { ConsolePage } from "./pages/ConsolePage";
import { OverviewPage } from "./pages/OverviewPage";
import { PlayersPage } from "./pages/PlayersPage";
import { SettingsPage } from "./pages/SettingsPage";
import { ServerProvider } from "./state/ServerContext";
import { GlobalStyle } from "./theme/GlobalStyle";
import { theme } from "./theme/theme";

const Shell = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
  border: 2px solid ${({ theme }) => theme.color.black};
`;

/** 石头纹理背景，四周压暗形成暗角。 */
const Stage = styled.div`
  display: flex;
  flex: 1;
  min-height: 0;
  background:
    radial-gradient(circle at center, transparent 30%, rgba(0, 0, 0, 0.7) 100%),
    var(--stone-texture);
  background-size:
    auto,
    96px 96px;
`;

const Content = styled.main`
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
  padding: 16px;
  overflow: auto;
`;

const PAGE_VIEWS = {
  overview: OverviewPage,
  console: ConsolePage,
  players: PlayersPage,
  settings: SettingsPage,
};

export function App() {
  const [page, setPage] = useState<PageId>("overview");
  const PageView = PAGE_VIEWS[page];

  return (
    <ThemeProvider theme={theme}>
      <GlobalStyle />
      <ServerProvider>
        <Shell>
          <TitleBar />
          <Stage>
            <Sidebar page={page} onSelect={setPage} />
            <Content>
              <ErrorBanner />
              <PageView />
            </Content>
          </Stage>
        </Shell>
      </ServerProvider>
    </ThemeProvider>
  );
}
