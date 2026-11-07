import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv, type Plugin } from "vite";

const FONT_STYLES = "https://api.fontshare.com https://fonts.googleapis.com";
const FONT_FILES = "https://cdn.fontshare.com https://fonts.gstatic.com";

const origin = (url: string | undefined) => (url ? new URL(url).origin : "");

/**
 * Writes the content security policy into the built page, so it holds wherever
 * the page is served, a static host included. Development keeps Vite's own
 * inline scripts working, so the policy is added to builds only.
 */
function contentSecurityPolicy(env: Record<string, string>): Plugin {
  const connect = ["'self'", origin(env.VITE_NEAR_RPC_URL), origin(env.VITE_RELAY_URL)].filter(Boolean);
  const policy = [
    "default-src 'self'",
    "script-src 'self' 'wasm-unsafe-eval'",
    `style-src 'self' ${FONT_STYLES}`,
    `font-src ${FONT_FILES}`,
    "img-src 'self' data:",
    `connect-src ${connect.join(" ")}`,
    "base-uri 'none'",
    "form-action 'none'",
    "object-src 'none'",
  ].join("; ");
  return {
    name: "content-security-policy",
    apply: "build",
    transformIndexHtml: (html) =>
      html.replace("<head>", `<head>\n    <meta http-equiv="Content-Security-Policy" content="${policy}" />`),
  };
}

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "VITE_");
  return {
    // Relative paths, so the same build works at a domain's root or under a path.
    base: "./",
    plugins: [react(), contentSecurityPolicy(env)],
    server: {
      proxy: { "/api": process.env.RELAY_URL ?? "http://localhost:8080" },
    },
  };
});
