use anyhow::{bail, Context, Result};
use futures::TryStreamExt;
use mongodb::bson::{doc, Bson, Document};
use mongodb::options::ClientOptions;
use mongodb::Client;

use crate::config::Connection;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode, ValueView};
use crate::visual::DEFAULT_PAGE_SIZE;

use super::{json_pretty, truncate_cell};

async fn client(conn: &Connection) -> Result<Client> {
    let uri = if let Some(url) = conn.url.as_ref().filter(|u| !u.is_empty()) {
        url.clone()
    } else {
        let port = if conn.port == 0 { 27017 } else { conn.port };
        match (&conn.username, &conn.password) {
            (Some(u), Some(p)) if !u.is_empty() => {
                format!("mongodb://{u}:{p}@{}:{port}", conn.host)
            }
            _ => format!("mongodb://{}:{port}", conn.host),
        }
    };
    let opts = ClientOptions::parse(&uri)
        .await
        .context("解析 MongoDB URI 失败")?;
    Client::with_options(opts).context("创建 MongoDB 客户端失败")
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let c = client(conn).await?;
    c.database("admin")
        .run_command(doc! { "ping": 1 })
        .await
        .context("MongoDB ping 失败")?;
    Ok(format!("MongoDB OK — {}", conn.endpoint_label()))
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    let c = client(conn).await?;
    match node.kind {
        NodeKind::Connection => {
            let names = c.list_database_names().await?;
            Ok(names
                .into_iter()
                .filter(|n| n != "local" && n != "config")
                .map(|name| TreeNode {
                    id: format!("mongo:{}:db:{name}", conn.id),
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
                .collect())
        }
        NodeKind::Database => {
            let db = node.meta.database.as_deref().unwrap_or("admin");
            let database = c.database(db);
            let names = database.list_collection_names().await?;
            let mut out = Vec::with_capacity(names.len());
            for name in names {
                let mut meta_line = None;
                if let Ok(n) = database
                    .collection::<Document>(&name)
                    .estimated_document_count()
                    .await
                {
                    meta_line = Some(crate::backends::format_row_count(n as i64));
                }
                out.push(TreeNode {
                    id: format!("mongo:{}:col:{db}:{name}", conn.id),
                    label: name.clone(),
                    kind: NodeKind::Collection,
                    connection_id: conn.id.clone(),
                    meta: NodeMeta {
                        database: Some(db.to_string()),
                        table: Some(name.clone()),
                        path: Some(name),
                        meta_line,
                        ..Default::default()
                    },
                    children: Vec::new(),
                    expandable: false,
                    loaded: true,
                });
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

pub async fn run_query(conn: &Connection, query: &str, context: &NodeMeta) -> Result<QueryResult> {
    let q = query.trim();
    if q.is_empty() {
        bail!("查询为空");
    }
    let c = client(conn).await?;
    let db_name = context
        .database
        .as_deref()
        .or(conn.database.as_deref())
        .unwrap_or("admin");

    // 支持两种：纯 filter JSON，或 command JSON
    let doc: Document = serde_json::from_str(q)
        .or_else(|_| {
            // 允许 JS 风格松散？仅严格 JSON
            Err(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid",
            )))
        })
        .context("Mongo 查询需为 JSON")?;

    if doc.contains_key("find")
        || doc.contains_key("aggregate")
        || doc.contains_key("count")
        || doc.contains_key("listIndexes")
        || doc.contains_key("collStats")
        || doc.contains_key("dbStats")
    {
        let result = c
            .database(db_name)
            .run_command(doc)
            .await
            .context("执行 Mongo 命令失败")?;
        return Ok(document_result(&result));
    }

    // 当作 filter，对当前 collection find
    let coll_name = context
        .table
        .as_deref()
        .or(context.path.as_deref())
        .context("请先在左侧选择集合，或在 JSON 中指定 find")?;
    let coll = c.database(db_name).collection::<Document>(coll_name);
    let mut cursor = coll
        .find(doc)
        .limit(i64::from(DEFAULT_PAGE_SIZE))
        .await?;
    let mut docs = Vec::new();
    while let Some(d) = cursor.try_next().await? {
        docs.push(d);
        if docs.len() as u32 >= DEFAULT_PAGE_SIZE {
            break;
        }
    }
    Ok(docs_to_table(&docs))
}

pub async fn preview(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    match node.kind {
        NodeKind::Collection => {
            let db = node.meta.database.as_deref().unwrap_or("admin");
            let coll_name = node.meta.table.as_deref().unwrap_or(&node.label);
            let c = client(conn).await?;
            let coll = c.database(db).collection::<Document>(coll_name);
            let mut cursor = coll
                .find(doc! {})
                .limit(i64::from(DEFAULT_PAGE_SIZE))
                .await?;
            let mut docs = Vec::new();
            while let Some(d) = cursor.try_next().await? {
                docs.push(d);
                if docs.len() as u32 >= DEFAULT_PAGE_SIZE {
                    break;
                }
            }
            Ok(docs_to_table(&docs))
        }
        _ => Ok(QueryResult {
            message: "请选择集合以预览文档".into(),
            ..Default::default()
        }),
    }
}

#[allow(dead_code)]
pub async fn get_value(conn: &Connection, node: &TreeNode) -> Result<ValueView> {
    let r = preview(conn, node).await?;
    Ok(ValueView {
        title: node.label.clone(),
        content: r
            .rows
            .iter()
            .map(|row| row.join(" | "))
            .collect::<Vec<_>>()
            .join("\n"),
        meta: r.message,
    })
}

fn docs_to_table(docs: &[Document]) -> QueryResult {
    let mut cols = std::collections::BTreeSet::new();
    for d in docs {
        for k in d.keys() {
            cols.insert(k.to_string());
        }
    }
    let columns: Vec<String> = cols.into_iter().collect();
    let rows: Vec<Vec<String>> = docs
        .iter()
        .map(|d| {
            columns
                .iter()
                .map(|c| {
                    d.get(c.as_str())
                        .map(bson_to_string)
                        .map(|s| truncate_cell(&s, 300))
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();
    QueryResult {
        message: format!("{} docs", rows.len()),
        columns,
        rows,
        ..Default::default()
    }
}

fn document_result(doc: &Document) -> QueryResult {
    if let Ok(cursor) = doc.get_document("cursor") {
        if let Ok(first) = cursor.get_array("firstBatch") {
            let docs: Vec<Document> = first
                .iter()
                .filter_map(|b| b.as_document().cloned())
                .collect();
            return docs_to_table(&docs);
        }
    }
    QueryResult {
        columns: vec!["json".into()],
        rows: vec![vec![json_pretty(&Bson::Document(doc.clone()).into_relaxed_extjson())]],
        message: "OK".into(),
        ..Default::default()
    }
}

fn bson_to_string(b: &Bson) -> String {
    match b {
        Bson::String(s) => s.clone(),
        Bson::ObjectId(oid) => oid.to_hex(),
        Bson::Int32(i) => i.to_string(),
        Bson::Int64(i) => i.to_string(),
        Bson::Double(f) => f.to_string(),
        Bson::Boolean(b) => b.to_string(),
        Bson::Null => "null".into(),
        other => other.clone().into_relaxed_extjson().to_string(),
    }
}
