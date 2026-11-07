/**
 * The relay's on-chain work: publishing heads, test payments and exit payouts.
 * Each of these is safe to leave to an untrusted relay, because the contracts
 * accept only what the enclave signed and the enclave checks payments itself.
 */

import { ed25519Key, hexToBase58, hexToBase64, base58ToHex } from "./encoding.ts";
import type { Near } from "./near.ts";

const CALL_GAS = 30_000_000_000_000n;
const PAY_GAS = 60_000_000_000_000n;
const FUND_GAS = 100_000_000_000_000n;
const ONE_YOCTO = 1n;
/** Attached for contract storage. Whatever is not used comes back. */
const STORAGE_DEPOSIT = 10_000_000_000_000_000_000_000n;
/** Registers an account with a NEP-141 token. */
const TOKEN_REGISTRATION = 1_250_000_000_000_000_000_000n;

export interface AnchorOutput {
  company: string;
  sequence: number;
  head: string;
  signature: string;
}

export interface PayoutInstruction {
  index: number;
  recipient: string;
  amount_micros: string;
  signature: string;
}

export class Anchors {
  constructor(
    private readonly near: Near,
    private readonly contract: string,
  ) {}

  async register(company: string, enclaveKeyHex: string): Promise<void> {
    await this.near.call({
      contract: this.contract,
      method: "register_company",
      args: { company_id: company, enclave_key: ed25519Key(enclaveKeyHex) },
      gas: CALL_GAS,
      deposit: STORAGE_DEPOSIT,
    });
  }

  async publish(anchor: AnchorOutput): Promise<void> {
    await this.near.call({
      contract: this.contract,
      method: "anchor",
      args: {
        company_id: anchor.company,
        sequence: anchor.sequence,
        head: hexToBase58(anchor.head),
        signature: hexToBase64(anchor.signature),
      },
      gas: CALL_GAS,
      deposit: STORAGE_DEPOSIT,
    });
  }

  /** The published head for `sequence`, as hex, or null. */
  async headAt(company: string, sequence: number): Promise<string | null> {
    const head = await this.near.view<string | null>(this.contract, "head_at", {
      company_id: company,
      sequence,
    });
    return head ? base58ToHex(head) : null;
  }
}

export class Tokens {
  constructor(private readonly near: Near) {}

  /** Makes `account` able to receive `token`. Harmless if it already can. */
  async register(token: string, account: string): Promise<void> {
    await this.near.call({
      contract: token,
      method: "storage_deposit",
      args: { account_id: account, registration_only: true },
      gas: CALL_GAS,
      deposit: TOKEN_REGISTRATION,
    });
  }

  /** Sends `micros` of `token` and returns the hash of the final transaction. */
  async transfer(token: string, receiver: string, micros: bigint, memo?: string): Promise<string> {
    const outcome = await this.near.call({
      contract: token,
      method: "ft_transfer",
      args: { receiver_id: receiver, amount: micros.toString(), ...(memo ? { memo } : {}) },
      gas: CALL_GAS,
      deposit: ONE_YOCTO,
    });
    return outcome.hash;
  }

  async transferCall(token: string, receiver: string, micros: bigint, msg: string): Promise<void> {
    await this.near.call({
      contract: token,
      method: "ft_transfer_call",
      args: { receiver_id: receiver, amount: micros.toString(), msg },
      gas: FUND_GAS,
      deposit: ONE_YOCTO,
    });
  }
}

export class Payouts {
  constructor(
    private readonly near: Near,
    private readonly tokens: Tokens,
    private readonly contract: string,
    private readonly token: string,
  ) {}

  async open(company: string, enclaveKeyHex: string, totalMicros: bigint): Promise<void> {
    await this.tokens.register(this.token, this.contract);
    await this.near.call({
      contract: this.contract,
      method: "open",
      args: {
        company_id: company,
        token: this.token,
        enclave_key: ed25519Key(enclaveKeyHex),
        total: totalMicros.toString(),
      },
      gas: CALL_GAS,
      deposit: STORAGE_DEPOSIT,
    });
  }

  async fund(company: string, micros: bigint): Promise<void> {
    await this.tokens.transferCall(this.token, this.contract, micros, company);
  }

  async pay(company: string, instruction: PayoutInstruction): Promise<void> {
    await this.tokens.register(this.token, instruction.recipient);
    await this.near.call({
      contract: this.contract,
      method: "pay",
      args: {
        company_id: company,
        index: instruction.index,
        recipient: instruction.recipient,
        amount: instruction.amount_micros,
        signature: instruction.signature,
      },
      gas: PAY_GAS,
      deposit: STORAGE_DEPOSIT,
    });
  }
}
