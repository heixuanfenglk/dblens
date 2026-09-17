//! Snowflake backend via `snowflake-api` (password auth).

use anyhow::{bail, Context, Result};
use arrow_array::Array;
use arrow_cast::display::array_value_to_string;
use snowflake_api::{JsonResult, QueryResult as SfResult, RecordBatch, SnowflakeApi};

use crate::config::Connection;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode};
use crate::visual::{DEFAULT_PAGE_SIZE, MAX_RESULT_ROWS};

use super::truncate_cell;

fn connect(conn: &Connection) -> Result<SnowflakeApi> {
    let account = conn.host.trim();
    if account.is_empty() {
        bail!("请填写 Snowflake Account（如 xy12345.us-east-1）");
    }
    let user = conn
        .username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .context("请填写用户名")?;
    let pass = conn.password.as_deref().unwrap_or("");
    if pass.is_empty() {
        bail!("请填写密码");
    }
    let warehouse = conn.warehouse.as_deref().filter(|s| !s.trim().is_empty());
    let database = conn.database.as_deref().filter(|s| !s.trim().is_empty());
    let role = conn.role.as_deref().filter(|s| !s.trim().is_empty());

    SnowflakeApi::with_password_auth(account, warehouse, database, None, user, role, pass)
        .context("创建 Snowflake 连接失败")
}

fn json_cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => truncate_cell(s, 2000),
        other => truncate_cell(&other.to_string(), 2000),
    }
}

fn from_json(j: JsonResult) -> QueryResult {
    let columns: Vec<String> = j.schema.iter().map(|f| f.name.clone()).collect();
    let column_types: Vec<String> = j.schema.iter().map(|f| format!("{:?}", f.type_)).collect();
    let mut rows = Vec::new();
    if let Some(arr) = j.value.as_array() {
        for row_v in arr.iter().take(MAX_RESULT_ROWS) {
            if let Some(cells) = row_v.as_array() {
                rows.push(cells.iter().map(json_cell).collect());
            } else if let Some(obj) = row_v.as_object() {
                rows.push(
                    columns
                        .iter()
                        .map(|c| obj.get(c).map(json_cell).unwrap_or_default())
                        .collect(),
                );
            }
        }
    }
    let truncated = j
        .value
        .as_array()
        .map(|a| a.len() > MAX_RESULT_ROWS)
        .unwrap_or(false);
    QueryResult {
        columns,
        column_types,
        rows,
        message: if truncated {
            format!("已截断显示前 {MAX_RESULT_ROWS} 行")
        } else {
            String::new()
        },
        truncated,
        ..Default::default()
    }
}

fn from_arrow(batches: Vec<RecordBatch>) -> Result<QueryResult> {
    if batches.is_empty() {
        return Ok(QueryResult::default());
    }
    let schema = batches[0].schema();
    let columns: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let column_types: Vec<String> = schema
        .fields()
        .iter()
        .map(|f| f.data_type().to_string())
        .collect();
    let mut rows = Vec::new();
    let mut truncated = false;
    'outer: for batch in &batches {
        for row_idx in 0..batch.num_rows() {
            if rows.len() >= MAX_RESULT_ROWS {
                truncated = true;
                break 'outer;
            }
            let mut cells = Vec::with_capacity(columns.len());
            for col_idx in 0..batch.num_columns() {
                let col = batch.column(col_idx);
                let s = if col.is_null(row_idx) {
                    String::new()
                } else {
                    array_value_to_string(col.as_ref(), row_idx)
                        .unwrap_or_else(|_| "?".into())
                };
                cells.push(truncate_cell(&s, 2000));
            }
            rows.push(cells);
        }
    }
    Ok(QueryResult {
        columns,
        column_types,
        rows,
        message: if truncated {
            format!("已截断显示前 {MAX_RESULT_ROWS} 行")
        } else {
            String::new()
        },
        truncated,
        ..Default::default()
    })
}

fn from_sf(result: SfResult) -> Result<QueryResult> {
    match result {
        SfResult::Empty => Ok(QueryResult {
            message: "完成".into(),
            ..Default::default()
        }),
        SfResult::Json(j) => Ok(from_json(j)),
        SfResult::Arrow(batches) => from_arrow(batches),
    }
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let api = connect(conn)?;
    let _ = api.exec("SELECT 1").await.context("Snowflake ping 失败")?;
    Ok(format!("Snowflake OK — {}", conn.endpoint_label()))
}

