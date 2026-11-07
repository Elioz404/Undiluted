/** Base58 as NEAR uses it, for hashes and keys read from the chain. */

const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE = 58n;

export function base58ToHex(value: string): string {
  let number = 0n;
  for (const character of value) {
    const digit = ALPHABET.indexOf(character);
    if (digit < 0) {
      throw new Error(`not base58: ${value}`);
    }
    number = number * BASE + BigInt(digit);
  }
  const body = number === 0n ? "" : number.toString(16);
  const leadingZeros = value.length - value.replace(/^1+/, "").length;
  return "00".repeat(leadingZeros) + (body.length % 2 ? `0${body}` : body);
}
