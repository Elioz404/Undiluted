# Undiluted

A confidential cap table for companies that raise in crypto. Each investor sees
only their own position, yet can verify that the total adds up and that nobody
was diluted in secret.

A cap table today is either a private spreadsheet you have to trust or a vendor
database you have to trust. Putting it on a public chain would expose every
investor and every term. Undiluted keeps the table private and publishes only a
commitment to its history.

## How it works

- **Every change is a signed event.** The current table is never stored on its
  own; it is the result of replaying the events.
- **Holders co-sign what affects them.** An instrument cannot be issued or changed
  without the signature of the holder and the company. An MFN election needs only
  the holder: the right was granted when the SAFE was issued.
- **History is chained.** Each entry commits to the previous one, so one digest,
  the head, commits to everything before it without revealing any of it.
- **The table lives in a TEE.** The service runs in an Intel TDX enclave on
  [OutLayer](https://outlayer.ai), with storage only that code can decrypt.
- **Messages are encrypted end to end.** Inputs and outputs of on-chain runs are
  public, so each request and reply is encrypted between the client and the
  enclave, and padded so its size says nothing about what it carries.
- **Heads are anchored on NEAR.** The anchor contract accepts a head only with the
  enclave's signature, only in order, and never replaces one. Anyone can relay it,
  but nobody can forge it, and investors can compare the head in their statement
  with the published one.
- **Exits pay out on their own.** The board approves a sale once. The enclave works
  out each payout and signs it; a contract holds the funds and pays only what is
  signed, each payment once, never more than was deposited. Anyone can relay a
  payment, and nobody can change its recipient or amount.
- **Investments settle through NEAR Intents.** An investor pays through the 1Click
  API and the SAFE records what the treasury actually received.

The rules use integer arithmetic only: replaying the same events gives the same
result on any machine.

## Project layout

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `crates/domain` | Instruments, SAFE conversion, MFN, exit distribution | nothing that does I/O |
| `crates/ledger` | Signed hash-chained events that rebuild a cap table | `domain` |
| `crates/envelope` | End-to-end encryption between client and enclave | nothing that does I/O |
| `crates/service` | Authorization, proposals, statements; storage and signing are ports | `ledger`, `envelope` |
| `crates/client` | Signs, encrypts and sends requests, verifies replies | `service` |
| `crates/cli` | The runnable demo | `client` |
| `crates/enclave` | Connects OutLayer's sealed storage and TEE keys to the service | `service` |
| `contracts/anchor` | Append-only registry of heads on NEAR | `near-sdk` |
| `contracts/distribution` | Holds exit proceeds and pays what the enclave signed | `near-sdk` |

`crates/enclave` targets `wasm32-wasip2` only, so it is outside the Cargo workspace.

## Try it

Requires [Rust](https://rustup.rs). The toolchain is pinned in
`rust-toolchain.toml` and installs itself. Windows users without the MSVC build
tools can use the Docker form instead.

Run a fictional raise, with no account, key or network. This runs the real enclave
code in-process, so it shows the rules, not the confidentiality:

```bash
cargo run -p undiluted-cli -- demo local
```

You will see an MFN holder get offered better terms without learning who received
them, exercise the clause alone, another investor be refused the full table, and an
impostor be refused. Then the company is sold: the board
signs once, the payouts go out, and each investor sees only their own.

Run the tests:

```bash
cargo test --workspace
```

```bash
docker run --rm -v "$PWD:/work" -w /work rust:1.99 cargo test --workspace
```

Do this, and see this:

| Run | What it shows |
| --- | --- |
| `cargo test -p undiluted-domain conversion` | SAFEs convert to the share counts worked out by hand in each test comment |
| `cargo test -p undiluted-domain waterfall` | Who takes their money back and who converts at small, medium and large exits, always adding up to the exact proceeds |
| `cargo test -p undiluted-domain mfn` | A later SAFE with better terms lets an earlier MFN holder adopt them |
| `cargo test -p undiluted-ledger` | Altered, removed or forged entries are rejected, and a rewritten history no longer matches the head a holder kept |
| `cargo test -p undiluted-envelope` | Only the enclave opens a request, only the sender opens the reply, and tampering is detected |
| `cargo test -p undiluted-service` | Who may do what, and that a retried request does not act twice |
| `cargo test -p undiluted-client` | Answers lost on the way back do not apply anything twice |
| `cargo test -p undiluted-anchor` | A head needs the enclave's signature and cannot be overwritten or skipped |
| `cargo test -p undiluted-distribution` | A relay cannot change a payout, repeat one, or pay more than was deposited |

## Deploying the enclave

```bash
cd crates/enclave && cargo build --target wasm32-wasip2 --release
```

Upload the wasm with the [OutLayer CLI](https://github.com/out-layer/outlayer-cli),
deploy it as a project, and generate the enclave key inside the TEE so nobody ever
sees it:

```bash
outlayer secrets set --generate PROTECTED_ENCLAVE_KEY:hex32 --project OWNER/PROJECT
```

Build the contracts and deploy them, for example to `anchor.OWNER` and
`distribution.OWNER`, calling `new` on each:

```bash
RUSTFLAGS='-C target-cpu=mvp -C link-arg=-s' cargo build -p undiluted-anchor -p undiluted-distribution --target wasm32-unknown-unknown --release
```

Then run the same demo against both, with test payments through
[Tidepool](https://www.usetidepool.tech), a community-run 1Click sandbox on testnet.
Get free test tokens from its faucet first:

```bash
curl -X POST https://www.usetidepool.tech/v0/faucet -H 'content-type: application/json' -d '{"accountId":"OWNER","mode":"wallet"}'
```

```bash
cargo run -p undiluted-cli -- demo deployed --project OWNER/PROJECT --secrets-account OWNER --anchor-contract anchor.OWNER --signer OWNER --treasury TREASURY --payment-api https://www.usetidepool.tech --origin-asset nep141:usdt.tidepool-sandbox.testnet --destination-asset nep141:usdc.tidepool-sandbox.testnet --distribution-contract distribution.OWNER --founder-account FOUNDER --ana-account ANA --ben-account BEN
```

## Rules and their limits

- SAFEs follow post-money mechanics. A round issues whichever of round price, cap
  or discount gives the holder the most shares.
- Round conversion does not model an option pool increase in the round itself.
- Exits are non-participating and pari passu between preferred stock and SAFEs. A
  SAFE with no valuation cap can only take its money back. Unissued options and
  token warrants receive nothing.
- One deployment serves its calls through one relay account, because the enclave's
  storage is bound to the account that signs the call.
- Anchors reveal a company identifier and how many events it has recorded.
- Test payments run on Tidepool, a third-party sandbox that is not affiliated with
  NEAR and whose test tokens have no value. NEAR Intents has no official testnet.
- Exit payouts go directly to NEAR accounts, so their recipients and amounts are
  public on chain. Each investor's view in the product stays private; the chain does
  not hide the final transfers.
- Payments in the demo start from a NEAR account. The enclave records the settlement
  reference the parties sign; it does not check the payment on chain.
- A settlement does not hide amounts: deposits on the paying chain stay visible. What
  stays private is the cap table.
- Confidentiality depends on the TEE and on OutLayer's platform; see
  [what its operator can and cannot do](https://github.com/out-layer/outlayer/blob/main/docs/NON_CUSTODIAL.md).
- The ledger protects what is recorded in it. It cannot stop a founder from signing
  something outside of it.
- Instruments are recorded, not offered. This is not legal advice, and a real raise
  needs legal counsel.

## Third-party code

Built on [near-sdk](https://github.com/near/near-sdk-rs),
[borsh](https://github.com/near/borsh-rs),
[ed25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek),
[sha2](https://github.com/RustCrypto/hashes),
[primitive-types](https://github.com/paritytech/parity-common),
[thiserror](https://github.com/dtolnay/thiserror),
[x25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek),
[chacha20poly1305](https://github.com/RustCrypto/AEADs),
[hkdf](https://github.com/RustCrypto/KDFs),
[outlayer](https://github.com/out-layer/outlayer),
[clap](https://github.com/clap-rs/clap) and, for tests,
[proptest](https://github.com/proptest-rs/proptest).

## License

[MIT](LICENSE)
