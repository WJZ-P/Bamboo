import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { startLogStream } from "./state/logStore";
import { startMetricsStream } from "./state/metricsStore";
import { installStoneTexture } from "./theme/stoneTexture";

installStoneTexture();
void startLogStream();
void startMetricsStream();

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html 里缺少 #root 节点");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
