//! UI 文案（中文集中管理，避免散落在业务代码里被编辑器/终端弄乱编码）。
#![allow(dead_code)]

pub const READY: &str = "就绪";
pub const WELCOME: &str = "欢迎使用 DbLens 万能查看器";
pub const SAVED_FMT: &str = "已保存 {}";
pub const SAVE_FAIL_FMT: &str = "保存失败: {e:#}";
pub const LIST_UPDATED: &str = "对象列表已更新";
pub const PICK_CONN: &str = "请先选择连接";
pub const QUERY_TITLE_OBJ_FMT: &str = "查询 / {} / {}";
pub const QUERY_PREFIX_FMT: &str = "查询 / {}";
pub const PICK_TAB: &str = "请先打开查询标签";
pub const VIEWER_SUBTITLE: &str = "万能查看器";
pub const NEW_CONN: &str = "新建连接";
pub const NEW_QUERY: &str = "新建查询";
pub const RUN: &str = "运行";
pub const REFRESH: &str = "刷新";
pub const SAVE_CFG: &str = "保存配置";
pub const T_B1FBE3: &str = "打开配置";
/// Short captions for Navicat-style toolbar icons.
pub const TB_CONN: &str = "连接";
pub const TB_NEW_QUERY: &str = "新建查询";
pub const TB_RUN: &str = "运行";
pub const TB_REFRESH: &str = "刷新";
pub const TB_SAVE: &str = "保存";
pub const TB_OPEN: &str = "打开";
pub const LOADED_FMT: &str = "已加载 {}";
pub const BUSY: &str = "忙碌";
pub const MESSAGES: &str = "消息";
pub const CLEAR: &str = "清空";
pub const T_970D1F: &str = "配置: {}";
pub const CONNECTION: &str = "连接";
pub const T_54D40F: &str = "筛选连接";
pub const NO_CONN: &str = "暂无连接";
pub const T_CONFIG_EXAMPLE_TOML_1CE9F0: &str = "点击「新建连接」添加，或参考 config.example.toml";
pub const T_AF3981: &str = "▼";
pub const T_DFF521: &str = "▶";
pub const CONNECT_REFRESH: &str = "连接 / 刷新";
pub const USE_IN_QUERY: &str = "在查询中使用";
pub const EDIT: &str = "编辑";
pub const DELETE: &str = "删除";
pub const OPEN: &str = "打开";
pub const CLOSE: &str = "关闭";
pub const CLOSE_OTHERS: &str = "关闭其他";
pub const T_90DD80: &str = "左侧选择连接，双击表/集合/键打开；或点「新建查询」";
pub const RUN_BTN: &str = "▶ 运行";
pub const RESULT: &str = "结果";
pub const ROWS_COLS_FMT: &str = "{row_n} 行 x {col_n} 列";
pub const T_TOTAL_2D009D: &str = "共 {total}";
pub const TRUNCATED: &str = "已截断";
pub const TRUNCATED_LONG: &str = "
…（内容过长已截断）";
pub const T_82B416: &str = "暂无表格数据";
pub const T_F82E51: &str = "明细";
pub const ROW_N_FMT: &str = "第 {} 行";
pub const T_550D79: &str = "点击表格行查看明细";
pub const T_0EEF42: &str = "（无数据）";
pub const T_14525A: &str = "在上方表格中点击一行查看字段明细";
pub const OBJECT: &str = "(对象)";
pub const T_C2FE62: &str = "筛选";
pub const T_A6F7F1: &str = "条数";
pub const T_E5F71F: &str = "搜索";
pub const T_T_8BFB06: &str = " / 共 {t}";
pub const T_PAGE_ROW_N_TOTAL_TXT_628B18: &str = "第 {page} 页  {row_n} 行{total_txt}";
pub const PREV_PAGE: &str = "上一页";
pub const NEXT_PAGE: &str = "下一页";
pub const T_99D4BF: &str = "暂无数据，点击「刷新」或「搜索」加载";
pub const T_F5416D: &str = "\n… 另有 {} 行未显示（已截断）";
pub const T_0A0A01: &str = "\n…内容过长已截断";
pub const EDIT_CONN: &str = "编辑连接";
pub const T_A22F6E: &str = "数据库类型 / 组件类型";
pub const ADVANCED: &str = "高级选项";
pub const T_URL_96AD22: &str = "可选：若填写完整 URL，将优先于主机/端口";
pub const T_TLS_686425: &str = "允许不安全 TLS / 跳过证书校验";
pub const TEST_CONN: &str = "测试连接";
pub const CANCEL: &str = "取消";
pub const NEXT_STEP: &str = "下一步";
pub const SAVE: &str = "保存";
pub const T_769D88: &str = "完成";
pub const BACK_STEP: &str = "上一步";
pub const CONN_NAME: &str = "连接名";
pub const EG_NAME: &str = "例如 prod-mysql";
pub const DB_FILE: &str = "数据库文件";
pub const T_DB_SQLITE_693A61: &str = "例如 .db / .sqlite 文件";
pub const BROWSE: &str = "浏览…";
pub const ALL_FILES: &str = "所有文件";
pub const NEW_FILE: &str = "新建…";
pub const HOST: &str = "主机";
pub const PORT: &str = "端口";
pub const USERNAME: &str = "用户名";
pub const PASSWORD: &str = "密码";
pub const DB_INDEX: &str = "DB 索引";
pub const DEFAULT_DB: &str = "默认库";
pub const DATABASE: &str = "数据库";
pub const INIT_DATABASE: &str = "初始数据库";
pub const AUTH_MODE: &str = "验证";
pub const MSSQL_HOST_HINT: &str = r"主机 或 主机\实例名";
pub const MSSQL_WINDOWS_HINT: &str = "使用当前 Windows 登录身份连接（无需填写用户名/密码）";
pub const SNOWFLAKE_ACCOUNT_HINT: &str = "xy12345.us-east-1";
pub const SERVICE_NAME: &str = "服务名 / SID";
pub const WAREHOUSE: &str = "Warehouse";
pub const ROLE: &str = "Role";
pub const STEP_TYPE: &str = "1 选择类型";
pub const T_2_34D252: &str = "2 常规设置";
pub const T_53158F: &str = "请填写连接名";
pub const NEED_SQLITE: &str = "请选择 SQLite 文件路径";
pub const NEED_HOST: &str = "请填写主机地址";

