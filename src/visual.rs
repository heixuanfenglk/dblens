//! Unified visual browse helpers for all component types.

use crate::kind::BackendKind;
use crate::models::NodeMeta;

/// Default rows fetched/shown per page in visual browse.
pub const DEFAULT_PAGE_SIZE: u32 = 25;
/// Hard cap for user-selectable page size (keeps UI responsive).
pub const MAX_PAGE_SIZE: u32 = 200;
/// Absolute max rows kept in a QueryResult (defense in depth).
pub const MAX_RESULT_ROWS: usize = 500;
/// Truncate cell text when painting the grid (detail pane keeps full text).
pub const CELL_DISPLAY_MAX: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Visual,
    Query,
}

/// Panel tabs — labels adapt per backend family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisualPanel {
    #[default]
    Data,
    Structure,
    Info,
}

impl VisualPanel {
    pub fn label(self, kind: BackendKind) -> &'static str {
        match (kind, self) {
            (BackendKind::Elasticsearch, Self::Data) => "文档",
            (BackendKind::Elasticsearch, Self::Structure) => "Mapping",
            (BackendKind::Elasticsearch, Self::Info) => "概览",
            (BackendKind::Mongodb, Self::Data) => "文档",
            (BackendKind::Mongodb, Self::Structure) => "索引",
            (BackendKind::Mongodb, Self::Info) => "概览",
            (k, Self::Data) if k.is_sql() => "数据",
            (k, Self::Structure) if k.is_sql() => "结构",
            (k, Self::Info) if k.is_sql() => "信息",
            (BackendKind::Redis | BackendKind::Memcached | BackendKind::Etcd, Self::Data) => {
                "键值"
            }
            (BackendKind::Redis | BackendKind::Memcached | BackendKind::Etcd, Self::Structure) => {
                "扫描"
            }
            (BackendKind::Redis | BackendKind::Memcached | BackendKind::Etcd, Self::Info) => {
                "信息"
            }
            (_, Self::Data) => "数据",
            (_, Self::Structure) => "结构",
            (_, Self::Info) => "信息",
        }
    }

    pub fn filter_hint(self, kind: BackendKind) -> &'static str {
        match (kind, self) {
            (BackendKind::Elasticsearch, Self::Data) => "关键词或 field:value，留空=全部",
            (BackendKind::Mongodb, Self::Data) => "JSON filter，如 {\"name\":\"a\"}，留空={}",
            (k, Self::Data) if k.is_sql() => "WHERE 条件，如 id>1 或 name LIKE '%x%'，留空=全部",
            (BackendKind::Redis, _) => "MATCH 模式，如 user:* ，留空=*",
            (BackendKind::Memcached, Self::Data) => "键名，或 STATS",
            (BackendKind::Etcd, _) => "键前缀，如 /config/",
            _ => "筛选（可选）",
        }
    }

    pub fn from_es_schema(schema: Option<&str>) -> Self {
        match schema {
            Some("mapping") => Self::Structure,
            Some("settings") | Some("stats") | Some("health") | Some("info") | Some("nodes")
            | Some("shards") | Some("templates") | Some("aliases") => Self::Info,
            _ => Self::Data,
        }
    }
}

pub fn object_title(conn_name: &str, kind: BackendKind, meta: &NodeMeta, fallback: &str) -> String {
    if let Some(name) = primary_object(kind, meta) {
        format!("{conn_name} / {name}")
    } else {
        format!("{conn_name} / {fallback}")
    }
}

pub fn primary_object(kind: BackendKind, meta: &NodeMeta) -> Option<String> {
    match kind {
        BackendKind::Elasticsearch => {
            // Cluster admin views are not index names.
            if meta.path.as_deref() == Some("__cluster__")
                || meta.path.as_deref() == Some("__indices__")
                || meta.path.as_deref() == Some("__sysindices__")
            {
                return None;
            }
            meta.path
                .as_ref()
                .or(meta.table.as_ref())
                .or(meta.database.as_ref())
                .filter(|p| !p.starts_with("__"))
                .cloned()
        }
        BackendKind::Mongodb => meta.table.clone().or(meta.path.clone()),
        k if k.is_sql() => meta.table.clone().or_else(|| {
            meta.path
                .clone()
                .filter(|p| !p.starts_with("nav:"))
        }),
        BackendKind::Redis | BackendKind::Memcached | BackendKind::Etcd => meta.path.clone(),
        _ => meta.path.clone().or(meta.table.clone()),
    }
}

