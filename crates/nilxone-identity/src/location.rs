// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use nilxone_contracts::{BondLocation, BondLocationMode, DecimalU64, GeoCoordinate};
use serde::Serialize;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::str::FromStr;
use thiserror::Error;

/// Application role of a human Bond, persisted in `bond_roles`.
///
/// This is not a Bond kind or protocol authority class. The role only gates
/// application capabilities. A Bond without a stored role is [`Self::User`],
/// the only role public registration creates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BondAccessRole {
    #[default]
    User,
    Admin,
    Business,
}

impl BondAccessRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Admin => "admin",
            Self::Business => "business",
        }
    }

    #[must_use]
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "admin" => Some(Self::Admin),
            "business" => Some(Self::Business),
            _ => None,
        }
    }

    /// Admin holds every user capability plus declaring a manual Bond
    /// location. Every role may submit a live location.
    #[must_use]
    pub const fn can_set_manual_location(self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[derive(Clone, Debug)]
pub struct BondLocationRepository {
    pool: SqlitePool,
}

impl BondLocationRepository {
    pub async fn connect(database_url: &str) -> Result<Self, BondLocationRepositoryError> {
        let max_connections = if database_url.contains(":memory:") {
            1
        } else {
            5
        };
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0009_bond_location_control.sql"))
            .execute(&pool)
            .await?;
        Ok(Self { pool })
    }

    /// Reads the active owner-submitted location. No row means this Bond has
    /// never submitted application location state.
    pub async fn read(
        &self,
        pub_dress: &str,
    ) -> Result<Option<BondLocation>, BondLocationRepositoryError> {
        let row = sqlx::query(
            "SELECT mode, longitude_e7, latitude_e7, updated_at \
             FROM bond_locations WHERE pub_dress = ?",
        )
        .bind(pub_dress)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };

        let mode = match row.get::<String, _>("mode").as_str() {
            "live" => BondLocationMode::Live,
            "manual" => BondLocationMode::Manual,
            _ => return Err(BondLocationRepositoryError::CorruptState),
        };
        let longitude_e7 = i32::try_from(row.get::<i64, _>("longitude_e7"))
            .map_err(|_| BondLocationRepositoryError::CorruptState)?;
        let latitude_e7 = i32::try_from(row.get::<i64, _>("latitude_e7"))
            .map_err(|_| BondLocationRepositoryError::CorruptState)?;
        let coordinate = GeoCoordinate::new(longitude_e7, latitude_e7)
            .map_err(|_| BondLocationRepositoryError::CorruptState)?;
        let updated_at = u64::try_from(row.get::<i64, _>("updated_at"))
            .map_err(|_| BondLocationRepositoryError::CorruptState)?;

        Ok(Some(BondLocation::new(
            coordinate,
            mode,
            DecimalU64::new(updated_at),
        )))
    }

    /// Refreshes a live coordinate without ever switching modes: a Bond in
    /// `manual` keeps its declared point. Returns whether the write applied.
    /// Leaving `manual` stays an explicit [`Self::write`].
    pub async fn refresh_live(
        &self,
        pub_dress: &str,
        coordinate: GeoCoordinate,
        updated_at: DecimalU64,
    ) -> Result<bool, BondLocationRepositoryError> {
        let updated_at = i64::try_from(updated_at.get())
            .map_err(|_| BondLocationRepositoryError::TimestampOutOfRange)?;
        let result = sqlx::query(
            "INSERT INTO bond_locations \
             (pub_dress, mode, longitude_e7, latitude_e7, updated_at) \
             VALUES (?, 'live', ?, ?, ?) \
             ON CONFLICT(pub_dress) DO UPDATE SET \
               longitude_e7 = excluded.longitude_e7, \
               latitude_e7 = excluded.latitude_e7, \
               updated_at = excluded.updated_at \
             WHERE bond_locations.mode = 'live'",
        )
        .bind(pub_dress)
        .bind(i64::from(coordinate.longitude_e7()))
        .bind(i64::from(coordinate.latitude_e7()))
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Commits a user-initiated manual walk only when the Bond still has the
    /// same declared starting point. This cannot enable manual mode, replace
    /// a live observation, or overwrite another host's move.
    pub async fn move_manual(
        &self,
        pub_dress: &str,
        expected: GeoCoordinate,
        target: GeoCoordinate,
        updated_at: DecimalU64,
    ) -> Result<bool, BondLocationRepositoryError> {
        let updated_at = i64::try_from(updated_at.get())
            .map_err(|_| BondLocationRepositoryError::TimestampOutOfRange)?;
        let result = sqlx::query(
            "UPDATE bond_locations SET longitude_e7 = ?, latitude_e7 = ?, updated_at = ? \
             WHERE pub_dress = ? AND mode = 'manual' \
             AND longitude_e7 = ? AND latitude_e7 = ?",
        )
        .bind(i64::from(target.longitude_e7()))
        .bind(i64::from(target.latitude_e7()))
        .bind(updated_at)
        .bind(pub_dress)
        .bind(i64::from(expected.longitude_e7()))
        .bind(i64::from(expected.latitude_e7()))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Replaces the one active location projection for this Bond.
    /// Replacing `manual` with `live` is an explicit new observation, not a
    /// toggle that resurrects an older device coordinate.
    pub async fn write(
        &self,
        pub_dress: &str,
        location: BondLocation,
    ) -> Result<(), BondLocationRepositoryError> {
        let mode = match location.mode {
            BondLocationMode::Live => "live",
            BondLocationMode::Manual => "manual",
        };
        let updated_at = i64::try_from(location.updated_at.get())
            .map_err(|_| BondLocationRepositoryError::TimestampOutOfRange)?;
        sqlx::query(
            "INSERT INTO bond_locations \
             (pub_dress, mode, longitude_e7, latitude_e7, updated_at) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(pub_dress) DO UPDATE SET \
               mode = excluded.mode, \
               longitude_e7 = excluded.longitude_e7, \
               latitude_e7 = excluded.latitude_e7, \
               updated_at = excluded.updated_at",
        )
        .bind(pub_dress)
        .bind(mode)
        .bind(i64::from(location.coordinate.longitude_e7()))
        .bind(i64::from(location.coordinate.latitude_e7()))
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum BondLocationRepositoryError {
    #[error("Bond location database failure: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Bond location timestamp is outside the supported range")]
    TimestampOutOfRange,
    #[error("stored Bond location violates its contract")]
    CorruptState,
}
