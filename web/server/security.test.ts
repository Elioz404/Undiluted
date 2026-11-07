import assert from "node:assert/strict";
import { test } from "node:test";

import { contentSecurityPolicy } from "./security.ts";

test("the policy allows scripts only from the site itself", () => {
  const policy = contentSecurityPolicy("https://rpc.testnet.fastnear.com/some/path");
  assert.match(policy, /script-src 'self' 'wasm-unsafe-eval';/);
  assert.doesNotMatch(policy, /unsafe-inline/);
  assert.match(policy, /connect-src 'self' https:\/\/rpc\.testnet\.fastnear\.com;/);
});
