import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./globals.css";
// PRAGMATIC: legacy.css holds pre-tailwind class styles (.dash-*, .lp-*, .pa-*)
// referenced by className in feature components. Slated for removal once the
// component tree migrates to tailwind. Until then it must ship with the bundle.
import "./legacy.css";
import { bootstrap } from "./orchestration/bootstrap";
import App from "./orchestration/app";

// Populate AppConfig before any feature code reaches for appConfig().
bootstrap();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