pub async fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let sql = query.trim().trim_end_matches(';');
    if sql.is_empty() {
        bail!("SQL 为空");
    }
    let api = connect(conn)?;
    let result = api.exec(sql).await.context("Snowflake 查询失败")?;
    from_sf(result)
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Pick a column from SHOW results by common name variants.
fn show_name(row: &[String], columns: &[String]) -> Option<String> {
    for key in ["name", "NAME", "table_name", "TABLE_NAME", "schema_name", "database_name"] {
        if let Some(i) = columns.iter().position(|c| c.eq_ignore_ascii_case(key)) {
            if let Some(v) = row.get(i).filter(|s| !s.is_empty()) {
                return Some(v.clone());
            }
        }
    }
    row.first().cloned().filter(|s| !s.is_empty())
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
                    let name = show_name(r, &qr.columns)?;
                    Some(TreeNode {
                        id: format!("sf:{}:db:{name}", conn.id),
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
            let sql = format!("SHOW SCHEMAS IN DATABASE {}", quote_ident(db));
            let qr = run_query(conn, &sql).await?;
            Ok(qr
                .rows
                .iter()
                .filter_map(|r| {
                    let name = show_name(r, &qr.columns)?;
                    Some(TreeNode {
                        id: format!("sf:{}:{db}:schema:{name}", conn.id),
                        label: name.clone(),
                        kind: NodeKind::Schema,
                        connection_id: conn.id.clone(),
                        meta: NodeMeta {
                            database: Some(db.into()),
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
        NodeKind::Schema => Ok(schema_folders(conn, &node.meta)),
        NodeKind::Folder if cat == "nav:tables" => list_tables(conn, &node.meta, false).await,
        NodeKind::Folder if cat == "nav:views" => list_tables(conn, &node.meta, true).await,
        NodeKind::Table => Ok(table_structure_folders(conn, &node.meta)),
        NodeKind::Folder if cat == "nav:columns" => list_columns(conn, &node.meta).await,
        _ => Ok(Vec::new()),
    }
}

fn schema_folders(conn: &Connection, meta: &NodeMeta) -> Vec<TreeNode> {
    let db = meta.database.as_deref().unwrap_or("");
    let schema = meta.schema.as_deref().unwrap_or("");
    ["tables", "views"]
        .into_iter()
        .map(|cat| {
            let path = format!("nav:{cat}");
            let label = if cat == "tables" { "表" } else { "视图" };
            TreeNode {
                id: format!("sf:{}:{db}.{schema}:{path}", conn.id),
                label: label.into(),
                kind: NodeKind::Folder,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: meta.database.clone(),
                    schema: meta.schema.clone(),
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
    let db = meta.database.as_deref().unwrap_or("");
    let schema = meta.schema.as_deref().unwrap_or("");
    let table = meta.table.as_deref().unwrap_or("");
    vec![TreeNode {
        id: format!("sf:{}:{db}.{schema}.{table}:nav:columns", conn.id),
        label: "列".into(),
        kind: NodeKind::Folder,
        connection_id: conn.id.clone(),
        meta: NodeMeta {
            database: meta.database.clone(),
            schema: meta.schema.clone(),
            table: meta.table.clone(),
            path: Some("nav:columns".into()),
            ..Default::default()
        },
        children: Vec::new(),
        expandable: true,
        loaded: false,
    }]
}

async fn list_tables(conn: &Connection, meta: &NodeMeta, views: bool) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("");
    let schema = meta.schema.as_deref().unwrap_or("");
    let kind = if views { "VIEWS" } else { "TABLES" };
    let sql = format!(
        "SHOW {kind} IN SCHEMA {}.{}",
        quote_ident(db),
        quote_ident(schema)
    );
    let qr = run_query(conn, &sql).await?;
    Ok(qr
        .rows
        .iter()
        .filter_map(|r| {
            let name = show_name(r, &qr.columns)?;
            Some(TreeNode {
                id: format!("sf:{}:{db}.{schema}:table:{name}", conn.id),
                label: name.clone(),
                kind: NodeKind::Table,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(db.into()),
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
    let db = meta.database.as_deref().unwrap_or("");
    let schema = meta.schema.as_deref().unwrap_or("");
    let table = meta.table.as_deref().unwrap_or("");
    let sql = format!(
        "DESCRIBE TABLE {}.{}.{}",
        quote_ident(db),
        quote_ident(schema),
        quote_ident(table)
    );
    let qr = run_query(conn, &sql).await?;
    Ok(qr
        .rows
        .iter()
        .filter_map(|r| {
            let name = show_name(r, &qr.columns)?;
            let dtype = r.get(1).cloned().unwrap_or_default();
            Some(TreeNode {
                id: format!("sf:{}:{db}.{schema}.{table}:col:{name}", conn.id),
                label: if dtype.is_empty() {
                    name.clone()
                } else {
                    format!("{name}  {dtype}")
                },
                kind: NodeKind::Column,
                connection_id: conn.id.clone(),
                meta: NodeMeta {
                    database: Some(db.into()),
                    schema: Some(schema.into()),
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
            let schema = node.meta.schema.as_deref().unwrap_or("PUBLIC");
            let table = node.meta.table.as_deref().unwrap_or(&node.label);
            let sql = format!(
                "SELECT * FROM {}.{}.{} LIMIT {}",
                quote_ident(db),
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
