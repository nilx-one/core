// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use nilxone_contracts::{
    DecimalU64, EarshotRadius, GeoCoordinate, PubDress, SpokenLine, SpokenLineId, SpokenText,
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::str::FromStr;
use thiserror::Error;
const MAX_LINES_SERVED: i64 = 50;
const METERS_TO_E7_LATITUDE: u64 = 91;

#[derive(Debug, Error)]
pub enum SpeechRepositoryError {
    #[error("speech database failure: {0}")]
    Database(#[from] sqlx::Error),
    #[error("stored spoken line violates its contract")]
    CorruptState,
}

/// A Telegram-bound Bond that has a location.
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    pub telegram_user_id: i64,
    pub coordinate: GeoCoordinate,
}

#[derive(Clone, Debug)]
pub struct SpeechRepository {
    pool: SqlitePool,
}

impl SpeechRepository {
    /// Opens the store. The identity repository must have initialised the
    /// same database first, because both tables reference `identities`.
    ///
    /// # Errors
    ///
    /// Returns [`SpeechRepositoryError`] when the database cannot be opened
    /// or migrated.
    pub async fn connect(database_url: &str) -> Result<Self, SpeechRepositoryError> {
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
        sqlx::raw_sql(include_str!("../migrations/0018_spoken_lines.sql"))
            .execute(&pool)
            .await?;
        let repository = Self { pool };
        repository.migrate_speakers().await?;
        Ok(repository)
    }

    /// Creates `bond_speakers` and carries the first two speakers over,
    /// exactly once: a Bond that registers one of those names later gets
    /// nothing from it.
    async fn migrate_speakers(&self) -> Result<(), SpeechRepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema \
             WHERE type = 'table' AND name = 'bond_speakers')",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !exists {
            sqlx::raw_sql(include_str!("../migrations/0019_bond_speakers.sql"))
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn is_speaker(&self, pub_dress: &PubDress) -> Result<bool, SpeechRepositoryError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM bond_speakers WHERE pub_dress = ?)",
        )
        .bind(pub_dress.as_str())
        .fetch_one(&self.pool)
        .await?)
    }

    /// Lets an existing human Bond speak. There is no public route to this;
    /// it is set out of band, like a role.
    pub async fn allow_speaker(&self, pub_dress: &PubDress) -> Result<bool, SpeechRepositoryError> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO bond_speakers (pub_dress) \
             SELECT pub_dress FROM identities \
             WHERE pub_dress = ? AND identity_kind = 'human'",
        )
        .bind(pub_dress.as_str())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Stores a line once. Returns whether this call created it.
    pub async fn record(
        &self,
        line: &SpokenLine,
        received_at: u64,
    ) -> Result<bool, SpeechRepositoryError> {
        let spoken_at = i64::try_from(line.spoken_at().get())
            .map_err(|_| SpeechRepositoryError::CorruptState)?;
        let received_at =
            i64::try_from(received_at).map_err(|_| SpeechRepositoryError::CorruptState)?;
        let result = sqlx::query(
            "INSERT OR IGNORE INTO spoken_lines (line_id, speaker, text, spoken_at, received_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(line.id().as_str())
        .bind(line.speaker().as_str())
        .bind(line.text().as_str())
        .bind(spoken_at)
        .bind(received_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Lines spoken at or after `since`, oldest first, capped.
    pub async fn recent(&self, since: u64) -> Result<Vec<SpokenLine>, SpeechRepositoryError> {
        let since = i64::try_from(since).map_err(|_| SpeechRepositoryError::CorruptState)?;
        let rows = sqlx::query(
            "SELECT line_id, speaker, text, spoken_at FROM ( \
               SELECT line_id, speaker, text, spoken_at FROM spoken_lines \
               WHERE spoken_at >= ? ORDER BY spoken_at DESC, line_id DESC LIMIT ? \
             ) ORDER BY spoken_at ASC, line_id ASC",
        )
        .bind(since)
        .bind(MAX_LINES_SERVED)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id = SpokenLineId::try_from(row.get::<String, _>("line_id"))
                    .map_err(|_| SpeechRepositoryError::CorruptState)?;
                let speaker = PubDress::try_from(row.get::<String, _>("speaker"))
                    .map_err(|_| SpeechRepositoryError::CorruptState)?;
                let text = SpokenText::try_from(row.get::<String, _>("text"))
                    .map_err(|_| SpeechRepositoryError::CorruptState)?;
                let spoken_at = u64::try_from(row.get::<i64, _>("spoken_at"))
                    .map_err(|_| SpeechRepositoryError::CorruptState)?;
                Ok(SpokenLine::new(
                    id,
                    speaker,
                    text,
                    DecimalU64::new(spoken_at),
                ))
            })
            .collect()
    }

    /// Telegram-bound human Bonds whose location falls in the latitude band
    /// of `center` ± `radius`, and that still place them somewhere. This is
    /// only a prefilter; callers decide earshot exactly with Core.
    pub async fn telegram_listeners_near(
        &self,
        center: GeoCoordinate,
        radius: EarshotRadius,
        fresh_since: u64,
        excluding: &PubDress,
    ) -> Result<Vec<Listener>, SpeechRepositoryError> {
        let band = i64::try_from(u64::from(radius.meters()) * METERS_TO_E7_LATITUDE + 100)
            .map_err(|_| SpeechRepositoryError::CorruptState)?;
        let fresh_since =
            i64::try_from(fresh_since).map_err(|_| SpeechRepositoryError::CorruptState)?;
        let rows = sqlx::query(
            "SELECT provider.provider_subject AS telegram_subject, \
                    location.longitude_e7, location.latitude_e7 \
             FROM bond_locations AS location \
             JOIN identities ON identities.pub_dress = location.pub_dress \
             JOIN identity_providers AS provider \
               ON provider.pub_dress = location.pub_dress AND provider.provider = 'telegram' \
             WHERE identities.identity_kind = 'human' \
               AND location.pub_dress != ? \
               AND (location.mode = 'manual' OR location.updated_at >= ?) \
               AND location.latitude_e7 BETWEEN ? AND ?",
        )
        .bind(excluding.as_str())
        .bind(fresh_since)
        .bind(i64::from(center.latitude_e7()) - band)
        .bind(i64::from(center.latitude_e7()) + band)
        .fetch_all(&self.pool)
        .await?;
        let mut listeners = Vec::with_capacity(rows.len());
        for row in rows {
            let Ok(telegram_user_id) = row.get::<String, _>("telegram_subject").parse::<i64>()
            else {
                continue;
            };
            let longitude = i32::try_from(row.get::<i64, _>("longitude_e7"))
                .map_err(|_| SpeechRepositoryError::CorruptState)?;
            let latitude = i32::try_from(row.get::<i64, _>("latitude_e7"))
                .map_err(|_| SpeechRepositoryError::CorruptState)?;
            let coordinate = GeoCoordinate::new(longitude, latitude)
                .map_err(|_| SpeechRepositoryError::CorruptState)?;
            listeners.push(Listener {
                telegram_user_id,
                coordinate,
            });
        }
        Ok(listeners)
    }
}