/// Display label for the visual header (Chinese cluster item names, index names, …).
pub fn display_object_label(kind: BackendKind, meta: &NodeMeta, fallback: &str) -> String {
    if kind == BackendKind::Elasticsearch && meta.path.as_deref() == Some("__cluster__") {
        return match meta.schema.as_deref() {
            Some("health") => "集群健康".into(),
            Some("info") => "集群信息".into(),
            Some("nodes") => "节点".into(),
            Some("shards") => "分片".into(),
            Some("aliases") => "别名".into(),
            Some("templates") => "模板".into(),
            Some("cluster") => "集群".into(),
            _ => fallback.to_string(),
        };
    }
    primary_object(kind, meta).unwrap_or_else(|| fallback.to_string())
}

/// Build the query string executed behind the visual UI.
pub fn build_query(
    kind: BackendKind,
    panel: VisualPanel,
    meta: &NodeMeta,
    filter: &str,
    page_size: u32,
    page_from: u32,
) -> String {
    let size = page_size.clamp(1, MAX_PAGE_SIZE);
    let from = page_from.min(100_000);
    match kind {
        BackendKind::Elasticsearch => build_es(panel, meta, filter, size, from),
        BackendKind::Mongodb => build_mongo(panel, meta, filter, size, from),
        BackendKind::Mysql | BackendKind::Mariadb => build_mysql(panel, meta, filter, size, from),
        BackendKind::Postgres => build_postgres(panel, meta, filter, size, from),
        BackendKind::Sqlite => build_sqlite(panel, meta, filter, size, from),
        BackendKind::Mssql => build_mssql(panel, meta, filter, size, from),
        BackendKind::Oracle => build_oracle(panel, meta, filter, size, from),
        BackendKind::Snowflake => build_snowflake(panel, meta, filter, size, from),
        BackendKind::Clickhouse => build_clickhouse(panel, meta, filter, size, from),
        BackendKind::Redis => build_redis(panel, meta, filter, size),
        BackendKind::Memcached => build_memcached(panel, filter),
        BackendKind::Etcd => build_etcd(panel, meta, filter, size, from),
    }
}

fn build_es(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    // Cluster navigation items (健康 / 信息 / 节点 / …) — use the correct API each time.
    if is_es_cluster_action(meta) {
        return es_cluster_query(meta.schema.as_deref().unwrap_or("health"));
    }
    let index = primary_object(BackendKind::Elasticsearch, meta).unwrap_or_else(|| "_all".into());
    let action = meta.schema.as_deref().unwrap_or("");
    match (panel, action) {
        (_, "mapping") | (VisualPanel::Structure, _) => format!("GET /{index}/_mapping"),
        (_, "settings") => format!("GET /{index}/_settings"),
        (_, "aliases") => format!("GET /{index}/_alias"),
        (_, "stats") | (VisualPanel::Info, _) => format!("GET /{index}/_stats"),
        _ => {
            let f = filter.trim();
            if f.is_empty() {
                format!(
                    r#"{{"query":{{"match_all":{{}}}},"size":{size},"from":{from},"track_total_hits":true}}"#
                )
            } else {
                let escaped = json_escape(f);
                format!(
                    r#"{{"query":{{"query_string":{{"query":"{escaped}","default_operator":"AND"}}}},"size":{size},"from":{from},"track_total_hits":true}}"#
                )
            }
        }
    }
}

fn is_es_cluster_action(meta: &NodeMeta) -> bool {
    meta.path.as_deref() == Some("__cluster__")
        || (meta.schema.as_deref() == Some("cluster")
            && meta.path.as_deref().is_none_or(|p| p.starts_with("__")))
}

fn es_cluster_query(action: &str) -> String {
    match action {
        "info" => "GET /".into(),
        "nodes" => {
            "GET /_cat/nodes?format=json&h=name,ip,heap.percent,ram.percent,cpu,load_1m,node.role,master"
                .into()
        }
        "shards" => {
            "GET /_cat/shards?format=json&h=index,shard,prirep,state,docs,store,ip,node".into()
        }
        "aliases" => "GET /_cat/aliases?format=json&h=alias,index,filter,routing.index,routing.search".into(),
        "templates" => "GET /_index_template".into(),
        "cluster" | "health" | _ => "GET /_cluster/health".into(),
    }
}

fn build_mongo(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let coll = primary_object(BackendKind::Mongodb, meta).unwrap_or_else(|| "collection".into());
    match panel {
        VisualPanel::Data => {
            let filter_json = normalize_mongo_filter(filter);
            format!(
                "{{\n  \"find\": \"{coll}\",\n  \"filter\": {filter_json},\n  \"skip\": {from},\n  \"limit\": {size}\n}}"
            )
        }
        VisualPanel::Structure => {
            // listIndexes via command
            format!("{{\n  \"listIndexes\": \"{coll}\"\n}}")
        }
        VisualPanel::Info => {
            format!("{{\n  \"collStats\": \"{coll}\"\n}}")
        }
    }
}

