// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

/// A stationed Avaia: one per shop of a business Bond, standing at the shop.
#[derive(Clone, Debug, PartialEq)]
pub struct BusinessStation {
    pub pub_dress: String,
    pub owner_pub_dress: String,
    /// The caption the business names the station by, as written.
    pub caption: Option<String>,
    pub coordinate: GeoCoordinate,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BusinessStationOutcome {
    Stationed(BusinessStation),
    /// The owner is not a human Bond.
    Unknown,
    /// The owner's stored role is not `business`.
    BusinessRoleRequired,
    /// A station's address is its business's own, `-`, then a place.
    NotOwnersAddress,
    /// Another identity, or another business's station, holds the address.
    AddressUnavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BusinessRegistrationOutcome {
    Registered(IdentityRecord),
    /// The Bond already exists and is a business: registering is idempotent.
    AlreadyBusiness(IdentityRecord),
    /// Another identity holds the address, or it is reserved.
    Unavailable,
    PublicLabelUnavailable,
}

impl IdentityRepository {
    /// Registers a business Bond out of band: a human identity with the
    /// `business` role and no sign-in yet. Its owner signs in later through
    /// the ordinary paths; nothing here creates a credential or a provider.
    pub async fn register_business(
        &self,
        pub_dress: &PubDress,
        now: u64,
    ) -> Result<BusinessRegistrationOutcome, RepositoryError> {
        // Operator-owned BusinessPoints follow a separate lifecycle. Reopening
        // human 0x0 registration does not silently widen business provisioning.
        if pub_dress.discriminator() == '0' {
            return Ok(BusinessRegistrationOutcome::Unavailable);
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = sqlx::query_scalar::<_, Option<String>>(
            "SELECT r.role FROM identities i \
             LEFT JOIN bond_roles r ON r.pub_dress = i.pub_dress \
             WHERE i.pub_dress = ? AND i.identity_kind = 'human'",
        )
        .bind(pub_dress.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(role) = existing {
            if role.as_deref() != Some(BondAccessRole::Business.as_str()) {
                transaction.rollback().await?;
                return Ok(BusinessRegistrationOutcome::Unavailable);
            }
            let record = identity_for_pub_dress_in(&mut transaction, pub_dress.to_string()).await?;
            transaction.commit().await?;
            return Ok(BusinessRegistrationOutcome::AlreadyBusiness(record));
        }

        match insert_human_with_public_label_in(&mut transaction, pub_dress, now).await? {
            HumanIdentityInsertOutcome::Inserted => {}
            HumanIdentityInsertOutcome::PubDressUnavailable => {
                transaction.rollback().await?;
                return Ok(BusinessRegistrationOutcome::Unavailable);
            }
            HumanIdentityInsertOutcome::PublicLabelUnavailable => {
                transaction.rollback().await?;
                return Ok(BusinessRegistrationOutcome::PublicLabelUnavailable);
            }
        }
        sqlx::query("INSERT INTO bond_roles (pub_dress, role) VALUES (?, ?)")
            .bind(pub_dress.as_str())
            .bind(BondAccessRole::Business.as_str())
            .execute(&mut *transaction)
            .await?;
        let record = identity_for_pub_dress_in(&mut transaction, pub_dress.to_string()).await?;
        transaction.commit().await?;
        Ok(BusinessRegistrationOutcome::Registered(record))
    }

    /// The stations one business keeps. A business is many points on the map,
    /// one per shop, and no single location of its own.
    pub async fn business_stations_of(
        &self,
        owner: &PubDress,
    ) -> Result<Vec<BusinessStation>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT avaia_pub_dress, owner_pub_dress, caption, longitude_e7, latitude_e7 \
             FROM business_stations WHERE owner_pub_dress = ? \
             ORDER BY created_at, avaia_pub_dress COLLATE BINARY",
        )
        .bind(owner.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(business_station_from_row).collect()
    }

    /// Stations a business Bond's Avaia at a shop, or restates the point (and
    /// caption, when given) of a station it already keeps. There is no public
    /// route to this: like a role, a station is assigned out of band.
    pub async fn station_business_avaia(
        &self,
        owner: &PubDress,
        station: &StationPubDress,
        caption: Option<&str>,
        coordinate: GeoCoordinate,
        now: u64,
    ) -> Result<BusinessStationOutcome, RepositoryError> {
        if station.owner() != owner {
            return Ok(BusinessStationOutcome::NotOwnersAddress);
        }
        let now = i64::try_from(now).map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        let role = sqlx::query_scalar::<_, Option<String>>(
            "SELECT r.role FROM identities i \
             LEFT JOIN bond_roles r ON r.pub_dress = i.pub_dress \
             WHERE i.pub_dress = ? AND i.identity_kind = 'human'",
        )
        .bind(owner.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        match role {
            None => {
                transaction.rollback().await?;
                return Ok(BusinessStationOutcome::Unknown);
            }
            Some(role) if role.as_deref() != Some(BondAccessRole::Business.as_str()) => {
                transaction.rollback().await?;
                return Ok(BusinessStationOutcome::BusinessRoleRequired);
            }
            Some(_) => {}
        }

        let keeper = sqlx::query_scalar::<_, String>(
            "SELECT owner_pub_dress FROM business_stations WHERE avaia_pub_dress = ?",
        )
        .bind(station.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        match keeper.as_deref() {
            Some(keeper) if keeper != owner.as_str() => {
                transaction.rollback().await?;
                return Ok(BusinessStationOutcome::AddressUnavailable);
            }
            Some(_) => {
                sqlx::query(
                    "UPDATE business_stations SET longitude_e7 = ?, latitude_e7 = ?, \
                     caption = COALESCE(?, caption) WHERE avaia_pub_dress = ?",
                )
                .bind(i64::from(coordinate.longitude_e7()))
                .bind(i64::from(coordinate.latitude_e7()))
                .bind(caption)
                .bind(station.as_str())
                .execute(&mut *transaction)
                .await?;
            }
            None => {
                sqlx::query("INSERT INTO business_station_intents (avaia_pub_dress) VALUES (?)")
                    .bind(station.as_str())
                    .execute(&mut *transaction)
                    .await?;
                let inserted = sqlx::query(
                    "INSERT INTO identities (pub_dress, identity_kind, owner_pub_dress, created_at) \
                     VALUES (?, 'avaia', NULL, ?) ON CONFLICT DO NOTHING",
                )
                .bind(station.as_str())
                .bind(now)
                .execute(&mut *transaction)
                .await?;
                if inserted.rows_affected() == 0 {
                    transaction.rollback().await?;
                    return Ok(BusinessStationOutcome::AddressUnavailable);
                }
                sqlx::query(
                    "INSERT INTO business_stations \
                     (avaia_pub_dress, owner_pub_dress, caption, longitude_e7, latitude_e7, \
                      created_at) VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(station.as_str())
                .bind(owner.as_str())
                .bind(caption)
                .bind(i64::from(coordinate.longitude_e7()))
                .bind(i64::from(coordinate.latitude_e7()))
                .bind(now)
                .execute(&mut *transaction)
                .await?;
                sqlx::query("DELETE FROM business_station_intents WHERE avaia_pub_dress = ?")
                    .bind(station.as_str())
                    .execute(&mut *transaction)
                    .await?;
            }
        }

        let row = sqlx::query(
            "SELECT avaia_pub_dress, owner_pub_dress, caption, longitude_e7, latitude_e7 \
             FROM business_stations WHERE avaia_pub_dress = ?",
        )
        .bind(station.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let stationed = business_station_from_row(&row)?;
        transaction.commit().await?;
        Ok(BusinessStationOutcome::Stationed(stationed))
    }

    /// Every station, in a stable order. Stations are public: the point a
    /// shop stands at is what a business publishes for every Bond to find.
    pub async fn business_stations(&self) -> Result<Vec<BusinessStation>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT avaia_pub_dress, owner_pub_dress, caption, longitude_e7, latitude_e7 \
             FROM business_stations \
             ORDER BY owner_pub_dress COLLATE BINARY, created_at, avaia_pub_dress COLLATE BINARY",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(business_station_from_row).collect()
    }
}

fn business_station_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<BusinessStation, RepositoryError> {
    let longitude_e7 = i32::try_from(row.get::<i64, _>("longitude_e7"))
        .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
    let latitude_e7 = i32::try_from(row.get::<i64, _>("latitude_e7"))
        .map_err(|_| RepositoryError::CorruptAvaiaLocation)?;
    Ok(BusinessStation {
        pub_dress: row.get("avaia_pub_dress"),
        owner_pub_dress: row.get("owner_pub_dress"),
        caption: row.get("caption"),
        coordinate: GeoCoordinate::new(longitude_e7, latitude_e7)
            .map_err(|_| RepositoryError::CorruptAvaiaLocation)?,
    })
}


#[cfg(test)]
mod business_station_repository_tests {
    use super::*;

    async fn business(repository: &IdentityRepository, pub_dress: &str, tg: i64) -> PubDress {
        let owner: PubDress = pub_dress.parse().expect("owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(tg), 100)
            .await
            .expect("registration");
        assert!(
            repository
                .set_role(&owner, BondAccessRole::Business)
                .await
                .expect("role")
        );
        owner
    }

    fn station_of(owner: &PubDress, value: &str) -> StationPubDress {
        StationPubDress::for_owner(owner, value).expect("station")
    }

    fn point(longitude: f64, latitude: f64) -> GeoCoordinate {
        GeoCoordinate::from_degrees(longitude, latitude).expect("coordinate")
    }

    #[tokio::test]
    async fn a_business_keeps_one_station_per_shop_beside_its_own_avaia() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner = business(&repository, "0xda-market", 9901).await;
        let first = station_of(&owner, "0xda-market-batkivshchyna");
        let second = station_of(&owner, "0xda-market-zoloti-vorota");

        for (station, at) in [(&first, point(30.5632, 50.4265)), (&second, point(30.5131, 50.4489))] {
            assert!(matches!(
                repository
                    .station_business_avaia(&owner, station, None, at, 101)
                    .await
                    .expect("station"),
                BusinessStationOutcome::Stationed(_)
            ));
        }
        // The companion Avaia is still the owner's one, independent of shops.
        assert_eq!(repository.owned_avaia_identity(&owner).await.expect("read"), None);
        let own: AvaiaPubDress = "xda-marketai".parse().expect("Avaia");
        assert!(matches!(
            repository
                .configure_owned_avaia(&owner, &own, 102)
                .await
                .expect("create"),
            AvaiaUpdateOutcome::Updated(_)
        ));

        let stations = repository.business_stations().await.expect("stations");
        assert_eq!(
            stations
                .iter()
                .map(|station| station.pub_dress.as_str())
                .collect::<Vec<_>>(),
            ["0xda-market-batkivshchyna", "0xda-market-zoloti-vorota"]
        );
        assert!(stations.iter().all(|station| station.owner_pub_dress == "0xda-market"));
        assert_eq!(stations[0].coordinate, point(30.5632, 50.4265));
    }

    #[tokio::test]
    async fn restationing_moves_the_point_and_keeps_one_station() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner = business(&repository, "0xda-market", 9902).await;
        let station = station_of(&owner, "0xda-market-batkivshchyna");
        for at in [point(30.56, 50.42), point(30.5632, 50.4265)] {
            repository
                .station_business_avaia(&owner, &station, None, at, 101)
                .await
                .expect("station");
        }
        let stations = repository.business_stations().await.expect("stations");
        assert_eq!(stations.len(), 1);
        assert_eq!(stations[0].coordinate, point(30.5632, 50.4265));
    }

    #[tokio::test]
    async fn only_a_business_stations_and_never_over_another_identity() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let user: PubDress = "0xduser".parse().expect("user");
        repository
            .register(&user, &ProviderIdentity::telegram(9903), 100)
            .await
            .expect("registration");
        let at = point(30.5131, 50.4489);
        let station = station_of(&user, "0xduser-shop");
        assert_eq!(
            repository
                .station_business_avaia(&user, &station, None, at, 101)
                .await
                .expect("user"),
            BusinessStationOutcome::BusinessRoleRequired
        );
        let stranger: PubDress = "0xdnobody".parse().expect("stranger");
        let nowhere = station_of(&stranger, "0xdnobody-shop");
        assert_eq!(
            repository
                .station_business_avaia(&stranger, &nowhere, None, at, 101)
                .await
                .expect("unknown"),
            BusinessStationOutcome::Unknown
        );

        let owner = business(&repository, "0xda-market", 9904).await;
        let elsewhere: PubDress = "0x1shop".parse().expect("other business");
        let foreign = station_of(&elsewhere, "0x1shop-place");
        assert_eq!(
            repository
                .station_business_avaia(&owner, &foreign, None, at, 101)
                .await
                .expect("discriminator"),
            BusinessStationOutcome::NotOwnersAddress
        );
        // A person already holds this address.
        let person: PubDress = "0xda-market-shop".parse().expect("person");
        repository
            .register(&person, &ProviderIdentity::telegram(9905), 102)
            .await
            .expect("registration");
        assert_eq!(
            repository
                .station_business_avaia(
                    &owner,
                    &station_of(&owner, "0xda-market-shop"),
                    None,
                    at,
                    103
                )
                .await
                .expect("taken"),
            BusinessStationOutcome::AddressUnavailable
        );
        // And a station's address is taken for every person after it.
        let shop = station_of(&owner, "0xda-market-zoloti-vorota");
        repository
            .station_business_avaia(&owner, &shop, None, at, 104)
            .await
            .expect("station");
        assert!(
            !repository
                .is_pub_dress_available(shop.as_pub_dress())
                .await
                .expect("check")
        );
        assert!(matches!(
            repository
                .register(shop.as_pub_dress(), &ProviderIdentity::telegram(9906), 105)
                .await
                .expect("register"),
            RegistrationOutcome::HandleUnavailable
        ));
    }

    #[tokio::test]
    async fn an_ownerless_avaia_without_a_station_intent_is_still_rejected() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        sqlx::query(
            "INSERT INTO identities (pub_dress, identity_kind, owner_pub_dress, created_at) \
             VALUES ('xdstrayai', 'avaia', NULL, 1)",
        )
        .execute(&repository.pool)
        .await
        .expect("insert");
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM identities WHERE identity_kind = 'avaia'",
        )
        .fetch_one(&repository.pool)
        .await
        .expect("count");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn stations_survive_a_restart_and_leave_with_their_business() {
        let database = tempfile::NamedTempFile::new().expect("database");
        let url = format!("sqlite://{}", database.path().display());
        let repository = IdentityRepository::connect(&url).await.expect("repository");
        let owner = business(&repository, "0xda-market", 9906).await;
        let station = station_of(&owner, "0xda-market-batkivshchyna");
        repository
            .station_business_avaia(&owner, &station, None, point(30.5632, 50.4265), 101)
            .await
            .expect("station");
        drop(repository);

        // Every start re-installs the 0013 guard before the station migration.
        let repository = IdentityRepository::connect(&url).await.expect("reopen");
        assert_eq!(repository.business_stations().await.expect("stations").len(), 1);
        let renamed: PubDress = "0xda-shop".parse().expect("rename");
        assert!(matches!(
            repository
                .rename_pub_dress(&owner, &renamed)
                .await
                .expect("rename"),
            PubDressRenameOutcome::Renamed(_)
        ));
        let stations = repository.business_stations().await.expect("stations");
        assert_eq!(stations[0].owner_pub_dress, "0xda-shop");
        assert_eq!(stations[0].pub_dress, "0xda-market-batkivshchyna");

        sqlx::query("DELETE FROM identities WHERE pub_dress = '0xda-shop'")
            .execute(&repository.pool)
            .await
            .expect("delete");
        assert!(repository.business_stations().await.expect("stations").is_empty());
        let left = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM identities WHERE pub_dress = '0xda-market-batkivshchyna'",
        )
        .fetch_one(&repository.pool)
        .await
        .expect("count");
        assert_eq!(left, 0);
    }

    #[tokio::test]
    async fn registers_a_business_without_a_sign_in_as_many_points() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let owner: PubDress = "0xda-market".parse().expect("owner");
        assert!(matches!(
            repository.register_business(&owner, 100).await.expect("register"),
            BusinessRegistrationOutcome::Registered(ref record) if record.pub_dress == "0xda-market"
        ));
        assert_eq!(
            repository.role_for("0xda-market").await.expect("role"),
            BondAccessRole::Business
        );
        let providers = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM identity_providers WHERE pub_dress = '0xda-market'",
        )
        .fetch_one(&repository.pool)
        .await
        .expect("providers");
        assert_eq!(providers, 0);
        assert!(matches!(
            repository.register_business(&owner, 101).await.expect("again"),
            BusinessRegistrationOutcome::AlreadyBusiness(_)
        ));

        let gate = station_of(&owner, "0xda-market-zoloti-vorota");
        let stationed = repository
            .station_business_avaia(
                &owner,
                &gate,
                Some("xda-market Золоті Ворота"),
                point(30.5131, 50.4489),
                102,
            )
            .await
            .expect("station");
        assert!(matches!(
            stationed,
            BusinessStationOutcome::Stationed(ref s)
                if s.caption.as_deref() == Some("xda-market Золоті Ворота")
        ));
        let mother = station_of(&owner, "0xda-market-batkivshchyna-maty");
        repository
            .station_business_avaia(
                &owner,
                &mother,
                Some("xda-market Батьківщина-Мати"),
                point(30.5632, 50.4265),
                103,
            )
            .await
            .expect("station");

        // A business is its points: every one of its stations, in order made.
        let points = repository.business_stations_of(&owner).await.expect("points");
        assert_eq!(
            points.iter().map(|s| s.pub_dress.as_str()).collect::<Vec<_>>(),
            ["0xda-market-zoloti-vorota", "0xda-market-batkivshchyna-maty"]
        );
        // And it declares no single location of its own.
        let declared = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bond_locations WHERE pub_dress = '0xda-market'",
        )
        .fetch_one(&repository.pool)
        .await
        .expect("bond location");
        assert_eq!(declared, 0);

