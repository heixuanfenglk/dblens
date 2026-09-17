use anyhow::{bail, Context, Result};
use futures::TryStreamExt;
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlRow, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgRow, PgSslMode};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions, SqliteRow};
use sqlx::{Column, ConnectOptions, Row, TypeInfo};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use crate::config::Connection;
use crate::kind::BackendKind;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode};
use crate::visual::{DEFAULT_PAGE_SIZE, MAX_RESULT_ROWS};

use super::truncate_cell;

enum SqlPool {
    Mysql(MySqlPool),
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

/// Build MySQL options without stuffing password into a URL (avoids @:#/% breakage).
fn mysql_connect_options(conn: &Connection) -> Result<MySqlConnectOptions> {
    let mut opts = if let Some(url) = conn.url.as_ref().filter(|u| !u.trim().is_empty()) {
        MySqlConnectOptions::from_str(url.trim()).context("解析 MySQL URL 失败")?
    } else {
        let host = if conn.host.trim().is_empty() {
            "127.0.0.1"
        } else {
            conn.host.trim()
        };
        let mut o = MySqlConnectOptions::new()
            .host(host)
            .port(if conn.port == 0 { 3306 } else { conn.port })
            .username(
                conn.username
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("root"),
            );
        // Pass password as-is (including empty) — never via unescaped URL.
        o = o.password(conn.password.as_deref().unwrap_or(""));
        if let Some(db) = conn
            .database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            o = o.database(db);
        }
        o
    };

    // If URL was used, still allow overriding password from the form fields
    // (URL may have been truncated at '@' or '#').
    if conn.url.as_ref().map(|u| !u.trim().is_empty()).unwrap_or(false) {
        if let Some(user) = conn
            .username
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            opts = opts.username(user);
        }
        if let Some(pass) = conn.password.as_ref() {
            opts = opts.password(pass);
        }
        if let Some(db) = conn
            .database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            opts = opts.database(db);
        }
    }

    opts = opts.charset("utf8mb4");
    // 「跳过 TLS」→ Disabled；否则 Preferred（本机 MySQL 5.7 通常也能连上）
    opts = opts.ssl_mode(if conn.insecure {
        MySqlSslMode::Disabled
    } else {
        MySqlSslMode::Preferred
    });
    opts = opts.disable_statement_logging();
    Ok(opts)
}

fn pg_connect_options(conn: &Connection) -> Result<PgConnectOptions> {
    let mut opts = if let Some(url) = conn.url.as_ref().filter(|u| !u.trim().is_empty()) {
        PgConnectOptions::from_str(url.trim()).context("解析 PostgreSQL URL 失败")?
    } else {
        let host = if conn.host.trim().is_empty() {
            "127.0.0.1"
        } else {
            conn.host.trim()
        };
        let mut o = PgConnectOptions::new()
            .host(host)
            .port(if conn.port == 0 { 5432 } else { conn.port })
            .username(
                conn.username
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("postgres"),
            )
            .password(conn.password.as_deref().unwrap_or(""));
        let db = conn
            .database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("postgres");
        o = o.database(db);
        o
    };

    if conn.url.as_ref().map(|u| !u.trim().is_empty()).unwrap_or(false) {
        if let Some(user) = conn
            .username
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            opts = opts.username(user);
        }
        if let Some(pass) = conn.password.as_ref() {
            opts = opts.password(pass);
        }
        if let Some(db) = conn
            .database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            opts = opts.database(db);
        }
    }

    opts = opts.ssl_mode(if conn.insecure {
        PgSslMode::Disable
    } else {
        PgSslMode::Prefer
    });
    opts = opts.disable_statement_logging();
    Ok(opts)
}

fn sqlite_url(conn: &Connection) -> Result<String> {
    if let Some(url) = conn.url.as_ref().filter(|u| !u.is_empty()) {
        return Ok(url.clone());
    }
    let path = conn
        .database
        .as_ref()
        .context("SQLite 需要 database 文件路径")?;
    let normalized = path.replace('\\', "/");
    let mode = if std::path::Path::new(path).exists() {
        "rw"
    } else {
        "rwc"
    };
    Ok(format!("sqlite://{normalized}?mode={mode}"))
}

