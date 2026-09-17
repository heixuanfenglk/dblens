use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Redis,
    Memcached,
    Elasticsearch,
    Etcd,
    Mysql,
    Mariadb,
    Mssql,
    Postgres,
    Sqlite,
    Oracle,
    Snowflake,
    Clickhouse,
    Mongodb,
}

#[allow(dead_code)]
impl BackendKind {
    pub const ALL: &'static [BackendKind] = &[
        Self::Mysql,
        Self::Mariadb,
        Self::Postgres,
        Self::Mssql,
        Self::Oracle,
        Self::Sqlite,
        Self::Snowflake,
        Self::Clickhouse,
        Self::Mongodb,
        Self::Redis,
        Self::Memcached,
        Self::Elasticsearch,
        Self::Etcd,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Redis => "redis",
            Self::Memcached => "memcached",
            Self::Elasticsearch => "elasticsearch",
            Self::Etcd => "etcd",
            Self::Mysql => "mysql",
            Self::Mariadb => "mariadb",
            Self::Mssql => "mssql",
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
            Self::Oracle => "oracle",
            Self::Snowflake => "snowflake",
            Self::Clickhouse => "clickhouse",
            Self::Mongodb => "mongodb",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Redis => "Redis",
            Self::Memcached => "Memcached",
            Self::Elasticsearch => "Elasticsearch",
            Self::Etcd => "etcd",
            Self::Mysql => "MySQL",
            Self::Mariadb => "MariaDB",
            Self::Mssql => "SQL Server",
            Self::Postgres => "PostgreSQL",
            Self::Sqlite => "SQLite",
            Self::Oracle => "Oracle",
            Self::Snowflake => "Snowflake",
            Self::Clickhouse => "ClickHouse",
            Self::Mongodb => "MongoDB",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Redis => 6379,
            Self::Memcached => 11211,
            Self::Elasticsearch => 9200,
            Self::Etcd => 2379,
            Self::Mysql | Self::Mariadb => 3306,
            Self::Mssql => 1433,
            Self::Postgres => 5432,
            Self::Sqlite => 0,
            Self::Oracle => 1521,
            Self::Snowflake => 443,
            Self::Clickhouse => 8123,
            Self::Mongodb => 27017,
        }
    }

    pub fn is_sql(self) -> bool {
        matches!(
            self,
            Self::Mysql
                | Self::Mariadb
                | Self::Mssql
                | Self::Postgres
                | Self::Sqlite
                | Self::Oracle
                | Self::Snowflake
                | Self::Clickhouse
        )
    }

    /// MySQL wire protocol family (sqlx mysql).
    pub fn is_mysql_family(self) -> bool {
        matches!(self, Self::Mysql | Self::Mariadb)
    }

    pub fn is_kv(self) -> bool {
        matches!(self, Self::Redis | Self::Memcached | Self::Etcd)
    }

    pub fn query_placeholder(self) -> &'static str {
        match self {
            Self::Mysql
            | Self::Mariadb
            | Self::Mssql
            | Self::Postgres
            | Self::Sqlite
            | Self::Clickhouse => "SELECT * FROM table_name LIMIT 100;",
            Self::Oracle => "SELECT * FROM dual WHERE ROWNUM <= 100;",
            Self::Snowflake => "SELECT * FROM table_name LIMIT 100;",
            Self::Redis => "SCAN 0 MATCH * COUNT 100\n# 或: GET key / KEYS pattern / INFO",
            Self::Memcached => "GET key_name\n# 或: STATS",
            Self::Elasticsearch => {
                "GET /_cat/indices?v\n# 或 JSON 搜索:\n# {\"query\":{\"match_all\":{}},\"size\":50}\n# 或 Lucene: q=field:value"
            }
            Self::Etcd => "/\n# 前缀查询，例如 /config/",
            Self::Mongodb => "{\n  \"find\": \"collection\",\n  \"filter\": {}\n}",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "redis" => Some(Self::Redis),
            "memcached" | "memcache" | "mc" => Some(Self::Memcached),
            "elasticsearch" | "elastic" | "es" => Some(Self::Elasticsearch),
            "etcd" => Some(Self::Etcd),
            "mysql" => Some(Self::Mysql),
            "mariadb" | "maria" => Some(Self::Mariadb),
            "mssql" | "sqlserver" | "sql_server" => Some(Self::Mssql),
            "postgres" | "postgresql" | "pg" => Some(Self::Postgres),
            "sqlite" | "sqlite3" => Some(Self::Sqlite),
            "oracle" | "oracledb" => Some(Self::Oracle),
            "snowflake" | "sf" => Some(Self::Snowflake),
            "clickhouse" | "ch" => Some(Self::Clickhouse),
            "mongodb" | "mongo" => Some(Self::Mongodb),
            _ => None,
        }
    }
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}
