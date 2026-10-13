# CLAUDE.md

Guidance for Claude Code in this repository.

## GitHub delivery

Applies to any task that touches GitHub, CI, pull requests, releases or
deploys. For anything else, ignore this section.

### Access and authority

- Assume GitHub is connected with read/write access to the account's
  repositories. Report an access problem only after a real permission error, a
  missing capability, or a step that needs account-owner privileges.
- Reversible actions (branches, commits, pull requests, comments) proceed
  without asking again.
- Ask first, every time, before: deploy, delete, transfer a repository, a
  destructive data operation, or anything irreversible outside the repository.

### One task, one pull request

- Make the smallest complete change that can be merged on its own.
- Split when parts have independent acceptance criteria, can be reviewed and
  merged separately, or when you find unrelated cleanup or a follow-up. Open
  that as its own pull request.
- Keep together only what would leave the repository invalid if split, or what
  is one atomic contract or migration.
- Never mix unrelated features, or a feature with unrelated cleanup. Never grow
  a pull request just to have fewer of them.
- Do not repeat work another pull request already did.

### Sequence

One delivery pull request is active per chain. Before starting the next:

1. Inspect the previous one and finish its requested scope if incomplete.
2. Merge it when it is ready, refresh the base branch, and branch from the
   latest base.

If the previous one is blocked, stop the dependent work and report the
blocker. Unrelated pull requests may run in parallel only when they are
independent and do not conflict; never merge one only to clear a queue.

### Flow

inspect state → resolve the previous pull request → branch from the latest
base → open a **draft** pull request → full CI → green → deploy validation (if
applicable) → mark ready for review → merge when ready → deploy by hand.

GitHub cannot merge a draft, and nobody can approve one. Once the head is green
and you have re-read your own diff, mark the pull request ready yourself. A
pull request that waits on a decision from the author, or that you could not
verify (say what was not checked), stays a draft.

Each stage has one job and does not redo another's:

- **Full CI** verifies correctness: format, lint, static analysis, tests,
  contract validation, and the build where correctness needs it. It is the
  authoritative gate and runs once per unchanged head. It never deploys.
- **Deploy validation** verifies deployability before merge, only where the
  repository deploys and that can be validated: the artifact or package, deploy
  configuration, environment contracts, migrations, preflight, readiness and
  pre-activation smoke contracts. Reuse CI results and artifacts. It does not
  rerun CI, tests, analysis or contract checks, rebuild an identical artifact,
  or activate a release.
- **Merge** is automatic and needs no user authorization once the requested
  task is complete, its scope is satisfied, CI is green, deploy validation is
  green or not applicable, and no known blocker invalidates it. Afterwards,
  record the merged state, refresh the base, and carry on with the sequence
  without waiting for confirmation. Merging is not deploying.
- **Deploy** is manual, comes after merge, and needs explicit user
  authorization every time. It consumes the verified artifact, makes the
  environment transition, activates the release, and checks minimal health. It
  does not rerun CI or validation, or rebuild an identical artifact.

Do not rerun full CI or deploy validation after merge. Repeat a check only when
the artifact changed, the relevant environment state changed, the platform
requires it, or the repository documents why. Keep a repeat narrow and
different from the earlier check.

### Shared-account co-authorship

The owner works from a phone. Agents must perform Git configuration, commit
creation, PR updates and verification in their connected environment; do not
require a local checkout or terminal on the owner's phone.

| Environment | Primary Git author | Required tool trailer |
| --- | --- | --- |
| ChatGPT / Codex | `0x0sky <0x0sky@aiaiaiai.org>` | `Assisted-by: OpenAI Codex` |
| Claude Code | `x0skai <x0skai@aiaiaiai.org>` | `Co-authored-by: Claude <noreply@anthropic.com>` |

These addresses were explicitly confirmed by the owner. GitHub-linked identity
must still be checked on pushed commits; signature verification is separate.
Never infer the Git author from the PR author, display name or model name.