async fn pool(conn: &Connection) -> Result<SqlPool> {
    match conn.kind {
        BackendKind::Mysql | BackendKind::Mariadb => {
            let label = conn.kind.display_name();
            let opts = mysql_connect_options(conn)?;
            let p = MySqlPoolOptions::new()
                .max_connections(3)
                .acquire_timeout(Duration::from_secs(10))
                .connect_with(opts)
                .await
                .map_err(|e| {
                    let hint = if e.to_string().contains("1045") {
                        "用户名或密码错误。注意：root@localhost 与 root@127.0.0.1 是不同账号；请编辑连接确认密码与主机一致。".to_string()
                    } else if e.to_string().contains("10061")
                        || e.to_string().contains("Connection refused")
                        || e.to_string().contains("timed out")
                    {
                        format!("无法连上端口，请确认 {label} 已启动且端口正确。")
                    } else {
                        "若本机无需 TLS，可在高级选项勾选「跳过 TLS / 允许不安全连接」。".to_string()
                    };
                    anyhow::anyhow!("连接 {label} 失败: {e}\n提示: {hint}")
                })?;
            Ok(SqlPool::Mysql(p))
        }
        BackendKind::Postgres => {
            let opts = pg_connect_options(conn)?;
            let p = PgPoolOptions::new()
                .max_connections(3)
                .acquire_timeout(Duration::from_secs(10))
                .connect_with(opts)
                .await
                .context("连接 PostgreSQL 失败")?;
            Ok(SqlPool::Postgres(p))
        }
        BackendKind::Sqlite => {
            let p = SqlitePoolOptions::new()
                .max_connections(3)
                .connect(&sqlite_url(conn)?)
                .await
                .context("连接 SQLite 失败")?;
            Ok(SqlPool::Sqlite(p))
        }
        _ => bail!("非 SQL 类型"),
    }
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let p = pool(conn).await?;
    match p {
        SqlPool::Mysql(p) => {
            let v: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&p).await?;
            Ok(format!(
                "{} OK (SELECT {}) — {}",
                conn.kind.display_name(),
                v.0,
                conn.endpoint_label()
            ))
        }
        SqlPool::Postgres(p) => {
            let v: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&p).await?;
            Ok(format!(
                "PostgreSQL OK (SELECT {0}) — {1}",
                v.0,
                conn.endpoint_label()
            ))
        }
        SqlPool::Sqlite(p) => {
            let v: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&p).await?;
            Ok(format!(
                "SQLite OK (SELECT {0}) — {1}",
                v.0,
                conn.endpoint_label()
            ))
        }
    }
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    let p = pool(conn).await?;
    let cat = node.meta.path.as_deref().unwrap_or("");

    match (&p, node.kind) {
        // —— MySQL ——
        (SqlPool::Mysql(_), NodeKind::Connection) => {
            let SqlPool::Mysql(pool) = &p else { unreachable!() };
            let rows = sqlx::query("SHOW DATABASES").fetch_all(pool).await?;
            Ok(rows
                .iter()
                .filter_map(|r| {
                    let name: String = r.try_get(0).ok()?;
                    Some(db_node(conn, &name))
                })
                .collect())
        }
        (SqlPool::Mysql(_), NodeKind::Database) => {
            let db = node.meta.database.as_deref().unwrap_or("");
            Ok(db_category_folders(conn, db, None))
        }
        (SqlPool::Mysql(pool), NodeKind::Folder) if cat == "nav:tables" => {
            let db = node.meta.database.as_deref().unwrap_or("");
            list_mysql_tables(pool, conn, db, false).await
        }
        (SqlPool::Mysql(pool), NodeKind::Folder) if cat == "nav:views" => {
            let db = node.meta.database.as_deref().unwrap_or("");
            list_mysql_tables(pool, conn, db, true).await
        }
        (SqlPool::Mysql(pool), NodeKind::Folder) if cat == "nav:indexes" => {
            let db = node.meta.database.as_deref().unwrap_or("");
            list_mysql_indexes(pool, conn, db, None).await
        }
        (SqlPool::Mysql(pool), NodeKind::Folder) if cat == "nav:triggers" => {
            let db = node.meta.database.as_deref().unwrap_or("");
            list_mysql_triggers(pool, conn, db, None).await
        }
        (SqlPool::Mysql(_), NodeKind::Table) => {
            Ok(table_structure_folders(conn, &node.meta))
        }
        (SqlPool::Mysql(pool), NodeKind::Folder) if cat == "nav:columns" => {
            list_mysql_columns(pool, conn, &node.meta).await
        }
        (SqlPool::Mysql(pool), NodeKind::Folder) if cat == "nav:tbl_indexes" => {
            let db = node.meta.database.as_deref().unwrap_or("");
            list_mysql_indexes(pool, conn, db, node.meta.table.as_deref()).await
        }
        (SqlPool::Mysql(pool), NodeKind::Folder) if cat == "nav:tbl_triggers" => {
            let db = node.meta.database.as_deref().unwrap_or("");
            list_mysql_triggers(pool, conn, db, node.meta.table.as_deref()).await
        }
        (SqlPool::Mysql(pool), NodeKind::Folder)
            if matches!(cat, "nav:fks" | "nav:uniques" | "nav:checks") =>
        {
            list_mysql_constraints(pool, conn, &node.meta, cat).await
        }

        // —— PostgreSQL ——
        (SqlPool::Postgres(_), NodeKind::Connection) => {
            let SqlPool::Postgres(pool) = &p else { unreachable!() };
            let rows = sqlx::query(
                "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY 1",
            )
            .fetch_all(pool)
            .await?;
            Ok(rows
                .iter()
                .filter_map(|r| {
                    let name: String = r.try_get(0).ok()?;
                    Some(db_node(conn, &name))
                })
                .collect())
        }
        (SqlPool::Postgres(_), NodeKind::Database) => {
            let db = node.meta.database.as_deref().unwrap_or("");
            Ok(db_category_folders(conn, db, None))
        }
        (SqlPool::Postgres(pool), NodeKind::Folder) if cat == "nav:tables" => {
            list_pg_relations(pool, conn, &node.meta, false).await
        }
        (SqlPool::Postgres(pool), NodeKind::Folder) if cat == "nav:views" => {
            list_pg_relations(pool, conn, &node.meta, true).await
        }
        (SqlPool::Postgres(pool), NodeKind::Folder) if cat == "nav:indexes" => {
            list_pg_indexes(pool, conn, &node.meta, None).await
        }
        (SqlPool::Postgres(pool), NodeKind::Folder) if cat == "nav:triggers" => {
            list_pg_triggers(pool, conn, &node.meta, None).await
        }
        (SqlPool::Postgres(_), NodeKind::Table) => Ok(table_structure_folders(conn, &node.meta)),
        (SqlPool::Postgres(pool), NodeKind::Folder) if cat == "nav:columns" => {
            list_pg_columns(pool, conn, &node.meta).await
        }
        (SqlPool::Postgres(pool), NodeKind::Folder) if cat == "nav:tbl_indexes" => {
            list_pg_indexes(pool, conn, &node.meta, node.meta.table.as_deref()).await
        }
        (SqlPool::Postgres(pool), NodeKind::Folder) if cat == "nav:tbl_triggers" => {
            list_pg_triggers(pool, conn, &node.meta, node.meta.table.as_deref()).await
        }
        (SqlPool::Postgres(pool), NodeKind::Folder)
            if matches!(cat, "nav:fks" | "nav:uniques" | "nav:checks") =>
        {
            list_pg_constraints(pool, conn, &node.meta, cat).await
        }

        // —— SQLite ——
        (SqlPool::Sqlite(_), NodeKind::Connection) => Ok(vec![db_node(conn, "main")]),
        (SqlPool::Sqlite(_), NodeKind::Database) => {
            let db = node.meta.database.as_deref().unwrap_or("main");
            Ok(db_category_folders(conn, db, None))
        }
        (SqlPool::Sqlite(pool), NodeKind::Folder) if cat == "nav:tables" => {
            list_sqlite_master(pool, conn, "table").await
        }
        (SqlPool::Sqlite(pool), NodeKind::Folder) if cat == "nav:views" => {
            list_sqlite_master(pool, conn, "view").await
        }
        (SqlPool::Sqlite(pool), NodeKind::Folder) if cat == "nav:indexes" => {
            list_sqlite_master(pool, conn, "index").await
        }
        (SqlPool::Sqlite(pool), NodeKind::Folder) if cat == "nav:triggers" => {
            list_sqlite_master(pool, conn, "trigger").await
        }
        (SqlPool::Sqlite(_), NodeKind::Table) => Ok(table_structure_folders(conn, &node.meta)),
        (SqlPool::Sqlite(pool), NodeKind::Folder) if cat == "nav:columns" => {
            list_sqlite_columns(pool, conn, &node.meta).await
        }
        (SqlPool::Sqlite(pool), NodeKind::Folder) if cat == "nav:tbl_indexes" => {
            list_sqlite_table_indexes(pool, conn, &node.meta).await
        }
        (SqlPool::Sqlite(pool), NodeKind::Folder) if cat == "nav:tbl_triggers" => {
            list_sqlite_table_triggers(pool, conn, &node.meta).await
        }
        (SqlPool::Sqlite(_), NodeKind::Folder)
            if matches!(cat, "nav:fks" | "nav:uniques" | "nav:checks") =>
        {
            // SQLite: expose PRAGMA foreign_key_list for fks; others may be empty
            if cat == "nav:fks" {
                let SqlPool::Sqlite(pool) = &p else { unreachable!() };
                list_sqlite_fks(pool, conn, &node.meta).await
            } else {
                Ok(Vec::new())
            }
        }

        _ => Ok(Vec::new()),
    }
}

