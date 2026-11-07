import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { Company } from "./pages/Company.tsx";
import { Holder } from "./pages/Holder.tsx";
import { Landing } from "./pages/Landing.tsx";
import { useRelayAt } from "./lib/relay.ts";
import { useRoute, type Route } from "./router.ts";
import "./styles.css";

useRelayAt(import.meta.env.VITE_RELAY_URL ?? "");

/** Where you are: the company, then whose view this is. */
function Crumbs({ route }: { route: Route }) {
  if (route.page === "landing") {
    return null;
  }
  return (
    <nav className="crumbs" aria-label="Breadcrumb">
      <span className="id optional">{route.company}</span>
      <span className="sep optional" aria-hidden>
        /
      </span>
      <span aria-current="page">{route.page === "company" ? "Company" : `Investor ${route.holder}`}</span>
    </nav>
  );
}

function App() {
  const route = useRoute();
  return (
    <>
      <header className="topbar">
        <div className="topbar-inner">
          <a href="#/" className="wordmark" aria-label="Undiluted, home">
            Undiluted
          </a>
          <Crumbs route={route} />
          <span className="network">NEAR testnet</span>
        </div>
      </header>
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
