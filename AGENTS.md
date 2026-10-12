# AGENTS.md

Instructions for OpenAI coding agents working in this repository.

## GitHub delivery

Apply this section whenever a task touches GitHub repositories, branches, commits,
pull requests, CI, releases, or deployment. Otherwise ignore it.

### Authority

Assume connected GitHub repositories are available for normal read/write work.
Inspect actual state before reporting an access limitation.

Proceed without additional confirmation for reversible repository actions,
including creating branches, editing files, committing and pushing, opening or
updating pull requests, commenting or reviewing, marking a pull request ready,
and merging when all merge conditions below are satisfied.

Always obtain explicit user authorization before deployment or release
activation, repository deletion or transfer, destructive data operations, or
irreversible actions outside the repository.

A merge is not a deploy. Deployment authorization is never implied by merge
authorization and is single-use unless the user explicitly says otherwise.

### Repository rules

- Never commit directly to the default branch.
- Start each task from the latest default branch unless an existing task branch
  is explicitly being continued.
- Use one branch and one pull request per independently reviewable task.
- Prefer the smallest complete change.
- Do not mix unrelated features, fixes, refactors, or cleanup.
- Do not repeat work already present in another branch or pull request.
- Repository-specific domain, build, test, and safety rules remain authoritative.
  If an older generic Git workflow conflicts with this section, this section
  wins unless the repository explicitly declares a current exception.

### Before changing anything

Inspect only the state needed for the current decision:

1. repository and default branch;
2. relevant repository instructions;
3. existing pull requests or branches for the same task;
4. targeted files and contracts;
5. relevant CI and merge requirements.

Reuse state already verified during the current task. Do not repeatedly fetch
or inspect unchanged state.

If a previous pull request is part of the same dependency chain, resolve it
before creating dependent work. Independent work may proceed in parallel when
it cannot conflict.

### Delivery flow

For each task, follow this state machine:

inspect
→ latest base
→ task branch
→ smallest complete implementation
→ draft pull request
→ targeted checks
→ full CI
→ deploy validation when applicable
→ self-review exact diff
→ ready for review
→ merge
→ refresh base

Deployment is a separate protected step after merge.

### Pull requests

Open the pull request as a draft while implementation or verification remains.

Keep it draft while implementation is incomplete, CI is unresolved, a required
decision is outstanding, required verification could not be performed, or a
known blocker remains.

Once the exact head revision is verified, required CI is green, applicable
deploy validation is green, the requested scope is complete, and no known
blocker remains, mark the pull request ready without asking the user again.

Merge automatically when all of the following are true:

- the requested task is complete;
- the final diff matches the intended scope;
- required CI for the exact head is successful;
- deploy validation is successful or not applicable;
- repository merge requirements are satisfied;
- no unresolved blocker invalidates the change.

Use an allowed repository merge method. A merge-method restriction does not
invalidate an otherwise verified head.

After merge, record the resulting state and refresh the default branch before
dependent work.

### Verification

Treat CI as the authoritative correctness gate where CI exists.

Full CI may include formatting, linting, static analysis, tests, contract
validation, and builds required for correctness.

Do not rerun successful CI for an unchanged commit merely because the pull
request was marked ready or merged.

Repeat verification only when relevant inputs changed, the head changed,
relevant environment state changed, the platform requires it, or repository
policy explicitly requires it. When repeating verification, run only affected
checks unless full CI is required.

A failing unrelated automation is not automatically a correctness blocker.
Inspect the failure and determine whether it is relevant to the requested
change or a required repository gate.

Never claim a check passed unless its result was actually observed.

### Deploy validation

Where deployment exists and can be validated before merge, verify deployability
without activating a release.

Deploy validation may check artifacts or packages, deployment configuration,
environment contracts, migrations, preflight, readiness, and pre-activation
smoke contracts.

Reuse CI artifacts and results where possible. Deploy validation must not
unnecessarily repeat correctness CI, rebuild an identical artifact, or activate
the release.

### Deployment

Deployment always requires explicit user authorization, even after an automatic
merge.

After authorization, deploy the already verified revision or artifact, perform
the environment transition, activate the release, and verify minimal
post-deploy health.

Do not silently expand deployment authorization to later deployments.

### Engineering behavior

Inspect before modifying.

Prefer verified state over assumptions, repository contracts over generic
conventions, root-cause fixes over symptom patches, the smallest sufficient
change over broad rewrites, explicit ownership and boundaries over hidden
coupling, predictable behavior over cleverness, and observable failures over
silent fallback.

When a supporting fix is inseparable from the requested task because the
repository could not otherwise validate or merge that task, make the smallest
such fix and explain why it belongs in the same pull request.

Unrelated defects discovered during the task belong in separate follow-up work.

### Tool behavior

Use available GitHub capabilities directly when they can complete the task.
Do not ask the user to perform GitHub actions the available tools can perform.

Prefer batched independent reads and precise writes. Reuse repository
identifiers, branch names, commit SHAs, pull request numbers, and verified
results rather than rediscovering them.

Inspect detailed CI logs only for failed or ambiguous checks. Do not poll state
that cannot yet have changed.

Never report repository state, successful writes, merges, CI results, or
deployment results without evidence from the corresponding operation.

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
- Put trailers in the *commit message*, each on its own line, after a blank
  line at the end of the message. Attribute only actual co-authors; never
  add the commit author as their own co-author or duplicate a trailer.
- Preserve legitimate tool/model attribution if present, but a line such as
  `Co-Authored-By: Claude Opus <noreply@anthropic.com>` does not identify
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

### Reporting

Report outcomes, not tool choreography.

At meaningful milestones or completion, state what changed, why, what was
verified, the current delivery stage, any remaining blocker or material risk,
and whether a protected action such as deployment still requires authorization.

Do not ask for confirmation when this policy already grants authority to
continue. If the task cannot be completed, identify the concrete blocker and
the exact state reached instead of giving a generic access or tooling
disclaimer.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
