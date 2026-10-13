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

The owner works from a phone. Agents must perform Git configuration, commit
creation, PR updates and verification in their connected environment; do not
require a local checkout or terminal on the owner's phone.

| Environment     | Primary Git author             | Required tool trailer                            |
| --------------- | ------------------------------ | ------------------------------------------------ |
| ChatGPT / Codex | `0x0sky <0x0sky@aiaiaiai.org>` | `Assisted-by: OpenAI Codex`                      |
| Claude Code     | `x0skai <x0skai@aiaiaiai.org>` | `Co-authored-by: Claude <noreply@anthropic.com>` |

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
