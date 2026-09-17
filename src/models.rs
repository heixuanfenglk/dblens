use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Connection,
    Database,
    Schema,
    Table,
    Collection,
    Index,
    Key,
    Folder,
    Column,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    /// 连接 id
    pub connection_id: String,
    /// 父节点路径信息（库名/表名等）
    #[serde(default)]
    pub meta: NodeMeta,
    #[serde(default)]
    pub children: Vec<TreeNode>,
    #[serde(default)]
    pub expandable: bool,
    #[serde(default)]
    pub loaded: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeMeta {
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub table: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    /// Status chip text, e.g. ES index health: green/yellow/red.
    #[serde(default)]
    pub status: Option<String>,
    /// Secondary muted line, e.g. "51.5M · 2tb".
    #[serde(default)]
    pub meta_line: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct QueryResult {
    pub columns: Vec<String>,
    /// Parallel to `columns` when known (SQL type names: INTEGER, TEXT, …).
    pub column_types: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub message: String,
    pub elapsed_ms: u64,
    pub affected: Option<u64>,
    /// Optional total hit count (e.g. ES track_total_hits).
    pub total: Option<u64>,
    /// True when rows were truncated to keep UI responsive.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ValueView {
    pub title: String,
    pub content: String,
    pub meta: String,
}

impl TreeNode {
    pub fn connection_root(conn_id: &str, label: &str) -> Self {
        Self {
            id: format!("conn:{conn_id}"),
            label: label.to_string(),
            kind: NodeKind::Connection,
            connection_id: conn_id.to_string(),
            meta: NodeMeta::default(),
            children: Vec::new(),
            expandable: true,
            loaded: false,
        }
    }
}
