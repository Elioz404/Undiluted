import assert from "node:assert/strict";
import { test } from "node:test";

import { base58ToHex, ed25519Key, hexToBase58, hexToBase64, isHex } from "./encoding.ts";

const HEAD = "3b7fcb8c5435dd4f9cde9dbec535dcc2211325f42808d4d95d0530d8d3ca1612";

test("a head converts to the base58 the contract stores, and back", () => {
  // Read from the anchor contract on testnet for this exact head.
  assert.equal(hexToBase58(HEAD), "51G5TPkpayAKTvJiYN75z6QaE48m8fA4AaXMLFUK7p7j");
  assert.equal(base58ToHex(hexToBase58(HEAD)), HEAD);
});

test("an enclave key becomes a NEAR public key", () => {
  // The enclave's signing key as the contracts record it on testnet.
  assert.equal(
    ed25519Key("4385ac1675dfd32bf15f994b0bd1effead92649d12ad7422b7ab003ef97f6ab4"),
    "ed25519:5YaY6Zm6oX7UdcRuC3Jwa3AdEkekoWQaoYmcnJdGEcCK",
  );
});

test("hex checks length and characters", () => {
  assert.equal(isHex(HEAD, 32), true);
  assert.equal(isHex(HEAD.toUpperCase(), 32), false);
  assert.equal(isHex(HEAD.slice(2), 32), false);
  assert.equal(isHex(42, 32), false);
});

test("signatures convert to base64", () => {
  assert.equal(hexToBase64("cdcd"), "zc0=");
});
