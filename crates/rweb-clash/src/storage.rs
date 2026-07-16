use crate::error::AppError;
use crate::paths::{
    ensure_private_directory, restrict_sensitive_file_permissions, sqlite_companion_path, AppPaths,
};
use crate::types::{
    FilterRule, FilterRuleInput, GroupFilterInput, LogEntryResponse, ProxyGroupResponse,
    ProxyNodeResponse, RuleResponse, RuleSetResponse, SubscriptionMemberGroup,
    SubscriptionMemberNode, SubscriptionMemberSection, SubscriptionMembersResponse,
    SubscriptionResponse, SystemConfig, TrafficQuota, BUILTIN_PROXY, SUB_DELIMITER,
};
use crate::util::{bool_to_i64, display_log_time, i64_to_bool, new_id, normalize_status, now_iso};
use serde_json::{Map, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone)]
pub struct Storage {
    pool: SqlitePool,
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
        let storage = Self { pool };
        storage.migrate().await?;
        storage.ensure_default_settings().await?;
        storage.ensure_builtin_rule_sets().await?;
        storage.ensure_builtin_rules().await?;
        storage.sync_builtin_proxy_group().await?;
        restrict_sqlite_file_permissions(&paths.database_file)?;
        info!("sqlite storage ready");
        Ok(storage)
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
        self.ensure_proxy_item_builtin_column().await?;
        self.normalize_rule_set_local_paths().await?;
        self.normalize_builtin_rule_set_formats().await?;
        sqlx::query(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES(1, 'initial', ?)",
        )
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        info!("database migrations applied");
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
        let mut tx = self.pool.begin().await?;
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
        let mut tx = self.pool.begin().await?;
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
        let mut tx = self.pool.begin().await?;
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
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM global_filter_rules")
            .execute(&mut *tx)
            .await?;
        for (index, rule) in rules.iter().enumerate() {
            let values = if rule.match_type.trim() == "in" || rule.has_values() {
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
        let rows =
            sqlx::query("SELECT id FROM subscriptions WHERE inherit_global_rules = 1 ORDER BY id")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|row| row.try_get("id").map_err(AppError::from))
            .collect()
    }

    pub async fn create_subscription(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        inherit_global: bool,
        rules: &[FilterRuleInput],
    ) -> Result<(), AppError> {
        let now = now_iso();
        let next_sync_at = now.clone();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
INSERT INTO subscriptions(
  id, name, url, status, interval_seconds, inherit_global_rules, node_count,
  next_sync_at, created_at, updated_at
) VALUES(?, ?, ?, 'syncing', ?, ?, 0, ?, ?, ?)
"#,
        )
        .bind(id)
        .bind(name)
        .bind(url)
        .bind(interval_seconds as i64)
        .bind(bool_to_i64(inherit_global))
        .bind(next_sync_at)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        insert_subscription_rules(&mut tx, id, rules, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_subscription(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        inherit_global: bool,
        rules: &[FilterRuleInput],
    ) -> Result<(), AppError> {
        let now = now_iso();
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
UPDATE subscriptions
SET name = ?, url = ?, interval_seconds = ?, inherit_global_rules = ?, updated_at = ?
WHERE id = ?
"#,
        )
        .bind(name)
        .bind(url)
        .bind(interval_seconds as i64)
        .bind(bool_to_i64(inherit_global))
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
        let result = sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "subscription_not_found",
                format!("subscription {id} not found"),
            ));
        }
        Ok(())
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<SubscriptionResponse>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id, name, url, source_format, status, interval_seconds, inherit_global_rules,
       upload_bytes, download_bytes, total_bytes, expire_at, node_count,
       last_update_at, last_error
