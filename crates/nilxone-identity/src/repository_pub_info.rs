// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// Who an experience event pays. The service stores the amount the owner
/// asserts; it does not price the action or attest that the play happened.
///
/// The wire label for every total this service answers is
/// [`EXPERIENCE_AUTHORITY`]. A client cannot choose a different one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExperienceEarner {
    Bond,
    Avaia,
}

impl ExperienceEarner {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bond => "bond",
            Self::Avaia => "avaia",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bond" => Some(Self::Bond),
            "avaia" => Some(Self::Avaia),
            _ => None,
        }
    }
}

/// Every experience object this service writes carries this authority.
/// The value is fixed: the totals are an owner assertion, not a service
/// attestation of play.
pub const EXPERIENCE_AUTHORITY: &str = "client";

/// The shared experience totals published in a Bond's `pub_info`.
///
/// These are the owner's report. Storing them does not make them evidence
/// that the Bond earned them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PubInfoExperience {
    pub bond_xp: u64,
    pub avaia_xp: u64,
}

impl PubInfoExperience {
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            bond_xp: 0,
            avaia_xp: 0,
        }
    }
}

/// One idempotent award. `id` is a client nonce, not a place.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExperienceAward {
    pub id: String,
    pub earner: ExperienceEarner,
    pub amount: u64,
}

impl IdentityRepository {
    /// The published experience totals. No row means this Bond has not
    /// published any yet, which is zero — not an error and not a guess.
    pub async fn read_pub_info(
        &self,
        owner: &PubDress,
    ) -> Result<PubInfoExperience, RepositoryError> {
        let row = sqlx::query(
            "SELECT bond_xp, avaia_xp FROM bond_pub_info WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_optional(&self.pool)
        .await?;
        match row {
            None => Ok(PubInfoExperience::zero()),
            Some(row) => Ok(PubInfoExperience {
                bond_xp: xp_column(row.try_get("bond_xp")?)?,
                avaia_xp: xp_column(row.try_get("avaia_xp")?)?,
            }),
        }
    }

    /// Folds a carry and a batch of events into the shared totals.
    ///
    /// Both numbers are the owner's assertion, not an attestation of play.
    /// Carry only ever rises: the greatest pre-sync total a device reports
    /// becomes the baseline, and a smaller report later does not lower it.
    /// An event id that was already stored is ignored, so a retry is not
    /// added twice. The id and the caps are not evidence the amount was
    /// earned. The returned totals are the report `pub_info` redistributes.
    pub async fn publish_experience(
        &self,
        owner: &PubDress,
        carry: Option<PubInfoExperience>,
        events: &[ExperienceAward],
        now: u64,
    ) -> Result<PubInfoExperience, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO bond_pub_info \
             (owner_pub_dress, carried_bond_xp, carried_avaia_xp, bond_xp, avaia_xp, updated_at) \
             VALUES (?, 0, 0, 0, 0, ?) \
             ON CONFLICT(owner_pub_dress) DO NOTHING",
        )
        .bind(owner.as_str())
        .bind(i64::try_from(now).map_err(|_| RepositoryError::CorruptPubInfo)?)
        .execute(&mut *transaction)
        .await?;

        if let Some(carry) = carry {
            apply_carry(&mut transaction, owner, carry, now).await?;
        }
        for event in events {
            apply_event(&mut transaction, owner, event, now).await?;
        }
        let experience = read_pub_info_in(&mut transaction, owner).await?;
        transaction.commit().await?;
        Ok(experience)
    }
}

fn xp_column(value: i64) -> Result<u64, RepositoryError> {
    u64::try_from(value).map_err(|_| RepositoryError::CorruptPubInfo)
}

fn xp_param(value: u64) -> Result<i64, RepositoryError> {
    i64::try_from(value).map_err(|_| RepositoryError::CorruptPubInfo)
}

