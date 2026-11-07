import assert from "node:assert/strict";
import { test } from "node:test";

import { contentSecurityPolicy, crossOrigin } from "./security.ts";

test("the policy allows scripts only from the site itself", () => {
  const policy = contentSecurityPolicy("https://rpc.testnet.fastnear.com/some/path");
  assert.match(policy, /script-src 'self' 'wasm-unsafe-eval';/);
  assert.doesNotMatch(policy, /unsafe-inline/);
  assert.match(policy, /connect-src 'self' https:\/\/rpc\.testnet\.fastnear\.com;/);
});

function answer(origin: string | undefined, method = "GET") {
  const headers: Record<string, string> = {};
  let status = 200;
  let passed = false;
  const response = {
    setHeader: (name: string, value: string) => {
      headers[name] = value;
    },
    status: (code: number) => {
      status = code;
      return { end: () => undefined };
    },
  };
  crossOrigin(["https://site.example"])(
    { headers: origin ? { origin } : {}, method } as never,
    response as never,
    () => {
      passed = true;
    },
  );
  return { headers, status, passed };
}

test("only listed origins may read the relay's answers", () => {
  assert.equal(answer("https://site.example").headers["Access-Control-Allow-Origin"], "https://site.example");
  assert.equal(answer("https://evil.example").headers["Access-Control-Allow-Origin"], undefined);
  assert.equal(answer(undefined).passed, true);
});

test("a preflight is answered without reaching the routes", () => {
  const preflight = answer("https://site.example", "OPTIONS");
  assert.equal(preflight.status, 204);
  assert.equal(preflight.passed, false);
});
