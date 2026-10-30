// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// One verified relationship from a human Bond to a Business Bond.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BusinessMembership {
    pub business_pub_dress: String,
    pub role: String,
    pub created_at: i64,
}

impl IdentityRepository {
    /// Read only the caller's own memberships. Never trust a caller-provided
    /// address: API handlers must derive `member` from their authenticated Bond.
    pub async fn business_memberships_for(
        &self,
        member: &PubDress,
    ) -> Result<Vec<BusinessMembership>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT m.business_pub_dress, m.role, m.created_at
             FROM business_memberships m
             JOIN identities b ON b.pub_dress = m.business_pub_dress
             JOIN bond_roles r ON r.pub_dress = b.pub_dress AND r.role = 'business'
             WHERE m.member_pub_dress = ? AND b.identity_kind = 'human'
             ORDER BY m.business_pub_dress COLLATE BINARY",
        )
        .bind(member.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| BusinessMembership {
                business_pub_dress: row.get("business_pub_dress"),
                role: row.get("role"),
                created_at: row.get("created_at"),
            })
            .collect())
    }
}

#[cfg(test)]
mod business_membership_read_tests {
    use super::*;

    #[tokio::test]
    async fn organizations_are_private_sorted_and_follow_a_rename() {
        let database = tempfile::NamedTempFile::new().expect("database");
        let url = format!("sqlite://{}", database.path().display());
        let repository = IdentityRepository::connect(&url).await.expect("repository");
        for (name, id) in [("0x1alice", 101), ("0x1bob", 102), ("0x1market", 103), ("0x1agency", 104)] {
            let address: PubDress = name.parse().expect("address");
            repository.register(&address, &ProviderIdentity::telegram(id), 100)
                .await.expect("registration");
        }
        for name in ["0x1market", "0x1agency"] {
            repository.set_role(&name.parse().expect("address"), BondAccessRole::Business)
                .await.expect("business");
        }
        for (business, member, role) in [
            ("0x1market", "0x1alice", "owner"),
            ("0x1agency", "0x1alice", "member"),
            ("0x1market", "0x1bob", "maintainer"),
        ] {
            sqlx::query(
                "INSERT INTO business_memberships
                 (business_pub_dress, member_pub_dress, role, created_at)
                 VALUES (?, ?, ?, 100)"
            )
            .bind(business).bind(member).bind(role)
            .execute(&repository.pool).await.expect("membership");
        }
        let alice: PubDress = "0x1alice".parse().unwrap();
        let bob: PubDress = "0x1bob".parse().unwrap();
        let read = repository.business_memberships_for(&alice).await.unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].business_pub_dress, "0x1agency");
        assert_eq!(read[0].role, "member");
        assert_eq!(read[1].business_pub_dress, "0x1market");
        assert_eq!(read[1].role, "owner");
        assert_eq!(repository.business_memberships_for(&bob).await.unwrap().len(), 1);
        let outsider: PubDress = "0x1other".parse().unwrap();
        assert!(repository.business_memberships_for(&outsider).await.unwrap().is_empty());

        let next: PubDress = "0x1renamed".parse().unwrap();
        assert!(matches!(
            repository.rename_pub_dress(&alice, &next).await.unwrap(),
            PubDressRenameOutcome::Renamed(_)
        ));
        assert!(repository.business_memberships_for(&alice).await.unwrap().is_empty());
        assert_eq!(repository.business_memberships_for(&next).await.unwrap().len(), 2);
        repository.pool.close().await;
        let reopened = IdentityRepository::connect(&url).await.unwrap();
        assert_eq!(reopened.business_memberships_for(&next).await.unwrap().len(), 2);
    }
}
