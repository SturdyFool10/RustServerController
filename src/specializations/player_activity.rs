use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
struct PlayerActivity {
    total_seconds: i64,
    current_session_started_at: Option<DateTime<Utc>>,
    last_joined_at: Option<DateTime<Utc>>,
    last_left_at: Option<DateTime<Utc>>,
    session_count: u64,
    active_session_id: Option<i64>,
}

/// Tracks in-memory player sessions for a single running server specialization.
#[derive(Clone, Debug, Default)]
pub struct PlayerActivityTracker {
    players: BTreeMap<String, PlayerActivity>,
    store: Option<PlayerActivityStore>,
}

impl PlayerActivityTracker {
    pub fn for_server(server_name: &str, specialization: &str) -> Self {
        match PlayerActivityStore::open(server_name, specialization) {
            Ok(store) => {
                let players = match store.load_players() {
                    Ok(players) => players,
                    Err(error) => {
                        tracing::warn!(
                            "Failed to load player activity for '{}': {}",
                            server_name,
                            error
                        );
                        BTreeMap::new()
                    }
                };
                let tracker = Self {
                    players,
                    store: Some(store),
                };
                tracker.record_player_count();
                tracker
            }
            Err(error) => {
                tracing::warn!(
                    "Failed to open player activity database for '{}': {}",
                    server_name,
                    error
                );
                Self::default()
            }
        }
    }

    pub fn player_joined(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }

        let now = Utc::now();
        let player = self.players.entry(name.to_string()).or_default();
        if player.current_session_started_at.is_some() {
            return false;
        }

        player.current_session_started_at = Some(now);
        player.last_joined_at = Some(now);
        player.session_count += 1;
        if let Some(store) = &self.store {
            match store.persist_join(name, player, now) {
                Ok(session_id) => {
                    player.active_session_id = Some(session_id);
                }
                Err(error) => {
                    tracing::warn!("Failed to persist player join for '{}': {}", name, error);
                }
            }
        }
        self.record_player_count();
        true
    }

    pub fn player_left(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }

        let Some(player) = self.players.get_mut(name) else {
            return false;
        };
        let Some(started_at) = player.current_session_started_at.take() else {
            return false;
        };

        let now = Utc::now();
        player.total_seconds += elapsed_seconds(started_at, now);
        player.last_left_at = Some(now);
        if let Some(store) = &self.store {
            if let Err(error) = store.persist_leave(name, player, started_at, now) {
                tracing::warn!("Failed to persist player leave for '{}': {}", name, error);
            }
        }
        self.record_player_count();
        true
    }

    pub fn mark_all_offline(&mut self) -> bool {
        let now = Utc::now();
        let mut changed = false;
        for (name, player) in self.players.iter_mut() {
            let Some(started_at) = player.current_session_started_at.take() else {
                continue;
            };
            player.total_seconds += elapsed_seconds(started_at, now);
            player.last_left_at = Some(now);
            if let Some(store) = &self.store {
                if let Err(error) = store.persist_leave(name, player, started_at, now) {
                    tracing::warn!("Failed to persist player exit for '{}': {}", name, error);
                }
            }
            changed = true;
        }
        if changed {
            self.record_player_count();
        }
        changed
    }

    pub fn online_count(&self) -> usize {
        self.players
            .values()
            .filter(|player| player.current_session_started_at.is_some())
            .count()
    }

    pub fn online_names(&self) -> Vec<String> {
        self.players
            .iter()
            .filter(|(_, player)| player.current_session_started_at.is_some())
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn known_player_count(&self) -> usize {
        self.players.len()
    }

    pub fn total_seconds(&self) -> i64 {
        self.players
            .values()
            .map(|player| player.total_seconds + current_session_seconds(player))
            .sum()
    }

    pub fn total_hours(&self) -> f64 {
        seconds_to_hours(self.total_seconds())
    }

    pub fn summaries(&self) -> Value {
        Value::Array(
            self.players
                .iter()
                .map(|(name, player)| player_summary(name, player))
                .collect(),
        )
    }

    pub fn recent_sessions(&self, limit: usize) -> Value {
        let Some(store) = &self.store else {
            return Value::Array(Vec::new());
        };

        match store.recent_sessions(limit) {
            Ok(sessions) => Value::Array(sessions),
            Err(error) => {
                tracing::warn!("Failed to load recent player sessions: {}", error);
                Value::Array(Vec::new())
            }
        }
    }

    pub fn timeframe_stats(&self) -> Value {
        let Some(store) = &self.store else {
            return Value::Object(serde_json::Map::new());
        };

        match store.timeframe_stats() {
            Ok(stats) => stats,
            Err(error) => {
                tracing::warn!("Failed to load player timeframe stats: {}", error);
                Value::Object(serde_json::Map::new())
            }
        }
    }

    fn record_player_count(&self) {
        let Some(store) = &self.store else {
            return;
        };

        if let Err(error) = store.record_player_count(self.online_count()) {
            tracing::warn!("Failed to persist player count sample: {}", error);
        }
    }
}

