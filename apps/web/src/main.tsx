import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { installTextures } from "./theme/textures";

installTextures();

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html is missing the #root node");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
