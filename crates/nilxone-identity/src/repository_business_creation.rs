// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// An authenticated human creates an independent Business Bond in one
/// transaction. Existing businesses can only be claimed out of band.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateBusinessOrganizationOutcome {
    Created,
    IdempotentReplay,
    CreatorNotHuman,
    AddressUnavailable,
    PublicLabelUnavailable,
}

impl IdentityRepository {
    pub async fn create_business_organization(
        &self,
        creator: &PubDress,
        business: &PubDress,
        creator_role: &str,
        now: u64,
    ) -> Result<CreateBusinessOrganizationOutcome, RepositoryError> {
        // Validate at the repository boundary, not merely in the HTTP handler.
        if business.discriminator() == '0'
            || !matches!(creator_role, "owner" | "maintainer")
        {
            return Ok(CreateBusinessOrganizationOutcome::AddressUnavailable);
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let is_person: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM identities i
                LEFT JOIN bond_roles r ON r.pub_dress = i.pub_dress
                WHERE i.pub_dress = ? AND i.identity_kind = 'human'
                  AND COALESCE(r.role, 'user') != 'business'
            )",
        )
        .bind(creator.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if !is_person {
            transaction.rollback().await?;
            return Ok(CreateBusinessOrganizationOutcome::CreatorNotHuman);
        }
        // A collision cannot become a membership grant. An exact replay is
        // read-only, possible only if the caller already holds that role.
        let occupied: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM identities WHERE pub_dress = ?)",
        )
        .bind(business.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if occupied {
            let exact_replay: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM business_memberships m
                    JOIN bond_roles r ON r.pub_dress = m.business_pub_dress
                    WHERE m.business_pub_dress = ? AND m.member_pub_dress = ?
                      AND m.role = ? AND r.role = 'business'
                )",
            )
            .bind(business.as_str())
            .bind(creator.as_str())
            .bind(creator_role)
            .fetch_one(&mut *transaction)
            .await?;
            transaction.rollback().await?;
            return Ok(if exact_replay {
                CreateBusinessOrganizationOutcome::IdempotentReplay
            } else {
                CreateBusinessOrganizationOutcome::AddressUnavailable
            });
        }
        match insert_human_with_public_label_in(&mut transaction, business, now).await? {
            HumanIdentityInsertOutcome::Inserted => {}
            HumanIdentityInsertOutcome::PubDressUnavailable => {
                transaction.rollback().await?;
                return Ok(CreateBusinessOrganizationOutcome::AddressUnavailable);
            }
            HumanIdentityInsertOutcome::PublicLabelUnavailable => {
                transaction.rollback().await?;
                return Ok(CreateBusinessOrganizationOutcome::PublicLabelUnavailable);
            }
        }
        sqlx::query("INSERT INTO bond_roles (pub_dress, role) VALUES (?, 'business')")
            .bind(business.as_str())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO business_memberships
             (business_pub_dress, member_pub_dress, role, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(business.as_str())
        .bind(creator.as_str())
        .bind(creator_role)
        .bind(now as i64)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(CreateBusinessOrganizationOutcome::Created)
    }
}

#[cfg(test)]
mod business_organization_creation_tests {
    use super::*;

    #[tokio::test]
    async fn creation_is_atomic_replay_safe_and_never_claims_another_business() {
        let repository = IdentityRepository::connect("sqlite::memory:").await.unwrap();
        let creator: PubDress = "0x1founder".parse().unwrap();
        let outsider: PubDress = "0x1outsider".parse().unwrap();
        let business: PubDress = "0xda-company".parse().unwrap();
        assert_eq!(
            repository.create_business_organization(&creator, &business, "owner", 1).await.unwrap(),
            CreateBusinessOrganizationOutcome::CreatorNotHuman
        );
        repository.register(&creator, &ProviderIdentity::telegram(401), 1).await.unwrap();
        repository.register(&outsider, &ProviderIdentity::telegram(402), 1).await.unwrap();
        assert_eq!(
            repository.create_business_organization(&creator, &business, "owner", 2).await.unwrap(),
            CreateBusinessOrganizationOutcome::Created
        );
        assert_eq!(
            repository.create_business_organization(&creator, &business, "owner", 3).await.unwrap(),
            CreateBusinessOrganizationOutcome::IdempotentReplay
        );
        assert_eq!(
            repository.create_business_organization(&outsider, &business, "owner", 4).await.unwrap(),
            CreateBusinessOrganizationOutcome::AddressUnavailable
        );
        assert_eq!(
            repository.create_business_organization(&creator, &business, "maintainer", 5).await.unwrap(),
            CreateBusinessOrganizationOutcome::AddressUnavailable
        );
        assert_eq!(
            repository.create_business_organization(&creator, &"0x0illegal".parse().unwrap(), "owner", 6).await.unwrap(),
            CreateBusinessOrganizationOutcome::AddressUnavailable
        );
        assert_eq!(
            repository.create_business_organization(&creator, &"0xdother".parse().unwrap(), "administrator", 6).await.unwrap(),
            CreateBusinessOrganizationOutcome::AddressUnavailable
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM business_memberships")
            .fetch_one(&repository.pool).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(repository.role_for(business.as_str()).await.unwrap(), BondAccessRole::Business);
    }

    #[tokio::test]
    async fn maintainer_creation_never_invents_an_owner() {
        let repository = IdentityRepository::connect("sqlite::memory:").await.unwrap();
        let creator: PubDress = "0x1founder".parse().unwrap();
        let business: PubDress = "0xda-studio".parse().unwrap();
        repository.register(&creator, &ProviderIdentity::telegram(403), 1).await.unwrap();
        assert_eq!(
            repository.create_business_organization(&creator, &business, "maintainer", 2).await.unwrap(),
            CreateBusinessOrganizationOutcome::Created
        );
        let role: String = sqlx::query_scalar(
            "SELECT role FROM business_memberships WHERE business_pub_dress = ?"
        ).bind(business.as_str()).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(role, "maintainer");
    }
}
