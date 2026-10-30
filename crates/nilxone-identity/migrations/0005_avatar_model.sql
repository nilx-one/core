-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- The avatar a human Bond is represented by. NULL means the person has not
-- chosen one: no body is assigned to anyone by default, and the client draws
-- the neutral study until a choice exists.
ALTER TABLE identities
    ADD COLUMN avatar_model TEXT
    CHECK (avatar_model IS NULL OR avatar_model IN ('sky-study', 'dasha-study', 'kai-study'));
