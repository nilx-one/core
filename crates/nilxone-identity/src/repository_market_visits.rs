// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

// Market visits bind an opaque reference to the Bond and the active session
// that opened it. Only the issuer worker reads them; no route exposes them.

/// What a visit lets the issuer worker check before a new debit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketVisit {
    /// The Bond's current handle; follows a rename.
    pub owner: PubDress,
    /// The handle Market was told, and so the one its order carries.
    pub recipient_pub_dress: String,
    pub session_token_hash: Vec<u8>,
    pub spend_until_ms: u64,
}

/// An already committed Special debit and grant, keyed by Market order id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecialSeedsReceipt {
    pub owner_pub_dress: String,
    pub sku: String,
    pub price_id: u64,
    pub amount_minor: u64,
    pub duration_ms: u64,
}

impl IdentityRepository {
    pub async fn create_market_visit(
        &self,
        visit_id: &str,
        owner: &PubDress,
        session_token_hash: &[u8],
        created_at_ms: u64,
        spend_until_ms: u64,
    ) -> Result<(), RepositoryError> {
        let (Ok(created), Ok(until)) = (i64::try_from(created_at_ms), i64::try_from(spend_until_ms)) else {
            return Err(RepositoryError::CorruptSeedsBalance);
        };
        sqlx::query(
            "INSERT INTO market_visits \
             (visit_id, owner_pub_dress, recipient_pub_dress, session_token_hash, created_at_ms, spend_until_ms) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(visit_id)
        .bind(owner.as_str())
        .bind(owner.as_str())
        .bind(session_token_hash)
        .bind(created)
        .bind(until)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_market_visit(&self, visit_id: &str) -> Result<Option<MarketVisit>, RepositoryError> {
        let row = sqlx::query(
            "SELECT owner_pub_dress, recipient_pub_dress, session_token_hash, spend_until_ms \
             FROM market_visits WHERE visit_id = ?",
        )
        .bind(visit_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        let owner = PubDress::from_str(&row.try_get::<String, _>("owner_pub_dress")?)
            .map_err(|_| RepositoryError::CorruptHumanPubDress)?;
        let spend_until_ms = u64::try_from(row.try_get::<i64, _>("spend_until_ms")?)
            .map_err(|_| RepositoryError::CorruptSeedsBalance)?;
        Ok(Some(MarketVisit {
            owner,
            recipient_pub_dress: row.try_get("recipient_pub_dress")?,
            session_token_hash: row.try_get("session_token_hash")?,
            spend_until_ms,
        }))
    }

    pub async fn find_special_seeds_receipt(
        &self,
        order_id: &str,
    ) -> Result<Option<SpecialSeedsReceipt>, RepositoryError> {
        let row = sqlx::query(
            "SELECT owner_pub_dress, sku, price_id, amount_minor, duration_ms \
             FROM bond_seed_subscription_debits WHERE order_id = ?",
        )
        .bind(order_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        let unsigned = |value: i64| u64::try_from(value).map_err(|_| RepositoryError::CorruptSeedsBalance);
        Ok(Some(SpecialSeedsReceipt {
            owner_pub_dress: row.try_get("owner_pub_dress")?,
            sku: row.try_get("sku")?,
            price_id: unsigned(row.try_get("price_id")?)?,
            amount_minor: unsigned(row.try_get("amount_minor")?)?,
            duration_ms: unsigned(row.try_get("duration_ms")?)?,
        }))
    }
}
