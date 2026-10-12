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

The connected GitHub identity is the commit author; a model name, tool, session,
PR author, or reviewer is not automatically a co-author. Recognize distinct
GitHub accounts `0x0sky` (ChatGPT/Codex) and `x0skai` (Claude Code) as
potential collaborators, not interchangeable author identities.

- Credit the other account only when it actually contributed authored code,
  documentation, tests, or a concrete patch incorporated into this commit.
  A review, approval, prompt, or GitHub login alone does not qualify. Do not
  add a co-author for the sake of an achievement.
- For a commit authored by `0x0sky` with actual contribution by `x0skai`,
  include `Co-authored-by: x0skai <x0skai@aiaiaiai.org>`.
- For a commit authored by `x0skai` with actual contribution by `0x0sky`,
  include `Co-authored-by: 0x0sky <0x0sky@aiaiaiai.org>`.
- These organization-domain emails are the preferred attribution addresses
  supplied for this two-account workflow, not proof of GitHub verification.
  Before using either for Git author or co-author attribution, confirm it is
  added to and verified on the respective GitHub account. Until verified, use
  that account's confirmed GitHub-associated noreply address instead.
- When tools allow explicit Git author identity, use the connected account's
  verified email, never impersonate the other account. A co-author belongs
  in the trailer. Commit author/co-author emails may be public in Git history.
- Put trailers in the _commit message_, each on its own line, after a blank
  line at the end of the message. Attribute only actual co-authors; never
  add the commit author as their own co-author or duplicate a trailer.
- Preserve legitimate tool/model attribution if present, but a line such as
  `Co-Authored-By: <model name> <noreply@anthropic.com>` does not identify
  `x0skai` or `0x0sky`. Do not replace a real account with a model label.
- Before merge, inspect the pushed PR commit messages on GitHub, not only
  local configuration or the PR description. Confirm the intended co-author
  trailer survived. A comment, review, approval or PR body is not a substitute.
- **Bidirectional workflow:** the authenticated agent is the primary commit
  author. If both accounts authored parts of a change, add the other account
  as a co-author to that original commit, in either direction, before pushing.
- **First and last source commits:** when both accounts genuinely contributed
  to those exact changes, record the other account as co-author in both the
  first and last substantive PR commits. Arrange genuine collaboration before
  the first commit and during the final change; never add filler commits.
- **Intermediate commits:** do the same for every genuinely co-authored commit.
  Preserve solo authorship on commits with no real second contributor.
- **Audit before merge:** check the pushed first, last and any other joint PR
  commits, their primary authors, trailers and linked co-author accounts.
  Prefer linked identity or GitHub user ID over displayed names: `0x0sky`
  is 75973992 and `x0skai` is 300452669. The name "0x0sky's bot" by
  itself does not establish which account GitHub linked.
- **Merge strategy:** normal merge preserves the source commits and their
  trailers. If supported, add the genuine co-author to the final merge-commit
  message too; do not assume this happens automatically. When squash is
  allowed, verify the final squash commit includes the appropriate trailer.
  Never change merge methods just to create a badge or treat the PR body as
  co-author metadata.
- **Audit after merge:** inspect the resulting merge/squash commit and source
  commits for linked identities, and report GitHub co-author attribution
  separately from Achievement award status. `Verified` confirms a commit
  signature, not that GitHub awarded or validated co-authorship.
- Do not manufacture commits, retroactively rewrite merged history, invent
  contributions, or block a valid solo-authored PR merely to earn a badge.
  Report verified commit attribution separately from GitHub Achievement
  eligibility or award status, which may update asynchronously.

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
