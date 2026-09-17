mod clickhouse;
mod elasticsearch;
mod etcd;
mod memcached;
mod mongodb;
mod mssql;
mod oracle;
mod redis;
mod snowflake;
mod sql;

use std::time::Instant;

use anyhow::Result;

use crate::config::Connection;
use crate::kind::BackendKind;
use crate::models::{NodeMeta, QueryResult, TreeNode, ValueView};
use crate::visual::MAX_RESULT_ROWS;

pub async fn test_connection(conn: &Connection) -> Result<String> {
    match conn.kind {
        BackendKind::Redis => redis::ping(conn).await,
        BackendKind::Memcached => memcached::ping(conn).await,
        BackendKind::Elasticsearch => elasticsearch::ping(conn).await,
        BackendKind::Etcd => etcd::ping(conn).await,
        BackendKind::Mysql
        | BackendKind::Mariadb
        | BackendKind::Postgres
        | BackendKind::Sqlite => sql::ping(conn).await,
        BackendKind::Mssql => mssql::ping(conn).await,
        BackendKind::Oracle => oracle::ping(conn).await,
        BackendKind::Snowflake => snowflake::ping(conn).await,
        BackendKind::Clickhouse => clickhouse::ping(conn).await,
        BackendKind::Mongodb => mongodb::ping(conn).await,
    }
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    match conn.kind {
        BackendKind::Redis => redis::list_children(conn, node).await,
        BackendKind::Memcached => memcached::list_children(conn, node).await,
        BackendKind::Elasticsearch => elasticsearch::list_children(conn, node).await,
        BackendKind::Etcd => etcd::list_children(conn, node).await,
        BackendKind::Mysql
        | BackendKind::Mariadb
        | BackendKind::Postgres
        | BackendKind::Sqlite => sql::list_children(conn, node).await,
        BackendKind::Mssql => mssql::list_children(conn, node).await,
        BackendKind::Oracle => oracle::list_children(conn, node).await,
        BackendKind::Snowflake => snowflake::list_children(conn, node).await,
        BackendKind::Clickhouse => clickhouse::list_children(conn, node).await,
        BackendKind::Mongodb => mongodb::list_children(conn, node).await,
    }
}

pub async fn run_query(conn: &Connection, query: &str, context: &NodeMeta) -> Result<QueryResult> {
    let started = Instant::now();
    let mut result = match conn.kind {
        BackendKind::Redis => redis::run_query(conn, query).await?,
        BackendKind::Memcached => memcached::run_query(conn, query).await?,
        BackendKind::Elasticsearch => elasticsearch::run_query(conn, query, context).await?,
        BackendKind::Etcd => etcd::run_query(conn, query).await?,
        BackendKind::Mysql
        | BackendKind::Mariadb
        | BackendKind::Postgres
        | BackendKind::Sqlite => sql::run_query(conn, query).await?,
        BackendKind::Mssql => mssql::run_query(conn, query).await?,
        BackendKind::Oracle => oracle::run_query(conn, query).await?,
        BackendKind::Snowflake => snowflake::run_query(conn, query).await?,
        BackendKind::Clickhouse => clickhouse::run_query(conn, query).await?,
        BackendKind::Mongodb => mongodb::run_query(conn, query, context).await?,
    };
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    cap_result_rows(&mut result);
    Ok(result)
}

pub async fn preview_object(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    let started = Instant::now();
    let mut result = match conn.kind {
        BackendKind::Redis => redis::preview(conn, node).await?,
        BackendKind::Memcached => memcached::preview(conn, node).await?,
        BackendKind::Elasticsearch => elasticsearch::preview(conn, node).await?,
        BackendKind::Etcd => etcd::preview(conn, node).await?,
        BackendKind::Mysql
        | BackendKind::Mariadb
        | BackendKind::Postgres
        | BackendKind::Sqlite => sql::preview(conn, node).await?,
        BackendKind::Mssql => mssql::preview(conn, node).await?,
        BackendKind::Oracle => oracle::preview(conn, node).await?,
        BackendKind::Snowflake => snowflake::preview(conn, node).await?,
        BackendKind::Clickhouse => clickhouse::preview(conn, node).await?,
        BackendKind::Mongodb => mongodb::preview(conn, node).await?,
    };
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    cap_result_rows(&mut result);
    Ok(result)
}

fn cap_result_rows(result: &mut QueryResult) {
    if result.rows.len() <= MAX_RESULT_ROWS {
        return;
    }
    let original = result.rows.len();
    result.rows.truncate(MAX_RESULT_ROWS);
    result.truncated = true;
    if result.total.is_none() {
        result.total = Some(original as u64);
    }
    result.message = format!(
        "{} · 已截断显示前 {} 行（共约 {}，请缩小范围或分页）",
        result.message.trim(),
        MAX_RESULT_ROWS,
        original
    );
}

/// Compact row-count label for the navigation tree (right side).
pub(crate) fn format_row_count(n: i64) -> String {
    if n < 0 {
        return "—".into();
    }
    let n = n as f64;
    if n >= 1_000_000_000.0 {
        format!("{:.1}B 行", n / 1_000_000_000.0)
    } else if n >= 1_000_000.0 {
        format!("{:.1}M 行", n / 1_000_000.0)
    } else if n >= 10_000.0 {
        format!("{:.1}K 行", n / 1_000.0)
    } else if n >= 1_000.0 {
        let i = n as i64;
        format!("{},{:03} 行", i / 1_000, i % 1_000)
    } else {
        format!("{} 行", n as i64)
    }
}

#[allow(dead_code)]
pub async fn get_value(conn: &Connection, node: &TreeNode) -> Result<ValueView> {
    match conn.kind {
        BackendKind::Redis => redis::get_value(conn, node).await,
        BackendKind::Memcached => memcached::get_value(conn, node).await,
        BackendKind::Etcd => etcd::get_value(conn, node).await,
        BackendKind::Elasticsearch => elasticsearch::get_value(conn, node).await,
        BackendKind::Mongodb => mongodb::get_value(conn, node).await,
        _ => Ok(ValueView {
            title: node.label.clone(),
            content: format!("对象类型: {:?}\n路径: {}", node.kind, node.id),
            meta: conn.kind.display_name().into(),
        }),
    }
}

pub(crate) fn truncate_cell(s: &str, max: usize) -> String {
    let t = s.replace('\n', "↵").replace('\r', "");
    if t.chars().count() <= max {
        t
    } else {
        let cut: String = t.chars().take(max).collect();
        format!("{cut}…")
    }
}

pub(crate) fn json_pretty(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}