pub async fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let sql = query.trim().trim_end_matches(';');
    if sql.is_empty() {
        bail!("SQL 为空");
    }
    let p = pool(conn).await?;
    match p {
        SqlPool::Mysql(p) => fetch_mysql(&p, sql).await,
        SqlPool::Postgres(p) => fetch_pg(&p, sql).await,
        SqlPool::Sqlite(p) => fetch_sqlite(&p, sql).await,
    }
}

pub async fn preview(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    match node.kind {
        NodeKind::Table => {
            let table = node.meta.table.as_deref().unwrap_or(&node.label);
            let limit = DEFAULT_PAGE_SIZE;
            let sql = match conn.kind {
                BackendKind::Mysql | BackendKind::Mariadb => {
                    let db = node.meta.database.as_deref().unwrap_or("");
                    format!("SELECT * FROM `{db}`.`{table}` LIMIT {limit}")
                }
                BackendKind::Postgres => {
                    let schema = node.meta.schema.as_deref().unwrap_or("public");
                    format!("SELECT * FROM \"{schema}\".\"{table}\" LIMIT {limit}")
                }
                BackendKind::Sqlite => format!("SELECT * FROM \"{table}\" LIMIT {limit}"),
                _ => bail!("不支持"),
            };
            run_query(conn, &sql).await
        }
        _ => Ok(QueryResult {
            message: "请选择表以预览数据".into(),
            ..Default::default()
        }),
    }
}

fn db_node(conn: &Connection, name: &str) -> TreeNode {
    TreeNode {
        id: format!("sql:{}:db:{name}", conn.id),
        label: name.to_string(),
        kind: NodeKind::Database,
        connection_id: conn.id.clone(),
        meta: NodeMeta {
            database: Some(name.to_string()),
            ..Default::default()
        },
        children: Vec::new(),
        expandable: true,
        loaded: false,
    }
}

