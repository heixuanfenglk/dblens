use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::config::Connection;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode, ValueView};

fn addr(conn: &Connection) -> String {
    if let Some(url) = conn.url.as_ref().filter(|u| !u.is_empty()) {
        // memcache://host:port 或 host:port
        return url
            .trim_start_matches("memcache://")
            .trim_start_matches("memcached://")
            .to_string();
    }
    let port = if conn.port == 0 { 11211 } else { conn.port };
    format!("{}:{port}", conn.host)
}

async fn send_command(conn: &Connection, command: &str) -> Result<String> {
    let stream = TcpStream::connect(addr(conn))
        .await
        .with_context(|| format!("连接 Memcached 失败: {}", addr(conn)))?;
    let (reader, mut writer) = stream.into_split();
    writer
        .write_all(command.as_bytes())
        .await
        .context("写入命令失败")?;
    if !command.ends_with("\r\n") {
        writer.write_all(b"\r\n").await?;
    }
    writer.flush().await?;

    let mut reader = BufReader::new(reader);
    let mut out = String::new();
    let mut line = String::new();
    // 读取直到 END / OK / ERROR / STORED 等终止行，或单行响应
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        out.push_str(&line);
        let t = line.trim_end();
        if t == "END"
            || t == "OK"
            || t == "STORED"
            || t == "NOT_STORED"
            || t == "EXISTS"
            || t == "NOT_FOUND"
            || t.starts_with("ERROR")
            || t.starts_with("CLIENT_ERROR")
            || t.starts_with("SERVER_ERROR")
            || t.starts_with("VALUE ")
            || (command.to_ascii_uppercase().starts_with("VERSION") && !t.is_empty())
            || (command.to_ascii_uppercase().starts_with("STATS") && t == "END")
        {
            // VALUE 后面还有数据行 + END
            if t.starts_with("VALUE ") {
                // 继续读到 END
                loop {
                    line.clear();
                    let n = reader.read_line(&mut line).await?;
                    if n == 0 {
                        break;
                    }
                    out.push_str(&line);
                    if line.trim_end() == "END" {
                        break;
                    }
                }
            }
            break;
        }
        // stats 多行直到 END
        if command.to_ascii_uppercase().starts_with("STATS") && t == "END" {
            break;
        }
    }
    Ok(out)
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let resp = send_command(conn, "version").await?;
    Ok(format!(
        "Memcached OK — {} ({})",
        conn.endpoint_label(),
        resp.trim()
    ))
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    match node.kind {
        NodeKind::Connection => Ok(vec![TreeNode {
            id: format!("mc:{}:stats", conn.id),
            label: "stats".into(),
            kind: NodeKind::Folder,
            connection_id: conn.id.clone(),
            meta: NodeMeta::default(),
            children: Vec::new(),
            expandable: true,
            loaded: false,
        }]),
        NodeKind::Folder => {
            let resp = send_command(conn, "stats").await?;
            let mut nodes = Vec::new();
            for line in resp.lines() {
                // STAT pid 123
                let Some(rest) = line.strip_prefix("STAT ") else {
                    continue;
                };
                let mut parts = rest.splitn(2, ' ');
                let k = parts.next().unwrap_or("").to_string();
                let v = parts.next().unwrap_or("").to_string();
                nodes.push(TreeNode {
                    id: format!("mc:{}:stat:{k}", conn.id),
                    label: format!("{k} = {v}"),
                    kind: NodeKind::Key,
                    connection_id: conn.id.clone(),
                    meta: NodeMeta {
                        path: Some(k),
                        ..Default::default()
                    },
                    children: Vec::new(),
                    expandable: false,
                    loaded: true,
                });
            }
            Ok(nodes)
        }
        _ => Ok(Vec::new()),
    }
}

pub async fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let q = query
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("")
        .to_string();
    if q.is_empty() {
        bail!("查询为空");
    }
    let upper = q.to_ascii_uppercase();
    if upper == "STATS" || upper.starts_with("STATS ") {
        let resp = send_command(conn, &q.to_ascii_lowercase()).await?;
        let mut rows = Vec::new();
        for line in resp.lines() {
            if let Some(rest) = line.strip_prefix("STAT ") {
                let mut parts = rest.splitn(2, ' ');
                let k = parts.next().unwrap_or("").to_string();
                let v = parts.next().unwrap_or("").to_string();
                rows.push(vec![k, v]);
            }
        }
        return Ok(QueryResult {
            columns: vec!["key".into(), "value".into()],
            rows,
            message: "STATS OK".into(),
            ..Default::default()
        });
    }

    let key = if let Some(k) = upper.strip_prefix("GET ") {
        k.trim()
    } else if !upper.contains(' ') {
        q.as_str()
    } else {
        bail!("仅支持 GET <key> 或 STATS");
    };

    let resp = send_command(conn, &format!("get {key}")).await?;
    let value = parse_get_value(&resp);
    Ok(QueryResult {
        columns: vec!["key".into(), "value".into()],
        rows: vec![vec![
            key.to_string(),
            value.unwrap_or_else(|| "(nil)".into()),
        ]],
        message: "GET OK".into(),
        ..Default::default()
    })
}

pub async fn preview(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    if node.kind == NodeKind::Key {
        if let Some(k) = &node.meta.path {
            return run_query(conn, &format!("GET {k}")).await;
        }
    }
    run_query(conn, "STATS").await
}

#[allow(dead_code)]
pub async fn get_value(conn: &Connection, node: &TreeNode) -> Result<ValueView> {
    let key = node
        .meta
        .path
        .clone()
        .unwrap_or_else(|| node.label.clone());
    let r = run_query(conn, &format!("GET {key}")).await?;
    Ok(ValueView {
        title: key,
        content: r
            .rows
            .first()
            .and_then(|r| r.get(1))
            .cloned()
            .unwrap_or_default(),
        meta: "memcached".into(),
    })
}

fn parse_get_value(resp: &str) -> Option<String> {
    let mut lines = resp.lines();
    let header = lines.next()?;
    if !header.starts_with("VALUE ") {
        return None;
    }
    let mut body = String::new();
    for line in lines {
        if line == "END" {
            break;
        }
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(line);
    }
    Some(body)
}
