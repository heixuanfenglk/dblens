//! ClickHouse backend over HTTP (`FORMAT JSON`).

use anyhow::{bail, Context, Result};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;

use crate::config::Connection;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode};
use crate::visual::{DEFAULT_PAGE_SIZE, MAX_RESULT_ROWS};

use super::truncate_cell;

fn base_url(conn: &Connection) -> Result<String> {
    if let Some(url) = conn.url.as_ref().filter(|u| !u.trim().is_empty()) {
        return Ok(url.trim().trim_end_matches('/').to_string());
    }
    let host = if conn.host.trim().is_empty() {
        "127.0.0.1"
    } else {
        conn.host.trim()
    };
    let port = if conn.port == 0 { 8123 } else { conn.port };
    let scheme = if conn.insecure || port == 8123 {
        "http"
    } else {
        "https"
    };
    // Prefer http for default 8123; https if port 8443 or explicit url.
    let scheme = if port == 8443 { "https" } else { scheme };
    Ok(format!("{scheme}://{host}:{port}"))
}

fn client(conn: &Connection) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(conn.insecure)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("创建 HTTP 客户端失败")
}

async fn exec_json(conn: &Connection, sql: &str) -> Result<Value> {
    let base = base_url(conn)?;
    let mut url = reqwest::Url::parse(&format!("{base}/"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("default_format", "JSON");
        if let Some(db) = conn.database.as_ref().filter(|d| !d.trim().is_empty()) {
            q.append_pair("database", db.trim());
        }
        if let Some(u) = conn.username.as_ref().filter(|u| !u.trim().is_empty()) {
            q.append_pair("user", u.trim());
        }
        if let Some(p) = conn.password.as_ref() {
            q.append_pair("password", p);
        }
    }

    let body = if sql.to_ascii_uppercase().contains("FORMAT ") {
        sql.to_string()
    } else {
        format!("{sql} FORMAT JSON")
    };

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));

    let resp = client(conn)?
        .post(url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .context("ClickHouse 请求失败")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("ClickHouse 错误 ({status}): {text}");
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).with_context(|| format!("解析 ClickHouse JSON 失败: {text}"))
}

fn from_ch_json(v: Value) -> QueryResult {
    if v.is_null() {
        return QueryResult {
            message: "完成".into(),
            ..Default::default()
        };
    }
    let meta = v
        .get("meta")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let columns: Vec<String> = meta
        .iter()
        .filter_map(|c| c.get("name")?.as_str().map(|s| s.to_string()))
        .collect();
    let column_types: Vec<String> = meta
        .iter()
        .filter_map(|c| c.get("type")?.as_str().map(|s| s.to_string()))
        .collect();
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    let truncated = data.len() > MAX_RESULT_ROWS;
    let rows: Vec<Vec<String>> = data
        .iter()
        .take(MAX_RESULT_ROWS)
        .map(|row| {
            if let Some(obj) = row.as_object() {
                columns
                    .iter()
                    .map(|c| {
                        obj.get(c)
                            .map(|v| match v {
                                Value::Null => String::new(),
                                Value::String(s) => truncate_cell(s, 2000),
                                other => truncate_cell(&other.to_string(), 2000),
                            })
                            .unwrap_or_default()
                    })
                    .collect()
            } else if let Some(arr) = row.as_array() {
                arr.iter()
                    .map(|v| match v {
                        Value::Null => String::new(),
                        Value::String(s) => truncate_cell(s, 2000),
                        other => truncate_cell(&other.to_string(), 2000),
                    })
                    .collect()
            } else {
                Vec::new()
            }
        })
        .collect();
    let total = v.get("rows").and_then(|r| r.as_u64());
    QueryResult {
        columns,
        column_types,
        rows,
        message: if truncated {
            format!("已截断显示前 {MAX_RESULT_ROWS} 行")
        } else {
            String::new()
        },
        total,
        truncated,
        ..Default::default()
    }
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let v = exec_json(conn, "SELECT 1 AS ok").await?;
    let _ = v;
    Ok(format!("ClickHouse OK — {}", conn.endpoint_label()))
}

pub async fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let sql = query.trim().trim_end_matches(';');
    if sql.is_empty() {
        bail!("SQL 为空");
    }
    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.starts_with("SELECT")
        || upper.starts_with("WITH")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESCRIBE")
        || upper.starts_with("DESC ")
        || upper.starts_with("EXISTS")
        || upper.starts_with("EXPLAIN")
    {
        let v = exec_json(conn, sql).await?;
        Ok(from_ch_json(v))
    } else {
        // DDL/DML — no JSON body expected
        let base = base_url(conn)?;
        let mut url = reqwest::Url::parse(&format!("{base}/"))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(db) = conn.database.as_ref().filter(|d| !d.trim().is_empty()) {
                q.append_pair("database", db.trim());
            }
            if let Some(u) = conn.username.as_ref().filter(|u| !u.trim().is_empty()) {
                q.append_pair("user", u.trim());
            }
            if let Some(p) = conn.password.as_ref() {
                q.append_pair("password", p);
            }
        }
        let resp = client(conn)?
            .post(url)
            .body(sql.to_string())
            .send()
            .await
            .context("ClickHouse 执行失败")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("ClickHouse 错误 ({status}): {text}");
        }
        Ok(QueryResult {
            message: if text.trim().is_empty() {
                "完成".into()
            } else {
                text
            },
            ..Default::default()
        })
    }
}

