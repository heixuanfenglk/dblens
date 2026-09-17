use anyhow::{bail, Context, Result};
use redis::AsyncCommands;

use crate::config::Connection;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode, ValueView};

use super::truncate_cell;

fn redis_url(conn: &Connection) -> String {
    if let Some(url) = conn.url.as_ref().filter(|u| !u.is_empty()) {
        return url.clone();
    }
    let db = conn
        .database
        .as_deref()
        .unwrap_or("0")
        .parse::<u8>()
        .unwrap_or(0);
    let auth = match (&conn.username, &conn.password) {
        (Some(u), Some(p)) if !u.is_empty() => format!("{u}:{p}@"),
        (_, Some(p)) if !p.is_empty() => format!(":{p}@"),
        _ => String::new(),
    };
    let port = if conn.port == 0 { 6379 } else { conn.port };
    format!("redis://{auth}{}:{port}/{db}", conn.host)
}

async fn connect(conn: &Connection) -> Result<redis::aio::MultiplexedConnection> {
    let client = redis::Client::open(redis_url(conn)).context("Redis URL 无效")?;
    client
        .get_multiplexed_async_connection()
        .await
        .context("连接 Redis 失败")
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let mut c = connect(conn).await?;
    let pong: String = redis::cmd("PING").query_async(&mut c).await?;
    Ok(format!("PONG ({pong}) — {}", conn.endpoint_label()))
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    let mut c = connect(conn).await?;
    match node.kind {
        NodeKind::Connection => {
            let info: String = redis::cmd("INFO")
                .arg("keyspace")
                .query_async(&mut c)
                .await
                .unwrap_or_default();
            let mut dbs = Vec::new();
            for line in info.lines() {
                if let Some(rest) = line.strip_prefix("db") {
                    if let Some((idx, _)) = rest.split_once(':') {
                        dbs.push(idx.to_string());
                    }
                }
            }
            if dbs.is_empty() {
                dbs.push(
                    conn.database
                        .clone()
                        .unwrap_or_else(|| "0".into()),
                );
            }
            Ok(dbs
                .into_iter()
                .map(|db| TreeNode {
                    id: format!("redis:{}:db:{db}", conn.id),
                    label: format!("db{db}"),
                    kind: NodeKind::Database,
                    connection_id: conn.id.clone(),
                    meta: NodeMeta {
                        database: Some(db),
                        ..Default::default()
                    },
                    children: Vec::new(),
                    expandable: true,
                    loaded: false,
                })
                .collect())
        }
        NodeKind::Database => {
            let pattern = "*";
            let mut cursor: u64 = 0;
            let mut keys: Vec<String> = Vec::new();
            loop {
                let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(pattern)
                    .arg("COUNT")
                    .arg(200)
                    .query_async(&mut c)
                    .await?;
                keys.extend(batch);
                cursor = next;
                if cursor == 0 || keys.len() >= 500 {
                    break;
                }
            }
            keys.sort();
            keys.truncate(500);
            let db = node.meta.database.clone();
            Ok(keys
                .into_iter()
                .map(|k| TreeNode {
                    id: format!("redis:{}:key:{k}", conn.id),
                    label: k.clone(),
                    kind: NodeKind::Key,
                    connection_id: conn.id.clone(),
                    meta: NodeMeta {
                        database: db.clone(),
                        path: Some(k),
                        ..Default::default()
                    },
                    children: Vec::new(),
                    expandable: false,
                    loaded: true,
                })
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

pub async fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let q = strip_comments(query);
    if q.is_empty() {
        bail!("查询为空");
    }
    let mut c = connect(conn).await?;
    let parts = split_args(&q);
    if parts.is_empty() {
        bail!("查询为空");
    }
    let mut cmd = redis::cmd(&parts[0]);
    for a in &parts[1..] {
        cmd.arg(a.as_str());
    }
    let value: redis::Value = cmd.query_async(&mut c).await.context("执行 Redis 命令失败")?;
    Ok(redis_value_to_result(&parts[0], value))
}

pub async fn preview(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    match node.kind {
        NodeKind::Key => {
            let key = node.meta.path.as_deref().unwrap_or(&node.label);
            run_query(conn, &format!("GET {key}")).await
        }
        NodeKind::Database | NodeKind::Connection => {
            run_query(conn, "INFO keyspace").await
        }
        _ => Ok(QueryResult {
            message: "无可预览内容".into(),
            ..Default::default()
        }),
    }
}

#[allow(dead_code)]
pub async fn get_value(conn: &Connection, node: &TreeNode) -> Result<ValueView> {
    let key = node
        .meta
        .path
        .clone()
        .unwrap_or_else(|| node.label.clone());
    let mut c = connect(conn).await?;
    let ty: String = c.key_type(&key).await.unwrap_or_else(|_| "none".into());
    let ttl: i64 = c.ttl(&key).await.unwrap_or(-2);
    let content = match ty.as_str() {
        "string" => {
            let v: String = c.get(&key).await.unwrap_or_default();
            v
        }
        "list" => {
            let v: Vec<String> = c.lrange(&key, 0, 99).await.unwrap_or_default();
            v.join("\n")
        }
        "set" => {
            let v: Vec<String> = c.smembers(&key).await.unwrap_or_default();
            v.join("\n")
        }
        "zset" => {
            let v: Vec<String> = redis::cmd("ZRANGE")
                .arg(&key)
                .arg(0)
                .arg(99)
                .arg("WITHSCORES")
                .query_async(&mut c)
                .await
                .unwrap_or_default();
            v.join("\n")
        }
        "hash" => {
            let v: Vec<String> = c.hgetall(&key).await.unwrap_or_default();
            v.chunks(2)
                .map(|c| format!("{} = {}", c.first().map(String::as_str).unwrap_or(""), c.get(1).map(String::as_str).unwrap_or("")))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => format!("(type={ty})"),
    };
    Ok(ValueView {
        title: key,
        content,
        meta: format!("type={ty} ttl={ttl}"),
    })
}

fn redis_value_to_result(cmd: &str, value: redis::Value) -> QueryResult {
    match value {
        redis::Value::Nil => QueryResult {
            columns: vec!["result".into()],
            rows: vec![vec!["(nil)".into()]],
            message: format!("{cmd} OK"),
            ..Default::default()
        },
        redis::Value::Int(i) => QueryResult {
            columns: vec!["result".into()],
            rows: vec![vec![i.to_string()]],
            message: format!("{cmd} OK"),
            ..Default::default()
        },
        redis::Value::Okay => QueryResult {
            columns: vec!["result".into()],
            rows: vec![vec!["OK".into()]],
            message: format!("{cmd} OK"),
            ..Default::default()
        },
        redis::Value::SimpleString(s) => QueryResult {
            columns: vec!["result".into()],
            rows: vec![vec![s]],
            message: format!("{cmd} OK"),
            ..Default::default()
        },
        redis::Value::BulkString(bytes) => {
            let s = String::from_utf8_lossy(&bytes).to_string();
            QueryResult {
                columns: vec!["value".into()],
                rows: vec![vec![s]],
                message: format!("{cmd} OK"),
                ..Default::default()
            }
        }
        redis::Value::Array(items) | redis::Value::Set(items) => {
            let rows: Vec<Vec<String>> = items
                .into_iter()
                .enumerate()
                .map(|(i, v)| vec![i.to_string(), format_redis_value(&v)])
                .collect();
            QueryResult {
                columns: vec!["#".into(), "value".into()],
                rows,
                message: format!("{cmd} OK"),
                ..Default::default()
            }
        }
        other => QueryResult {
            columns: vec!["value".into()],
            rows: vec![vec![format!("{other:?}")]],
            message: format!("{cmd} OK"),
            ..Default::default()
        },
    }
}

fn format_redis_value(v: &redis::Value) -> String {
    match v {
        redis::Value::Nil => "(nil)".into(),
        redis::Value::Int(i) => i.to_string(),
        redis::Value::BulkString(b) => truncate_cell(&String::from_utf8_lossy(b), 500),
        redis::Value::SimpleString(s) => s.clone(),
        redis::Value::Okay => "OK".into(),
        redis::Value::Array(items) | redis::Value::Set(items) => items
            .iter()
            .map(format_redis_value)
            .collect::<Vec<_>>()
            .join(", "),
        redis::Value::Double(f) => f.to_string(),
        redis::Value::Boolean(b) => b.to_string(),
        _ => format!("{v:?}"),
    }
}

fn strip_comments(q: &str) -> String {
    q.lines()
        .map(|l| {
            let t = l.trim();
            if t.starts_with('#') || t.starts_with("//") {
                ""
            } else {
                t
            }
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote: Option<char> = None;
    for ch in s.chars() {
        match in_quote {
            Some(q) => {
                if ch == q {
                    in_quote = None;
                } else {
                    cur.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    in_quote = Some(ch);
                } else if ch.is_whitespace() {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                } else {
                    cur.push(ch);
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
