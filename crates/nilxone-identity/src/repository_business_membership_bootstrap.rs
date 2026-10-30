// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// Operator-only grants to existing businesses. Not a general membership API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusinessBootstrapRole {
    Owner,
    Maintainer,
}
impl BusinessBootstrapRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Maintainer => "maintainer",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusinessMembershipBootstrapOutcome {
    Created,
    AlreadyPresent,
    MemberNotHuman,
    BusinessNotFound,
    SameBond,
    RoleConflict,
}

impl IdentityRepository {
    /// Explicit first-member grant for a vetted operator only. The binary
    /// command never surfaces this as a public route or runs it at startup.
    /// The transaction cannot overwrite an existing membership or owner.
    pub async fn bootstrap_business_membership(
        &self,
        member: &PubDress,
        business: &PubDress,
        role: BusinessBootstrapRole,
        now: u64,
    ) -> Result<BusinessMembershipBootstrapOutcome, RepositoryError> {
        if member == business {
            return Ok(BusinessMembershipBootstrapOutcome::SameBond);
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let member_is_human: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM identities i LEFT JOIN bond_roles r ON i.pub_dress = r.pub_dress
                WHERE i.pub_dress = ? AND i.identity_kind = 'human'
                AND COALESCE(r.role, 'user') != 'business'
            )",
        )
        .bind(member.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if !member_is_human {
            transaction.rollback().await?;
            return Ok(BusinessMembershipBootstrapOutcome::MemberNotHuman);
        }
        let business_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM identities i JOIN bond_roles r ON i.pub_dress = r.pub_dress
                WHERE i.pub_dress = ? AND i.identity_kind = 'human' AND r.role = 'business'
            )",
        )
        .bind(business.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if !business_exists {
            transaction.rollback().await?;
            return Ok(BusinessMembershipBootstrapOutcome::BusinessNotFound);
        }
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT role FROM business_memberships
             WHERE business_pub_dress = ? AND member_pub_dress = ?",
        )
        .bind(business.as_str())
        .bind(member.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(existing) = existing {
            transaction.rollback().await?;
            return Ok(if existing == role.as_str() {
                BusinessMembershipBootstrapOutcome::AlreadyPresent
            } else {
                BusinessMembershipBootstrapOutcome::RoleConflict
            });
        }
        sqlx::query(
            "INSERT INTO business_memberships
             (business_pub_dress, member_pub_dress, role, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(business.as_str())
        .bind(member.as_str())
        .bind(role.as_str())
        .bind(now as i64)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(BusinessMembershipBootstrapOutcome::Created)
    }
}

#[cfg(test)]
mod membership_operator_bootstrap_tests {
    use super::*;

    #[tokio::test]
    async fn operator_grant_checks_both_bonds_and_never_overwrites_roles() {
        let repository = IdentityRepository::connect("sqlite::memory:").await.unwrap();
        let person: PubDress = "0x1alice".parse().unwrap();
        let business: PubDress = "0xda-market".parse().unwrap();
        assert_eq!(
            repository.bootstrap_business_membership(&person, &business, BusinessBootstrapRole::Owner, 1).await.unwrap(),
            BusinessMembershipBootstrapOutcome::MemberNotHuman
        );
        repository.register(&person, &ProviderIdentity::telegram(991), 1).await.unwrap();
        assert_eq!(
            repository.bootstrap_business_membership(&person, &business, BusinessBootstrapRole::Owner, 2).await.unwrap(),
            BusinessMembershipBootstrapOutcome::BusinessNotFound
        );
        repository.register_business(&business, 2).await.unwrap();
        assert_eq!(
            repository.bootstrap_business_membership(&person, &business, BusinessBootstrapRole::Owner, 3).await.unwrap(),
            BusinessMembershipBootstrapOutcome::Created
        );
        assert_eq!(
            repository.bootstrap_business_membership(&person, &business, BusinessBootstrapRole::Owner, 4).await.unwrap(),
            BusinessMembershipBootstrapOutcome::AlreadyPresent
        );
        assert_eq!(
            repository.bootstrap_business_membership(&person, &business, BusinessBootstrapRole::Maintainer, 5).await.unwrap(),
            BusinessMembershipBootstrapOutcome::RoleConflict
        );
        let role: String = sqlx::query_scalar(
            "SELECT role FROM business_memberships WHERE business_pub_dress = ? AND member_pub_dress = ?"
        ).bind(business.as_str()).bind(person.as_str())
         .fetch_one(&repository.pool).await.unwrap();
        assert_eq!(role, "owner");
        assert_eq!(
            repository.bootstrap_business_membership(&business, &business, BusinessBootstrapRole::Owner, 6).await.unwrap(),
            BusinessMembershipBootstrapOutcome::SameBond
        );
    }
}
