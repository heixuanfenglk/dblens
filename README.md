# DbLens

万能多主机数据查看器 —— 在一个桌面应用里浏览、查询多种数据库与中间件。

基于 Rust + egui，当前主要面向 Windows。

## 功能概览

- 多连接管理：新建 / 编辑 / 测试连接
- 导航树：库 → 架构 / 分类 → 表、视图、索引等
- SQL / 查询结果表格（斑马纹、可选中行、列可调整）
- 可视化面板：数据 / 结构 / 信息（按后端适配）
- Elasticsearch 集群视图：健康、节点、分片等图形化仪表盘
- 配置持久化：本地 TOML

## 支持的后端

| 类型 | `kind` | 说明 |
|------|--------|------|
| MySQL | `mysql` | sqlx |
| MariaDB | `mariadb` | 兼容 MySQL 协议 |
| PostgreSQL | `postgres` | sqlx |
| SQL Server | `mssql` | tiberius；支持 Windows 验证、命名实例 |
| Oracle | `oracle` | 纯 Rust 驱动，无需 Instant Client |
| SQLite | `sqlite` | 本地文件 |
| Snowflake | `snowflake` | Account + Warehouse / Role |
| ClickHouse | `clickhouse` | HTTP（默认 8123） |
| MongoDB | `mongodb` | |
| Redis | `redis` | |
| Memcached | `memcached` | |
| Elasticsearch | `elasticsearch` | |
| etcd | `etcd` | |

## 环境要求

- [Rust](https://rustup.rs/)（edition 2021）
- Windows：MSVC 工具链（打包图标依赖 `winres`）
- 可选：[Inno Setup 6](https://jrsoftware.org/isinfo.php)（生成安装包）

## 构建与运行

```powershell
# 开发
cargo run

# 发布
cargo run --release
```

产物路径：`target/release/dblens.exe`

## 一键打包

```powershell
.\pack.ps1
# 或跳过编译，仅打安装包：
.\pack.ps1 -SkipBuild
```

输出目录：`dist/`

- 便携版：`dist/dblens.exe`
- 安装包：`dist/DbLens-0.1.0-Setup.exe`

## 配置

连接可在应用内通过「新建连接」向导添加，也可手写 TOML。

查找顺序大致为：

1. 当前目录 `dblens.toml`（兼容旧名 `allink.toml`）
2. `%APPDATA%\dblens\config.toml`（兼容旧目录 `allink`）

示例见 [`config.example.toml`](config.example.toml)：

```toml
[[connections]]
name = "local-mysql"
kind = "mysql"
host = "127.0.0.1"
port = 3306
username = "root"
password = "secret"
database = "demo"
```

Snowflake 额外字段：`warehouse`、`role`；主机填 Account（如 `xy12345.us-east-1`）。  
Oracle 的 `database` 表示服务名 / SID。

## 项目结构

```
src/
  app.rs          # 主界面
  backends/       # 各数据源实现
  visual.rs       # 可视化查询拼装
  es_dash.rs      # ES 集群仪表盘
  config.rs       # 连接与配置
assets/           # 图标
packaging/        # Inno Setup 脚本
pack.ps1          # 一键打包
```

## 许可证

按需自行补充。