FROM subscriptions
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
            let interval_seconds: i64 = row.try_get("interval_seconds")?;
            output.push(SubscriptionResponse {
                id: id.clone(),
                name: row.try_get("name")?,
                url: row.try_get("url")?,
                format: row.try_get("source_format")?,
                nodes: row.try_get("node_count")?,
                status: normalize_status(&row.try_get::<String, _>("status")?),
                traffic: TrafficQuota {
                    used: (upload.unwrap_or(0).saturating_add(download.unwrap_or(0))) as u64,
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

    pub async fn due_subscription_ids(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id
FROM subscriptions
WHERE interval_seconds > 0
  AND status != 'syncing'
  AND (
    last_update_at IS NULL
    OR CAST(strftime('%s', last_update_at) AS INTEGER) + interval_seconds
       <= CAST(strftime('%s', 'now') AS INTEGER)
  )
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
WHERE interval_seconds > 0
  AND (
    status = 'syncing'
    OR last_update_at IS NULL
    OR CAST(strftime('%s', last_update_at) AS INTEGER) + interval_seconds
       <= CAST(strftime('%s', 'now') AS INTEGER)
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
        if items
            .iter()
            .any(|item| item.subscription_id.as_deref() != Some(subscription_id))
        {
            return Err(AppError::internal(
                "subscription asset batch contained an item owned by another subscription",
            ));
        }

        let now = now_iso();
        let mut tx = self.pool.begin().await?;
        let migration = migrate_subscription_asset_references(
            &mut tx,
            subscription_id,
            items,
            group_members,
            &now,
        )
        .await?;
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
    last_error = NULL,
    source_format = ?,
    raw_content_hash = ?,
    updated_at = ?
WHERE id = ?
"#,
        )
        .bind(commit.subscription_name)
        .bind(commit.upload_bytes.map(|value| value as i64))
        .bind(commit.download_bytes.map(|value| value as i64))
        .bind(commit.total_bytes.map(|value| value as i64))
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

    pub async fn upsert_proxy_item(&self, item: &ProxyItemRecord) -> Result<(), AppError> {
        let now = now_iso();
        let mut tx = self.pool.begin().await?;
        upsert_proxy_item_in_transaction(&mut tx, item, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn sync_builtin_proxy_group(&self) -> Result<(), AppError> {
        let members = self.valid_node_names().await?;
        let current_now = self
            .current_group_now(BUILTIN_PROXY_GROUP_NAME)
            .await?
            .filter(|selected| members.iter().any(|member| member == selected));
        let selected = current_now.or_else(|| members.first().cloned());
        self.upsert_proxy_item(&ProxyItemRecord {
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
            delay_ms: None,
            tolerance_ms: None,
            url: None,
            interval_seconds: None,
            strategy_json: selected
                .map(|name| serde_json::json!({ "now": name }).to_string())
                .unwrap_or_else(|| "{}".to_string()),
            position: -100_000,
            enabled: true,
        })
        .await?;
        self.replace_group_members(BUILTIN_PROXY_GROUP_NAME, &members)
            .await?;
        sqlx::query("DELETE FROM proxy_group_filters WHERE group_name = ?")
            .bind(BUILTIN_PROXY_GROUP_NAME)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn replace_group_members(
        &self,
        group_name: &str,
        members: &[String],
    ) -> Result<(), AppError> {
        let now = now_iso();
        let selected = self
            .current_group_now(group_name)
            .await?
            .filter(|selected| members.contains(selected))
            .or_else(|| members.first().cloned());
        let strategy = selected
            .map(|name| serde_json::json!({ "now": name }).to_string())
            .unwrap_or_else(|| "{}".to_string());
        let mut tx = self.pool.begin().await?;
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
        sqlx::query(
            "UPDATE proxy_items SET strategy_json = ?, updated_at = ? WHERE name = ? AND kind = 'group'",
        )
        .bind(strategy)
        .bind(&now)
        .bind(group_name)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn replace_group_filters(
        &self,
        group_name: &str,
        filters: &[GroupFilterInput],
    ) -> Result<(), AppError> {
        let now = now_iso();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM proxy_group_filters WHERE group_name = ?")
            .bind(group_name)
            .execute(&mut *tx)
            .await?;
        for (index, filter) in filters.iter().enumerate() {
            let values = if filter.operator.trim() == "in" || filter.has_values() {
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
            .bind(filter.operator.trim())
            .bind(filter.value.trim())
            .bind(values_json)
            .bind(bool_to_i64(filter.enabled.unwrap_or(true)))
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_custom_group(&self, group_name: &str) -> Result<(), AppError> {
        match self.group_source(group_name).await? {
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
        let result = sqlx::query(
            "DELETE FROM proxy_items WHERE name = ? AND kind = 'group' AND source = 'custom'",
        )
        .bind(group_name)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "proxy_group_not_found",
                format!("custom proxy group {group_name} not found"),
            ));
        }
        Ok(())
    }

    pub async fn set_group_now(&self, group_name: &str, member_name: &str) -> Result<(), AppError> {
        let now = now_iso();
        let strategy = serde_json::json!({ "now": member_name }).to_string();
        sqlx::query("UPDATE proxy_items SET strategy_json = ?, updated_at = ? WHERE name = ? AND kind = 'group'")
            .bind(strategy)
            .bind(now)
            .bind(group_name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_node_delay(&self, node_name: &str, delay: i64) -> Result<(), AppError> {
        let now = now_iso();
        sqlx::query("UPDATE proxy_items SET latency_ms = ?, last_test_at = ?, updated_at = ? WHERE name = ? AND kind = 'node'")
            .bind(delay)
            .bind(&now)
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

    pub async fn custom_group_names(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            "SELECT name FROM proxy_items WHERE kind = 'group' AND source = 'custom' AND enabled = 1 ORDER BY position, name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("name").map_err(AppError::from))
            .collect()
    }

    pub async fn policy_reference_count(&self, policy: &str) -> Result<i64, AppError> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS count FROM routing_rules WHERE enabled = 1 AND policy = ?",
        )
        .bind(policy)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get("count")?)
    }

    pub async fn proxy_topology(
        &self,
    ) -> Result<(Vec<ProxyGroupResponse>, Vec<ProxyNodeResponse>), AppError> {
        self.sync_builtin_proxy_group().await?;
        let groups_rows = sqlx::query(
            r#"
SELECT name, display_name, source, builtin, source_name, group_type, delay_ms, strategy_json
FROM proxy_items
WHERE kind = 'group' AND enabled = 1
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
            let members = self.group_members(&name).await?;
            let filters = self.group_filters(&name).await?;
            let strategy: String = row.try_get("strategy_json")?;
            let now = serde_json::from_str::<Value>(&strategy)
                .ok()
                .and_then(|value| value.get("now").and_then(Value::as_str).map(str::to_string))
                .or_else(|| members.first().cloned());
            groups.push(ProxyGroupResponse {
                name,
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

        let node_rows = sqlx::query(
            r#"
SELECT name, protocol, latency_ms, country, subscription_id, source_name
FROM proxy_items
WHERE kind = 'node' AND filtered_out = 0 AND enabled = 1
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
        rows.into_iter()
            .map(|row| {
                let operator: String = row.try_get("operator")?;
                let mut value: String = row.try_get("value")?;
                let values_json: String = row.try_get("values_json")?;
                let mut values: Vec<String> =
                    serde_json::from_str(&values_json).unwrap_or_default();
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
            })
            .collect()
    }

    async fn valid_node_names(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT name
FROM proxy_items
WHERE kind = 'node' AND enabled = 1 AND filtered_out = 0
ORDER BY subscription_id, position, display_name, name
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("name").map_err(AppError::from))
            .collect()
    }

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

    pub async fn all_node_records(&self) -> Result<Vec<ProxyNodeResponse>, AppError> {
        let (_, nodes) = self.proxy_topology().await?;
        Ok(nodes)
    }

    pub async fn proxy_items_for_runtime(&self) -> Result<Vec<ProxyItemRecord>, AppError> {
        self.sync_builtin_proxy_group().await?;
        let rows = sqlx::query(
            r#"
SELECT name, kind, subscription_id, display_name, source, builtin, source_name, protocol, country,
       group_type, raw_json, content_hash, latency_ms, alive, filtered_out, filter_reason,
       delay_ms, tolerance_ms, url, interval_seconds, strategy_json, position, enabled
FROM proxy_items
WHERE enabled = 1
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
        let id = id.unwrap_or_else(|| new_id("rule"));
        let now = now_iso();
        let mut tx = self.pool.begin().await?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM routing_rules WHERE id = ?)",
        )
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;

        if exists {
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
        tx.commit().await?;
        self.rule_by_id(&id).await
    }

    pub async fn update_rule(
        &self,
        id: &str,
        rule_type: &str,
        value: &str,
        policy: &str,
        desc: Option<&str>,
        enabled: bool,
    ) -> Result<RuleResponse, AppError> {
        let now = now_iso();
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
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "rule_not_found",
                format!("rule {id} not found"),
            ));
        }
        self.rule_by_id(id).await
    }

    pub async fn delete_rule(&self, id: &str) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM routing_rules WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found(
                "rule_not_found",
                format!("rule {id} not found"),
            ));
        }
        Ok(())
    }

    pub async fn list_rule_sets(&self) -> Result<Vec<RuleSetResponse>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, url, behavior, format, rule_count, last_update_at, last_error FROM rule_sets ORDER BY name",
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
                })
            })
            .collect()
    }

    pub async fn rule_sets_for_runtime(&self) -> Result<Vec<RuleSetRecord>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, url, behavior, format, local_path FROM rule_sets ORDER BY name",
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
                })
            })
            .collect()
    }

    pub async fn due_rule_set_ids(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            r#"
SELECT id
FROM rule_sets
WHERE interval_seconds > 0
  AND (
    last_update_at IS NULL
    OR CAST(strftime('%s', last_update_at) AS INTEGER) + interval_seconds
       <= CAST(strftime('%s', 'now') AS INTEGER)
  )
"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("id").map_err(AppError::from))
            .collect()
    }

    pub async fn create_rule_set(
        &self,
        id: &str,
        name: &str,
        url: &str,
        interval_seconds: u64,
        behavior: Option<&str>,
        format: &str,
    ) -> Result<(), AppError> {
        let now = now_iso();
        sqlx::query(
            r#"
INSERT INTO rule_sets(id, name, url, behavior, format, interval_seconds, created_at, updated_at)
VALUES(?, ?, ?, ?, ?, ?, ?, ?)
"#,
        )
        .bind(id)
        .bind(name)
        .bind(url)
        .bind(behavior)
        .bind(format)
        .bind(interval_seconds as i64)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_rule_set(&self, id: &str) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;
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
    ) -> Result<(), AppError> {
        let now = now_iso();
        sqlx::query(
            r#"
UPDATE rule_sets
SET local_path = ?, file_size_bytes = ?, rule_count = ?, content_hash = ?,
    format = ?, last_update_at = ?, last_error = ?, updated_at = ?
WHERE id = ?
"#,
        )
        .bind(local_path)
        .bind(file_size_bytes as i64)
        .bind(rule_count as i64)
        .bind(content_hash)
        .bind(format)
        .bind(&now)
        .bind(last_error)
        .bind(&now)
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

    async fn rule_by_id(&self, id: &str) -> Result<RuleResponse, AppError> {
        let row = sqlx::query(
            "SELECT id, position, rule_type, value, policy, source, enabled, desc FROM routing_rules WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::not_found("rule_not_found", format!("rule {id} not found")))?;
        rule_from_row(row)
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

    let filter_rows = sqlx::query("SELECT id, value, values_json FROM proxy_group_filters")
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
    })
}

#[derive(Default)]
struct SubscriptionAssetMigration {
    reference_count: usize,
    group_selections: HashMap<String, String>,
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
        let values = if rule.match_type.trim() == "in" || rule.has_values() {
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
