# core

The shared Rust core for 0x1.

This repository owns deterministic product behavior shared across official 0x1 clients. Protocol truth remains defined by the canonical [`nilx-one/0x1`](https://github.com/nilx-one/0x1) specification; the normative client boundary is `documents/19-core-client-contract.md` there.

## Workspace

```text
crates/
├── nilxone-contracts        # versioned binding-safe values
├── nilxone-kernel           # deterministic transitions and explicit ports
├── nilxone-bindings-wasm    # WebAssembly translation boundary
├── nilxone-bindings-uniffi  # Swift/UniFFI translation boundary
└── nilxone-test-support     # deterministic fixtures, test-only
```

Dependency direction is one-way: bindings and test support may depend on the kernel and contracts; the kernel may depend only on contracts. Contracts never depend on the kernel or a platform binding.

The current representation shell implements the normative Core contract `0.1.0`: canonical binding-safe identifiers and integer strings, directional version compatibility, closed generic envelopes, deterministic typed failures, explicit external ports, and the native handshake. Production command/event/effect/projection registries remain empty until an owning interaction contract exists.

## Avaia and derived personal state

Core owns facts and deterministic state transitions, not an AI interpretation of a person.

A completed Interaction may become BondChain evidence. Avaia may consume that evidence together with other explicitly permitted signals to derive behavioral patterns, preferences, tendencies, and eventually a stable personal identity model. Those derived values are not BondChain facts and must never rewrite the interaction history that produced them.

```text
Interaction -> BondChain fact -> derived personal state -> Avaia behavior
              authoritative      interpretive             adaptive
```

The boundary is intentional:

- BondChain records what observably happened between exactly two Bonds.
- Preferences, behavioral patterns, traits, and Avaia identity are derived personal state.
- Model output, inference, confidence, or a personality projection never completes an Interaction.
- A derived trait may change as evidence accumulates without changing historical BondChain facts.
- Device-local runtime choices such as model size or quantization do not create a new Bond or a new identity.

Core may expose typed contracts needed to transport or project derived state, but the learning algorithm and model-specific latent representation belong to `nilx-one/ai`.

## Local verification

Use the pinned Rust toolchain from `rust-toolchain.toml`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
python3 scripts/check_architecture.py
```

Dependency license, source, and advisory policy is enforced with `cargo deny check` in CI.

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md), [CLA.md](CLA.md), and [TRADEMARKS.md](TRADEMARKS.md) before submitting substantial work.

New authored source files must carry the canonical aiaiaiai copyright signature and `SPDX-License-Identifier: MPL-2.0`. Repository policy CI validates this automatically.

## License

Licensed under the Mozilla Public License, Version 2.0 (`MPL-2.0`). See [LICENSE](LICENSE) and [NOTICE](NOTICE).

---

© 2026 aiaiaiai · aiaiaiai.org