fn backtick(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    let cat = node.meta.path.as_deref().unwrap_or("");
    match node.kind {
        NodeKind::Connection => {
            let qr = run_query(conn, "SHOW DATABASES").await?;
            Ok(qr
                .rows
                .iter()
                .filter_map(|r| {
                    let name = r.first()?.clone();
                    Some(TreeNode {
                        id: format!("ch:{}:db:{name}", conn.id),
                        label: name.clone(),
                        kind: NodeKind::Database,
                        connection_id: conn.id.clone(),
                        meta: NodeMeta {
                            database: Some(name),
                            ..Default::default()
                        },
                        children: Vec::new(),
                expandable: true,
                loaded: false,
                    })
                })
                .collect())
        }
        NodeKind::Database => {
            let db = node.meta.database.as_deref().unwrap_or(&node.label);
            Ok(vec![
                folder(conn, db, "nav:tables", "表"),
                folder(conn, db, "nav:views", "视图"),
            ])
        }
        NodeKind::Folder if cat == "nav:tables" => {
            list_tables(conn, node.meta.database.as_deref().unwrap_or(""), false).await
        }
        NodeKind::Folder if cat == "nav:views" => {
            list_tables(conn, node.meta.database.as_deref().unwrap_or(""), true).await
        }
        NodeKind::Table => Ok(vec![TreeNode {
            id: format!(
                "ch:{}:{}:{}:nav:columns",
                conn.id,
                node.meta.database.as_deref().unwrap_or(""),
                node.meta.table.as_deref().unwrap_or("")
            ),
            label: "列".into(),
            kind: NodeKind::Folder,
            connection_id: conn.id.clone(),
            meta: NodeMeta {
                database: node.meta.database.clone(),
                table: node.meta.table.clone(),
                path: Some("nav:columns".into()),
                ..Default::default()
            },
            children: Vec::new(),
                expandable: true,
                loaded: false,
        }]),
        NodeKind::Folder if cat == "nav:columns" => list_columns(conn, &node.meta).await,
        _ => Ok(Vec::new()),
    }
}

fn folder(conn: &Connection, db: &str, path: &str, label: &str) -> TreeNode {
    TreeNode {
        id: format!("ch:{}:{db}:{path}", conn.id),
        label: label.into(),
        kind: NodeKind::Folder,
        connection_id: conn.id.clone(),
        meta: NodeMeta {
            database: Some(db.into()),
            path: Some(path.into()),
            ..Default::default()
        },
        children: Vec::new(),
        expandable: true,
        loaded: false,
    }
}

async fn list_tables(conn: &Connection, db: &str, views: bool) -> Result<Vec<TreeNode>> {
    let engine_filter = if views {
        "AND engine LIKE '%View%'"
    } else {
        "AND engine NOT LIKE '%View%'"
    };
    let sql = format!(
        "SELECT name, engine, total_rows FROM system.tables \
         WHERE database = '{}' {engine_filter} ORDER BY name",
        db.replace('\'', "''")
    );
    let qr = run_query(conn, &sql).await?;
    Ok(qr
        .rows
        .iter()
        .filter_map(|r| {
            let name = r.first()?.clone();
            let engine = r.get(1).cloned().unwrap_or_default();
            let rows = r.get(2).cloned().unwrap_or_default();
            Some(TreeNode {
                id: format!("ch:{}:{db}:table:{name}", conn.id),
                label: name.clone(),
                kind: NodeKind::Table,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(db.into()),
                    table: Some(name),
                    meta_line: Some(if rows.is_empty() {
                        engine
                    } else {
                        format!("{engine} · {rows}")
                    }),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: true,
                loaded: false,
            })
        })
        .collect())
}

async fn list_columns(conn: &Connection, meta: &NodeMeta) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("");
    let table = meta.table.as_deref().unwrap_or("");
    let sql = format!(
        "SELECT name, type FROM system.columns \
         WHERE database = '{}' AND table = '{}' ORDER BY position",
        db.replace('\'', "''"),
        table.replace('\'', "''")
    );
    let qr = run_query(conn, &sql).await?;
    Ok(qr
        .rows
        .iter()
        .filter_map(|r| {
            let name = r.first()?.clone();
            let dtype = r.get(1).cloned().unwrap_or_default();
            Some(TreeNode {
                id: format!("ch:{}:{db}:{table}:col:{name}", conn.id),
                label: format!("{name}  {dtype}"),
                kind: NodeKind::Column,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(db.into()),
                    table: Some(table.into()),
                    path: Some(name),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: false,
                loaded: true,
            })
        })
        .collect())
}

pub async fn preview(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    match node.kind {
        NodeKind::Table => {
            let db = node.meta.database.as_deref().unwrap_or("");
            let table = node.meta.table.as_deref().unwrap_or(&node.label);
            let sql = format!(
                "SELECT * FROM {}.{} LIMIT {}",
                backtick(db),
                backtick(table),
                DEFAULT_PAGE_SIZE
            );
            run_query(conn, &sql).await
        }
        _ => Ok(QueryResult {
            message: "请选择表以预览数据".into(),
            ..Default::default()
        }),
    }
}
