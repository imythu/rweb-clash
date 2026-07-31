use crate::error::AppError;
use crate::paths::{
    ensure_private_directory, restrict_sensitive_file_permissions, sqlite_companion_path, AppPaths,
};
use crate::types::{
    DownloadRoute, FilterRule, FilterRuleInput, GroupFilterInput, LogEntryResponse,
    ManualNodeResponse, ProxyGroupResponse, ProxyNodeResponse, RuleResponse, RuleSetResponse,
    SubscriptionMemberGroup, SubscriptionMemberNode, SubscriptionMemberSection,
    SubscriptionMembersResponse, SubscriptionResponse, SystemConfig, TrafficQuota, BUILTIN_DIRECT,
    BUILTIN_GLOBAL, BUILTIN_PROXY, BUILTIN_REJECT, SUB_DELIMITER,
};
use crate::util::{bool_to_i64, display_log_time, i64_to_bool, new_id, normalize_status, now_iso};
use serde_json::{Map, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::info;

#[derive(Debug, Clone)]
pub struct Storage {
    pool: SqlitePool,
    topology_mutation: Arc<Mutex<()>>,
}

#[derive(Debug, Clone)]
pub struct ProxyItemRecord {
    pub name: String,
    pub kind: String,
    pub subscription_id: Option<String>,
    pub display_name: String,
    pub source: String,
    pub builtin: bool,
    pub source_name: Option<String>,
    pub protocol: Option<String>,
    pub country: Option<String>,
    pub group_type: Option<String>,
    pub raw_json: Option<String>,
    pub content_hash: Option<String>,
    pub latency_ms: Option<i64>,
    pub alive: bool,
    pub filtered_out: bool,
    pub filter_reason: Option<String>,
    pub delay_ms: Option<i64>,
    pub tolerance_ms: Option<i64>,
    pub url: Option<String>,
    pub interval_seconds: Option<i64>,
    pub strategy_json: String,
    pub position: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct SubscriptionSyncCommit {
    pub subscription_name: String,
    pub node_count: i64,
    pub upload_bytes: Option<u64>,
    pub download_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub expire_at: Option<String>,
    pub source_format: String,
    pub raw_content_hash: String,
}

#[derive(Debug, Clone)]
pub struct RuleSetRecord {
    pub id: String,
    pub name: String,
    pub url: String,
    pub behavior: Option<String>,
    pub format: String,
    pub local_path: Option<String>,
    pub download_route: DownloadRoute,
}

#[derive(Debug, Clone)]
pub struct RuleSetRefreshState {
    pub ready: bool,
    pub local_path: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub rule_count: i64,
    pub content_hash: Option<String>,
    pub format: String,
    pub last_update_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct BuiltinRuleSet {
    id: &'static str,
    name: &'static str,
    behavior: &'static str,
    url: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct BuiltinRule {
    id: &'static str,
    rule_type: &'static str,
    value: &'static str,
    policy: &'static str,
}

const BUILTIN_RULE_SET_SEED_MARKER: &str = "builtin_rule_sets_v2";
const BUILTIN_RULE_SEED_MARKER: &str = "builtin_rules_v2";
const BUILTIN_PROXY_GROUP_NAME: &str = BUILTIN_PROXY;
const BUILTIN_RULE_SET_INTERVAL_SECONDS: i64 = 86_400;
const BUILTIN_RULE_SET_FORMAT: &str = "yaml";
const BUILTIN_RULE_SETS: &[BuiltinRuleSet] = &[
    BuiltinRuleSet {
        id: "rs_builtin_reject",
        name: "reject",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/reject.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_icloud",
        name: "icloud",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/icloud.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_apple",
        name: "apple",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/apple.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_google",
        name: "google",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/google.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_proxy",
        name: "proxy",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/proxy.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_direct",
        name: "direct",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/direct.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_private",
        name: "private",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/private.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_gfw",
        name: "gfw",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/gfw.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_tld_not_cn",
        name: "tld-not-cn",
        behavior: "domain",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/tld-not-cn.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_telegramcidr",
        name: "telegramcidr",
        behavior: "ipcidr",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/telegramcidr.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_cncidr",
        name: "cncidr",
        behavior: "ipcidr",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/cncidr.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_lancidr",
        name: "lancidr",
        behavior: "ipcidr",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/lancidr.txt",
    },
    BuiltinRuleSet {
        id: "rs_builtin_applications",
        name: "applications",
        behavior: "classical",
        url: "https://cdn.jsdelivr.net/gh/Loyalsoldier/clash-rules@release/applications.txt",
    },
];
const BUILTIN_RULES: &[BuiltinRule] = &[
    BuiltinRule {
        id: "rule_builtin_applications",
        rule_type: "RULE-SET",
        value: "applications",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_clash_dashboard",
        rule_type: "DOMAIN",
        value: "clash.razord.top",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_yacd_dashboard",
        rule_type: "DOMAIN",
        value: "yacd.haishan.me",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_private",
        rule_type: "RULE-SET",
        value: "private",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_reject",
        rule_type: "RULE-SET",
        value: "reject",
        policy: "REJECT",
    },
    BuiltinRule {
        id: "rule_builtin_icloud",
        rule_type: "RULE-SET",
        value: "icloud",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_apple",
        rule_type: "RULE-SET",
        value: "apple",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_google",
        rule_type: "RULE-SET",
        value: "google",
        policy: "PROXY",
    },
    BuiltinRule {
        id: "rule_builtin_proxy",
        rule_type: "RULE-SET",
        value: "proxy",
        policy: "PROXY",
    },
    BuiltinRule {
        id: "rule_builtin_direct",
        rule_type: "RULE-SET",
        value: "direct",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_lancidr",
        rule_type: "RULE-SET",
        value: "lancidr",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_cncidr",
        rule_type: "RULE-SET",
        value: "cncidr",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_telegramcidr",
        rule_type: "RULE-SET",
        value: "telegramcidr",
        policy: "PROXY",
    },
    BuiltinRule {
        id: "rule_builtin_geoip_lan",
        rule_type: "GEOIP",
        value: "LAN",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_geoip_cn",
        rule_type: "GEOIP",
        value: "CN",
        policy: "DIRECT",
    },
    BuiltinRule {
        id: "rule_builtin_match",
        rule_type: "MATCH",
        value: "ANY",
        policy: "PROXY",
    },
];

impl Storage {
    pub async fn connect(paths: &AppPaths) -> Result<Self, AppError> {
        if let Some(parent) = paths.database_file.parent() {
            ensure_private_directory(parent)?;
        }
        #[cfg(unix)]
        prepare_private_database_file(&paths.database_file)?;
        restrict_sqlite_file_permissions(&paths.database_file)?;
        info!(
            database = %AppPaths::display(&paths.database_file),
            "connecting sqlite storage"
        );

        let options = SqliteConnectOptions::new()
            .filename(&paths.database_file)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        let storage = Self {
            pool,
            topology_mutation: Arc::new(Mutex::new(())),
        };
        storage.migrate().await?;
        storage.normalize_routing_match_rules().await?;
        storage.cleanup_pending_subscriptions().await?;
        storage.cleanup_pending_rule_sets().await?;
        storage.ensure_default_settings().await?;
        storage.ensure_builtin_rule_sets().await?;
        storage.ensure_builtin_rules().await?;
        storage.sync_builtin_proxy_group().await?;
        restrict_sqlite_file_permissions(&paths.database_file)?;
        info!("sqlite storage ready");
        Ok(storage)
    }

    pub async fn backup_database(&self, destination: &Path) -> Result<(), AppError> {
        if let Some(parent) = destination.parent() {
            ensure_private_directory(parent)?;
        }
        match tokio::fs::remove_file(destination).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        sqlx::query("VACUUM INTO ?")
            .bind(destination.to_string_lossy().as_ref())
            .execute(&self.pool)
            .await?;
        restrict_sensitive_file_permissions(destination)?;
        Ok(())
    }

    pub async fn restore_database(&self, source: &Path) -> Result<(), AppError> {
        if !source.is_file() {
            return Err(AppError::bad_request(
                "backup_invalid",
                "backup database snapshot is missing",
            ));
        }
        let mut connection = self.pool.acquire().await?;
        sqlx::query("ATTACH DATABASE ? AS backup")
            .bind(source.to_string_lossy().as_ref())
            .execute(&mut *connection)
            .await?;
        let restore = async {
            let integrity = sqlx::query_scalar::<_, String>("PRAGMA backup.quick_check")
                .fetch_one(&mut *connection)
                .await?;
            if integrity != "ok" {
                return Err(AppError::bad_request(
                    "backup_invalid",
                    format!("backup database integrity check failed: {integrity}"),
                ));
            }

            const DELETE_ORDER: &[&str] = &[
                "proxy_group_members",
                "proxy_group_filters",
                "subscription_rules",
                "proxy_items",
                "routing_rules",
                "rule_sets",
                "subscriptions",
                "global_filter_rules",
                "traffic_snapshots",
                "log_entries",
                "app_settings",
            ];
            const INSERT_ORDER: &[&str] = &[
                "app_settings",
                "subscriptions",
                "global_filter_rules",
                "subscription_rules",
                "proxy_items",
                "proxy_group_filters",
                "proxy_group_members",
                "routing_rules",
                "rule_sets",
                "traffic_snapshots",
                "log_entries",
            ];

            for table in INSERT_ORDER {
                let exists = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM backup.sqlite_master WHERE type = 'table' AND name = ?)",
                )
                .bind(table)
                .fetch_one(&mut *connection)
                .await?;
                if !exists {
                    return Err(AppError::bad_request(
                        "backup_incompatible",
                        format!("backup database is missing table {table}"),
                    ));
                }
            }

            sqlx::query("BEGIN IMMEDIATE")
                .execute(&mut *connection)
                .await?;
            let transaction_result: Result<(), AppError> = async {
                for table in DELETE_ORDER {
                    sqlx::query(&format!("DELETE FROM main.{table}"))
                        .execute(&mut *connection)
                        .await?;
                }
                for table in INSERT_ORDER {
                    let main_columns = attached_table_columns(&mut connection, "main", table).await?;
                    let backup_columns =
                        attached_table_columns(&mut connection, "backup", table).await?;
                    let columns = main_columns
                        .into_iter()
                        .filter(|column| backup_columns.contains(column))
                        .collect::<Vec<_>>();
                    if columns.is_empty() {
                        return Err(AppError::bad_request(
                            "backup_incompatible",
                            format!("backup table {table} has no compatible columns"),
                        ));
                    }
                    let columns = columns.join(", ");
                    sqlx::query(&format!(
                        "INSERT INTO main.{table} ({columns}) SELECT {columns} FROM backup.{table}"
                    ))
                    .execute(&mut *connection)
                    .await?;
                }
                Ok(())
            }
            .await;
            match transaction_result {
                Ok(()) => {
                    sqlx::query("COMMIT").execute(&mut *connection).await?;
                    Ok(())
                }
                Err(error) => {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    Err(error)
                }
            }
        }
        .await;
        let detach = sqlx::query("DETACH DATABASE backup")
            .execute(&mut *connection)
            .await;
        restore?;
        detach?;
        drop(connection);

        self.normalize_routing_match_rules().await?;
        self.cleanup_pending_subscriptions().await?;
        self.cleanup_pending_rule_sets().await?;
        self.ensure_default_settings().await?;
        self.ensure_builtin_rule_sets().await?;
        self.ensure_builtin_rules().await?;
        self.sync_builtin_proxy_group().await?;
        Ok(())
    }

    async fn migrate(&self) -> Result<(), AppError> {
        let migrations = [
            r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  applied_at TEXT NOT NULL
)"#,
            r#"
CREATE TABLE IF NOT EXISTS app_settings (
  scope TEXT NOT NULL DEFAULT 'system',
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (scope, key)
)"#,
            r#"
CREATE TABLE IF NOT EXISTS subscriptions (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  url TEXT NOT NULL,
  ready INTEGER NOT NULL DEFAULT 1,
  source_format TEXT NOT NULL DEFAULT 'unknown',
  status TEXT NOT NULL DEFAULT 'online',
  interval_seconds INTEGER NOT NULL DEFAULT 21600,
  inherit_global_rules INTEGER NOT NULL DEFAULT 1,
  upload_bytes INTEGER,
  download_bytes INTEGER,
  total_bytes INTEGER,
  expire_at TEXT,
  node_count INTEGER NOT NULL DEFAULT 0,
  sync_started_at TEXT,
  sync_finished_at TEXT,
  sync_duration_ms INTEGER,
  sync_error_count INTEGER NOT NULL DEFAULT 0,
  next_sync_at TEXT,
  last_update_at TEXT,
  last_error TEXT,
  raw_etag TEXT,
  raw_last_modified TEXT,
  raw_content_hash TEXT,
  download_route TEXT NOT NULL DEFAULT 'auto',
  last_route TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
)"#,
            "CREATE INDEX IF NOT EXISTS idx_subscriptions_next_refresh ON subscriptions(next_sync_at)",
            r#"
CREATE TABLE IF NOT EXISTS global_filter_rules (
  id TEXT PRIMARY KEY,
  position INTEGER NOT NULL,
  action TEXT NOT NULL,
  match_type TEXT NOT NULL,
  pattern TEXT NOT NULL,
  values_json TEXT NOT NULL DEFAULT '[]',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
)"#,
            "CREATE INDEX IF NOT EXISTS idx_global_filter_rules_position ON global_filter_rules(position)",
            r#"
CREATE TABLE IF NOT EXISTS subscription_rules (
  id TEXT PRIMARY KEY,
  subscription_id TEXT NOT NULL,
  position INTEGER NOT NULL,
  action TEXT NOT NULL,
  match_type TEXT NOT NULL,
  pattern TEXT NOT NULL,
  values_json TEXT NOT NULL DEFAULT '[]',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (subscription_id) REFERENCES subscriptions(id) ON DELETE CASCADE
)"#,
            "CREATE INDEX IF NOT EXISTS idx_subscription_rules_sub_position ON subscription_rules(subscription_id, position)",
            r#"
CREATE TABLE IF NOT EXISTS proxy_items (
  name TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('node', 'group')),
  subscription_id TEXT,
  display_name TEXT NOT NULL,
  source TEXT NOT NULL,
  builtin INTEGER NOT NULL DEFAULT 0,
  source_name TEXT,
  protocol TEXT,
  country TEXT,
  group_type TEXT,
  raw_json TEXT,
  content_hash TEXT,
  latency_ms INTEGER,
  last_test_at TEXT,
  last_good_latency_ms INTEGER,
  probe_status TEXT NOT NULL DEFAULT 'unknown',
  probe_failures INTEGER NOT NULL DEFAULT 0,
  next_probe_at TEXT,
  last_success_at TEXT,
  last_probe_error TEXT,
  alive INTEGER NOT NULL DEFAULT 1,
  filtered_out INTEGER NOT NULL DEFAULT 0,
  filter_reason TEXT,
  delay_ms INTEGER,
  tolerance_ms INTEGER,
  url TEXT,
  interval_seconds INTEGER,
  strategy_json TEXT NOT NULL DEFAULT '{}',
  position INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (subscription_id) REFERENCES subscriptions(id) ON DELETE CASCADE
)"#,
            "CREATE INDEX IF NOT EXISTS idx_proxy_items_kind ON proxy_items(kind)",
            "CREATE INDEX IF NOT EXISTS idx_proxy_items_subscription ON proxy_items(subscription_id)",
            "CREATE INDEX IF NOT EXISTS idx_proxy_items_lookup ON proxy_items(kind, filtered_out, country, protocol)",
            "CREATE INDEX IF NOT EXISTS idx_proxy_items_latency ON proxy_items(kind, alive, latency_ms)",
            "CREATE INDEX IF NOT EXISTS idx_proxy_items_position ON proxy_items(kind, position)",
            r#"
CREATE TABLE IF NOT EXISTS proxy_group_filters (
  id TEXT PRIMARY KEY,
  group_name TEXT NOT NULL,
  position INTEGER NOT NULL,
  action TEXT NOT NULL,
  field TEXT NOT NULL,
  operator TEXT NOT NULL,
  value TEXT NOT NULL DEFAULT '',
  values_json TEXT NOT NULL DEFAULT '[]',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (group_name) REFERENCES proxy_items(name) ON DELETE CASCADE
)"#,
            "CREATE INDEX IF NOT EXISTS idx_proxy_group_filters_group_position ON proxy_group_filters(group_name, position)",
            r#"
CREATE TABLE IF NOT EXISTS proxy_group_members (
  group_name TEXT NOT NULL,
  member_name TEXT NOT NULL,
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  PRIMARY KEY (group_name, member_name),
  FOREIGN KEY (group_name) REFERENCES proxy_items(name) ON DELETE CASCADE
)"#,
            "CREATE INDEX IF NOT EXISTS idx_proxy_group_members_name ON proxy_group_members(member_name)",
            "CREATE INDEX IF NOT EXISTS idx_proxy_group_members_position ON proxy_group_members(group_name, position)",
            r#"
CREATE TABLE IF NOT EXISTS routing_rules (
  id TEXT PRIMARY KEY,
  position INTEGER NOT NULL,
  rule_type TEXT NOT NULL,
  value TEXT NOT NULL,
  policy TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'user',
  enabled INTEGER NOT NULL DEFAULT 1,
  desc TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
)"#,
            "CREATE INDEX IF NOT EXISTS idx_routing_rules_active_position ON routing_rules(enabled, position)",
            "CREATE INDEX IF NOT EXISTS idx_routing_rules_search ON routing_rules(rule_type, value, policy)",
            r#"
CREATE TABLE IF NOT EXISTS rule_sets (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  url TEXT NOT NULL,
  ready INTEGER NOT NULL DEFAULT 1,
  behavior TEXT,
  format TEXT NOT NULL DEFAULT 'text',
  local_path TEXT,
  file_size_bytes INTEGER,
  interval_seconds INTEGER NOT NULL DEFAULT 86400,
  rule_count INTEGER NOT NULL DEFAULT 0,
  last_update_at TEXT,
  last_error TEXT,
  raw_etag TEXT,
  raw_last_modified TEXT,
  content_hash TEXT,
  staged_local_path TEXT,
  staged_file_size_bytes INTEGER,
  staged_rule_count INTEGER,
  staged_content_hash TEXT,
  staged_format TEXT,
  staged_update_at TEXT,
  staged_last_error TEXT,
  download_route TEXT NOT NULL DEFAULT 'auto',
  last_route TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
)"#,
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_rule_sets_name ON rule_sets(name)",
            "CREATE INDEX IF NOT EXISTS idx_rule_sets_refresh ON rule_sets(interval_seconds, last_update_at)",
            r#"
CREATE TABLE IF NOT EXISTS traffic_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  bucket_start INTEGER NOT NULL,
  bucket_seconds INTEGER NOT NULL,
  avg_upload_bps INTEGER NOT NULL,
  avg_download_bps INTEGER NOT NULL,
  max_upload_bps INTEGER NOT NULL,
  max_download_bps INTEGER NOT NULL,
  avg_active_connections INTEGER NOT NULL DEFAULT 0,
  max_active_connections INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
)"#,
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_traffic_snapshots_bucket ON traffic_snapshots(bucket_start, bucket_seconds)",
            r#"
CREATE TABLE IF NOT EXISTS log_entries (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  time TEXT NOT NULL,
  level TEXT NOT NULL,
  payload TEXT NOT NULL,
  parsed_host TEXT,
  created_at TEXT NOT NULL
)"#,
            "CREATE INDEX IF NOT EXISTS idx_log_entries_time ON log_entries(time)",
            "CREATE INDEX IF NOT EXISTS idx_log_entries_level_time ON log_entries(level, time)",
            "CREATE INDEX IF NOT EXISTS idx_log_entries_parsed_host ON log_entries(parsed_host)",
        ];

        for statement in migrations {
            sqlx::query(statement).execute(&self.pool).await?;
        }
        self.ensure_proxy_group_filter_values_column().await?;
        self.ensure_filter_rule_values_columns().await?;
        self.ensure_subscription_source_format_column().await?;
        self.ensure_text_column("subscriptions", "download_route", "'auto'")
            .await?;
        self.ensure_nullable_column("subscriptions", "last_route", "TEXT")
            .await?;
        self.ensure_integer_column("subscriptions", "ready", "1")
            .await?;
        self.ensure_proxy_item_builtin_column().await?;
        self.ensure_nullable_column("proxy_items", "last_good_latency_ms", "INTEGER")
            .await?;
        self.ensure_text_column("proxy_items", "probe_status", "'unknown'")
            .await?;
        self.ensure_integer_column("proxy_items", "probe_failures", "0")
            .await?;
        self.ensure_nullable_column("proxy_items", "next_probe_at", "TEXT")
            .await?;
        self.ensure_nullable_column("proxy_items", "last_success_at", "TEXT")
            .await?;
        self.ensure_nullable_column("proxy_items", "last_probe_error", "TEXT")
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_proxy_items_probe_due ON proxy_items(kind, enabled, filtered_out, next_probe_at)",
        )
        .execute(&self.pool)
        .await?;
        self.ensure_integer_column("rule_sets", "ready", "1")
            .await?;
        self.ensure_rule_set_staging_columns().await?;
        self.ensure_text_column("rule_sets", "download_route", "'auto'")
            .await?;
        self.ensure_nullable_column("rule_sets", "last_route", "TEXT")
            .await?;
        self.normalize_rule_set_local_paths().await?;
        self.normalize_builtin_rule_set_formats().await?;
        sqlx::query(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES(1, 'initial', ?)",
        )
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES(2, 'source-routes', ?)",
        )
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        info!("database migrations applied");
        Ok(())
    }

    async fn cleanup_pending_rule_sets(&self) -> Result<(), AppError> {
        sqlx::query("UPDATE rule_sets SET ready = 1 WHERE ready = 3")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            r#"
UPDATE rule_sets
SET staged_local_path = NULL,
    staged_file_size_bytes = NULL,
    staged_rule_count = NULL,
    staged_content_hash = NULL,
    staged_format = NULL,
    staged_update_at = NULL,
    staged_last_error = NULL
WHERE ready = 1
"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM rule_sets WHERE ready <> 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn ensure_rule_set_staging_columns(&self) -> Result<(), AppError> {
        for (column, sql_type) in [
            ("staged_local_path", "TEXT"),
            ("staged_file_size_bytes", "INTEGER"),
            ("staged_rule_count", "INTEGER"),
            ("staged_content_hash", "TEXT"),
            ("staged_format", "TEXT"),
            ("staged_update_at", "TEXT"),
            ("staged_last_error", "TEXT"),
        ] {
            self.ensure_nullable_column("rule_sets", column, sql_type)
                .await?;
        }
        Ok(())
    }

    async fn cleanup_pending_subscriptions(&self) -> Result<(), AppError> {
        sqlx::query("UPDATE subscriptions SET ready = 1 WHERE ready = 2")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM subscriptions WHERE ready = 0")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn normalize_routing_match_rules(&self) -> Result<(), AppError> {
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            r#"
WITH ranked AS (
  SELECT id,
         ROW_NUMBER() OVER (
           PARTITION BY source
           ORDER BY position, created_at, id
         ) AS match_rank
  FROM routing_rules
  WHERE rule_type = 'MATCH' AND enabled = 1
)
UPDATE routing_rules
SET enabled = 0, updated_at = ?
WHERE id IN (SELECT id FROM ranked WHERE match_rank > 1)
"#,
        )
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
WITH ordered AS (
  SELECT id,
         ROW_NUMBER() OVER (
           PARTITION BY source
           ORDER BY CASE WHEN rule_type = 'MATCH' THEN 1 ELSE 0 END,
                    position, created_at, id
         ) * 1024 AS normalized_position
  FROM routing_rules
)
UPDATE routing_rules
SET position = (SELECT normalized_position FROM ordered WHERE ordered.id = routing_rules.id),
    updated_at = ?
