use anyhow::{bail, Context, Result};
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;

use crate::config::Connection;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode, ValueView};
use crate::visual::DEFAULT_PAGE_SIZE;

use super::{json_pretty, truncate_cell};

struct EsHttp {
    client: reqwest::Client,
    base: String,
}

fn build(conn: &Connection) -> Result<EsHttp> {
    let base = if let Some(url) = conn.url.as_ref().filter(|u| !u.is_empty()) {
        url.trim_end_matches('/').to_string()
    } else {
        // Prefer explicit scheme in host (user may paste full URL into host)
        let host_raw = conn.host.trim();
        if host_raw.contains("://") {
            host_raw.trim_end_matches('/').to_string()
        } else {
            // Default HTTP for ES data ports; only use HTTPS for 443 or when user opts in via URL
            let scheme = if conn.port == 443 { "https" } else { "http" };
            let port = if conn.port == 0 { 9200 } else { conn.port };
            format!("{scheme}://{host_raw}:{port}")
        }
    };
    // If somehow https was forced to an http-only node, users can set url=http://...

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let (Some(u), Some(p)) = (&conn.username, &conn.password) {
        if !u.is_empty() {
            let token = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"));
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Basic {token}"))?,
            );
        }
    }

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(conn.insecure)
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    Ok(EsHttp { client, base })
}

async fn get_json(es: &EsHttp, path: &str) -> Result<Value> {
    let url = format!("{}{}", es.base, path);
    let resp = es.client.get(&url).send().await.with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        bail!("HTTP {status}: {}", truncate_cell(&text, 400));
    }
    serde_json::from_str(&text).with_context(|| format!("JSON 解析失败: {path}"))
}

async fn get_text(es: &EsHttp, path: &str) -> Result<(u16, String)> {
    let url = format!("{}{}", es.base, path);
    let resp = es.client.get(&url).send().await.with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await?;
    if !(200..300).contains(&status) {
        bail!("HTTP {status}: {}", truncate_cell(&text, 400));
    }
    Ok((status, text))
}

async fn post_json(es: &EsHttp, path: &str, body: &str) -> Result<Value> {
    let url = format!("{}{}", es.base, path);
    let resp = es
        .client
        .post(&url)
        .body(body.to_string())
        .send()
        .await
        .with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        bail!("HTTP {status}: {}", truncate_cell(&text, 500));
    }
    serde_json::from_str(&text).context("JSON 解析失败")
}

fn json_field_str(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string().trim_matches('"').to_string(),
        None => "-".into(),
    }
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let es = build(conn)?;
    let v = get_json(&es, "/").await?;
    let ver = v
        .pointer("/version/number")
        .and_then(|x| x.as_str())
        .unwrap_or("?");
    let name = v.get("cluster_name").and_then(|x| x.as_str()).unwrap_or("?");
    let tag = v.get("tagline").and_then(|x| x.as_str()).unwrap_or("");
    Ok(format!(
        "Elasticsearch {ver} — cluster={name} — {}",
        if tag.is_empty() {
            conn.endpoint_label()
        } else {
            tag.to_string()
        }
    ))
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    let es = build(conn)?;
    match node.kind {
        NodeKind::Connection => {
            // Top-level: 集群 + 索引 + 系统索引
            let mut out = Vec::new();
            out.push(TreeNode {
                id: format!("es:{}:cluster", conn.id),
                label: "集群".into(),
                kind: NodeKind::Folder,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    path: Some("__cluster__".into()),
                    schema: Some("cluster".into()),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: true,
                loaded: false,
            });
            out.push(TreeNode {
                id: format!("es:{}:indices", conn.id),
                label: "索引".into(),
                kind: NodeKind::Folder,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    path: Some("__indices__".into()),
                    schema: Some("indices".into()),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: true,
                loaded: false,
            });
            out.push(TreeNode {
                id: format!("es:{}:sysindices", conn.id),
                label: "系统索引".into(),
                kind: NodeKind::Folder,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    path: Some("__sysindices__".into()),
                    schema: Some("sysindices".into()),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: true,
                loaded: false,
            });
            Ok(out)
        }
        NodeKind::Folder => {
            let kind = node.meta.schema.as_deref().unwrap_or("");
            match kind {
                "cluster" => list_cluster_children(&es, conn).await,
                "indices" => list_index_nodes(&es, conn, false).await,
                "sysindices" => list_index_nodes(&es, conn, true).await,
                _ => Ok(Vec::new()),
            }
        }
        NodeKind::Index => {
            let index = node
                .meta
                .path
                .clone()
                .unwrap_or_else(|| clean_index_label(&node.label));
            Ok(index_detail_children(conn, &index))
        }
        _ => Ok(Vec::new()),
    }
}

