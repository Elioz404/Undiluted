/**
 * Response headers for the site and the API. The policy allows only what the
 * page really loads: its own code and WebAssembly, the two font hosts, and the
 * NEAR RPC it reads heads from. Keys live in the browser, so keeping injected
 * scripts out matters more here than on most demo pages.
 */

import type { NextFunction, Request, Response } from "express";

const FONT_STYLES = "https://api.fontshare.com https://fonts.googleapis.com";
const FONT_FILES = "https://cdn.fontshare.com https://fonts.gstatic.com";

export function contentSecurityPolicy(rpcUrl: string): string {
  return [
    "default-src 'self'",
    "script-src 'self' 'wasm-unsafe-eval'",
    `style-src 'self' ${FONT_STYLES}`,
    `font-src ${FONT_FILES}`,
    "img-src 'self' data:",
    `connect-src 'self' ${new URL(rpcUrl).origin}`,
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
    "object-src 'none'",
  ].join("; ");
}

export function securityHeaders(rpcUrl: string) {
  const policy = contentSecurityPolicy(rpcUrl);
  return (_request: Request, response: Response, next: NextFunction) => {
    response.setHeader("Content-Security-Policy", policy);
    response.setHeader("X-Content-Type-Options", "nosniff");
    response.setHeader("Referrer-Policy", "no-referrer");
    response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
    response.setHeader("Permissions-Policy", "camera=(), microphone=(), geolocation=()");
    next();
  };
}

/**
 * Lets the site call the relay from other origins it is published on, such as
 * GitHub Pages. Only the listed origins get an answer they can read.
 */
export function crossOrigin(allowed: readonly string[]) {
  return (request: Request, response: Response, next: NextFunction) => {
    const origin = request.headers.origin;
    if (origin && allowed.includes(origin)) {
      response.setHeader("Access-Control-Allow-Origin", origin);
      response.setHeader("Vary", "Origin");
      response.setHeader("Access-Control-Allow-Methods", "GET, POST");
      response.setHeader("Access-Control-Allow-Headers", "content-type");
      response.setHeader("Access-Control-Max-Age", "600");
    }
    if (request.method === "OPTIONS") {
      response.status(204).end();
      return;
    }
    next();
  };
}
