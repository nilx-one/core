-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Database boundary for the application rule: one account per provider type
-- per Bond. Existing duplicate rows are not silently discarded; if any exist,
-- this migration fails visibly so the conflict is resolved deliberately before rollout.
CREATE UNIQUE INDEX IF NOT EXISTS identity_providers_one_account_per_type
ON identity_providers (pub_dress, provider);