async fn list_cluster_children(es: &EsHttp, conn: &Connection) -> Result<Vec<TreeNode>> {
    let health = get_json(es, "/_cluster/health").await.ok();
    let status = health
        .as_ref()
        .and_then(|v| v.get("status"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let nodes = health
        .as_ref()
        .and_then(|v| v.get("number_of_nodes"))
        .and_then(|x| x.as_u64());
    let shards = health
        .as_ref()
        .and_then(|v| v.get("active_shards"))
        .and_then(|x| x.as_u64());
    let name = get_json(es, "/")
        .await
        .ok()
        .and_then(|v| {
            v.get("cluster_name")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        });
    let aliases_n = get_text(es, "/_cat/aliases?format=json")
        .await
        .ok()
        .and_then(|(_s, t)| serde_json::from_str::<Vec<Value>>(&t).ok())
        .map(|a| a.len() as u64);
    let templates_n = match get_json(es, "/_index_template").await {
        Ok(v) => v
            .get("index_templates")
            .and_then(|x| x.as_array())
            .map(|a| a.len() as u64),
        Err(_) => get_json(es, "/_template")
            .await
            .ok()
            .and_then(|v| v.as_object().map(|o| o.len() as u64)),
    };
    Ok(cluster_children_with_meta(
        conn,
        status.as_deref(),
        name.as_deref(),
        nodes,
        shards,
        aliases_n,
        templates_n,
    ))
}

fn cluster_children_with_meta(
    conn: &Connection,
    health_status: Option<&str>,
    cluster_name: Option<&str>,
    nodes: Option<u64>,
    shards: Option<u64>,
    aliases_n: Option<u64>,
    templates_n: Option<u64>,
) -> Vec<TreeNode> {
    let items = [
        (
            "health",
            "集群健康",
            health_status.map(|s| s.to_string()),
            health_status.map(|s| s.to_ascii_lowercase()),
        ),
        (
            "info",
            "集群信息",
            cluster_name.map(|s| s.to_string()),
            None,
        ),
        (
            "nodes",
            "节点",
            nodes.map(|n| format!("{n} 节点")),
            None,
        ),
        (
            "shards",
            "分片",
            shards.map(|n| format!("{n} 分片")),
            None,
        ),
        (
            "aliases",
            "别名",
            aliases_n.map(|n| format!("{n} 个别名")),
            None,
        ),
        (
            "templates",
            "模板",
            templates_n.map(|n| format!("{n} 个模板")),
            None,
        ),
    ];
    items
        .into_iter()
        .map(|(action, label, meta_line, status)| TreeNode {
            id: format!("es:{}:cluster:{action}", conn.id),
            label: label.into(),
            kind: NodeKind::Key,
            connection_id: conn.id.clone(),
            meta: NodeMeta {
                path: Some("__cluster__".into()),
                schema: Some(action.into()),
                table: Some(action.into()),
                meta_line,
                status,
                ..Default::default()
            },
            children: Vec::new(),
            expandable: false,
            loaded: true,
        })
        .collect()
}

fn index_detail_children(conn: &Connection, index: &str) -> Vec<TreeNode> {
    let items = [
        ("docs", "文档"),
        ("mapping", "Mapping"),
        ("settings", "Settings"),
        ("stats", "Stats"),
        ("aliases", "别名"),
    ];
    items
        .into_iter()
        .map(|(action, label)| TreeNode {
            id: format!("es:{}:idx:{index}:{action}", conn.id),
            label: label.into(),
            kind: if action == "docs" {
                NodeKind::Table
            } else {
                NodeKind::Key
            },
            connection_id: conn.id.clone(),
            meta: NodeMeta {
                path: Some(index.to_string()),
                table: Some(index.to_string()),
                schema: Some(action.into()),
                database: Some(index.to_string()),
                ..Default::default()
            },
            children: Vec::new(),
            expandable: false,
            loaded: true,
        })
        .collect()
}

async fn list_index_nodes(es: &EsHttp, conn: &Connection, system_only: bool) -> Result<Vec<TreeNode>> {
    let (_status, text) = get_text(
        es,
        "/_cat/indices?format=json&h=index,docs.count,store.size,health,status,pri,rep",
    )
    .await?;
    let arr: Vec<Value> = serde_json::from_str(&text).unwrap_or_default();
    let mut nodes: Vec<TreeNode> = arr
        .into_iter()
        .filter_map(|item| {
            let index = json_field_str(&item, "index");
            if index == "-" || index.is_empty() {
                return None;
            }
            let is_sys = index.starts_with('.');
            if system_only != is_sys {
                return None;
            }
            let docs = json_field_str(&item, "docs.count");
            let health = json_field_str(&item, "health");
            let size = json_field_str(&item, "store.size");
            Some(TreeNode {
                id: format!("es:{}:idx:{index}", conn.id),
                label: index.clone(),
                kind: NodeKind::Index,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    path: Some(index.clone()),
                    table: Some(index.clone()),
                    database: Some(index),
                    status: Some(health),
                    meta_line: Some(format_index_meta(&docs, &size)),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: true,
                loaded: false,
            })
        })
        .collect();
    nodes.sort_by(|a, b| a.meta.path.cmp(&b.meta.path));
    Ok(nodes)
}

fn format_index_meta(docs: &str, size: &str) -> String {
    let docs_fmt = format_count(docs);
    let size_fmt = if size.is_empty() || size == "-" {
        "—".into()
    } else {
        size.to_ascii_uppercase()
    };
    format!("{docs_fmt} · {size_fmt}")
}

fn format_count(raw: &str) -> String {
    let n: f64 = raw.replace(',', "").parse().unwrap_or(-1.0);
    if n < 0.0 {
        return "—".into();
    }
    if n >= 1_000_000_000.0 {
        format!("{:.1}B docs", n / 1_000_000_000.0)
    } else if n >= 1_000_000.0 {
        format!("{:.1}M docs", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}K docs", n / 1_000.0)
    } else {
        format!("{:.0} docs", n)
    }
}

fn clean_index_label(label: &str) -> String {
    label
        .split_whitespace()
        .next()
        .unwrap_or(label)
        .to_string()
}

pub async fn run_query(conn: &Connection, query: &str, context: &NodeMeta) -> Result<QueryResult> {
    let q = strip_comments(query);
    if q.is_empty() {
        bail!("查询为空");
    }
    let es = build(conn)?;

    // JSON body → _search
    if q.starts_with('{') {
        let index = context
            .path
            .as_deref()
            .filter(|p| !p.starts_with("__"))
            .or(context.table.as_deref())
            .or(context.database.as_deref())
            .unwrap_or("_all");
        let v = post_json(&es, &format!("/{index}/_search"), &q).await?;
        return Ok(hits_to_result(&v));
    }

    // Lucene query string shortcut: q=foo OR just plain text
    if let Some(rest) = q.strip_prefix("q=") {
        let index = context
            .path
            .as_deref()
            .filter(|p| !p.starts_with("__"))
            .unwrap_or("_all");
        let encoded = urlencoding_minimal(rest.trim());
        let v = get_json(&es, &format!("/{index}/_search?q={encoded}&size=50")).await?;
        return Ok(hits_to_result(&v));
    }

    // HTTP style: GET /_cat/indices
    let (method, path) = if let Some(rest) = q
        .strip_prefix("GET ")
        .or_else(|| q.strip_prefix("get "))
    {
        ("GET", rest.trim())
    } else if let Some(rest) = q
        .strip_prefix("POST ")
        .or_else(|| q.strip_prefix("post "))
    {
        ("POST", rest.trim())
    } else if q.starts_with('/') {
        ("GET", q.as_str())
    } else {
        // treat as Lucene query on context index
        let index = context
            .path
            .as_deref()
            .filter(|p| !p.starts_with("__"))
            .unwrap_or("_all");
        let encoded = urlencoding_minimal(&q);
        let v = get_json(&es, &format!("/{index}/_search?q={encoded}&size=50")).await?;
        return Ok(hits_to_result(&v));
    };

    // POST with optional body after blank line
    if method == "POST" {
        let (path_only, body) = split_path_body(path);
        let url_path = if path_only.starts_with('/') {
            path_only.to_string()
        } else {
            format!("/{path_only}")
        };
        if body.is_empty() {
            let v = post_json(&es, &url_path, "{}").await?;
            return Ok(value_to_result(&v, "POST"));
        }
        let v = post_json(&es, &url_path, body).await?;
        return Ok(value_to_result(&v, "POST"));
    }

    let url_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let (_s, text) = get_text(&es, &url_path).await?;
    Ok(text_to_result(&text))
}

pub async fn preview(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    let es = build(conn)?;
    let action = node.meta.schema.as_deref().unwrap_or("");
    let path = node.meta.path.as_deref().unwrap_or("");

    match node.kind {
        NodeKind::Connection => {
            let v = get_json(&es, "/_cluster/health").await?;
            Ok(value_to_result(&v, "cluster health"))
        }
        NodeKind::Index => {
            // default: sample documents
            let index = path;
            if index.is_empty() || index.starts_with("__") {
                bail!("无效索引");
            }
            let body = format!(
                r#"{{"query":{{"match_all":{{}}}},"size":{DEFAULT_PAGE_SIZE}}}"#
            );
            let v = post_json(&es, &format!("/{index}/_search"), &body).await?;
            Ok(hits_to_result(&v))
        }
        NodeKind::Table if action == "docs" || action.is_empty() => {
            let index = path;
            let body = format!(
                r#"{{"query":{{"match_all":{{}}}},"size":{DEFAULT_PAGE_SIZE}}}"#
            );
            let v = post_json(&es, &format!("/{index}/_search"), &body).await?;
            Ok(hits_to_result(&v))
        }
        NodeKind::Key | NodeKind::Table => match action {
            "health" => {
                let v = get_json(&es, "/_cluster/health").await?;
                Ok(value_to_result(&v, "集群健康"))
            }
            "info" => {
                let v = get_json(&es, "/").await?;
                Ok(value_to_result(&v, "集群信息"))
            }
            "nodes" => {
                let v = get_json(
                    &es,
                    "/_cat/nodes?format=json&h=name,ip,heap.percent,ram.percent,cpu,load_1m,node.role,master",
                )
                .await?;
                Ok(value_to_result(&v, "节点"))
            }
            "shards" => {
                let v = get_json(
                    &es,
                    "/_cat/shards?format=json&h=index,shard,prirep,state,docs,store,ip,node",
                )
                .await?;
                Ok(value_to_result(&v, "分片"))
            }
            "aliases" if path == "__cluster__" || path.is_empty() => {
                let v = get_json(
                    &es,
                    "/_cat/aliases?format=json&h=alias,index,filter,routing.index,routing.search",
                )
                .await?;
                Ok(value_to_result(&v, "别名"))
            }
            "templates" => {
                let v = match get_json(&es, "/_index_template").await {
                    Ok(v) => v,
                    Err(_) => get_json(&es, "/_template").await?,
                };
                Ok(value_to_result(&v, "模板"))
            }
            "mapping" => {
                let v = get_json(&es, &format!("/{path}/_mapping")).await?;
                Ok(value_to_result(&v, "mapping"))
            }
            "settings" => {
                let v = get_json(&es, &format!("/{path}/_settings")).await?;
                Ok(value_to_result(&v, "settings"))
            }
            "stats" => {
                let v = get_json(&es, &format!("/{path}/_stats")).await?;
                Ok(value_to_result(&v, "stats"))
            }
            "aliases" => {
                let v = get_json(&es, &format!("/{path}/_alias")).await?;
                Ok(value_to_result(&v, "aliases"))
            }
            "docs" => {
                let body = format!(
                    r#"{{"query":{{"match_all":{{}}}},"size":{DEFAULT_PAGE_SIZE}}}"#
                );
                let v = post_json(&es, &format!("/{path}/_search"), &body).await?;
                Ok(hits_to_result(&v))
            }
            _ => {
                // folder preview
                if action == "indices" || action == "sysindices" {
                    let (_s, text) = get_text(&es, "/_cat/indices?v").await?;
                    return Ok(cat_text_to_result(&text));
                }
                if action == "cluster" {
                    let v = get_json(&es, "/_cluster/health").await?;
                    return Ok(value_to_result(&v, "cluster"));
                }
                Ok(QueryResult {
                    message: format!("请双击或展开查看: {}", node.label),
                    ..Default::default()
                })
            }
        },
        NodeKind::Folder => {
            match action {
                "cluster" => {
                    let v = get_json(&es, "/_cluster/health").await?;
                    Ok(value_to_result(&v, "cluster"))
                }
                "indices" | "sysindices" => {
                    let (_s, text) = get_text(&es, "/_cat/indices?v").await?;
                    Ok(cat_text_to_result(&text))
                }
                _ => Ok(QueryResult {
                    message: "展开文件夹以查看子项".into(),
                    ..Default::default()
                }),
            }
        }
        _ => Ok(QueryResult {
            message: "无可预览内容".into(),
            ..Default::default()
        }),
    }
}

#[allow(dead_code)]
pub async fn get_value(conn: &Connection, node: &TreeNode) -> Result<ValueView> {
    let r = preview(conn, node).await?;
    Ok(ValueView {
        title: node.label.clone(),
        content: if r.columns.len() == 1 && r.rows.len() == 1 {
            r.rows[0][0].clone()
        } else {
            r.message
        },
        meta: "elasticsearch".into(),
    })
}

fn strip_comments(q: &str) -> String {
    q.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_path_body(s: &str) -> (&str, &str) {
    if let Some((p, b)) = s.split_once('\n') {
        (p.trim(), b.trim())
    } else {
        (s.trim(), "")
    }
}

fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn text_to_result(text: &str) -> QueryResult {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return value_to_result(&v, "OK");
    }
    cat_text_to_result(text)
}

fn cat_text_to_result(text: &str) -> QueryResult {
    let mut lines = text.lines().filter(|l| !l.is_empty());
    let Some(header) = lines.next() else {
        return QueryResult {
            message: "empty".into(),
            ..Default::default()
        };
    };
    let columns: Vec<String> = header.split_whitespace().map(|s| s.to_string()).collect();
    let rows: Vec<Vec<String>> = lines
        .map(|line| {
            let parts: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();
            if parts.len() < columns.len() {
                let mut p = parts;
                p.resize(columns.len(), String::new());
                p
            } else if parts.len() > columns.len() {
                let mut p = parts[..columns.len() - 1].to_vec();
                p.push(parts[columns.len() - 1..].join(" "));
                p
            } else {
                parts
            }
        })
        .collect();
    QueryResult {
        message: format!("{} rows", rows.len()),
        columns,
        rows,
        ..Default::default()
    }
}

fn value_to_result(v: &Value, msg: &str) -> QueryResult {
    if v.pointer("/hits/hits").is_some() {
        return hits_to_result(v);
    }
    if let Some(arr) = v.as_array() {
        return json_array_table(arr);
    }
    // Index templates (ES 7.8+)
    if let Some(arr) = v.get("index_templates").and_then(|x| x.as_array()) {
        let rows: Vec<Value> = arr
            .iter()
            .map(|item| {
                let mut obj = serde_json::Map::new();
                if let Some(name) = item.get("name") {
                    obj.insert("name".into(), name.clone());
                }
                if let Some(t) = item.get("index_template") {
                    if let Some(patterns) = t.get("index_patterns") {
                        obj.insert("index_patterns".into(), patterns.clone());
                    }
                    if let Some(prio) = t.get("priority") {
                        obj.insert("priority".into(), prio.clone());
                    }
                }
                if obj.is_empty() {
                    item.clone()
                } else {
                    Value::Object(obj)
                }
            })
            .collect();
        return json_array_table(&rows);
    }
    // Flat / mostly-flat objects → key-value table (cluster health, root info, …)
    if let Some(obj) = v.as_object() {
        let mut rows = Vec::new();
        flatten_json_kv("", v, &mut rows);
        if !rows.is_empty() && rows.len() <= 80 {
            return QueryResult {
                columns: vec!["字段".into(), "值".into()],
                rows: rows.into_iter().map(|(k, val)| vec![k, val]).collect(),
                message: msg.into(),
                ..Default::default()
            };
        }
        // Legacy template map: name → body
        if obj.values().all(|x| x.is_object()) && !obj.is_empty() {
            let rows: Vec<Value> = obj
                .iter()
                .map(|(name, body)| {
                    let mut m = serde_json::Map::new();
                    m.insert("name".into(), Value::String(name.clone()));
                    if let Some(p) = body.get("index_patterns") {
                        m.insert("index_patterns".into(), p.clone());
                    }
                    Value::Object(m)
                })
                .collect();
            return json_array_table(&rows);
        }
        let _ = obj;
    }
    QueryResult {
        columns: vec!["json".into()],
        rows: vec![vec![json_pretty(v)]],
        message: msg.into(),
        ..Default::default()
    }
}

fn flatten_json_kv(prefix: &str, v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                match val {
                    Value::Object(_) | Value::Array(_) => flatten_json_kv(&key, val, out),
                    Value::Null => out.push((key, "null".into())),
                    Value::Bool(b) => out.push((key, b.to_string())),
                    Value::Number(n) => out.push((key, n.to_string())),
                    Value::String(s) => out.push((key, s.clone())),
                }
            }
        }
        Value::Array(arr) => {
            if arr.iter().all(|x| {
                matches!(x, Value::String(_) | Value::Number(_) | Value::Bool(_))
            }) {
                let s = arr
                    .iter()
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push((prefix.to_string(), s));
            } else {
                out.push((
                    prefix.to_string(),
                    truncate_cell(&v.to_string(), 240),
                ));
            }
        }
        other => out.push((prefix.to_string(), other.to_string())),
    }
}

