use anyhow::{Context, Result};
use etcdrs::Client;
use futures::StreamExt;

use crate::config::Connection;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode, ValueView};

use super::truncate_cell;
use crate::visual::MAX_RESULT_ROWS;

fn connect(conn: &Connection) -> Result<Client> {
    let endpoint = if let Some(url) = conn.url.as_ref().filter(|u| !u.is_empty()) {
        if url.contains("://") {
            url.clone()
        } else {
            format!("http://{url}")
        }
    } else {
        let port = if conn.port == 0 { 2379 } else { conn.port };
        format!("http://{}:{port}", conn.host)
    };

    let mut builder = Client::builder()
        .connection_string(&endpoint)
        .context("etcd connection_string 无效")?;

    match (&conn.username, &conn.password) {
        (Some(u), Some(p)) if !u.is_empty() => {
            builder = builder.credentials(u, p);
        }
        _ => {}
    }

    builder.build().context("连接 etcd 失败")
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let client = connect(conn)?;
    let resp = client.member_list().await.context("etcd member_list 失败")?;
    let n = resp.members().len();
    Ok(format!(
        "etcd OK — {n} members — {}",
        conn.endpoint_label()
    ))
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    let client = connect(conn)?;
    let prefix = match node.kind {
        NodeKind::Connection => "",
        NodeKind::Folder | NodeKind::Key => node.meta.path.as_deref().unwrap_or(""),
        _ => "",
    };

    let view = if prefix.is_empty() {
        client.list(..).keys_only().await.context("etcd list 失败")?
    } else {
        client
            .list_prefix(prefix)
            .keys_only()
            .await
            .context("etcd list_prefix 失败")?
    };

    let mut keys = Vec::new();
    let mut stream = view.into_stream();
    while let Some(item) = stream.next().await {
        let km = item.context("读取 etcd key 失败")?;
        keys.push(String::from_utf8_lossy(km.key().as_ref()).into_owned());
        if keys.len() >= 500 {
            break;
        }
    }
    keys.sort();

    let prefix_norm = if prefix.is_empty() {
        String::new()
    } else if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };

    let mut children = std::collections::BTreeSet::new();
    for k in &keys {
        let rest = if prefix_norm.is_empty() {
            k.as_str()
        } else {
            k.strip_prefix(&prefix_norm).unwrap_or(k.as_str())
        };
        if rest.is_empty() {
            continue;
        }
        if let Some((head, _)) = rest.split_once('/') {
            children.insert(format!("{prefix_norm}{head}/"));
        } else {
            children.insert(k.clone());
        }
    }

    Ok(children
        .into_iter()
        .map(|p| {
            let is_folder = p.ends_with('/');
            let label = p
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(&p)
                .to_string()
                + if is_folder { "/" } else { "" };
            TreeNode {
                id: format!("etcd:{}:{p}", conn.id),
                label,
                kind: if is_folder {
                    NodeKind::Folder
                } else {
                    NodeKind::Key
                },
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    path: Some(p),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: is_folder,
                loaded: !is_folder,
            }
        })
        .collect())
}

pub async fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let mut limit = MAX_RESULT_ROWS.min(200);
    let mut from = 0usize;
    let mut prefix = String::new();
    for line in query.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix("# limit=") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                limit = n.clamp(1, MAX_RESULT_ROWS);
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("# from=") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                from = n.min(100_000);
            }
            continue;
        }
        if t.starts_with('#') {
            continue;
        }
        prefix = t.to_string();
        break;
    }
    let client = connect(conn)?;
    let view = if prefix.is_empty() {
        client.list(..).await.context("etcd list 失败")?
    } else {
        client
            .list_prefix(prefix.as_str())
            .await
            .context("etcd list_prefix 失败")?
    };

    let mut rows = Vec::new();
    let mut stream = view.into_stream();
    let mut skipped = 0usize;
    let mut truncated = false;
    while let Some(item) = stream.next().await {
        let record = item.context("读取 etcd 记录失败")?;
        if skipped < from {
            skipped += 1;
            continue;
        }
        let key = String::from_utf8_lossy(record.key().as_ref()).into_owned();
        let value = String::from_utf8_lossy(record.value().as_ref()).into_owned();
        rows.push(vec![key, truncate_cell(&value, 400)]);
        if rows.len() >= limit {
            truncated = true;
            break;
        }
    }

    Ok(QueryResult {
        columns: vec!["key".into(), "value".into()],
        rows,
        message: format!(
            "prefix={} from={}{}",
            if prefix.is_empty() { "/" } else { &prefix },
            from,
            if truncated { " · 已截断" } else { "" }
        ),
        truncated,
        ..Default::default()
    })
}

pub async fn preview(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    let path = node.meta.path.as_deref().unwrap_or("");
    run_query(conn, path).await
}

#[allow(dead_code)]
pub async fn get_value(conn: &Connection, node: &TreeNode) -> Result<ValueView> {
    let key = node
        .meta
        .path
        .clone()
        .unwrap_or_else(|| node.label.clone());
    let client = connect(conn)?;
    let resp = client.get(key.as_str()).await.context("etcd get 失败")?;
    let content = resp
        .record()
        .map(|r| String::from_utf8_lossy(r.value().as_ref()).into_owned())
        .unwrap_or_else(|| "(nil)".into());
    Ok(ValueView {
        title: key,
        content,
        meta: "etcd".into(),
    })
}