WHERE position <> (SELECT normalized_position FROM ordered WHERE ordered.id = routing_rules.id)
"#,
        )
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_routing_rules_one_enabled_match_per_source
ON routing_rules(source)
WHERE rule_type = 'MATCH' AND enabled = 1
"#,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn normalize_rule_set_local_paths(&self) -> Result<(), AppError> {
        sqlx::query(
            r#"
UPDATE rule_sets
SET local_path = replace(local_path, 'data/rule-sets/', 'data/profiles/rule-sets/')
WHERE local_path LIKE 'data/rule-sets/%'
"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn normalize_builtin_rule_set_formats(&self) -> Result<(), AppError> {
        for rule_set in BUILTIN_RULE_SETS {
            sqlx::query("UPDATE rule_sets SET format = ? WHERE id = ?")
                .bind(BUILTIN_RULE_SET_FORMAT)
                .bind(rule_set.id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn ensure_filter_rule_values_columns(&self) -> Result<(), AppError> {
        self.ensure_text_column("global_filter_rules", "values_json", "'[]'")
            .await?;
        self.ensure_text_column("subscription_rules", "values_json", "'[]'")
            .await
    }

    async fn ensure_proxy_group_filter_values_column(&self) -> Result<(), AppError> {
        self.ensure_text_column("proxy_group_filters", "values_json", "'[]'")
            .await
    }

    async fn ensure_subscription_source_format_column(&self) -> Result<(), AppError> {
        self.ensure_text_column("subscriptions", "source_format", "'unknown'")
            .await
    }

    async fn ensure_proxy_item_builtin_column(&self) -> Result<(), AppError> {
        self.ensure_integer_column("proxy_items", "builtin", "0")
            .await?;
        sqlx::query(
            "UPDATE proxy_items SET builtin = 1 WHERE kind = 'group' AND source = 'system'",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_text_column(
        &self,
        table: &str,
        column: &str,
        default_value: &str,
    ) -> Result<(), AppError> {
        let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(&self.pool)
            .await?;
        let has_column = rows.iter().any(|row| {
            row.try_get::<String, _>("name")
                .map(|name| name == column)
                .unwrap_or(false)
        });
        if !has_column {
            sqlx::query(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} TEXT NOT NULL DEFAULT {default_value}",
            ))
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn ensure_integer_column(
        &self,
        table: &str,
        column: &str,
        default_value: &str,
    ) -> Result<(), AppError> {
        let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(&self.pool)
            .await?;
        let has_column = rows.iter().any(|row| {
            row.try_get::<String, _>("name")
                .map(|name| name == column)
                .unwrap_or(false)
        });
        if !has_column {
            sqlx::query(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} INTEGER NOT NULL DEFAULT {default_value}",
            ))
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn ensure_nullable_column(
        &self,
        table: &str,
        column: &str,
        sql_type: &str,
    ) -> Result<(), AppError> {
        let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(&self.pool)
            .await?;
        let has_column = rows.iter().any(|row| {
            row.try_get::<String, _>("name")
                .map(|name| name == column)
                .unwrap_or(false)
        });
        if !has_column {
            sqlx::query(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {sql_type}",
            ))
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn ensure_default_settings(&self) -> Result<(), AppError> {
        let rows = sqlx::query("SELECT COUNT(*) AS count FROM app_settings WHERE scope = 'system'")
            .fetch_one(&self.pool)
            .await?;
        let count: i64 = rows.try_get("count")?;
        if count == 0 {
            let config = SystemConfig {
                secret: new_id("controller_secret"),
                ..SystemConfig::default()
            };
            self.save_config(&config).await?;
        } else {
            let mut config = self.load_config().await?;
            if matches!(
                config.secret.as_str(),
                "r-clash-secret" | "r-clash-secret-2024"
            ) {
                config.secret = new_id("controller_secret");
                self.save_config(&config).await?;
            }
        }
        Ok(())
    }

    async fn ensure_builtin_rule_sets(&self) -> Result<(), AppError> {
        self.seed_builtin_rule_sets().await?;
        self.mark_builtin_rule_sets_seeded().await
    }

    async fn seed_builtin_rule_sets(&self) -> Result<(), AppError> {
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        for rule_set in BUILTIN_RULE_SETS {
            sqlx::query(
                r#"
INSERT OR IGNORE INTO rule_sets(
  id, name, url, behavior, format, interval_seconds, created_at, updated_at
) VALUES(?, ?, ?, ?, ?, ?, ?, ?)
"#,
            )
            .bind(rule_set.id)
            .bind(rule_set.name)
            .bind(rule_set.url)
            .bind(rule_set.behavior)
            .bind(BUILTIN_RULE_SET_FORMAT)
            .bind(BUILTIN_RULE_SET_INTERVAL_SECONDS)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        info!(count = BUILTIN_RULE_SETS.len(), "seeded builtin rule sets");
        Ok(())
    }

    async fn mark_builtin_rule_sets_seeded(&self) -> Result<(), AppError> {
        let now = now_iso();
        sqlx::query(
            r#"
INSERT INTO app_settings(scope, key, value, created_at, updated_at)
VALUES('seed', ?, 'true', ?, ?)
ON CONFLICT(scope, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
"#,
        )
        .bind(BUILTIN_RULE_SET_SEED_MARKER)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_builtin_rules(&self) -> Result<(), AppError> {
        self.seed_builtin_rules().await?;
        self.mark_builtin_rules_seeded().await
    }

    async fn seed_builtin_rules(&self) -> Result<(), AppError> {
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        for (index, rule) in BUILTIN_RULES.iter().enumerate() {
            sqlx::query(
                r#"
INSERT OR IGNORE INTO routing_rules(
  id, position, rule_type, value, policy, source, enabled, desc, created_at, updated_at
) VALUES(?, ?, ?, ?, ?, 'system', 1, 'Builtin default rule', ?, ?)
"#,
            )
            .bind(rule.id)
            .bind(((index + 1) as i64) * 1024)
            .bind(rule.rule_type)
            .bind(rule.value)
            .bind(rule.policy)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        info!(count = BUILTIN_RULES.len(), "seeded builtin routing rules");
        Ok(())
    }

    async fn mark_builtin_rules_seeded(&self) -> Result<(), AppError> {
        let now = now_iso();
        sqlx::query(
            r#"
INSERT INTO app_settings(scope, key, value, created_at, updated_at)
VALUES('seed', ?, 'true', ?, ?)
ON CONFLICT(scope, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
"#,
        )
        .bind(BUILTIN_RULE_SEED_MARKER)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_config(&self) -> Result<SystemConfig, AppError> {
        let rows = sqlx::query("SELECT key, value FROM app_settings WHERE scope = 'system'")
            .fetch_all(&self.pool)
            .await?;
        let mut map = serde_json::Map::new();
        for row in rows {
            let key: String = row.try_get("key")?;
            let raw: String = row.try_get("value")?;
            let value = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
            map.insert(key, value);
        }
        let merged = merge_default_config(map);
        Ok(serde_json::from_value(merged).unwrap_or_default())
    }

    pub async fn save_config(&self, config: &SystemConfig) -> Result<(), AppError> {
        let value = serde_json::to_value(config).map_err(AppError::from)?;
        let Some(object) = value.as_object() else {
            return Err(AppError::internal(
                "system config did not serialize as object",
            ));
        };
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        for (key, value) in object {
            sqlx::query(
                r#"
INSERT INTO app_settings(scope, key, value, created_at, updated_at)
VALUES('system', ?, ?, ?, ?)
ON CONFLICT(scope, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
"#,
            )
            .bind(key)
            .bind(value.to_string())
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_global_filter_rules(&self) -> Result<Vec<FilterRule>, AppError> {
        let rows = sqlx::query(
            "SELECT id, action, match_type, pattern, values_json, enabled FROM global_filter_rules ORDER BY position",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(filter_rule_from_row)
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn replace_global_filter_rules(
        &self,
        rules: &[FilterRuleInput],
    ) -> Result<Vec<FilterRule>, AppError> {
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM global_filter_rules")
            .execute(&mut *tx)
            .await?;
        for (index, rule) in rules.iter().enumerate() {
            let values = if matches!(rule.match_type.trim(), "in" | "equals") {
                rule.effective_values()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let values_json = serde_json::to_string(&values)?;
            sqlx::query(
                r#"
INSERT INTO global_filter_rules(id, position, action, match_type, pattern, values_json, enabled, created_at, updated_at)
VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)
"#,
            )
            .bind(rule.id.clone().unwrap_or_else(|| new_id("gfr")))
            .bind(((index + 1) as i64) * 1024)
            .bind(rule.action.trim())
            .bind(rule.match_type.trim())
            .bind(if rule.match_type.trim() == "in" {
                ""
            } else {
                rule.pattern.trim()
            })
            .bind(values_json)
            .bind(bool_to_i64(rule.enabled.unwrap_or(true)))
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.list_global_filter_rules().await
    }

    pub async fn subscription_ids_inheriting_global_rules(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            "SELECT id FROM subscriptions WHERE inherit_global_rules = 1 AND ready = 1 ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("id").map_err(AppError::from))
            .collect()
    }

    #[cfg(test)]
    pub async fn create_subscription(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        inherit_global: bool,
        rules: &[FilterRuleInput],
    ) -> Result<(), AppError> {
        self.create_subscription_with_ready(
            id,
            name,
            url,
            interval_seconds,
            inherit_global,
            rules,
            DownloadRoute::Auto,
            true,
        )
        .await
    }

    #[cfg(test)]
    pub async fn create_pending_subscription(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        inherit_global: bool,
        rules: &[FilterRuleInput],
    ) -> Result<(), AppError> {
        self.create_subscription_with_ready(
            id,
            name,
            url,
            interval_seconds,
            inherit_global,
            rules,
            DownloadRoute::Auto,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_pending_subscription_with_route(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        inherit_global: bool,
        rules: &[FilterRuleInput],
        download_route: DownloadRoute,
    ) -> Result<(), AppError> {
        self.create_subscription_with_ready(
            id,
            name,
            url,
            interval_seconds,
            inherit_global,
            rules,
            download_route,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_subscription_with_ready(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        inherit_global: bool,
        rules: &[FilterRuleInput],
        download_route: DownloadRoute,
        ready: bool,
    ) -> Result<(), AppError> {
        let interval_seconds = sqlite_i64(interval_seconds, "subscription interval_seconds")?;
        let now = now_iso();
        let next_sync_at = now.clone();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            r#"
INSERT INTO subscriptions(
  id, name, url, ready, status, interval_seconds, inherit_global_rules, node_count,
  next_sync_at, download_route, created_at, updated_at
) VALUES(?, ?, ?, ?, 'syncing', ?, ?, 0, ?, ?, ?, ?)
"#,
        )
        .bind(id)
        .bind(name)
        .bind(url)
        .bind(bool_to_i64(ready))
        .bind(interval_seconds)
        .bind(bool_to_i64(inherit_global))
        .bind(next_sync_at)
        .bind(download_route.as_str())
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        insert_subscription_rules(&mut tx, id, rules, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn activate_subscription(&self, id: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = sqlx::query(
            "UPDATE subscriptions SET ready = 1, updated_at = ? WHERE id = ? AND ready = 0",
        )
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "subscription_not_found",
                format!("pending subscription {id} not found"),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    #[cfg(test)]
    pub async fn update_subscription(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        inherit_global: bool,
        rules: &[FilterRuleInput],
    ) -> Result<(), AppError> {
        self.update_subscription_with_route(
            id,
            name,
            url,
            interval_seconds,
            inherit_global,
            rules,
            DownloadRoute::Auto,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_subscription_with_route(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        inherit_global: bool,
        rules: &[FilterRuleInput],
        download_route: DownloadRoute,
    ) -> Result<(), AppError> {
        let interval_seconds = sqlite_i64(interval_seconds, "subscription interval_seconds")?;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = sqlx::query(
            r#"
UPDATE subscriptions
SET name = ?, url = ?, interval_seconds = ?, inherit_global_rules = ?, download_route = ?, updated_at = ?
WHERE id = ?
"#,
        )
        .bind(name)
        .bind(url)
        .bind(interval_seconds)
        .bind(bool_to_i64(inherit_global))
        .bind(download_route.as_str())
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "subscription_not_found",
                format!("subscription {id} not found"),
            ));
        }
        sqlx::query("DELETE FROM subscription_rules WHERE subscription_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        insert_subscription_rules(&mut tx, id, rules, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_subscription(&self, id: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let available_before = available_policy_targets_in_transaction(&mut tx).await?;
        let referenced_by = sqlx::query_scalar::<_, String>(
            r#"
SELECT rules.id
FROM routing_rules rules
JOIN proxy_items items ON items.name = rules.policy
WHERE items.subscription_id = ?
LIMIT 1
"#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(rule_id) = referenced_by {
            return Err(AppError::conflict(
                "subscription_referenced",
                format!("subscription assets are referenced by routing rule {rule_id}"),
            ));
        }
        let result = sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "subscription_not_found",
                format!("subscription {id} not found"),
            ));
        }
        validate_no_referenced_policy_became_unavailable(&mut tx, &available_before).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn stage_subscription_deletion(&self, id: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let ready = sqlx::query_scalar::<_, i64>("SELECT ready FROM subscriptions WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    "subscription_not_found",
                    format!("subscription {id} not found"),
                )
            })?;
        if ready != 1 {
            return Err(AppError::conflict(
                "subscription_busy",
                format!("subscription {id} is not active"),
            ));
        }
        let available_before = available_policy_targets_in_transaction(&mut tx).await?;
        let referenced_by = sqlx::query_scalar::<_, String>(
            r#"
SELECT rules.id
FROM routing_rules rules
JOIN proxy_items items ON items.name = rules.policy
WHERE items.subscription_id = ?
LIMIT 1
"#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(rule_id) = referenced_by {
            return Err(AppError::conflict(
                "subscription_referenced",
                format!("subscription assets are referenced by routing rule {rule_id}"),
            ));
        }
        sqlx::query("UPDATE subscriptions SET ready = 2, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        validate_no_referenced_policy_became_unavailable(&mut tx, &available_before).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn restore_subscription_deletion(&self, id: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = sqlx::query(
            "UPDATE subscriptions SET ready = 1, updated_at = ? WHERE id = ? AND ready = 2",
        )
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "subscription_not_found",
                format!("staged subscription deletion {id} not found"),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<SubscriptionResponse>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id, name, url, source_format, status, interval_seconds, inherit_global_rules,
       upload_bytes, download_bytes, total_bytes, expire_at, node_count,
       last_update_at, last_error, download_route, last_route
FROM subscriptions
WHERE ready = 1
ORDER BY created_at DESC
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id")?;
            let upload: Option<i64> = row.try_get("upload_bytes")?;
            let download: Option<i64> = row.try_get("download_bytes")?;
            let total: Option<i64> = row.try_get("total_bytes")?;
            let interval_seconds = row.try_get::<i64, _>("interval_seconds")?.max(0);
            output.push(SubscriptionResponse {
                id: id.clone(),
                name: row.try_get("name")?,
                url: row.try_get("url")?,
                format: row.try_get("source_format")?,
                nodes: row.try_get("node_count")?,
                status: normalize_status(&row.try_get::<String, _>("status")?),
                traffic: TrafficQuota {
                    used: upload
                        .unwrap_or(0)
                        .max(0)
                        .saturating_add(download.unwrap_or(0).max(0))
                        as u64,
                    total: total.unwrap_or(0).max(0) as u64,
                },
                expiry: row.try_get("expire_at")?,
                interval_seconds,
                interval: interval_seconds / 60,
                inherit_global: i64_to_bool(row.try_get("inherit_global_rules")?),
                rules: self.list_subscription_rules(&id).await?,
                breakdown: self.subscription_breakdown(&id).await?,
                last_update: row.try_get("last_update_at")?,
                last_error: row.try_get("last_error")?,
                download_route: download_route_from_str(
                    &row.try_get::<String, _>("download_route")?,
                ),
                last_route: row.try_get("last_route")?,
            });
        }
        Ok(output)
    }

    pub async fn subscription_members(
        &self,
        id: &str,
    ) -> Result<SubscriptionMembersResponse, AppError> {
        let subscription = sqlx::query("SELECT name FROM subscriptions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    "subscription_not_found",
                    format!("subscription {id} not found"),
                )
            })?;
        let subscription_name: String = subscription.try_get("name")?;

        let node_rows = sqlx::query(
            r#"
SELECT name, display_name, protocol, country, latency_ms, filtered_out, filter_reason
FROM proxy_items
WHERE subscription_id = ? AND kind = 'node' AND enabled = 1
ORDER BY position, display_name
"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let mut all_nodes = Vec::with_capacity(node_rows.len());
        for row in node_rows {
            all_nodes.push(SubscriptionMemberNode {
                name: row.try_get("name")?,
                display_name: row.try_get("display_name")?,
                protocol: row
                    .try_get::<Option<String>, _>("protocol")?
                    .unwrap_or_else(|| "unknown".into()),
                country: row.try_get("country")?,
                latency: row.try_get::<Option<i64>, _>("latency_ms")?.unwrap_or(-1),
                filtered_out: i64_to_bool(row.try_get("filtered_out")?),
                filter_reason: row.try_get("filter_reason")?,
            });
        }

        let group_rows = sqlx::query(
            r#"
SELECT name, display_name, group_type, filtered_out, filter_reason
FROM proxy_items
WHERE subscription_id = ? AND kind = 'group' AND enabled = 1 AND filtered_out = 0
ORDER BY position, display_name
"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let mut groups = Vec::with_capacity(group_rows.len());
        for row in group_rows {
            let name: String = row.try_get("name")?;
            let members = self.group_members(&name).await?;
            groups.push(SubscriptionMemberGroup {
                name,
                display_name: row.try_get("display_name")?,
                group_type: row
                    .try_get::<Option<String>, _>("group_type")?
                    .unwrap_or_else(|| "select".into()),
                member_count: members.len(),
                members,
                filtered_out: i64_to_bool(row.try_get("filtered_out")?),
                filter_reason: row.try_get("filter_reason")?,
            });
        }

        let filtered_nodes = all_nodes
            .iter()
            .filter(|node| !node.filtered_out)
            .cloned()
            .collect();

        Ok(SubscriptionMembersResponse {
            subscription_id: id.to_string(),
            subscription_name,
            filtered: SubscriptionMemberSection {
                nodes: filtered_nodes,
                groups,
            },
            before_filter: SubscriptionMemberSection {
                nodes: all_nodes,
                groups: Vec::new(),
            },
        })
    }

    pub async fn subscription_rules_for_sync(
        &self,
        subscription_id: &str,
    ) -> Result<(bool, Vec<FilterRule>, Vec<FilterRule>), AppError> {
        let row = sqlx::query("SELECT inherit_global_rules FROM subscriptions WHERE id = ?")
            .bind(subscription_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    "subscription_not_found",
                    format!("subscription {subscription_id} not found"),
                )
            })?;
        let inherit_global = i64_to_bool(row.try_get("inherit_global_rules")?);
        let global = if inherit_global {
            self.list_global_filter_rules().await?
        } else {
            Vec::new()
        };
        let local = self.list_subscription_rules(subscription_id).await?;
        Ok((inherit_global, global, local))
    }

    pub async fn get_subscription_url(&self, id: &str) -> Result<(String, String), AppError> {
        let row = sqlx::query("SELECT name, url FROM subscriptions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    "subscription_not_found",
                    format!("subscription {id} not found"),
                )
            })?;
        Ok((row.try_get("name")?, row.try_get("url")?))
    }

    pub async fn get_subscription_download_route(
        &self,
        id: &str,
    ) -> Result<DownloadRoute, AppError> {
        let route = sqlx::query_scalar::<_, String>(
            "SELECT download_route FROM subscriptions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            AppError::not_found(
                "subscription_not_found",
                format!("subscription {id} not found"),
            )
        })?;
        Ok(download_route_from_str(&route))
    }

    pub async fn set_subscription_last_route(&self, id: &str, route: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE subscriptions SET last_route = ?, updated_at = ? WHERE id = ?")
            .bind(route)
            .bind(now_iso())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn due_subscription_ids(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id
FROM subscriptions
WHERE ready = 1
  AND interval_seconds > 0
  AND status != 'syncing'
  AND COALESCE(
    CASE
      WHEN sync_finished_at IS NULL AND last_update_at IS NULL
        THEN CAST(strftime('%s', next_sync_at) AS INTEGER)
      WHEN CAST(strftime('%s', next_sync_at) AS INTEGER)
        > CAST(strftime('%s', COALESCE(sync_finished_at, last_update_at)) AS INTEGER)
        THEN CAST(strftime('%s', next_sync_at) AS INTEGER)
      ELSE NULL
    END,
    CAST(strftime('%s', COALESCE(sync_finished_at, last_update_at, created_at)) AS INTEGER)
      + MAX(interval_seconds, 21600)
  ) <= CAST(strftime('%s', 'now') AS INTEGER)
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("id").map_err(AppError::from))
            .collect()
    }

    pub async fn startup_subscription_ids(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id
FROM subscriptions
WHERE ready = 1
  AND interval_seconds > 0
  AND (
    status = 'syncing'
    OR COALESCE(
      CASE
        WHEN sync_finished_at IS NULL AND last_update_at IS NULL
          THEN CAST(strftime('%s', next_sync_at) AS INTEGER)
        WHEN CAST(strftime('%s', next_sync_at) AS INTEGER)
          > CAST(strftime('%s', COALESCE(sync_finished_at, last_update_at)) AS INTEGER)
          THEN CAST(strftime('%s', next_sync_at) AS INTEGER)
        ELSE NULL
      END,
      CAST(strftime('%s', COALESCE(sync_finished_at, last_update_at, created_at)) AS INTEGER)
        + MAX(interval_seconds, 21600)
    ) <= CAST(strftime('%s', 'now') AS INTEGER)
  )
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("id").map_err(AppError::from))
            .collect()
    }

    pub async fn mark_subscription_sync_start(&self, id: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE subscriptions SET status = 'syncing', sync_started_at = ?, updated_at = ? WHERE id = ?")
            .bind(now_iso())
            .bind(now_iso())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_subscription_sync_error(
        &self,
        id: &str,
        message: &str,
    ) -> Result<(), AppError> {
        let now = now_iso();
        sqlx::query(
            r#"
UPDATE subscriptions
SET status = 'offline',
    sync_finished_at = ?,
    sync_error_count = sync_error_count + 1,
    next_sync_at = CASE
      WHEN interval_seconds <= 0 THEN NULL
      ELSE datetime(
        'now',
        printf(
          '+%d seconds',
          MIN(
            MAX(interval_seconds, 21600),
            CASE sync_error_count
              WHEN 0 THEN 900
              WHEN 1 THEN 1800
              WHEN 2 THEN 3600
              WHEN 3 THEN 7200
              ELSE 14400
            END
          )
        )
      )
    END,
    last_error = ?,
    updated_at = ?
WHERE id = ?
"#,
        )
        .bind(&now)
        .bind(message)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn replace_subscription_assets(
        &self,
        subscription_id: &str,
        items: &[ProxyItemRecord],
        group_members: &[(String, Vec<String>)],
        commit: SubscriptionSyncCommit,
    ) -> Result<(), AppError> {
        let mut incoming_names = HashSet::with_capacity(items.len());
        let mut incoming_groups = HashSet::new();
        for item in items {
            if item.subscription_id.as_deref() != Some(subscription_id) {
                return Err(AppError::internal(
                    "subscription asset batch contained an item owned by another subscription",
                ));
            }
            if !incoming_names.insert(item.name.as_str()) {
                return Err(AppError::internal(
                    "subscription asset batch contained duplicate runtime names",
                ));
            }
            if item.kind == "group" {
                incoming_groups.insert(item.name.as_str());
            }
        }
        let mut member_groups = HashSet::with_capacity(group_members.len());
        for (group_name, members) in group_members {
            if !incoming_groups.contains(group_name.as_str())
                || !member_groups.insert(group_name.as_str())
            {
                return Err(AppError::internal(
                    "subscription group-member batch targeted an undeclared or duplicate group",
                ));
            }
            if members.iter().any(|member| {
                !incoming_names.contains(member.as_str()) && !is_builtin_policy(member)
            }) {
                return Err(AppError::internal(
                    "subscription group-member batch referenced an undeclared asset",
                ));
            }
        }
        let candidate_member_map = group_members.iter().cloned().collect::<HashMap<_, _>>();
        let candidate_policies =
            crate::runtime::available_policy_targets_from_assets(items, &candidate_member_map)?;

        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if !incoming_names.is_empty() {
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT name, subscription_id FROM proxy_items WHERE name IN (",
            );
            let mut names = query.separated(", ");
            for name in &incoming_names {
                names.push_bind(name);
            }
            names.push_unseparated(")");
            let existing = query.build().fetch_all(&mut *tx).await?;
            for row in existing {
                let name: String = row.try_get("name")?;
                let owner: Option<String> = row.try_get("subscription_id")?;
                if owner.as_deref() != Some(subscription_id) {
                    return Err(AppError::conflict(
                        "subscription_asset_conflict",
                        format!("subscription asset name {name} is already in use"),
                    ));
                }
            }
        }
        let available_before = available_policy_targets_in_transaction(&mut tx).await?;
        let referenced_policies = sqlx::query_scalar::<_, String>(
            r#"
SELECT DISTINCT rules.policy
FROM routing_rules rules
JOIN proxy_items items ON items.name = rules.policy
WHERE items.subscription_id = ?
"#,
        )
        .bind(subscription_id)
        .fetch_all(&mut *tx)
        .await?;
        let migration = migrate_subscription_asset_references(
            &mut tx,
            subscription_id,
            items,
            group_members,
            &now,
        )
        .await?;
        if let Some(policy) = referenced_policies.into_iter().find(|policy| {
            let candidate = migration
                .name_migrations
                .get(policy)
                .map(String::as_str)
                .unwrap_or(policy);
            !candidate_policies.contains(candidate)
        }) {
            return Err(AppError::conflict(
                "subscription_asset_referenced",
                format!("subscription refresh would make referenced policy {policy} unavailable"),
            ));
        }
        sqlx::query("DELETE FROM proxy_items WHERE subscription_id = ?")
            .bind(subscription_id)
            .execute(&mut *tx)
            .await?;
        for item in items {
            if let Some(selected) = migration.group_selections.get(&item.name) {
                let mut item = item.clone();
                item.strategy_json = serde_json::json!({ "now": selected }).to_string();
                upsert_proxy_item_in_transaction(&mut tx, &item, &now).await?;
            } else {
                upsert_proxy_item_in_transaction(&mut tx, item, &now).await?;
            }
        }
        for (group_name, members) in group_members {
            sqlx::query("DELETE FROM proxy_group_members WHERE group_name = ?")
                .bind(group_name)
                .execute(&mut *tx)
                .await?;
            for (index, member) in members.iter().enumerate() {
                sqlx::query(
                    "INSERT OR IGNORE INTO proxy_group_members(group_name, member_name, position, created_at) VALUES(?, ?, ?, ?)",
                )
                .bind(group_name)
                .bind(member)
                .bind(((index + 1) as i64) * 1024)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }
        }
        validate_no_referenced_policy_became_unavailable(&mut tx, &available_before).await?;
        let upload_bytes = optional_sqlite_i64(commit.upload_bytes, "subscription upload_bytes")?;
        let download_bytes =
            optional_sqlite_i64(commit.download_bytes, "subscription download_bytes")?;
        let total_bytes = optional_sqlite_i64(commit.total_bytes, "subscription total_bytes")?;
        let result = sqlx::query(
            r#"
UPDATE subscriptions
SET name = ?,
    status = 'online',
    upload_bytes = ?,
    download_bytes = ?,
    total_bytes = ?,
    expire_at = ?,
    node_count = ?,
    sync_finished_at = ?,
    last_update_at = ?,
    sync_error_count = 0,
    next_sync_at = CASE
      WHEN interval_seconds <= 0 THEN NULL
      ELSE datetime('now', printf('+%d seconds', MAX(interval_seconds, 21600)))
    END,
    last_error = NULL,
    source_format = ?,
    raw_content_hash = ?,
    updated_at = ?
WHERE id = ?
"#,
        )
        .bind(commit.subscription_name)
        .bind(upload_bytes)
        .bind(download_bytes)
        .bind(total_bytes)
        .bind(commit.expire_at)
        .bind(commit.node_count)
        .bind(&now)
        .bind(&now)
        .bind(commit.source_format)
        .bind(commit.raw_content_hash)
        .bind(&now)
        .bind(subscription_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "subscription_not_found",
                format!("subscription {subscription_id} not found"),
            ));
        }
        tx.commit().await?;
        if migration.reference_count > 0 {
            info!(
                subscription_id,
                migrated_references = migration.reference_count,
                "migrated subscription asset references to stable runtime names"
            );
        }
        Ok(())
    }

    #[cfg(test)]
    pub async fn upsert_proxy_item(&self, item: &ProxyItemRecord) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        upsert_proxy_item_in_transaction(&mut tx, item, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_manual_nodes(&self) -> Result<Vec<ManualNodeResponse>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT name, display_name, protocol, raw_json, latency_ms
FROM proxy_items
WHERE kind = 'node' AND source = 'manual'
ORDER BY position, created_at, name
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let raw_json: String = row.try_get("raw_json")?;
                Ok(ManualNodeResponse {
                    name: row.try_get("name")?,
                    display_name: row.try_get("display_name")?,
                    protocol: row.try_get("protocol")?,
                    config: serde_json::from_str(&raw_json).map_err(AppError::from)?,
                    latency: row.try_get::<Option<i64>, _>("latency_ms")?.unwrap_or(-1),
                })
            })
            .collect()
    }

    pub async fn create_manual_node(&self, item: &ProxyItemRecord) -> Result<(), AppError> {
        validate_manual_record(item)?;
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM proxy_items WHERE name = ?)",
        )
        .bind(&item.name)
        .fetch_one(&mut *tx)
        .await?;
        if exists {
            return Err(AppError::conflict(
                "manual_node_exists",
                format!("proxy item {} already exists", item.name),
            ));
        }
        let mut item = item.clone();
        item.position = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(position), 0) + 1024 FROM proxy_items WHERE kind = 'node'",
        )
        .fetch_one(&mut *tx)
        .await?;
        upsert_proxy_item_in_transaction(&mut tx, &item, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_manual_node(&self, item: &ProxyItemRecord) -> Result<(), AppError> {
        validate_manual_record(item)?;
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = sqlx::query(
            "SELECT source, position, latency_ms FROM proxy_items WHERE name = ? AND kind = 'node'",
        )
        .bind(&item.name)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            AppError::not_found(
                "manual_node_not_found",
                format!("manual node {} not found", item.name),
            )
        })?;
        if existing.try_get::<String, _>("source")? != "manual" {
            return Err(AppError::conflict(
                "manual_node_readonly",
                "subscription nodes cannot be edited as manual nodes",
            ));
        }
        let mut item = item.clone();
        item.position = existing.try_get("position")?;
        item.latency_ms = existing.try_get("latency_ms")?;
        upsert_proxy_item_in_transaction(&mut tx, &item, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_manual_node(&self, name: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let source = sqlx::query_scalar::<_, String>(
            "SELECT source FROM proxy_items WHERE name = ? AND kind = 'node'",
        )
        .bind(name)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            AppError::not_found(
                "manual_node_not_found",
                format!("manual node {name} not found"),
            )
        })?;
        if source != "manual" {
            return Err(AppError::conflict(
                "manual_node_readonly",
                "subscription nodes cannot be deleted as manual nodes",
            ));
        }
        if let Some(rule_id) =
            sqlx::query_scalar::<_, String>("SELECT id FROM routing_rules WHERE policy = ? LIMIT 1")
                .bind(name)
                .fetch_optional(&mut *tx)
                .await?
        {
            return Err(AppError::conflict(
                "manual_node_referenced",
                format!("manual node {name} is referenced by routing rule {rule_id}"),
            ));
        }
        sqlx::query("DELETE FROM proxy_group_members WHERE member_name = ?")
            .bind(name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM proxy_items WHERE name = ?")
            .bind(name)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn sync_builtin_proxy_group(&self) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut members = self.valid_node_names().await?;
        let group_names = sqlx::query_scalar::<_, String>(
            r#"
SELECT name FROM proxy_items
WHERE kind = 'group' AND name <> ? AND enabled = 1 AND filtered_out = 0
  AND (
    subscription_id IS NULL
    OR EXISTS (
      SELECT 1 FROM subscriptions
      WHERE subscriptions.id = proxy_items.subscription_id
        AND subscriptions.ready = 1
    )
  )
ORDER BY position, name
"#,
        )
        .bind(BUILTIN_PROXY_GROUP_NAME)
        .fetch_all(&self.pool)
        .await?;
        for group_name in group_names {
            let depends_on_proxy = sqlx::query_scalar::<_, bool>(
                r#"
WITH RECURSIVE dependencies(name) AS (
  SELECT member_name FROM proxy_group_members WHERE group_name = ?
  UNION
  SELECT members.member_name
  FROM proxy_group_members members
  JOIN dependencies ON members.group_name = dependencies.name
)
SELECT EXISTS(SELECT 1 FROM dependencies WHERE name = ?)
"#,
            )
            .bind(&group_name)
            .bind(BUILTIN_PROXY_GROUP_NAME)
            .fetch_one(&self.pool)
            .await?;
            if !depends_on_proxy {
                members.push(group_name);
            }
        }
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current_delay = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT delay_ms FROM proxy_items WHERE name = ? AND kind = 'group'",
        )
        .bind(BUILTIN_PROXY_GROUP_NAME)
        .fetch_optional(&mut *tx)
        .await?
        .flatten();
        let current_now = current_group_now_in_transaction(&mut tx, BUILTIN_PROXY_GROUP_NAME)
            .await?
            .filter(|selected| members.iter().any(|member| member == selected));
        let selected = current_now.or_else(|| members.first().cloned());
        upsert_proxy_item_in_transaction(
            &mut tx,
            &ProxyItemRecord {
                name: BUILTIN_PROXY_GROUP_NAME.into(),
                kind: "group".into(),
                subscription_id: None,
                display_name: BUILTIN_PROXY_GROUP_NAME.into(),
                source: "system".into(),
                builtin: true,
                source_name: Some("system".into()),
                protocol: None,
                country: None,
                group_type: Some("select".into()),
                raw_json: None,
                content_hash: None,
                latency_ms: None,
                alive: true,
                filtered_out: false,
                filter_reason: None,
                delay_ms: current_delay,
                tolerance_ms: None,
                url: None,
                interval_seconds: None,
                strategy_json: selected
                    .as_ref()
                    .map(|name| serde_json::json!({ "now": name }).to_string())
                    .unwrap_or_else(|| "{}".to_string()),
                position: -100_000,
                enabled: true,
            },
            &now,
        )
        .await?;
        replace_group_members_in_transaction(
            &mut tx,
            BUILTIN_PROXY_GROUP_NAME,
            &members,
            selected,
            &now,
        )
        .await?;
        sqlx::query("DELETE FROM proxy_group_filters WHERE group_name = ?")
            .bind(BUILTIN_PROXY_GROUP_NAME)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    #[cfg(test)]
    pub async fn replace_group_members(
        &self,
        group_name: &str,
        members: &[String],
    ) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        replace_group_members_in_transaction(&mut tx, group_name, members, None, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    #[cfg(test)]
    pub async fn replace_group_filters(
        &self,
        group_name: &str,
        filters: &[GroupFilterInput],
    ) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        replace_group_filters_in_transaction(&mut tx, group_name, filters, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn save_custom_group(
        &self,
        old_name: Option<&str>,
        item: &ProxyItemRecord,
        filters: &[GroupFilterInput],
    ) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        if item.kind != "group" || item.source != "custom" || item.builtin {
            return Err(AppError::internal("invalid custom proxy group record"));
        }

        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let previous_selection = if let Some(old_name) = old_name {
            let source = sqlx::query_scalar::<_, String>(
                "SELECT source FROM proxy_items WHERE name = ? AND kind = 'group'",
            )
            .bind(old_name)
            .fetch_optional(&mut *tx)
            .await?;
            match source {
                Some(source) if source == "custom" => {}
                Some(_) => {
                    return Err(AppError::conflict(
                        "proxy_group_readonly",
                        "system builtin and subscription managed proxy groups are read-only",
                    ));
                }
                None => {
                    return Err(AppError::not_found(
                        "proxy_group_not_found",
                        format!("proxy group {old_name} not found"),
                    ));
                }
            }

            let selection = current_group_now_in_transaction(&mut tx, old_name).await?;
            if old_name != item.name {
                let target_exists = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM proxy_items WHERE name = ?)",
                )
                .bind(&item.name)
                .fetch_one(&mut *tx)
                .await?;
                if target_exists {
                    return Err(AppError::conflict(
                        "proxy_group_exists",
                        format!("proxy group {} already exists", item.name),
                    ));
                }
                let reference_count = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM routing_rules WHERE policy = ?",
                )
                .bind(old_name)
                .fetch_one(&mut *tx)
                .await?;
                if reference_count > 0 {
                    return Err(AppError::conflict(
                        "proxy_group_referenced",
                        "proxy groups referenced by routing rules cannot be renamed",
                    ));
                }
                sqlx::query(
                    "DELETE FROM proxy_items WHERE name = ? AND kind = 'group' AND source = 'custom'",
                )
                .bind(old_name)
                .execute(&mut *tx)
                .await?;
            }
            selection
        } else {
            let target_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM proxy_items WHERE name = ?)",
            )
            .bind(&item.name)
            .fetch_one(&mut *tx)
            .await?;
            if target_exists {
                return Err(AppError::conflict(
                    "proxy_group_exists",
                    format!("proxy group {} already exists", item.name),
                ));
            }
            None
        };

        let nodes = valid_node_records_in_transaction(&mut tx).await?;
        let members = crate::proxy::calculate_members(&nodes, filters);
        if members.is_empty() {
            return Err(AppError::bad_request(
                "proxy_group_empty",
                "custom proxy group filters did not match any nodes",
            ));
        }

        upsert_proxy_item_in_transaction(&mut tx, item, &now).await?;
        replace_group_filters_in_transaction(&mut tx, &item.name, filters, &now).await?;
        replace_group_members_in_transaction(
            &mut tx,
            &item.name,
            &members,
            previous_selection,
            &now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_custom_group(&self, group_name: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let source = sqlx::query_scalar::<_, String>(
            "SELECT source FROM proxy_items WHERE name = ? AND kind = 'group'",
        )
        .bind(group_name)
        .fetch_optional(&mut *tx)
        .await?;
        match source {
            Some(source) if source == "custom" => {}
            Some(_) => {
                return Err(AppError::conflict(
                    "proxy_group_readonly",
                    "system builtin and subscription managed proxy groups are read-only",
                ));
            }
            None => {
                return Err(AppError::not_found(
                    "proxy_group_not_found",
                    format!("proxy group {group_name} not found"),
                ));
            }
        }
        let reference_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM routing_rules WHERE policy = ?")
                .bind(group_name)
                .fetch_one(&mut *tx)
                .await?;
        if reference_count > 0 {
            return Err(AppError::conflict(
                "proxy_group_referenced",
                "proxy groups referenced by routing rules cannot be deleted",
            ));
        }
        let result = sqlx::query(
            "DELETE FROM proxy_items WHERE name = ? AND kind = 'group' AND source = 'custom'",
        )
        .bind(group_name)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "proxy_group_not_found",
                format!("custom proxy group {group_name} not found"),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_group_now(
        &self,
        group_name: &str,
        member_name: &str,
    ) -> Result<Option<String>, AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_select_group_member_in_transaction(&mut tx, group_name, member_name).await?;
        let previous = current_group_now_in_transaction(&mut tx, group_name).await?;
        let now = now_iso();
        let strategy = serde_json::json!({ "now": member_name }).to_string();
        sqlx::query("UPDATE proxy_items SET strategy_json = ?, updated_at = ? WHERE name = ? AND kind = 'group'")
            .bind(strategy)
            .bind(now)
            .bind(group_name)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(previous)
    }

    pub async fn restore_group_now(
        &self,
        group_name: &str,
        member_name: Option<&str>,
    ) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let group_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM proxy_items WHERE name = ? AND kind = 'group')",
        )
        .bind(group_name)
        .fetch_one(&mut *tx)
        .await?;
        if !group_exists {
            return Err(AppError::not_found(
                "proxy_group_not_found",
                format!("proxy group {group_name} not found"),
            ));
        }
        if let Some(member_name) = member_name {
            let member_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM proxy_group_members WHERE group_name = ? AND member_name = ?)",
            )
            .bind(group_name)
            .bind(member_name)
            .fetch_one(&mut *tx)
            .await?;
            if !member_exists {
                return Err(AppError::bad_request(
                    "proxy_group_member_not_found",
                    format!("proxy {member_name} is not a member of group {group_name}"),
                ));
            }
        }
        let strategy = member_name
            .map(|name| serde_json::json!({ "now": name }).to_string())
            .unwrap_or_else(|| "{}".into());
        sqlx::query(
            "UPDATE proxy_items SET strategy_json = ?, updated_at = ? WHERE name = ? AND kind = 'group'",
        )
        .bind(strategy)
        .bind(now_iso())
        .bind(group_name)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn clear_group_selections(&self) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        sqlx::query(
            "UPDATE proxy_items SET strategy_json = '{}', updated_at = ? WHERE kind = 'group' AND strategy_json <> '{}'",
        )
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn due_probe_node_names(&self, limit: i64) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT proxy_items.name
FROM proxy_items
WHERE proxy_items.kind = 'node'
  AND proxy_items.enabled = 1
  AND proxy_items.filtered_out = 0
  AND (
    proxy_items.subscription_id IS NULL
    OR EXISTS (
      SELECT 1 FROM subscriptions
      WHERE subscriptions.id = proxy_items.subscription_id
        AND subscriptions.ready = 1
    )
  )
  AND (
    proxy_items.next_probe_at IS NULL
    OR proxy_items.next_probe_at <= ?
  )
  AND NOT EXISTS (
    SELECT 1
    FROM proxy_group_members
    JOIN proxy_items AS active_groups
      ON active_groups.name = proxy_group_members.group_name
     AND active_groups.kind = 'group'
    WHERE proxy_group_members.member_name = proxy_items.name
      AND active_groups.group_type IN ('url-test', 'fallback', 'load-balance')
  )
ORDER BY
  CASE WHEN proxy_items.probe_status = 'unknown' THEN 0 ELSE 1 END,
  COALESCE(proxy_items.next_probe_at, '1970-01-01T00:00:00Z'),
  proxy_items.position,
  proxy_items.name
LIMIT ?
"#,
        )
        .bind(now_iso())
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("name").map_err(AppError::from))
            .collect()
    }

    pub async fn record_node_probe_success(
        &self,
        node_name: &str,
        delay: i64,
    ) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let next_probe_at = crate::probe::next_probe_at(node_name, true, 0);
        sqlx::query(
            r#"
UPDATE proxy_items
SET latency_ms = ?,
    last_good_latency_ms = ?,
    last_test_at = ?,
    probe_status = 'healthy',
    probe_failures = 0,
    next_probe_at = ?,
    last_success_at = ?,
    last_probe_error = NULL,
    updated_at = ?
WHERE name = ? AND kind = 'node'
"#,
        )
        .bind(delay)
        .bind(delay)
        .bind(&now)
        .bind(next_probe_at)
        .bind(&now)
        .bind(&now)
        .bind(node_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record_node_probe_failure(
        &self,
        node_name: &str,
        error: &str,
    ) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let current_failures = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(probe_failures, 0) FROM proxy_items WHERE name = ? AND kind = 'node'",
        )
        .bind(node_name)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(0)
        .max(0) as u32;
        let failures = current_failures.saturating_add(1);
        let next_probe_at = crate::probe::next_probe_at(node_name, false, failures);
        let now = now_iso();
        let error = error.chars().take(512).collect::<String>();
        sqlx::query(
            r#"
UPDATE proxy_items
SET latency_ms = 0,
    last_test_at = ?,
    probe_status = CASE WHEN ? >= 3 THEN 'unhealthy' ELSE 'degraded' END,
    probe_failures = ?,
    next_probe_at = ?,
    last_probe_error = ?,
    updated_at = ?
WHERE name = ? AND kind = 'node'
"#,
        )
        .bind(&now)
        .bind(failures as i64)
        .bind(failures as i64)
        .bind(next_probe_at)
        .bind(error)
        .bind(&now)
        .bind(node_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_group_delay(&self, group_name: &str, delay: i64) -> Result<(), AppError> {
        let now = now_iso();
        sqlx::query("UPDATE proxy_items SET delay_ms = ?, last_test_at = ?, updated_at = ? WHERE name = ? AND kind = 'group'")
            .bind(delay)
            .bind(&now)
            .bind(&now)
            .bind(group_name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn group_source(&self, group_name: &str) -> Result<Option<String>, AppError> {
        let row = sqlx::query("SELECT source FROM proxy_items WHERE name = ? AND kind = 'group'")
            .bind(group_name)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| row.try_get("source"))
            .transpose()
            .map_err(AppError::from)
    }

    pub async fn policy_reference_count(&self, policy: &str) -> Result<i64, AppError> {
        let row = sqlx::query("SELECT COUNT(*) AS count FROM routing_rules WHERE policy = ?")
            .bind(policy)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("count")?)
    }

    pub async fn proxy_topology(
        &self,
    ) -> Result<(Vec<ProxyGroupResponse>, Vec<ProxyNodeResponse>), AppError> {
        self.sync_builtin_proxy_group().await?;
        let node_rows = sqlx::query(
            r#"
SELECT name, display_name, protocol, latency_ms, country, subscription_id, source_name
FROM proxy_items
WHERE kind = 'node' AND filtered_out = 0 AND enabled = 1
  AND (
    subscription_id IS NULL
    OR EXISTS (
      SELECT 1 FROM subscriptions
      WHERE subscriptions.id = proxy_items.subscription_id
        AND subscriptions.ready = 1
    )
  )
ORDER BY subscription_id, display_name
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        let nodes = node_rows
            .into_iter()
            .map(|row| {
                Ok(ProxyNodeResponse {
                    name: row.try_get("name")?,
                    display_name: row.try_get("display_name")?,
                    protocol: row
                        .try_get::<Option<String>, _>("protocol")?
                        .unwrap_or_else(|| "unknown".into()),
                    latency: row.try_get::<Option<i64>, _>("latency_ms")?.unwrap_or(0),
                    country: row.try_get("country")?,
                    subscription_id: row.try_get("subscription_id")?,
                    subscription_name: row.try_get("source_name")?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let groups_rows = sqlx::query(
            r#"
SELECT name, display_name, source, builtin, source_name, group_type, delay_ms, strategy_json
FROM proxy_items
WHERE kind = 'group' AND enabled = 1 AND filtered_out = 0
  AND (
    subscription_id IS NULL
    OR EXISTS (
      SELECT 1 FROM subscriptions
      WHERE subscriptions.id = proxy_items.subscription_id
        AND subscriptions.ready = 1
    )
  )
ORDER BY position, created_at
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut groups = Vec::with_capacity(groups_rows.len());
        for row in groups_rows {
            let name: String = row.try_get("name")?;
            let source: String = row.try_get("source")?;
            let subscription_name = if source == "subscription" {
                row.try_get("source_name")?
            } else {
                None
            };
            let filters = self.group_filters(&name).await?;
            let members = if source == "custom" {
                crate::proxy::calculate_members(&nodes, &filters)
            } else {
                self.group_members(&name).await?
            };
            let strategy: String = row.try_get("strategy_json")?;
            let now = serde_json::from_str::<Value>(&strategy)
                .ok()
                .and_then(|value| value.get("now").and_then(Value::as_str).map(str::to_string))
                .filter(|selected| members.contains(selected))
                .or_else(|| members.first().cloned());
            groups.push(ProxyGroupResponse {
                name,
                display_name: row.try_get("display_name")?,
                group_type: row
                    .try_get::<Option<String>, _>("group_type")?
                    .unwrap_or_else(|| "select".into()),
                source,
                subscription_name,
                builtin: i64_to_bool(row.try_get("builtin")?),
                now,
                delay: row.try_get::<Option<i64>, _>("delay_ms")?.unwrap_or(0),
                all: members,
                filter: filters,
            });
        }

        Ok((groups, nodes))
    }

    pub async fn group_members(&self, group_name: &str) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            "SELECT member_name FROM proxy_group_members WHERE group_name = ? ORDER BY position, member_name",
        )
        .bind(group_name)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("member_name").map_err(AppError::from))
            .collect()
    }

    pub async fn group_filters(&self, group_name: &str) -> Result<Vec<GroupFilterInput>, AppError> {
        let rows = sqlx::query(
            "SELECT id, action, field, operator, value, values_json, enabled FROM proxy_group_filters WHERE group_name = ? ORDER BY position",
        )
        .bind(group_name)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(group_filter_from_row).collect()
    }

    async fn valid_node_names(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT name
FROM proxy_items
WHERE kind = 'node' AND enabled = 1 AND filtered_out = 0
  AND (
    subscription_id IS NULL
    OR EXISTS (
      SELECT 1 FROM subscriptions
      WHERE subscriptions.id = proxy_items.subscription_id
        AND subscriptions.ready = 1
    )
  )
ORDER BY subscription_id, position, display_name, name
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("name").map_err(AppError::from))
            .collect()
    }

    #[cfg(test)]
    async fn current_group_now(&self, group_name: &str) -> Result<Option<String>, AppError> {
        let row =
            sqlx::query("SELECT strategy_json FROM proxy_items WHERE name = ? AND kind = 'group'")
                .bind(group_name)
                .fetch_optional(&self.pool)
                .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let strategy: String = row.try_get("strategy_json")?;
        Ok(serde_json::from_str::<Value>(&strategy)
            .ok()
            .and_then(|value| value.get("now").and_then(Value::as_str).map(str::to_string)))
    }

    pub async fn proxy_items_for_runtime(&self) -> Result<Vec<ProxyItemRecord>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT name, kind, subscription_id, display_name, source, builtin, source_name, protocol, country,
       group_type, raw_json, content_hash, latency_ms, alive, filtered_out, filter_reason,
       delay_ms, tolerance_ms, url, interval_seconds, strategy_json, position, enabled
FROM proxy_items
WHERE enabled = 1
  AND (
    subscription_id IS NULL
    OR EXISTS (
      SELECT 1 FROM subscriptions
      WHERE subscriptions.id = proxy_items.subscription_id
        AND subscriptions.ready <> 2
    )
  )
ORDER BY kind, position, created_at
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(proxy_item_from_row)
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn list_rules(&self) -> Result<Vec<RuleResponse>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id, position, rule_type, value, policy, source, enabled, desc
FROM routing_rules
ORDER BY CASE WHEN source = 'user' THEN 0 ELSE 1 END, position, created_at, id
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(rule_from_row)
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn upsert_rule(
        &self,
        id: Option<String>,
        rule_type: &str,
        value: &str,
        policy: &str,
        desc: Option<&str>,
        enabled: bool,
    ) -> Result<RuleResponse, AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let id = id.unwrap_or_else(|| new_id("rule"));
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_rule_target_in_transaction(&mut tx, rule_type, value, policy).await?;
        let existing_source =
            sqlx::query_scalar::<_, String>("SELECT source FROM routing_rules WHERE id = ?")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?;
        let source = existing_source.as_deref().unwrap_or("user");
        validate_single_enabled_match_in_transaction(
            &mut tx,
            source,
            Some(&id),
            rule_type,
            enabled,
        )
        .await?;

        if existing_source.is_some() {
            sqlx::query(
                r#"
UPDATE routing_rules
SET rule_type = ?, value = ?, policy = ?, enabled = ?, desc = ?, updated_at = ?
WHERE id = ?
"#,
            )
            .bind(rule_type)
            .bind(value)
            .bind(policy)
            .bind(bool_to_i64(enabled))
            .bind(desc)
            .bind(&now)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
            move_rule_in_transaction(&mut tx, &id, None, &now).await?;
        } else {
            let first_match_position = if rule_type == "MATCH" {
                None
            } else {
                sqlx::query_scalar::<_, Option<i64>>(
                    "SELECT MIN(position) FROM routing_rules WHERE source = 'user' AND rule_type = 'MATCH'",
                )
                .fetch_one(&mut *tx)
                .await?
            };
            let position = if let Some(position) = first_match_position {
                sqlx::query(
                    "UPDATE routing_rules SET position = position + 1024 WHERE source = 'user' AND position >= ?",
                )
                .bind(position)
                .execute(&mut *tx)
                .await?;
                position
            } else {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COALESCE(MAX(position), 0) + 1024 FROM routing_rules WHERE source = 'user'",
                )
                .fetch_one(&mut *tx)
                .await?
            };

            sqlx::query(
                r#"
INSERT INTO routing_rules(id, position, rule_type, value, policy, source, enabled, desc, created_at, updated_at)
VALUES(?, ?, ?, ?, ?, 'user', ?, ?, ?, ?)
"#,
            )
            .bind(&id)
            .bind(position)
            .bind(rule_type)
            .bind(value)
            .bind(policy)
            .bind(bool_to_i64(enabled))
            .bind(desc)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
                .await?;
        }
        let rule = rule_by_id_in_transaction(&mut tx, &id).await?;
        tx.commit().await?;
        Ok(rule)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_rule(
        &self,
        id: &str,
        rule_type: &str,
        value: &str,
        policy: &str,
        desc: Option<&str>,
        enabled: bool,
        target_position: Option<usize>,
    ) -> Result<RuleResponse, AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let source =
            sqlx::query_scalar::<_, String>("SELECT source FROM routing_rules WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| {
                    AppError::not_found("rule_not_found", format!("rule {id} not found"))
                })?;
        validate_rule_target_in_transaction(&mut tx, rule_type, value, policy).await?;
        validate_single_enabled_match_in_transaction(
            &mut tx,
            &source,
            Some(id),
            rule_type,
            enabled,
        )
        .await?;
        let result = sqlx::query(
            r#"
UPDATE routing_rules
SET rule_type = ?, value = ?, policy = ?, enabled = ?, desc = ?, updated_at = ?
WHERE id = ?
"#,
        )
        .bind(rule_type)
        .bind(value)
        .bind(policy)
        .bind(bool_to_i64(enabled))
        .bind(desc)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "rule_not_found",
                format!("rule {id} not found"),
            ));
        }
        move_rule_in_transaction(&mut tx, id, target_position, &now).await?;
        let rule = rule_by_id_in_transaction(&mut tx, id).await?;
        tx.commit().await?;
        Ok(rule)
    }

    #[cfg(test)]
    pub async fn move_rule(
        &self,
        id: &str,
        target_position: usize,
    ) -> Result<RuleResponse, AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        move_rule_in_transaction(&mut tx, id, Some(target_position), &now).await?;
        let rule = rule_by_id_in_transaction(&mut tx, id).await?;
        tx.commit().await?;
        Ok(rule)
    }

    pub async fn delete_rule(&self, id: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = sqlx::query("DELETE FROM routing_rules WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "rule_not_found",
                format!("rule {id} not found"),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_rule_sets(&self) -> Result<Vec<RuleSetResponse>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, url, behavior, format, rule_count, last_update_at, last_error, download_route, last_route FROM rule_sets WHERE ready = 1 ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(RuleSetResponse {
                    id: row.try_get("id")?,
                    name: row.try_get("name")?,
                    url: row.try_get("url")?,
                    behavior: row.try_get("behavior")?,
                    format: row.try_get("format")?,
                    rule_count: row.try_get("rule_count")?,
                    last_update: row.try_get("last_update_at")?,
                    last_error: row.try_get("last_error")?,
                    download_route: download_route_from_str(
                        &row.try_get::<String, _>("download_route")?,
                    ),
                    last_route: row.try_get("last_route")?,
                })
            })
            .collect()
    }

    pub async fn rule_set_including_staged(
        &self,
        id: &str,
    ) -> Result<Option<RuleSetResponse>, AppError> {
        let row = sqlx::query(
            r#"
SELECT id, name, url, behavior,
       COALESCE(staged_format, format) AS format,
       COALESCE(staged_rule_count, rule_count) AS rule_count,
       COALESCE(staged_update_at, last_update_at) AS last_update_at,
       COALESCE(staged_last_error, last_error) AS last_error,
       download_route, last_route
FROM rule_sets
WHERE id = ? AND (ready = 1 OR staged_local_path IS NOT NULL)
"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(RuleSetResponse {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                url: row.try_get("url")?,
                behavior: row.try_get("behavior")?,
                format: row.try_get("format")?,
                rule_count: row.try_get("rule_count")?,
                last_update: row.try_get("last_update_at")?,
                last_error: row.try_get("last_error")?,
                download_route: download_route_from_str(
                    &row.try_get::<String, _>("download_route")?,
                ),
                last_route: row.try_get("last_route")?,
            })
        })
        .transpose()
    }

    pub async fn rule_sets_for_runtime(&self) -> Result<Vec<RuleSetRecord>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id, name, url, behavior,
       COALESCE(staged_format, format) AS format,
       COALESCE(staged_local_path, local_path) AS local_path,
       download_route
