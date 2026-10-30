// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

// xPing's engine (migrations/0028_xping_engines.sql): stored as Core's
// `XpingEngine`, read and replaced in one write transaction so two devices
// forcing at once cannot both spend from the same energy.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpingSeedsRepairRefusal {
    SessionChanged,
    StaleEngine,
    InsufficientSeeds,
    Engine(nilxone_contracts::XpingEngineError),
}

impl IdentityRepository {
    /// Repairs xPing and debits the Identity-owned seeds wallet in one
    /// transaction. Local inventory is never payment evidence.
    pub async fn repair_xping_with_seeds(
        &self,
        owner: &PubDress,
        active_session_hash: &[u8],
        expected: nilxone_contracts::XpingEngine,
        now_ms: u64,
    ) -> Result<Result<nilxone_contracts::XpingEngine, XpingSeedsRepairRefusal>, RepositoryError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if !spend_session_active_in(&mut tx, owner, active_session_hash, now_ms / 1000).await? {
            return Ok(Err(XpingSeedsRepairRefusal::SessionChanged));
        }

        let current = read_xping_engine_in(&mut tx, owner, now_ms).await?;
        if current != expected {
            return Ok(Err(XpingSeedsRepairRefusal::StaleEngine));
        }
        let price = current.repair_seeds(now_ms);
        let next = match current.repaired(price, now_ms) {
            Ok(next) => next,
            Err(error) => return Ok(Err(XpingSeedsRepairRefusal::Engine(error))),
        };
        let amount = i64::try_from(price).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
        let spent = sqlx::query(
            "UPDATE bond_seeds SET balance = balance - ? \
             WHERE owner_pub_dress = ? AND balance >= ?",
        )
        .bind(amount)
        .bind(owner.as_str())
        .bind(amount)
        .execute(&mut *tx)
        .await?;
        if spent.rows_affected() != 1 {
            return Ok(Err(XpingSeedsRepairRefusal::InsufficientSeeds));
        }

        let to_i64 = |value: u64| i64::try_from(value).map_err(|_| RepositoryError::CorruptPubInfo);
        sqlx::query(
            "INSERT INTO xping_engines \
             (owner_pub_dress, energy_permille, at_ms, empty_forced_ms, broken_at_ms) \
             VALUES (?, ?, ?, ?, ?) ON CONFLICT(owner_pub_dress) DO UPDATE SET \
             energy_permille = excluded.energy_permille, at_ms = excluded.at_ms, \
             empty_forced_ms = excluded.empty_forced_ms, broken_at_ms = excluded.broken_at_ms",
        )
        .bind(owner.as_str())
        .bind(i64::from(next.energy_permille))
        .bind(to_i64(next.at_ms.get())?)
        .bind(to_i64(next.empty_forced_ms.get())?)
        .bind(next.broken_at_ms.map(|at| to_i64(at.get())).transpose()?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Ok(next))
    }

    /// Applies `change` to the Bond's engine at `now_ms` and stores the
    /// answer. A Bond with no stored engine starts from a full one. A refused
    /// change stores nothing and is answered as it was refused.
    pub async fn change_xping_engine<E>(
        &self,
        owner: &PubDress,
        now_ms: u64,
        change: impl FnOnce(nilxone_contracts::XpingEngine) -> Result<nilxone_contracts::XpingEngine, E>,
    ) -> Result<Result<nilxone_contracts::XpingEngine, E>, RepositoryError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = read_xping_engine_in(&mut transaction, owner, now_ms).await?;
        let next = match change(current) {
            Ok(next) => next,
            Err(refused) => return Ok(Err(refused)),
        };
        let to_i64 = |value: u64| i64::try_from(value).map_err(|_| RepositoryError::CorruptPubInfo);
        sqlx::query(
            "INSERT INTO xping_engines \
             (owner_pub_dress, energy_permille, at_ms, empty_forced_ms, broken_at_ms) \
             VALUES (?, ?, ?, ?, ?) ON CONFLICT(owner_pub_dress) DO UPDATE SET \
             energy_permille = excluded.energy_permille, at_ms = excluded.at_ms, \
             empty_forced_ms = excluded.empty_forced_ms, broken_at_ms = excluded.broken_at_ms",
        )
        .bind(owner.as_str())
        .bind(i64::from(next.energy_permille))
        .bind(to_i64(next.at_ms.get())?)
        .bind(to_i64(next.empty_forced_ms.get())?)
        .bind(next.broken_at_ms.map(|at| to_i64(at.get())).transpose()?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Ok(next))
    }

    /// The Bond's engine, or a full one at `now_ms` when none is stored.
    pub async fn read_xping_engine(
        &self,
        owner: &PubDress,
        now_ms: u64,
    ) -> Result<nilxone_contracts::XpingEngine, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let engine = read_xping_engine_in(&mut transaction, owner, now_ms).await?;
        transaction.commit().await?;
        Ok(engine)
    }
}

async fn read_xping_engine_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
    now_ms: u64,
) -> Result<nilxone_contracts::XpingEngine, RepositoryError> {
    let row = sqlx::query_as::<_, (i64, i64, i64, Option<i64>)>(
        "SELECT energy_permille, at_ms, empty_forced_ms, broken_at_ms \
         FROM xping_engines WHERE owner_pub_dress = ?",
    )
    .bind(owner.as_str())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((energy, at_ms, empty_forced_ms, broken_at_ms)) = row else {
        return Ok(nilxone_contracts::XpingEngine::new(now_ms));
    };
    let to_u64 = |value: i64| u64::try_from(value).map_err(|_| RepositoryError::CorruptPubInfo);
    Ok(nilxone_contracts::XpingEngine {
        energy_permille: u16::try_from(energy)
            .ok()
            .filter(|energy| *energy <= nilxone_contracts::XPING_FULL_ENERGY)
            .ok_or(RepositoryError::CorruptPubInfo)?,
        at_ms: DecimalU64::new(to_u64(at_ms)?),
        empty_forced_ms: DecimalU64::new(to_u64(empty_forced_ms)?),
        broken_at_ms: broken_at_ms
            .map(|at| to_u64(at).map(DecimalU64::new))
            .transpose()?,
    })
}