- When the other account contributed code, tests, documentation or an
  incorporated patch to this exact commit, add its canonical trailer:
  `Co-authored-by: x0skai <x0skai@aiaiaiai.org>` or
  `Co-authored-by: 0x0sky <0x0sky@aiaiaiai.org>`.
- Credit Claude with `Co-authored-by: Claude <noreply@anthropic.com>` when
  Claude contributed, including a Codex-authored commit incorporating its work.
  Credit Codex with `Assisted-by: OpenAI Codex` when Codex contributed,
  including a Claude-authored commit incorporating its work.
- Account credit and tool credit are distinct and may coexist. Never add
  the primary author as their own co-author or duplicate a trailer.
- Put all trailers in the final commit-message paragraph, separated from the
  body by a blank line, with one trailer per line and no semicolons.
- Apply this to the first, intermediate and final substantive commits whenever
  the corresponding contribution exists. A review, prompt, approval or account
  ownership alone does not establish joint authorship. Do not manufacture
  contributions or filler commits for an achievement.
- Before the first commit, inspect the authenticated account and Git author.
  In a shell, set repository-local `user.name` and `user.email` from the table.
  If an API chooses author metadata itself, inspect the resulting commit.
  The current GitHub API adapter cannot set author email and uses the verified
  `75973992+0x0sky@users.noreply.github.com` identity. This exact adapter
  exception is accepted by CI and must be disclosed in the PR; it does not
  change the preferred email or co-author addresses. Do not impersonate the
  other account or silently substitute any other email.
- Before handing work to another agent, update the PR's Attribution / handoff
  section: account and tool used, authored files/patch or SHA, contribution
  incorporated from the other agent (or none), verified head/checks, remaining
  work and merge authorization. Preserve that evidence across context changes.
- Run `python scripts/check_commit_attribution.py BASE HEAD BASELINE` before
  delivery, using the policy baseline pinned in the attribution workflow.
  CI checks new source commits, not the synthetic PR merge commit. Existing
  history at adoption is exempt; never rewrite merged history to add credit.
- Inspect pushed first/last/joint commit messages and linked GitHub authors.
  The checker validates metadata, not whether a contribution actually occurred.
  It does not prove email verification, account linkage or achievement status.
- For squash, construct and inspect the final message explicitly: preserve all
  genuine account/tool credits across the squashed changes, excluding the final
  primary author from co-author trailers. Do not assume GitHub preserves them.
  Inspect the resulting commit after an authorized merge.
- Keep the PR draft until required checks pass. Do not merge without explicit
  user authorization; deployment requires separate authorization. This rule
  overrides generic automatic-merge wording elsewhere in these instructions.
- The `Commit attribution` workflow reports failures; repository administrators
  must make its check required if merge blocking is desired. Do not claim branch
  protection is configured merely because the workflow exists.

### Efficiency

- Read only the state the current decision needs: targeted files and metadata
  over whole-repository scans, pull request metadata before the full diff,
  CI logs only for failed or ambiguous checks. Do not re-read what you already
  verified in this task.
- Batch independent reads. Reuse identifiers, SHAs and results you already
  have. Do not poll a status that cannot have changed. Prefer one precise write
  over several incremental ones.
- Run a fast, targeted check locally when it can fail early, and leave the
  repository's full CI as the final gate. A green run for the exact same head
  is reused, not repeated; rerun only what changed inputs affect.

### Engineering

Inspect before modifying. Prefer verified state and contracts over assumptions,
the root cause over a symptom patch, the smallest sufficient change, the
repository's own conventions over generic preferences, and a complete change
over a partial one.

### Reporting

Say what changed, why, what was verified, which delivery stage it is at, any
remaining risk or blocker, and whether deploy or another protected action
needs authorization. Do not narrate every tool call, repeat logs without
interpreting them, claim success you did not verify, or ask for confirmation
the policy already grants.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
