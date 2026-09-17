use anyhow::{bail, Context, Result};
use tiberius::{
    AuthMethod, Client, ColumnType, Config as TibConfig, EncryptionLevel, QueryItem, Row,
    SqlBrowser,
};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::config::{Connection, MssqlAuthMode};
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode};
use crate::visual::{DEFAULT_PAGE_SIZE, MAX_RESULT_ROWS};

use super::truncate_cell;

/// Split `HOST\INSTANCE` / `HOST/INSTANCE` into host + optional instance name.
fn parse_mssql_host(raw: &str) -> (String, Option<String>) {
    let raw = raw.trim();
    if raw.is_empty() {
        return ("127.0.0.1".into(), None);
    }
    for sep in ['\\', '/'] {
        if let Some((h, inst)) = raw.split_once(sep) {
            let h = h.trim();
            let inst = inst.trim();
            if !h.is_empty() && !inst.is_empty() {
                return (h.to_string(), Some(inst.to_string()));
            }
        }
    }
    (raw.to_string(), None)
}

async fn connect(conn: &Connection) -> Result<Client<Compat<TcpStream>>> {
    let mut config = TibConfig::new();
    let raw_host = if conn.host.trim().is_empty() {
        "127.0.0.1"
    } else {
        conn.host.trim()
    };
    let (host, instance) = parse_mssql_host(raw_host);
    let port = if conn.port == 0 { 1433 } else { conn.port };
    config.host(&host);
    if let Some(inst) = &instance {
        config.instance_name(inst);
    } else {
        config.port(port);
    }
    if let Some(db) = conn.database.as_ref().filter(|d| !d.trim().is_empty()) {
        config.database(db.trim());
    }

    apply_auth(&mut config, conn)?;

    if conn.insecure {
        config.encryption(EncryptionLevel::NotSupported);
    } else {
        // Local / self-signed certs are common; trust so handshake can proceed.
        config.encryption(EncryptionLevel::Required);
        config.trust_cert();
    }

    let tcp = if instance.is_some() {
        TcpStream::connect_named(&config)
            .await
            .with_context(|| {
                format!(
                    "SQL Browser 解析命名实例失败: {}\\{}",
                    host,
                    instance.as_deref().unwrap_or("")
                )
            })?
    } else {
        TcpStream::connect(config.get_addr())
            .await
            .with_context(|| format!("TCP 连接失败: {}", config.get_addr()))?
    };
    tcp.set_nodelay(true)?;
    Client::connect(config, tcp.compat_write())
        .await
        .context("MSSQL 握手失败")
}

fn apply_auth(config: &mut TibConfig, conn: &Connection) -> Result<()> {
    let user = conn.username.as_deref().unwrap_or("").trim();
    let pass = conn.password.as_deref().unwrap_or("");
    match conn.auth_mode {
        MssqlAuthMode::SqlServer => {
            let u = if user.is_empty() { "sa" } else { user };
            config.authentication(AuthMethod::sql_server(u, pass));
            Ok(())
        }
        MssqlAuthMode::Windows => {
            #[cfg(windows)]
            {
                if user.is_empty() {
                    config.authentication(AuthMethod::Integrated);
                } else {
                    config.authentication(AuthMethod::windows(user, pass));
                }
                Ok(())
            }
            #[cfg(not(windows))]
            {
                let _ = (config, user, pass);
                bail!("Windows 验证仅在 Windows 平台可用");
            }
        }
        mode => bail!(
            "{} 暂未实现，请改用「SQL Server 验证」或「Windows 验证」",
            mode.label()
        ),
    }
}

pub async fn ping(conn: &Connection) -> Result<String> {
    let mut c = connect(conn).await?;
    let stream = c.simple_query("SELECT 1 AS ok").await?;
    let _ = stream.into_results().await?;
    Ok(format!("SQL Server OK — {}", conn.endpoint_label()))
}

