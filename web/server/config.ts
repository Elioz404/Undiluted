/**
 * Everything the relay needs, read once from the environment. Nothing about a
 * deployment is written in the code, so the same build runs on any network.
 */

export interface Config {
  port: number;
  rpcUrl: string;
  network: string;
  relayAccount: string;
  /** How the relay signs: with its own key, or through the local `near` tool. */
  signer: { kind: "key"; privateKey: string } | { kind: "near-cli"; path: string };
  outlayerContract: string;
  enclaveProject: string;
  /** OutLayer's API, where each run's attestation is published. */
  attestationApi: string;
  secretsAccount: string;
  secretsProfile: string;
  anchorContract: string;
  distributionContract: string;
  treasury: string;
  paymentApi: string;
  originAsset: string;
  destinationAsset: string;
  /** The largest test payment the relay will make on someone's behalf. */
  maxPaymentMicros: bigint;
  /** The largest exit the relay will fund from its own test balance. */
  maxExitMicros: bigint;
  /** What the relay spends on everyone's behalf in one day, at most. */
  dailyBudgetMicros: bigint;
  /** Other origins the site is published on, allowed to call the relay. */
  allowedOrigins: string[];
  /** Calls one address may make per minute, and spending calls per hour. */
  callsPerMinute: number;
  spendingCallsPerHour: number;
}

type Environment = Record<string, string | undefined>;

function required(env: Environment, name: string): string {
  const value = env[name]?.trim();
  if (!value) {
    throw new Error(`${name} is not set`);
  }
  return value;
}

function optional(env: Environment, name: string, fallback: string): string {
  return env[name]?.trim() || fallback;
}

function amount(env: Environment, name: string, fallback: string): bigint {
  const value = optional(env, name, fallback);
  if (!/^\d+$/.test(value)) {
    throw new Error(`${name} must be a whole number of micro units`);
  }
  return BigInt(value);
}

function count(env: Environment, name: string, fallback: string): number {
  const value = Number(optional(env, name, fallback));
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new Error(`${name} must be a whole number above zero`);
  }
  return value;
}

function signerFrom(env: Environment): Config["signer"] {
  const kind = optional(env, "RELAY_SIGNER", "key");
  if (kind === "key") {
    return { kind, privateKey: required(env, "RELAY_PRIVATE_KEY") };
  }
  if (kind === "near-cli") {
    return { kind, path: optional(env, "NEAR_CLI", "near") };
  }
  throw new Error("RELAY_SIGNER must be key or near-cli");
}

export function loadConfig(env: Environment): Config {
  return {
    port: Number(optional(env, "PORT", "8080")),
    rpcUrl: required(env, "NEAR_RPC_URL"),
    network: required(env, "NEAR_NETWORK"),
    relayAccount: required(env, "RELAY_ACCOUNT"),
    signer: signerFrom(env),
    outlayerContract: required(env, "OUTLAYER_CONTRACT"),
    enclaveProject: required(env, "ENCLAVE_PROJECT"),
    attestationApi: required(env, "OUTLAYER_API_URL"),
    secretsAccount: required(env, "SECRETS_ACCOUNT"),
    secretsProfile: optional(env, "SECRETS_PROFILE", "default"),
    anchorContract: required(env, "ANCHOR_CONTRACT"),
    distributionContract: required(env, "DISTRIBUTION_CONTRACT"),
    treasury: required(env, "TREASURY_ACCOUNT"),
    paymentApi: required(env, "PAYMENT_API"),
    originAsset: required(env, "ORIGIN_ASSET"),
    destinationAsset: required(env, "DESTINATION_ASSET"),
    maxPaymentMicros: amount(env, "MAX_PAYMENT_MICROS", "5000000"),
    maxExitMicros: amount(env, "MAX_EXIT_MICROS", "10000000"),
    dailyBudgetMicros: amount(env, "DAILY_BUDGET_MICROS", "60000000"),
    allowedOrigins: optional(env, "ALLOWED_ORIGINS", "")
      .split(",")
      .map((origin) => origin.trim())
      .filter(Boolean),
    callsPerMinute: count(env, "CALLS_PER_MINUTE", "30"),
    spendingCallsPerHour: count(env, "SPENDING_CALLS_PER_HOUR", "10"),
  };
}

/** `nep141:<contract>` to `<contract>`. */
export function tokenContract(asset: string): string {
  const prefix = "nep141:";
  if (!asset.startsWith(prefix)) {
    throw new Error(`not a NEAR token: ${asset}`);
  }
  return asset.slice(prefix.length);
}