fn normalize_mongo_filter(filter: &str) -> String {
    let f = filter.trim();
    if f.is_empty() {
        return "{}".into();
    }
    if f.starts_with('{') {
        f.to_string()
    } else {
        // treat as _id equality string
        format!("{{\"_id\":\"{}\"}}", json_escape(f))
    }
}

fn build_mysql(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let table = meta.table.as_deref().unwrap_or("table");
    let db = meta.database.as_deref().unwrap_or("");
    let qualified = if db.is_empty() {
        format!("`{table}`")
    } else {
        format!("`{db}`.`{table}`")
    };
    match panel {
        VisualPanel::Data => {
            let where_c = sql_where(filter);
            format!("SELECT * FROM {qualified}{where_c} LIMIT {size} OFFSET {from}")
        }
        VisualPanel::Structure => format!("SHOW FULL COLUMNS FROM {qualified}"),
        VisualPanel::Info => format!(
            "SELECT TABLE_NAME, ENGINE, TABLE_ROWS, DATA_LENGTH, INDEX_LENGTH, CREATE_TIME \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA='{db}' AND TABLE_NAME='{table}'"
        ),
    }
}

fn build_postgres(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let table = meta.table.as_deref().unwrap_or("table");
    let schema = meta.schema.as_deref().unwrap_or("public");
    let qualified = format!("\"{schema}\".\"{table}\"");
    match panel {
        VisualPanel::Data => {
            let where_c = sql_where(filter);
            format!("SELECT * FROM {qualified}{where_c} LIMIT {size} OFFSET {from}")
        }
        VisualPanel::Structure => format!(
            "SELECT column_name, data_type, is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema='{schema}' AND table_name='{table}' ORDER BY ordinal_position"
        ),
        VisualPanel::Info => format!(
            "SELECT schemaname, relname, n_live_tup, n_dead_tup, last_vacuum, last_analyze \
             FROM pg_stat_user_tables WHERE schemaname='{schema}' AND relname='{table}'"
        ),
    }
}

fn build_sqlite(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let table = meta.table.as_deref().unwrap_or("table");
    match panel {
        VisualPanel::Data => {
            let where_c = sql_where(filter);
            format!("SELECT * FROM \"{table}\"{where_c} LIMIT {size} OFFSET {from}")
        }
        VisualPanel::Structure => format!("PRAGMA table_info(\"{table}\")"),
        VisualPanel::Info => {
            format!("SELECT name, sql FROM sqlite_master WHERE type='table' AND name='{table}'")
        }
    }
}

fn build_mssql(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let table = meta.table.as_deref().unwrap_or("table");
    let schema = meta.schema.as_deref().unwrap_or("dbo");
    let db = meta.database.as_deref().unwrap_or("master");
    let qualified = format!("[{db}].[{schema}].[{table}]");
    match panel {
        VisualPanel::Data => {
            let where_c = sql_where(filter);
            // OFFSET/FETCH requires ORDER BY
            format!(
                "SELECT * FROM {qualified}{where_c} ORDER BY (SELECT NULL) \
                 OFFSET {from} ROWS FETCH NEXT {size} ROWS ONLY"
            )
        }
        VisualPanel::Structure => format!(
            "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE, CHARACTER_MAXIMUM_LENGTH \
             FROM [{db}].INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA=N'{schema}' AND TABLE_NAME=N'{table}' ORDER BY ORDINAL_POSITION"
        ),
        VisualPanel::Info => format!(
            "SELECT t.name AS table_name, SUM(p.[rows]) AS row_count \
             FROM [{db}].sys.tables t \
             JOIN [{db}].sys.schemas s ON t.schema_id=s.schema_id \
             JOIN [{db}].sys.partitions p ON t.object_id=p.object_id \
             WHERE s.name=N'{schema}' AND t.name=N'{table}' AND p.index_id IN (0,1) \
             GROUP BY t.name"
        ),
    }
}

fn build_oracle(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let table = meta.table.as_deref().unwrap_or("table");
    let schema = meta.schema.as_deref().or(meta.database.as_deref()).unwrap_or("USER");
    let qualified = format!("\"{schema}\".\"{table}\"");
    match panel {
        VisualPanel::Data => {
            let where_c = sql_where(filter);
            format!(
                "SELECT * FROM {qualified}{where_c} \
                 OFFSET {from} ROWS FETCH NEXT {size} ROWS ONLY"
            )
        }
        VisualPanel::Structure => format!(
            "SELECT column_name, data_type, nullable, data_default \
             FROM all_tab_columns WHERE owner='{schema}' AND table_name='{table}' \
             ORDER BY column_id"
        ),
        VisualPanel::Info => format!(
            "SELECT owner, table_name, tablespace_name, num_rows, last_analyzed \
             FROM all_tables WHERE owner='{schema}' AND table_name='{table}'"
        ),
    }
}

