import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { Company } from "./pages/Company.tsx";
import { Holder } from "./pages/Holder.tsx";
import { Landing } from "./pages/Landing.tsx";
import { useRoute } from "./router.ts";
import "./styles.css";

function App() {
  const route = useRoute();
  return (
    <>
      <nav className="nav">
        <a href="#/" className="brand">
          Undiluted
        </a>
        <span className="muted">NEAR testnet</span>
      </nav>
      {route.page === "company" && <Company key={route.company} company={route.company} />}
      {route.page === "holder" && <Holder key={`${route.company}/${route.holder}`} company={route.company} holder={route.holder} />}
      {route.page === "landing" && <Landing />}
    </>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
