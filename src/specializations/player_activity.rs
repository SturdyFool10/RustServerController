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

/// Most players listed per game in the network-wide player list. The full set
/// can be millions of players; the stats page only ever needs the busiest/most
/// recent ones, and per-game/per-server totals are reported separately.
const GLOBAL_PLAYERS_PER_GAME: usize = 200;

/// Most players included in one server's own "User Activity" stat.
const SERVER_SUMMARY_LIMIT: usize = 200;

/// How long expensive aggregate queries are reused before a background refresh.
const AGGREGATE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);
const TIMEFRAME_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Default)]
struct SwrState {
    value: Option<Value>,
    refreshed_at: Option<std::time::Instant>,
    refreshing: bool,
}

/// Stale-while-revalidate cache for expensive aggregate queries: callers get
/// the last computed value immediately, and a stale value triggers one
/// background recomputation, so slow scans over huge session tables never run
/// on (or block) the server loop or a websocket request.
#[derive(Clone, Default)]
struct SwrCache(std::sync::Arc<std::sync::Mutex<SwrState>>);

impl std::fmt::Debug for SwrCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SwrCache")
    }
}

impl SwrCache {
    /// `block_if_cold`: compute inline when there's nothing cached yet
    /// (otherwise the first caller would get an empty placeholder).
    fn get(
        &self,
        ttl: std::time::Duration,
        block_if_cold: bool,
        empty: Value,
        compute: impl FnOnce() -> Value + Send + 'static,
    ) -> Value {
        let spawn_refresh = {
            let Ok(mut state) = self.0.lock() else {
                return empty;
            };
            if let Some(value) = state.value.clone() {
                let fresh = state.refreshed_at.is_some_and(|at| at.elapsed() < ttl);
                if fresh || state.refreshing {
                    return value;
                }
                state.refreshing = true;
                Some(value)
            } else if state.refreshing {
                return empty;
            } else {
                state.refreshing = true;
                None
            }
        };

        let shared = self.0.clone();
        let store = move |value: Value| {
            if let Ok(mut state) = shared.lock() {
                state.value = Some(value);
                state.refreshed_at = Some(std::time::Instant::now());
                state.refreshing = false;
            }
        };
        match spawn_refresh {
            Some(stale) => {
                std::thread::spawn(move || store(compute()));
                stale
            }
            None if block_if_cold => {
                let value = compute();
                store(value.clone());
                value
            }
            None => {
                std::thread::spawn(move || store(compute()));
                empty
            }
        }
    }

    /// Forget the cached value so the next `get` recomputes it.
    fn clear(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.value = None;
            state.refreshed_at = None;
        }
    }
}

fn global_cache() -> &'static SwrCache {
    static CACHE: std::sync::OnceLock<SwrCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(SwrCache::default)
}

fn archived_cache() -> &'static SwrCache {
    static CACHE: std::sync::OnceLock<SwrCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(SwrCache::default)
}

