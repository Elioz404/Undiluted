import type { Event } from "./types.ts";

const MICROS = 1_000_000n;

/** Micro units of a stablecoin as dollars, e.g. "1993690" to "1.99". */
export function dollars(micros: string | null | undefined): string {
  if (!micros) {
    return "-";
  }
  const value = BigInt(micros);
  const cents = (value * 100n + MICROS / 2n) / MICROS;
  return `$${cents / 100n}.${(cents % 100n).toString().padStart(2, "0")}`;
}

export function toMicros(dollarsText: string): string {
  const match = /^\s*(\d+)(?:\.(\d{1,6}))?\s*$/.exec(dollarsText);
  if (!match) {
    throw new Error("enter an amount like 2 or 2.50");
  }
  const fraction = (match[2] ?? "").padEnd(6, "0");
  return (BigInt(match[1]!) * MICROS + BigInt(fraction)).toString();
}

export function percent(basisPoints: string | null): string {
  if (basisPoints === null) {
    return "set at the next priced round";
  }
  const value = Number(basisPoints);
  return `${(value / 100).toFixed(2)}%`;
}

export function shortHex(hex: string): string {
  return `${hex.slice(0, 10)}…${hex.slice(-6)}`;
}

/** What a signer is being asked to approve, in plain words. */
export function describe(event: Event): string {
  switch (event.kind) {
    case "treasury_designated":
      return `Investments must arrive at ${event.account}, in ${event.token}`;
    case "holder_registered":
      return `${event.holder} joins the cap table with key ${shortHex(event.key)}`;
    case "common_issued":
      return `${event.holder} receives ${Number(event.shares).toLocaleString()} common shares (${event.instrument})`;
    case "safe_issued": {
      const terms = [
        event.valuation_cap_micros ? `${dollars(event.valuation_cap_micros)} post-money cap` : "no cap",
        event.discount_basis_points ? `${event.discount_basis_points / 100}% discount` : null,
        event.mfn ? "MFN clause" : null,
      ].filter(Boolean);
      return `${event.holder} receives a ${dollars(event.purchase_micros)} SAFE (${terms.join(", ")}), paid in ${shortHex(event.settlement)}`;
    }
    case "mfn_elected":
      return `${event.target} adopts the terms of ${event.source}`;
    case "payout_destination_set":
      return `${event.holder} wants exit proceeds sent to ${event.destination}`;
    case "exit_approved":
      return `The company is sold for ${dollars(event.proceeds_micros)}`;
    case "other":
      return event.description;
  }
}