async fn apply_carry(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
    carry: PubInfoExperience,
    now: u64,
) -> Result<(), RepositoryError> {
    let carried_bond = xp_param(carry.bond_xp)?;
    let carried_avaia = xp_param(carry.avaia_xp)?;
    let now = xp_param(now)?;
    // The increase is the part of this report the baseline has not absorbed.
    // A report at or below the baseline changes nothing.
    sqlx::query(
        "UPDATE bond_pub_info SET \
            bond_xp = bond_xp + MAX(0, ?1 - carried_bond_xp), \
            avaia_xp = avaia_xp + MAX(0, ?2 - carried_avaia_xp), \
            carried_bond_xp = MAX(carried_bond_xp, ?1), \
            carried_avaia_xp = MAX(carried_avaia_xp, ?2), \
            updated_at = ?3 \
         WHERE owner_pub_dress = ?4 \
           AND (carried_bond_xp < ?1 OR carried_avaia_xp < ?2)",
    )
    .bind(carried_bond)
    .bind(carried_avaia)
    .bind(now)
    .bind(owner.as_str())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn apply_event(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
    event: &ExperienceAward,
    now: u64,
) -> Result<(), RepositoryError> {
    let inserted = sqlx::query(
        "INSERT INTO bond_experience_events (owner_pub_dress, event_id, earner, amount) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(owner_pub_dress, event_id) DO NOTHING",
    )
    .bind(owner.as_str())
    .bind(&event.id)
    .bind(event.earner.as_str())
    .bind(xp_param(event.amount)?)
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Ok(());
    }
    let amount = xp_param(event.amount)?;
    let now = xp_param(now)?;
    let update = match event.earner {
        ExperienceEarner::Bond => {
            sqlx::query(
                "UPDATE bond_pub_info SET bond_xp = bond_xp + ?, updated_at = ? \
                 WHERE owner_pub_dress = ?",
            )
        }
        ExperienceEarner::Avaia => {
            sqlx::query(
                "UPDATE bond_pub_info SET avaia_xp = avaia_xp + ?, updated_at = ? \
                 WHERE owner_pub_dress = ?",
            )
        }
    };
    update
        .bind(amount)
        .bind(now)
        .bind(owner.as_str())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn read_pub_info_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
) -> Result<PubInfoExperience, RepositoryError> {
    let row = sqlx::query(
        "SELECT bond_xp, avaia_xp FROM bond_pub_info WHERE owner_pub_dress = ?",
    )
    .bind(owner.as_str())
    .fetch_one(&mut **transaction)
    .await?;
    Ok(PubInfoExperience {
        bond_xp: xp_column(row.try_get("bond_xp")?)?,
        avaia_xp: xp_column(row.try_get("avaia_xp")?)?,
    })
}

#[cfg(test)]
mod pub_info_tests {
    use super::{ExperienceAward, ExperienceEarner, IdentityRepository, PubInfoExperience};
    use crate::{ProviderIdentity, PubDress};

    async fn repository() -> (IdentityRepository, PubDress) {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner: PubDress = "0x1sky".parse().expect("owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(7), 100)
            .await
            .expect("registration");
        (repository, owner)
    }

    fn award(id: &str, earner: ExperienceEarner, amount: u64) -> ExperienceAward {
        ExperienceAward {
            id: id.to_owned(),
            earner,
            amount,
        }
    }

    #[tokio::test]
    async fn an_unpublished_bond_reads_as_zero() {
        let (repository, owner) = repository().await;
        let experience = repository.read_pub_info(&owner).await.expect("read");
        assert_eq!(experience, PubInfoExperience::zero());
    }

    #[tokio::test]
    async fn carry_rises_to_the_greatest_report_and_an_event_pays_once() {
        let (repository, owner) = repository().await;
        let first = repository
            .publish_experience(
                &owner,
                Some(PubInfoExperience {
                    bond_xp: 30,
                    avaia_xp: 10,
                }),
                &[award("xp:zone", ExperienceEarner::Bond, 30)],
                200,
            )
            .await
            .expect("publish");
        assert_eq!(
            first,
            PubInfoExperience {
                bond_xp: 60,
                avaia_xp: 10
            }
        );

        let replay = repository
            .publish_experience(
                &owner,
                Some(PubInfoExperience {
                    bond_xp: 10,
                    avaia_xp: 0,
                }),
                &[
                    award("xp:zone", ExperienceEarner::Bond, 30),
                    award("xp:study", ExperienceEarner::Avaia, 45),
                ],
                300,
            )
            .await
            .expect("replay");
        // The smaller carry does not lower the baseline, the repeated event
        // does not pay again, and the new event does.
        assert_eq!(
            replay,
            PubInfoExperience {
                bond_xp: 60,
                avaia_xp: 55
            }
        );
        assert_eq!(
            repository.read_pub_info(&owner).await.expect("read"),
            replay
        );
    }

    #[tokio::test]
    async fn a_higher_carry_from_another_device_is_absorbed_once() {
        let (repository, owner) = repository().await;
        repository
            .publish_experience(
                &owner,
                Some(PubInfoExperience {
                    bond_xp: 40,
                    avaia_xp: 0,
                }),
                &[],
                200,
            )
            .await
            .expect("first device");
        let merged = repository
            .publish_experience(
                &owner,
                Some(PubInfoExperience {
                    bond_xp: 90,
                    avaia_xp: 15,
                }),
                &[],
                300,
            )
            .await
            .expect("second device");
        assert_eq!(
            merged,
            PubInfoExperience {
                bond_xp: 90,
                avaia_xp: 15
            }
        );
        let again = repository
            .publish_experience(
                &owner,
                Some(PubInfoExperience {
                    bond_xp: 90,
                    avaia_xp: 15,
                }),
                &[],
                400,
            )
            .await
            .expect("same carry again");
        assert_eq!(again, merged);
    }
}
