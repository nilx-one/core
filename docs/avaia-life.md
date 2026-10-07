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
increases by 1 and energy decreases by 1 (3 when walking). Hunger >=7000 or
energy <=3000 triggers return_home. Within 50 m of home recovery removes 20
hunger and adds 10 energy per second until hunger <=1000 and energy >=9000.
Arrival never instantly restores needs. Subsecond remainders are persisted.
These are simulated product needs, not biological claims.

Core projects activity and intent. Hosts route and render; return_home/recover
outrank optional outing choices. A blocked route keeps position unchanged and
retains return_home intent. Wasm and UniFFI use the identical implementation.

© 2026 aiaiaiai · aiaiaiai.org
