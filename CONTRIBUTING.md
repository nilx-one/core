# Contributing to 0x1 core

Contributions are welcome. The preferred outcome of useful external work is improvement of the canonical 0x1 implementation rather than incompatible fragmentation.

## Before Changing Code

Read the canonical contracts in `nilx-one/0x1` first. Protocol truth takes precedence over implementation convenience, UI behavior, or local assumptions.

Keep contributions narrowly scoped. Explain the problem, preserve authority boundaries, and include verification appropriate to the affected surface.

## Pull Requests

Prefer one coherent task per pull request. A pull request should state:

- what changes;
- why the change is needed;
- which contract owns the behavior;
- what was verified;
- whether compatibility, licensing, security, or migration behavior changes.

## Source Licensing

New authored source and configuration files that support comments must begin with the canonical repository header for their file format. For Rust and other `//`-comment formats:

```text
// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0
```

Use the equivalent comment syntax for other formats. Do not inject headers into JSON, lockfiles, generated output, vendored third-party files, snapshots, or formats where comments would break the contract.

Run `python scripts/check_repository_policy.py MPL-2.0` before submitting. Required GitHub CI runs the same check.

## Contribution Rights

Contributors keep ownership of their original contributions.

By intentionally submitting work for inclusion in this repository, the contributor is expected to provide the rights described in [CLA.md](CLA.md). The intended grant lets the project integrate, modify, distribute, sublicense, and relicense accepted work while leaving the contributor free to use their original contribution elsewhere.

The CLA is currently provisional until a production acceptance mechanism is finalized. Maintainers may require explicit signed or electronic acceptance before merging an external contribution.

## Third-Party Material

Do not introduce code, assets, generated material, or dependencies whose terms conflict with MPL-2.0 or with the project's ability to distribute accepted work. Preserve legally required upstream notices.

## Product Identity

Open-source permission does not grant permission to present a derivative as official 0x1. See [TRADEMARKS.md](TRADEMARKS.md).

---

© 2026 aiaiaiai · aiaiaiai.org