use std::fmt::Display;

pub fn saved(path: impl Display) -> String {
    format!("已保存 {path}")
}

pub fn save_fail(e: impl Display) -> String {
    format!("保存失败: {e:#}")
}

pub fn loaded(path: impl Display) -> String {
    format!("已加载 {path}")
}

pub fn config_path_line(path: impl Display) -> String {
    format!("配置: {path}")
}

pub fn query_title(conn: impl Display) -> String {
    format!("查询 / {conn}")
}

pub fn query_title_obj(conn: impl Display, obj: impl Display) -> String {
    format!("查询 / {conn} / {obj}")
}

pub fn rows_cols(row_n: impl Display, col_n: impl Display) -> String {
    format!("{row_n} 行 x {col_n} 列")
}

pub fn total(total: impl Display) -> String {
    format!("总计 {total}")
}

pub fn row_n(n: impl Display) -> String {
    format!("第 {n} 行")
}

pub fn page_info(page: impl Display, row_n: impl Display, total_txt: impl Display) -> String {
    format!("第 {page} 页 · {row_n} 行{total_txt}")
}

pub fn more_rows_hidden(n: impl Display) -> String {
    format!("\n… 还有 {n} 行未显示（仅展示前部分）")
}

pub fn total_txt_suffix(t: impl Display) -> String {
    format!(" / 总 {t}")
}