FROM rule_sets
WHERE ready = 1 OR staged_local_path IS NOT NULL
ORDER BY name
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(RuleSetRecord {
                    id: row.try_get("id")?,
                    name: row.try_get("name")?,
                    url: row.try_get("url")?,
                    behavior: row.try_get("behavior")?,
                    format: row.try_get("format")?,
                    local_path: row.try_get("local_path")?,
                    download_route: download_route_from_str(
                        &row.try_get::<String, _>("download_route")?,
                    ),
                })
            })
            .collect()
    }

    pub async fn rule_set_snapshot_paths(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query("SELECT local_path, staged_local_path FROM rule_sets")
            .fetch_all(&self.pool)
            .await?;
        let mut paths = Vec::with_capacity(rows.len() * 2);
        for row in rows {
            if let Some(path) = row.try_get::<Option<String>, _>("local_path")? {
                paths.push(path);
            }
            if let Some(path) = row.try_get::<Option<String>, _>("staged_local_path")? {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    pub async fn rule_set_snapshot_paths_for_id(&self, id: &str) -> Result<Vec<String>, AppError> {
        let row = sqlx::query("SELECT local_path, staged_local_path FROM rule_sets WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(Vec::new());
        };
        let mut paths = Vec::with_capacity(2);
        if let Some(path) = row.try_get::<Option<String>, _>("local_path")? {
            paths.push(path);
        }
        if let Some(path) = row.try_get::<Option<String>, _>("staged_local_path")? {
            paths.push(path);
        }
        Ok(paths)
    }

    pub async fn rule_set_for_refresh(&self, id: &str) -> Result<Option<RuleSetRecord>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, url, behavior, format, local_path, download_route FROM rule_sets WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(RuleSetRecord {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                url: row.try_get("url")?,
                behavior: row.try_get("behavior")?,
                format: row.try_get("format")?,
                local_path: row.try_get("local_path")?,
                download_route: download_route_from_str(
                    &row.try_get::<String, _>("download_route")?,
                ),
            })
        })
        .transpose()
    }

    pub async fn due_rule_set_ids(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id
FROM rule_sets
WHERE ready = 1
  AND interval_seconds > 0
  AND CAST(
    strftime(
      '%s',
      CASE WHEN last_error IS NOT NULL THEN updated_at ELSE COALESCE(last_update_at, created_at) END
    ) AS INTEGER
  ) + CASE
    WHEN last_error IS NOT NULL THEN 3600
    ELSE MAX(interval_seconds, 21600)
  END <= CAST(strftime('%s', 'now') AS INTEGER)
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("id").map_err(AppError::from))
            .collect()
    }

    #[cfg(test)]
    pub async fn create_rule_set(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        behavior: Option<&str>,
        format: &str,
    ) -> Result<(), AppError> {
        self.create_rule_set_with_ready(
            id,
            name,
            url,
            interval_seconds,
            behavior,
            format,
            DownloadRoute::Auto,
            true,
        )
        .await
    }

    #[cfg(test)]
    pub async fn create_pending_rule_set(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        behavior: Option<&str>,
        format: &str,
    ) -> Result<(), AppError> {
        self.create_rule_set_with_ready(
            id,
            name,
            url,
            interval_seconds,
            behavior,
            format,
            DownloadRoute::Auto,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_pending_rule_set_with_route(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        behavior: Option<&str>,
        format: &str,
        download_route: DownloadRoute,
    ) -> Result<(), AppError> {
        self.create_rule_set_with_ready(
            id,
            name,
            url,
            interval_seconds,
            behavior,
            format,
            download_route,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_rule_set_with_ready(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        behavior: Option<&str>,
        format: &str,
        download_route: DownloadRoute,
        ready: bool,
    ) -> Result<(), AppError> {
        let interval_seconds = sqlite_i64(interval_seconds, "rule set interval_seconds")?;
        let _mutation = self.topology_mutation.lock().await;
        let now = now_iso();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = sqlx::query_scalar::<_, String>(
            "SELECT id FROM rule_sets WHERE id = ? OR name = ? LIMIT 1",
        )
        .bind(id)
        .bind(name)
        .fetch_optional(&mut *tx)
        .await?;
        if existing.is_some() {
            return Err(AppError::conflict(
                "ruleset_exists",
                format!("rule set {name} already exists"),
            ));
        }
        sqlx::query(
            r#"
INSERT INTO rule_sets(id, name, url, ready, behavior, format, interval_seconds, download_route, created_at, updated_at)
VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#,
        )
        .bind(id)
        .bind(name)
        .bind(url)
        .bind(bool_to_i64(ready))
        .bind(behavior)
        .bind(format)
        .bind(interval_seconds)
        .bind(download_route.as_str())
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_rule_set(&self, id: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT name FROM rule_sets WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                AppError::not_found("ruleset_not_found", format!("rule set {id} not found"))
            })?;
        if BUILTIN_RULE_SETS.iter().any(|rule_set| rule_set.id == id) {
            return Err(AppError::conflict(
                "ruleset_readonly",
                "builtin rule sets cannot be deleted",
            ));
        }
        let name: String = row.try_get("name")?;
        let referenced_by = sqlx::query_scalar::<_, String>(
            "SELECT id FROM routing_rules WHERE rule_type = 'RULE-SET' AND value = ? LIMIT 1",
        )
        .bind(&name)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(rule_id) = referenced_by {
            return Err(AppError::conflict(
                "ruleset_in_use",
                format!("rule set {name} is referenced by routing rule {rule_id}"),
            ));
        }
        sqlx::query("DELETE FROM rule_sets WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn stage_rule_set_deletion(&self, id: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT name FROM rule_sets WHERE id = ? AND ready = 1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                AppError::not_found("ruleset_not_found", format!("rule set {id} not found"))
            })?;
        if BUILTIN_RULE_SETS.iter().any(|rule_set| rule_set.id == id) {
            return Err(AppError::conflict(
                "ruleset_readonly",
                "builtin rule sets cannot be deleted",
            ));
        }
        let name: String = row.try_get("name")?;
        let referenced_by = sqlx::query_scalar::<_, String>(
            "SELECT id FROM routing_rules WHERE rule_type = 'RULE-SET' AND value = ? LIMIT 1",
        )
        .bind(&name)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(rule_id) = referenced_by {
            return Err(AppError::conflict(
                "ruleset_in_use",
                format!("rule set {name} is referenced by routing rule {rule_id}"),
            ));
        }
        sqlx::query("UPDATE rule_sets SET ready = 3, updated_at = ? WHERE id = ?")
            .bind(now_iso())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn restore_rule_set_deletion(&self, id: &str) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE rule_sets SET ready = 1, updated_at = ? WHERE id = ? AND ready = 3",
        )
        .bind(now_iso())
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "ruleset_not_found",
                format!("staged rule-set deletion {id} not found"),
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_rule_set_refresh(
        &self,
        id: &str,
        local_path: &str,
        file_size_bytes: u64,
        rule_count: u64,
        content_hash: &str,
        format: &str,
        last_error: Option<&str>,
    ) -> Result<RuleSetRefreshState, AppError> {
        let previous = self
            .stage_rule_set_refresh(
                id,
                local_path,
                file_size_bytes,
                rule_count,
                content_hash,
                format,
                last_error,
            )
            .await?;
        if let Err(error) = self.activate_rule_set(id).await {
            let restore = self.restore_rule_set_refresh(id, &previous).await;
            return Err(AppError::internal(format!(
                "committing rule set {id} failed ({error}); restoring previous metadata: {restore:?}"
            )));
        }
        Ok(previous)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn stage_rule_set_refresh(
        &self,
        id: &str,
        local_path: &str,
        file_size_bytes: u64,
        rule_count: u64,
        content_hash: &str,
        format: &str,
        last_error: Option<&str>,
    ) -> Result<RuleSetRefreshState, AppError> {
        let file_size_bytes = sqlite_i64(file_size_bytes, "rule set file_size_bytes")?;
        let rule_count = sqlite_i64(rule_count, "rule set rule_count")?;
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let previous = sqlx::query(
            "SELECT ready, local_path, file_size_bytes, rule_count, content_hash, format, last_update_at, last_error FROM rule_sets WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            AppError::not_found("ruleset_not_found", format!("rule set {id} not found"))
        })?;
        let previous = RuleSetRefreshState {
            ready: i64_to_bool(previous.try_get("ready")?),
            local_path: previous.try_get("local_path")?,
            file_size_bytes: previous.try_get("file_size_bytes")?,
            rule_count: previous.try_get("rule_count")?,
            content_hash: previous.try_get("content_hash")?,
            format: previous.try_get("format")?,
            last_update_at: previous.try_get("last_update_at")?,
            last_error: previous.try_get("last_error")?,
        };
        let now = now_iso();
        let result = sqlx::query(
            r#"
UPDATE rule_sets
SET staged_local_path = ?,
    staged_file_size_bytes = ?,
    staged_rule_count = ?,
    staged_content_hash = ?,
    staged_format = ?,
    staged_update_at = ?,
    staged_last_error = ?,
    updated_at = ?
WHERE id = ?
"#,
        )
        .bind(local_path)
        .bind(file_size_bytes)
        .bind(rule_count)
        .bind(content_hash)
        .bind(format)
        .bind(&now)
        .bind(last_error)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "ruleset_not_found",
                format!("rule set {id} not found"),
            ));
        }
        tx.commit().await?;
        Ok(previous)
    }

    pub async fn activate_rule_set(&self, id: &str) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = sqlx::query(
            r#"
UPDATE rule_sets
SET ready = 1,
    local_path = staged_local_path,
    file_size_bytes = staged_file_size_bytes,
    rule_count = staged_rule_count,
    content_hash = staged_content_hash,
    format = staged_format,
    last_update_at = staged_update_at,
    last_error = staged_last_error,
    staged_local_path = NULL,
    staged_file_size_bytes = NULL,
    staged_rule_count = NULL,
    staged_content_hash = NULL,
    staged_format = NULL,
    staged_update_at = NULL,
    staged_last_error = NULL,
    updated_at = ?
WHERE id = ? AND staged_local_path IS NOT NULL
"#,
        )
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "ruleset_not_found",
                format!("staged rule set {id} not found"),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn restore_rule_set_refresh(
        &self,
        id: &str,
        previous: &RuleSetRefreshState,
    ) -> Result<(), AppError> {
        let _mutation = self.topology_mutation.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = sqlx::query(
            r#"
UPDATE rule_sets
SET ready = ?, local_path = ?, file_size_bytes = ?, rule_count = ?, content_hash = ?,
    format = ?, last_update_at = ?, last_error = ?,
    staged_local_path = NULL, staged_file_size_bytes = NULL, staged_rule_count = NULL,
    staged_content_hash = NULL, staged_format = NULL, staged_update_at = NULL,
    staged_last_error = NULL, updated_at = ?
WHERE id = ?
"#,
        )
        .bind(bool_to_i64(previous.ready))
        .bind(&previous.local_path)
        .bind(previous.file_size_bytes)
        .bind(previous.rule_count)
        .bind(&previous.content_hash)
        .bind(&previous.format)
        .bind(&previous.last_update_at)
        .bind(&previous.last_error)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "ruleset_not_found",
                format!("rule set {id} not found"),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_rule_set_refresh_error(
        &self,
        id: &str,
        message: &str,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE rule_sets SET last_error = ?, updated_at = ? WHERE id = ?")
            .bind(message)
            .bind(now_iso())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_rule_set_last_route(&self, id: &str, route: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE rule_sets SET last_route = ?, updated_at = ? WHERE id = ?")
            .bind(route)
            .bind(now_iso())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn append_log(
        &self,
        level: &str,
        payload: &str,
        parsed_host: Option<&str>,
    ) -> Result<(), AppError> {
        let now = now_iso();
        sqlx::query(
            "INSERT INTO log_entries(time, level, payload, parsed_host, created_at) VALUES(?, ?, ?, ?, ?)",
        )
        .bind(&now)
        .bind(level)
        .bind(payload)
        .bind(parsed_host)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "DELETE FROM log_entries WHERE id NOT IN (SELECT id FROM log_entries ORDER BY id DESC LIMIT 10000)",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_logs(
        &self,
        level: Option<&str>,
        search: Option<&str>,
        limit: i64,
    ) -> Result<Vec<LogEntryResponse>, AppError> {
        let level = level.unwrap_or("all");
        let search = search.unwrap_or("");
        let rows = sqlx::query(
            r#"
SELECT time, level, payload, parsed_host
FROM (
  SELECT id, time, level, payload, parsed_host
  FROM log_entries
  WHERE (? = 'all' OR level = ?)
    AND payload LIKE '%' || ? || '%'
  ORDER BY id DESC
  LIMIT ?
) AS recent_logs
ORDER BY id ASC
"#,
        )
        .bind(level)
        .bind(level)
        .bind(search)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let time: String = row.try_get("time")?;
                Ok(LogEntryResponse {
                    time: display_log_time(&time),
                    level: row.try_get("level")?,
                    payload: row.try_get("payload")?,
                    parsed_host: row.try_get("parsed_host")?,
                })
            })
            .collect()
    }

    pub async fn clear_logs(&self) -> Result<(), AppError> {
        sqlx::query("DELETE FROM log_entries")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn log_export_text(&self) -> Result<String, AppError> {
        let rows = self.list_logs(Some("all"), None, 100_000).await?;
        let mut output = String::new();
        for row in rows {
            output.push_str(&format!("[{}] {} {}\n", row.time, row.level, row.payload));
        }
        Ok(output)
    }

    async fn list_subscription_rules(&self, id: &str) -> Result<Vec<FilterRule>, AppError> {
        let rows = sqlx::query(
            "SELECT id, action, match_type, pattern, values_json, enabled FROM subscription_rules WHERE subscription_id = ? ORDER BY position",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(filter_rule_from_row)
            .collect::<Result<Vec<_>, _>>()
    }

    async fn subscription_breakdown(&self, id: &str) -> Result<Map<String, Value>, AppError> {
        let rows = sqlx::query(
            "SELECT COALESCE(protocol, 'unknown') AS protocol, COUNT(*) AS count FROM proxy_items WHERE subscription_id = ? AND kind = 'node' AND filtered_out = 0 GROUP BY protocol",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let mut map = Map::new();
        for row in rows {
            let protocol: String = row.try_get("protocol")?;
            let count: i64 = row.try_get("count")?;
            map.insert(protocol, Value::from(count));
        }
        Ok(map)
    }
}

#[cfg(unix)]
fn prepare_private_database_file(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(crate::paths::PRIVATE_FILE_MODE)
        .open(path)?;
    restrict_sensitive_file_permissions(path)
}

fn restrict_sqlite_file_permissions(database: &std::path::Path) -> std::io::Result<()> {
    restrict_sensitive_file_permissions(database)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        restrict_sensitive_file_permissions(&sqlite_companion_path(database, suffix))?;
    }
    Ok(())
}

async fn migrate_subscription_asset_references(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    subscription_id: &str,
    items: &[ProxyItemRecord],
    group_members: &[(String, Vec<String>)],
    now: &str,
) -> Result<SubscriptionAssetMigration, AppError> {
    let old_rows = sqlx::query(
        r#"
SELECT name, kind, display_name, source_name, strategy_json
FROM proxy_items
WHERE subscription_id = ?
ORDER BY kind, position, name
"#,
    )
    .bind(subscription_id)
    .fetch_all(&mut **tx)
    .await?;
    if old_rows.is_empty() {
        return Ok(SubscriptionAssetMigration::default());
    }

    let mut new_names = HashMap::<(String, String), VecDeque<String>>::new();
    for item in items {
        new_names
            .entry((item.kind.clone(), item.display_name.clone()))
            .or_default()
            .push_back(item.name.clone());
    }

    let mut all_name_migrations = HashMap::new();
    let mut pending_group_selections = Vec::new();
    for row in old_rows {
        let old_name: String = row.try_get("name")?;
        let kind: String = row.try_get("kind")?;
        let display_name: String = row.try_get("display_name")?;
        let source_name: Option<String> = row.try_get("source_name")?;
        let direct_key = (kind.clone(), display_name);
        let key = if new_names.contains_key(&direct_key) {
            direct_key
        } else if let Some(legacy_display_name) =
            legacy_asset_display_name(&old_name, source_name.as_deref(), subscription_id)
        {
            (kind.clone(), legacy_display_name)
        } else {
            continue;
        };
        let Some(new_name) = new_names.get_mut(&key).and_then(VecDeque::pop_front) else {
            continue;
        };
        if kind == "group" {
            let strategy_json: String = row.try_get("strategy_json")?;
            if let Some(selected) =
                serde_json::from_str::<Value>(&strategy_json)
                    .ok()
                    .and_then(|strategy| {
                        strategy
                            .get("now")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
            {
                pending_group_selections.push((new_name.clone(), selected));
            }
        }
        all_name_migrations.insert(old_name, new_name);
    }
    let migrations = all_name_migrations
        .iter()
        .filter(|(old_name, new_name)| old_name != new_name)
        .map(|(old_name, new_name)| (old_name.clone(), new_name.clone()))
        .collect::<HashMap<_, _>>();

    let migration_batch = new_id("asset_ref_migration");
    let staged_migrations = migrations
        .iter()
        .enumerate()
        .map(|(index, (old_name, new_name))| {
            (
                old_name.clone(),
                new_name.clone(),
                format!("{migration_batch}_{index}"),
            )
        })
        .collect::<Vec<_>>();

    for (old_name, _, temporary_name) in &staged_migrations {
        sqlx::query("UPDATE routing_rules SET policy = ?, updated_at = ? WHERE policy = ?")
            .bind(temporary_name)
            .bind(now)
            .bind(old_name)
            .execute(&mut **tx)
            .await?;
        sqlx::query(
            r#"
INSERT OR IGNORE INTO proxy_group_members(group_name, member_name, position, created_at)
SELECT group_name, ?, position, created_at
FROM proxy_group_members
WHERE member_name = ?
"#,
        )
        .bind(temporary_name)
        .bind(old_name)
        .execute(&mut **tx)
        .await?;
        sqlx::query("DELETE FROM proxy_group_members WHERE member_name = ?")
            .bind(old_name)
            .execute(&mut **tx)
            .await?;
    }
    for (_, new_name, temporary_name) in &staged_migrations {
        sqlx::query("UPDATE routing_rules SET policy = ?, updated_at = ? WHERE policy = ?")
            .bind(new_name)
            .bind(now)
            .bind(temporary_name)
            .execute(&mut **tx)
            .await?;
        sqlx::query(
            r#"
INSERT OR IGNORE INTO proxy_group_members(group_name, member_name, position, created_at)
SELECT group_name, ?, position, created_at
FROM proxy_group_members
WHERE member_name = ?
"#,
        )
        .bind(new_name)
        .bind(temporary_name)
        .execute(&mut **tx)
        .await?;
        sqlx::query("DELETE FROM proxy_group_members WHERE member_name = ?")
            .bind(temporary_name)
            .execute(&mut **tx)
            .await?;
    }

    let strategy_rows =
        sqlx::query("SELECT name, strategy_json FROM proxy_items WHERE kind = 'group'")
            .fetch_all(&mut **tx)
            .await?;
    for row in strategy_rows {
        let group_name: String = row.try_get("name")?;
        let strategy_json: String = row.try_get("strategy_json")?;
        let Ok(mut strategy) = serde_json::from_str::<Value>(&strategy_json) else {
            continue;
        };
        let replacement = strategy
            .get("now")
            .and_then(Value::as_str)
            .and_then(|selected| migrations.get(selected))
            .cloned();
        let Some(replacement) = replacement else {
            continue;
        };
        let Some(strategy) = strategy.as_object_mut() else {
            continue;
        };
        strategy.insert("now".into(), Value::String(replacement));
        sqlx::query("UPDATE proxy_items SET strategy_json = ?, updated_at = ? WHERE name = ?")
            .bind(serde_json::to_string(strategy)?)
            .bind(now)
            .bind(group_name)
            .execute(&mut **tx)
            .await?;
    }

    let filter_rows = sqlx::query(
        "SELECT id, value, values_json FROM proxy_group_filters WHERE field = 'name' AND operator IN ('equals', 'in')",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in filter_rows {
        let filter_id: String = row.try_get("id")?;
        let mut value: String = row.try_get("value")?;
        let values_json: String = row.try_get("values_json")?;
        let mut changed = false;
        if let Some(replacement) = migrations.get(&value) {
            value = replacement.clone();
            changed = true;
        }
        let rewritten_values_json = match serde_json::from_str::<Vec<String>>(&values_json) {
            Ok(mut values) => {
                for item in &mut values {
                    if let Some(replacement) = migrations.get(item) {
                        *item = replacement.clone();
                        changed = true;
                    }
                }
                serde_json::to_string(&values)?
            }
            Err(_) => values_json,
        };
        if !changed {
            continue;
        }
        sqlx::query(
            "UPDATE proxy_group_filters SET value = ?, values_json = ?, updated_at = ? WHERE id = ?",
        )
        .bind(value)
        .bind(rewritten_values_json)
        .bind(now)
        .bind(filter_id)
        .execute(&mut **tx)
        .await?;
    }

    let candidate_members = group_members
        .iter()
        .map(|(group_name, members)| {
            (
                group_name.as_str(),
                members.iter().map(String::as_str).collect::<HashSet<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let group_selections = pending_group_selections
        .into_iter()
        .filter_map(|(group_name, selected)| {
            let selected = all_name_migrations
                .get(&selected)
                .cloned()
                .unwrap_or(selected);
            candidate_members
                .get(group_name.as_str())
                .is_some_and(|members| members.contains(selected.as_str()))
                .then_some((group_name, selected))
        })
        .collect();
    Ok(SubscriptionAssetMigration {
        reference_count: migrations.len(),
        group_selections,
        name_migrations: all_name_migrations,
    })
}

#[derive(Default)]
struct SubscriptionAssetMigration {
    reference_count: usize,
    group_selections: HashMap<String, String>,
    name_migrations: HashMap<String, String>,
}

fn legacy_asset_display_name(
    runtime_name: &str,
    source_name: Option<&str>,
    subscription_id: &str,
) -> Option<String> {
    let source_name = source_name?.trim();
    let mut base = runtime_name;
    if let Some((prefix, duplicate_index)) = base.rsplit_once(SUB_DELIMITER) {
        if duplicate_index
            .parse::<usize>()
            .is_ok_and(|index| index >= 2)
        {
            base = prefix;
        }
    }
    let legacy_suffix = format!(
        "{SUB_DELIMITER}{source_name}{SUB_DELIMITER}{}",
        subscription_id.trim()
    );
    base.strip_suffix(&legacy_suffix).map(str::to_string)
}

async fn upsert_proxy_item_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    item: &ProxyItemRecord,
    now: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
INSERT INTO proxy_items(
  name, kind, subscription_id, display_name, source, builtin, source_name, protocol, country, group_type,
  raw_json, content_hash, latency_ms, alive, filtered_out, filter_reason, delay_ms,
  tolerance_ms, url, interval_seconds, strategy_json, position, enabled, created_at, updated_at
) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(name) DO UPDATE SET
  kind = excluded.kind,
  subscription_id = excluded.subscription_id,
  display_name = excluded.display_name,
  source = excluded.source,
  builtin = excluded.builtin,
  source_name = excluded.source_name,
  protocol = excluded.protocol,
  country = excluded.country,
  group_type = excluded.group_type,
  raw_json = excluded.raw_json,
  content_hash = excluded.content_hash,
  alive = excluded.alive,
  filtered_out = excluded.filtered_out,
  filter_reason = excluded.filter_reason,
  delay_ms = excluded.delay_ms,
  tolerance_ms = excluded.tolerance_ms,
  url = excluded.url,
  interval_seconds = excluded.interval_seconds,
  strategy_json = excluded.strategy_json,
  position = excluded.position,
  enabled = excluded.enabled,
  updated_at = excluded.updated_at
WHERE proxy_items.builtin = 0 OR excluded.builtin = 1
"#,
    )
    .bind(&item.name)
    .bind(&item.kind)
    .bind(&item.subscription_id)
    .bind(&item.display_name)
    .bind(&item.source)
    .bind(bool_to_i64(item.builtin))
    .bind(&item.source_name)
    .bind(&item.protocol)
    .bind(&item.country)
    .bind(&item.group_type)
    .bind(&item.raw_json)
    .bind(&item.content_hash)
    .bind(item.latency_ms)
    .bind(bool_to_i64(item.alive))
    .bind(bool_to_i64(item.filtered_out))
    .bind(&item.filter_reason)
    .bind(item.delay_ms)
    .bind(item.tolerance_ms)
    .bind(&item.url)
    .bind(item.interval_seconds)
    .bind(&item.strategy_json)
    .bind(item.position)
    .bind(bool_to_i64(item.enabled))
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn valid_node_records_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<Vec<ProxyNodeResponse>, AppError> {
    let rows = sqlx::query(
        r#"
SELECT name, display_name, protocol, latency_ms, country, subscription_id, source_name
FROM proxy_items
WHERE kind = 'node' AND filtered_out = 0 AND enabled = 1
  AND (
    subscription_id IS NULL
    OR EXISTS (
      SELECT 1 FROM subscriptions
      WHERE subscriptions.id = proxy_items.subscription_id
        AND subscriptions.ready = 1
    )
  )
ORDER BY subscription_id, display_name
"#,
    )
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ProxyNodeResponse {
                name: row.try_get("name")?,
                display_name: row.try_get("display_name")?,
                protocol: row
                    .try_get::<Option<String>, _>("protocol")?
                    .unwrap_or_else(|| "unknown".into()),
                latency: row.try_get::<Option<i64>, _>("latency_ms")?.unwrap_or(0),
                country: row.try_get("country")?,
                subscription_id: row.try_get("subscription_id")?,
                subscription_name: row.try_get("source_name")?,
            })
        })
        .collect()
}

async fn group_filters_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    group_name: &str,
) -> Result<Vec<GroupFilterInput>, AppError> {
    let rows = sqlx::query(
        "SELECT id, action, field, operator, value, values_json, enabled FROM proxy_group_filters WHERE group_name = ? ORDER BY position",
    )
    .bind(group_name)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter().map(group_filter_from_row).collect()
}

fn group_filter_from_row(row: sqlx::sqlite::SqliteRow) -> Result<GroupFilterInput, AppError> {
    let operator: String = row.try_get("operator")?;
    let mut value: String = row.try_get("value")?;
    let values_json: String = row.try_get("values_json")?;
    let mut values: Vec<String> = serde_json::from_str(&values_json).unwrap_or_default();
    if !matches!(operator.as_str(), "in" | "equals") {
        values.clear();
    }
    if values.is_empty() && operator == "in" && !value.trim().is_empty() {
        values = value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }
    if operator == "in" && !values.is_empty() {
        value.clear();
    }
    Ok(GroupFilterInput {
        id: Some(row.try_get("id")?),
        action: row.try_get("action")?,
        field: row.try_get("field")?,
        operator,
        value,
        values,
        enabled: Some(i64_to_bool(row.try_get("enabled")?)),
    })
}

async fn current_group_now_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    group_name: &str,
) -> Result<Option<String>, AppError> {
    let strategy = sqlx::query_scalar::<_, String>(
        "SELECT strategy_json FROM proxy_items WHERE name = ? AND kind = 'group'",
    )
    .bind(group_name)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(strategy.and_then(|strategy| {
        serde_json::from_str::<Value>(&strategy)
            .ok()
            .and_then(|value| value.get("now").and_then(Value::as_str).map(str::to_string))
    }))
}

async fn validate_select_group_member_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    group_name: &str,
    member_name: &str,
) -> Result<(), AppError> {
    let group_type = sqlx::query_scalar::<_, Option<String>>(
        "SELECT group_type FROM proxy_items WHERE name = ? AND kind = 'group'",
    )
    .bind(group_name)
    .fetch_optional(&mut **tx)
    .await?
    .flatten()
    .ok_or_else(|| {
        AppError::not_found(
            "proxy_group_not_found",
            format!("proxy group {group_name} not found"),
        )
    })?;
    if group_type != "select" {
        return Err(AppError::bad_request(
            "proxy_group_not_selectable",
            format!("proxy group {group_name} has type {group_type} and cannot be selected"),
        ));
    }
    let member_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM proxy_group_members WHERE group_name = ? AND member_name = ?)",
    )
    .bind(group_name)
    .bind(member_name)
    .fetch_one(&mut **tx)
    .await?;
    if !member_exists {
        return Err(AppError::bad_request(
            "proxy_group_member_not_found",
            format!("proxy {member_name} is not a member of group {group_name}"),
        ));
    }
    Ok(())
}

