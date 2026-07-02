import "@fontsource-variable/inter/index.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./i18n";
import { ThemeProvider } from "./lib/theme";
import "./styles/globals.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </React.StrictMode>,
);