#[derive(Clone, Debug)]
struct PlayerActivityStore {
    db_path: PathBuf,
    server_name: String,
    specialization: String,
}

impl PlayerActivityStore {
    fn open(server_name: &str, specialization: &str) -> rusqlite::Result<Self> {
        Self::open_at(database_path(), server_name, specialization)
    }

    fn open_at(
        db_path: PathBuf,
        server_name: &str,
        specialization: &str,
    ) -> rusqlite::Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }

        let store = Self {
            db_path,
            server_name: server_name.to_string(),
            specialization: specialization.to_string(),
        };
        store.with_connection(|connection| initialize_schema(connection))?;
        Ok(store)
    }

    fn load_players(&self) -> rusqlite::Result<BTreeMap<String, PlayerActivity>> {
        self.close_stale_sessions()?;
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT player_name, total_seconds, current_session_started_at,
                        last_joined_at, last_left_at, session_count, active_session_id
                   FROM player_activity
                  WHERE server_name = ?1 AND specialization = ?2
                  ORDER BY player_name",
            )?;
            let rows =
                statement.query_map(params![self.server_name, self.specialization], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        PlayerActivity {
                            total_seconds: row.get(1)?,
                            current_session_started_at: parse_timestamp(row.get(2)?),
                            last_joined_at: parse_timestamp(row.get(3)?),
                            last_left_at: parse_timestamp(row.get(4)?),
                            session_count: row.get(5)?,
                            active_session_id: row.get(6)?,
                        },
                    ))
                })?;

            let mut players = BTreeMap::new();
            for row in rows {
                let (name, player) = row?;
                players.insert(name, player);
            }
            Ok(players)
        })
    }

    fn persist_join(
        &self,
        player_name: &str,
        player: &PlayerActivity,
        joined_at: DateTime<Utc>,
    ) -> rusqlite::Result<i64> {
        self.with_connection(|connection| {
            let tx = connection.transaction()?;
            tx.execute(
                "INSERT INTO player_activity_sessions
                    (server_name, specialization, player_name, joined_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    self.server_name,
                    self.specialization,
                    player_name,
                    format_timestamp(Some(joined_at))
                ],
            )?;
            let session_id = tx.last_insert_rowid();
            upsert_player(
                &tx,
                &self.server_name,
                &self.specialization,
                player_name,
                player,
                Some(session_id),
            )?;
            tx.commit()?;
            Ok(session_id)
        })
    }

    fn persist_leave(
        &self,
        player_name: &str,
        player: &mut PlayerActivity,
        joined_at: DateTime<Utc>,
        left_at: DateTime<Utc>,
    ) -> rusqlite::Result<()> {
        let duration_seconds = elapsed_seconds(joined_at, left_at);
        self.with_connection(|connection| {
            let tx = connection.transaction()?;
            if let Some(session_id) = player.active_session_id {
                tx.execute(
                    "UPDATE player_activity_sessions
                        SET left_at = ?1, duration_seconds = ?2
                      WHERE id = ?3",
                    params![format_timestamp(Some(left_at)), duration_seconds, session_id],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO player_activity_sessions
                        (server_name, specialization, player_name, joined_at, left_at, duration_seconds)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        self.server_name,
                        self.specialization,
                        player_name,
                        format_timestamp(Some(joined_at)),
                        format_timestamp(Some(left_at)),
                        duration_seconds
                    ],
                )?;
            }
            player.active_session_id = None;
            upsert_player(&tx, &self.server_name, &self.specialization, player_name, player, None)?;
            tx.commit()
        })
    }

    fn close_stale_sessions(&self) -> rusqlite::Result<()> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT player_name, current_session_started_at
                   FROM player_activity
                  WHERE server_name = ?1
                    AND specialization = ?2
                    AND current_session_started_at IS NOT NULL",
            )?;
            let rows = statement
                .query_map(params![self.server_name, self.specialization], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?;

            let now = Utc::now();
            let mut stale_sessions = Vec::new();
            for row in rows {
                let (player_name, started_at) = row?;
                let Some(started_at) = parse_timestamp(started_at) else {
                    continue;
                };
                stale_sessions.push((player_name, started_at));
            }
            drop(statement);

            let tx = connection.transaction()?;
            for (player_name, started_at) in stale_sessions {
                let duration_seconds = elapsed_seconds(started_at, now);
                tx.execute(
                    "UPDATE player_activity
                        SET total_seconds = total_seconds + ?1,
                            current_session_started_at = NULL,
                            last_left_at = ?2,
                            active_session_id = NULL
                      WHERE server_name = ?3 AND specialization = ?4 AND player_name = ?5",
                    params![
                        duration_seconds,
                        format_timestamp(Some(now)),
                        self.server_name,
                        self.specialization,
                        player_name
                    ],
                )?;
                tx.execute(
                    "UPDATE player_activity_sessions
                        SET left_at = ?1, duration_seconds = ?2
                      WHERE server_name = ?3
                        AND specialization = ?4
                        AND player_name = ?5
                        AND left_at IS NULL",
                    params![
                        format_timestamp(Some(now)),
                        duration_seconds,
                        self.server_name,
                        self.specialization,
                        player_name
                    ],
                )?;
            }
            tx.commit()
        })
    }

    fn recent_sessions(&self, limit: usize) -> rusqlite::Result<Vec<Value>> {
        let limit = i64::try_from(limit).unwrap_or(25);
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT player_name, joined_at, left_at, duration_seconds
                   FROM player_activity_sessions
                  WHERE server_name = ?1 AND specialization = ?2
                  ORDER BY joined_at DESC
                  LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![self.server_name, self.specialization, limit],
                |row| {
                    let player_name: String = row.get(0)?;
                    let joined_at: Option<String> = row.get(1)?;
                    let left_at: Option<String> = row.get(2)?;
                    let duration_seconds: Option<i64> = row.get(3)?;
                    Ok(json!({
                        "name": player_name,
                        "joined_at": joined_at,
                        "left_at": left_at,
                        "duration_seconds": duration_seconds.unwrap_or(0),
                        "duration_hours": seconds_to_hours(duration_seconds.unwrap_or(0)),
                    }))
                },
            )?;

            let mut sessions = Vec::new();
            for row in rows {
                sessions.push(row?);
            }
            Ok(sessions)
        })
    }

    fn record_player_count(&self, online_count: usize) -> rusqlite::Result<()> {
        let online_count = i64::try_from(online_count).unwrap_or(i64::MAX);
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO player_count_samples
                    (server_name, specialization, sampled_at, online_count)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    self.server_name,
                    self.specialization,
                    format_timestamp(Some(Utc::now())),
                    online_count
                ],
            )?;
            Ok(())
        })
    }

    fn timeframe_stats(&self) -> rusqlite::Result<Value> {
        let now = Utc::now();
        let timeframes = [
            ("day", ChronoDuration::days(1)),
            ("week", ChronoDuration::weeks(1)),
            ("month", ChronoDuration::days(30)),
            ("year", ChronoDuration::days(365)),
        ];
        let mut stats = serde_json::Map::new();

        for (name, duration) in timeframes {
            let start = now - duration;
            stats.insert(name.to_string(), self.timeframe_summary(start, now)?);
        }

        Ok(Value::Object(stats))
    }

    fn timeframe_summary(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> rusqlite::Result<Value> {
        let samples = self.player_count_samples(start)?;
        let busy_by_hour = self.busy_by_hour(start)?;
        let logged_seconds = self.logged_seconds_between(start, end)?;
        let unique_players = self.unique_players_since(start)?;
        let average_online = average_online(&samples);
        let peak_online = samples
            .iter()
            .filter_map(|sample| sample.get("players").and_then(|value| value.as_i64()))
            .max()
            .unwrap_or(0);

        Ok(json!({
            "start": format_timestamp(Some(start)),
            "end": format_timestamp(Some(end)),
            "logged_seconds": logged_seconds,
            "logged_hours": seconds_to_hours(logged_seconds),
            "unique_players": unique_players,
            "average_online": round_two(average_online),
            "peak_online": peak_online,
            "sample_count": samples.len(),
            "player_count_samples": samples,
            "busy_by_hour": busy_by_hour,
        }))
    }

    fn player_count_samples(&self, start: DateTime<Utc>) -> rusqlite::Result<Vec<Value>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT sampled_at, online_count
                   FROM player_count_samples
                  WHERE server_name = ?1
                    AND specialization = ?2
                    AND sampled_at >= ?3
                  ORDER BY sampled_at ASC
                  LIMIT 1000",
            )?;
            let rows = statement.query_map(
                params![
                    self.server_name,
                    self.specialization,
                    format_timestamp(Some(start))
                ],
                |row| {
                    let sampled_at: Option<String> = row.get(0)?;
                    let online_count: i64 = row.get(1)?;
                    Ok(json!({
                        "timestamp": sampled_at,
                        "players": online_count,
                    }))
                },
            )?;

            let mut samples = Vec::new();
            for row in rows {
                samples.push(row?);
            }
            Ok(samples)
        })
    }

    fn busy_by_hour(&self, start: DateTime<Utc>) -> rusqlite::Result<Vec<Value>> {
        self.with_connection(|connection| {
            let mut hours: Vec<Value> = (0..24)
                .map(|hour| {
                    json!({
                        "hour": hour,
                        "average_online": 0.0,
                        "samples": 0,
                    })
                })
                .collect();
            let mut statement = connection.prepare(
                "SELECT CAST(strftime('%H', sampled_at) AS INTEGER) AS hour,
                        AVG(online_count) AS average_online,
                        COUNT(*) AS samples
                   FROM player_count_samples
                  WHERE server_name = ?1
                    AND specialization = ?2
                    AND sampled_at >= ?3
                  GROUP BY hour
                  ORDER BY hour",
            )?;
            let rows = statement.query_map(
                params![
                    self.server_name,
                    self.specialization,
                    format_timestamp(Some(start))
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )?;

            for row in rows {
                let (hour, average, samples) = row?;
                if let Some(slot) = usize::try_from(hour)
                    .ok()
                    .and_then(|index| hours.get_mut(index))
                {
                    *slot = json!({
                        "hour": hour,
                        "average_online": round_two(average),
                        "samples": samples,
                    });
                }
            }
            Ok(hours)
        })
    }

    fn logged_seconds_between(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> rusqlite::Result<i64> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT joined_at, left_at
                   FROM player_activity_sessions
                  WHERE server_name = ?1
                    AND specialization = ?2
                    AND joined_at <= ?3
                    AND (left_at IS NULL OR left_at >= ?4)",
            )?;
            let rows = statement.query_map(
                params![
                    self.server_name,
                    self.specialization,
                    format_timestamp(Some(end)),
                    format_timestamp(Some(start))
                ],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )?;

            let mut seconds = 0;
            for row in rows {
                let (joined_at, left_at) = row?;
                let Some(joined_at) = parse_timestamp(joined_at) else {
                    continue;
                };
                let left_at = parse_timestamp(left_at).unwrap_or(end);
                let overlap_start = joined_at.max(start);
                let overlap_end = left_at.min(end);
                if overlap_end > overlap_start {
                    seconds += elapsed_seconds(overlap_start, overlap_end);
                }
            }
            Ok(seconds)
        })
    }

    fn unique_players_since(&self, start: DateTime<Utc>) -> rusqlite::Result<i64> {
        self.with_connection(|connection| {
            connection.query_row(
                "SELECT COUNT(DISTINCT player_name)
                   FROM player_activity_sessions
                  WHERE server_name = ?1
                    AND specialization = ?2
                    AND (joined_at >= ?3 OR left_at >= ?3 OR left_at IS NULL)",
                params![
                    self.server_name,
                    self.specialization,
                    format_timestamp(Some(start))
                ],
                |row| row.get(0),
            )
        })
    }

    fn with_connection<T>(
        &self,
        action: impl FnOnce(&mut Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let mut connection = Connection::open(&self.db_path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        action(&mut connection)
    }
}

fn initialize_schema(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS player_activity (
            server_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            player_name TEXT NOT NULL,
            total_seconds INTEGER NOT NULL DEFAULT 0,
            current_session_started_at TEXT,
            last_joined_at TEXT,
            last_left_at TEXT,
            session_count INTEGER NOT NULL DEFAULT 0,
            active_session_id INTEGER,
            PRIMARY KEY (server_name, specialization, player_name)
        );

        CREATE TABLE IF NOT EXISTS player_activity_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            player_name TEXT NOT NULL,
            joined_at TEXT NOT NULL,
            left_at TEXT,
            duration_seconds INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_player_activity_sessions_lookup
            ON player_activity_sessions (server_name, specialization, player_name, joined_at);

        CREATE TABLE IF NOT EXISTS player_count_samples (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            sampled_at TEXT NOT NULL,
            online_count INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_player_count_samples_lookup
            ON player_count_samples (server_name, specialization, sampled_at);
        "#,
    )
}

fn upsert_player(
    connection: &Connection,
    server_name: &str,
    specialization: &str,
    player_name: &str,
    player: &PlayerActivity,
    active_session_id: Option<i64>,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO player_activity
            (server_name, specialization, player_name, total_seconds,
             current_session_started_at, last_joined_at, last_left_at, session_count, active_session_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(server_name, specialization, player_name) DO UPDATE SET
             total_seconds = excluded.total_seconds,
             current_session_started_at = excluded.current_session_started_at,
             last_joined_at = excluded.last_joined_at,
             last_left_at = excluded.last_left_at,
             session_count = excluded.session_count,
             active_session_id = excluded.active_session_id",
        params![
            server_name,
            specialization,
            player_name,
            player.total_seconds,
            format_timestamp(player.current_session_started_at),
            format_timestamp(player.last_joined_at),
            format_timestamp(player.last_left_at),
            player.session_count,
            active_session_id
        ],
    )?;
    Ok(())
}

fn database_path() -> PathBuf {
    std::env::var("RSC_PLAYER_ACTIVITY_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("controller_data").join("player_activity.sqlite3"))
}

fn player_summary(name: &str, player: &PlayerActivity) -> Value {
    let current_session_seconds = current_session_seconds(player);
    let total_seconds = player.total_seconds + current_session_seconds;

    json!({
        "name": name,
        "online": player.current_session_started_at.is_some(),
        "sessions": player.session_count,
        "total_seconds": total_seconds,
        "total_hours": seconds_to_hours(total_seconds),
        "current_session_seconds": current_session_seconds,
        "current_session_hours": seconds_to_hours(current_session_seconds),
        "current_session_started_at": format_timestamp(player.current_session_started_at),
        "last_joined_at": format_timestamp(player.last_joined_at),
        "last_left_at": format_timestamp(player.last_left_at),
    })
}

fn current_session_seconds(player: &PlayerActivity) -> i64 {
    player
        .current_session_started_at
        .map(|started_at| elapsed_seconds(started_at, Utc::now()))
        .unwrap_or(0)
}

fn elapsed_seconds(started_at: DateTime<Utc>, ended_at: DateTime<Utc>) -> i64 {
    ended_at
        .signed_duration_since(started_at)
        .num_seconds()
        .max(0)
}

fn seconds_to_hours(seconds: i64) -> f64 {
    round_two(seconds as f64 / 3600.0)
}

fn average_online(samples: &[Value]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }

    let total: i64 = samples
        .iter()
        .filter_map(|sample| sample.get("players").and_then(|value| value.as_i64()))
        .sum();
    total as f64 / samples.len() as f64
}

fn round_two(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn format_timestamp(timestamp: Option<DateTime<Utc>>) -> Option<String> {
    timestamp.map(|value| value.to_rfc3339())
}

fn parse_timestamp(timestamp: Option<String>) -> Option<DateTime<Utc>> {
    timestamp
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_join_and_leave_without_duplicate_sessions() {
        let mut tracker = PlayerActivityTracker::default();

        assert!(tracker.player_joined("PlayerOne"));
        assert!(!tracker.player_joined("PlayerOne"));
        assert_eq!(tracker.online_count(), 1);
        assert_eq!(tracker.known_player_count(), 1);

        assert!(tracker.player_left("PlayerOne"));
        assert!(!tracker.player_left("PlayerOne"));
        assert_eq!(tracker.online_count(), 0);
    }

    #[test]
    fn mark_all_offline_closes_current_sessions() {
        let mut tracker = PlayerActivityTracker::default();

        tracker.player_joined("PlayerOne");

        assert!(tracker.mark_all_offline());
        assert!(!tracker.mark_all_offline());
        assert_eq!(tracker.online_names(), Vec::<String>::new());
    }

    #[test]
    fn sqlite_store_persists_players_and_sessions() -> rusqlite::Result<()> {
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let store = PlayerActivityStore::open_at(db_path.clone(), "Server One", "Minecraft")?;
        let mut tracker = PlayerActivityTracker {
            players: store.load_players()?,
            store: Some(store),
        };

        assert!(tracker.player_joined("PlayerOne"));
        assert!(tracker.player_left("PlayerOne"));

        let reloaded_store =
            PlayerActivityStore::open_at(db_path.clone(), "Server One", "Minecraft")?;
        let reloaded_tracker = PlayerActivityTracker {
            players: reloaded_store.load_players()?,
            store: Some(reloaded_store),
        };

        assert_eq!(reloaded_tracker.known_player_count(), 1);
        assert_eq!(
            reloaded_tracker
                .recent_sessions(10)
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        let timeframe_stats = reloaded_tracker.timeframe_stats();
        assert!(timeframe_stats
            .get("day")
            .and_then(|stats| stats.get("logged_hours"))
            .is_some());
        assert!(timeframe_stats
            .get("day")
            .and_then(|stats| stats.get("player_count_samples"))
            .and_then(|samples| samples.as_array())
            .is_some_and(|samples| !samples.is_empty()));

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }
}
