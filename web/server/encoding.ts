/** Conversions between the encodings the enclave and the contracts use. */

import { baseDecode, baseEncode } from "near-api-js";

export function hexToBase58(hex: string): string {
  return baseEncode(Buffer.from(hex, "hex"));
}

export function base58ToHex(value: string): string {
  return Buffer.from(baseDecode(value)).toString("hex");
}

export function hexToBase64(hex: string): string {
  return Buffer.from(hex, "hex").toString("base64");
}

/** A NEAR public key string from 32 raw bytes in hex. */
export function ed25519Key(hex: string): string {
  return `ed25519:${hexToBase58(hex)}`;
}

export function isHex(value: unknown, bytes: number): value is string {
  return typeof value === "string" && new RegExp(`^[0-9a-f]{${bytes * 2}}$`).test(value);
}
