//! Oracle backend via pure-Rust `oracle-rs` (no Instant Client).

use anyhow::{bail, Context, Result};
use oracle_rs::{Config as OraConfig, Connection as OraConn, Value};

use crate::config::Connection;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode};
use crate::visual::{DEFAULT_PAGE_SIZE, MAX_RESULT_ROWS};

use super::truncate_cell;

async fn connect(conn: &Connection) -> Result<OraConn> {
    let host = if conn.host.trim().is_empty() {
        "127.0.0.1"
    } else {
        conn.host.trim()
    };
    let port = if conn.port == 0 { 1521 } else { conn.port };
    let service = conn
        .database
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("ORCL");
    let user = conn
        .username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("system");
    let pass = conn.password.as_deref().unwrap_or("");

    let cfg = OraConfig::new(host, port, service, user, pass);
    OraConn::connect_with_config(cfg)
        .await
        .with_context(|| format!("连接 Oracle 失败: {host}:{port}/{service}"))
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        other => truncate_cell(&other.to_string(), 2000),
    }
}

fn from_ora_result(result: oracle_rs::QueryResult) -> QueryResult {
    let columns: Vec<String> = result.columns.iter().map(|c| c.name.clone()).collect();
    let column_types: Vec<String> = result
        .columns
        .iter()
        .map(|c| format!("{:?}", c.oracle_type))
        .collect();
    let mut rows = Vec::with_capacity(result.rows.len().min(MAX_RESULT_ROWS));
    for row in result.rows.iter().take(MAX_RESULT_ROWS) {
        let cells: Vec<String> = (0..columns.len())
            .map(|i| row.get(i).map(value_to_string).unwrap_or_default())
            .collect();
        rows.push(cells);
    }
    let truncated = result.rows.len() > MAX_RESULT_ROWS;
    let affected = if result.rows_affected > 0 {
        Some(result.rows_affected)
    } else {
        None
    };
    QueryResult {
        columns,
        column_types,
        rows,
        message: if truncated {
            format!("已截断显示前 {MAX_RESULT_ROWS} 行")
        } else {
            String::new()
        },
        elapsed_ms: 0,
        affected,
        total: None,
        truncated,
    }
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let c = connect(conn).await?;
    let _ = c.query("SELECT 1 FROM dual", &[]).await?;
    Ok(format!("Oracle OK — {}", conn.endpoint_label()))
}

pub async fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let sql = query.trim().trim_end_matches(';');
    if sql.is_empty() {
        bail!("SQL 为空");
    }
    let c = connect(conn).await?;
    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.starts_with("SELECT")
        || upper.starts_with("WITH")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESCRIBE")
        || upper.starts_with("DESC ")
        || upper.starts_with("EXPLAIN")
    {
        let result = c.query(sql, &[]).await.context("Oracle 查询失败")?;
        Ok(from_ora_result(result))
    } else {
        let result = c.execute(sql, &[]).await.context("Oracle 执行失败")?;
        let _ = c.commit().await;
        Ok(QueryResult {
            message: format!("完成，影响 {} 行", result.rows_affected),
            affected: Some(result.rows_affected),
            ..Default::default()
        })
    }
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    let cat = node.meta.path.as_deref().unwrap_or("");
    match node.kind {
        NodeKind::Connection => list_schemas(conn).await,
        NodeKind::Database | NodeKind::Schema => {
            let schema = node
                .meta
                .schema
                .clone()
                .or_else(|| node.meta.database.clone())
                .unwrap_or_else(|| node.label.clone());
            Ok(schema_folders(conn, &schema))
        }
        NodeKind::Folder if cat == "nav:tables" => {
            list_relations(conn, &node.meta, false).await
        }
        NodeKind::Folder if cat == "nav:views" => list_relations(conn, &node.meta, true).await,
        NodeKind::Table => Ok(table_structure_folders(conn, &node.meta)),
        NodeKind::Folder if cat == "nav:columns" => list_columns(conn, &node.meta).await,
        NodeKind::Folder if cat == "nav:tbl_indexes" => list_indexes(conn, &node.meta).await,
        NodeKind::Folder if matches!(cat, "nav:fks" | "nav:uniques" | "nav:checks") => {
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

async fn list_schemas(conn: &Connection) -> Result<Vec<TreeNode>> {
    let c = connect(conn).await?;
    let sql = "SELECT username FROM all_users ORDER BY 1";
    let result = c.query(sql, &[]).await.context("列出 Oracle schema 失败")?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| {
            let name = r.get_string(0)?.to_string();
            Some(TreeNode {
                id: format!("ora:{}:schema:{name}", conn.id),
                label: name.clone(),
                kind: NodeKind::Schema,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(name.clone()),
                    schema: Some(name),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: true,
                loaded: false,
            })
        })
        .collect())
}

