/**
 * Signs and sends transactions as the relay account.
 *
 * A transaction's hash is known as soon as it is signed. If the RPC times out,
 * the relay looks that same transaction up instead of sending a new one, so a
 * slow network can never make it pay or act twice.
 *
 * Two ways to sign: with a key held by the relay (for a hosted relay), or
 * through the `near` command-line tool, which keeps the key in the system
 * keychain (for running locally without copying any key).
 */

import { spawn } from "node:child_process";

import {
  Account,
  JsonRpcProvider,
  KeyPairSigner,
  actions,
  baseEncode,
  type KeyPairString,
} from "near-api-js";

import { isTimeout, readOutcome, type Outcome } from "./outcome.ts";

export class Refused extends Error {
  override readonly name = "Refused";
}

export interface ContractCall {
  contract: string;
  method: string;
  args: unknown;
  gas: bigint;
  deposit: bigint;
}

export interface Settling {
  checks: number;
  intervalMs: number;
}

/** Sends a signed call. Returns its hash, and its final result if it has one. */
export interface Submitter {
  submit(call: ContractCall): Promise<{ hash: string; response: unknown | null }>;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const YOCTO_PER_NEAR = 10n ** 24n;

export class KeySubmitter implements Submitter {
  private readonly signer: KeyPairSigner;

  constructor(
    private readonly provider: JsonRpcProvider,
    private readonly accountId: string,
    privateKey: string,
  ) {
    this.signer = KeyPairSigner.fromSecretKey(privateKey as KeyPairString);
  }

  async submit(call: ContractCall) {
    const account = new Account(this.accountId, this.provider, this.signer);
    const transaction = await account.createTransaction({
      receiverId: call.contract,
      actions: [actions.functionCall(call.method, call.args as object, call.gas, call.deposit)],
      publicKey: await this.signer.getPublicKey(),
    });
    const { txHash, signedTransaction } = await this.signer.signTransaction(transaction);
    const hash = baseEncode(txHash);
    try {
      return { hash, response: await this.provider.sendTransactionUntil(signedTransaction, "FINAL") };
    } catch (error) {
      if (!isTimeout(error)) {
        throw error;
      }
      return { hash, response: null };
    }
  }
}

/** Formats yoctoNEAR the way the `near` tool expects, without losing precision. */
export function nearAmount(yocto: bigint): string {
  const whole = yocto / YOCTO_PER_NEAR;
  const fraction = (yocto % YOCTO_PER_NEAR).toString().padStart(24, "0").replace(/0+$/, "");
  return `${whole}${fraction ? `.${fraction}` : ""} NEAR`;
}

/** The hash the `near` tool printed, which it does even when it gave up waiting. */
export function printedHash(output: string): string | null {
  return /Transaction ID:\s*([1-9A-HJ-NP-Za-km-z]{43,44})/.exec(output)?.[1] ?? null;
}

export class CliSubmitter implements Submitter {
  constructor(
    private readonly cli: string,
    private readonly accountId: string,
    private readonly network: string,
  ) {}

  submit(call: ContractCall) {
    const args = [
      "contract", "call-function", "as-transaction", call.contract, call.method,
      "json-args", JSON.stringify(call.args),
      "prepaid-gas", `${call.gas / 1_000_000_000_000n} Tgas`,
      "attached-deposit", nearAmount(call.deposit),
      "sign-as", this.accountId,
      "network-config", this.network,
      "sign-with-keychain", "send",
    ];
    return new Promise<{ hash: string; response: null }>((resolve, reject) => {
      const child = spawn(this.cli, args, { stdio: ["ignore", "pipe", "pipe"] });
      let output = "";
      child.stdout.on("data", (chunk) => (output += chunk));
      child.stderr.on("data", (chunk) => (output += chunk));
      child.on("error", reject);
      child.on("close", () => {
        const hash = printedHash(output);
        if (hash) {
          resolve({ hash, response: null });
        } else {
          reject(new Error(`the near tool sent nothing: ${output.slice(-400)}`));
        }
      });
    });
  }
}

export class Near {
  /** One transaction at a time, because they share the key's nonce. */
  private queue: Promise<unknown> = Promise.resolve();

  constructor(
    private readonly provider: JsonRpcProvider,
    readonly accountId: string,
    private readonly submitter: Submitter,
    private readonly settling: Settling,
  ) {}

  async view<T>(contract: string, method: string, args: unknown): Promise<T> {
    return (await this.provider.callFunction({
      contractId: contract,
      method,
      args: args as Record<string, unknown>,
    })) as T;
  }

  /** Sends a call and waits for its final outcome. Throws `Refused` if a contract said no. */
  call(call: ContractCall): Promise<Outcome> {
    const next = this.queue.then(() => this.send(call));
    this.queue = next.catch(() => undefined);
    return next;
  }

  private async send(call: ContractCall): Promise<Outcome> {
    const { hash, response } = await this.submitter.submit(call);
    const outcome = readOutcome(hash, response ?? (await this.waitFor(hash)));
    if (outcome.refusal !== null) {
      throw new Refused(outcome.refusal);
    }
    if (!outcome.succeeded) {
      throw new Error(`transaction ${hash} failed`);
    }
    return outcome;
  }

  private async waitFor(hash: string): Promise<unknown> {
    for (let check = 0; check < this.settling.checks; check += 1) {
      try {
        return await this.provider.viewTransactionStatus({
          txHash: hash,
          accountId: this.accountId,
          waitUntil: "FINAL",
        } as Parameters<JsonRpcProvider["viewTransactionStatus"]>[0]);
      } catch (error) {
        if (!isTimeout(error)) {
          throw error;
        }
      }
      await sleep(this.settling.intervalMs);
    }
    throw new Error(`transaction ${hash} did not settle in time`);
  }
}
