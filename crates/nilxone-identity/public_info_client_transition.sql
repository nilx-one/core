-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Opt-in client ID transition only. The predecessor remains the owner row:
-- no UPDATE to (issuer, subject), no XP or product reference movement.
-- A successor alias exists only after a freshly verified AIS continuity
-- token proves that both pairwise subjects belong to the same AIS identity.
CREATE TABLE IF NOT EXISTS product_client_subject_links (
    issuer TEXT NOT NULL COLLATE BINARY,
    predecessor_subject TEXT NOT NULL COLLATE BINARY,
    successor_subject TEXT NOT NULL COLLATE BINARY,
    profile_id TEXT NOT NULL COLLATE BINARY,
    linked_at INTEGER NOT NULL CHECK (linked_at >= 0),
    PRIMARY KEY (issuer, successor_subject),
    UNIQUE (issuer, predecessor_subject),
    FOREIGN KEY (issuer, predecessor_subject)
        REFERENCES public_profiles (issuer, subject) ON DELETE CASCADE,
    FOREIGN KEY (profile_id)
        REFERENCES public_profiles (profile_id) ON DELETE CASCADE
) STRICT;

-- Fail closed if either old or new client tries the ordinary first-sign-in
-- upsert after the signed continuity has been claimed. Otherwise a later
-- callback could silently fork a new product profile or reopen a legacy
-- writer despite the transactional session invalidation.
CREATE TRIGGER IF NOT EXISTS product_subject_link_no_re_registration
BEFORE INSERT ON public_profiles
WHEN EXISTS (
    SELECT 1 FROM product_client_subject_links
    WHERE issuer = NEW.issuer
      AND (predecessor_subject = NEW.subject OR successor_subject = NEW.subject)
)
BEGIN
    SELECT RAISE(ABORT, 'product client subject already transitioned');
END;

CREATE TRIGGER IF NOT EXISTS product_subject_link_no_subject_move
BEFORE UPDATE OF issuer, subject ON public_profiles
WHEN EXISTS (
    SELECT 1 FROM product_client_subject_links
    WHERE issuer = NEW.issuer
      AND (predecessor_subject = NEW.subject OR successor_subject = NEW.subject)
)
BEGIN
    SELECT RAISE(ABORT, 'product client subject already transitioned');
END;