        // Restating a point keeps its caption.
        let restated = repository
            .station_business_avaia(&owner, &gate, None, point(30.5133, 50.4490), 104)
            .await
            .expect("restate");
        assert!(matches!(
            restated,
            BusinessStationOutcome::Stationed(ref s)
                if s.caption.as_deref() == Some("xda-market Золоті Ворота")
        ));
    }

    #[tokio::test]
    async fn a_business_never_registers_over_a_person() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let person: PubDress = "0xdperson".parse().expect("person");
        repository
            .register(&person, &ProviderIdentity::telegram(9910), 100)
            .await
            .expect("registration");
        assert_eq!(
            repository.register_business(&person, 101).await.expect("taken"),
            BusinessRegistrationOutcome::Unavailable
        );
        let reserved: PubDress = "0x0shop".parse().expect("reserved");
        assert_eq!(
            repository.register_business(&reserved, 101).await.expect("reserved"),
            BusinessRegistrationOutcome::Unavailable
        );
        // A person keeps no stations.
        assert!(
            repository
                .business_stations_of(&person)
                .await
                .expect("none")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_deployed_single_location_table_is_dropped_on_start() {
        let database = tempfile::NamedTempFile::new().expect("database");
        let url = format!("sqlite://{}", database.path().display());
        let repository = IdentityRepository::connect(&url).await.expect("repository");
        // The shape 0026 left in production.
        sqlx::query(
            "CREATE TABLE business_locations (owner_pub_dress TEXT PRIMARY KEY, \
             avaia_pub_dress TEXT NOT NULL UNIQUE) STRICT",
        )
        .execute(&repository.pool)
        .await
        .expect("legacy table");
        drop(repository);

        let repository = IdentityRepository::connect(&url).await.expect("reopen");
        let present = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema \
             WHERE type = 'table' AND name = 'business_locations')",
        )
        .fetch_one(&repository.pool)
        .await
        .expect("schema");
        assert!(!present);
    }
}
