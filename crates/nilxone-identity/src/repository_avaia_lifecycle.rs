// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvaiaConfigurationState {
    Unconfigured,
    Configured,
}

impl AvaiaConfigurationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unconfigured => "unconfigured",
            Self::Configured => "configured",
        }
    }

    fn from_storage(value: &str) -> Option<Self> {
        match value {
            "unconfigured" => Some(Self::Unconfigured),
            "configured" => Some(Self::Configured),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvaiaIdentityRecord {
    pub pub_dress: String,
    pub owner_pub_dress: String,
    pub configuration_state: AvaiaConfigurationState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AvaiaUpdateOutcome {
    Updated(AvaiaIdentityRecord),
    AvaiaUnavailable,
    OwnerDiscriminatorMismatch,
    Unknown,
}

/// Owner-published location for the Avaia a human Bond owns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvaiaLocation {
    pub coordinate: GeoCoordinate,
    pub updated_at: DecimalU64,
    /// Increments only when an administrator explicitly travels with Avaia.
    pub travel_revision: u64,
}

impl AvaiaLocation {
    #[must_use]
    pub const fn new(coordinate: GeoCoordinate, updated_at: DecimalU64) -> Self {
        Self {
            coordinate,
            updated_at,
            travel_revision: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvaiaTravelOutcome {
    Arrived,
    AdminRequired,
    AvaiaUnavailable,
}

impl IdentityRepository {
    /// Applies the additive Avaia configuration schema. The lifecycle guard
    /// itself is installed by the always-run Avaia location migration so Bond
    /// registration is protected before any profile endpoint is read.
    pub async fn initialize_avaia_configuration(&self) -> Result<(), RepositoryError> {
        sqlx::raw_sql(include_str!("../migrations/0007_avaia_configuration.sql"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn owned_avaia_identity(
        &self,
        owner: &PubDress,
    ) -> Result<Option<AvaiaIdentityRecord>, RepositoryError> {
        self.initialize_avaia_configuration().await?;
        let row = sqlx::query(
            "SELECT avaia_pub_dress, owner_pub_dress, configuration_state \
             FROM avaia_configuration WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(avaia_identity_from_row).transpose()
    }

    /// The only path that may create an Avaia identity. The repository installs
    /// a transaction-local intent before calling the shared legacy insertion
    /// helper; the database lifecycle guard rejects every other insertion.
    pub async fn configure_owned_avaia(
        &self,
        owner: &PubDress,
        next: &AvaiaPubDress,
        now: u64,
    ) -> Result<AvaiaUpdateOutcome, RepositoryError> {
        if next.owner_discriminator() != owner.discriminator() {
            return Ok(AvaiaUpdateOutcome::OwnerDiscriminatorMismatch);
        }

        self.initialize_avaia_configuration().await?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let human_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities \
             WHERE pub_dress = ? AND identity_kind = 'human')",
        )
        .bind(owner.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if !human_exists {
            transaction.rollback().await?;
            return Ok(AvaiaUpdateOutcome::Unknown);
        }

        sqlx::query(
            "INSERT INTO avaia_creation_intents (owner_pub_dress) VALUES (?) \
             ON CONFLICT(owner_pub_dress) DO NOTHING",
        )
        .bind(owner.as_str())
        .execute(&mut *transaction)
        .await?;

        if owned_avaia_for_owner_in(&mut transaction, owner.as_str())
            .await?
            .is_none()
        {
            let Some(_) = create_owned_avaia_in(&mut transaction, owner, now).await? else {
                transaction.rollback().await?;
                return Ok(AvaiaUpdateOutcome::AvaiaUnavailable);
            };
        }

        let Some(current) = avaia_identity_for_owner_in(&mut transaction, owner.as_str()).await?
        else {
            transaction.rollback().await?;
            return Ok(AvaiaUpdateOutcome::AvaiaUnavailable);
        };
        if current.pub_dress == next.as_str() {
            sqlx::query(
                "UPDATE avaia_configuration SET configuration_state = 'configured' \
                 WHERE owner_pub_dress = ?",
            )
            .bind(owner.as_str())
            .execute(&mut *transaction)
            .await?;
            let Some(record) =
                avaia_identity_for_owner_in(&mut transaction, owner.as_str()).await?
            else {
                transaction.rollback().await?;
                return Ok(AvaiaUpdateOutcome::AvaiaUnavailable);
            };
            sqlx::query("DELETE FROM avaia_creation_intents WHERE owner_pub_dress = ?")
                .bind(owner.as_str())
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            return Ok(AvaiaUpdateOutcome::Updated(record));
        }

        let updated = sqlx::query(
            "UPDATE OR IGNORE identities SET pub_dress = ? \
             WHERE identity_kind = 'avaia' AND owner_pub_dress = ?",
        )
        .bind(next.as_str())
        .bind(owner.as_str())
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(AvaiaUpdateOutcome::AvaiaUnavailable);
        }

        sqlx::query(
            "UPDATE avaia_configuration SET configuration_state = 'configured' \
             WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .execute(&mut *transaction)
        .await?;
        let Some(record) = avaia_identity_for_owner_in(&mut transaction, owner.as_str()).await?
        else {
            transaction.rollback().await?;
            return Ok(AvaiaUpdateOutcome::AvaiaUnavailable);
        };
        sqlx::query("DELETE FROM avaia_creation_intents WHERE owner_pub_dress = ?")
            .bind(owner.as_str())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(AvaiaUpdateOutcome::Updated(record))
    }

    pub async fn read_avaia_location(
        &self,
        owner: &PubDress,
    ) -> Result<Option<AvaiaLocation>, RepositoryError> {
        let row = sqlx::query(
            "SELECT longitude_e7, latitude_e7, updated_at, travel_revision \
             FROM avaia_locations WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };

        let longitude_e7 = i32::try_from(row.get::<i64, _>("longitude_e7"))
            .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
        let latitude_e7 = i32::try_from(row.get::<i64, _>("latitude_e7"))
            .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
        let coordinate = GeoCoordinate::new(longitude_e7, latitude_e7)
            .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
        let updated_at = u64::try_from(row.get::<i64, _>("updated_at"))
            .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
        let travel_revision = u64::try_from(row.get::<i64, _>("travel_revision"))
            .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
        Ok(Some(AvaiaLocation {
            coordinate,
            updated_at: DecimalU64::new(updated_at),
            travel_revision,
        }))
    }

    pub async fn write_avaia_location(
        &self,
        owner: &PubDress,
        location: AvaiaLocation,
    ) -> Result<(), RepositoryError> {
        let updated_at = i64::try_from(location.updated_at.get())
            .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
        sqlx::query(
            "INSERT INTO avaia_locations \
             (owner_pub_dress, longitude_e7, latitude_e7, updated_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(owner_pub_dress) DO UPDATE SET \
               longitude_e7 = excluded.longitude_e7, \
               latitude_e7 = excluded.latitude_e7, \
               updated_at = excluded.updated_at",
        )
        .bind(owner.as_str())
        .bind(i64::from(location.coordinate.longitude_e7()))
        .bind(i64::from(location.coordinate.latitude_e7()))
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Manual Telegram travel declares both positions atomically.
    pub async fn travel_with_avaia(
        &self,
        owner: &PubDress,
        coordinate: GeoCoordinate,
        at: DecimalU64,
    ) -> Result<AvaiaTravelOutcome, RepositoryError> {
        self.travel_avaia(owner, coordinate, at, true).await
    }

    /// A Bond physically present at the GPS fix only brings its Avaia.
    /// No declared/manual Bond position is written.
    pub async fn bring_avaia_to_bond(
        &self,
        owner: &PubDress,
        coordinate: GeoCoordinate,
        at: DecimalU64,
    ) -> Result<AvaiaTravelOutcome, RepositoryError> {
        self.travel_avaia(owner, coordinate, at, false).await
    }

    async fn travel_avaia(
        &self,
        owner: &PubDress,
        coordinate: GeoCoordinate,
        at: DecimalU64,
        declare_bond: bool,
    ) -> Result<AvaiaTravelOutcome, RepositoryError> {
        self.initialize_avaia_configuration().await?;
        let at = i64::try_from(at.get())
            .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        // Recheck the effective stored role INSIDE the write transaction.
        let role = sqlx::query_scalar::<_, String>(
            "SELECT CASE WHEN i.identity_kind = 'human' AND substr(i.pub_dress, 1, 3) = '0x0' \
             THEN 'admin' ELSE COALESCE(r.role, 'user') END \
             FROM identities i LEFT JOIN bond_roles r ON r.pub_dress = i.pub_dress \
             WHERE i.pub_dress = ? AND i.identity_kind = 'human'",
        )
        .bind(owner.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        if role.as_deref() != Some("admin") {
            return Ok(AvaiaTravelOutcome::AdminRequired);
        }
        let has_avaia = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM avaia_configuration c \
             JOIN identities a ON a.pub_dress = c.avaia_pub_dress \
             WHERE c.owner_pub_dress = ? AND a.identity_kind = 'avaia')",
        )
        .bind(owner.as_str())
        .fetch_one(&mut *tx)
        .await?;
        if !has_avaia {
            return Ok(AvaiaTravelOutcome::AvaiaUnavailable);
        }

        if declare_bond {
            sqlx::query(
                "INSERT INTO bond_locations \
                 (pub_dress, mode, longitude_e7, latitude_e7, updated_at) \
                 VALUES (?, 'manual', ?, ?, ?) \
                 ON CONFLICT(pub_dress) DO UPDATE SET mode = 'manual', \
                 longitude_e7 = excluded.longitude_e7, latitude_e7 = excluded.latitude_e7, \
                 updated_at = excluded.updated_at",
            )
            .bind(owner.as_str())
            .bind(i64::from(coordinate.longitude_e7()))
            .bind(i64::from(coordinate.latitude_e7()))
            .bind(at)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "INSERT INTO avaia_locations \
             (owner_pub_dress, longitude_e7, latitude_e7, updated_at, travel_revision) \
             VALUES (?, ?, ?, ?, 1) \
             ON CONFLICT(owner_pub_dress) DO UPDATE SET \
             longitude_e7 = excluded.longitude_e7, latitude_e7 = excluded.latitude_e7, \
             updated_at = excluded.updated_at, \
             travel_revision = avaia_locations.travel_revision + 1",
        )
        .bind(owner.as_str())
        .bind(i64::from(coordinate.longitude_e7()))
        .bind(i64::from(coordinate.latitude_e7()))
        .bind(at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(AvaiaTravelOutcome::Arrived)
    }
}

async fn avaia_identity_for_owner_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner_pub_dress: &str,
) -> Result<Option<AvaiaIdentityRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT avaia_pub_dress, owner_pub_dress, configuration_state \
         FROM avaia_configuration WHERE owner_pub_dress = ?",
    )
    .bind(owner_pub_dress)
    .fetch_optional(&mut **transaction)
    .await?;
    row.map(avaia_identity_from_row).transpose()
}

fn avaia_identity_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<AvaiaIdentityRecord, RepositoryError> {
    let raw_state: String = row.get("configuration_state");
    let Some(configuration_state) = AvaiaConfigurationState::from_storage(&raw_state) else {
        return Err(RepositoryError::Storage(sqlx::Error::RowNotFound));
    };
    Ok(AvaiaIdentityRecord {
        pub_dress: row.get("avaia_pub_dress"),
        owner_pub_dress: row.get("owner_pub_dress"),
        configuration_state,
    })
}

#[cfg(test)]
mod avaia_lifecycle_repository_tests {
    use super::*;

    #[tokio::test]
    async fn registration_does_not_create_avaia() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner: PubDress = "0x1sky".parse().expect("owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(8801), 100)
            .await
            .expect("registration");

        assert_eq!(repository.owned_avaia_identity(&owner).await.expect("read"), None);
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM identities WHERE identity_kind = 'avaia' AND owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_one(&repository.pool)
        .await
        .expect("count");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn explicit_create_persists_exactly_one_configured_avaia() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner: PubDress = "0x1sky".parse().expect("owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(8802), 100)
            .await
            .expect("registration");
        let requested: AvaiaPubDress = "x1newai".parse().expect("Avaia");

        let first = repository
            .configure_owned_avaia(&owner, &requested, 101)
            .await
            .expect("create");
        assert!(matches!(
            first,
            AvaiaUpdateOutcome::Updated(ref profile)
                if profile.pub_dress == "x1newai"
                    && profile.configuration_state == AvaiaConfigurationState::Configured
        ));

        let second = repository
            .configure_owned_avaia(&owner, &requested, 102)
            .await
            .expect("replay");
        assert!(matches!(second, AvaiaUpdateOutcome::Updated(_)));

        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM identities WHERE identity_kind = 'avaia' AND owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_one(&repository.pool)
        .await
        .expect("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn location_storage_does_not_imply_avaia_identity() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner: PubDress = "0x1sky".parse().expect("owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(8803), 100)
            .await
            .expect("registration");
        let coordinate = GeoCoordinate::from_degrees(30.5234, 50.4501).expect("coordinate");
        repository
            .write_avaia_location(&owner, AvaiaLocation::new(coordinate, DecimalU64::new(101)))
            .await
            .expect("location");
        assert_eq!(repository.owned_avaia_identity(&owner).await.expect("read"), None);
    }

    #[tokio::test]
    async fn travel_requires_admin_and_an_existing_avaia_without_partial_writes() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner: PubDress = "0x1sky".parse().expect("owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(8901), 100)
            .await
            .expect("registration");
        let point = GeoCoordinate::from_degrees(2.3522, 48.8566).expect("point");

        assert_eq!(
            repository
                .travel_with_avaia(&owner, point, DecimalU64::new(101))
                .await
                .expect("travel"),
            AvaiaTravelOutcome::AdminRequired
        );
        repository
            .set_role(&owner, BondAccessRole::Admin)
            .await
            .expect("role");
        assert_eq!(
            repository
                .travel_with_avaia(&owner, point, DecimalU64::new(102))
                .await
                .expect("travel"),
            AvaiaTravelOutcome::AvaiaUnavailable
        );
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM bond_locations WHERE pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_one(&repository.pool)
        .await
        .expect("bond count");
        assert_eq!(count, 0);

        let avaia: AvaiaPubDress = "x1skai".parse().expect("avaia");
        repository
            .configure_owned_avaia(&owner, &avaia, 103)
            .await
            .expect("create Avaia");
        assert_eq!(
            repository
                .travel_with_avaia(&owner, point, DecimalU64::new(104))
                .await
                .expect("travel"),
            AvaiaTravelOutcome::Arrived
        );
        let bond = sqlx::query(
            "SELECT mode, longitude_e7, latitude_e7 FROM bond_locations WHERE pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_one(&repository.pool)
        .await
        .expect("Bond coordinate");
        assert_eq!(bond.get::<String, _>("mode"), "manual");
        assert_eq!(bond.get::<i64, _>("longitude_e7"), i64::from(point.longitude_e7()));
        assert_eq!(bond.get::<i64, _>("latitude_e7"), i64::from(point.latitude_e7()));
        let arrived = repository
            .read_avaia_location(&owner)
            .await
            .expect("Avaia coordinate")
            .expect("Avaia location");
        assert_eq!(arrived.coordinate, point);
        assert_eq!(arrived.travel_revision, 1);

        let next = GeoCoordinate::from_degrees(30.5234, 50.4501).expect("next point");
        assert_eq!(
            repository
                .travel_with_avaia(&owner, next, DecimalU64::new(105))
                .await
                .expect("second journey"),
            AvaiaTravelOutcome::Arrived
        );
        assert_eq!(
            repository.read_avaia_location(&owner).await.expect("read").expect("location").travel_revision,
            2
        );
        repository
            .write_avaia_location(&owner, AvaiaLocation::new(point, DecimalU64::new(106)))
            .await
            .expect("ordinary location publication");
        assert_eq!(
            repository.read_avaia_location(&owner).await.expect("read").expect("location").travel_revision,
            2,
            "ordinary Avaia location writes do not cause another teleport"
        );
        repository
            .set_role(&owner, BondAccessRole::User)
            .await
            .expect("revoke admin");
        assert_eq!(
            repository.travel_with_avaia(&owner, point, DecimalU64::new(107)).await.expect("denied"),
            AvaiaTravelOutcome::AdminRequired
        );
    }

    #[tokio::test]
    async fn confirmed_physical_arrival_only_moves_avaia() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner: PubDress = "0x1sky".parse().expect("owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(8903), 100)
            .await
            .expect("registration");
        let point = GeoCoordinate::from_degrees(30.5234, 50.4501).expect("point");
        assert_eq!(
            repository
                .bring_avaia_to_bond(&owner, point, DecimalU64::new(101))
                .await
                .expect("role denial"),
            AvaiaTravelOutcome::AdminRequired
        );
        repository
            .set_role(&owner, BondAccessRole::Admin)
            .await
            .expect("role");
        assert_eq!(
            repository
                .bring_avaia_to_bond(&owner, point, DecimalU64::new(102))
                .await
                .expect("missing Avaia"),
            AvaiaTravelOutcome::AvaiaUnavailable
        );
        let avaia: AvaiaPubDress = "x1skai".parse().expect("avaia");
        repository
            .configure_owned_avaia(&owner, &avaia, 103)
            .await
            .expect("configure");
        assert_eq!(
            repository
                .bring_avaia_to_bond(&owner, point, DecimalU64::new(104))
                .await
                .expect("arrival"),
            AvaiaTravelOutcome::Arrived
        );
        let bond_rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM bond_locations WHERE pub_dress = ?")
                .bind(owner.as_str())
                .fetch_one(&repository.pool)
                .await
                .expect("bond rows");
        assert_eq!(bond_rows, 0, "GPS travel must not declare a manual Bond position");
        let arrived = repository
            .read_avaia_location(&owner)
            .await
            .expect("read")
            .expect("Avaia at destination");
        assert_eq!(arrived.coordinate, point);
        assert_eq!(arrived.travel_revision, 1);
        repository
            .set_role(&owner, BondAccessRole::User)
            .await
            .expect("revoke admin");
        assert_eq!(
            repository
                .bring_avaia_to_bond(&owner, point, DecimalU64::new(105))
                .await
                .expect("denied again"),
            AvaiaTravelOutcome::AdminRequired
        );
        assert_eq!(
            repository
                .read_avaia_location(&owner)
                .await
                .expect("read")
                .expect("location")
                .travel_revision,
            1
        );
    }

    #[tokio::test]
    async fn failing_avaia_write_rolls_back_bond_teleport() {
        let repository = IdentityRepository::connect("sqlite::memory:").await.expect("repository");
        let owner: PubDress = "0x1sky".parse().expect("owner");
        repository.register(&owner, &ProviderIdentity::telegram(8902), 100).await.expect("registration");
        repository.set_role(&owner, BondAccessRole::Admin).await.expect("admin");
        let avaia: AvaiaPubDress = "x1skai".parse().expect("avaia");
        repository.configure_owned_avaia(&owner, &avaia, 101).await.expect("create");
        sqlx::query(
            "CREATE TRIGGER block_travel BEFORE INSERT ON avaia_locations \
             BEGIN SELECT RAISE(ABORT, 'blocked'); END"
        )
        .execute(&repository.pool)
        .await
        .expect("failure injection");
        let point = GeoCoordinate::from_degrees(2.3522, 48.8566).expect("point");
        assert!(repository.travel_with_avaia(&owner, point, DecimalU64::new(102)).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bond_locations WHERE pub_dress = ?")
            .bind(owner.as_str())
            .fetch_one(&repository.pool)
            .await
            .expect("bond count");
        assert_eq!(count, 0, "failed journey never relocates Bond only");
        assert!(repository.read_avaia_location(&owner).await.expect("read").is_none());
    }
}
