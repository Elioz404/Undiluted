/** The JSON shapes the browser protocol speaks. They mirror crates/web/src/dto.rs. */

export type Actor = { role: "company" } | { role: "holder"; holder: string };

export type Event =
  | { kind: "treasury_designated"; account: string; token: string }
  | { kind: "holder_registered"; holder: string; key: string }
  | { kind: "common_issued"; instrument: string; holder: string; shares: string }
  | {
      kind: "safe_issued";
      instrument: string;
      holder: string;
      purchase_micros: string;
      valuation_cap_micros: string | null;
      discount_basis_points: number | null;
      mfn: boolean;
      settlement: string;
    }
  | { kind: "mfn_elected"; target: string; source: string }
  | { kind: "payout_destination_set"; holder: string; destination: string }
  | { kind: "exit_approved"; proceeds_micros: string }
  | { kind: "other"; description: string };

export type Command =
  | { kind: "create_company" }
  | { kind: "propose"; event: Event }
  | { kind: "sign"; digest: string }
  | { kind: "cancel_proposal" }
  | { kind: "get_proposal" }
  | { kind: "get_cap_table" }
  | { kind: "get_statement" }
  | { kind: "get_payout_plan" };

export interface Line {
  instrument: string;
  holder: string;
  shares: string | null;
  ownership_basis_points: string | null;
}

export interface Offer {
  target: string;
  source: string;
  valuation_cap_micros: string | null;
  discount_basis_points: number | null;
}

export interface Payout {
  instrument: string;
  holder: string;
  amount_micros: string;
  took_preference: boolean;
}

export interface Proposal {
  digest: string;
  sequence: number;
  event: Event;
  company_signature_missing: boolean;
  holder_signatures_missing: string[];
}

export interface Statement {
  head: string;
  entries: number;
  capitalization: string;
  reconciles: boolean;
  lines: Line[];
  mfn_offers: Offer[];
  exit: { proceeds_micros: string; total_matches_proceeds: boolean; payouts: Payout[] } | null;
}

export interface Instruction {
  index: number;
  recipient: string;
  amount_micros: string;
  signature: string;
}

export type Response =
  | { kind: "company_created"; head: string }
  | ({ kind: "proposal" } & Proposal)
  | { kind: "committed"; sequence: number; head: string }
  | { kind: "cancelled" }
  | {
      kind: "cap_table";
      head: string;
      entries: number;
      holders: string[];
      capitalization: string;
      option_pool: string;
      lines: Line[];
    }
  | ({ kind: "statement" } & Statement)
  | { kind: "payout_plan"; proceeds_micros: string; instructions: Instruction[] }
  | { kind: "rejected"; reason: string };

export interface Anchor {
  company: string;
  sequence: number;
  head: string;
  signature: string;
}

export type EnclaveOutput =
  | { status: "identity"; encryption_key: string; signing_key: string }
  | { status: "reply"; envelope: string; anchor: Anchor | null }
  | { status: "error"; message: string };

export interface RelayConfig {
  network: string;
  treasury: string;
  token: string;
  anchorContract: string;
  distributionContract: string;
  maxPaymentMicros: string;
  maxExitMicros: string;
}

export interface Settlement {
  reference: string;
  receivedMicros: string;
}