fn table_node(conn: &Connection, db: &str, schema: Option<&str>, name: &str) -> TreeNode {
    let label = match schema {
        Some(s) if !s.is_empty() => format!("{s}.{name}"),
        _ => name.to_string(),
    };
    TreeNode {
        id: format!("sql:{}:tbl:{db}:{label}", conn.id),
        label: label.clone(),
        kind: NodeKind::Table,
        connection_id: conn.id.clone(),
        meta: NodeMeta {
            database: Some(db.to_string()),
            schema: schema.filter(|s| !s.is_empty()).map(|s| s.to_string()),
            table: Some(name.to_string()),
            path: Some(label),
            ..Default::default()
        },
        children: Vec::new(),
        expandable: true,
        loaded: false,
    }
}

fn view_node(conn: &Connection, db: &str, schema: Option<&str>, name: &str) -> TreeNode {
    let mut n = table_node(conn, db, schema, name);
    n.id = format!("sql:{}:view:{db}:{}", conn.id, n.label);
    n.meta.status = Some("view".into());
    n
}

fn folder_node(
    conn: &Connection,
    id_suffix: &str,
    label: &str,
    meta: NodeMeta,
) -> TreeNode {
    TreeNode {
        id: format!("sql:{}:{id_suffix}", conn.id),
        label: label.into(),
        kind: NodeKind::Folder,
        connection_id: conn.id.clone(),
        meta,
        children: Vec::new(),
        expandable: true,
        loaded: false,
    }
}

fn leaf_node(
    conn: &Connection,
    id_suffix: &str,
    label: &str,
    kind: NodeKind,
    meta: NodeMeta,
) -> TreeNode {
    TreeNode {
        id: format!("sql:{}:{id_suffix}", conn.id),
        label: label.into(),
        kind,
        connection_id: conn.id.clone(),
        meta,
        children: Vec::new(),
        expandable: false,
        loaded: true,
    }
}

fn db_category_folders(conn: &Connection, db: &str, schema: Option<&str>) -> Vec<TreeNode> {
    let base = NodeMeta {
        database: Some(db.into()),
        schema: schema.map(|s| s.into()),
        ..Default::default()
    };
    [
        ("nav:tables", "表"),
        ("nav:views", "视图"),
        ("nav:indexes", "索引"),
        ("nav:triggers", "触发器"),
    ]
    .into_iter()
    .map(|(path, label)| {
        let mut meta = base.clone();
        meta.path = Some(path.into());
        folder_node(conn, &format!("db:{db}:{path}"), label, meta)
    })
    .collect()
}

fn table_structure_folders(conn: &Connection, parent: &NodeMeta) -> Vec<TreeNode> {
    let db = parent.database.as_deref().unwrap_or("");
    let table = parent.table.as_deref().unwrap_or("");
    let key = format!(
        "{}:{}",
        db,
        parent.path.as_deref().unwrap_or(table)
    );
    [
        ("nav:columns", "字段"),
        ("nav:tbl_indexes", "索引"),
        ("nav:fks", "外键"),
        ("nav:uniques", "唯一键"),
        ("nav:checks", "检查"),
        ("nav:tbl_triggers", "触发器"),
    ]
    .into_iter()
    .map(|(path, label)| {
        let mut meta = parent.clone();
        meta.path = Some(path.into());
        folder_node(conn, &format!("tbl:{key}:{path}"), label, meta)
    })
    .collect()
}

async fn list_mysql_tables(
    pool: &MySqlPool,
    conn: &Connection,
    db: &str,
    views: bool,
) -> Result<Vec<TreeNode>> {
    let filter = if views {
        "WHERE Table_type='VIEW'"
    } else {
        "WHERE Table_type='BASE TABLE'"
    };
    // SHOW FULL TABLES is the most reliable listing (information_schema may be restricted).
    let sql = format!("SHOW FULL TABLES FROM `{db}` {filter}");
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    let counts = if views {
        HashMap::new()
    } else {
        mysql_table_row_counts(pool, db).await.unwrap_or_default()
    };
    Ok(rows
        .iter()
        .filter_map(|r| {
            let name: String = r.try_get(0).ok()?;
            Some(if views {
                view_node(conn, db, None, &name)
            } else {
                let mut node = table_node(conn, db, None, &name);
                if let Some(c) = counts.get(&name) {
                    node.meta.meta_line = Some(super::format_row_count(*c));
                }
                node
            })
        })
        .collect())
}

/// Best-effort row estimates from information_schema (InnoDB approximate).
async fn mysql_table_row_counts(pool: &MySqlPool, db: &str) -> Result<HashMap<String, i64>> {
    let rows = match sqlx::query(
        "SELECT TABLE_NAME, CAST(IFNULL(TABLE_ROWS, 0) AS SIGNED) AS cnt \
         FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'",
    )
    .bind(db)
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(_) => {
            let sql = format!(
                "SELECT TABLE_NAME, TABLE_ROWS FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA='{db}' AND TABLE_TYPE='BASE TABLE'"
            );
            sqlx::query(&sql).fetch_all(pool).await?
        }
    };

    let mut map = HashMap::with_capacity(rows.len());
    for r in &rows {
        let Ok(name) = r.try_get::<String, _>(0) else {
            continue;
        };
        if let Some(c) = mysql_cell_i64(r, 1) {
            map.insert(name, c.max(0));
        }
    }
    Ok(map)
}

