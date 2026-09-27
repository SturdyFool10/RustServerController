use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
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

/// Minimum time between periodic player-count samples, so `player_count_samples`
/// accumulates enough resolution over hours/days/weeks for the rolling day/week/
/// month/year windows to genuinely diverge instead of all reading the same
/// handful of join/leave-triggered samples.
const PERIODIC_SAMPLE_INTERVAL_SECONDS: i64 = 5 * 60;

/// Tracks in-memory name sessions for a single running server instance.
#[derive(Clone, Debug, Default)]
pub struct PlayerActivityTracker {
    players: BTreeMap<String, PlayerActivity>,
    store: Option<PlayerActivityStore>,
    last_periodic_sample_at: Option<DateTime<Utc>>,
}

impl PlayerActivityTracker {
    pub fn for_server(server_uuid: &str, display_name: &str, specialization: &str) -> Self {
        match PlayerActivityStore::open(server_uuid, display_name, specialization) {
            Ok(store) => {
                let players = match store.load_players() {
                    Ok(players) => players,
                    Err(error) => {
                        tracing::warn!(
                            "Failed to load player activity for '{}': {}",
                            display_name,
                            error
                        );
                        BTreeMap::new()
                    }
                };
                let tracker = Self {
                    players,
                    store: Some(store),
                    last_periodic_sample_at: None,
                };
                tracker.record_player_count();
                tracker
            }
            Err(error) => {
                tracing::warn!(
                    "Failed to open player activity database for '{}': {}",
                    display_name,
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
            return Value::Array(Vec::new());
        };

        match store.timeframe_stats() {
            Ok(stats) => Value::Array(stats),
            Err(error) => {
                tracing::warn!("Failed to load player timeframe stats: {}", error);
                Value::Array(Vec::new())
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

    /// Records a player-count sample if at least `PERIODIC_SAMPLE_INTERVAL_SECONDS`
    /// has passed since the last one. Call this frequently (e.g. on every log
    /// line) from callers that don't otherwise have a timer; join/leave events
    /// alone are too sparse to give the rolling day/week/month/year windows
    /// enough resolution to differ from each other.
    pub fn maybe_sample_periodic(&mut self) {
        let now = Utc::now();
        let due = self
            .last_periodic_sample_at
            .is_none_or(|last| (now - last).num_seconds() >= PERIODIC_SAMPLE_INTERVAL_SECONDS);
        if !due {
            return;
        }
        self.last_periodic_sample_at = Some(now);
        self.record_player_count();
    }
}

pub fn migrate_server_name_to_uuid(server_name: &str, server_uuid: &str) {
    if server_name.trim().is_empty() || server_uuid.trim().is_empty() || server_name == server_uuid
    {
        return;
    }

    let store = PlayerActivityStore {
        db_path: database_path(),
        server_uuid: server_uuid.to_string(),
        display_name: server_name.to_string(),
        specialization: String::new(),
    };
    if let Err(error) = store.migrate_legacy_server_name(server_name) {
        tracing::warn!(
            "Failed to migrate player activity from server name '{}' to UUID '{}': {}",
            server_name,
            server_uuid,
            error
        );
    }
}

/// Records the current name for a known player UUID (e.g. observed from a
/// Minecraft server's `usercache.json`), migrating all recorded activity from
/// any previously-known name for that UUID. Returns the previous name if a
/// rename was detected and migrated, so callers can also update any other
/// name-keyed records (e.g. whitelist/ban list entries) that reference it.
pub fn sync_player_identity(uuid: &str, name: &str) -> Option<String> {
    let uuid = uuid.trim();
    let name = name.trim();
    if uuid.is_empty() || name.is_empty() {
        return None;
    }
    match PlayerActivityStore::sync_player_identity(database_path(), uuid, name) {
        Ok(renamed_from) => renamed_from,
        Err(error) => {
            tracing::warn!("Failed to sync player identity for '{}': {}", name, error);
            None
        }
    }
}

pub fn archived_server_stats(active_server_uuids: &[String]) -> Value {
    match PlayerActivityStore::archived_server_stats(database_path(), active_server_uuids) {
        Ok(stats) => Value::Array(stats),
        Err(error) => {
            tracing::warn!("Failed to load archived player activity stats: {}", error);
            Value::Array(Vec::new())
        }
    }
}

/// Aggregates player activity across every server sharing this controller's
/// activity database, one entry per distinct player name, listing which
/// server(s) they're associated with (and currently online on).
pub fn global_player_activity() -> Value {
    match PlayerActivityStore::global_player_activity(database_path()) {
        Ok(players) => Value::Array(players),
        Err(error) => {
            tracing::warn!("Failed to load global player activity: {}", error);
            Value::Array(Vec::new())
        }
    }
}

pub fn delete_server_stats(server_uuid: &str) -> rusqlite::Result<()> {
    PlayerActivityStore::delete_server_stats(database_path(), server_uuid)
}

#[derive(Clone, Debug)]
struct PlayerActivityStore {
    db_path: PathBuf,
    server_uuid: String,
    display_name: String,
    specialization: String,
}

impl PlayerActivityStore {
    fn open(server_uuid: &str, display_name: &str, specialization: &str) -> rusqlite::Result<Self> {
        Self::open_at(database_path(), server_uuid, display_name, specialization)
    }

    fn open_at(
        db_path: PathBuf,
        server_uuid: &str,
        display_name: &str,
        specialization: &str,
    ) -> rusqlite::Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }

        let store = Self {
            db_path,
            server_uuid: server_uuid.to_string(),
            display_name: display_name.to_string(),
            specialization: specialization.to_string(),
        };
        store.with_connection(|connection| initialize_schema(connection))?;
        store.upsert_server_metadata()?;
        Ok(store)
    }

    fn load_players(&self) -> rusqlite::Result<BTreeMap<String, PlayerActivity>> {
        self.close_stale_sessions()?;
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT player_name,
                        SUM(total_seconds) AS total_seconds,
                        MAX(current_session_started_at) AS current_session_started_at,
                        MAX(last_joined_at) AS last_joined_at,
                        MAX(last_left_at) AS last_left_at,
                        SUM(session_count) AS session_count,
                        MAX(active_session_id) AS active_session_id
                   FROM player_activity
                  WHERE server_name = ?1
                  GROUP BY player_name
                  ORDER BY player_name",
            )?;
            let rows = statement.query_map(params![self.server_uuid], |row| {
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

    fn upsert_server_metadata(&self) -> rusqlite::Result<()> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO player_activity_servers
                    (server_uuid, display_name, specialization, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(server_uuid) DO UPDATE SET
                    display_name = excluded.display_name,
                    specialization = excluded.specialization,
                    last_seen_at = excluded.last_seen_at",
                params![
                    self.server_uuid,
                    self.display_name,
                    self.specialization,
                    format_timestamp(Some(Utc::now()))
                ],
            )?;
            Ok(())
        })
    }

    fn migrate_legacy_server_name(&self, server_name: &str) -> rusqlite::Result<()> {
        self.with_connection(|connection| {
            initialize_schema(connection)?;
            let tx = connection.transaction()?;
            for table in [
                "player_activity",
                "player_activity_sessions",
                "player_count_samples",
            ] {
                tx.execute(
                    &format!("UPDATE {table} SET server_name = ?1 WHERE server_name = ?2"),
                    params![self.server_uuid, server_name],
                )?;
            }
            tx.execute(
                "INSERT INTO player_activity_servers
                    (server_uuid, display_name, specialization, last_seen_at)
                 VALUES (?1, ?2, '', ?3)
                 ON CONFLICT(server_uuid) DO UPDATE SET
                    display_name = excluded.display_name,
                    last_seen_at = excluded.last_seen_at",
                params![
                    self.server_uuid,
                    self.display_name,
                    format_timestamp(Some(Utc::now()))
                ],
            )?;
            tx.execute(
                "DELETE FROM player_activity_servers WHERE server_uuid = ?1",
                params![server_name],
            )?;
            tx.commit()
        })
    }

    fn sync_player_identity(
        db_path: PathBuf,
        uuid: &str,
        name: &str,
    ) -> rusqlite::Result<Option<String>> {
        let store = Self {
            db_path,
            server_uuid: String::new(),
            display_name: String::new(),
            specialization: String::new(),
        };
        store.with_connection(|connection| {
            initialize_schema(connection)?;
            let previous_name: Option<String> = connection
                .query_row(
                    "SELECT name FROM player_identities WHERE uuid = ?1",
                    params![uuid],
                    |row| row.get(0),
                )
                .optional()?;

            connection.execute(
                "INSERT INTO player_identities (uuid, name, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(uuid) DO UPDATE SET
                    name = excluded.name,
                    updated_at = excluded.updated_at",
                params![uuid, name, format_timestamp(Some(Utc::now()))],
            )?;

            let Some(previous_name) = previous_name else {
                return Ok(None);
            };
            if previous_name == name {
                return Ok(None);
            }

            let tx = connection.transaction()?;
            for table in ["player_activity", "player_activity_sessions"] {
                tx.execute(
                    &format!("UPDATE {table} SET player_name = ?1 WHERE player_name = ?2"),
                    params![name, previous_name],
                )?;
            }
            tx.commit()?;
            Ok(Some(previous_name))
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
                    self.server_uuid,
                    self.specialization,
                    player_name,
                    format_timestamp(Some(joined_at))
                ],
            )?;
            let session_id = tx.last_insert_rowid();
            upsert_player(
                &tx,
                &self.server_uuid,
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
                        self.server_uuid,
                        self.specialization,
                        player_name,
                        format_timestamp(Some(joined_at)),
                        format_timestamp(Some(left_at)),
                        duration_seconds
                    ],
                )?;
            }
            player.active_session_id = None;
            upsert_player(&tx, &self.server_uuid, &self.specialization, player_name, player, None)?;
            tx.commit()
        })
    }

    fn close_stale_sessions(&self) -> rusqlite::Result<()> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT player_name, current_session_started_at
                   FROM player_activity
                  WHERE server_name = ?1
                    AND current_session_started_at IS NOT NULL",
            )?;
            let rows = statement.query_map(params![self.server_uuid], |row| {
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
                      WHERE server_name = ?3 AND player_name = ?4",
                    params![
                        duration_seconds,
                        format_timestamp(Some(now)),
                        self.server_uuid,
                        player_name
                    ],
                )?;
                tx.execute(
                    "UPDATE player_activity_sessions
                        SET left_at = ?1, duration_seconds = ?2
                      WHERE server_name = ?3
                        AND player_name = ?4
                        AND left_at IS NULL",
                    params![
                        format_timestamp(Some(now)),
                        duration_seconds,
                        self.server_uuid,
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
                  WHERE server_name = ?1
                  ORDER BY joined_at DESC
                  LIMIT ?2",
            )?;
            let rows = statement.query_map(params![self.server_uuid, limit], |row| {
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
            })?;

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
                    self.server_uuid,
                    self.specialization,
                    format_timestamp(Some(Utc::now())),
                    online_count
                ],
            )?;
            Ok(())
        })
    }

    /// Returns rolling-window stats ordered smallest-to-largest window
    /// (day, week, month, year, all_time). This is a `Vec`, not a
    /// `serde_json::Map`, because `Map` is backed by a `BTreeMap` and would
    /// silently re-sort the windows alphabetically (day, month, week, year).
    fn timeframe_stats(&self) -> rusqlite::Result<Vec<Value>> {
        let now = Utc::now();
        let timeframes = [
            ("day", ChronoDuration::days(1)),
            ("week", ChronoDuration::weeks(1)),
            ("month", ChronoDuration::days(30)),
            ("year", ChronoDuration::days(365)),
        ];
        let mut stats = Vec::with_capacity(timeframes.len() + 1);

        for (name, duration) in timeframes {
            let start = now - duration;
            let mut summary = self.timeframe_summary(start, now)?;
            if let Some(object) = summary.as_object_mut() {
                object.insert("name".to_string(), Value::String(name.to_string()));
            }
            stats.push(summary);
        }

        // Unlike the fixed rolling windows above, "all_time" never expires, so a
        // player who was seen more than a year ago is still remembered here
        // instead of quietly falling out of every bucket.
        let earliest = self.earliest_activity_at()?.unwrap_or(now);
        let mut all_time = self.timeframe_summary(earliest, now)?;
        if let Some(object) = all_time.as_object_mut() {
            object.insert("name".to_string(), Value::String("all_time".to_string()));
        }
        stats.push(all_time);

        Ok(stats)
    }

    /// Earliest recorded join across this server's session history, used as the
    /// start of the "all_time" window so unique players are never dropped just
    /// because they predate the rolling year window.
    fn earliest_activity_at(&self) -> rusqlite::Result<Option<DateTime<Utc>>> {
        self.with_connection(|connection| {
            let earliest: Option<String> = connection.query_row(
                "SELECT MIN(joined_at) FROM player_activity_sessions WHERE server_name = ?1",
                params![self.server_uuid],
                |row| row.get(0),
            )?;
            Ok(parse_timestamp(earliest))
        })
    }

    fn timeframe_summary(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> rusqlite::Result<Value> {
        let samples = self.player_count_samples(start)?;
        let busy_by_hour = self.busy_by_hour(start)?;
        let logged_seconds = self.logged_seconds_between(start, end)?;
        let distinct_names = self.distinct_names_since(start)?;
        let average_online = average_online(&samples);
        let peak_online = samples
            .iter()
            .filter_map(|sample| sample.get("players").and_then(|value| value.as_i64()))
            .max()
            .unwrap_or(0);

        Ok(json!({
            "window": "rolling",
            "start": format_timestamp(Some(start)),
            "end": format_timestamp(Some(end)),
            "logged_seconds": logged_seconds,
            "logged_hours": seconds_to_hours(logged_seconds),
            "distinct_names": distinct_names,
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
                    AND sampled_at >= ?2
                  ORDER BY sampled_at ASC
                  LIMIT 1000",
            )?;
            let rows = statement.query_map(
                params![self.server_uuid, format_timestamp(Some(start))],
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
                    AND sampled_at >= ?2
                  GROUP BY hour
                  ORDER BY hour",
            )?;
            let rows = statement.query_map(
                params![self.server_uuid, format_timestamp(Some(start))],
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
                    AND joined_at <= ?2
                    AND (left_at IS NULL OR left_at >= ?3)",
            )?;
            let rows = statement.query_map(
                params![
                    self.server_uuid,
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

    fn distinct_names_since(&self, start: DateTime<Utc>) -> rusqlite::Result<i64> {
        self.with_connection(|connection| {
            connection.query_row(
                "SELECT COUNT(DISTINCT player_name)
                   FROM player_activity_sessions
                  WHERE server_name = ?1
                    AND (joined_at >= ?2 OR left_at >= ?2 OR left_at IS NULL)",
                params![self.server_uuid, format_timestamp(Some(start))],
                |row| row.get(0),
            )
        })
    }

    fn archived_server_stats(
        db_path: PathBuf,
        active_server_uuids: &[String],
    ) -> rusqlite::Result<Vec<Value>> {
        let active: BTreeSet<&str> = active_server_uuids.iter().map(String::as_str).collect();
        let store = Self {
            db_path: db_path.clone(),
            server_uuid: String::new(),
            display_name: String::new(),
            specialization: String::new(),
        };
        let metadata = store.with_connection(|connection| {
            initialize_schema(connection)?;
            let mut statement = connection.prepare(
                "SELECT server_uuid, display_name, specialization, last_seen_at
                   FROM player_activity_servers
                  ORDER BY display_name, server_uuid",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;

            let mut archives = Vec::new();
            for row in rows {
                archives.push(row?);
            }
            Ok(archives)
        })?;

        let mut archives = Vec::new();
        for (server_uuid, display_name, specialization, last_seen_at) in metadata {
            if active.contains(server_uuid.as_str()) {
                continue;
            }
            let scoped_store = Self {
                db_path: db_path.clone(),
                server_uuid: server_uuid.clone(),
                display_name: display_name.clone(),
                specialization: specialization.clone(),
            };
            let stats = scoped_store.timeframe_stats()?;
            // `player_activity_servers` gets a row every time a specialization
            // opens its store (see `open_at`), including for a server-uuid that
            // ends up churning on restart before ever seeing a real join. Those
            // rows never accumulate any session history, so once the uuid falls
            // out of the active set they'd otherwise show up here forever as an
            // empty, indistinguishable "Vanilla"/etc. card with nothing in it.
            // Skip anything that never actually observed a player.
            let observed_names = stats
                .last()
                .and_then(|entry| entry.get("distinct_names"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if observed_names == 0 {
                continue;
            }
            archives.push(json!({
                "server_uuid": server_uuid,
                "name": display_name,
                "specialization": specialization,
                "last_seen_at": last_seen_at,
                "stats": stats,
                "recent_sessions": scoped_store.recent_sessions(25)?,
                "observed_names": observed_names,
            }));
        }
        Ok(archives)
    }

    fn global_player_activity(db_path: PathBuf) -> rusqlite::Result<Vec<Value>> {
        let store = Self {
            db_path,
            server_uuid: String::new(),
            display_name: String::new(),
            specialization: String::new(),
        };
        store.with_connection(|connection| {
            initialize_schema(connection)?;
            // Sourced from `player_activity_sessions`, not the `player_activity`
            // aggregate table, so this list can never drift out of sync with
            // `distinct_names_since` (used by the rolling/all-time timeframe
            // cards) - sessions are the append-only ground truth, while the
            // aggregate table is mutated in place by upserts and rename
            // migrations that have historically been able to silently leave a
            // player's aggregate row missing or stale even though their
            // session history is intact.
            let mut statement = connection.prepare(
                "SELECT ps.player_name,
                        ps.server_name,
                        COALESCE(s.display_name, ps.server_name) AS server_display_name,
                        MAX(ps.specialization) AS specialization,
                        SUM(COALESCE(ps.duration_seconds, 0)) AS total_seconds,
                        MAX(CASE WHEN ps.left_at IS NULL THEN ps.joined_at END) AS current_session_started_at,
                        MAX(ps.joined_at) AS last_joined_at,
                        MAX(ps.left_at) AS last_left_at,
                        COUNT(*) AS session_count
                   FROM player_activity_sessions ps
                   LEFT JOIN player_activity_servers s ON s.server_uuid = ps.server_name
                  GROUP BY ps.player_name, ps.server_name, server_display_name
                  ORDER BY ps.player_name, server_display_name",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })?;

            let mut players: Vec<(String, Vec<Value>)> = Vec::new();
            for row in rows {
                let (
                    player_name,
                    server_uuid,
                    server_display_name,
                    specialization,
                    total_seconds,
                    current_session_started_at,
                    last_joined_at,
                    last_left_at,
                    session_count,
                ) = row?;
                let entry = json!({
                    "server_uuid": server_uuid,
                    "server_name": server_display_name,
                    "specialization": specialization,
                    "online": current_session_started_at.is_some(),
                    "total_seconds": total_seconds,
                    "total_hours": seconds_to_hours(total_seconds),
                    "last_joined_at": last_joined_at,
                    "last_left_at": last_left_at,
                    "session_count": session_count,
                });
                match players.last_mut() {
                    Some((name, servers)) if *name == player_name => servers.push(entry),
                    _ => players.push((player_name, vec![entry])),
                }
            }

            let mut summaries: Vec<(bool, Option<DateTime<Utc>>, Value)> = players
                .into_iter()
                .map(|(player_name, servers)| {
                    let online = servers
                        .iter()
                        .any(|server| server.get("online").and_then(Value::as_bool) == Some(true));
                    let total_seconds: i64 = servers
                        .iter()
                        .filter_map(|server| server.get("total_seconds").and_then(Value::as_i64))
                        .sum();
                    let last_joined_at = servers
                        .iter()
                        .filter_map(|server| {
                            server
                                .get("last_joined_at")
                                .and_then(Value::as_str)
                                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                        })
                        .max();
                    let sort_key = last_joined_at.map(|value| value.with_timezone(&Utc));
                    let summary = json!({
                        "name": player_name,
                        "online": online,
                        "total_seconds": total_seconds,
                        "total_hours": seconds_to_hours(total_seconds),
                        "last_joined_at": last_joined_at.map(|value| value.to_rfc3339()),
                        "servers": servers,
                    });
                    (online, sort_key, summary)
                })
                .collect();

            summaries.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then_with(|| right.1.cmp(&left.1))
            });

            Ok(summaries
                .into_iter()
                .map(|(_, _, summary)| summary)
                .collect())
        })
    }

    fn delete_server_stats(db_path: PathBuf, server_uuid: &str) -> rusqlite::Result<()> {
        let store = Self {
            db_path,
            server_uuid: server_uuid.to_string(),
            display_name: String::new(),
            specialization: String::new(),
        };
        store.with_connection(|connection| {
            let tx = connection.transaction()?;
            tx.execute(
                "DELETE FROM player_activity WHERE server_name = ?1",
                params![server_uuid],
            )?;
            tx.execute(
                "DELETE FROM player_activity_sessions WHERE server_name = ?1",
                params![server_uuid],
            )?;
            tx.execute(
                "DELETE FROM player_count_samples WHERE server_name = ?1",
                params![server_uuid],
            )?;
            tx.execute(
                "DELETE FROM player_activity_servers WHERE server_uuid = ?1",
                params![server_uuid],
            )?;
            tx.commit()
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
            PRIMARY KEY (server_name, player_name)
        );

        CREATE TABLE IF NOT EXISTS player_activity_by_server (
            server_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            player_name TEXT NOT NULL,
            total_seconds INTEGER NOT NULL DEFAULT 0,
            current_session_started_at TEXT,
            last_joined_at TEXT,
            last_left_at TEXT,
            session_count INTEGER NOT NULL DEFAULT 0,
            active_session_id INTEGER,
            PRIMARY KEY (server_name, player_name)
        );

        INSERT OR REPLACE INTO player_activity_by_server
            (server_name, specialization, player_name, total_seconds,
             current_session_started_at, last_joined_at, last_left_at, session_count, active_session_id)
        SELECT server_name,
               MAX(specialization),
               player_name,
               SUM(total_seconds),
               MAX(current_session_started_at),
               MAX(last_joined_at),
               MAX(last_left_at),
               SUM(session_count),
               MAX(active_session_id)
          FROM player_activity
         GROUP BY server_name, player_name;

        DROP TABLE player_activity;
        ALTER TABLE player_activity_by_server RENAME TO player_activity;

        CREATE INDEX IF NOT EXISTS idx_player_activity_server_lookup
            ON player_activity (server_name, player_name);

        CREATE TABLE IF NOT EXISTS player_activity_servers (
            server_uuid TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            last_seen_at TEXT
        );

        INSERT OR IGNORE INTO player_activity_servers
            (server_uuid, display_name, specialization, last_seen_at)
        SELECT server_name, server_name, MAX(specialization), MAX(last_joined_at)
          FROM player_activity
         GROUP BY server_name;

        CREATE TABLE IF NOT EXISTS player_activity_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            player_name TEXT NOT NULL,
            joined_at TEXT NOT NULL,
            left_at TEXT,
            duration_seconds INTEGER
        );

        DROP INDEX IF EXISTS idx_player_activity_sessions_lookup;
        CREATE INDEX IF NOT EXISTS idx_player_activity_sessions_lookup
            ON player_activity_sessions (server_name, player_name, joined_at);

        CREATE TABLE IF NOT EXISTS player_count_samples (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            sampled_at TEXT NOT NULL,
            online_count INTEGER NOT NULL
        );

        DROP INDEX IF EXISTS idx_player_count_samples_lookup;
        CREATE INDEX IF NOT EXISTS idx_player_count_samples_lookup
            ON player_count_samples (server_name, sampled_at);

        CREATE TABLE IF NOT EXISTS player_identities (
            uuid TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            updated_at TEXT
        );
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
    let updated = connection.execute(
        "UPDATE player_activity
            SET specialization = ?1,
                total_seconds = ?2,
                current_session_started_at = ?3,
                last_joined_at = ?4,
                last_left_at = ?5,
                session_count = ?6,
                active_session_id = ?7
          WHERE server_name = ?8 AND player_name = ?9",
        params![
            specialization,
            player.total_seconds,
            format_timestamp(player.current_session_started_at),
            format_timestamp(player.last_joined_at),
            format_timestamp(player.last_left_at),
            player.session_count,
            active_session_id,
            server_name,
            player_name
        ],
    )?;

    if updated == 0 {
        connection.execute(
            "INSERT INTO player_activity
                (server_name, specialization, player_name, total_seconds,
                 current_session_started_at, last_joined_at, last_left_at, session_count, active_session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
    }

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
        let store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-one", "Server One", "Minecraft")?;
        let mut tracker = PlayerActivityTracker {
            players: store.load_players()?,
            store: Some(store),
        last_periodic_sample_at: None,
        };

        assert!(tracker.player_joined("PlayerOne"));
        assert!(tracker.player_left("PlayerOne"));

        let reloaded_store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-one", "Server One", "Minecraft")?;
        let reloaded_tracker = PlayerActivityTracker {
            players: reloaded_store.load_players()?,
            store: Some(reloaded_store),
        last_periodic_sample_at: None,
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
        let timeframe_names: Vec<&str> = timeframe_stats
            .as_array()
            .expect("timeframe stats should be an array")
            .iter()
            .filter_map(|entry| entry.get("name").and_then(Value::as_str))
            .collect();
        assert_eq!(
            timeframe_names,
            vec!["day", "week", "month", "year", "all_time"]
        );
        let day = timeframe_stats
            .as_array()
            .and_then(|entries| entries.first())
            .expect("day stats should be present");
        assert!(day.get("logged_hours").is_some());
        assert!(day
            .get("player_count_samples")
            .and_then(|samples| samples.as_array())
            .is_some_and(|samples| !samples.is_empty()));

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn sqlite_store_keeps_same_name_separate_by_server_instance() -> rusqlite::Result<()> {
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let scopes = [
            ("uuid-one", "Server One", "Minecraft"),
            ("uuid-two", "Server Two", "Minecraft"),
            ("uuid-one", "Server One", "Terraria"),
        ];

        for (server_uuid, display_name, specialization) in scopes {
            let store = PlayerActivityStore::open_at(
                db_path.clone(),
                server_uuid,
                display_name,
                specialization,
            )?;
            let mut tracker = PlayerActivityTracker {
                players: store.load_players()?,
                store: Some(store),
            last_periodic_sample_at: None,
            };
            assert!(tracker.player_joined("SharedName"));
            assert!(tracker.player_left("SharedName"));
        }

        let connection = Connection::open(&db_path)?;
        let stored_rows: i64 = connection.query_row(
            "SELECT COUNT(*)
               FROM player_activity
              WHERE player_name = 'SharedName'",
            [],
            |row| row.get(0),
        )?;
        let stored_sessions: i64 = connection.query_row(
            "SELECT COUNT(*)
               FROM player_activity_sessions
              WHERE player_name = 'SharedName'",
            [],
            |row| row.get(0),
        )?;

        assert_eq!(stored_rows, 2);
        assert_eq!(stored_sessions, 3);

        let server_one_store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-one", "Server One", "Minecraft")?;
        let server_one_tracker = PlayerActivityTracker {
            players: server_one_store.load_players()?,
            store: Some(server_one_store),
        last_periodic_sample_at: None,
        };
        assert_eq!(server_one_tracker.known_player_count(), 1);
        assert_eq!(
            server_one_tracker
                .recent_sessions(10)
                .as_array()
                .map(Vec::len),
            Some(2)
        );

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn timeframe_stats_reports_week_as_rolling_seven_day_window() -> rusqlite::Result<()> {
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-one", "Server One", "Minecraft")?;
        let now = Utc::now();
        let recent_joined = now - ChronoDuration::hours(2);
        let recent_left = now - ChronoDuration::hours(1);
        let old_joined = now - ChronoDuration::days(9);
        let old_left = now - ChronoDuration::days(8);

        store.with_connection(|connection| {
            connection.execute(
                "INSERT INTO player_activity_sessions
                    (server_name, specialization, player_name, joined_at, left_at, duration_seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "uuid-one",
                    "Minecraft",
                    "RecentName",
                    format_timestamp(Some(recent_joined)),
                    format_timestamp(Some(recent_left)),
                    elapsed_seconds(recent_joined, recent_left)
                ],
            )?;
            connection.execute(
                "INSERT INTO player_activity_sessions
                    (server_name, specialization, player_name, joined_at, left_at, duration_seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "uuid-one",
                    "Minecraft",
                    "OldName",
                    format_timestamp(Some(old_joined)),
                    format_timestamp(Some(old_left)),
                    elapsed_seconds(old_joined, old_left)
                ],
            )?;
            Ok(())
        })?;

        let stats = store.timeframe_stats()?;
        let week = stats
            .iter()
            .find(|entry| entry.get("name").and_then(Value::as_str) == Some("week"))
            .expect("week stats should be present");
        let start = parse_timestamp(
            week.get("start")
                .and_then(Value::as_str)
                .map(str::to_string),
        )
        .expect("week start should parse");
        let end = parse_timestamp(week.get("end").and_then(Value::as_str).map(str::to_string))
            .expect("week end should parse");

        assert_eq!(week.get("window").and_then(Value::as_str), Some("rolling"));
        assert_eq!(end.signed_duration_since(start), ChronoDuration::weeks(1));
        assert_eq!(
            week.get("logged_seconds").and_then(Value::as_i64),
            Some(3600)
        );
        assert_eq!(week.get("distinct_names").and_then(Value::as_i64), Some(1));

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn archived_stats_lists_and_deletes_removed_server_data() -> rusqlite::Result<()> {
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let active_store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-active", "Active", "Minecraft")?;
        let archived_store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-archived", "Archived", "Terraria")?;
        let mut archived_tracker = PlayerActivityTracker {
            players: archived_store.load_players()?,
            store: Some(archived_store),
        last_periodic_sample_at: None,
        };
        assert!(archived_tracker.player_joined("ArchivedName"));
        assert!(archived_tracker.player_left("ArchivedName"));

        let archives =
            PlayerActivityStore::archived_server_stats(db_path.clone(), &["uuid-active".into()])?;
        assert_eq!(archives.len(), 1);
        assert_eq!(
            archives[0].get("server_uuid").and_then(Value::as_str),
            Some("uuid-archived")
        );

        PlayerActivityStore::delete_server_stats(db_path.clone(), "uuid-archived")?;
        let archives =
            PlayerActivityStore::archived_server_stats(db_path.clone(), &["uuid-active".into()])?;
        assert!(archives.is_empty());

        drop(active_store);
        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn archived_stats_skips_servers_that_never_saw_a_player() -> rusqlite::Result<()> {
        // Regression test: opening a store (e.g. a server-uuid that churned on
        // restart before anyone ever joined) always registers a
        // `player_activity_servers` row via `upsert_server_metadata`, even
        // though it has no session history. Once such a uuid falls out of the
        // active set it must not show up as a hollow "Retained Server Data"
        // card with nothing in it.
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let empty_store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-empty", "Vanilla", "Minecraft")?;
        drop(empty_store);

        let real_store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-real", "Vanilla", "Minecraft")?;
        let mut real_tracker = PlayerActivityTracker {
            players: real_store.load_players()?,
            store: Some(real_store),
        last_periodic_sample_at: None,
        };
        assert!(real_tracker.player_joined("RealName"));
        assert!(real_tracker.player_left("RealName"));

        let archives = PlayerActivityStore::archived_server_stats(db_path.clone(), &[])?;
        let archived_uuids: Vec<&str> = archives
            .iter()
            .filter_map(|archive| archive.get("server_uuid").and_then(Value::as_str))
            .collect();
        assert_eq!(archived_uuids, vec!["uuid-real"]);

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn global_player_activity_includes_players_missing_from_aggregate_table() -> rusqlite::Result<()>
    {
        // Regression test: the "User Activity" list used to be sourced from the
        // mutable `player_activity` aggregate table, which could drift out of
        // sync with (and undercount relative to) `player_activity_sessions` -
        // the same append-only table `distinct_names_since` counts against for
        // the timeframe cards. A player with real session history but no (or a
        // stale) aggregate row would silently vanish from the remembered-users
        // list while still counting toward "Distinct Names" elsewhere.
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-one", "Server One", "Minecraft")?;
        let joined_at = Utc::now() - ChronoDuration::days(1);
        let left_at = joined_at + ChronoDuration::hours(1);

        store.with_connection(|connection| {
            // Only a session row exists for this player - no matching row in
            // the `player_activity` aggregate table at all.
            connection.execute(
                "INSERT INTO player_activity_sessions
                    (server_name, specialization, player_name, joined_at, left_at, duration_seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "uuid-one",
                    "Minecraft",
                    "SessionOnlyName",
                    format_timestamp(Some(joined_at)),
                    format_timestamp(Some(left_at)),
                    elapsed_seconds(joined_at, left_at)
                ],
            )?;
            Ok(())
        })?;

        let players = PlayerActivityStore::global_player_activity(db_path.clone())?;
        let names: Vec<&str> = players
            .iter()
            .filter_map(|player| player.get("name").and_then(Value::as_str))
            .collect();
        assert_eq!(names, vec!["SessionOnlyName"]);

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }
}
