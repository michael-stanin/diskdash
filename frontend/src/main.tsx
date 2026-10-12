import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "react-router";
import { Providers } from "@/app/Providers";
import { createRouter } from "@/app/router";
import "./index.css";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

createRoot(root).render(
  <StrictMode>
    <Providers>
      <RouterProvider router={createRouter()} />
    </Providers>
  </StrictMode>,
);