fn schema_folders(conn: &Connection, schema: &str) -> Vec<TreeNode> {
    ["tables", "views"]
        .into_iter()
        .map(|cat| {
            let path = format!("nav:{cat}");
            let label = match cat {
                "tables" => "表",
                "views" => "视图",
                _ => cat,
            };
            TreeNode {
                id: format!("ora:{}:{}:{path}", conn.id, schema),
                label: label.into(),
                kind: NodeKind::Folder,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(schema.into()),
                    schema: Some(schema.into()),
                    path: Some(path),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: true,
                loaded: false,
            }
        })
        .collect()
}

fn table_structure_folders(conn: &Connection, meta: &NodeMeta) -> Vec<TreeNode> {
    let schema = meta.schema.as_deref().or(meta.database.as_deref()).unwrap_or("");
    let table = meta.table.as_deref().unwrap_or("");
    ["columns", "tbl_indexes"]
        .into_iter()
        .map(|cat| {
            let path = format!("nav:{cat}");
            let label = match cat {
                "columns" => "列",
                "tbl_indexes" => "索引",
                _ => cat,
            };
            TreeNode {
                id: format!("ora:{}:{schema}:{table}:{path}", conn.id),
                label: label.into(),
                kind: NodeKind::Folder,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: meta.database.clone(),
                    schema: meta.schema.clone(),
                    table: meta.table.clone(),
                    path: Some(path),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: true,
                loaded: false,
            }
        })
        .collect()
}

async fn list_relations(conn: &Connection, meta: &NodeMeta, views: bool) -> Result<Vec<TreeNode>> {
    let schema = meta
        .schema
        .as_deref()
        .or(meta.database.as_deref())
        .unwrap_or("");
    let c = connect(conn).await?;
    let sql = if views {
        format!(
            "SELECT view_name FROM all_views WHERE owner = '{}' ORDER BY 1",
            schema.replace('\'', "''")
        )
    } else {
        format!(
            "SELECT table_name FROM all_tables WHERE owner = '{}' ORDER BY 1",
            schema.replace('\'', "''")
        )
    };
    let result = c.query(&sql, &[]).await?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| {
            let name = r.get_string(0)?.to_string();
            Some(TreeNode {
                id: format!("ora:{}:{schema}:table:{name}", conn.id),
                label: name.clone(),
                kind: NodeKind::Table,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(schema.into()),
                    schema: Some(schema.into()),
                    table: Some(name),
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
    let schema = meta
        .schema
        .as_deref()
        .or(meta.database.as_deref())
        .unwrap_or("");
    let table = meta.table.as_deref().unwrap_or("");
    let c = connect(conn).await?;
    let sql = format!(
        "SELECT column_name, data_type, nullable FROM all_tab_columns \
         WHERE owner = '{}' AND table_name = '{}' ORDER BY column_id",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );
    let result = c.query(&sql, &[]).await?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| {
            let name = r.get_string(0)?.to_string();
            let dtype = r.get_string(1).unwrap_or("");
            let nullable = r.get_string(2).unwrap_or("");
            Some(TreeNode {
                id: format!("ora:{}:{schema}:{table}:col:{name}", conn.id),
                label: format!("{name}  {dtype}"),
                kind: NodeKind::Column,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(schema.into()),
                    schema: Some(schema.into()),
                    table: Some(table.into()),
                    path: Some(name),
                    meta_line: Some(if nullable.eq_ignore_ascii_case("Y") {
                        "NULL".into()
                    } else {
                        "NOT NULL".into()
                    }),
                    ..Default::default()
                },
                children: Vec::new(),
                expandable: false,
                loaded: true,
            })
        })
        .collect())
}

async fn list_indexes(conn: &Connection, meta: &NodeMeta) -> Result<Vec<TreeNode>> {
    let schema = meta
        .schema
        .as_deref()
        .or(meta.database.as_deref())
        .unwrap_or("");
    let table = meta.table.as_deref().unwrap_or("");
    let c = connect(conn).await?;
    let sql = format!(
        "SELECT index_name, uniqueness FROM all_indexes \
         WHERE owner = '{}' AND table_name = '{}' ORDER BY 1",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );
    let result = c.query(&sql, &[]).await?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| {
            let name = r.get_string(0)?.to_string();
            let uniq = r.get_string(1).unwrap_or("");
            Some(TreeNode {
                id: format!("ora:{}:{schema}:{table}:idx:{name}", conn.id),
                label: name.clone(),
                kind: NodeKind::Index,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(schema.into()),
                    schema: Some(schema.into()),
                    table: Some(table.into()),
                    path: Some(name),
                    meta_line: Some(uniq.to_string()),
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
            let schema = node
                .meta
                .schema
                .as_deref()
                .or(node.meta.database.as_deref())
                .unwrap_or("");
            let table = node.meta.table.as_deref().unwrap_or(&node.label);
            let sql = format!(
                "SELECT * FROM {}.{} FETCH FIRST {} ROWS ONLY",
                quote_ident(schema),
                quote_ident(table),
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
