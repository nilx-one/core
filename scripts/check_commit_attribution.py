# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: MPL-2.0
"""Validate new source commit attribution without rewriting Git history."""
import re
import subprocess
import sys


ACCOUNTS = {
    "0x0sky": "75973992+0x0sky@users.noreply.github.com",
    "x0skai": "x0skai@aiaiaiai.org",
}
ACCOUNT_BY_ADDRESS = {address.lower(): name for name, address in ACCOUNTS.items()}
CLAUDE = "Claude <noreply@anthropic.com>"


def validate(email, message):
    errors = []
    parsed = subprocess.run(
        ["git", "interpret-trailers", "--parse"], input=message,
        text=True, capture_output=True, check=True,
    ).stdout
    trailers = [tuple(line.split(":", 1)) for line in parsed.splitlines()]
    coauthors = [v.strip() for k, v in trailers if k.lower() == "co-authored-by"]
    assisted = [v.strip() for k, v in trailers if k.lower() == "assisted-by"]
    relevant = coauthors + assisted
    if len(relevant) != len(set(v.lower() for v in relevant)):
        errors.append("duplicate attribution trailer")
    # None means an external contributor: account rules do not apply to them.
    author = ACCOUNT_BY_ADDRESS.get(email.lower())
    for value in coauthors:
        match = re.fullmatch(r"([^<>]+) <([^<>\s]+)>", value)
        if not match:
            errors.append("malformed co-author trailer")
            continue
        name, address = match.groups()
        if (author and name == author) or address.lower() == email.lower():
            errors.append("primary author repeated as co-author")
        if name in ACCOUNTS and address.lower() != ACCOUNTS[name]:
            errors.append("incorrect account co-author address")
        if address.lower() in ACCOUNT_BY_ADDRESS and name != ACCOUNT_BY_ADDRESS[address.lower()]:
            errors.append("incorrect account co-author name")
        if name.lower() == "claude" and value != CLAUDE:
            errors.append("incorrect Claude attribution")
    if any(v.startswith("OpenAI") and v != "OpenAI Codex" for v in assisted):
        errors.append("use Assisted-by: OpenAI Codex")
    return errors


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: check_commit_attribution.py BASE HEAD BASELINE")
    base, head, baseline = sys.argv[1:]
    for ref in (base, head, baseline):
        if not re.fullmatch(r"[0-9a-f]{40}", ref):
            raise SystemExit("expected full commit SHAs")
    commits = subprocess.check_output(
        ["git", "rev-list", "--no-merges", f"{base}..{head}", f"^{baseline}"], text=True
    ).splitlines()
    failed = False
    for sha in commits:
        raw = subprocess.check_output(["git", "show", "-s", "--format=%ae%x00%B", sha], text=True)
        email, message = raw.split("\0", 1)
        for error in validate(email.strip(), message):
            print(f"{sha}: {error}")
            failed = True
    if failed:
        raise SystemExit(1)
    print(f"Attribution OK: {len(commits)} new source commits")


if __name__ == "__main__":
    main()
