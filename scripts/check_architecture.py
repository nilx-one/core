#!/usr/bin/env python3
# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: MPL-2.0

from __future__ import annotations

import pathlib
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]

FORBIDDEN = {
    "crates/nilxone-contracts/Cargo.toml": {
        "tokio",
        "wasm-bindgen",
        "uniffi",
    },
    "crates/nilxone-kernel/Cargo.toml": {
        "tokio",
        "wasm-bindgen",
        "uniffi",
        "rand",
        "getrandom",
    },
}


def dependency_names(path: pathlib.Path) -> set[str]:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    names: set[str] = set()
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        names.update(data.get(section, {}).keys())
    return names


def main() -> int:
    failures: list[str] = []
    for relative, forbidden in FORBIDDEN.items():
        present = dependency_names(ROOT / relative) & forbidden
        if present:
            failures.append(f"{relative}: forbidden dependencies: {', '.join(sorted(present))}")

    kernel = dependency_names(ROOT / "crates/nilxone-kernel/Cargo.toml")
    if kernel != {"nilxone-contracts"}:
        failures.append(
            "crates/nilxone-kernel/Cargo.toml: kernel dependencies must be exactly nilxone-contracts in C1"
        )

    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1

    print("architecture dependency boundary: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
