# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: MPL-2.0

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_SPDX = sys.argv[1] if len(sys.argv) > 1 else ""

REQUIRED_FILES = (
    "LICENSE",
    "NOTICE",
    "README.md",
    "CONTRIBUTING.md",
    "CLA.md",
    "TRADEMARKS.md",
    "SECURITY.md",
)

AUTHORED_EXTENSIONS = {
    ".bash", ".c", ".cc", ".cjs", ".cpp", ".css", ".cxx", ".h", ".hpp",
    ".htm", ".html", ".hxx", ".java", ".js", ".jsx", ".kt", ".kts", ".less",
    ".m", ".mjs", ".mm", ".py", ".rake", ".rb", ".rs", ".sass", ".scss",
    ".sh", ".sql", ".swift", ".toml", ".ts", ".tsx", ".yaml", ".yml", ".zsh",
}
AUTHORED_FILENAMES = {"Dockerfile", "Makefile"}
EXCLUDED_DIRS = {
    ".git", ".venv", "build", "coverage", "dist", "fixtures", "generated",
    "node_modules", "snapshots", "target", "third_party", "vendor", "venv",
}
EXCLUDED_FILENAMES = {
    "Cargo.lock", "Gemfile.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock",
}
COPYRIGHT = re.compile(r"© 20\d{2}(?:–20\d{2})? aiaiaiai · aiaiaiai\.org")


def fail(message: str, errors: list[str]) -> None:
    errors.append(message)


def authored_file(path: Path) -> bool:
    if path.name in EXCLUDED_FILENAMES:
        return False
    if any(part in EXCLUDED_DIRS for part in path.relative_to(ROOT).parts):
        return False
    return path.name in AUTHORED_FILENAMES or path.suffix.lower() in AUTHORED_EXTENSIONS


def validate() -> list[str]:
    errors: list[str] = []
    if EXPECTED_SPDX not in {"MPL-2.0", "Apache-2.0"}:
        return ["Expected SPDX id must be MPL-2.0 or Apache-2.0"]

    for relative in REQUIRED_FILES:
        if not (ROOT / relative).is_file():
            fail(f"missing required file: {relative}", errors)

    license_path = ROOT / "LICENSE"
    if license_path.is_file():
        text = license_path.read_text(encoding="utf-8")
        marker = "Mozilla Public License Version 2.0" if EXPECTED_SPDX == "MPL-2.0" else "Apache License"
        if marker not in text:
            fail(f"LICENSE does not match {EXPECTED_SPDX}", errors)

    notice_path = ROOT / "NOTICE"
    if notice_path.is_file() and "© 2026 aiaiaiai · aiaiaiai.org" not in notice_path.read_text(encoding="utf-8"):
        fail("NOTICE does not contain the canonical aiaiaiai signature", errors)

    readme_path = ROOT / "README.md"
    if readme_path.is_file() and EXPECTED_SPDX not in readme_path.read_text(encoding="utf-8"):
        fail(f"README.md does not declare {EXPECTED_SPDX}", errors)

    expected_line = f"SPDX-License-Identifier: {EXPECTED_SPDX}"
    for path in sorted(ROOT.rglob("*")):
        if not path.is_file() or not authored_file(path):
            continue
        relative = path.relative_to(ROOT)
        try:
            head = "\n".join(path.read_text(encoding="utf-8").splitlines()[:12])
        except UnicodeDecodeError:
            fail(f"authored source is not UTF-8: {relative}", errors)
            continue
        if not COPYRIGHT.search(head):
            fail(f"missing canonical copyright header: {relative}", errors)
        if expected_line not in head:
            fail(f"missing {expected_line}: {relative}", errors)

    return errors


if __name__ == "__main__":
    problems = validate()
    if problems:
        for problem in problems:
            print(f"ERROR: {problem}")
        raise SystemExit(1)
    print(f"repository policy OK ({EXPECTED_SPDX})")
