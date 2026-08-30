# Security Policy

Security reports are welcome and should be handled privately when disclosure could expose users, infrastructure, credentials, or an exploitable implementation flaw.

## Reporting

Prefer GitHub private vulnerability reporting or a private security advisory when that capability is available for this repository. Do not publish exploit details in a public issue before maintainers have had a reasonable opportunity to assess and remediate the problem.

A useful report includes:

- affected version, commit, or surface;
- impact and realistic attack preconditions;
- reproduction steps or a minimal proof of concept when safe;
- suggested mitigation, if known;
- whether the issue affects protocol semantics or only this implementation.

## Scope

Security fixes must preserve protocol truth. A security workaround in an implementation must not silently redefine canonical 0x1 behavior.

Third-party dependency vulnerabilities should identify the dependency and affected range. Never include live credentials, private keys, access tokens, or personal data in a report or test fixture.

---

© 2026 aiaiaiai · aiaiaiai.org