/// Tracks in-memory name sessions for a single running server instance.
#[derive(Clone, Debug, Default)]
pub struct PlayerActivityTracker {
    players: BTreeMap<String, PlayerActivity>,
    store: Option<PlayerActivityStore>,
    last_periodic_sample_at: Option<DateTime<Utc>>,
    timeframe_cache: SwrCache,
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
                    timeframe_cache: SwrCache::default(),
                };
                // Warm the (slow) timeframe stats off-thread.
                let _ = tracker.timeframe_stats();
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

    /// Like [`Self::player_joined`], but for games that report a stable
    /// player UUID alongside the (changeable) name.
    ///
    /// If this UUID was previously seen on this server under a different
    /// name, everything recorded under the old name (sessions, totals, the
    /// in-memory entry) is migrated to the new name first, so the player keeps
    /// one continuous history. Returns the previous name when a rename was
    /// migrated, alongside whether a new session was started.
    pub fn player_joined_with_uuid(&mut self, uuid: &str, name: &str) -> (bool, Option<String>) {
        let renamed_from = self.sync_identity(uuid, name);
        (self.player_joined(name), renamed_from)
    }

    fn sync_identity(&mut self, uuid: &str, name: &str) -> Option<String> {
        let (uuid, name) = (uuid.trim(), name.trim());
        if uuid.is_empty() || name.is_empty() {
            return None;
        }
        let store = self.store.as_ref()?;
        let previous = match store.sync_server_identity(uuid, name) {
            Ok(previous) => previous?,
            Err(error) => {
                tracing::warn!("Failed to sync player identity for '{}': {}", name, error);
                return None;
            }
        };

        if let Some(old) = self.players.remove(&previous) {
            match self.players.get_mut(name) {
                Some(existing) => {
                    existing.total_seconds += old.total_seconds;
                    existing.session_count += old.session_count;
                    existing.last_joined_at = existing.last_joined_at.max(old.last_joined_at);
                    existing.last_left_at = existing.last_left_at.max(old.last_left_at);
                }
                None => {
                    self.players.insert(name.to_string(), old);
                }
            }
        }
        Some(previous)
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

    /// Summaries for the busiest slice of players (online first, then most
    /// recently seen), capped so a server with millions of known players
    /// doesn't serialize all of them on every stats update.
    pub fn summaries(&self) -> Value {
        let mut entries: Vec<(&String, &PlayerActivity)> = self.players.iter().collect();
        let rank = |player: &PlayerActivity| {
            (
                player.current_session_started_at.is_some(),
                player.last_joined_at,
            )
        };
        let by_rank = |left: &(&String, &PlayerActivity), right: &(&String, &PlayerActivity)| {
            rank(right.1).cmp(&rank(left.1))
        };
        if entries.len() > SERVER_SUMMARY_LIMIT {
            entries.select_nth_unstable_by(SERVER_SUMMARY_LIMIT, by_rank);
            entries.truncate(SERVER_SUMMARY_LIMIT);
        }
        entries.sort_by(|left, right| by_rank(left, right).then_with(|| left.0.cmp(right.0)));
        Value::Array(
            entries
                .into_iter()
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

    /// Never blocks: returns the last computed stats (empty until the first
    /// background computation finishes) and refreshes them off-thread.
    pub fn timeframe_stats(&self) -> Value {
        let Some(store) = self.store.clone() else {
            return Value::Array(Vec::new());
        };
        self.timeframe_cache.get(
            TIMEFRAME_CACHE_TTL,
            false,
            Value::Array(Vec::new()),
            move || match store.timeframe_stats() {
                Ok(stats) => Value::Array(stats),
                Err(error) => {
                    tracing::warn!("Failed to load player timeframe stats: {}", error);
                    Value::Array(Vec::new())
                }
            },
        )
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

/// Records the current name for a known player UUID of the given
/// `specialization` (e.g. observed from a Minecraft server's `usercache.json`), migrating all recorded activity from
/// any previously-known name for that UUID. Returns the previous name if a
/// rename was detected and migrated, so callers can also update any other
/// name-keyed records (e.g. whitelist/ban list entries) that reference it.
pub fn sync_player_identity(specialization: &str, uuid: &str, name: &str) -> Option<String> {
    let uuid = uuid.trim();
    let name = name.trim();
    if uuid.is_empty() || name.is_empty() {
        return None;
    }
    match PlayerActivityStore::sync_player_identity(database_path(), specialization, uuid, name) {
        Ok(renamed_from) => renamed_from,
        Err(error) => {
            tracing::warn!("Failed to sync player identity for '{}': {}", name, error);
            None
        }
    }
}

/// Stats for servers that no longer exist, minus any in `active_server_uuids`.
/// Computed for every known server and cached, so a changing active set never
/// forces a recompute.
pub fn archived_server_stats(active_server_uuids: &[String]) -> Value {
    let all = archived_cache().get(
        AGGREGATE_CACHE_TTL,
        true,
        Value::Array(Vec::new()),
        || match PlayerActivityStore::archived_server_stats(database_path(), &[]) {
            Ok(stats) => Value::Array(stats),
            Err(error) => {
                tracing::warn!("Failed to load archived player activity stats: {}", error);
                Value::Array(Vec::new())
            }
        },
    );
    match all {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .filter(|item| {
                    !item
                        .get("server_uuid")
                        .and_then(Value::as_str)
                        .is_some_and(|uuid| active_server_uuids.iter().any(|active| active == uuid))
                })
                .collect(),
        ),
        other => other,
    }
}

/// Network-wide player activity, kept separate per game (the same name in two
/// different games is two different players):
///
/// * `players` - the busiest [`GLOBAL_PLAYERS_PER_GAME`] players of each game,
///   each with a per-server breakdown;
/// * `server_totals` - one row per server (player count, online count, total
///   seconds), from which any total can be summed without listing every
///   player, and which permission filtering can apply per server.
///
/// Cached and refreshed in the background; safe to call from a request path.
pub fn global_player_activity() -> Value {
    global_cache().get(
        AGGREGATE_CACHE_TTL,
        true,
        json!({ "players": [], "server_totals": [] }),
        || {
            let db_path = database_path();
            let players = PlayerActivityStore::global_player_activity(db_path.clone())
                .unwrap_or_else(|error| {
                    tracing::warn!("Failed to load global player activity: {}", error);
                    Vec::new()
                });
            let server_totals = PlayerActivityStore::global_server_totals(db_path)
                .unwrap_or_else(|error| {
                    tracing::warn!("Failed to load server player totals: {}", error);
                    Vec::new()
                });
            json!({ "players": players, "server_totals": server_totals })
        },
    )
}

pub fn delete_server_stats(server_uuid: &str) -> rusqlite::Result<()> {
    let result = PlayerActivityStore::delete_server_stats(database_path(), server_uuid);
    global_cache().clear();
    archived_cache().clear();
    result
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
        store.with_connection(|_| Ok(()))?;
        store.upsert_server_metadata()?;
        Ok(store)
    }

    fn load_players(&self) -> rusqlite::Result<BTreeMap<String, PlayerActivity>> {
        self.close_stale_sessions()?;
        self.with_connection(|connection| {
            // Sourced from `player_activity_sessions`, not the `player_activity`
            // aggregate table, for the same reason as `global_player_activity`:
            // sessions are append-only ground truth, while the aggregate table
            // is mutated in place by upserts/rename migrations that have been
            // able to silently leave a player's row missing or stale. This is
            // what seeds the in-memory tracker, so it's what every per-server
            // "known players" view (online count, summaries, etc.) is built on.
            let mut statement = connection.prepare(
                "SELECT player_name,
                        SUM(COALESCE(duration_seconds, 0)) AS total_seconds,
                        MAX(CASE WHEN left_at IS NULL THEN joined_at END) AS current_session_started_at,
                        MAX(joined_at) AS last_joined_at,
                        MAX(left_at) AS last_left_at,
                        COUNT(*) AS session_count,
                        MAX(CASE WHEN left_at IS NULL THEN id END) AS active_session_id
                   FROM player_activity_sessions
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
            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
        specialization: &str,
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

            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            for table in ["player_activity", "player_activity_sessions"] {
                tx.execute(
                    // Only this game's rows: the same name in another game is
                    // a different player.
                    &format!(
                        "UPDATE {table} SET player_name = ?1
                          WHERE player_name = ?2 AND specialization = ?3"
                    ),
                    params![name, previous_name, specialization],
                )?;
            }
            tx.commit()?;
            Ok(Some(previous_name))
        })
    }

    /// Records `name` as the current name of `uuid` on this server, and if the
    /// UUID was previously recorded here under a different name, migrates that
    /// name's sessions and aggregate row to the new name (merging into an
    /// existing row for the new name if there is one). Returns the previous
    /// name when a migration happened. Scoped to this server only, so another
    /// server's (or another game's) same-named players are never touched.
    fn sync_server_identity(&self, uuid: &str, name: &str) -> rusqlite::Result<Option<String>> {
        self.with_connection(|connection| {
            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let previous: Option<String> = tx
                .query_row(
                    "SELECT name FROM player_server_identities WHERE server_name = ?1 AND uuid = ?2",
                    params![self.server_uuid, uuid],
                    |row| row.get(0),
                )
                .optional()?;
            tx.execute(
                "INSERT INTO player_server_identities (server_name, uuid, name, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(server_name, uuid) DO UPDATE SET
                    name = excluded.name,
                    updated_at = excluded.updated_at",
                params![
                    self.server_uuid,
                    uuid,
                    name,
                    format_timestamp(Some(Utc::now()))
                ],
            )?;
            let Some(previous) = previous.filter(|previous| previous != name) else {
                tx.commit()?;
                return Ok(None);
            };

            tx.execute(
                "UPDATE player_activity_sessions SET player_name = ?1
                  WHERE server_name = ?2 AND player_name = ?3",
                params![name, self.server_uuid, previous],
            )?;

            type AggregateRow = (i64, i64, Option<String>, Option<String>);
            let read_row = |tx: &rusqlite::Transaction, player: &str| -> rusqlite::Result<Option<AggregateRow>> {
                tx.query_row(
                    "SELECT total_seconds, session_count, last_joined_at, last_left_at
                       FROM player_activity WHERE server_name = ?1 AND player_name = ?2",
                    params![self.server_uuid, player],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
            };
            if let Some(old) = read_row(&tx, &previous)? {
                if let Some(existing) = read_row(&tx, name)? {
                    tx.execute(
                        "UPDATE player_activity
                            SET total_seconds = ?1, session_count = ?2,
                                last_joined_at = ?3, last_left_at = ?4
                          WHERE server_name = ?5 AND player_name = ?6",
                        params![
                            existing.0 + old.0,
                            existing.1 + old.1,
                            existing.2.clone().max(old.2.clone()),
                            existing.3.clone().max(old.3.clone()),
                            self.server_uuid,
                            name
                        ],
                    )?;
                    tx.execute(
                        "DELETE FROM player_activity WHERE server_name = ?1 AND player_name = ?2",
                        params![self.server_uuid, previous],
                    )?;
                } else {
                    tx.execute(
                        "UPDATE player_activity SET player_name = ?1
                          WHERE server_name = ?2 AND player_name = ?3",
                        params![name, self.server_uuid, previous],
                    )?;
                }
            }
            tx.commit()?;
            Ok(Some(previous))
        })
    }

    fn persist_join(
        &self,
        player_name: &str,
        player: &PlayerActivity,
        joined_at: DateTime<Utc>,
    ) -> rusqlite::Result<i64> {
        self.with_connection(|connection| {
            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
            // Keyed off `player_activity_sessions` by row id, not by joining
            // through `player_activity`, so a session left dangling open for a
            // player whose aggregate row is missing or stale still gets closed
            // instead of being invisible to this cleanup.
            let mut statement = connection.prepare(
                "SELECT id, player_name, joined_at
                   FROM player_activity_sessions
                  WHERE server_name = ?1
                    AND left_at IS NULL",
            )?;
            let rows = statement.query_map(params![self.server_uuid], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?;

            let now = Utc::now();
            let mut stale_sessions = Vec::new();
            for row in rows {
                let (id, player_name, joined_at) = row?;
                let Some(joined_at) = parse_timestamp(joined_at) else {
                    continue;
                };
                stale_sessions.push((id, player_name, joined_at));
            }
            drop(statement);

            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            for (id, player_name, joined_at) in stale_sessions {
                let duration_seconds = elapsed_seconds(joined_at, now);
                tx.execute(
                    "UPDATE player_activity_sessions
                        SET left_at = ?1, duration_seconds = ?2
                      WHERE id = ?3",
                    params![format_timestamp(Some(now)), duration_seconds, id],
                )?;
                // Best-effort: keep the aggregate table's copy of this player in
                // sync too, for anything still reading it. A missing row here
                // is fine (this UPDATE simply matches zero rows).
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

    /// The busiest [`GLOBAL_PLAYERS_PER_GAME`] players of each game, with a
    /// per-server breakdown. Sourced from `player_activity_sessions` (the
    /// append-only ground truth) rather than the aggregate table, which has
    /// historically been able to drift. Players are keyed by (game, name).
    fn global_player_activity(db_path: PathBuf) -> rusqlite::Result<Vec<Value>> {
        let store = Self {
            db_path,
            server_uuid: String::new(),
            display_name: String::new(),
            specialization: String::new(),
        };
        store.with_connection(|connection| {
            let games: Vec<String> = connection
                .prepare("SELECT DISTINCT specialization FROM player_activity_sessions")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;

            let mut top_statement = connection.prepare(
                "SELECT player_name,
                        MAX(CASE WHEN left_at IS NULL THEN 1 ELSE 0 END) AS online,
                        MAX(joined_at) AS last_joined
                   FROM player_activity_sessions
                  WHERE specialization = ?1
                  GROUP BY player_name
                  ORDER BY online DESC, last_joined DESC
                  LIMIT ?2",
            )?;
            let mut server_statement = connection.prepare(
                "SELECT ps.server_name,
                        COALESCE(s.display_name, ps.server_name),
                        SUM(COALESCE(ps.duration_seconds, 0)),
                        MAX(CASE WHEN ps.left_at IS NULL THEN ps.joined_at END),
                        MAX(ps.joined_at),
                        MAX(ps.left_at),
                        COUNT(*)
                   FROM player_activity_sessions ps
                   LEFT JOIN player_activity_servers s ON s.server_uuid = ps.server_name
                  WHERE ps.specialization = ?1 AND ps.player_name = ?2
                  GROUP BY ps.server_name
                  ORDER BY 2",
            )?;

            let mut summaries: Vec<(bool, Option<DateTime<Utc>>, Value)> = Vec::new();
            for game in &games {
                let top: Vec<String> = top_statement
                    .query_map(params![game, GLOBAL_PLAYERS_PER_GAME as i64], |row| {
                        row.get(0)
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                for player_name in top {
                    let servers: Vec<Value> = server_statement
                        .query_map(params![game, player_name], |row| {
                            let total_seconds: i64 = row.get(2)?;
                            let current: Option<String> = row.get(3)?;
                            Ok(json!({
                                "server_uuid": row.get::<_, String>(0)?,
                                "server_name": row.get::<_, String>(1)?,
                                "specialization": game,
                                "online": current.is_some(),
                                "total_seconds": total_seconds,
                                "total_hours": seconds_to_hours(total_seconds),
                                "last_joined_at": row.get::<_, Option<String>>(4)?,
                                "last_left_at": row.get::<_, Option<String>>(5)?,
                                "session_count": row.get::<_, i64>(6)?,
                            }))
                        })?
                        .collect::<rusqlite::Result<_>>()?;

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
                    summaries.push((
                        online,
                        last_joined_at.map(|value| value.with_timezone(&Utc)),
                        json!({
                            "name": player_name,
                            "specialization": game,
                            "online": online,
                            "total_seconds": total_seconds,
                            "total_hours": seconds_to_hours(total_seconds),
                            "last_joined_at": last_joined_at.map(|value| value.to_rfc3339()),
                            "servers": servers,
                        }),
                    ));
                }
            }

            summaries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
            Ok(summaries.into_iter().map(|(_, _, summary)| summary).collect())
        })
    }

    /// One row per server: distinct players, how many are online, and total
    /// logged seconds. Cheap to ship and safe to filter per server.
    fn global_server_totals(db_path: PathBuf) -> rusqlite::Result<Vec<Value>> {
        let store = Self {
            db_path,
            server_uuid: String::new(),
            display_name: String::new(),
            specialization: String::new(),
        };
        store.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT ps.server_name,
                        COALESCE(s.display_name, ps.server_name),
                        MAX(ps.specialization),
                        COUNT(DISTINCT ps.player_name),
                        COUNT(DISTINCT CASE WHEN ps.left_at IS NULL THEN ps.player_name END),
                        SUM(COALESCE(ps.duration_seconds, 0))
                   FROM player_activity_sessions ps
                   LEFT JOIN player_activity_servers s ON s.server_uuid = ps.server_name
                  GROUP BY ps.server_name
                  ORDER BY 2",
            )?;
            let rows = statement.query_map([], |row| {
                let total_seconds: i64 = row.get(5)?;
                Ok(json!({
                    "server_uuid": row.get::<_, String>(0)?,
                    "server_name": row.get::<_, String>(1)?,
                    "specialization": row.get::<_, String>(2)?,
                    "players": row.get::<_, i64>(3)?,
                    "online": row.get::<_, i64>(4)?,
                    "total_seconds": total_seconds,
                    "total_hours": seconds_to_hours(total_seconds),
                }))
            })?;
            rows.collect()
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
            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
        // Many servers (and controllers' background refreshes) write to this
        // one database; wait for the lock instead of failing the write.
        connection.busy_timeout(std::time::Duration::from_secs(30))?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        if !schema_ready(&self.db_path) {
            // Serialize first-time setup: several servers open their stores at
            // once on startup, and concurrent DDL/migration would collide.
            static SCHEMA_SETUP: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let _setup = SCHEMA_SETUP.lock().unwrap_or_else(|e| e.into_inner());
            if !schema_ready(&self.db_path) {
                // Switching to WAL needs an exclusive lock (and ignores the
                // busy timeout), so it happens once per database, here. The
                // mode is stored in the file and applies to every connection.
                connection.pragma_update(None, "journal_mode", "WAL")?;
                initialize_schema(&connection)?;
                mark_schema_ready(&self.db_path);
            }
        }
        action(&mut connection)
    }
}

/// Paths whose schema has already been initialized by this process, so the
/// (potentially expensive) setup below runs once per database, not per query.
fn schema_ready(db_path: &Path) -> bool {
    static READY: std::sync::OnceLock<std::sync::Mutex<BTreeSet<PathBuf>>> =
        std::sync::OnceLock::new();
    READY
        .get_or_init(Default::default)
        .lock()
        .map(|ready| ready.contains(db_path))
        .unwrap_or(false)
}

fn mark_schema_ready(db_path: &Path) {
    static READY: std::sync::OnceLock<std::sync::Mutex<BTreeSet<PathBuf>>> =
        std::sync::OnceLock::new();
    if let Ok(mut ready) = READY.get_or_init(Default::default).lock() {
        ready.insert(db_path.to_path_buf());
    }
}

fn sqlite_master_sql(connection: &Connection, kind: &str, name: &str) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
            params![kind, name],
            |row| row.get(0),
        )
        .optional()
}

/// Creates `name` if missing, and rebuilds it only if an existing index of that
/// name doesn't have the expected definition (never on every start).
fn ensure_index(
    connection: &Connection,
    name: &str,
    table_and_columns: &str,
) -> rusqlite::Result<()> {
    let expected = table_and_columns.replace(' ', "");
    if let Some(existing) = sqlite_master_sql(connection, "index", name)? {
        if existing.replace(' ', "").contains(&expected) {
            return Ok(());
        }
        connection.execute(&format!("DROP INDEX {name}"), [])?;
    }
    connection.execute(&format!("CREATE INDEX {name} ON {table_and_columns}"), [])?;
    Ok(())
}

/// Idempotent and cheap once the database is up to date: the one-off data
/// migrations below only run when the old table layout / missing metadata is
/// actually detected, so this never rewrites large tables on a healthy
/// database.
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
        "#,
    )?;

    // Legacy layout (pre per-server primary key): rebuild once.
    let has_current_layout = sqlite_master_sql(connection, "table", "player_activity")?
        .is_some_and(|sql| sql.contains("PRIMARY KEY (server_name, player_name)"));
    if !has_current_layout {
        connection.execute_batch(
            r#"
            DROP TABLE IF EXISTS player_activity_by_server;
            CREATE TABLE player_activity_by_server (
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
            "#,
        )?;
    }

    let servers_table_existed =
        sqlite_master_sql(connection, "table", "player_activity_servers")?.is_some();
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS player_activity_servers (
            server_uuid TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            last_seen_at TEXT
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

        CREATE TABLE IF NOT EXISTS player_count_samples (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_name TEXT NOT NULL,
            specialization TEXT NOT NULL,
            sampled_at TEXT NOT NULL,
            online_count INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS player_identities (
            uuid TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            updated_at TEXT
        );

        CREATE TABLE IF NOT EXISTS player_server_identities (
            server_name TEXT NOT NULL,
            uuid TEXT NOT NULL,
            name TEXT NOT NULL,
            updated_at TEXT,
            PRIMARY KEY (server_name, uuid)
        );
        "#,
    )?;
    if !servers_table_existed {
        connection.execute(
            "INSERT OR IGNORE INTO player_activity_servers
                (server_uuid, display_name, specialization, last_seen_at)
             SELECT server_name, server_name, MAX(specialization), MAX(last_joined_at)
               FROM player_activity
              GROUP BY server_name",
            [],
        )?;
    }

    ensure_index(
        connection,
        "idx_player_activity_server_lookup",
        "player_activity (server_name, player_name)",
    )?;
    ensure_index(
        connection,
        "idx_player_activity_sessions_lookup",
        "player_activity_sessions (server_name, player_name, joined_at)",
    )?;
    // Per-server time-range scans (recent sessions, timeframe windows).
    ensure_index(
        connection,
        "idx_player_activity_sessions_server_time",
        "player_activity_sessions (server_name, joined_at)",
    )?;
    // Per-game, per-player lookups for the network-wide player list.
    ensure_index(
        connection,
        "idx_player_activity_sessions_game_player",
        "player_activity_sessions (specialization, player_name, server_name)",
    )?;
    ensure_index(
        connection,
        "idx_player_count_samples_lookup",
        "player_count_samples (server_name, sampled_at)",
    )?;
    Ok(())
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
            timeframe_cache: SwrCache::default(),
        };

        assert!(tracker.player_joined("PlayerOne"));
        assert!(tracker.player_left("PlayerOne"));

        let reloaded_store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-one", "Server One", "Minecraft")?;
        let reloaded_tracker = PlayerActivityTracker {
            players: reloaded_store.load_players()?,
            store: Some(reloaded_store),
        last_periodic_sample_at: None,
            timeframe_cache: SwrCache::default(),
        };

        assert_eq!(reloaded_tracker.known_player_count(), 1);
        assert_eq!(
            reloaded_tracker
                .recent_sessions(10)
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        // Timeframe stats are computed in the background and served from a
        // cache, so the first calls may be empty.
        let mut timeframe_stats = reloaded_tracker.timeframe_stats();
        for _ in 0..100 {
            if timeframe_stats.as_array().is_some_and(|stats| !stats.is_empty()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            timeframe_stats = reloaded_tracker.timeframe_stats();
        }
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
    fn uuid_rename_migrates_history_on_one_server_only() -> rusqlite::Result<()> {
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let open = |server: &str| -> rusqlite::Result<PlayerActivityTracker> {
            let store = PlayerActivityStore::open_at(db_path.clone(), server, server, "Hytale")?;
            Ok(PlayerActivityTracker {
                players: store.load_players()?,
                store: Some(store),
                last_periodic_sample_at: None,
                timeframe_cache: SwrCache::default(),
            })
        };

        let mut one = open("server-one")?;
        let mut two = open("server-two")?;
        // Same display name on a different server, which must stay untouched.
        two.player_joined("OldName");
        two.player_left("OldName");

        assert_eq!(one.player_joined_with_uuid("uuid-1", "OldName"), (true, None));
        one.player_left("OldName");

        let (started, renamed_from) = one.player_joined_with_uuid("uuid-1", "NewName");
        assert!(started);
        assert_eq!(renamed_from.as_deref(), Some("OldName"));
        assert_eq!(one.known_player_count(), 1);
        assert_eq!(one.online_names(), vec!["NewName".to_string()]);
        one.player_left("NewName");

        let reloaded = open("server-one")?;
        assert_eq!(reloaded.known_player_count(), 1);
        let summaries = reloaded.summaries().to_string();
        assert!(summaries.contains("NewName") && !summaries.contains("OldName"));
        assert_eq!(
            reloaded.recent_sessions(10).as_array().map(Vec::len),
            Some(2)
        );

        let other = open("server-two")?;
        assert!(other.summaries().to_string().contains("OldName"));

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    /// Manual scale check: `cargo test --release scale_smoke -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn scale_smoke() -> rusqlite::Result<()> {
        use std::time::Instant;
        let db_path = std::env::temp_dir().join(format!("rsc-scale-{}.sqlite3", uuid::Uuid::new_v4()));
        let sessions: i64 = std::env::var("SCALE_SESSIONS").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000_000);
        let store = PlayerActivityStore::open_at(db_path.clone(), "srv-a", "A", "Minecraft")?;
        store.with_connection(|c| {
            c.execute(
                "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i+1 FROM n WHERE i < ?1)
                 INSERT INTO player_activity_sessions
                    (server_name, specialization, player_name, joined_at, left_at, duration_seconds)
                 SELECT 'srv-' || (i % 3), CASE WHEN i % 3 = 2 THEN 'Hytale' ELSE 'Minecraft' END,
                        'player' || (i % 500000),
                        strftime('%Y-%m-%dT%H:%M:%S+00:00', '2026-01-01', '+' || (i % 20000000) || ' seconds'),
                        strftime('%Y-%m-%dT%H:%M:%S+00:00', '2026-01-01', '+' || ((i % 20000000) + 600) || ' seconds'),
                        600
                   FROM n",
                params![sessions],
            )?;
            Ok(())
        })?;
        let t = Instant::now();
        let reopened = PlayerActivityStore::open_at(db_path.clone(), "srv-0", "S0", "Minecraft")?;
        println!("reopen/schema: {:?}", t.elapsed());
        let t = Instant::now();
        let players = reopened.load_players()?;
        println!("load_players: {:?} ({} players)", t.elapsed(), players.len());
        let t = Instant::now();
        let global = PlayerActivityStore::global_player_activity(db_path.clone())?;
        println!("global_player_activity: {:?} ({} listed)", t.elapsed(), global.len());
        let t = Instant::now();
        let totals = PlayerActivityStore::global_server_totals(db_path.clone())?;
        println!("global_server_totals: {:?} ({} rows)", t.elapsed(), totals.len());
        let t = Instant::now();
        let stats = reopened.timeframe_stats()?;
        println!("timeframe_stats: {:?} ({} windows)", t.elapsed(), stats.len());
        let t = Instant::now();
        let recent = reopened.recent_sessions(25)?;
        println!("recent_sessions: {:?} ({})", t.elapsed(), recent.len());
        let mut tracker = PlayerActivityTracker {
            players,
            store: Some(reopened),
            last_periodic_sample_at: None,
            timeframe_cache: SwrCache::default(),
        };
        let t = Instant::now();
        let summaries = tracker.summaries();
        println!("summaries: {:?} ({} entries)", t.elapsed(), summaries.as_array().map_or(0, Vec::len));
        let t = Instant::now();
        tracker.player_joined_with_uuid("uuid-x", "NewPlayer");
        tracker.player_left("NewPlayer");
        println!("join+leave persist: {:?}", t.elapsed());
        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn concurrent_servers_never_hit_database_locked() -> rusqlite::Result<()> {
        let db_path = std::env::temp_dir().join(format!(
            "rsc-concurrent-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        const THREADS: usize = 16;
        const ROUNDS: usize = 60;
        let handles: Vec<_> = (0..THREADS)
            .map(|thread| {
                let db_path = db_path.clone();
                std::thread::spawn(move || -> rusqlite::Result<()> {
                    // Every thread opens its own store at once, like servers
                    // starting together, then joins/leaves repeatedly.
                    let store = PlayerActivityStore::open_at(
                        db_path,
                        &format!("server-{thread}"),
                        "S",
                        "Hytale",
                    )?;
                    for round in 0..ROUNDS {
                        let name = format!("p{thread}-{round}");
                        let joined_at = Utc::now();
                        let mut player = PlayerActivity::default();
                        player.session_count = 1;
                        player.current_session_started_at = Some(joined_at);
                        let session_id = store.persist_join(&name, &player, joined_at)?;
                        player.active_session_id = Some(session_id);
                        store.persist_leave(&name, &mut player, joined_at, Utc::now())?;
                        store.sync_server_identity(&format!("uuid-{thread}-{round}"), &name)?;
                    }
                    Ok(())
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("thread panicked")?;
        }

        let store = PlayerActivityStore::open_at(db_path.clone(), "check", "C", "Hytale")?;
        let sessions: i64 = store.with_connection(|connection| {
            connection.query_row("SELECT COUNT(*) FROM player_activity_sessions", [], |row| {
                row.get(0)
            })
        })?;
        assert_eq!(sessions as usize, THREADS * ROUNDS);
        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn same_name_in_different_games_stays_separate() -> rusqlite::Result<()> {
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let open = |server: &str, game: &str| -> rusqlite::Result<PlayerActivityTracker> {
            let store = PlayerActivityStore::open_at(db_path.clone(), server, server, game)?;
            Ok(PlayerActivityTracker {
                players: store.load_players()?,
                store: Some(store),
                last_periodic_sample_at: None,
                timeframe_cache: SwrCache::default(),
            })
        };
        let mut minecraft = open("mc", "Minecraft")?;
        let mut hytale = open("ht", "Hytale")?;
        minecraft.player_joined("Sturdy");
        minecraft.player_left("Sturdy");
        hytale.player_joined("Sturdy");
        hytale.player_left("Sturdy");

        let global = PlayerActivityStore::global_player_activity(db_path.clone())?;
        assert_eq!(global.len(), 2, "one entry per game, not merged by name");
        let games: BTreeSet<&str> = global
            .iter()
            .filter_map(|player| player.get("specialization").and_then(Value::as_str))
            .collect();
        assert_eq!(games, BTreeSet::from(["Hytale", "Minecraft"]));

        // A Minecraft UUID rename must not rewrite the Hytale player's rows.
        PlayerActivityStore::sync_player_identity(db_path.clone(), "Minecraft", "mc-uuid", "Sturdy")?;
        let renamed = PlayerActivityStore::sync_player_identity(
            db_path.clone(),
            "Minecraft",
            "mc-uuid",
            "SturdyNew",
        )?;
        assert_eq!(renamed.as_deref(), Some("Sturdy"));
        let names: BTreeSet<(String, String)> = PlayerActivityStore::global_player_activity(db_path.clone())?
            .iter()
            .map(|player| {
                (
                    player["specialization"].as_str().unwrap_or_default().to_string(),
                    player["name"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        assert!(names.contains(&("Hytale".to_string(), "Sturdy".to_string())));
        assert!(names.contains(&("Minecraft".to_string(), "SturdyNew".to_string())));

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
            timeframe_cache: SwrCache::default(),
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
            timeframe_cache: SwrCache::default(),
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
            timeframe_cache: SwrCache::default(),
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
            timeframe_cache: SwrCache::default(),
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

    #[test]
    fn load_players_includes_players_missing_from_aggregate_table() -> rusqlite::Result<()> {
        // Same regression as `global_player_activity_includes_players_missing_
        // from_aggregate_table`, but for the in-memory tracker's own view of a
        // single server (`load_players`, which seeds `summaries()` for the
        // per-server "Specialization Stats > User Activity" list). This used to
        // read from the `player_activity` aggregate table too, so it could
        // undercount relative to the top-level, sessions-backed "User Activity"
        // card even for the exact same server.
        let db_path = std::env::temp_dir().join(format!(
            "rsc-player-activity-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let store =
            PlayerActivityStore::open_at(db_path.clone(), "uuid-one", "Server One", "Minecraft")?;
        let joined_at = Utc::now() - ChronoDuration::days(1);
        let left_at = joined_at + ChronoDuration::hours(1);

        store.with_connection(|connection| {
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

        let players = store.load_players()?;
        assert!(players.contains_key("SessionOnlyName"));
        assert_eq!(
            players["SessionOnlyName"].total_seconds,
            elapsed_seconds(joined_at, left_at)
        );

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }
}
