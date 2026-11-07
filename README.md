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
  without the signature of the holder and the company.
- **History is chained.** Each entry commits to the previous one, so one digest,
  the head, commits to everything before it without revealing any of it.
- **Heads are anchored on NEAR.** The anchor contract only accepts sequences in
  order and never replaces a recorded head, so a company cannot show two different
  histories to two different investors.

The rules use integer arithmetic only: replaying the same events gives the same
result on any machine.

## Project layout

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `crates/domain` | Instruments, SAFE conversion, MFN, exit distribution | nothing that does I/O |
| `crates/ledger` | Signed hash-chained events that rebuild a cap table | `domain` |
| `contracts/anchor` | Append-only registry of heads on NEAR | `near-sdk` |

## Try it

Requires [Rust](https://rustup.rs). The toolchain is pinned in
`rust-toolchain.toml` and installs itself. Windows users without the MSVC build
tools can use the Docker command below instead.

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
| `cargo test -p undiluted-anchor` | A recorded head cannot be overwritten, skipped or written by anyone but the authority |

## Rules and their limits

- SAFEs follow post-money mechanics. A round issues whichever of round price, cap
  or discount gives the holder the most shares.
- Round conversion does not model an option pool increase in the round itself.
- Exits are non-participating and pari passu between preferred stock and SAFEs. A
  SAFE with no valuation cap can only take its money back. Unissued options and
  token warrants receive nothing.
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
[thiserror](https://github.com/dtolnay/thiserror) and, for tests,
[proptest](https://github.com/proptest-rs/proptest).

## License

[MIT](LICENSE)
