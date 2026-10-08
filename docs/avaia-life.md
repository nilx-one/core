# Avaia life

`apply_avaia_life(state, owner, subject, command)` owns the local needs and
spatial state of one owned AI Bond. This is implementation-owned runtime state
under the AI Bonds contract, not BondChain, public presence or signing authority.

Authenticated identity supplies owner and Avaia addresses. Core validates their
grammars and discriminator consistency, not ownership. The current identity API
exposes no canonical BondId: do not fabricate one from an address. Storage is
scoped to the identity pair; address rotation needs an authenticated migration.

Initialize once with home and position (Core E7 coordinates). Home stays fixed;
no command accepts the owner's moving coordinate. Observe actual route position,
motion (idle, walking, studying), and active elapsed milliseconds as a decimal
string from 0 to 60000. Observer-gated execution remains unchanged. Reopening
must not invent offline travel or time. Persist actual progress, not the end of
an unfinished route.

Responses are `{ok:true,state}` or `{ok:false,error}`; retain stored state on
failure. Needs are decimal strings from 0 to 10000. Per active second hunger
increases by 1. Idle or studying energy decreases by 1 per active second.
Walking instead consumes 100 energy units (1%) per 50 metres, charged from
the walked distance and never less than the idle rate. The distance is the
larger of the straight line from the previous observed position and the
optional `walked_m` (decimal string, metres of route the host actually
followed since the last report), so loops and turns inside one observation
are not free. It is clamped to 6 m/s of the reported `elapsed_ms`: an
implausible jump is charged at that ceiling and the position still advances.
No simulated offline motion is charged; zero-duration reports do not claim
travel. Hunger >=7000 or
energy <=3000 triggers return_home. Within 50 m of home recovery removes 20
hunger and adds 10 energy per second until hunger <=1000 and energy >=9000.
Arrival never instantly restores needs. Subsecond remainders are persisted.
These are simulated product needs, not biological claims.

Core projects activity and intent. Hosts route and render; return_home/recover
outrank optional outing choices. A blocked route keeps position unchanged and
retains return_home intent. Wasm and UniFFI use the identical implementation.

See also `docs/avaia-proximity.md` for distance-bound reveal capability.

© 2026 aiaiaiai · aiaiaiai.org
