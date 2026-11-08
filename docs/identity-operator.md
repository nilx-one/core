# Identity operator workflows

The **List registered Bonds**, **Create Business Bond** and **Add Business Bond
member** workflows in Core prepare a validated request
and a static Linux operator binary from the exact master revision whose Rust CI
is green. It does not hold production SSH credentials, deploy a service, or open
a database on a GitHub runner. To execute the prepared request, an operator
separately starts **Identity operator** in `aiaiaiai-org/infra`, entering the Core
request run ID. Until that infra workflow is installed, preparation is not a
working production operation. Neither PR merges nor successful request builds
prove a grant.

## Operations

| Workflow operation | Required input | Effect |
| --- | --- | --- |
| `list-bonds` | none | All registered humans except businesses; excludes Avaia/stations |
| `list-business-bonds` | none | All registered Business Bonds |
| `create-business` | `business`, `member` (the owner) | Atomic business allocation + owner membership |
| `add-member` | `business`, `member`, `role` | Add a member/owner, or explicitly promote member to owner |

`owner` is a required existing human's exact `pub_dress` for creation; there is
no default owner and the workflow actor is not silently substituted. The role
input applies only to add-member; creation always assigns owner. A business
address is never claimed on collision. Exact creation replay by the same owner
returns `already_owned` without changing state.

Duplicate additions fail with a nonzero exit code. Only an explicit owner
request promotes an existing member. It retains the original membership
creation time. Existing owner cannot be demoted by requesting member. Legacy
roles remain readable and cannot be overwritten by this two-role surface.
Adding another owner does not transfer or remove an existing owner's role.

The JSON listing exposes only pub_dress and global role, in binary address
order. It never exports private subjects, providers, credentials or sessions.
Results from infra must be handled as operator data, not public profile data.

## Database boundary

`connect_operator` opens an existing database without service initialization,
migrations, name-based grants or backfills. Listing opens SQLite read-only.
Creation/add use the same database transaction rules as the service. An
incompatible schema fails visibly; an operator command never fixes or migrates
production opportunistically.

Infrastructure must verify the live image, DATABASE_URL and exact persistent
volume before running the tool. It must validate the request's repository,
workflow, event, master ancestry, successful conclusion and trusted operator,
and verify the downloaded artifact's checksums. It must not evaluate an input
as shell code or run a user-selected arbitrary command.

## Lifecycle decisions still outstanding

`freed` (24 hours), `hire`, and empty-business deletion (72 hours) are not
implemented or scheduled here. Their outstanding decisions are whether freed
removes management rights immediately, whether hire is employment requiring
acceptance, and when an empty-business deadline is cancelled. No membership
operation implies employment, consent, a completed Interaction or BondChain.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
