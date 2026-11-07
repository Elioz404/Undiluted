import { useEffect, useState } from "react";

export type Route =
  | { page: "landing" }
  | { page: "company"; company: string }
  | { page: "holder"; company: string; holder: string };

export function parse(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean).map(decodeURIComponent);
  if (parts[0] === "company" && parts[1]) {
    return { page: "company", company: parts[1] };
  }
  if (parts[0] === "holder" && parts[1] && parts[2]) {
    return { page: "holder", company: parts[1], holder: parts[2] };
  }
  return { page: "landing" };
}

export function navigate(path: string): void {
  window.location.hash = path;
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parse(window.location.hash));
  useEffect(() => {
    const update = () => setRoute(parse(window.location.hash));
    window.addEventListener("hashchange", update);
    return () => window.removeEventListener("hashchange", update);
  }, []);
  return route;
}