async fn replace_group_members_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    group_name: &str,
    members: &[String],
    preferred_selection: Option<String>,
    now: &str,
) -> Result<(), AppError> {
    let selected = match preferred_selection {
        Some(selected) => Some(selected),
        None => current_group_now_in_transaction(tx, group_name).await?,
    }
    .filter(|selected| members.contains(selected))
    .or_else(|| members.first().cloned());
    let strategy = selected
        .map(|name| serde_json::json!({ "now": name }).to_string())
        .unwrap_or_else(|| "{}".to_string());

    sqlx::query("DELETE FROM proxy_group_members WHERE group_name = ?")
        .bind(group_name)
        .execute(&mut **tx)
        .await?;
    for (index, member) in members.iter().enumerate() {
        sqlx::query(
            "INSERT OR IGNORE INTO proxy_group_members(group_name, member_name, position, created_at) VALUES(?, ?, ?, ?)",
        )
        .bind(group_name)
        .bind(member)
        .bind(((index + 1) as i64) * 1024)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "UPDATE proxy_items SET strategy_json = ?, updated_at = ? WHERE name = ? AND kind = 'group'",
    )
    .bind(strategy)
    .bind(now)
    .bind(group_name)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn replace_group_filters_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    group_name: &str,
    filters: &[GroupFilterInput],
    now: &str,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM proxy_group_filters WHERE group_name = ?")
        .bind(group_name)
        .execute(&mut **tx)
        .await?;
    for (index, filter) in filters.iter().enumerate() {
        let operator = filter.operator.trim();
        let values = if operator == "in" || (operator == "equals" && filter.has_values()) {
            filter
                .effective_values()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let values_json = serde_json::to_string(&values)?;
        sqlx::query(
            r#"
INSERT INTO proxy_group_filters(id, group_name, position, action, field, operator, value, values_json, enabled, created_at, updated_at)
VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#,
        )
        .bind(filter.id.clone().unwrap_or_else(|| new_id("pgf")))
        .bind(group_name)
        .bind(((index + 1) as i64) * 1024)
        .bind(filter.action.trim())
        .bind(filter.field.trim())
        .bind(operator)
        .bind(filter.value.trim())
        .bind(values_json)
        .bind(bool_to_i64(filter.enabled.unwrap_or(true)))
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn validate_rule_target_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    rule_type: &str,
    value: &str,
    policy: &str,
) -> Result<(), AppError> {
    if rule_type == "RULE-SET" {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM rule_sets WHERE name = ? AND ready = 1)",
        )
        .bind(value)
        .fetch_one(&mut **tx)
        .await?;
        if !exists {
            return Err(AppError::bad_request(
                "rule_invalid_ruleset",
                format!("rule set {value} does not exist"),
            ));
        }
    }
    if !is_builtin_policy(policy)
        && !available_policy_targets_in_transaction(tx)
            .await?
            .contains(policy)
    {
        return Err(AppError::bad_request(
            "rule_invalid_policy",
            format!("rule policy {policy} is not available"),
        ));
    }
    Ok(())
}