fn mysql_cell_i64(row: &MySqlRow, idx: usize) -> Option<i64> {
    if let Ok(v) = row.try_get::<i64, _>(idx) {
        return Some(v);
    }
    if let Ok(v) = row.try_get::<u64, _>(idx) {
        return Some(v as i64);
    }
    if let Ok(v) = row.try_get::<i32, _>(idx) {
        return Some(i64::from(v));
    }
    if let Ok(v) = row.try_get::<u32, _>(idx) {
        return Some(i64::from(v));
    }
    if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(idx) {
        return Some(v);
    }
    if let Ok(s) = row.try_get::<String, _>(idx) {
        return s.trim().parse().ok();
    }
    if let Ok(s) = row.try_get::<Vec<u8>, _>(idx) {
        return String::from_utf8_lossy(&s).trim().parse().ok();
    }
    None
}

async fn list_mysql_indexes(
    pool: &MySqlPool,
    conn: &Connection,
    db: &str,
    table: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let sql = if let Some(t) = table {
        format!(
            "SELECT DISTINCT INDEX_NAME FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA='{db}' AND TABLE_NAME='{t}' ORDER BY 1"
        )
    } else {
        format!(
            "SELECT DISTINCT TABLE_NAME, INDEX_NAME FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA='{db}' ORDER BY 1,2"
        )
    };
    let rows = sqlx::query(&sql).fetch_all(pool).await.unwrap_or_default();
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let label = if table.is_some() {
                r.try_get::<String, _>(0).ok()?
            } else {
                let t: String = r.try_get(0).ok()?;
                let idx: String = r.try_get(1).ok()?;
                format!("{t}.{idx}")
            };
            Some(leaf_node(
                conn,
                &format!("idx:{db}:{label}:{i}"),
                &label,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    table: table.map(|t| t.into()),
                    path: Some("nav:index".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_mysql_triggers(
    pool: &MySqlPool,
    conn: &Connection,
    db: &str,
    table: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let sql = if let Some(t) = table {
        format!("SHOW TRIGGERS FROM `{db}` WHERE `Table`='{t}'")
    } else {
        format!("SHOW TRIGGERS FROM `{db}`")
    };
    let rows = sqlx::query(&sql).fetch_all(pool).await.unwrap_or_default();
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let name: String = r.try_get(0).ok()?;
            Some(leaf_node(
                conn,
                &format!("trg:{db}:{name}:{i}"),
                &name,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    table: table.map(|t| t.into()),
                    path: Some("nav:trigger".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_mysql_columns(
    pool: &MySqlPool,
    conn: &Connection,
    meta: &NodeMeta,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("");
    let table = meta.table.as_deref().unwrap_or("");
    let sql = format!("SHOW FULL COLUMNS FROM `{db}`.`{table}`");
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let name: String = r.try_get(0).ok()?;
            let ty: String = r.try_get(1).unwrap_or_default();
            let label = if ty.is_empty() {
                name.clone()
            } else {
                format!("{name}  {ty}")
            };
            let mut m = meta.clone();
            m.path = Some(name.clone());
            Some(leaf_node(
                conn,
                &format!("col:{db}:{table}:{name}:{i}"),
                &label,
                NodeKind::Column,
                m,
            ))
        })
        .collect())
}

async fn list_mysql_constraints(
    pool: &MySqlPool,
    conn: &Connection,
    meta: &NodeMeta,
    cat: &str,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("");
    let table = meta.table.as_deref().unwrap_or("");
    let (sql, path_tag) = match cat {
        "nav:fks" => (
            format!(
                "SELECT CONSTRAINT_NAME FROM information_schema.TABLE_CONSTRAINTS \
                 WHERE TABLE_SCHEMA='{db}' AND TABLE_NAME='{table}' AND CONSTRAINT_TYPE='FOREIGN KEY'"
            ),
            "nav:fk",
        ),
        "nav:uniques" => (
            format!(
                "SELECT CONSTRAINT_NAME FROM information_schema.TABLE_CONSTRAINTS \
                 WHERE TABLE_SCHEMA='{db}' AND TABLE_NAME='{table}' AND CONSTRAINT_TYPE='UNIQUE'"
            ),
            "nav:unique",
        ),
        _ => (
            format!(
                "SELECT CONSTRAINT_NAME FROM information_schema.TABLE_CONSTRAINTS \
                 WHERE TABLE_SCHEMA='{db}' AND TABLE_NAME='{table}' AND CONSTRAINT_TYPE='CHECK'"
            ),
            "nav:check",
        ),
    };
    let rows = sqlx::query(&sql).fetch_all(pool).await.unwrap_or_default();
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let name: String = r.try_get(0).ok()?;
            Some(leaf_node(
                conn,
                &format!("cst:{db}:{table}:{name}:{i}"),
                &name,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    table: Some(table.into()),
                    path: Some(path_tag.into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_pg_relations(
    pool: &PgPool,
    conn: &Connection,
    meta: &NodeMeta,
    views: bool,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("");
    if views {
        let rows = sqlx::query(
            "SELECT table_schema, table_name FROM information_schema.tables \
             WHERE table_type='VIEW' AND table_schema NOT IN ('pg_catalog','information_schema') \
             ORDER BY 1,2",
        )
        .fetch_all(pool)
        .await?;
        return Ok(rows
            .iter()
            .filter_map(|r| {
                let schema: String = r.try_get(0).ok()?;
                let name: String = r.try_get(1).ok()?;
                Some(view_node(conn, db, Some(&schema), &name))
            })
            .collect());
    }
    // reltuples is a planner estimate (updated by ANALYZE / autovacuum).
    let rows = sqlx::query(
        "SELECT n.nspname, c.relname, \
            CASE WHEN c.reltuples < 0 THEN 0 ELSE ROUND(c.reltuples)::bigint END AS row_est \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind = 'r' \
           AND n.nspname NOT IN ('pg_catalog','information_schema') \
         ORDER BY 1,2",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let schema: String = r.try_get(0).ok()?;
            let name: String = r.try_get(1).ok()?;
            let mut node = table_node(conn, db, Some(&schema), &name);
            if let Ok(c) = r.try_get::<i64, _>(2) {
                node.meta.meta_line = Some(super::format_row_count(c.max(0)));
            }
            Some(node)
        })
        .collect())
}

async fn list_pg_indexes(
    pool: &PgPool,
    conn: &Connection,
    meta: &NodeMeta,
    table: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("");
    let rows = if let Some(t) = table {
        let schema = meta.schema.as_deref().unwrap_or("public");
        sqlx::query(
            "SELECT indexname FROM pg_indexes WHERE schemaname=$1 AND tablename=$2 ORDER BY 1",
        )
        .bind(schema)
        .bind(t)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
    } else {
        sqlx::query(
            "SELECT schemaname, tablename, indexname FROM pg_indexes \
             WHERE schemaname NOT IN ('pg_catalog','information_schema') ORDER BY 1,2,3",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default()
    };
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let label = if table.is_some() {
                r.try_get::<String, _>(0).ok()?
            } else {
                let s: String = r.try_get(0).ok()?;
                let t: String = r.try_get(1).ok()?;
                let idx: String = r.try_get(2).ok()?;
                format!("{s}.{t}.{idx}")
            };
            Some(leaf_node(
                conn,
                &format!("pgidx:{db}:{label}:{i}"),
                &label,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    path: Some("nav:index".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_pg_triggers(
    pool: &PgPool,
    conn: &Connection,
    meta: &NodeMeta,
    table: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("");
    let rows = if let Some(t) = table {
        let schema = meta.schema.as_deref().unwrap_or("public");
        sqlx::query(
            "SELECT tgname FROM pg_trigger t \
             JOIN pg_class c ON t.tgrelid=c.oid \
             JOIN pg_namespace n ON c.relnamespace=n.oid \
             WHERE NOT t.tgisinternal AND n.nspname=$1 AND c.relname=$2 ORDER BY 1",
        )
        .bind(schema)
        .bind(t)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
    } else {
        sqlx::query(
            "SELECT n.nspname, c.relname, tgname FROM pg_trigger t \
             JOIN pg_class c ON t.tgrelid=c.oid \
             JOIN pg_namespace n ON c.relnamespace=n.oid \
             WHERE NOT t.tgisinternal AND n.nspname NOT IN ('pg_catalog','information_schema') \
             ORDER BY 1,2,3",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default()
    };
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let label = if table.is_some() {
                r.try_get::<String, _>(0).ok()?
            } else {
                let s: String = r.try_get(0).ok()?;
                let t: String = r.try_get(1).ok()?;
                let tg: String = r.try_get(2).ok()?;
                format!("{s}.{t}.{tg}")
            };
            Some(leaf_node(
                conn,
                &format!("pgtrg:{db}:{label}:{i}"),
                &label,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    path: Some("nav:trigger".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_pg_columns(
    pool: &PgPool,
    conn: &Connection,
    meta: &NodeMeta,
) -> Result<Vec<TreeNode>> {
    let schema = meta.schema.as_deref().unwrap_or("public");
    let table = meta.table.as_deref().unwrap_or("");
    let rows = sqlx::query(
        "SELECT column_name, data_type FROM information_schema.columns \
         WHERE table_schema=$1 AND table_name=$2 ORDER BY ordinal_position",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let name: String = r.try_get(0).ok()?;
            let ty: String = r.try_get(1).unwrap_or_default();
            let label = format!("{name}  {ty}");
            let mut m = meta.clone();
            m.path = Some(name.clone());
            Some(leaf_node(
                conn,
                &format!("pgcol:{schema}:{table}:{name}:{i}"),
                &label,
                NodeKind::Column,
                m,
            ))
        })
        .collect())
}

async fn list_pg_constraints(
    pool: &PgPool,
    conn: &Connection,
    meta: &NodeMeta,
    cat: &str,
) -> Result<Vec<TreeNode>> {
    let schema = meta.schema.as_deref().unwrap_or("public");
    let table = meta.table.as_deref().unwrap_or("");
    let typ = match cat {
        "nav:fks" => "FOREIGN KEY",
        "nav:uniques" => "UNIQUE",
        _ => "CHECK",
    };
    let rows = sqlx::query(
        "SELECT constraint_name FROM information_schema.table_constraints \
         WHERE table_schema=$1 AND table_name=$2 AND constraint_type=$3 ORDER BY 1",
    )
    .bind(schema)
    .bind(table)
    .bind(typ)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let name: String = r.try_get(0).ok()?;
            Some(leaf_node(
                conn,
                &format!("pgcst:{schema}:{table}:{name}:{i}"),
                &name,
                NodeKind::Key,
                NodeMeta {
                    schema: Some(schema.into()),
                    table: Some(table.into()),
                    path: Some(cat.into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_sqlite_master(
    pool: &SqlitePool,
    conn: &Connection,
    typ: &str,
) -> Result<Vec<TreeNode>> {
    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type=? AND name NOT LIKE 'sqlite_%' ORDER BY 1",
    )
    .bind(typ)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let name: String = match r.try_get(0) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let node = match typ {
            "view" => view_node(conn, "main", None, &name),
            "table" => {
                let mut n = table_node(conn, "main", None, &name);
                let q = format!("SELECT COUNT(*) FROM \"{}\"", name.replace('"', "\"\""));
                if let Ok(c) = sqlx::query_scalar::<_, i64>(&q).fetch_one(pool).await {
                    n.meta.meta_line = Some(super::format_row_count(c.max(0)));
                }
                n
            }
            _ => leaf_node(
                conn,
                &format!("sqlite:{typ}:{name}"),
                &name,
                NodeKind::Key,
                NodeMeta {
                    database: Some("main".into()),
                    path: Some(format!("nav:{typ}")),
                    ..Default::default()
                },
            ),
        };
        out.push(node);
    }
    Ok(out)
}

async fn list_sqlite_columns(
    pool: &SqlitePool,
    conn: &Connection,
    meta: &NodeMeta,
) -> Result<Vec<TreeNode>> {
    let table = meta.table.as_deref().unwrap_or("");
    let rows = sqlx::query(&format!("PRAGMA table_info(\"{table}\")"))
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let name: String = r.try_get(1).ok()?;
            let ty: String = r.try_get(2).unwrap_or_default();
            let label = if ty.is_empty() {
                name.clone()
            } else {
                format!("{name}  {ty}")
            };
            let mut m = meta.clone();
            m.path = Some(name.clone());
            Some(leaf_node(
                conn,
                &format!("sqlitecol:{table}:{name}:{i}"),
                &label,
                NodeKind::Column,
                m,
            ))
        })
        .collect())
}

async fn list_sqlite_table_indexes(
    pool: &SqlitePool,
    conn: &Connection,
    meta: &NodeMeta,
) -> Result<Vec<TreeNode>> {
    let table = meta.table.as_deref().unwrap_or("");
    let rows = sqlx::query(&format!("PRAGMA index_list(\"{table}\")"))
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let name: String = r.try_get(1).ok()?;
            Some(leaf_node(
                conn,
                &format!("sqliteidx:{table}:{name}:{i}"),
                &name,
                NodeKind::Key,
                NodeMeta {
                    database: Some("main".into()),
                    table: Some(table.into()),
                    path: Some("nav:index".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_sqlite_table_triggers(
    pool: &SqlitePool,
    conn: &Connection,
    meta: &NodeMeta,
) -> Result<Vec<TreeNode>> {
    let table = meta.table.as_deref().unwrap_or("");
    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type='trigger' AND tbl_name=? ORDER BY 1",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let name: String = r.try_get(0).ok()?;
            Some(leaf_node(
                conn,
                &format!("sqlitetrg:{table}:{name}:{i}"),
                &name,
                NodeKind::Key,
                NodeMeta {
                    database: Some("main".into()),
                    table: Some(table.into()),
                    path: Some("nav:trigger".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_sqlite_fks(
    pool: &SqlitePool,
    conn: &Connection,
    meta: &NodeMeta,
) -> Result<Vec<TreeNode>> {
    let table = meta.table.as_deref().unwrap_or("");
    let rows = sqlx::query(&format!("PRAGMA foreign_key_list(\"{table}\")"))
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let to_table: String = r.try_get(2).ok()?;
            let from_col: String = r.try_get(3).unwrap_or_default();
            let to_col: String = r.try_get(4).unwrap_or_default();
            let label = format!("{from_col} → {to_table}.{to_col}");
            Some(leaf_node(
                conn,
                &format!("sqlitefk:{table}:{i}"),
                &label,
                NodeKind::Key,
                NodeMeta {
                    database: Some("main".into()),
                    table: Some(table.into()),
                    path: Some("nav:fk".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn fetch_mysql(pool: &MySqlPool, sql: &str) -> Result<QueryResult> {
    if is_select(sql) {
        let mut stream = sqlx::query(sql).fetch(pool);
        let mut columns: Vec<String> = Vec::new();
        let mut column_types: Vec<String> = Vec::new();
        let mut data: Vec<Vec<String>> = Vec::new();
        let mut truncated = false;
        while let Some(row) = stream.try_next().await.context("执行 SQL 失败")? {
            if columns.is_empty() {
                columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                column_types = row
                    .columns()
                    .iter()
                    .map(|c| c.type_info().name().to_string())
                    .collect();
            }
            data.push(
                columns
                    .iter()
                    .enumerate()
                    .map(|(i, _)| mysql_cell(&row, i))
                    .collect(),
            );
            if data.len() >= MAX_RESULT_ROWS {
                truncated = true;
                break;
            }
        }
        Ok(rows_to_result(columns, column_types, data, truncated))
    } else {
        let r = sqlx::query(sql).execute(pool).await.context("执行 SQL 失败")?;
        Ok(QueryResult {
            message: format!("OK, affected={}", r.rows_affected()),
            affected: Some(r.rows_affected()),
            ..Default::default()
        })
    }
}

async fn fetch_pg(pool: &PgPool, sql: &str) -> Result<QueryResult> {
    if is_select(sql) {
        let mut stream = sqlx::query(sql).fetch(pool);
        let mut columns: Vec<String> = Vec::new();
        let mut column_types: Vec<String> = Vec::new();
        let mut data: Vec<Vec<String>> = Vec::new();
        let mut truncated = false;
        while let Some(row) = stream.try_next().await.context("执行 SQL 失败")? {
            if columns.is_empty() {
                columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                column_types = row
                    .columns()
                    .iter()
                    .map(|c| c.type_info().name().to_string())
                    .collect();
            }
            data.push((0..columns.len()).map(|i| pg_cell(&row, i)).collect());
            if data.len() >= MAX_RESULT_ROWS {
                truncated = true;
                break;
            }
        }
        Ok(rows_to_result(columns, column_types, data, truncated))
    } else {
        let r = sqlx::query(sql).execute(pool).await.context("执行 SQL 失败")?;
        Ok(QueryResult {
            message: format!("OK, affected={}", r.rows_affected()),
            affected: Some(r.rows_affected()),
            ..Default::default()
        })
    }
}

async fn fetch_sqlite(pool: &SqlitePool, sql: &str) -> Result<QueryResult> {
    if is_select(sql) {
        let mut stream = sqlx::query(sql).fetch(pool);
        let mut columns: Vec<String> = Vec::new();
        let mut column_types: Vec<String> = Vec::new();
        let mut data: Vec<Vec<String>> = Vec::new();
        let mut truncated = false;
        while let Some(row) = stream.try_next().await.context("执行 SQL 失败")? {
            if columns.is_empty() {
                columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                column_types = row
                    .columns()
                    .iter()
                    .map(|c| c.type_info().name().to_string())
                    .collect();
            }
            data.push((0..columns.len()).map(|i| sqlite_cell(&row, i)).collect());
            if data.len() >= MAX_RESULT_ROWS {
                truncated = true;
                break;
            }
        }
        Ok(rows_to_result(columns, column_types, data, truncated))
    } else {
        let r = sqlx::query(sql).execute(pool).await.context("执行 SQL 失败")?;
        Ok(QueryResult {
            message: format!("OK, affected={}", r.rows_affected()),
            affected: Some(r.rows_affected()),
            ..Default::default()
        })
    }
}

fn rows_to_result(
    columns: Vec<String>,
    column_types: Vec<String>,
    data: Vec<Vec<String>>,
    truncated: bool,
) -> QueryResult {
    if columns.is_empty() && data.is_empty() {
        return QueryResult {
            message: "0 rows".into(),
            ..Default::default()
        };
    }
    let n = data.len();
    let message = if truncated {
        format!("{n}+ rows (已截断)")
    } else {
        format!("{n} rows")
    };
    QueryResult {
        message,
        columns,
        column_types,
        rows: data,
        truncated,
        ..Default::default()
    }
}

fn is_select(sql: &str) -> bool {
    let t = sql.trim_start().to_ascii_lowercase();
    t.starts_with("select")
        || t.starts_with("show")
        || t.starts_with("describe")
        || t.starts_with("desc ")
        || t.starts_with("explain")
        || t.starts_with("pragma")
        || t.starts_with("with")
}

fn mysql_cell(row: &MySqlRow, i: usize) -> String {
    if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        return truncate_cell(&v.unwrap_or_default(), 500);
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v.map(|x| x.to_string()).unwrap_or_default();
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v.map(|x| x.to_string()).unwrap_or_default();
    }
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(i) {
        return v
            .map(|b| truncate_cell(&String::from_utf8_lossy(&b), 200))
            .unwrap_or_default();
    }
    "(?)".into()
}

fn pg_cell(row: &PgRow, i: usize) -> String {
    if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        return truncate_cell(&v.unwrap_or_default(), 500);
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v.map(|x| x.to_string()).unwrap_or_default();
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v.map(|x| x.to_string()).unwrap_or_default();
    }
    if let Ok(v) = row.try_get::<Option<bool>, _>(i) {
        return v.map(|x| x.to_string()).unwrap_or_default();
    }
    "(?)".into()
}

fn sqlite_cell(row: &SqliteRow, i: usize) -> String {
    if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        return truncate_cell(&v.unwrap_or_default(), 500);
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v.map(|x| x.to_string()).unwrap_or_default();
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v.map(|x| x.to_string()).unwrap_or_default();
    }
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(i) {
        return v
            .map(|b| truncate_cell(&String::from_utf8_lossy(&b), 200))
            .unwrap_or_default();
    }
    "(?)".into()
}