fn build_snowflake(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let table = meta.table.as_deref().unwrap_or("table");
    let schema = meta.schema.as_deref().unwrap_or("PUBLIC");
    let db = meta.database.as_deref().unwrap_or("DATABASE");
    let qualified = format!("\"{db}\".\"{schema}\".\"{table}\"");
    match panel {
        VisualPanel::Data => {
            let where_c = sql_where(filter);
            format!("SELECT * FROM {qualified}{where_c} LIMIT {size} OFFSET {from}")
        }
        VisualPanel::Structure => format!("DESCRIBE TABLE {qualified}"),
        VisualPanel::Info => format!(
            "SHOW TABLES LIKE '{table}' IN SCHEMA \"{db}\".\"{schema}\""
        ),
    }
}

fn build_clickhouse(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let table = meta.table.as_deref().unwrap_or("table");
    let db = meta.database.as_deref().unwrap_or("default");
    let qualified = format!("`{db}`.`{table}`");
    match panel {
        VisualPanel::Data => {
            let where_c = sql_where(filter);
            format!("SELECT * FROM {qualified}{where_c} LIMIT {size} OFFSET {from}")
        }
        VisualPanel::Structure => format!(
            "SELECT name, type, default_kind, default_expression \
             FROM system.columns WHERE database='{db}' AND table='{table}' ORDER BY position"
        ),
        VisualPanel::Info => format!(
            "SELECT name, engine, total_rows, total_bytes, metadata_modification_time \
             FROM system.tables WHERE database='{db}' AND name='{table}'"
        ),
    }
}

fn sql_where(filter: &str) -> String {
    let f = filter.trim();
    if f.is_empty() {
        String::new()
    } else if f.to_ascii_lowercase().starts_with("where ") {
        format!(" {f}")
    } else {
        format!(" WHERE {f}")
    }
}

fn build_redis(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32) -> String {
    match panel {
        VisualPanel::Data => {
            if let Some(key) = meta.path.as_ref().filter(|k| !k.is_empty()) {
                // single key selected
                format!("TYPE {key}")
            } else {
                let pattern = if filter.trim().is_empty() {
                    "*"
                } else {
                    filter.trim()
                };
                format!("SCAN 0 MATCH {pattern} COUNT {size}")
            }
        }
        VisualPanel::Structure => {
            let pattern = if filter.trim().is_empty() {
                "*"
            } else {
                filter.trim()
            };
            format!("SCAN 0 MATCH {pattern} COUNT {size}")
        }
        VisualPanel::Info => "INFO keyspace".into(),
    }
}

fn build_memcached(panel: VisualPanel, filter: &str) -> String {
    match panel {
        VisualPanel::Data => {
            let f = filter.trim();
            if f.is_empty() || f.eq_ignore_ascii_case("stats") {
                "STATS".into()
            } else {
                format!("GET {f}")
            }
        }
        VisualPanel::Structure | VisualPanel::Info => "STATS".into(),
    }
}

fn build_etcd(panel: VisualPanel, meta: &NodeMeta, filter: &str, size: u32, from: u32) -> String {
    let prefix = match panel {
        VisualPanel::Data | VisualPanel::Structure => {
            if !filter.trim().is_empty() {
                filter.trim().to_string()
            } else if let Some(p) = &meta.path {
                p.clone()
            } else {
                "/".into()
            }
        }
        VisualPanel::Info => "/".into(),
    };
    format!("# limit={size}\n# from={from}\n{prefix}")
}

/// Shorten cell text for dense grid painting (keeps layout cheap).
pub fn display_cell(s: &str) -> String {
    let t = s.replace('\n', "↵").replace('\r', "");
    let max = CELL_DISPLAY_MAX;
    if t.chars().count() <= max {
        t
    } else {
        let cut: String = t.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// 1-based page index from offset + size.
pub fn page_number(page_from: u32, page_size: u32) -> u32 {
    let size = page_size.max(1);
    page_from / size + 1
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn row_detail(columns: &[String], row: &[String]) -> String {
    let mut lines = Vec::new();
    for (i, col) in columns.iter().enumerate() {
        let val = row.get(i).map(|s| s.as_str()).unwrap_or("");
        lines.push(format!("{col}: {val}"));
    }
    lines.join("\n")
}

/// Whether opening this object should default to visual browse.
pub fn prefer_visual(kind: BackendKind, meta: &NodeMeta) -> bool {
    match kind {
        BackendKind::Elasticsearch => {
            primary_object(kind, meta).is_some()
                || meta.schema.as_deref() == Some("cluster")
                || meta.path.as_deref() == Some("__cluster__")
        }
        BackendKind::Mongodb => meta.table.is_some() || meta.path.is_some(),
        k if k.is_sql() => meta.table.is_some(),
        BackendKind::Redis | BackendKind::Memcached | BackendKind::Etcd => true,
        _ => true,
    }
}