fn hits_to_result(v: &Value) -> QueryResult {
    let hits = v
        .pointer("/hits/hits")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let total = v
        .pointer("/hits/total/value")
        .or_else(|| v.pointer("/hits/total"))
        .map(|x| match x {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| hits.len().to_string());

    let mut col_set = std::collections::BTreeSet::new();
    col_set.insert("_id".into());
    col_set.insert("_score".into());
    col_set.insert("_index".into());

    let mut flat_rows: Vec<Vec<(String, String)>> = Vec::new();
    for hit in &hits {
        let mut flat = Vec::new();
        flat.push((
            "_id".into(),
            hit.get("_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        ));
        flat.push((
            "_score".into(),
            hit.get("_score")
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".into()),
        ));
        flat.push((
            "_index".into(),
            hit.get("_index")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        ));
        if let Some(source) = hit.get("_source") {
            flatten_json("", source, &mut flat);
        }
        for (k, _) in &flat {
            col_set.insert(k.clone());
        }
        flat_rows.push(flat);
    }

    let mut columns: Vec<String> = vec!["_id".into(), "_score".into(), "_index".into()];
    for k in col_set {
        if k != "_id" && k != "_score" && k != "_index" {
            columns.push(k);
        }
    }
    if columns.len() > 30 {
        columns.truncate(30);
    }

    let total_num = v
        .pointer("/hits/total/value")
        .or_else(|| v.pointer("/hits/total"))
        .and_then(|x| x.as_u64());

    let rows: Vec<Vec<String>> = flat_rows
        .iter()
        .map(|flat| {
            let map: std::collections::HashMap<_, _> = flat.iter().cloned().collect();
            columns
                .iter()
                .map(|c| {
                    map.get(c)
                        .cloned()
                        .map(|s| truncate_cell(&s, 200))
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();

    QueryResult {
        columns,
        rows,
        message: format!("hits={total}"),
        total: total_num,
        ..Default::default()
    }
}

fn flatten_json(prefix: &str, v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_json(&key, val, out);
            }
        }
        Value::Array(arr) => {
            out.push((
                prefix.to_string(),
                truncate_cell(&Value::Array(arr.clone()).to_string(), 200),
            ));
        }
        Value::String(s) => out.push((prefix.to_string(), s.clone())),
        other => out.push((prefix.to_string(), other.to_string())),
    }
}

fn json_array_table(arr: &[Value]) -> QueryResult {
    let mut cols = std::collections::BTreeSet::new();
    for item in arr {
        if let Some(obj) = item.as_object() {
            for k in obj.keys() {
                cols.insert(k.clone());
            }
        }
    }
    let columns: Vec<String> = cols.into_iter().collect();
    if columns.is_empty() {
        return QueryResult {
            columns: vec!["json".into()],
            rows: arr.iter().map(|v| vec![v.to_string()]).collect(),
            message: format!("{} items", arr.len()),
            ..Default::default()
        };
    }
    let rows = arr
        .iter()
        .map(|item| {
            columns
                .iter()
                .map(|c| {
                    item.get(c)
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();
    QueryResult {
        columns,
        rows,
        message: format!("{} rows", arr.len()),
        ..Default::default()
    }
}