async fn validate_single_enabled_match_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source: &str,
    current_id: Option<&str>,
    rule_type: &str,
    enabled: bool,
) -> Result<(), AppError> {
    if rule_type != "MATCH" || !enabled {
        return Ok(());
    }
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id FROM routing_rules WHERE source = ? AND rule_type = 'MATCH' AND enabled = 1 AND (? IS NULL OR id <> ?) LIMIT 1",
    )
    .bind(source)
    .bind(current_id)
    .bind(current_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(existing) = existing {
        return Err(AppError::conflict(
            "rule_match_exists",
            format!("rule source {source} already has enabled MATCH rule {existing}"),
        ));
    }
    Ok(())
}

async fn available_policy_targets_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<HashSet<String>, AppError> {
    let items = sqlx::query(
        r#"
SELECT name, kind, subscription_id, display_name, source, builtin, source_name, protocol, country,
       group_type, raw_json, content_hash, latency_ms, alive, filtered_out, filter_reason,
       delay_ms, tolerance_ms, url, interval_seconds, strategy_json, position, enabled
FROM proxy_items
WHERE enabled = 1
  AND (
    subscription_id IS NULL
    OR EXISTS (
      SELECT 1 FROM subscriptions
      WHERE subscriptions.id = proxy_items.subscription_id
        AND subscriptions.ready = 1
    )
  )
ORDER BY kind, position, created_at
"#,
    )
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(proxy_item_from_row)
    .collect::<Result<Vec<_>, _>>()?;
    let member_rows = sqlx::query(
        "SELECT group_name, member_name FROM proxy_group_members ORDER BY group_name, position, member_name",
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut member_map = HashMap::<String, Vec<String>>::new();
    for row in member_rows {
        member_map
            .entry(row.try_get("group_name")?)
            .or_default()
            .push(row.try_get("member_name")?);
    }
    let nodes = items
        .iter()
        .filter(|item| item.kind == "node" && !item.filtered_out)
        .map(|item| ProxyNodeResponse {
            name: item.name.clone(),
            display_name: item.display_name.clone(),
            protocol: item.protocol.clone().unwrap_or_else(|| "unknown".into()),
            latency: item.latency_ms.unwrap_or(0),
            country: item.country.clone(),
            subscription_id: item.subscription_id.clone(),
            subscription_name: item.source_name.clone(),
        })
        .collect::<Vec<_>>();
    let custom_group_names = items
        .iter()
        .filter(|item| item.kind == "group" && item.source == "custom" && !item.filtered_out)
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();
    for group_name in custom_group_names {
        let filters = group_filters_in_transaction(tx, &group_name).await?;
        member_map.insert(
            group_name,
            crate::proxy::calculate_members(&nodes, &filters),
        );
    }
    crate::runtime::available_policy_targets_from_assets(&items, &member_map)
}

async fn validate_no_referenced_policy_became_unavailable(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    available_before: &HashSet<String>,
) -> Result<(), AppError> {
    let available_after = available_policy_targets_in_transaction(tx).await?;
    let referenced_policies =
        sqlx::query_scalar::<_, String>("SELECT DISTINCT policy FROM routing_rules")
            .fetch_all(&mut **tx)
            .await?;
    if let Some(policy) = referenced_policies
        .into_iter()
        .find(|policy| available_before.contains(policy) && !available_after.contains(policy))
    {
        return Err(AppError::conflict(
            "subscription_asset_referenced",
            format!("subscription mutation would make referenced policy {policy} unavailable"),
        ));
    }
    Ok(())
}

fn is_builtin_policy(value: &str) -> bool {
    matches!(
        value,
        BUILTIN_DIRECT | BUILTIN_REJECT | BUILTIN_GLOBAL | BUILTIN_PROXY
    )
}

fn download_route_from_str(value: &str) -> DownloadRoute {
    match value.trim().to_ascii_lowercase().as_str() {
        "direct" => DownloadRoute::Direct,
        "core" => DownloadRoute::Core,
        "system" => DownloadRoute::System,
        _ => DownloadRoute::Auto,
    }
}

fn validate_manual_record(item: &ProxyItemRecord) -> Result<(), AppError> {
    if item.kind != "node"
        || item.source != "manual"
        || item.subscription_id.is_some()
        || item.raw_json.is_none()
    {
        return Err(AppError::internal("invalid manual proxy item record"));
    }
    Ok(())
}

async fn attached_table_columns(
    connection: &mut sqlx::pool::PoolConnection<Sqlite>,
    schema: &str,
    table: &str,
) -> Result<HashSet<String>, AppError> {
    let rows = sqlx::query(&format!("PRAGMA {schema}.table_info({table})"))
        .fetch_all(&mut **connection)
        .await?;
    rows.into_iter()
        .map(|row| row.try_get("name").map_err(AppError::from))
        .collect()
}

async fn move_rule_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    target_position: Option<usize>,
    now: &str,
) -> Result<(), AppError> {
    let source = sqlx::query_scalar::<_, String>("SELECT source FROM routing_rules WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found("rule_not_found", format!("rule {id} not found")))?;
    let rows = sqlx::query(
        "SELECT id, rule_type FROM routing_rules WHERE source = ? ORDER BY position, created_at, id",
    )
    .bind(&source)
    .fetch_all(&mut **tx)
    .await?;
    let mut ids = rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("id"),
                row.get::<String, _>("rule_type"),
            )
        })
        .collect::<Vec<_>>();
    let current = ids
        .iter()
        .position(|(rule_id, _)| rule_id == id)
        .ok_or_else(|| AppError::not_found("rule_not_found", format!("rule {id} not found")))?;
    let item = ids.remove(current);
    let target = if item.1 == "MATCH" {
        ids.len()
    } else {
        let max_target = ids
            .iter()
            .position(|(_, kind)| kind == "MATCH")
            .unwrap_or(ids.len());
        target_position
            .map(|position| position.saturating_sub(1))
            .unwrap_or(current)
            .min(max_target)
    };
    ids.insert(target, item);
    for (index, (rule_id, _)) in ids.iter().enumerate() {
        sqlx::query("UPDATE routing_rules SET position = ?, updated_at = ? WHERE id = ?")
            .bind(((index + 1) as i64) * 1024)
            .bind(now)
            .bind(rule_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn rule_by_id_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
) -> Result<RuleResponse, AppError> {
    let row = sqlx::query(
        "SELECT id, position, rule_type, value, policy, source, enabled, desc FROM routing_rules WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::not_found("rule_not_found", format!("rule {id} not found")))?;
    rule_from_row(row)
}

fn merge_default_config(mut map: Map<String, Value>) -> Value {
    let defaults =
        serde_json::to_value(SystemConfig::default()).unwrap_or(Value::Object(Map::new()));
    if let Some(defaults) = defaults.as_object() {
        for (key, value) in defaults {
            map.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    Value::Object(map)
}

fn filter_rule_from_row(row: sqlx::sqlite::SqliteRow) -> Result<FilterRule, AppError> {
    let match_type: String = row.try_get("match_type")?;
    let mut pattern: String = row.try_get("pattern")?;
    let values_json: String = row.try_get("values_json")?;
    let mut values: Vec<String> = serde_json::from_str(&values_json).unwrap_or_default();
    if !matches!(match_type.as_str(), "in" | "equals") {
        values.clear();
    }
    if values.is_empty() && match_type == "in" && !pattern.trim().is_empty() {
        values = pattern
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }
    if match_type == "in" && !values.is_empty() {
        pattern.clear();
    }
    Ok(FilterRule {
        id: row.try_get("id")?,
        action: row.try_get("action")?,
        match_type,
        pattern,
        values,
        enabled: i64_to_bool(row.try_get("enabled")?),
    })
}

async fn insert_subscription_rules(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    subscription_id: &str,
    rules: &[FilterRuleInput],
    now: &str,
) -> Result<(), AppError> {
    for (index, rule) in rules.iter().enumerate() {
        let values = if matches!(rule.match_type.trim(), "in" | "equals") {
            rule.effective_values()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let values_json = serde_json::to_string(&values)?;
        sqlx::query(
            r#"
INSERT INTO subscription_rules(id, subscription_id, position, action, match_type, pattern, values_json, enabled, created_at, updated_at)
VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#,
        )
        .bind(rule.id.clone().unwrap_or_else(|| new_id("sfr")))
        .bind(subscription_id)
        .bind(((index + 1) as i64) * 1024)
        .bind(rule.action.trim())
        .bind(rule.match_type.trim())
        .bind(if rule.match_type.trim() == "in" {
            ""
        } else {
            rule.pattern.trim()
        })
        .bind(values_json)
        .bind(bool_to_i64(rule.enabled.unwrap_or(true)))
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn proxy_item_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ProxyItemRecord, AppError> {
    Ok(ProxyItemRecord {
        name: row.try_get("name")?,
        kind: row.try_get("kind")?,
        subscription_id: row.try_get("subscription_id")?,
        display_name: row.try_get("display_name")?,
        source: row.try_get("source")?,
        builtin: i64_to_bool(row.try_get("builtin")?),
        source_name: row.try_get("source_name")?,
        protocol: row.try_get("protocol")?,
        country: row.try_get("country")?,
        group_type: row.try_get("group_type")?,
        raw_json: row.try_get("raw_json")?,
        content_hash: row.try_get("content_hash")?,
        latency_ms: row.try_get("latency_ms")?,
        alive: i64_to_bool(row.try_get("alive")?),
        filtered_out: i64_to_bool(row.try_get("filtered_out")?),
        filter_reason: row.try_get("filter_reason")?,
        delay_ms: row.try_get("delay_ms")?,
        tolerance_ms: row.try_get("tolerance_ms")?,
        url: row.try_get("url")?,
        interval_seconds: row.try_get("interval_seconds")?,
        strategy_json: row.try_get("strategy_json")?,
        position: row.try_get("position")?,
        enabled: i64_to_bool(row.try_get("enabled")?),
    })
}

fn sqlite_i64(value: u64, field: &str) -> Result<i64, AppError> {
    i64::try_from(value).map_err(|_| {
        AppError::bad_request(
            "numeric_value_out_of_range",
            format!("{field} exceeds the supported maximum of {}", i64::MAX),
        )
    })
}

fn optional_sqlite_i64(value: Option<u64>, field: &str) -> Result<Option<i64>, AppError> {
    value.map(|value| sqlite_i64(value, field)).transpose()
}

fn rule_from_row(row: sqlx::sqlite::SqliteRow) -> Result<RuleResponse, AppError> {
    Ok(RuleResponse {
        id: row.try_get("id")?,
        rule_type: row.try_get("rule_type")?,
        value: row.try_get("value")?,
        policy: row.try_get("policy")?,
        position: row.try_get("position")?,
        source: row.try_get("source")?,
        enabled: i64_to_bool(row.try_get("enabled")?),
        desc: row.try_get("desc")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn sqlite_database_and_sidecars_use_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TestDir::new("sqlite-private-permissions");
        let paths = AppPaths::from_root(temp.path());
        std::fs::create_dir_all(&paths.data_dir).expect("create data directory");
        std::fs::write(&paths.database_file, b"").expect("create existing database file");
        std::fs::set_permissions(&paths.database_file, std::fs::Permissions::from_mode(0o644))
            .expect("set permissive database mode");

        let storage = Storage::connect(&paths).await.expect("connect storage");

        for path in [
            paths.database_file.clone(),
            sqlite_companion_path(&paths.database_file, "-wal"),
            sqlite_companion_path(&paths.database_file, "-shm"),
        ] {
            assert!(path.is_file(), "{} was not created", path.display());
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600,
                "{}",
                path.display()
            );
        }

        storage.pool.close().await;
    }

    #[tokio::test]
    async fn user_rules_have_an_independent_order_and_stay_before_builtin_rules() {
        let temp = TestDir::new("rule-order");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let before = storage.list_rules().await.expect("list seeded rules");
        let before_system = before
            .iter()
            .map(|rule| (rule.id.clone(), rule.position))
            .collect::<Vec<_>>();

        let first = storage
            .upsert_rule(
                Some("rule_user_first".into()),
                "DOMAIN",
                "first.example",
                "PROXY",
                None,
                true,
            )
            .await
            .expect("insert first user rule");
        let user_match = storage
            .upsert_rule(
                Some("rule_user_match".into()),
                "MATCH",
                "ANY",
                "DIRECT",
                None,
                true,
            )
            .await
            .expect("insert user MATCH rule");
        let second = storage
            .upsert_rule(
                Some("rule_user_second".into()),
                "DOMAIN",
                "second.example",
                "PROXY",
                None,
                true,
            )
            .await
            .expect("insert second user rule");

        let after = storage.list_rules().await.expect("list updated rules");
        let user_rules = after
            .iter()
            .filter(|rule| rule.source == "user")
            .collect::<Vec<_>>();
        assert_eq!(
            user_rules
                .iter()
                .map(|rule| rule.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                first.id.as_str(),
                second.id.as_str(),
                user_match.id.as_str()
            ]
        );
        assert!(user_rules
            .windows(2)
            .all(|pair| pair[0].position < pair[1].position));
        assert_eq!(
            user_rules.last().map(|rule| rule.rule_type.as_str()),
            Some("MATCH")
        );

        let after_system = after
            .iter()
            .filter(|rule| rule.source != "user")
            .map(|rule| (rule.id.clone(), rule.position))
            .collect::<Vec<_>>();
        assert_eq!(after_system, before_system);
        assert!(after
            .iter()
            .take(user_rules.len())
            .all(|rule| rule.source == "user"));
    }

    #[tokio::test]
    async fn reconnect_normalizes_legacy_duplicate_match_rules_and_enforces_uniqueness() {
        let temp = TestDir::new("normalize-legacy-match-rules");
        let paths = AppPaths::from_root(temp.path());
        let storage = Storage::connect(&paths)
            .await
            .expect("connect initial storage");
        sqlx::query("DROP INDEX idx_routing_rules_one_enabled_match_per_source")
            .execute(&storage.pool)
            .await
            .expect("simulate a database created before the unique index");
        let now = now_iso();
        for (id, position, rule_type) in [
            ("legacy_match_first", 1024_i64, "MATCH"),
            ("legacy_normal", 2048_i64, "DOMAIN"),
            ("legacy_match_second", 3072_i64, "MATCH"),
        ] {
            sqlx::query(
                r#"
INSERT INTO routing_rules(
  id, position, rule_type, value, policy, source, enabled, created_at, updated_at
) VALUES(?, ?, ?, ?, 'DIRECT', 'legacy', 1, ?, ?)
"#,
            )
            .bind(id)
            .bind(position)
            .bind(rule_type)
            .bind(if rule_type == "MATCH" {
                "ANY"
            } else {
                "legacy.example"
            })
            .bind(&now)
            .bind(&now)
            .execute(&storage.pool)
            .await
            .expect("insert legacy rule");
        }
        storage.pool.close().await;

        let reopened = Storage::connect(&paths)
            .await
            .expect("reconnect and normalize storage");
        let legacy = reopened
            .list_rules()
            .await
            .expect("list normalized rules")
            .into_iter()
            .filter(|rule| rule.source == "legacy")
            .collect::<Vec<_>>();
        assert_eq!(
            legacy
                .iter()
                .map(|rule| (rule.id.as_str(), rule.enabled))
                .collect::<Vec<_>>(),
            vec![
                ("legacy_normal", true),
                ("legacy_match_first", true),
                ("legacy_match_second", false),
            ]
        );
        assert!(legacy
            .windows(2)
            .all(|pair| pair[0].position < pair[1].position));

        let duplicate = sqlx::query(
            r#"
INSERT INTO routing_rules(
  id, position, rule_type, value, policy, source, enabled, created_at, updated_at
) VALUES('legacy_match_third', 4096, 'MATCH', 'ANY', 'DIRECT', 'legacy', 1, ?, ?)
"#,
        )
        .bind(&now)
        .bind(&now)
        .execute(&reopened.pool)
        .await;
        assert!(
            duplicate.is_err(),
            "the partial unique index must reject a second enabled MATCH"
        );
    }

    #[tokio::test]
    async fn moving_rules_uses_one_based_positions_and_keeps_match_last() {
        let temp = TestDir::new("move-rule");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        for (id, kind) in [("one", "DOMAIN"), ("two", "DOMAIN"), ("match", "MATCH")] {
            storage
                .upsert_rule(Some(id.into()), kind, id, "DIRECT", None, true)
                .await
                .expect("insert rule");
        }

        storage.move_rule("two", 1).await.expect("move to top");
        storage
            .move_rule("match", 1)
            .await
            .expect("keep match last");
        let ids = storage
            .list_rules()
            .await
            .expect("list rules")
            .into_iter()
            .filter(|rule| rule.source == "user")
            .map(|rule| rule.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["two", "one", "match"]);
    }

    #[tokio::test]
    async fn rule_updates_apply_fields_and_order_together_and_keep_match_last() {
        let temp = TestDir::new("atomic-rule-update-order");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        for id in ["one", "two", "three"] {
            storage
                .upsert_rule(
                    Some(format!("rule_{id}")),
                    "DOMAIN",
                    &format!("{id}.example.com"),
                    "DIRECT",
                    None,
                    true,
                )
                .await
                .expect("insert user rule");
        }

        let updated = storage
            .update_rule(
                "rule_three",
                "DOMAIN-SUFFIX",
                "updated.example.com",
                "PROXY",
                Some("updated fields and order"),
                false,
                Some(1),
            )
            .await
            .expect("update fields and move in one transaction");
        assert_eq!(updated.rule_type, "DOMAIN-SUFFIX");
        assert_eq!(updated.value, "updated.example.com");
        assert_eq!(updated.policy, "PROXY");
        assert_eq!(updated.desc.as_deref(), Some("updated fields and order"));
        assert!(!updated.enabled);
        assert_eq!(updated.position, 1024);

        let user_rules = storage
            .list_rules()
            .await
            .expect("list updated rules")
            .into_iter()
            .filter(|rule| rule.source == "user")
            .collect::<Vec<_>>();
        assert_eq!(
            user_rules
                .iter()
                .map(|rule| rule.id.as_str())
                .collect::<Vec<_>>(),
            vec!["rule_three", "rule_one", "rule_two"]
        );
        assert_eq!(user_rules[0].rule_type, "DOMAIN-SUFFIX");
        assert_eq!(user_rules[0].value, "updated.example.com");

        storage
            .update_rule("rule_one", "MATCH", "ANY", "DIRECT", None, true, None)
            .await
            .expect("change an existing rule to MATCH without a position");
        let after_update = storage
            .list_rules()
            .await
            .expect("list rules after MATCH update")
            .into_iter()
            .filter(|rule| rule.source == "user")
            .map(|rule| rule.id)
            .collect::<Vec<_>>();
        assert_eq!(after_update, vec!["rule_three", "rule_two", "rule_one"]);

        let duplicate_match = storage
            .upsert_rule(
                Some("rule_two".into()),
                "MATCH",
                "ANY",
                "DIRECT",
                None,
                true,
            )
            .await
            .expect_err("reject a second enabled MATCH in one source");
        assert_eq!(duplicate_match.code, "rule_match_exists");
        let after_upsert = storage
            .list_rules()
            .await
            .expect("list rules after MATCH upsert")
            .into_iter()
            .filter(|rule| rule.source == "user")
            .map(|rule| (rule.id, rule.rule_type))
            .collect::<Vec<_>>();
        assert_eq!(
            after_upsert,
            vec![
                ("rule_three".into(), "DOMAIN-SUFFIX".into()),
                ("rule_two".into(), "DOMAIN".into()),
                ("rule_one".into(), "MATCH".into()),
            ]
        );
    }

    #[tokio::test]
    async fn replacing_subscription_assets_migrates_runtime_name_references() {
        let temp = TestDir::new("asset-name-migration");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let subscription_id = "sub_migrate";
        storage
            .create_subscription(
                subscription_id,
                "Provider",
                "https://example.com/profile.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");

        let old_node = "HK 01^_^Provider^_^sub_migrate";
        let old_group = "Select^_^Provider^_^sub_migrate";
        let mut old_group_item = test_proxy_item(
            old_group,
            "group",
            Some(subscription_id),
            old_group,
            Some("Provider"),
        );
        old_group_item.strategy_json = serde_json::json!({ "now": old_node }).to_string();
        storage
            .replace_subscription_assets(
                subscription_id,
                &[
                    test_proxy_item(
                        old_node,
                        "node",
                        Some(subscription_id),
                        "HK 01",
                        Some("Provider"),
                    ),
                    old_group_item,
                ],
                &[(old_group.to_string(), vec![old_node.to_string()])],
                test_sync_commit(),
            )
            .await
            .expect("store legacy assets");

        let mut custom_group = test_proxy_item("Custom", "group", None, "Custom", None);
        custom_group.source = "custom".into();
        custom_group.strategy_json = serde_json::json!({ "now": old_node }).to_string();
        storage
            .upsert_proxy_item(&custom_group)
            .await
            .expect("create custom group");
        storage
            .replace_group_members("Custom", &[old_node.to_string(), old_group.to_string()])
            .await
            .expect("store custom group members");
        storage
            .replace_group_filters(
                "Custom",
                &[
                    GroupFilterInput {
                        operator: "in".into(),
                        values: vec![old_node.into()],
                        ..GroupFilterInput::default()
                    },
                    GroupFilterInput {
                        operator: "equals".into(),
                        value: old_group.into(),
                        ..GroupFilterInput::default()
                    },
                    GroupFilterInput {
                        field: "country".into(),
                        operator: "is".into(),
                        value: old_node.into(),
                        ..GroupFilterInput::default()
                    },
                    GroupFilterInput {
                        field: "name".into(),
                        operator: "contains".into(),
                        value: old_node.into(),
                        ..GroupFilterInput::default()
                    },
                ],
            )
            .await
            .expect("store custom group filters");
        storage
            .upsert_rule(
                Some("rule_migrated_policy".into()),
                "DOMAIN",
                "example.com",
                old_group,
                None,
                true,
            )
            .await
            .expect("store rule policy reference");

        let new_node = "HK 01^_^sub_migrate";
        let new_group = "Select^_^sub_migrate";
        let mut new_group_item = test_proxy_item(
            new_group,
            "group",
            Some(subscription_id),
            "Select",
            Some("Provider Renamed"),
        );
        new_group_item.strategy_json = serde_json::json!({ "now": new_node }).to_string();
        let mut renamed_commit = test_sync_commit();
        renamed_commit.subscription_name = "Provider Renamed".into();
        storage
            .replace_subscription_assets(
                subscription_id,
                &[
                    test_proxy_item(
                        new_node,
                        "node",
                        Some(subscription_id),
                        "HK 01",
                        Some("Provider Renamed"),
                    ),
                    new_group_item,
                ],
                &[(new_group.to_string(), vec![new_node.to_string()])],
                renamed_commit,
            )
            .await
            .expect("replace assets with stable names");

        let migrated_rule = storage
            .list_rules()
            .await
            .expect("list rules")
            .into_iter()
            .find(|rule| rule.id == "rule_migrated_policy")
            .expect("migrated rule");
        assert_eq!(migrated_rule.policy, new_group);
        assert_eq!(
            storage.group_members("Custom").await.expect("list members"),
            vec![new_node.to_string(), new_group.to_string()]
        );
        let filters = storage.group_filters("Custom").await.expect("list filters");
        assert_eq!(filters[0].values, vec![new_node.to_string()]);
        assert_eq!(filters[1].value, new_group);
        assert_eq!(filters[2].value, old_node);
        assert_eq!(filters[3].value, old_node);
        let custom_group = storage
            .proxy_items_for_runtime()
            .await
            .expect("list runtime items")
            .into_iter()
            .find(|item| item.name == "Custom")
            .expect("custom group");
        assert_eq!(
            serde_json::from_str::<Value>(&custom_group.strategy_json)
                .expect("parse strategy")
                .get("now")
                .and_then(Value::as_str),
            Some(new_node)
        );
        assert_eq!(
            storage
                .get_subscription_url(subscription_id)
                .await
                .expect("load renamed subscription")
                .0,
            "Provider Renamed"
        );
    }

    #[tokio::test]
    async fn subscription_group_refresh_preserves_a_still_available_selection() {
        let temp = TestDir::new("subscription-group-selection");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let subscription_id = "sub_selection";
        storage
            .create_subscription(
                subscription_id,
                "Provider",
                "https://example.com/profile.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        let node_a = "A^_^sub_selection";
        let node_b = "B^_^sub_selection";
        let group_name = "Select^_^sub_selection";
        let legacy_node_a = "A^_^Provider^_^sub_selection";
        let legacy_node_b = "B^_^Provider^_^sub_selection";
        let legacy_group_name = "Select^_^Provider^_^sub_selection";
        let mut legacy_group = test_proxy_item(
            legacy_group_name,
            "group",
            Some(subscription_id),
            "Select",
            Some("Provider"),
        );
        legacy_group.strategy_json = serde_json::json!({ "now": legacy_node_a }).to_string();
        let initial_items = vec![
            test_proxy_item(
                legacy_node_a,
                "node",
                Some(subscription_id),
                "A",
                Some("Provider"),
            ),
            test_proxy_item(
                legacy_node_b,
                "node",
                Some(subscription_id),
                "B",
                Some("Provider"),
            ),
            legacy_group,
        ];
        storage
            .replace_subscription_assets(
                subscription_id,
                &initial_items,
                &[(
                    legacy_group_name.to_string(),
                    vec![legacy_node_a.to_string(), legacy_node_b.to_string()],
                )],
                test_sync_commit(),
            )
            .await
            .expect("store initial assets");
        storage
            .set_group_now(legacy_group_name, legacy_node_b)
            .await
            .expect("select second member");

        let mut group = test_proxy_item(
            group_name,
            "group",
            Some(subscription_id),
            "Select",
            Some("Provider"),
        );
        group.strategy_json = serde_json::json!({ "now": node_a }).to_string();
        let refreshed_items = vec![
            test_proxy_item(node_a, "node", Some(subscription_id), "A", Some("Provider")),
            test_proxy_item(node_b, "node", Some(subscription_id), "B", Some("Provider")),
            group.clone(),
        ];
        storage
            .replace_subscription_assets(
                subscription_id,
                &refreshed_items,
                &[(
                    group_name.to_string(),
                    vec![node_a.to_string(), node_b.to_string()],
                )],
                test_sync_commit(),
            )
            .await
            .expect("refresh subscription assets");

        let refreshed = storage
            .proxy_items_for_runtime()
            .await
            .expect("list runtime items")
            .into_iter()
            .find(|item| item.name == group_name)
            .expect("refreshed group");
        assert_eq!(
            serde_json::from_str::<Value>(&refreshed.strategy_json)
                .expect("parse refreshed strategy")
                .get("now")
                .and_then(Value::as_str),
            Some(node_b)
        );

        group.strategy_json = serde_json::json!({ "now": node_a }).to_string();
        storage
            .replace_subscription_assets(
                subscription_id,
                &[
                    test_proxy_item(node_a, "node", Some(subscription_id), "A", Some("Provider")),
                    group,
                ],
                &[(group_name.to_string(), vec![node_a.to_string()])],
                test_sync_commit(),
            )
            .await
            .expect("refresh after selected member disappears");
        let fallback = storage
            .proxy_items_for_runtime()
            .await
            .expect("list fallback items")
            .into_iter()
            .find(|item| item.name == group_name)
            .expect("fallback group");
        assert_eq!(
            serde_json::from_str::<Value>(&fallback.strategy_json)
                .expect("parse fallback strategy")
                .get("now")
                .and_then(Value::as_str),
            Some(node_a)
        );
    }

    #[tokio::test]
    async fn subscription_asset_batches_cannot_overwrite_custom_group_members() {
        let temp = TestDir::new("subscription-custom-group-conflict");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .upsert_proxy_item(&test_proxy_item("Custom", "group", None, "Custom", None))
            .await
            .expect("store custom group");
        storage
            .replace_group_members("Custom", &[BUILTIN_DIRECT.into()])
            .await
            .expect("store custom members");
        storage
            .create_subscription(
                "sub_conflict",
                "Provider",
                "https://example.com/profile.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        let node = test_proxy_item(
            "Node^_^sub_conflict",
            "node",
            Some("sub_conflict"),
            "Node",
            Some("Provider"),
        );

        let undeclared = storage
            .replace_subscription_assets(
                "sub_conflict",
                std::slice::from_ref(&node),
                &[("Custom".into(), vec![node.name.clone()])],
                test_sync_commit(),
            )
            .await
            .expect_err("reject member writes to a group outside the batch");
        assert_eq!(undeclared.code, "internal_error");

        let incoming_group = test_proxy_item(
            "Custom",
            "group",
            Some("sub_conflict"),
            "Custom",
            Some("Provider"),
        );
        let conflict = storage
            .replace_subscription_assets(
                "sub_conflict",
                &[node.clone(), incoming_group],
                &[("Custom".into(), vec![node.name])],
                test_sync_commit(),
            )
            .await
            .expect_err("reject taking ownership of a custom group");
        assert_eq!(conflict.code, "subscription_asset_conflict");
        assert_eq!(
            storage.group_members("Custom").await.expect("load members"),
            vec![BUILTIN_DIRECT.to_string()]
        );
        assert_eq!(
            storage
                .group_source("Custom")
                .await
                .expect("load source")
                .as_deref(),
            Some("custom")
        );
    }

    #[tokio::test]
    async fn subscription_asset_batches_cannot_take_another_subscriptions_name() {
        let temp = TestDir::new("subscription-owner-conflict");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        for (id, name) in [("sub_owner", "Owner"), ("sub_other", "Other")] {
            storage
                .create_subscription(
                    id,
                    name,
                    "https://example.com/profile.yaml",
                    3600,
                    true,
                    &[],
                )
                .await
                .expect("create subscription");
        }
        let shared_name = "Shared^_^runtime";
        storage
            .replace_subscription_assets(
                "sub_owner",
                &[test_proxy_item(
                    shared_name,
                    "node",
                    Some("sub_owner"),
                    "Shared",
                    Some("Owner"),
                )],
                &[],
                test_sync_commit(),
            )
            .await
            .expect("store owned asset");
        let error = storage
            .replace_subscription_assets(
                "sub_other",
                &[test_proxy_item(
                    shared_name,
                    "node",
                    Some("sub_other"),
                    "Shared",
                    Some("Other"),
                )],
                &[],
                test_sync_commit(),
            )
            .await
            .expect_err("reject taking another subscription asset");
        assert_eq!(error.code, "subscription_asset_conflict");
        let owner = sqlx::query_scalar::<_, Option<String>>(
            "SELECT subscription_id FROM proxy_items WHERE name = ?",
        )
        .bind(shared_name)
        .fetch_one(&storage.pool)
        .await
        .expect("load preserved owner");
        assert_eq!(owner.as_deref(), Some("sub_owner"));
    }

    #[tokio::test]
    async fn referenced_subscription_node_cannot_be_refreshed_as_filtered_out() {
        let temp = TestDir::new("referenced-filtered-subscription-node");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let subscription_id = "sub_filtered";
        let node_name = "Node^_^sub_filtered";
        storage
            .create_subscription(
                subscription_id,
                "Provider",
                "https://example.com/profile.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        let node = test_proxy_item(
            node_name,
            "node",
            Some(subscription_id),
            "Node",
            Some("Provider"),
        );
        storage
            .replace_subscription_assets(
                subscription_id,
                std::slice::from_ref(&node),
                &[],
                test_sync_commit(),
            )
            .await
            .expect("store node");
        storage
            .upsert_rule(
                Some("rule_filtered_node".into()),
                "DOMAIN",
                "filtered.example.com",
                node_name,
                None,
                true,
            )
            .await
            .expect("reference node");
        let mut filtered = node;
        filtered.filtered_out = true;
        filtered.filter_reason = Some("test filter".into());
        let error = storage
            .replace_subscription_assets(subscription_id, &[filtered], &[], test_sync_commit())
            .await
            .expect_err("preserve referenced node availability");
        assert_eq!(error.code, "subscription_asset_referenced");
        let filtered_out =
            sqlx::query_scalar::<_, bool>("SELECT filtered_out FROM proxy_items WHERE name = ?")
                .bind(node_name)
                .fetch_one(&storage.pool)
                .await
                .expect("load preserved node");
        assert!(!filtered_out);
    }

    #[tokio::test]
    async fn deleting_subscription_cannot_empty_a_referenced_custom_group() {
        let temp = TestDir::new("referenced-custom-group-subscription-delete");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let subscription_id = "sub_custom_member";
        let node_name = "Node^_^sub_custom_member";
        storage
            .create_subscription(
                subscription_id,
                "Provider",
                "https://example.com/profile.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        storage
            .replace_subscription_assets(
                subscription_id,
                &[test_proxy_item(
                    node_name,
                    "node",
                    Some(subscription_id),
                    "Node",
                    Some("Provider"),
                )],
                &[],
                test_sync_commit(),
            )
            .await
            .expect("store node");
        storage
            .save_custom_group(
                None,
                &test_proxy_item("Custom", "group", None, "Custom", None),
                &[GroupFilterInput {
                    field: "name".into(),
                    operator: "equals".into(),
                    value: node_name.into(),
                    ..GroupFilterInput::default()
                }],
            )
            .await
            .expect("create custom group");
        storage
            .upsert_rule(
                Some("rule_custom".into()),
                "DOMAIN",
                "custom.example.com",
                "Custom",
                None,
                true,
            )
            .await
            .expect("reference custom group");
        let error = storage
            .delete_subscription(subscription_id)
            .await
            .expect_err("preserve the custom group's only member");
        assert_eq!(error.code, "subscription_asset_referenced");
        assert_eq!(
            storage.group_members("Custom").await.expect("load members"),
            vec![node_name.to_string()]
        );
        storage
            .get_subscription_url(subscription_id)
            .await
            .expect("subscription deletion was rolled back");
    }

    #[tokio::test]
    async fn subscription_mutations_preserve_assets_used_as_rule_policies() {
        let temp = TestDir::new("referenced-subscription-assets");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let subscription_id = "sub_referenced";
        let node_name = "Node^_^sub_referenced";
        storage
            .create_subscription(
                subscription_id,
                "Provider",
                "https://example.com/profile.yaml",
                3_600,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        storage
            .replace_subscription_assets(
                subscription_id,
                &[test_proxy_item(
                    node_name,
                    "node",
                    Some(subscription_id),
                    "Node",
                    Some("Provider"),
                )],
                &[],
                test_sync_commit(),
            )
            .await
            .expect("store subscription node");
        let rule = storage
            .upsert_rule(
                Some("rule_subscription_node".into()),
                "DOMAIN",
                "node.example.com",
                node_name,
                None,
                false,
            )
            .await
            .expect("store disabled subscription-node reference");

        let delete_error = storage
            .delete_subscription(subscription_id)
            .await
            .expect_err("preserve a referenced subscription");
        assert_eq!(delete_error.code, "subscription_referenced");
        let refresh_error = storage
            .replace_subscription_assets(subscription_id, &[], &[], test_sync_commit())
            .await
            .expect_err("preserve a referenced asset during refresh");
        assert_eq!(refresh_error.code, "subscription_asset_referenced");
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM proxy_items WHERE name = ?)",
        )
        .bind(node_name)
        .fetch_one(&storage.pool)
        .await
        .expect("check preserved asset"));

        storage
            .delete_rule(&rule.id)
            .await
            .expect("delete asset reference");
        storage
            .delete_subscription(subscription_id)
            .await
            .expect("delete unreferenced subscription");
    }

    #[tokio::test]
    async fn reference_migration_does_not_cascade_through_another_legacy_name() {
        let temp = TestDir::new("asset-name-chain-migration");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let subscription_id = "sub_chain";
        storage
            .create_subscription(
                subscription_id,
                "Provider",
                "https://example.com/profile.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");

        const CHAIN_LENGTH: usize = 6;
        let display_names = (0..CHAIN_LENGTH)
            .map(|depth| format!("X{}", "^_^Provider".repeat(depth)))
            .collect::<Vec<_>>();
        let old_names = display_names
            .iter()
            .map(|display_name| format!("{display_name}^_^Provider^_^sub_chain"))
            .collect::<Vec<_>>();
        let new_names = display_names
            .iter()
            .map(|display_name| format!("{display_name}^_^sub_chain"))
            .collect::<Vec<_>>();
        for depth in 1..CHAIN_LENGTH {
            assert_eq!(new_names[depth], old_names[depth - 1]);
        }
        let old_items = old_names
            .iter()
            .zip(&display_names)
            .enumerate()
            .map(|(index, (old_name, display_name))| {
                let mut item = test_proxy_item(
                    old_name,
                    "group",
                    Some(subscription_id),
                    display_name,
                    Some("Provider"),
                );
                item.position = ((index + 1) as i64) * 1024;
                item
            })
            .collect::<Vec<_>>();
        let old_group_members = old_names
            .iter()
            .map(|name| (name.clone(), vec!["DIRECT".into()]))
            .collect::<Vec<_>>();
        storage
            .replace_subscription_assets(
                subscription_id,
                &old_items,
                &old_group_members,
                test_sync_commit(),
            )
            .await
            .expect("store chained legacy names");

        let mut custom_group = test_proxy_item("Custom Chain", "group", None, "Custom Chain", None);
        custom_group.source = "custom".into();
        storage
            .upsert_proxy_item(&custom_group)
            .await
            .expect("create custom group");
        storage
            .replace_group_members("Custom Chain", &old_names)
            .await
            .expect("store legacy references");
        for (index, old_name) in old_names.iter().enumerate() {
            storage
                .upsert_rule(
                    Some(format!("rule_chain_{index}")),
                    "DOMAIN",
                    &format!("chain-{index}.example"),
                    old_name,
                    None,
                    true,
                )
                .await
                .expect("store chained reference");
        }

        let new_items = new_names
            .iter()
            .zip(&display_names)
            .enumerate()
            .map(|(index, (new_name, display_name))| {
                let mut item = test_proxy_item(
                    new_name,
                    "group",
                    Some(subscription_id),
                    display_name,
                    Some("Provider"),
                );
                item.position = ((index + 1) as i64) * 1024;
                item
            })
            .collect::<Vec<_>>();
        let new_group_members = new_names
            .iter()
            .map(|name| (name.clone(), vec!["DIRECT".into()]))
            .collect::<Vec<_>>();
        storage
            .replace_subscription_assets(
                subscription_id,
                &new_items,
                &new_group_members,
                test_sync_commit(),
            )
            .await
            .expect("migrate chained runtime names");

        let policies = storage
            .list_rules()
            .await
            .expect("list migrated rules")
            .into_iter()
            .map(|rule| (rule.id, rule.policy))
            .collect::<HashMap<_, _>>();
        for (index, new_name) in new_names.iter().enumerate() {
            assert_eq!(policies.get(&format!("rule_chain_{index}")), Some(new_name));
        }
        assert_eq!(
            storage
                .group_members("Custom Chain")
                .await
                .expect("list migrated members"),
            new_names
        );
        let custom_group = storage
            .proxy_items_for_runtime()
            .await
            .expect("list runtime items")
            .into_iter()
            .find(|item| item.name == "Custom Chain")
            .expect("custom group");
        assert_eq!(
            serde_json::from_str::<Value>(&custom_group.strategy_json)
                .expect("parse strategy")
                .get("now")
                .and_then(Value::as_str),
            new_names.first().map(String::as_str)
        );
    }

    #[tokio::test]
    async fn rule_sets_cannot_be_deleted_when_builtin_or_referenced() {
        let temp = TestDir::new("rule-set-delete");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");

        let builtin_error = storage
            .delete_rule_set("rs_builtin_applications")
            .await
            .expect_err("builtin rule set must be protected");
        assert_eq!(builtin_error.code, "ruleset_readonly");

        storage
            .create_rule_set(
                "rs_test_referenced",
                "test-referenced",
                "https://example.com/rules.txt",
                3600,
                Some("domain"),
                "text",
            )
            .await
            .expect("create custom rule set");
        let rule = storage
            .upsert_rule(
                Some("rule_test_reference".into()),
                "RULE-SET",
                "test-referenced",
                "PROXY",
                None,
                true,
            )
            .await
            .expect("create reference");

        let referenced_error = storage
            .delete_rule_set("rs_test_referenced")
            .await
            .expect_err("referenced rule set must be protected");
        assert_eq!(referenced_error.code, "ruleset_in_use");

        storage
            .delete_rule(&rule.id)
            .await
            .expect("delete reference");
        storage
            .delete_rule_set("rs_test_referenced")
            .await
            .expect("delete unreferenced custom rule set");
    }

    #[tokio::test]
    async fn rule_set_names_are_unique_and_rule_writes_require_an_existing_name() {
        let temp = TestDir::new("validated-rule-set-reference");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .create_rule_set(
                "rs_unique_one",
                "unique-name",
                "https://example.com/one.list",
                3600,
                Some("domain"),
                "text",
            )
            .await
            .expect("create rule set");
        let duplicate = storage
            .create_rule_set(
                "rs_unique_two",
                "unique-name",
                "https://example.com/two.list",
                3600,
                Some("domain"),
                "text",
            )
            .await
            .expect_err("reject a duplicate rule-set name");
        assert_eq!(duplicate.code, "ruleset_exists");

        let missing = storage
            .upsert_rule(
                Some("rule_missing_ruleset".into()),
                "RULE-SET",
                "missing-name",
                "DIRECT",
                None,
                true,
            )
            .await
            .expect_err("reject a missing rule-set reference");
        assert_eq!(missing.code, "rule_invalid_ruleset");
    }

    #[tokio::test]
    async fn pending_rule_sets_are_hidden_until_the_first_snapshot_is_ready() {
        let temp = TestDir::new("pending-rule-set");
        let paths = AppPaths::from_root(temp.path());
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        storage
            .create_pending_rule_set(
                "rs_pending",
                "pending-name",
                "https://example.com/pending.list",
                3_600,
                Some("domain"),
                "text",
            )
            .await
            .expect("create pending rule set");
        assert!(storage
            .list_rule_sets()
            .await
            .expect("list ready rule sets")
            .into_iter()
            .all(|rule_set| rule_set.id != "rs_pending"));
        let pending_reference = storage
            .upsert_rule(
                Some("rule_pending".into()),
                "RULE-SET",
                "pending-name",
                "DIRECT",
                None,
                true,
            )
            .await
            .expect_err("pending rule set must not be referenceable");
        assert_eq!(pending_reference.code, "rule_invalid_ruleset");

        storage
            .mark_rule_set_refresh_error("rs_pending", "temporary failure")
            .await
            .expect("store refresh error");
        storage
            .stage_rule_set_refresh(
                "rs_pending",
                "data/profiles/rule-sets/rs_pending.ready.list",
                12,
                1,
                "ready",
                "text",
                None,
            )
            .await
            .expect("stage first snapshot");
        assert!(storage
            .list_rule_sets()
            .await
            .expect("list rule sets while the snapshot is staged")
            .into_iter()
            .all(|rule_set| rule_set.id != "rs_pending"));
        assert!(storage
            .rule_sets_for_runtime()
            .await
            .expect("list candidate runtime rule sets")
            .into_iter()
            .any(|rule_set| rule_set.id == "rs_pending"));
        let staged_reference = storage
            .upsert_rule(
                Some("rule_staged".into()),
                "RULE-SET",
                "pending-name",
                "DIRECT",
                None,
                true,
            )
            .await
            .expect_err("staged rule set must not be referenceable");
        assert_eq!(staged_reference.code, "rule_invalid_ruleset");

        storage
            .activate_rule_set("rs_pending")
            .await
            .expect("commit the validated snapshot");
        let ready = storage
            .list_rule_sets()
            .await
            .expect("list activated rule set")
            .into_iter()
            .find(|rule_set| rule_set.id == "rs_pending")
            .expect("ready rule set");
        assert!(ready.last_error.is_none());
        storage
            .upsert_rule(
                Some("rule_ready".into()),
                "RULE-SET",
                "pending-name",
                "DIRECT",
                None,
                true,
            )
            .await
            .expect("ready rule set can be referenced");
        let referenced_delete = storage
            .stage_rule_set_deletion("rs_pending")
            .await
            .expect_err("referenced rule set cannot enter deleting state");
        assert_eq!(referenced_delete.code, "ruleset_in_use");
        storage
            .delete_rule("rule_ready")
            .await
            .expect("remove rule-set reference");
        storage
            .stage_rule_set_deletion("rs_pending")
            .await
            .expect("stage rule-set deletion");
        assert!(storage
            .list_rule_sets()
            .await
            .expect("hide deleting rule set")
            .into_iter()
            .all(|rule_set| rule_set.id != "rs_pending"));
        assert!(storage
            .rule_sets_for_runtime()
            .await
            .expect("compile deletion candidate")
            .into_iter()
            .all(|rule_set| rule_set.id != "rs_pending"));
        storage
            .restore_rule_set_deletion("rs_pending")
            .await
            .expect("restore staged deletion");

        storage
            .stage_rule_set_refresh(
                "rs_pending",
                "data/profiles/rule-sets/rs_pending.candidate.list",
                99,
                7,
                "candidate",
                "yaml",
                None,
            )
            .await
            .expect("stage replacement snapshot");
        let still_active = storage
            .list_rule_sets()
            .await
            .expect("keep active metadata while refresh is staged")
            .into_iter()
            .find(|rule_set| rule_set.id == "rs_pending")
            .expect("active rule set");
        assert_eq!(still_active.rule_count, 1);
        assert_eq!(still_active.format, "text");
        let candidate = storage
            .rule_sets_for_runtime()
            .await
            .expect("compile staged candidate")
            .into_iter()
            .find(|rule_set| rule_set.id == "rs_pending")
            .expect("candidate rule set");
        assert_eq!(candidate.format, "yaml");
        assert!(candidate
            .local_path
            .as_deref()
            .is_some_and(|path| path.ends_with("rs_pending.candidate.list")));

        storage.pool.close().await;
        let reopened = Storage::connect(&paths)
            .await
            .expect("discard interrupted refresh on reconnect");
        let restored = reopened
            .list_rule_sets()
            .await
            .expect("list restored active rule set")
            .into_iter()
            .find(|rule_set| rule_set.id == "rs_pending")
            .expect("restored rule set");
        assert_eq!(restored.rule_count, 1);
        assert_eq!(restored.format, "text");
        let restored_runtime = reopened
            .rule_sets_for_runtime()
            .await
            .expect("compile restored runtime")
            .into_iter()
            .find(|rule_set| rule_set.id == "rs_pending")
            .expect("restored runtime rule set");
        assert!(restored_runtime
            .local_path
            .as_deref()
            .is_some_and(|path| path.ends_with("rs_pending.ready.list")));
    }

    #[tokio::test]
    async fn freshly_updated_remote_resources_are_not_immediately_due() {
        let temp = TestDir::new("fresh-resource-due-state");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let subscription_id = "sub_fresh_due_test";
        storage
            .create_subscription(
                subscription_id,
                "Fresh Provider",
                "https://example.com/profile.yaml",
                86_400,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        storage
            .replace_subscription_assets(subscription_id, &[], &[], test_sync_commit())
            .await
            .expect("mark subscription refreshed");

        assert!(!storage
            .due_subscription_ids()
            .await
            .expect("query due subscriptions")
            .iter()
            .any(|id| id == subscription_id));
        assert!(!storage
            .startup_subscription_ids()
            .await
            .expect("query startup subscriptions")
            .iter()
            .any(|id| id == subscription_id));

        sqlx::query(
            "UPDATE subscriptions SET next_sync_at = datetime('now', '-1 day') WHERE id = ?",
        )
        .bind(subscription_id)
        .execute(&storage.pool)
        .await
        .expect("simulate a stale legacy next_sync_at value");
        assert!(!storage
            .due_subscription_ids()
            .await
            .expect("query subscription with legacy schedule")
            .iter()
            .any(|id| id == subscription_id));
        assert!(!storage
            .startup_subscription_ids()
            .await
            .expect("query startup subscription with legacy schedule")
            .iter()
            .any(|id| id == subscription_id));

        let rule_set_id = "rs_fresh_due_test";
        storage
            .create_rule_set(
                rule_set_id,
                "fresh-due-test",
                "https://example.com/rules.txt",
                86_400,
                Some("domain"),
                "text",
            )
            .await
            .expect("create rule set");
        storage
            .update_rule_set_refresh(
                rule_set_id,
                "data/profiles/rule-sets/rs_fresh_due_test.list",
                12,
                1,
                "fresh-hash",
                "text",
                None,
            )
            .await
            .expect("mark rule set refreshed");
        assert!(!storage
            .due_rule_set_ids()
            .await
            .expect("query due rule sets")
            .iter()
            .any(|id| id == rule_set_id));
    }

    #[tokio::test]
    async fn failed_remote_resources_wait_before_retrying() {
        let temp = TestDir::new("failed-resource-backoff");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let subscription_id = "sub_failed_backoff";
        storage
            .create_subscription(
                subscription_id,
                "Failing Provider",
                "https://example.com/profile.yaml",
                60,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        storage
            .mark_subscription_sync_error(subscription_id, "network unavailable")
            .await
            .expect("record subscription failure");

        assert!(!storage
            .due_subscription_ids()
            .await
            .expect("query backed-off subscriptions")
            .iter()
            .any(|id| id == subscription_id));
        assert!(!storage
            .startup_subscription_ids()
            .await
            .expect("query startup backed-off subscriptions")
            .iter()
            .any(|id| id == subscription_id));
        let next_sync_at = sqlx::query_scalar::<_, Option<String>>(
            "SELECT next_sync_at FROM subscriptions WHERE id = ?",
        )
        .bind(subscription_id)
        .fetch_one(&storage.pool)
        .await
        .expect("read retry timestamp");
        assert!(next_sync_at.is_some());

        sqlx::query(
            r#"
UPDATE subscriptions
SET sync_finished_at = datetime('now', '-1 hour'),
    next_sync_at = datetime('now', '-1 second')
WHERE id = ?
"#,
        )
        .bind(subscription_id)
        .execute(&storage.pool)
        .await
        .expect("expire retry backoff");
        assert!(storage
            .due_subscription_ids()
            .await
            .expect("query expired subscription backoff")
            .iter()
            .any(|id| id == subscription_id));

        storage
            .replace_subscription_assets(subscription_id, &[], &[], test_sync_commit())
            .await
            .expect("complete successful retry");
        let error_count =
            sqlx::query_scalar::<_, i64>("SELECT sync_error_count FROM subscriptions WHERE id = ?")
                .bind(subscription_id)
                .fetch_one(&storage.pool)
                .await
                .expect("read reset error count");
        assert_eq!(error_count, 0);

        let rule_set_id = "rs_failed_backoff";
        storage
            .create_rule_set(
                rule_set_id,
                "failing-rules",
                "https://example.com/rules.txt",
                60,
                Some("domain"),
                "text",
            )
            .await
            .expect("create rule set");
        storage
            .mark_rule_set_refresh_error(rule_set_id, "network unavailable")
            .await
            .expect("record rule-set failure");
        assert!(!storage
            .due_rule_set_ids()
            .await
            .expect("query backed-off rule sets")
            .iter()
            .any(|id| id == rule_set_id));

        sqlx::query("UPDATE rule_sets SET updated_at = datetime('now', '-2 hours') WHERE id = ?")
            .bind(rule_set_id)
            .execute(&storage.pool)
            .await
            .expect("expire rule-set backoff");
        assert!(storage
            .due_rule_set_ids()
            .await
            .expect("query expired rule-set backoff")
            .iter()
            .any(|id| id == rule_set_id));
    }

    #[tokio::test]
    async fn builtin_proxy_contains_nodes_and_non_cyclic_strategy_groups() {
        let temp = TestDir::new("builtin-proxy-members");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .upsert_proxy_item(&test_proxy_item("Node", "node", None, "Node", None))
            .await
            .expect("store node");
        for name in ["Regional", "Depends on PROXY"] {
            storage
                .upsert_proxy_item(&test_proxy_item(name, "group", None, name, None))
                .await
                .expect("store group");
        }
        storage
            .replace_group_members("Regional", &["Node".into()])
            .await
            .expect("store regional members");
        storage
            .replace_group_members("Depends on PROXY", &[BUILTIN_PROXY.into()])
            .await
            .expect("store cyclic members");

        let (groups, _) = storage
            .proxy_topology()
            .await
            .expect("load synchronized topology");
        let members = &groups
            .iter()
            .find(|group| group.name == BUILTIN_PROXY)
            .expect("builtin proxy group")
            .all;
        assert!(members.iter().any(|member| member == "Node"));
        assert!(members.iter().any(|member| member == "Regional"));
        assert!(!members.iter().any(|member| member == "Depends on PROXY"));
    }

    #[tokio::test]
    async fn builtin_proxy_sync_preserves_the_last_delay() {
        let temp = TestDir::new("builtin-proxy-delay");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .set_group_delay(BUILTIN_PROXY, 321)
            .await
            .expect("store proxy delay");
        storage
            .sync_builtin_proxy_group()
            .await
            .expect("synchronize builtin proxy");
        let (groups, _) = storage.proxy_topology().await.expect("load topology");
        assert_eq!(
            groups
                .into_iter()
                .find(|group| group.name == BUILTIN_PROXY)
                .expect("builtin proxy")
                .delay,
            321
        );
    }

    #[tokio::test]
    async fn oversized_unsigned_values_are_rejected_before_sqlite_writes() {
        let temp = TestDir::new("oversized-unsigned-values");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");

        let subscription_error = storage
            .create_subscription(
                "sub_oversized",
                "Provider",
                "https://example.com/subscription",
                u64::MAX,
                true,
                &[],
            )
            .await
            .expect_err("reject an interval that cannot fit in SQLite INTEGER");
        assert_eq!(subscription_error.code, "numeric_value_out_of_range");
        assert!(!storage
            .list_subscriptions()
            .await
            .expect("list subscriptions")
            .iter()
            .any(|subscription| subscription.id == "sub_oversized"));

        let rule_set_error = storage
            .create_rule_set(
                "rs_oversized",
                "oversized",
                "https://example.com/rules",
                u64::MAX,
                None,
                "text",
            )
            .await
            .expect_err("reject an oversized rule-set interval");
        assert_eq!(rule_set_error.code, "numeric_value_out_of_range");

        storage
            .create_subscription(
                "sub_quota",
                "Provider",
                "https://example.com/subscription",
                3_600,
                true,
                &[],
            )
            .await
            .expect("create quota test subscription");
        let mut commit = test_sync_commit();
        commit.upload_bytes = Some(u64::MAX);
        let quota_error = storage
            .replace_subscription_assets("sub_quota", &[], &[], commit)
            .await
            .expect_err("reject quota metadata that cannot fit in SQLite INTEGER");
        assert_eq!(quota_error.code, "numeric_value_out_of_range");

        sqlx::query(
            "UPDATE subscriptions SET interval_seconds = -1, upload_bytes = -1, download_bytes = 42, total_bytes = -1 WHERE id = ?",
        )
        .bind("sub_quota")
        .execute(&storage.pool)
        .await
        .expect("simulate legacy wrapped values");
        let recovered = storage
            .list_subscriptions()
            .await
            .expect("list hardened subscription values")
            .into_iter()
            .find(|subscription| subscription.id == "sub_quota")
            .expect("quota test subscription");
        assert_eq!(recovered.interval_seconds, 0);
        assert_eq!(recovered.interval, 0);
        assert_eq!(recovered.traffic.used, 42);
        assert_eq!(recovered.traffic.total, 0);
    }

    #[tokio::test]
    async fn custom_group_deletion_preserves_all_rule_targets() {
        let temp = TestDir::new("referenced-custom-group");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .upsert_proxy_item(&test_proxy_item("Node", "node", None, "Node", None))
            .await
            .expect("store available custom-group member");
        for (suffix, enabled) in [("enabled", true), ("disabled", false)] {
            let group_name = format!("Strategy {suffix}");
            storage
                .upsert_proxy_item(&test_proxy_item(
                    &group_name,
                    "group",
                    None,
                    &group_name,
                    None,
                ))
                .await
                .expect("store custom group");
            storage
                .upsert_rule(
                    Some(format!("rule_strategy_{suffix}")),
                    "DOMAIN",
                    &format!("{suffix}.example.com"),
                    &group_name,
                    None,
                    enabled,
                )
                .await
                .expect("store group reference");

            assert_eq!(
                storage
                    .policy_reference_count(&group_name)
                    .await
                    .expect("count all references"),
                1
            );
            let error = storage
                .delete_custom_group(&group_name)
                .await
                .expect_err("keep a group referenced by any rule");
            assert_eq!(error.code, "proxy_group_referenced");
            assert_eq!(
                storage
                    .group_source(&group_name)
                    .await
                    .expect("read group source")
                    .as_deref(),
                Some("custom")
            );
        }
    }

    #[tokio::test]
    async fn pending_subscription_assets_are_runtime_only_until_activation() {
        let temp = TestDir::new("pending-subscription-activation");
        let paths = AppPaths::from_root(temp.path());
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let subscription_id = "sub_pending";
        let asset_name = "Pending Node";
        storage
            .create_pending_subscription(
                subscription_id,
                "Pending Provider",
                "https://example.com/pending.yaml",
                3_600,
                true,
                &[],
            )
            .await
            .expect("create pending subscription");
        storage
            .replace_subscription_assets(
                subscription_id,
                &[test_proxy_item(
                    asset_name,
                    "node",
                    Some(subscription_id),
                    asset_name,
                    Some("Pending Provider"),
                )],
                &[],
                test_sync_commit(),
            )
            .await
            .expect("stage subscription assets");

        assert!(storage
            .list_subscriptions()
            .await
            .expect("list visible subscriptions")
            .into_iter()
            .all(|subscription| subscription.id != subscription_id));
        assert!(!storage
            .due_subscription_ids()
            .await
            .expect("list due subscriptions")
            .contains(&subscription_id.to_string()));
        assert!(storage
            .proxy_items_for_runtime()
            .await
            .expect("compile candidate runtime assets")
            .into_iter()
            .any(|item| item.name == asset_name));
        let pending_reference = storage
            .upsert_rule(
                Some("rule_pending_subscription".into()),
                "DOMAIN",
                "pending.example.com",
                asset_name,
                None,
                true,
            )
            .await
            .expect_err("pending subscription asset must not be referenceable");
        assert_eq!(pending_reference.code, "rule_invalid_policy");

        storage
            .activate_subscription(subscription_id)
            .await
            .expect("activate subscription");
        assert!(storage
            .list_subscriptions()
            .await
            .expect("list activated subscriptions")
            .into_iter()
            .any(|subscription| subscription.id == subscription_id));
        storage
            .upsert_rule(
                Some("rule_active_subscription".into()),
                "DOMAIN",
                "active.example.com",
                asset_name,
                None,
                true,
            )
            .await
            .expect("activated subscription asset can be referenced");
        let referenced_delete = storage
            .stage_subscription_deletion(subscription_id)
            .await
            .expect_err("referenced subscription cannot enter deleting state");
        assert_eq!(referenced_delete.code, "subscription_referenced");
        storage
            .delete_rule("rule_active_subscription")
            .await
            .expect("remove subscription reference");
        storage
            .stage_subscription_deletion(subscription_id)
            .await
            .expect("stage subscription deletion");
        assert!(storage
            .list_subscriptions()
            .await
            .expect("hide deleting subscription")
            .into_iter()
            .all(|subscription| subscription.id != subscription_id));
        assert!(storage
            .proxy_items_for_runtime()
            .await
            .expect("compile deletion candidate")
            .into_iter()
            .all(|item| item.name != asset_name));
        let deleting_reference = storage
            .upsert_rule(
                Some("rule_deleting_subscription".into()),
                "DOMAIN",
                "deleting.example.com",
                asset_name,
                None,
                true,
            )
            .await
            .expect_err("deleting subscription asset must not be referenceable");
        assert_eq!(deleting_reference.code, "rule_invalid_policy");

        storage
            .create_pending_subscription(
                "sub_crashed",
                "Crashed Provider",
                "https://example.com/crashed.yaml",
                3_600,
                true,
                &[],
            )
            .await
            .expect("create interrupted subscription");
        storage.pool.close().await;
        let reopened = Storage::connect(&paths)
            .await
            .expect("clean interrupted subscription on reconnect");
        assert!(reopened.get_subscription_url("sub_crashed").await.is_err());
        assert!(reopened
            .list_subscriptions()
            .await
            .expect("restore interrupted deletion")
            .into_iter()
            .any(|subscription| subscription.id == subscription_id));
        assert!(reopened
            .proxy_items_for_runtime()
            .await
            .expect("restore interrupted subscription assets")
            .into_iter()
            .any(|item| item.name == asset_name));
    }

    #[tokio::test]
    async fn routing_rule_writes_require_an_available_policy_target() {
        let temp = TestDir::new("validated-rule-policy");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        let missing_error = storage
            .upsert_rule(
                Some("rule_missing_policy".into()),
                "DOMAIN",
                "missing.example.com",
                "Missing Group",
                None,
                true,
            )
            .await
            .expect_err("reject a missing policy target");
        assert_eq!(missing_error.code, "rule_invalid_policy");

        storage
            .upsert_proxy_item(&test_proxy_item("Node", "node", None, "Node", None))
            .await
            .expect("store available custom-group member");
        storage
            .upsert_proxy_item(&test_proxy_item(
                "Strategy", "group", None, "Strategy", None,
            ))
            .await
            .expect("store policy target");
        let rule = storage
            .upsert_rule(
                Some("rule_valid_policy".into()),
                "DOMAIN",
                "valid.example.com",
                "Strategy",
                None,
                true,
            )
            .await
            .expect("store valid policy target");
        let update_error = storage
            .update_rule(
                &rule.id,
                "DOMAIN",
                "valid.example.com",
                "Missing Group",
                None,
                true,
                None,
            )
            .await
            .expect_err("reject an update to a missing policy target");
        assert_eq!(update_error.code, "rule_invalid_policy");
        assert_eq!(
            storage
                .list_rules()
                .await
                .expect("list preserved rules")
                .into_iter()
                .find(|item| item.id == rule.id)
                .expect("find preserved rule")
                .policy,
            "Strategy"
        );
    }

    #[tokio::test]
    async fn custom_group_rename_preserves_disabled_rule_targets() {
        let temp = TestDir::new("rename-referenced-custom-group");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .upsert_proxy_item(&test_proxy_item("Node", "node", None, "Node", None))
            .await
            .expect("store available custom-group member");
        storage
            .upsert_proxy_item(&test_proxy_item(
                "Strategy", "group", None, "Strategy", None,
            ))
            .await
            .expect("store custom group");
        storage
            .upsert_rule(
                Some("rule_disabled_strategy".into()),
                "DOMAIN",
                "disabled.example.com",
                "Strategy",
                None,
                false,
            )
            .await
            .expect("store disabled group reference");

        let service = crate::proxy::ProxyService::new(storage.clone());
        let error = service
            .update_group(
                "Strategy",
                crate::types::ProxyGroupRequest {
                    name: "Renamed Strategy".into(),
                    group_type: "select".into(),
                    filter: Vec::new(),
                },
            )
            .await
            .expect_err("reject renaming a group referenced by a disabled rule");

        assert_eq!(error.code, "proxy_group_referenced");
        assert_eq!(
            storage
                .group_source("Strategy")
                .await
                .expect("read original group")
                .as_deref(),
            Some("custom")
        );
        assert_eq!(
            storage
                .group_source("Renamed Strategy")
                .await
                .expect("check target group"),
            None
        );
    }

    #[tokio::test]
    async fn custom_group_rename_is_atomic_and_preserves_filter_ids_and_selection() {
        let temp = TestDir::new("atomic-custom-group-rename");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        for item in [
            test_proxy_item("Strategy", "group", None, "Strategy", None),
            test_proxy_item("Node A", "node", None, "Node A", None),
            test_proxy_item("Node B", "node", None, "Node B", None),
        ] {
            storage
                .upsert_proxy_item(&item)
                .await
                .expect("store proxy item");
        }
        storage
            .replace_group_members("Strategy", &["Node A".into(), "Node B".into()])
            .await
            .expect("store initial members");
        storage
            .set_group_now("Strategy", "Node B")
            .await
            .expect("select second member");
        let filter = GroupFilterInput {
            id: Some("filter_strategy".into()),
            action: "keep".into(),
            field: "name".into(),
            operator: "starts_with".into(),
            value: "Node".into(),
            ..GroupFilterInput::default()
        };
        storage
            .replace_group_filters("Strategy", std::slice::from_ref(&filter))
            .await
            .expect("store initial filter");

        crate::proxy::ProxyService::new(storage.clone())
            .update_group(
                "Strategy",
                crate::types::ProxyGroupRequest {
                    name: "Renamed Strategy".into(),
                    group_type: "select".into(),
                    filter: vec![filter],
                },
            )
            .await
            .expect("rename custom group atomically");

        assert_eq!(
            storage
                .group_source("Strategy")
                .await
                .expect("check old group"),
            None
        );
        assert_eq!(
            storage
                .group_source("Renamed Strategy")
                .await
                .expect("check renamed group")
                .as_deref(),
            Some("custom")
        );
        assert_eq!(
            storage
                .group_filters("Renamed Strategy")
                .await
                .expect("load renamed filters")[0]
                .id
                .as_deref(),
            Some("filter_strategy")
        );
        assert_eq!(
            storage
                .current_group_now("Renamed Strategy")
                .await
                .expect("read renamed selection")
                .as_deref(),
            Some("Node B")
        );
    }

    #[tokio::test]
    async fn custom_group_rename_and_rule_creation_cannot_leave_a_dangling_policy() {
        let temp = TestDir::new("concurrent-group-rename-rule");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        for item in [
            test_proxy_item("Strategy", "group", None, "Strategy", None),
            test_proxy_item("Node A", "node", None, "Node A", None),
        ] {
            storage
                .upsert_proxy_item(&item)
                .await
                .expect("store proxy item");
        }

        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let rename_service = crate::proxy::ProxyService::new(storage.clone());
        let rename_barrier = barrier.clone();
        let rename = tokio::spawn(async move {
            rename_barrier.wait().await;
            rename_service
                .update_group(
                    "Strategy",
                    crate::types::ProxyGroupRequest {
                        name: "Renamed Strategy".into(),
                        group_type: "select".into(),
                        filter: vec![GroupFilterInput {
                            action: "keep".into(),
                            field: "name".into(),
                            operator: "starts_with".into(),
                            value: "Node".into(),
                            ..GroupFilterInput::default()
                        }],
                    },
                )
                .await
        });
        let rule_storage = storage.clone();
        let rule_barrier = barrier.clone();
        let rule = tokio::spawn(async move {
            rule_barrier.wait().await;
            rule_storage
                .upsert_rule(
                    Some("rule_concurrent_strategy".into()),
                    "DOMAIN",
                    "concurrent.example.com",
                    "Strategy",
                    None,
                    true,
                )
                .await
        });
        barrier.wait().await;
        let rename_result = rename.await.expect("join rename task");
        let rule_result = rule.await.expect("join rule task");

        assert_ne!(rename_result.is_ok(), rule_result.is_ok());
        if rename_result.is_ok() {
            assert_eq!(
                rule_result.expect_err("old policy must disappear").code,
                "rule_invalid_policy"
            );
            assert!(storage
                .group_source("Strategy")
                .await
                .expect("check old group")
                .is_none());
            assert_eq!(
                storage
                    .group_source("Renamed Strategy")
                    .await
                    .expect("check renamed group")
                    .as_deref(),
                Some("custom")
            );
        } else {
            assert_eq!(
                rename_result
                    .expect_err("referenced group cannot be renamed")
                    .code,
                "proxy_group_referenced"
            );
            assert_eq!(
                rule_result.expect("rule creation should win").policy,
                "Strategy"
            );
            assert_eq!(
                storage
                    .group_source("Strategy")
                    .await
                    .expect("check preserved group")
                    .as_deref(),
                Some("custom")
            );
            assert!(storage
                .group_source("Renamed Strategy")
                .await
                .expect("check absent target")
                .is_none());
        }
    }

    #[tokio::test]
    async fn scalar_group_filter_values_stay_scalar_after_storage_round_trip() {
        let temp = TestDir::new("scalar-group-filter");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .upsert_proxy_item(&test_proxy_item(
                "Strategy", "group", None, "Strategy", None,
            ))
            .await
            .expect("store custom group");
        storage
            .replace_group_filters(
                "Strategy",
                &[GroupFilterInput {
                    field: "protocol".into(),
                    operator: "is".into(),
                    value: "trojan".into(),
                    ..GroupFilterInput::default()
                }],
            )
            .await
            .expect("store scalar filter");

        let filters = storage
            .group_filters("Strategy")
            .await
            .expect("reload scalar filter");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].operator, "is");
        assert_eq!(filters[0].value, "trojan");
        assert!(filters[0].values.is_empty());
    }

    #[tokio::test]
    async fn group_selection_requires_an_existing_group_member() {
        let temp = TestDir::new("validated-group-selection");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .upsert_proxy_item(&test_proxy_item(
                "Strategy", "group", None, "Strategy", None,
            ))
            .await
            .expect("store custom group");
        storage
            .replace_group_members("Strategy", &["Node A".into(), "Node B".into()])
            .await
            .expect("store group members");

        let missing_group = storage
            .set_group_now("Missing", "Node A")
            .await
            .expect_err("reject a missing group");
        assert_eq!(missing_group.code, "proxy_group_not_found");
        let missing_member = storage
            .set_group_now("Strategy", "Unknown")
            .await
            .expect_err("reject a non-member selection");
        assert_eq!(missing_member.code, "proxy_group_member_not_found");
        let mut automatic = test_proxy_item("Automatic", "group", None, "Automatic", None);
        automatic.group_type = Some("url-test".into());
        storage
            .upsert_proxy_item(&automatic)
            .await
            .expect("store automatic group");
        storage
            .replace_group_members("Automatic", &["Node A".into()])
            .await
            .expect("store automatic group members");
        let automatic_selection = storage
            .set_group_now("Automatic", "Node A")
            .await
            .expect_err("automatic groups cannot persist manual selections");
        assert_eq!(automatic_selection.code, "proxy_group_not_selectable");
        assert_eq!(
            storage
                .current_group_now("Strategy")
                .await
                .expect("read preserved selection")
                .as_deref(),
            Some("Node A")
        );

        storage
            .set_group_now("Strategy", "Node B")
            .await
            .expect("select a valid member");
        assert_eq!(
            storage
                .current_group_now("Strategy")
                .await
                .expect("read updated selection")
                .as_deref(),
            Some("Node B")
        );
    }

    #[tokio::test]
    async fn group_selection_returns_and_restores_the_previous_value() {
        let temp = TestDir::new("group-selection-rollback");
        let storage = Storage::connect(&AppPaths::from_root(temp.path()))
            .await
            .expect("connect test storage");
        storage
            .upsert_proxy_item(&test_proxy_item(
                "Strategy", "group", None, "Strategy", None,
            ))
            .await
            .expect("store group");
        storage
            .replace_group_members("Strategy", &["Node A".into(), "Node B".into()])
            .await
            .expect("store members");

        let previous = storage
            .set_group_now("Strategy", "Node B")
            .await
            .expect("select second member");
        assert_eq!(previous.as_deref(), Some("Node A"));
        storage
            .restore_group_now("Strategy", previous.as_deref())
            .await
            .expect("restore previous member");
        assert_eq!(
            storage
                .current_group_now("Strategy")
                .await
                .expect("load selection")
                .as_deref(),
            Some("Node A")
        );

        storage
            .restore_group_now("Strategy", None)
            .await
            .expect("clear selection");
        assert!(storage
            .current_group_now("Strategy")
            .await
            .expect("load cleared selection")
            .is_none());
        let previous = storage
            .set_group_now("Strategy", "Node B")
            .await
            .expect("select from an empty prior state");
        assert!(previous.is_none());
        storage
            .restore_group_now("Strategy", previous.as_deref())
            .await
            .expect("restore empty prior state");
        assert!(storage
            .current_group_now("Strategy")
            .await
            .expect("load restored empty selection")
            .is_none());
    }

    fn test_proxy_item(
        name: &str,
        kind: &str,
        subscription_id: Option<&str>,
        display_name: &str,
        source_name: Option<&str>,
    ) -> ProxyItemRecord {
        ProxyItemRecord {
            name: name.into(),
            kind: kind.into(),
            subscription_id: subscription_id.map(str::to_string),
            display_name: display_name.into(),
            source: if subscription_id.is_some() {
                "subscription".into()
            } else {
                "custom".into()
            },
            builtin: false,
            source_name: source_name.map(str::to_string),
            protocol: (kind == "node").then(|| "ss".into()),
            country: None,
            group_type: (kind == "group").then(|| "select".into()),
            raw_json: (kind == "node").then(|| "{}".into()),
            content_hash: None,
            latency_ms: None,
            alive: true,
            filtered_out: false,
            filter_reason: None,
            delay_ms: None,
            tolerance_ms: None,
            url: None,
            interval_seconds: None,
            strategy_json: "{}".into(),
            position: 1024,
            enabled: true,
        }
    }

    fn test_sync_commit() -> SubscriptionSyncCommit {
        SubscriptionSyncCommit {
            subscription_name: "Provider".into(),
            node_count: 1,
            upload_bytes: None,
            download_bytes: None,
            total_bytes: None,
            expire_at: None,
            source_format: "clash".into(),
            raw_content_hash: "test".into(),
        }
    }

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("rweb-clash-{name}-{}", new_id("test")));
            std::fs::create_dir_all(&path).expect("create test directory");
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