fn bracket_ident(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

fn sql_nvarchar_literal(s: &str) -> String {
    format!("N'{}'", s.replace('\'', "''"))
}

async fn use_database(c: &mut Client<Compat<TcpStream>>, db: &str) -> Result<()> {
    let sql = format!("USE {}", bracket_ident(db));
    let stream = c
        .simple_query(sql)
        .await
        .with_context(|| format!("切换数据库失败: {db}"))?;
    let _ = stream.into_results().await?;
    Ok(())
}

pub async fn list_children(conn: &Connection, node: &TreeNode) -> Result<Vec<TreeNode>> {
    let cat = node.meta.path.as_deref().unwrap_or("");

    // Pure UI structure — no round-trip.
    match node.kind {
        NodeKind::Schema => {
            return Ok(schema_category_folders(conn, &node.meta));
        }
        NodeKind::Table => {
            return Ok(table_structure_folders(conn, &node.meta));
        }
        _ => {}
    }

    let mut c = connect(conn).await?;
    match node.kind {
        NodeKind::Connection => {
            // ONLINE only — offline DBs can't be browsed and confuse the tree.
            let names = query_cols(
                &mut c,
                "SELECT name FROM sys.databases WHERE state = 0 ORDER BY name",
                1,
            )
            .await?;
            Ok(names
                .into_iter()
                .filter_map(|row| {
                    let name = row.into_iter().next()?;
                    if name.is_empty() {
                        return None;
                    }
                    Some(db_node(conn, &name))
                })
                .collect())
        }
        // Navicat / DBeaver style: database → schemas (dbo, guest, …)
        NodeKind::Database => {
            let db = node.meta.database.as_deref().unwrap_or("master");
            list_mssql_schemas(&mut c, conn, db).await
        }
        NodeKind::Folder if cat == "nav:tables" => {
            list_mssql_tables(&mut c, conn, &node.meta, false).await
        }
        NodeKind::Folder if cat == "nav:views" => {
            list_mssql_tables(&mut c, conn, &node.meta, true).await
        }
        NodeKind::Folder if cat == "nav:functions" => {
            list_mssql_functions(&mut c, conn, &node.meta).await
        }
        NodeKind::Folder if cat == "nav:indexes" => {
            list_mssql_indexes(&mut c, conn, &node.meta, None).await
        }
        NodeKind::Folder if cat == "nav:triggers" => {
            list_mssql_triggers(&mut c, conn, &node.meta, None).await
        }
        NodeKind::Folder if cat == "nav:columns" => {
            list_mssql_columns(&mut c, conn, &node.meta).await
        }
        NodeKind::Folder if cat == "nav:tbl_indexes" => {
            list_mssql_indexes(&mut c, conn, &node.meta, node.meta.table.as_deref()).await
        }
        NodeKind::Folder if cat == "nav:tbl_triggers" => {
            list_mssql_triggers(&mut c, conn, &node.meta, node.meta.table.as_deref()).await
        }
        NodeKind::Folder if matches!(cat, "nav:fks" | "nav:uniques" | "nav:checks") => {
            list_mssql_constraints(&mut c, conn, &node.meta, cat).await
        }
        _ => Ok(Vec::new()),
    }
}

fn db_node(conn: &Connection, name: &str) -> TreeNode {
    TreeNode {
        id: format!("mssql:{}:db:{name}", conn.id),
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

fn schema_node(conn: &Connection, db: &str, schema: &str) -> TreeNode {
    TreeNode {
        id: format!("mssql:{}:schema:{db}:{schema}", conn.id),
        label: schema.to_string(),
        kind: NodeKind::Schema,
        connection_id: conn.id.clone(),
        meta: NodeMeta {
            database: Some(db.to_string()),
            schema: Some(schema.to_string()),
            ..Default::default()
        },
        children: Vec::new(),
        expandable: true,
        loaded: false,
    }
}

fn table_node(conn: &Connection, db: &str, schema: &str, name: &str, is_view: bool) -> TreeNode {
    // Under a schema folder the label is just the object name (Navicat-style).
    let label = name.to_string();
    TreeNode {
        id: format!(
            "mssql:{}:{}:{db}:{schema}.{name}",
            conn.id,
            if is_view { "view" } else { "tbl" }
        ),
        label,
        kind: NodeKind::Table,
        connection_id: conn.id.clone(),
        meta: NodeMeta {
            database: Some(db.to_string()),
            schema: Some(schema.to_string()),
            table: Some(name.to_string()),
            path: Some(format!("{schema}.{name}")),
            status: is_view.then(|| "view".into()),
            ..Default::default()
        },
        children: Vec::new(),
        expandable: true,
        loaded: false,
    }
}

fn folder_node(conn: &Connection, id_suffix: &str, label: &str, meta: NodeMeta) -> TreeNode {
    TreeNode {
        id: format!("mssql:{}:{id_suffix}", conn.id),
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
        id: format!("mssql:{}:{id_suffix}", conn.id),
        label: label.into(),
        kind,
        connection_id: conn.id.clone(),
        meta,
        children: Vec::new(),
        expandable: false,
        loaded: true,
    }
}

/// Folders under a schema — mirrors Navicat / DBeaver.
fn schema_category_folders(conn: &Connection, parent: &NodeMeta) -> Vec<TreeNode> {
    let db = parent.database.as_deref().unwrap_or("");
    let schema = parent.schema.as_deref().unwrap_or("dbo");
    let key = format!("{db}:{schema}");
    [
        ("nav:tables", "表"),
        ("nav:views", "视图"),
        ("nav:functions", "函数"),
        ("nav:indexes", "索引"),
        ("nav:triggers", "触发器"),
    ]
    .into_iter()
    .map(|(path, label)| {
        let mut meta = parent.clone();
        meta.path = Some(path.into());
        folder_node(conn, &format!("sch:{key}:{path}"), label, meta)
    })
    .collect()
}

fn table_structure_folders(conn: &Connection, parent: &NodeMeta) -> Vec<TreeNode> {
    let db = parent.database.as_deref().unwrap_or("");
    let schema = parent.schema.as_deref().unwrap_or("dbo");
    let table = parent.table.as_deref().unwrap_or("");
    let key = format!("{db}:{schema}:{table}");
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

async fn query_cols(
    c: &mut Client<Compat<TcpStream>>,
    sql: impl AsRef<str>,
    ncols: usize,
) -> Result<Vec<Vec<String>>> {
    let mut stream = c.simple_query(sql.as_ref()).await?;
    let mut out = Vec::new();
    while let Some(item) = futures::StreamExt::next(&mut stream).await {
        let item = item?;
        if let QueryItem::Row(row) = item {
            let mut cols = Vec::with_capacity(ncols);
            for i in 0..ncols {
                cols.push(mssql_cell(&row, i));
            }
            out.push(cols);
        }
    }
    Ok(out)
}

async fn list_mssql_schemas(
    c: &mut Client<Compat<TcpStream>>,
    conn: &Connection,
    db: &str,
) -> Result<Vec<TreeNode>> {
    use_database(c, db).await?;
    // User-visible schemas (hide db_* roles / sys / INFORMATION_SCHEMA).
    let sql = "SELECT name FROM sys.schemas \
         WHERE name NOT IN ('sys','INFORMATION_SCHEMA') \
           AND name NOT LIKE 'db\\_%' ESCAPE '\\' \
         ORDER BY CASE WHEN name = 'dbo' THEN 0 ELSE 1 END, name";
    let rows = query_cols(c, sql, 1).await?;
    let mut nodes: Vec<_> = rows
        .into_iter()
        .filter_map(|row| {
            let name = row.into_iter().next()?;
            if name.is_empty() {
                return None;
            }
            Some(schema_node(conn, db, &name))
        })
        .collect();
    // Always ensure dbo exists even on empty DBs.
    if !nodes.iter().any(|n| n.label == "dbo") {
        nodes.insert(0, schema_node(conn, db, "dbo"));
    }
    Ok(nodes)
}

async fn list_mssql_tables(
    c: &mut Client<Compat<TcpStream>>,
    conn: &Connection,
    meta: &NodeMeta,
    views: bool,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("master");
    let schema = meta.schema.as_deref().unwrap_or("dbo");
    use_database(c, db).await?;

    let schema_lit = sql_nvarchar_literal(schema);
    if views {
        let sql = format!(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.VIEWS \
             WHERE TABLE_SCHEMA = {schema_lit} ORDER BY 1"
        );
        let rows = query_cols(c, sql, 1).await.unwrap_or_default();
        return Ok(rows
            .into_iter()
            .filter_map(|row| {
                let name = row.into_iter().next()?;
                if name.is_empty() {
                    return None;
                }
                Some(table_node(conn, db, schema, &name, true))
            })
            .collect());
    }

    // Prefer sys.tables + row counts in current DB context (after USE).
    let sql_counts = format!(
        "SELECT t.name, CAST(SUM(p.[rows]) AS BIGINT) \
         FROM sys.tables t \
         INNER JOIN sys.schemas s ON t.schema_id = s.schema_id \
         INNER JOIN sys.partitions p ON t.object_id = p.object_id AND p.index_id IN (0,1) \
         WHERE s.name = {schema_lit} AND t.is_ms_shipped = 0 \
         GROUP BY t.name \
         ORDER BY t.name"
    );
    if let Ok(rows) = query_cols(c, &sql_counts, 2).await {
        let nodes: Vec<_> = rows
            .into_iter()
            .filter_map(|row| {
                let name = row.first()?.clone();
                if name.is_empty() {
                    return None;
                }
                let mut node = table_node(conn, db, schema, &name, false);
                if let Some(raw) = row.get(1) {
                    if let Ok(cnt) = raw.replace(',', "").trim().parse::<i64>() {
                        node.meta.meta_line = Some(super::format_row_count(cnt.max(0)));
                    }
                }
                Some(node)
            })
            .collect();
        if !nodes.is_empty() {
            return Ok(nodes);
        }
    }

    // Fallback: INFORMATION_SCHEMA (no counts).
    let sql = format!(
        "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES \
         WHERE TABLE_TYPE = N'BASE TABLE' AND TABLE_SCHEMA = {schema_lit} \
         ORDER BY 1"
    );
    let rows = query_cols(c, sql, 1)
        .await
        .with_context(|| format!("列出表失败 ({db}.{schema})"))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let name = row.into_iter().next()?;
            if name.is_empty() {
                return None;
            }
            Some(table_node(conn, db, schema, &name, false))
        })
        .collect())
}

async fn list_mssql_functions(
    c: &mut Client<Compat<TcpStream>>,
    conn: &Connection,
    meta: &NodeMeta,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("master");
    let schema = meta.schema.as_deref().unwrap_or("dbo");
    use_database(c, db).await?;
    let schema_lit = sql_nvarchar_literal(schema);
    let sql = format!(
        "SELECT o.name FROM sys.objects o \
         INNER JOIN sys.schemas s ON o.schema_id = s.schema_id \
         WHERE s.name = {schema_lit} \
           AND o.type IN ('FN','IF','TF','FS','FT','AF') \
           AND o.is_ms_shipped = 0 \
         ORDER BY 1"
    );
    let rows = query_cols(c, sql, 1).await.unwrap_or_default();
    Ok(rows
        .into_iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let name = row.into_iter().next()?;
            if name.is_empty() {
                return None;
            }
            Some(leaf_node(
                conn,
                &format!("fn:{db}:{schema}:{name}:{i}"),
                &name,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    schema: Some(schema.into()),
                    path: Some("nav:function".into()),
                    table: Some(name.clone()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_mssql_indexes(
    c: &mut Client<Compat<TcpStream>>,
    conn: &Connection,
    meta: &NodeMeta,
    table: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("master");
    let schema = meta.schema.as_deref().unwrap_or("dbo");
    use_database(c, db).await?;
    let schema_lit = sql_nvarchar_literal(schema);
    let sql = if let Some(t) = table {
        let t_lit = sql_nvarchar_literal(t);
        format!(
            "SELECT i.name FROM sys.indexes i \
             INNER JOIN sys.tables t ON i.object_id=t.object_id \
             INNER JOIN sys.schemas s ON t.schema_id=s.schema_id \
             WHERE s.name={schema_lit} AND t.name={t_lit} AND i.name IS NOT NULL ORDER BY 1"
        )
    } else {
        format!(
            "SELECT t.name, i.name FROM sys.indexes i \
             INNER JOIN sys.tables t ON i.object_id=t.object_id \
             INNER JOIN sys.schemas s ON t.schema_id=s.schema_id \
             WHERE s.name={schema_lit} AND i.name IS NOT NULL ORDER BY 1,2"
        )
    };
    let ncols = if table.is_some() { 1 } else { 2 };
    let rows = query_cols(c, sql, ncols).await.unwrap_or_default();
    Ok(rows
        .into_iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let label = if table.is_some() {
                row.first()?.clone()
            } else {
                format!("{}.{}", row.first()?, row.get(1)?)
            };
            Some(leaf_node(
                conn,
                &format!("idx:{db}:{schema}:{label}:{i}"),
                &label,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    schema: Some(schema.into()),
                    table: table.map(|t| t.into()),
                    path: Some("nav:index".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_mssql_triggers(
    c: &mut Client<Compat<TcpStream>>,
    conn: &Connection,
    meta: &NodeMeta,
    table: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("master");
    let schema = meta.schema.as_deref().unwrap_or("dbo");
    use_database(c, db).await?;
    let schema_lit = sql_nvarchar_literal(schema);
    let sql = if let Some(t) = table {
        let t_lit = sql_nvarchar_literal(t);
        format!(
            "SELECT tr.name FROM sys.triggers tr \
             INNER JOIN sys.tables t ON tr.parent_id=t.object_id \
             INNER JOIN sys.schemas s ON t.schema_id=s.schema_id \
             WHERE s.name={schema_lit} AND t.name={t_lit} ORDER BY 1"
        )
    } else {
        format!(
            "SELECT t.name, tr.name FROM sys.triggers tr \
             INNER JOIN sys.tables t ON tr.parent_id=t.object_id \
             INNER JOIN sys.schemas s ON t.schema_id=s.schema_id \
             WHERE s.name={schema_lit} ORDER BY 1,2"
        )
    };
    let ncols = if table.is_some() { 1 } else { 2 };
    let rows = query_cols(c, sql, ncols).await.unwrap_or_default();
    Ok(rows
        .into_iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let label = if table.is_some() {
                row.first()?.clone()
            } else {
                format!("{}.{}", row.first()?, row.get(1)?)
            };
            Some(leaf_node(
                conn,
                &format!("trg:{db}:{schema}:{label}:{i}"),
                &label,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    schema: Some(schema.into()),
                    table: table.map(|t| t.into()),
                    path: Some("nav:trigger".into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

async fn list_mssql_columns(
    c: &mut Client<Compat<TcpStream>>,
    conn: &Connection,
    meta: &NodeMeta,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("master");
    let schema = meta.schema.as_deref().unwrap_or("dbo");
    let table = meta.table.as_deref().unwrap_or("");
    use_database(c, db).await?;
    let sql = format!(
        "SELECT COLUMN_NAME, DATA_TYPE FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA={} AND TABLE_NAME={} ORDER BY ORDINAL_POSITION",
        sql_nvarchar_literal(schema),
        sql_nvarchar_literal(table)
    );
    let rows = query_cols(c, sql, 2).await?;
    Ok(rows
        .into_iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let name = row.first()?.clone();
            let ty = row.get(1).cloned().unwrap_or_default();
            let label = if ty.is_empty() {
                name.clone()
            } else {
                format!("{name}  {ty}")
            };
            let mut m = meta.clone();
            m.path = Some(name.clone());
            Some(leaf_node(
                conn,
                &format!("col:{db}:{schema}:{table}:{name}:{i}"),
                &label,
                NodeKind::Column,
                m,
            ))
        })
        .collect())
}

async fn list_mssql_constraints(
    c: &mut Client<Compat<TcpStream>>,
    conn: &Connection,
    meta: &NodeMeta,
    cat: &str,
) -> Result<Vec<TreeNode>> {
    let db = meta.database.as_deref().unwrap_or("master");
    let schema = meta.schema.as_deref().unwrap_or("dbo");
    let table = meta.table.as_deref().unwrap_or("");
    use_database(c, db).await?;
    let typ = match cat {
        "nav:fks" => "FOREIGN KEY",
        "nav:uniques" => "UNIQUE",
        _ => "CHECK",
    };
    let sql = format!(
        "SELECT CONSTRAINT_NAME FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS \
         WHERE TABLE_SCHEMA={} AND TABLE_NAME={} AND CONSTRAINT_TYPE={} \
         ORDER BY 1",
        sql_nvarchar_literal(schema),
        sql_nvarchar_literal(table),
        sql_nvarchar_literal(typ)
    );
    let rows = query_cols(c, sql, 1).await.unwrap_or_default();
    Ok(rows
        .into_iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let name = row.first()?.clone();
            Some(leaf_node(
                conn,
                &format!("cst:{db}:{schema}:{table}:{name}:{i}"),
                &name,
                NodeKind::Key,
                NodeMeta {
                    database: Some(db.into()),
                    schema: Some(schema.into()),
                    table: Some(table.into()),
                    path: Some(cat.into()),
                    ..Default::default()
                },
            ))
        })
        .collect())
}

pub async fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let sql = query.trim().trim_end_matches(';');
    if sql.is_empty() {
        bail!("SQL 为空");
    }
    let mut c = connect(conn).await?;
    let mut stream = c.simple_query(sql).await.context("执行 SQL 失败")?;
    let mut columns: Vec<String> = Vec::new();
    let mut column_types: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;

    while let Some(item) = futures::StreamExt::next(&mut stream).await {
        let item = item?;
        match item {
            QueryItem::Metadata(meta) => {
                columns = meta.columns().iter().map(|c| c.name().to_string()).collect();
                column_types = meta
                    .columns()
                    .iter()
                    .map(|c| mssql_type_label(c.column_type()))
                    .collect();
            }
            QueryItem::Row(row) => {
                if columns.is_empty() {
                    columns = (0..row.columns().len())
                        .map(|i| format!("col{i}"))
                        .collect();
                }
                let mut vals = Vec::with_capacity(columns.len());
                for i in 0..columns.len() {
                    vals.push(mssql_cell(&row, i));
                }
                rows.push(vals);
                if rows.len() >= MAX_RESULT_ROWS {
                    truncated = true;
                    break;
                }
            }
        }
    }

    Ok(QueryResult {
        message: if rows.is_empty() {
            "OK".into()
        } else if truncated {
            format!("{}+ rows (已截断)", rows.len())
        } else {
            format!("{} rows", rows.len())
        },
        columns,
        column_types,
        rows,
        truncated,
        affected: None,
        ..Default::default()
    })
}

fn mssql_type_label(ct: ColumnType) -> String {
    // Human-friendly labels for common TDS types; fall back to Debug name.
    match ct {
        ColumnType::Bit => "BIT".into(),
        ColumnType::Int1 => "TINYINT".into(),
        ColumnType::Int2 => "SMALLINT".into(),
        ColumnType::Int4 | ColumnType::Intn => "INT".into(),
        ColumnType::Int8 => "BIGINT".into(),
        ColumnType::Float4 => "REAL".into(),
        ColumnType::Float8 | ColumnType::Floatn => "FLOAT".into(),
        ColumnType::BigVarChar | ColumnType::BigChar | ColumnType::Text => "VARCHAR".into(),
        ColumnType::NVarchar | ColumnType::NChar | ColumnType::NText => "NVARCHAR".into(),
        ColumnType::Decimaln | ColumnType::Numericn => "DECIMAL".into(),
        other => format!("{other:?}").to_ascii_uppercase(),
    }
}

pub async fn preview(conn: &Connection, node: &TreeNode) -> Result<QueryResult> {
    match node.kind {
        NodeKind::Table => {
            let db = node.meta.database.as_deref().unwrap_or("master");
            let schema = node.meta.schema.as_deref().unwrap_or("dbo");
            let table = node.meta.table.as_deref().unwrap_or(&node.label);
            let limit = DEFAULT_PAGE_SIZE;
            let sql = format!(
                "SELECT TOP {limit} * FROM {}.{}.{}",
                bracket_ident(db),
                bracket_ident(schema),
                bracket_ident(table)
            );
            run_query(conn, &sql).await
        }
        _ => Ok(QueryResult {
            message: "请选择表以预览数据".into(),
            ..Default::default()
        }),
    }
}

fn mssql_cell(row: &Row, i: usize) -> String {
    if let Some(v) = row.get::<&str, _>(i) {
        return truncate_cell(v, 500);
    }
    if let Some(v) = row.get::<i16, _>(i) {
        return v.to_string();
    }
    if let Some(v) = row.get::<i32, _>(i) {
        return v.to_string();
    }
    if let Some(v) = row.get::<i64, _>(i) {
        return v.to_string();
    }
    if let Some(v) = row.get::<u8, _>(i) {
        return v.to_string();
    }
    if let Some(v) = row.get::<f32, _>(i) {
        return v.to_string();
    }
    if let Some(v) = row.get::<f64, _>(i) {
        return v.to_string();
    }
    if let Some(v) = row.get::<bool, _>(i) {
        return v.to_string();
    }
    "".into()
}
