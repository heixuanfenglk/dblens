use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Pos2, Rect, RichText, Sense,
    Stroke, Ui, Vec2,
};
use egui_extras::{Column, TableBuilder};
use crate::app_state::AppState;
use crate::config::{Config, Connection, MssqlAuthMode};
use crate::kind::BackendKind;
use crate::models::{NodeKind, NodeMeta, QueryResult, TreeNode};
use crate::visual::{self, ViewMode, VisualPanel};
use crate::worker::{Event, Job, Worker};
// Navicat-style light professional theme
const ACCENT: Color32 = Color32::from_rgb(45, 120, 200);
const ACCENT_DARK: Color32 = Color32::from_rgb(30, 90, 160);
const TEXT: Color32 = Color32::from_rgb(32, 36, 42);
const MUTED: Color32 = Color32::from_rgb(110, 118, 128);
const BG_TOOLBAR: Color32 = Color32::from_rgb(245, 247, 250);
const BG_NAV: Color32 = Color32::from_rgb(252, 252, 253);
const BG_WORK: Color32 = Color32::from_rgb(255, 255, 255);
const BG_MSG: Color32 = Color32::from_rgb(248, 249, 251);
const BORDER: Color32 = Color32::from_rgb(210, 216, 224);
const SEL: Color32 = Color32::from_rgb(220, 236, 252);
const OK_GREEN: Color32 = Color32::from_rgb(46, 140, 90);
const ERR_RED: Color32 = Color32::from_rgb(200, 70, 60);
const TABLE_HEADER_BG: Color32 = Color32::from_rgb(245, 247, 249);
const TABLE_HEADER_FG: Color32 = Color32::from_rgb(32, 36, 42);
const TABLE_TYPE_FG: Color32 = Color32::from_rgb(128, 128, 128);
/// Soft horizontal hairline — rows read as bands.
const TABLE_HLINE: Color32 = Color32::from_rgb(228, 232, 238);
/// Near-invisible column guides (row stays one unit).
const TABLE_VLINE: Color32 = Color32::from_rgb(238, 241, 245);
const TABLE_ROW_ALT: Color32 = Color32::from_rgb(248, 249, 251);
const TABLE_SEL_BG: Color32 = Color32::from_rgb(0, 120, 215);
const TABLE_SEL_GUTTER: Color32 = Color32::from_rgb(0, 90, 180);
const TABLE_OUTER: Color32 = Color32::from_rgb(198, 208, 220);
const TABLE_GUTTER_BG: Color32 = Color32::from_rgb(245, 247, 249);

#[derive(Clone)]

struct WorkspaceTab {
    id: u64,
    title: String,
    connection_id: String,
    context: NodeMeta,
    query: String,
    result: QueryResult,
    editor_ratio: f32,
    view_mode: ViewMode,
    visual_panel: VisualPanel,
    filter: String,
    page_size: u32,
    page_from: u32,
    selected_row: Option<usize>,
    object_name: Option<String>,
    kind: BackendKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WizardStep {
    ChooseType,
    General,
}

#[allow(dead_code)]

enum _WizardStepDocs {
    ChooseType,
    /// 连接名 / 主机 / 认证
    General,
    /// 可选 URL / TLS
    Advanced,
    /// 确认完成
    Finish,
}

struct ConnForm {
    open: bool,
    editing_id: Option<String>,
    draft: Connection,
    error: Option<String>,
    step: WizardStep,
    test_ok: Option<String>,
    /// Credentials used by the in-flight / last Test click (may be unsaved).
    pending_test: Option<Connection>,
}

impl Default for ConnForm {
    fn default() -> Self {
        Self {
            open: false,
            editing_id: None,
            draft: Connection::with_kind(BackendKind::Mysql),
            error: None,
            step: WizardStep::ChooseType,
            test_ok: None,
            pending_test: None,
        }
    }
}

impl ConnForm {
    fn new_wizard() -> Self {
        Self {
            open: true,
            editing_id: None,
            draft: Connection::with_kind(BackendKind::Mysql),
            error: None,
            step: WizardStep::ChooseType,
            test_ok: None,
            pending_test: None,
        }
    }

    fn edit(conn: Connection) -> Self {
        Self {
            open: true,
            editing_id: Some(conn.id.clone()),
            draft: conn,
            error: None,
            step: WizardStep::General,
            test_ok: None,
            pending_test: None,
        }
    }
}

enum WizardAction {
    None,
    Cancel,
    Back,
    Next,
    Test,
    Save,
}

fn wizard_step_ord(step: WizardStep, editing: bool) -> u8 {
    match step {
        WizardStep::ChooseType => 0,
        WizardStep::General => {
            if editing {
                0
            } else {
                1
            }
        }
    }
}

pub struct DblensApp {
    config: Config,
    config_path: PathBuf,
    app_state: AppState,
    worker: Worker,
    events: Receiver<Event>,
    busy: bool,
    status: String,
    messages: Vec<(bool, String)>,
    trees: HashMap<String, TreeNode>,
    expanded: HashMap<String, bool>,
    selected_node: Option<TreeNode>,
    selected_conn_id: Option<String>,
    tabs: Vec<WorkspaceTab>,
    active_tab: Option<u64>,
    next_tab_id: u64,
    conn_form: ConnForm,
    nav_filter: String,
    connected: HashMap<String, bool>,
}

impl DblensApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_cjk_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);
        let app_state = AppState::load();
        let (config, config_path) = Config::load(None).unwrap_or_else(|e| {
            eprintln!("load config failed: {e:#}");
            (Config::default(), Config::discover_path())
        });
        let (worker, events) = Worker::spawn();
        let mut trees = HashMap::new();
        for c in &config.connections {
            trees.insert(c.id.clone(), TreeNode::connection_root(&c.id, &c.name));
        }
        let mut app = Self {
            config,
            config_path,
            app_state,
            worker,
            events,
            busy: false,
            status: crate::ui_text::READY.into(),
            messages: vec![(true, crate::ui_text::WELCOME.into())],
            trees,
            expanded: HashMap::new(),
            selected_node: None,
            selected_conn_id: None,
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 1,
            conn_form: ConnForm::default(),
            nav_filter: String::new(),
            connected: HashMap::new(),
        };
        app.app_state.remember_config(&app.config_path);
        if let Some(def) = app.config.default.clone() {
            if let Some(c) = app.config.find(&def) {
                app.selected_conn_id = Some(c.id.clone());
            }
        }
        app
    }

    fn push_msg(&mut self, ok: bool, msg: impl Into<String>) {
        let m = msg.into();
        self.status = m.clone();
        self.messages.push((ok, m));
        if self.messages.len() > 200 {
            self.messages.drain(0..self.messages.len() - 200);
        }
    }

    fn save_config(&mut self) {
        match self.config.save(&self.config_path) {
            Ok(()) => self.push_msg(true, crate::ui_text::saved(self.config_path.display())),
            Err(e) => self.push_msg(false, crate::ui_text::save_fail(e)),
        }
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Busy(b) => {
                    self.busy = b;
                    ctx.request_repaint();
                }
                Event::Progress(s) => {
                    self.status = s;
                    ctx.request_repaint();
                }
                Event::Error(e) => {
                    if self.conn_form.open {
                        self.conn_form.error = Some(e.clone());
                        self.conn_form.test_ok = None;
                    }
                    self.push_msg(false, e);
                    ctx.request_repaint();
                }
                Event::TestOk {
                    connection_id,
                    message,
                } => {
                    self.connected.insert(connection_id.clone(), true);
                    if self.conn_form.open {
                        self.conn_form.test_ok = Some(message.clone());
                        self.conn_form.error = None;
                    }
                    self.push_msg(true, message);
                    // While the form is open, always expand with the credentials just tested
                    // (draft / pending_test), not the possibly stale saved config.
                    let conn_for_expand = if self.conn_form.open {
                        self.conn_form
                            .pending_test
                            .as_ref()
                            .filter(|c| c.id == connection_id)
                            .cloned()
                            .or_else(|| {
                                if self.conn_form.draft.id == connection_id {
                                    let mut d = self.conn_form.draft.clone();
                                    d.ensure_defaults();
                                    Some(d)
                                } else {
                                    None
                                }
                            })
                    } else {
                        self.config.find(&connection_id).cloned()
                    };
                    if let Some(conn) = conn_for_expand {
                        let root = TreeNode::connection_root(&conn.id, &conn.name);
                        self.expanded.insert(root.id.clone(), true);
                        self.worker.submit(Job::ExpandNode {
                            conn,
                            node: root.clone(),
                        });
                        self.trees.insert(connection_id, root);
                    }
                }
                Event::Children {
                    connection_id,
                    parent_id,
                    children,
                } => {
                    if let Some(root) = self.trees.get_mut(&connection_id) {
                        attach_children(root, &parent_id, children);
                    }
                    self.push_msg(true, crate::ui_text::LIST_UPDATED);
                    ctx.request_repaint();
                }
                Event::QueryDone { tab_id, result } => {
                    // Clone title before mutably borrowing tab (avoid borrow conflict)
                    let title = self
                        .tabs
                        .iter()
                        .find(|t| t.id == tab_id)
                        .map(|t| t.title.clone());
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        let msg = result.message.clone();
                        tab.result = result;
                        if let Some(title) = title {
                            self.push_msg(true, format!("[{title}] {msg}"));
                        }
                    }
                }
            }
        }
    }

    fn open_query_tab_for_selection(&mut self) {
        let Some(conn_id) = self.selected_conn_id.clone() else {
            self.push_msg(false, crate::ui_text::PICK_CONN);
            return;
        };
        let Some(conn) = self.config.find(&conn_id).cloned() else {
            self.push_msg(false, crate::ui_text::PICK_CONN);
            return;
        };
        let (context, query, title) = if let Some(node) = self.selected_node.clone() {
            let q = default_query_for_node(&conn, &node);
            let title = crate::ui_text::query_title_obj(conn.name, node.label);
            (node.meta.clone(), q, title)
        } else {
            (
                NodeMeta::default(),
                conn.kind.query_placeholder().to_string(),
                crate::ui_text::query_title(conn.name),
            )
        };
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let kind = conn.kind;
        let object_name = visual::primary_object(kind, &context);
        // New Query always opens a dedicated Query editor tab.
        self.tabs.push(WorkspaceTab {
            id,
            title,
            connection_id: conn.id.clone(),
            context,
            query,
            result: QueryResult::default(),
            editor_ratio: 0.35,
            view_mode: ViewMode::Query,
            visual_panel: VisualPanel::Data,
            filter: String::new(),
            page_size: visual::DEFAULT_PAGE_SIZE,
            page_from: 0,
            selected_row: None,
            object_name,
            kind,
        });
        self.active_tab = Some(id);
    }

    fn open_object_tab(&mut self, node: &TreeNode) {
        let Some(conn) = self.config.find(&node.connection_id).cloned() else {
            self.push_msg(false, crate::ui_text::PICK_CONN);
            return;
        };
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let kind = conn.kind;
        let query = default_query_for_node(&conn, node);
        let title = visual::object_title(&conn.name, kind, &node.meta, &node.label);
        let object_name = visual::primary_object(kind, &node.meta).or_else(|| {
            // Cluster admin items: show Chinese label in the visual header.
            if kind == BackendKind::Elasticsearch
                && node.meta.path.as_deref() == Some("__cluster__")
            {
                Some(visual::display_object_label(kind, &node.meta, &node.label))
            } else {
                None
            }
        });
        let visual_panel = if kind == BackendKind::Elasticsearch {
            VisualPanel::from_es_schema(node.meta.schema.as_deref())
        } else {
            VisualPanel::Data
        };
        let use_visual = visual::prefer_visual(kind, &node.meta)
            || matches!(
                node.kind,
                NodeKind::Table | NodeKind::Collection | NodeKind::Index | NodeKind::Key
            );
        self.tabs.push(WorkspaceTab {
            id,
            title: if use_visual {
                title
            } else {
                crate::ui_text::query_title(title)
            },
            connection_id: conn.id.clone(),
            context: node.meta.clone(),
            query,
            result: QueryResult::default(),
            editor_ratio: if use_visual { 0.0 } else { 0.35 },
            view_mode: if use_visual {
                ViewMode::Visual
            } else {
                ViewMode::Query
            },
            visual_panel,
            filter: String::new(),
            page_size: visual::DEFAULT_PAGE_SIZE,
            page_from: 0,
            selected_row: None,
            object_name,
            kind,
        });
        self.active_tab = Some(id);
        self.selected_conn_id = Some(conn.id.clone());
        if use_visual {
            self.run_visual(id);
        } else {
            self.worker.submit(Job::Preview {
                conn,
                node: node.clone(),
                tab_id: id,
            });
        }
    }

    fn run_active_query(&mut self) {
        let Some(tab_id) = self.active_tab else {
            self.push_msg(false, crate::ui_text::PICK_TAB);
            return;
        };
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let Some(conn) = self.config.find(&tab.connection_id).cloned() else {
            self.push_msg(false, crate::ui_text::PICK_CONN);
            return;
        };
        let query = tab.query.clone();
        let context = tab.context.clone();
        self.worker.submit(Job::RunQuery {
            conn,
            query,
            context,
            tab_id,
        });
    }

    fn run_visual(&mut self, tab_id: u64) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id).cloned() else {
            return;
        };
        let Some(conn) = self.config.find(&tab.connection_id).cloned() else {
            self.push_msg(false, crate::ui_text::PICK_CONN);
            return;
        };
        // KV single-key: use Preview for typed value rendering
        let kv_key = matches!(
            tab.kind,
            BackendKind::Redis | BackendKind::Memcached | BackendKind::Etcd
        ) && tab.visual_panel == VisualPanel::Data
            && tab.context.path.as_ref().map(|p| !p.is_empty()).unwrap_or(false)
            && tab.object_name.is_some();
        if kv_key {
            let node = TreeNode {
                id: format!("visual-key-{}", tab_id),
                label: tab.object_name.clone().unwrap_or_default(),
                kind: NodeKind::Key,
                connection_id: conn.id.clone(),
                meta: tab.context.clone(),
                children: Vec::new(),
                expandable: false,
                loaded: true,
            };
            self.worker.submit(Job::Preview {
                conn,
                node,
                tab_id,
            });
            return;
        }
        let mut context = tab.context.clone();
        // Don't overwrite special ES cluster paths with the display label.
        if let Some(name) = &tab.object_name {
            let special = context
                .path
                .as_deref()
                .is_some_and(|p| p.starts_with("__"));
            if !special {
                context.path = Some(name.clone());
                if context.table.is_none() {
                    context.table = Some(name.clone());
                }
            }
        }
        // For ES cluster views, keep schema action and force Info panel query path.
        let query = visual::build_query(
            tab.kind,
            tab.visual_panel,
            &context,
            &tab.filter,
            tab.page_size,
            tab.page_from,
        );
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            t.query = query.clone();
            t.selected_row = None;
            t.context = context.clone();
        }
        self.worker.submit(Job::RunQuery {
            conn,
            query,
            context,
            tab_id,
        });
    }
}

impl eframe::App for DblensApp {

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events(ctx);
        egui::TopBottomPanel::top("toolbar")
            .exact_height(72.0)
            .frame(Frame::new().fill(BG_TOOLBAR).stroke(Stroke::new(1.0, BORDER)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);

                    if crate::icons::toolbar_icon_btn(
                        ui,
                        crate::icons::ToolbarIcon::NewConnection,
                        crate::ui_text::TB_CONN,
                        ACCENT,
                    )
                    .on_hover_text(crate::ui_text::NEW_CONN)
                    .clicked()
                    {
                        self.conn_form = ConnForm::new_wizard();
                    }
                    if crate::icons::toolbar_icon_btn(
                        ui,
                        crate::icons::ToolbarIcon::OpenQuery,
                        crate::ui_text::TB_NEW_QUERY,
                        ACCENT_DARK,
                    )
                    .on_hover_text(crate::ui_text::NEW_QUERY)
                    .clicked()
                    {
                        self.open_query_tab_for_selection();
                    }
                    if crate::icons::toolbar_icon_btn(
                        ui,
                        crate::icons::ToolbarIcon::Run,
                        crate::ui_text::TB_RUN,
                        OK_GREEN,
                    )
                    .on_hover_text(crate::ui_text::RUN)
                    .clicked()
                    {
                        self.run_active_query();
                    }

                    crate::icons::toolbar_separator(ui);

                    if crate::icons::toolbar_icon_btn(
                        ui,
                        crate::icons::ToolbarIcon::Refresh,
                        crate::ui_text::TB_REFRESH,
                        ACCENT,
                    )
                    .on_hover_text(crate::ui_text::REFRESH)
                    .clicked()
                    {
                        if let Some(id) = self.selected_conn_id.clone() {
                            if let Some(conn) = self.config.find(&id).cloned() {
                                let root = TreeNode::connection_root(&conn.id, &conn.name);
                                self.trees.insert(id.clone(), root.clone());
                                self.worker.submit(Job::ExpandNode { conn, node: root });
                            }
                        }
                    }
                    if crate::icons::toolbar_icon_btn(
                        ui,
                        crate::icons::ToolbarIcon::Save,
                        crate::ui_text::TB_SAVE,
                        ACCENT_DARK,
                    )
                    .on_hover_text(crate::ui_text::SAVE_CFG)
                    .clicked()
                    {
                        self.save_config();
                    }
                    if crate::icons::toolbar_icon_btn(
                        ui,
                        crate::icons::ToolbarIcon::OpenFile,
                        crate::ui_text::TB_OPEN,
                        Color32::from_rgb(200, 150, 40),
                    )
                    .on_hover_text(crate::ui_text::T_B1FBE3)
                    .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("TOML", &["toml"])
                            .pick_file()
                        {
                            match Config::load(Some(&path)) {
                                Ok((cfg, p)) => {
                                    self.config = cfg;
                                    self.config_path = p;
                                    self.trees.clear();
                                    for c in &self.config.connections {
                                        self.trees.insert(
                                            c.id.clone(),
                                            TreeNode::connection_root(&c.id, &c.name),
                                        );
                                    }
                                    self.app_state.remember_config(&self.config_path);
                                    self.push_msg(
                                        true,
                                        crate::ui_text::loaded(self.config_path.display()),
                                    );
                                }
                                Err(e) => self.push_msg(false, format!("{e:#}")),
                            }
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(10.0);
                        if self.busy {
                            ui.spinner();
                            ui.label(RichText::new(crate::ui_text::BUSY).color(ACCENT));
                        }
                        ui.label(RichText::new(&self.status).size(12.0).color(MUTED));
                    });
                });
            });
        egui::TopBottomPanel::bottom("messages")
            .resizable(true)
            .default_height(110.0)
            .min_height(60.0)
            .frame(Frame::new().fill(BG_MSG).stroke(Stroke::new(1.0, BORDER)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::ui_text::MESSAGES).strong().color(TEXT));
                    if ui.small_button(crate::ui_text::CLEAR).clicked() {
                        self.messages.clear();
                    }
                    ui.label(
                        RichText::new(crate::ui_text::config_path_line(self.config_path.display()))
                            .size(11.0)
                            .color(MUTED),
                    );
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (ok, msg) in self.messages.iter().rev().take(80) {
                            let color = if *ok { OK_GREEN } else { ERR_RED };
                            ui.label(RichText::new(msg).size(12.0).color(color));
                        }
                    });
            });
        egui::SidePanel::left("navigator")
            .resizable(true)
            .default_width(280.0)
            .min_width(200.0)
            .frame(Frame::new().fill(BG_NAV).stroke(Stroke::new(1.0, BORDER)))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::ui_text::CONNECTION).strong().color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{}", self.config.connections.len()))
                                .size(11.0)
                                .color(MUTED),
                        );
                    });
                });
                ui.add(
                    egui::TextEdit::singleline(&mut self.nav_filter)
                        .hint_text(crate::ui_text::T_54D40F)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for kind in BackendKind::ALL {
                            let group: Vec<_> = self
                                .config
                                .connections
                                .iter()
                                .filter(|c| c.kind == *kind)
                                .filter(|c| {
                                    self.nav_filter.is_empty()
                                        || c.name
                                            .to_ascii_lowercase()
                                            .contains(&self.nav_filter.to_ascii_lowercase())
                                })
                                .cloned()
                                .collect();
                            if group.is_empty() {
                                continue;
                            }
                            egui::CollapsingHeader::new(
                                RichText::new(kind.display_name()).strong().color(ACCENT_DARK),
                            )
                            .default_open(true)
                            .show(ui, |ui| {
                                for conn in group {
                                    self.draw_connection_tree(ui, &conn);
                                }
                            });
                        }
                        if self.config.connections.is_empty() {
                            ui.add_space(12.0);
                            ui.label(RichText::new(crate::ui_text::NO_CONN).color(MUTED));
                            ui.label(
                                RichText::new(crate::ui_text::T_CONFIG_EXAMPLE_TOML_1CE9F0)
                                    .size(12.0)
                                    .color(MUTED),
                            );
                        }
                    });
            });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG_WORK).inner_margin(0.0))
            .show(ctx, |ui| {
                self.draw_workspace(ui);
            });
        self.draw_conn_form(ctx);
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
            self.run_active_query();
        }
    }
}

impl DblensApp {

    fn draw_connection_tree(&mut self, ui: &mut Ui, conn: &Connection) {
        let root = self
            .trees
            .entry(conn.id.clone())
            .or_insert_with(|| TreeNode::connection_root(&conn.id, &conn.name))
            .clone();
        let selected = self.selected_conn_id.as_deref() == Some(conn.id.as_str());
        let online = self.connected.get(&conn.id).copied().unwrap_or(false);
        let expanded = self.expanded.get(&root.id).copied().unwrap_or(false);
        let loaded = self
            .trees
            .get(&conn.id)
            .map(|t| t.loaded)
            .unwrap_or(false);
        ui.horizontal(|ui| {
            let arrow = if expanded { crate::ui_text::T_AF3981 } else { crate::ui_text::T_DFF521 };
            if ui
                .add(egui::Button::new(RichText::new(arrow).size(11.0).color(MUTED)).frame(false))
                .clicked()
            {
                let next = !expanded;
                self.expanded.insert(root.id.clone(), next);
                if next && !loaded {
                    if !online {
                        self.worker.submit(Job::TestConnection {
                            conn: conn.clone(),
                        });
                    } else {
                        self.worker.submit(Job::ExpandNode {
                            conn: conn.clone(),
                            node: root.clone(),
                        });
                    }
                }
            }
            crate::icons::kind_badge(ui, conn.kind, 16.0);
            crate::icons::status_led(ui, online);
            let ep = conn.endpoint_label();
            let shown = if conn.name.trim() == ep || conn.name.contains("://") {
                ep
            } else {
                format!("{}  {}", conn.name, ep)
            };
            let label = RichText::new(shown)
                .color(if selected { ACCENT_DARK } else { TEXT })
                .size(13.0);
            let resp = ui.selectable_label(selected, label);
            // Double-click: expand/collapse. Single-click: select + details.
            if resp.double_clicked() {
                self.selected_conn_id = Some(conn.id.clone());
                self.selected_node = Some(root.clone());
                let next = !expanded;
                self.expanded.insert(root.id.clone(), next);
                if next && !loaded {
                    if !online {
                        self.worker.submit(Job::TestConnection {
                            conn: conn.clone(),
                        });
                    } else {
                        self.worker.submit(Job::ExpandNode {
                            conn: conn.clone(),
                            node: root.clone(),
                        });
                    }
                }
            } else if resp.clicked() {
                self.selected_conn_id = Some(conn.id.clone());
                self.selected_node = Some(root.clone());
                self.open_object_tab(&root);
            }
            resp.context_menu(|ui| {
                if ui.button(crate::ui_text::CONNECT_REFRESH).clicked() {
                    self.selected_conn_id = Some(conn.id.clone());
                    self.worker.submit(Job::TestConnection {
                        conn: conn.clone(),
                    });
                    ui.close_menu();
                }
                if ui.button(crate::ui_text::USE_IN_QUERY).clicked() {
                    self.selected_conn_id = Some(conn.id.clone());
                    self.open_query_tab_for_selection();
                    ui.close_menu();
                }
                if ui.button(crate::ui_text::EDIT).clicked() {
                    self.conn_form = ConnForm::edit(conn.clone());
                    ui.close_menu();
                }
                if ui.button(crate::ui_text::DELETE).clicked() {
                    self.config.connections.retain(|c| c.id != conn.id);
                    self.trees.remove(&conn.id);
                    self.save_config();
                    ui.close_menu();
                }
            });
        });
        // Only show children when expanded (fixes arrow unable to collapse).
        if expanded {
            ui.indent(format!("indent-{}", conn.id), |ui| {
                let children = self
                    .trees
                    .get(&conn.id)
                    .map(|t| t.children.clone())
                    .unwrap_or_default();
                for child in &children {
                    self.draw_tree_node(ui, conn, child, 1);
                }
            });
        }
    }

    fn draw_tree_node(&mut self, ui: &mut Ui, conn: &Connection, node: &TreeNode, depth: i32) {
        let selected = self
            .selected_node
            .as_ref()
            .map(|n| n.id == node.id)
            .unwrap_or(false);
        let expandable = node.expandable;
        let exp = self.expanded.get(&node.id).copied().unwrap_or(false);
        let has_meta = node.meta.status.is_some() || node.meta.meta_line.is_some();
        let height = if has_meta { 28.0 } else { 24.0 };
        let full_w = ui.available_width().max(48.0);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(full_w, height), Sense::click());

        let hovered = resp.hovered();
        let bg = if selected {
            SEL
        } else if hovered {
            Color32::from_rgb(236, 242, 248)
        } else {
            Color32::TRANSPARENT
        };
        if bg != Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), bg);
        }
        if selected {
            ui.painter().rect_filled(
                Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
                CornerRadius {
                    nw: 4,
                    sw: 4,
                    ne: 0,
                    se: 0,
                },
                ACCENT,
            );
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // subtle tree guide
        if depth > 0 {
            let gx = rect.min.x + 7.0;
            ui.painter().line_segment(
                [
                    Pos2::new(gx, rect.min.y),
                    Pos2::new(gx, rect.center().y),
                ],
                Stroke::new(1.0, Color32::from_rgb(220, 226, 234)),
            );
            ui.painter().line_segment(
                [
                    Pos2::new(gx, rect.center().y),
                    Pos2::new(gx + 8.0, rect.center().y),
                ],
                Stroke::new(1.0, Color32::from_rgb(220, 226, 234)),
            );
        }

        let arrow_zone = Rect::from_min_size(rect.min, Vec2::new(22.0, height));
        if expandable {
            let arrow = if exp { crate::ui_text::T_AF3981 } else { crate::ui_text::T_DFF521 };
            ui.painter().text(
                arrow_zone.center(),
                Align2::CENTER_CENTER,
                arrow,
                FontId::proportional(11.0),
                if hovered || selected { ACCENT_DARK } else { MUTED },
            );
        }

        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.min.x + 32.0, rect.center().y),
            Vec2::splat(15.0),
        );
        let icon_accent = match node.kind {
            NodeKind::Index => Color32::from_rgb(45, 120, 200),
            NodeKind::Folder => Color32::from_rgb(90, 130, 180),
            NodeKind::Database => Color32::from_rgb(0, 117, 143),
            NodeKind::Schema => Color32::from_rgb(60, 130, 170),
            NodeKind::Collection => Color32::from_rgb(77, 153, 85),
            NodeKind::Table => Color32::from_rgb(30, 90, 160),
            _ => ACCENT,
        };
        crate::icons::paint_tree_icon(
            ui,
            icon_rect,
            node.kind,
            node.meta.schema.as_deref(),
            node.meta.path.as_deref(),
            node.meta.status.as_deref(),
            icon_accent,
        );

        let label_color = if selected { ACCENT_DARK } else { TEXT };
        let title_x = rect.min.x + 44.0;
        let title_size = if node.kind == NodeKind::Index {
            12.5
        } else {
            12.0
        };
        let font_id = FontId::proportional(title_size);
        let meta_reserve = if node.meta.meta_line.is_some() {
            78.0
        } else {
            8.0
        };
        let label_right = (rect.max.x - meta_reserve).max(title_x + 24.0);
        let galley = ui.fonts(|f| f.layout_no_wrap(node.label.clone(), font_id, label_color));
        let title_w = galley.size().x.min(label_right - title_x);
        let title_pos = Pos2::new(title_x, rect.center().y - galley.size().y * 0.5);
        // with_clip_rect intersects with ScrollArea clip — never replace it
        // (set_clip_rect alone lets scrolled labels paint into the toolbar).
        ui.painter()
            .with_clip_rect(Rect::from_min_max(
                Pos2::new(title_x, rect.min.y),
                Pos2::new(label_right, rect.max.y),
            ))
            .galley(title_pos, galley, label_color);

        let cursor_x = title_x + title_w + 8.0;

        if let Some(status) = node.meta.status.as_deref() {
            let (chip_bg, chip_fg) = match status.to_ascii_lowercase().as_str() {
                "green" => (
                    Color32::from_rgb(220, 242, 230),
                    Color32::from_rgb(30, 120, 70),
                ),
                "yellow" => (
                    Color32::from_rgb(255, 244, 214),
                    Color32::from_rgb(160, 110, 20),
                ),
                "red" => (
                    Color32::from_rgb(252, 228, 226),
                    Color32::from_rgb(170, 50, 45),
                ),
                _ => (Color32::from_rgb(232, 236, 240), MUTED),
            };
            let chip_w = (status.len() as f32 * 6.5 + 14.0).clamp(36.0, 56.0);
            let chip_h = 16.0;
            let chip = Rect::from_min_size(
                Pos2::new(cursor_x, rect.center().y - chip_h * 0.5),
                Vec2::new(chip_w, chip_h),
            );
            if chip.max.x < rect.max.x - meta_reserve {
                ui.painter()
                    .rect_filled(chip, CornerRadius::same(8), chip_bg);
                ui.painter().circle_filled(
                    Pos2::new(chip.min.x + 8.0, chip.center().y),
                    3.0,
                    chip_fg,
                );
                ui.painter().text(
                    Pos2::new(chip.min.x + 14.0, chip.center().y),
                    Align2::LEFT_CENTER,
                    status,
                    FontId::proportional(10.0),
                    chip_fg,
                );
            }
        }

        if let Some(meta_line) = node.meta.meta_line.as_deref() {
            ui.painter().text(
                Pos2::new(rect.max.x - 6.0, rect.center().y),
                Align2::RIGHT_CENTER,
                meta_line,
                FontId::proportional(11.0),
                MUTED,
            );
        }

        let click_on_arrow = resp
            .interact_pointer_pos()
            .map(|p| arrow_zone.contains(p))
            .unwrap_or(false);

        // Arrow: expand/collapse. Double-click object: open. Single-click: select.
        if resp.double_clicked() {
            self.selected_node = Some(node.clone());
            self.selected_conn_id = Some(conn.id.clone());
            match node.kind {
                NodeKind::Table
                | NodeKind::Collection
                | NodeKind::Index
                | NodeKind::Key
                | NodeKind::Column => {
                    self.open_object_tab(node);
                }
                _ if expandable => {
                    let next = !exp;
                    self.expanded.insert(node.id.clone(), next);
                    if next && !node.loaded {
                        self.worker.submit(Job::ExpandNode {
                            conn: conn.clone(),
                            node: node.clone(),
                        });
                    }
                }
                _ => {}
            }
        } else if resp.clicked() {
            if expandable && click_on_arrow {
                let next = !exp;
                self.expanded.insert(node.id.clone(), next);
                if next && !node.loaded {
                    self.worker.submit(Job::ExpandNode {
                        conn: conn.clone(),
                        node: node.clone(),
                    });
                }
            } else {
                self.selected_node = Some(node.clone());
                self.selected_conn_id = Some(conn.id.clone());
            }
        }
        resp.context_menu(|ui| {
            if ui.button(crate::ui_text::OPEN).clicked() {
                self.open_object_tab(node);
                ui.close_menu();
            }
            if ui.button(crate::ui_text::USE_IN_QUERY).clicked() {
                self.selected_node = Some(node.clone());
                self.open_query_tab_for_selection();
                ui.close_menu();
            }
        });

        let exp_now = self.expanded.get(&node.id).copied().unwrap_or(false);
        if exp_now && !node.children.is_empty() {
            ui.indent(format!("n-{}-{depth}", node.id), |ui| {
                if let Some(fresh) = find_node(self.trees.get(&conn.id), &node.id) {
                    for child in &fresh.children.clone() {
                        self.draw_tree_node(ui, conn, child, depth + 1);
                    }
                } else {
                    for child in &node.children {
                        self.draw_tree_node(ui, conn, child, depth + 1);
                    }
                }
            });
        }
    }

    fn draw_workspace(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut close_id = None;
            let tab_ids: Vec<(u64, String, bool)> = self
                .tabs
                .iter()
                .map(|t| (t.id, t.title.clone(), self.active_tab == Some(t.id)))
                .collect();
            for (id, title, active) in tab_ids {
                let fill = if active { SEL } else { BG_TOOLBAR };
                let text = if active {
                    RichText::new(format!("  {title}  "))
                        .strong()
                        .color(ACCENT_DARK)
                } else {
                    RichText::new(format!("  {title}  ")).color(MUTED)
                };
                let resp = ui.add(
                    egui::Button::new(text)
                        .fill(fill)
                        .corner_radius(CornerRadius::ZERO),
                );
                if resp.clicked() {
                    self.active_tab = Some(id);
                }
                if resp.middle_clicked() {
                    close_id = Some(id);
                }
                resp.context_menu(|ui| {
                    if ui.button(crate::ui_text::CLOSE).clicked() {
                        close_id = Some(id);
                        ui.close_menu();
                    }
                    if ui.button(crate::ui_text::CLOSE_OTHERS).clicked() {
                        self.tabs.retain(|t| t.id == id);
                        self.active_tab = Some(id);
                        ui.close_menu();
                    }
                });
            }
            if ui
                .add(egui::Button::new(RichText::new(" + ").color(ACCENT)).fill(BG_TOOLBAR))
                .on_hover_text(crate::ui_text::NEW_QUERY)
                .clicked()
            {
                self.open_query_tab_for_selection();
            }
            if let Some(id) = close_id {
                self.tabs.retain(|t| t.id != id);
                if self.active_tab == Some(id) {
                    self.active_tab = self.tabs.last().map(|t| t.id);
                }
            }
        });
        ui.separator();
        let Some(active) = self.active_tab else {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(
                    RichText::new("DbLens")
                        .size(28.0)
                        .strong()
                        .color(ACCENT_DARK),
                );
                ui.label(
                    RichText::new(crate::ui_text::T_90DD80)
                        .color(MUTED),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "MySQL / PostgreSQL / SQL Server / SQLite / MongoDB / Redis / Memcached / Elasticsearch / etcd",
                    )
                    .size(12.0)
                    .color(MUTED),
                );
            });
            return;
        };
        let Some(tab_index) = self.tabs.iter().position(|t| t.id == active) else {
            return;
        };
        {
            let tab = &self.tabs[tab_index];
            let conn_name = self
                .config
                .find(&tab.connection_id)
                .map(|c| format!("{} ({})", c.name, c.kind.display_name()))
                .unwrap_or_else(|| tab.connection_id.clone());
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::ui_text::CONNECTION).size(12.0).color(MUTED));
                ui.label(RichText::new(conn_name).size(12.0).color(TEXT));
                ui.separator();
                ui.label(
                    RichText::new(self.tabs[tab_index].kind.display_name())
                        .size(12.0)
                        .color(MUTED),
                );
                // Query tabs show Run; Visual tabs use their own refresh toolbar.
                if self.tabs[tab_index].view_mode == ViewMode::Query {
                    ui.separator();
                    if ui
                        .add(
                            egui::Button::new(RichText::new(crate::ui_text::RUN_BTN).color(Color32::WHITE))
                                .fill(OK_GREEN),
                        )
                        .clicked()
                    {
                        self.run_active_query();
                    }
                    ui.label(RichText::new("Ctrl+Enter").size(11.0).color(MUTED));
                }
            });
        }
        ui.add_space(4.0);
        if self.tabs[tab_index].view_mode == ViewMode::Visual {
            self.draw_visual(ui, tab_index);
            return;
        }
        // Query view only: editor + results (fully separate from Visual).
        let avail = ui.available_height();
        let ratio = self.tabs[tab_index].editor_ratio.clamp(0.15, 0.75);
        let editor_h = avail * ratio;
        egui::Frame::new()
            .fill(Color32::from_rgb(250, 251, 252))
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.set_min_height(editor_h);
                let tab = &mut self.tabs[tab_index];
                egui::ScrollArea::vertical()
                    .max_height(editor_h - 8.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut tab.query)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(8)
                                .font(egui::TextStyle::Monospace),
                        );
                    });
            });
        let sep = ui.allocate_response(Vec2::new(ui.available_width(), 6.0), Sense::drag());
        if sep.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if sep.dragged() {
            let delta = sep.drag_delta().y;
            let tab = &mut self.tabs[tab_index];
            tab.editor_ratio = (tab.editor_ratio + delta / avail).clamp(0.15, 0.75);
        }
        ui.painter()
            .rect_filled(sep.rect, CornerRadius::ZERO, Color32::from_rgb(230, 234, 238));
        let tab = &self.tabs[tab_index];
        let col_n = tab.result.columns.len();
        let row_n = tab.result.rows.len();
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::ui_text::RESULT).strong().color(TEXT));
            ui.label(
                RichText::new(crate::ui_text::rows_cols(row_n, col_n))
                    .size(12.0)
                    .strong()
                    .color(ACCENT_DARK),
            );
            ui.label(RichText::new(&tab.result.message).size(12.0).color(MUTED));
            if let Some(a) = tab.result.affected {
                ui.label(RichText::new(format!("affected={a}")).size(12.0).color(MUTED));
            }
            if let Some(total) = tab.result.total {
                ui.label(RichText::new(crate::ui_text::total(total)).size(12.0).color(MUTED));
            }
            if tab.result.truncated {
                ui.label(RichText::new(crate::ui_text::TRUNCATED).size(12.0).color(ERR_RED));
            }
            ui.label(
                RichText::new(format!("{} ms", tab.result.elapsed_ms))
                    .size(12.0)
                    .color(MUTED),
            );
        });
        let columns = tab.result.columns.clone();
        let col_types = tab.result.column_types.clone();
        let rows = tab.result.rows.clone();
        let selected = tab.selected_row;
        if columns.is_empty() {
            ui.label(RichText::new(crate::ui_text::T_82B416).color(MUTED));
            return;
        }
        let avail = ui.available_height();
        let table_h = (avail * 0.62).max(120.0);
        if let Some(i) = draw_pro_table(ui, &columns, &col_types, &rows, selected, table_h) {
            self.tabs[tab_index].selected_row = Some(i);
        }
        ui.add_space(4.0);
        ui.separator();
        let sel = self.tabs[tab_index].selected_row;
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::ui_text::T_F82E51).strong().color(TEXT));
            if let Some(i) = sel {
                ui.label(
                    RichText::new(crate::ui_text::row_n(i + 1))
                        .size(12.0)
                        .color(MUTED),
                );
            } else {
                ui.label(RichText::new(crate::ui_text::T_550D79).size(12.0).color(MUTED));
            }
        });
        let detail = if let Some(i) = sel {
            rows.get(i)
                .map(|row| visual::row_detail(&columns, row))
                .unwrap_or_else(|| crate::ui_text::T_0EEF42.into())
        } else {
            crate::ui_text::T_14525A.into()
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut owned = detail;
                ui.add(
                    egui::TextEdit::multiline(&mut owned)
                        .desired_width(f32::INFINITY)
                        .desired_rows(8)
                        .font(egui::TextStyle::Monospace),
                );
            });
    }

    fn draw_visual(&mut self, ui: &mut Ui, tab_index: usize) {
        let tab_id = self.tabs[tab_index].id;
        let kind = self.tabs[tab_index].kind;
        let object_label = visual::display_object_label(
            kind,
            &self.tabs[tab_index].context,
            self.tabs[tab_index]
                .object_name
                .as_deref()
                .unwrap_or(crate::ui_text::OBJECT),
        );
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} / {}", kind.display_name(), object_label))
                    .strong()
                    .color(TEXT),
            );
            ui.separator();
            for panel in [VisualPanel::Data, VisualPanel::Structure, VisualPanel::Info] {
                // Cluster admin views only use Info-style dashboards.
                if kind == BackendKind::Elasticsearch
                    && self.tabs[tab_index].context.path.as_deref() == Some("__cluster__")
                    && panel != VisualPanel::Info
                {
                    continue;
                }
                let active = self.tabs[tab_index].visual_panel == panel;
                let fill = if active { SEL } else { BG_TOOLBAR };
                let label = panel.label(kind);
                let text = if active {
                    RichText::new(label).strong().color(ACCENT_DARK)
                } else {
                    RichText::new(label).color(MUTED)
                };
                if ui.add(egui::Button::new(text).fill(fill)).clicked() {
                    self.tabs[tab_index].visual_panel = panel;
                    self.run_visual(tab_id);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new(crate::ui_text::REFRESH).color(Color32::WHITE)).fill(ACCENT),
                    )
                    .clicked()
                {
                    self.run_visual(tab_id);
                }
            });
        });
        ui.add_space(6.0);
        // filter toolbar for Data (and Structure for KV scan)
        let show_filter = matches!(
            self.tabs[tab_index].visual_panel,
            VisualPanel::Data | VisualPanel::Structure
        ) || matches!(
            kind,
            BackendKind::Redis | BackendKind::Etcd | BackendKind::Memcached
        );
        if show_filter && self.tabs[tab_index].visual_panel != VisualPanel::Info {
            let hint = self.tabs[tab_index]
                .visual_panel
                .filter_hint(kind);
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::ui_text::T_C2FE62).color(MUTED));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.tabs[tab_index].filter)
                        .desired_width(300.0)
                        .hint_text(hint),
                );
                if matches!(
                    kind,
                    BackendKind::Mysql
                        | BackendKind::Mariadb
                        | BackendKind::Postgres
                        | BackendKind::Sqlite
                        | BackendKind::Mssql
                        | BackendKind::Oracle
                        | BackendKind::Snowflake
                        | BackendKind::Clickhouse
                        | BackendKind::Mongodb
                        | BackendKind::Elasticsearch
                        | BackendKind::Etcd
                ) && self.tabs[tab_index].visual_panel == VisualPanel::Data
                {
                    ui.label(RichText::new(crate::ui_text::T_A6F7F1).color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.tabs[tab_index].page_size)
                            .range(5..=visual::MAX_PAGE_SIZE)
                            .speed(1),
                    );
                }
                let go = ui
                    .add(
                        egui::Button::new(RichText::new(crate::ui_text::T_E5F71F).color(Color32::WHITE))
                            .fill(OK_GREEN),
                    )
                    .clicked();
                if go || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    let tab = &mut self.tabs[tab_index];
                    tab.page_size = tab.page_size.clamp(1, visual::MAX_PAGE_SIZE);
                    tab.page_from = 0;
                    self.run_visual(tab_id);
                }
                if self.tabs[tab_index].visual_panel == VisualPanel::Data
                    && (kind.is_sql()
                        || kind == BackendKind::Elasticsearch
                        || kind == BackendKind::Mongodb
                        || kind == BackendKind::Etcd)
                {
                    ui.separator();
                    let size = self.tabs[tab_index].page_size.max(1);
                    let from = self.tabs[tab_index].page_from;
                    let page = visual::page_number(from, size);
                    let row_n = self.tabs[tab_index].result.rows.len() as u32;
                    let has_more = row_n >= size
                        || self.tabs[tab_index]
                            .result
                            .total
                            .map(|t| from + row_n < t as u32)
                            .unwrap_or(false);
                    let total_txt = self.tabs[tab_index]
                        .result
                        .total
                        .map(|t| crate::ui_text::total_txt_suffix(t))
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(crate::ui_text::page_info(page, row_n, &total_txt))
                            .size(12.0)
                            .color(MUTED),
                    );
                    if ui
                        .add_enabled(from > 0, egui::Button::new(crate::ui_text::PREV_PAGE))
                        .clicked()
                    {
                        let tab = &mut self.tabs[tab_index];
                        tab.page_from = tab.page_from.saturating_sub(tab.page_size.max(1));
                        self.run_visual(tab_id);
                    }
                    if ui
                        .add_enabled(has_more, egui::Button::new(crate::ui_text::NEXT_PAGE))
                        .clicked()
                    {
                        self.tabs[tab_index].page_from =
                            self.tabs[tab_index].page_from.saturating_add(size);
                        self.run_visual(tab_id);
                    }
                }
            });
            ui.add_space(4.0);
        }
        ui.separator();
        {
            let tab = &self.tabs[tab_index];
            let col_n = tab.result.columns.len();
            let row_n = tab.result.rows.len();
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::ui_text::RESULT).strong().color(TEXT));
                ui.label(
                    RichText::new(crate::ui_text::rows_cols(row_n, col_n))
                        .size(12.0)
                        .strong()
                        .color(ACCENT_DARK),
                );
                ui.label(RichText::new(&tab.result.message).size(12.0).color(MUTED));
                if let Some(total) = tab.result.total {
                    ui.label(RichText::new(crate::ui_text::total(total)).size(12.0).color(MUTED));
                }
                if tab.result.truncated {
                    ui.label(RichText::new(crate::ui_text::TRUNCATED).size(12.0).color(ERR_RED));
                }
                ui.label(
                    RichText::new(format!("{} ms", tab.result.elapsed_ms))
                        .size(12.0)
                        .color(MUTED),
                );
            });
        }
        let columns = self.tabs[tab_index].result.columns.clone();
        let col_types = self.tabs[tab_index].result.column_types.clone();
        let rows = self.tabs[tab_index].result.rows.clone();
        let panel = self.tabs[tab_index].visual_panel;
        let selected = self.tabs[tab_index].selected_row;
        let es_action = self.tabs[tab_index].context.schema.clone();
        let is_es_cluster = kind == BackendKind::Elasticsearch
            && self.tabs[tab_index].context.path.as_deref() == Some("__cluster__");

        if columns.is_empty() {
            ui.add_space(20.0);
            ui.label(RichText::new(crate::ui_text::T_99D4BF).color(MUTED));
            return;
        }

        // ES cluster admin → graphical dashboard (not a raw table).
        if is_es_cluster {
            let result = self.tabs[tab_index].result.clone();
            if crate::es_dash::try_draw_cluster_dashboard(
                ui,
                es_action.as_deref().unwrap_or("health"),
                &result,
            ) {
                return;
            }
        }
        let is_json_view = columns.len() == 1 && (columns[0] == "json" || columns[0] == "value");
        // Structure/Info often JSON or wide text - use editor view
        if is_json_view || (panel != VisualPanel::Data && columns.len() == 1) {
            let max_json_rows = visual::MAX_RESULT_ROWS.min(200);
            let mut json_text = rows
                .iter()
                .take(max_json_rows)
                .map(|r| r.join(" | "))
                .collect::<Vec<_>>()
                .join("\n");
            if rows.len() > max_json_rows {
                json_text.push_str(&crate::ui_text::more_rows_hidden(rows.len() - max_json_rows));
            }
            if json_text.len() > 200_000 {
                json_text.truncate(200_000);
                json_text.push_str(crate::ui_text::TRUNCATED_LONG);
            }
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut owned = json_text;
                    ui.add(
                        egui::TextEdit::multiline(&mut owned)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(28)
                            .font(egui::TextStyle::Monospace),
                    );
                });
            return;
        }
        let avail = ui.available_height();
        let table_h = (avail * 0.55).max(140.0);
        if let Some(i) = draw_pro_table(ui, &columns, &col_types, &rows, selected, table_h) {
            self.tabs[tab_index].selected_row = Some(i);
        }
        ui.add_space(4.0);
        ui.separator();
        let sel = self.tabs[tab_index].selected_row;
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::ui_text::T_F82E51).strong().color(TEXT));
            if let Some(i) = sel {
                ui.label(
                    RichText::new(crate::ui_text::row_n(i + 1))
                        .size(12.0)
                        .color(MUTED),
                );
            } else {
                ui.label(RichText::new(crate::ui_text::T_550D79).size(12.0).color(MUTED));
            }
        });
        let detail = if let Some(i) = sel {
            rows.get(i)
                .map(|row| visual::row_detail(&columns, row))
                .unwrap_or_else(|| crate::ui_text::T_0EEF42.into())
        } else {
            crate::ui_text::T_14525A.into()
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut owned = detail;
                ui.add(
                    egui::TextEdit::multiline(&mut owned)
                        .desired_width(f32::INFINITY)
                        .desired_rows(10)
                        .font(egui::TextStyle::Monospace),
                );
            });
    }


    fn draw_conn_form(&mut self, ctx: &egui::Context) {
        if !self.conn_form.open {
            return;
        }
        let mut open = true;
        let editing = self.conn_form.editing_id.is_some();
        let title = if editing {
            crate::ui_text::EDIT_CONN
        } else {
            crate::ui_text::NEW_CONN
        };
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([740.0, if editing { 480.0 } else { 640.0 }])
            .min_size([700.0, if editing { 420.0 } else { 600.0 }])
            .show(ctx, |ui| {
                let mut draft = self.conn_form.draft.clone();
                let step = self.conn_form.step;
                let test_ok = self.conn_form.test_ok.clone();
                let mut action = WizardAction::None;

                if !editing {
                    self.draw_wizard_steps(ui, step, editing);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                }

                match step {
                    WizardStep::ChooseType => {
                        ui.label(
                            RichText::new(crate::ui_text::T_A22F6E)
                                .strong()
                                .size(14.0)
                                .color(TEXT),
                        );
                        ui.add_space(8.0);
                        // Fit all kinds without a scrollbar (4×4 grid for 13 types).
                        let kinds = BackendKind::ALL;
                        let cols = 4;
                        let card = Vec2::new(158.0, 86.0);
                        egui::Grid::new("kind_grid")
                            .num_columns(cols)
                            .spacing([12.0, 12.0])
                            .show(ui, |ui| {
                                for (i, k) in kinds.iter().enumerate() {
                                    let selected = draft.kind == *k;
                                    let resp =
                                        crate::icons::kind_type_card(ui, *k, selected, card);
                                    if resp.clicked() {
                                        let keep_id = draft.id.clone();
                                        let keep_name = draft.name.clone();
                                        draft = Connection::with_kind(*k);
                                        if editing {
                                            draft.id = keep_id;
                                            draft.name = keep_name;
                                        }
                                    }
                                    if (i + 1) % cols == 0 {
                                        ui.end_row();
                                    }
                                }
                                if kinds.len() % cols != 0 {
                                    ui.end_row();
                                }
                            });
                    }
                    WizardStep::General => {
                        ui.horizontal(|ui| {
                            crate::icons::kind_badge(ui, draft.kind, 28.0);
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} - {}",
                                    draft.kind.display_name(),
                                    if editing { crate::ui_text::EDIT_CONN } else { crate::ui_text::NEW_CONN }
                                ))
                                .strong()
                                .size(14.0)
                                .color(TEXT),
                            );
                        });
                        ui.add_space(8.0);
                        Self::draw_conn_general_fields(ui, &mut draft);
                        ui.add_space(8.0);
                        egui::CollapsingHeader::new(
                            RichText::new(crate::ui_text::ADVANCED).color(MUTED),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(crate::ui_text::T_URL_96AD22)
                                    .size(12.0)
                                    .color(MUTED),
                            );
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.set_min_width(90.0);
                                ui.label("URL");
                                let mut url = draft.url.clone().unwrap_or_default();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut url)
                                            .desired_width(360.0)
                                            .hint_text(
                                                "mysql://... / postgres://... / redis://...",
                                            ),
                                    )
                                    .changed()
                                {
                                    draft.url = if url.is_empty() { None } else { Some(url) };
                                }
                            });
                            ui.add_space(6.0);
                            ui.checkbox(
                                &mut draft.insecure,
                                crate::ui_text::T_TLS_686425,
                            );
                        });

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(crate::ui_text::TEST_CONN).color(Color32::WHITE),
                                    )
                                    .fill(ACCENT),
                                )
                                .clicked()
                            {
                                action = WizardAction::Test;
                            }
                            if let Some(msg) = &test_ok {
                                ui.label(RichText::new(msg).color(OK_GREEN));
                            }
                        });
                    }
                }

                if let Some(err) = &self.conn_form.error {
                    ui.add_space(6.0);
                    ui.colored_label(ERR_RED, err);
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(crate::ui_text::CANCEL).clicked() {
                        action = WizardAction::Cancel;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match step {
                            WizardStep::ChooseType => {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(crate::ui_text::NEXT_STEP).color(Color32::WHITE),
                                        )
                                        .fill(OK_GREEN),
                                    )
                                    .clicked()
                                {
                                    action = WizardAction::Next;
                                }
                            }
                            WizardStep::General => {
                                let save_label = if editing { crate::ui_text::SAVE } else { crate::ui_text::T_769D88 };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(save_label).color(Color32::WHITE),
                                        )
                                        .fill(OK_GREEN),
                                    )
                                    .clicked()
                                {
                                    action = WizardAction::Save;
                                }
                                if !editing
                                    && ui.button(crate::ui_text::BACK_STEP).clicked()
                                {
                                    action = WizardAction::Back;
                                }
                            }
                        }
                    });
                });

                self.conn_form.draft = draft.clone();
                match action {
                    WizardAction::None => {}
                    WizardAction::Cancel => {
                        self.conn_form.open = false;
                    }
                    WizardAction::Back => {
                        self.conn_form.test_ok = None;
                        self.conn_form.error = None;
                        self.conn_form.step = WizardStep::ChooseType;
                    }
                    WizardAction::Next => {
                        self.conn_form.test_ok = None;
                        self.conn_form.error = None;
                        self.conn_form.step = WizardStep::General;
                    }
                    WizardAction::Test => {
                        if let Some(err) = validate_conn_draft(&draft) {
                            self.conn_form.error = Some(err);
                            self.conn_form.test_ok = None;
                        } else {
                            let mut d = draft.clone();
                            d.ensure_defaults();
                            self.conn_form.draft = d.clone();
                            self.conn_form.pending_test = Some(d.clone());
                            self.conn_form.error = None;
                            self.conn_form.test_ok = None;
                            self.worker.submit(Job::TestConnection { conn: d });
                        }
                    }
                    WizardAction::Save => {
                        if let Some(err) = validate_conn_draft(&draft) {
                            self.conn_form.error = Some(err);
                            self.conn_form.draft = draft;
                        } else {
                            let mut d = draft;
                            d.ensure_defaults();
                            let id = d.id.clone();
                            if let Some(edit_id) = self.conn_form.editing_id.clone() {
                                if let Some(slot) = self.config.find_mut(&edit_id) {
                                    *slot = d.clone();
                                }
                            } else {
                                self.config.connections.push(d.clone());
                            }
                            self.trees
                                .insert(id.clone(), TreeNode::connection_root(&id, &d.name));
                            self.conn_form.draft = d.clone();
                            self.conn_form.error = None;
                            self.conn_form.test_ok = None;
                            self.conn_form.pending_test = None;
                            self.save_config();
                            self.selected_conn_id = Some(id.clone());
                            self.worker.submit(Job::TestConnection { conn: d });
                            self.conn_form.open = false;
                        }
                    }
                }
            });
        if !open {
            self.conn_form.open = false;
            self.conn_form.pending_test = None;
        }
    }

    fn draw_conn_general_fields(ui: &mut Ui, draft: &mut Connection) {
        ui.horizontal(|ui| {
            ui.set_min_width(90.0);
            ui.label(crate::ui_text::CONN_NAME);
            ui.add(
                egui::TextEdit::singleline(&mut draft.name)
                    .desired_width(320.0)
                    .hint_text(crate::ui_text::EG_NAME),
            );
        });
        ui.add_space(4.0);
        if draft.kind == BackendKind::Sqlite {
            ui.horizontal(|ui| {
                ui.set_min_width(90.0);
                ui.label(crate::ui_text::DB_FILE);
                let mut path = draft.database.clone().unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut path)
                            .desired_width(220.0)
                            .hint_text(crate::ui_text::T_DB_SQLITE_693A61),
                    )
                    .changed()
                {
                    draft.database = Some(path);
                }
                if ui.button(crate::ui_text::BROWSE).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("SQLite", &["db", "sqlite", "sqlite3"])
                        .add_filter(crate::ui_text::ALL_FILES, &["*"])
                        .pick_file()
                    {
                        draft.database = Some(p.display().to_string());
                    }
                }
                if ui.button(crate::ui_text::NEW_FILE).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("SQLite", &["db", "sqlite", "sqlite3"])
                        .set_file_name("data.db")
                        .save_file()
                    {
                        draft.database = Some(p.display().to_string());
                    }
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.set_min_width(90.0);
                ui.label(crate::ui_text::HOST);
                ui.add(
                    egui::TextEdit::singleline(&mut draft.host)
                        .desired_width(220.0)
                        .hint_text(match draft.kind {
                            BackendKind::Mssql => crate::ui_text::MSSQL_HOST_HINT,
                            BackendKind::Snowflake => crate::ui_text::SNOWFLAKE_ACCOUNT_HINT,
                            _ => "127.0.0.1",
                        }),
                );
                if draft.kind != BackendKind::Snowflake {
                    ui.label(crate::ui_text::PORT);
                    ui.add(egui::DragValue::new(&mut draft.port).range(0..=65535));
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.set_min_width(90.0);
                let db_label = match draft.kind {
                    BackendKind::Redis => crate::ui_text::DB_INDEX,
                    BackendKind::Mongodb => crate::ui_text::DEFAULT_DB,
                    BackendKind::Mssql => crate::ui_text::INIT_DATABASE,
                    BackendKind::Oracle => crate::ui_text::SERVICE_NAME,
                    BackendKind::Snowflake => crate::ui_text::DATABASE,
                    _ => crate::ui_text::DATABASE,
                };
                ui.label(db_label);
                let mut d = draft.database.clone().unwrap_or_default();
                let hint = match draft.kind {
                    BackendKind::Redis => "0",
                    BackendKind::Oracle => "ORCL / FREEPDB1",
                    _ => "",
                };
                ui.add(
                    egui::TextEdit::singleline(&mut d)
                        .desired_width(320.0)
                        .hint_text(hint),
                );
                draft.database = if d.is_empty() { None } else { Some(d) };
            });
            if draft.kind == BackendKind::Snowflake {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.set_min_width(90.0);
                    ui.label(crate::ui_text::WAREHOUSE);
                    let mut w = draft.warehouse.clone().unwrap_or_default();
                    ui.add(egui::TextEdit::singleline(&mut w).desired_width(140.0));
                    draft.warehouse = if w.is_empty() { None } else { Some(w) };
                    ui.label(crate::ui_text::ROLE);
                    let mut r = draft.role.clone().unwrap_or_default();
                    ui.add(egui::TextEdit::singleline(&mut r).desired_width(140.0));
                    draft.role = if r.is_empty() { None } else { Some(r) };
                });
            }
            if draft.kind == BackendKind::Mssql {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.set_min_width(90.0);
                    ui.label(crate::ui_text::AUTH_MODE);
                    egui::ComboBox::from_id_salt("mssql_auth_mode")
                        .width(280.0)
                        .selected_text(draft.auth_mode.label())
                        .show_ui(ui, |ui| {
                            for mode in MssqlAuthMode::ALL {
                                ui.selectable_value(
                                    &mut draft.auth_mode,
                                    *mode,
                                    mode.label(),
                                );
                            }
                        });
                });
            }
            let show_creds = draft.kind != BackendKind::Mssql
                || draft.auth_mode.needs_credentials();
            if show_creds {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.set_min_width(90.0);
                    ui.label(crate::ui_text::USERNAME);
                    let mut u = draft.username.clone().unwrap_or_default();
                    ui.add(egui::TextEdit::singleline(&mut u).desired_width(160.0));
                    // Always sync back — don't rely on Response::changed (misses some edits).
                    draft.username = if u.is_empty() { None } else { Some(u) };
                    ui.label(crate::ui_text::PASSWORD);
                    let mut p = draft.password.clone().unwrap_or_default();
                    ui.add(
                        egui::TextEdit::singleline(&mut p)
                            .password(true)
                            .desired_width(140.0),
                    );
                    draft.password = if p.is_empty() { None } else { Some(p) };
                });
            } else if draft.kind == BackendKind::Mssql
                && draft.auth_mode == MssqlAuthMode::Windows
            {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(crate::ui_text::MSSQL_WINDOWS_HINT)
                        .size(11.0)
                        .color(MUTED),
                );
            }
        }
    }

    fn draw_wizard_steps(&self, ui: &mut Ui, step: WizardStep, _editing: bool) {
        let steps: &[(&str, WizardStep)] = &[
            (crate::ui_text::STEP_TYPE, WizardStep::ChooseType),
            (crate::ui_text::T_2_34D252, WizardStep::General),
        ];
        ui.horizontal(|ui| {
            for (i, (label, s)) in steps.iter().enumerate() {
                let active = *s == step;
                let done = wizard_step_ord(step, false) > wizard_step_ord(*s, false);
                let color = if active {
                    ACCENT_DARK
                } else if done {
                    OK_GREEN
                } else {
                    MUTED
                };
                let text = if active {
                    RichText::new(*label).strong().color(color)
                } else {
                    RichText::new(*label).color(color)
                };
                ui.label(text);
                if i + 1 < steps.len() {
                    ui.label(RichText::new("  >  ").color(BORDER));
                }
            }
        });
    }
}

fn validate_conn_draft(draft: &Connection) -> Option<String> {
    if draft.name.trim().is_empty() {
        return Some(crate::ui_text::T_53158F.into());
    }
    if draft.kind == BackendKind::Sqlite
        && draft.database.as_deref().unwrap_or("").trim().is_empty()
    {
        return Some(crate::ui_text::NEED_SQLITE.into());
    }
    if draft.kind != BackendKind::Sqlite
        && draft.url.as_deref().unwrap_or("").is_empty()
        && draft.host.trim().is_empty()
    {
        return Some(crate::ui_text::NEED_HOST.into());
    }
    None
}


fn default_query_for_node(conn: &Connection, node: &TreeNode) -> String {
    match node.kind {
        NodeKind::Table => {
            let table = node
                .meta
                .table
                .clone()
                .unwrap_or_else(|| node.label.clone());
            match conn.kind {
                BackendKind::Mysql | BackendKind::Mariadb => {
                    if let Some(db) = &node.meta.database {
                        format!("SELECT * FROM `{db}`.`{table}` LIMIT 25;")
                    } else {
                        format!("SELECT * FROM `{table}` LIMIT 25;")
                    }
                }
                BackendKind::Postgres => {
                    if let (Some(schema), _) = (&node.meta.schema, &node.meta.database) {
                        format!("SELECT * FROM \"{schema}\".\"{table}\" LIMIT 25;")
                    } else {
                        format!("SELECT * FROM \"{table}\" LIMIT 25;")
                    }
                }
                BackendKind::Mssql => {
                    let schema = node.meta.schema.as_deref().unwrap_or("dbo");
                    if let Some(db) = &node.meta.database {
                        format!("SELECT TOP 25 * FROM [{db}].[{schema}].[{table}];")
                    } else {
                        format!("SELECT TOP 25 * FROM [{schema}].[{table}];")
                    }
                }
                BackendKind::Oracle => {
                    let schema = node
                        .meta
                        .schema
                        .as_deref()
                        .or(node.meta.database.as_deref())
                        .unwrap_or("USER");
                    format!(
                        "SELECT * FROM \"{schema}\".\"{table}\" FETCH FIRST 25 ROWS ONLY;"
                    )
                }
                BackendKind::Snowflake => {
                    let db = node.meta.database.as_deref().unwrap_or("DATABASE");
                    let schema = node.meta.schema.as_deref().unwrap_or("PUBLIC");
                    format!("SELECT * FROM \"{db}\".\"{schema}\".\"{table}\" LIMIT 25;")
                }
                BackendKind::Clickhouse => {
                    if let Some(db) = &node.meta.database {
                        format!("SELECT * FROM `{db}`.`{table}` LIMIT 25;")
                    } else {
                        format!("SELECT * FROM `{table}` LIMIT 25;")
                    }
                }
                BackendKind::Sqlite => format!("SELECT * FROM \"{table}\" LIMIT 25;"),
                _ => conn.kind.query_placeholder().to_string(),
            }
        }
        NodeKind::Index => {
            let index = node
                .meta
                .path
                .clone()
                .unwrap_or_else(|| node.label.split_whitespace().next().unwrap_or("*").to_string());
            format!("{{\n  \"query\": {{\"match_all\": {{}}}} ,\n  \"size\": 50\n}}\n# index: {index}")
        }
        NodeKind::Key if conn.kind == BackendKind::Elasticsearch => {
            match node.meta.schema.as_deref() {
                Some("docs") => {
                    let index = node.meta.path.as_deref().unwrap_or("_all");
                    format!("{{\n  \"query\": {{\"match_all\": {{}}}} ,\n  \"size\": 50\n}}\n# index: {index}")
                }
                Some("health") => "GET /_cluster/health".into(),
                Some("nodes") => "GET /_cat/nodes?v".into(),
                Some("mapping") => {
                    let index = node.meta.path.as_deref().unwrap_or("_all");
                    format!("GET /{index}/_mapping")
                }
                Some("settings") => {
                    let index = node.meta.path.as_deref().unwrap_or("_all");
                    format!("GET /{index}/_settings")
                }
                _ => conn.kind.query_placeholder().to_string(),
            }
        }
        NodeKind::Collection => {
            let coll = node
                .meta
                .table
                .clone()
                .unwrap_or_else(|| node.label.clone());
            format!("{{\n  \"find\": \"{coll}\",\n  \"filter\": {{}},\n  \"limit\": 100\n}}")
        }
        NodeKind::Key => node
            .meta
            .path
            .clone()
            .unwrap_or_else(|| node.label.clone()),
        _ => conn.kind.query_placeholder().to_string(),
    }
}

fn attach_children(root: &mut TreeNode, parent_id: &str, children: Vec<TreeNode>) -> bool {
    if root.id == parent_id {
        root.children = children;
        root.loaded = true;
        return true;
    }
    for child in &mut root.children {
        if find_node(Some(child), parent_id).is_some() {
            return attach_children(child, parent_id, children);
        }
    }
    false
}

fn find_node<'a>(root: Option<&'a TreeNode>, id: &str) -> Option<&'a TreeNode> {
    let root = root?;
    if root.id == id {
        return Some(root);
    }
    for c in &root.children {
        if let Some(n) = find_node(Some(c), id) {
            return Some(n);
        }
    }
    None
}


fn draw_pro_table(
    ui: &mut Ui,
    columns: &[String],
    column_types: &[String],
    rows: &[Vec<String>],
    selected: Option<usize>,
    table_h: f32,
) -> Option<usize> {
    let mut clicked = None;
    let row_h = 24.0;
    let header_h = 40.0;
    let gutter_w = 28.0;

    let numeric_flags: Vec<bool> = (0..columns.len())
        .map(|i| {
            let ty = column_types.get(i).map(|s| s.as_str()).unwrap_or("");
            if !ty.is_empty() {
                is_numeric_sql_type(ty)
            } else {
                infer_numeric_column(rows, i)
            }
        })
        .collect();
    let type_labels: Vec<String> = (0..columns.len())
        .map(|i| {
            let ty = column_types.get(i).map(|s| s.as_str()).unwrap_or("").trim();
            if !ty.is_empty() {
                display_sql_type(ty)
            } else if numeric_flags[i] {
                "NUMBER".into()
            } else {
                "TEXT".into()
            }
        })
        .collect();

    Frame::new()
        .stroke(Stroke::new(1.0, TABLE_OUTER))
        .corner_radius(CornerRadius::same(2))
        .inner_margin(0.0)
        .fill(Color32::WHITE)
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .max_height(table_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut table = TableBuilder::new(ui)
                        .striped(false)
                        .resizable(true)
                        .sense(Sense::click())
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .vscroll(true)
                        .min_scrolled_height((table_h - 4.0).max(80.0))
                        .auto_shrink([false, false])
                        .column(Column::exact(gutter_w).clip(true).resizable(false));
                    for (i, _) in columns.iter().enumerate() {
                        let w = if i == 0 { 140.0 } else { 120.0 };
                        table = table.column(
                            Column::initial(w)
                                .clip(true)
                                .resizable(true)
                                .at_least(56.0),
                        );
                    }
                    table
                        .header(header_h, |mut header| {
                            header.col(|ui| {
                                paint_header_cell(ui, TABLE_GUTTER_BG);
                            });
                            for (ci, col) in columns.iter().enumerate() {
                                header.col(|ui| {
                                    paint_header_cell(ui, TABLE_HEADER_BG);
                                    // soft column guide (header only, still faint)
                                    let rect = ui.max_rect();
                                    ui.painter().vline(
                                        rect.left(),
                                        rect.y_range(),
                                        Stroke::new(1.0, TABLE_VLINE),
                                    );
                                    ui.vertical(|ui| {
                                        ui.add_space(3.0);
                                        ui.horizontal(|ui| {
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new(col)
                                                    .strong()
                                                    .size(12.0)
                                                    .color(TABLE_HEADER_FG),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.add_space(6.0);
                                            let glyph = if numeric_flags[ci] { "#" } else { "abc" };
                                            ui.label(
                                                RichText::new(glyph)
                                                    .size(10.0)
                                                    .color(TABLE_TYPE_FG)
                                                    .monospace(),
                                            );
                                            ui.label(
                                                RichText::new(&type_labels[ci])
                                                    .size(10.0)
                                                    .color(TABLE_TYPE_FG),
                                            );
                                        });
                                    });
                                });
                            }
                        })
                        .body(|body| {
                            body.rows(row_h, rows.len(), |mut r| {
                                let row_idx = r.index();
                                let row = &rows[row_idx];
                                let is_sel = selected == Some(row_idx);
                                let alt = row_idx % 2 == 1;
                                let row_bg = if is_sel {
                                    TABLE_SEL_BG
                                } else if alt {
                                    TABLE_ROW_ALT
                                } else {
                                    Color32::WHITE
                                };
                                let text_color = if is_sel { Color32::WHITE } else { TEXT };
                                r.set_selected(is_sel);

                                // row gutter
                                r.col(|ui| {
                                    let rect = ui.max_rect();
                                    let gutter_bg = if is_sel {
                                        TABLE_SEL_GUTTER
                                    } else {
                                        TABLE_GUTTER_BG
                                    };
                                    ui.painter().rect_filled(
                                        rect,
                                        CornerRadius::ZERO,
                                        gutter_bg,
                                    );
                                    if is_sel {
                                        // focus bar on left edge
                                        let bar = Rect::from_min_max(
                                            rect.left_top(),
                                            Pos2::new(rect.left() + 3.0, rect.bottom()),
                                        );
                                        ui.painter().rect_filled(
                                            bar,
                                            CornerRadius::ZERO,
                                            Color32::from_rgb(0, 70, 150),
                                        );
                                    }
                                    // gutter | data — slightly clearer than column guides
                                    ui.painter().vline(
                                        rect.right() - 0.5,
                                        rect.y_range(),
                                        Stroke::new(1.0, TABLE_HLINE),
                                    );
                                });

                                for ci in 0..columns.len() {
                                    let cell = row.get(ci).map(|s| s.as_str()).unwrap_or("");
                                    let right = numeric_flags.get(ci).copied().unwrap_or(false);
                                    r.col(|ui| {
                                        let rect = ui.max_rect();
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::ZERO,
                                            row_bg,
                                        );
                                        // bottom hairline only — no column vlines so the row reads as one band
                                        ui.painter().hline(
                                            rect.x_range(),
                                            rect.bottom() - 0.5,
                                            Stroke::new(1.0, TABLE_HLINE),
                                        );
                                        let shown = visual::display_cell(cell);
                                        let text = RichText::new(shown)
                                            .size(12.0)
                                            .color(text_color)
                                            .monospace();
                                        if right {
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.add_space(6.0);
                                                    ui.label(text);
                                                },
                                            );
                                        } else {
                                            ui.add_space(6.0);
                                            ui.label(text);
                                        }
                                    });
                                }

                                let resp = r.response();
                                if resp.clicked() {
                                    clicked = Some(row_idx);
                                }
                            });
                        });
                });
        });
    clicked
}

fn paint_header_cell(ui: &mut Ui, bg: Color32) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, bg);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, TABLE_HLINE),
    );
}

fn display_sql_type(ty: &str) -> String {
    let base = ty.split('(').next().unwrap_or(ty).trim();
    // sqlx / PG often return lowercase or short names
    let up = base.to_ascii_uppercase();
    match up.as_str() {
        "INT2" | "SMALLINT" => "SMALLINT".into(),
        "INT4" | "INT" | "INTEGER" | "SERIAL" => "INTEGER".into(),
        "INT8" | "BIGINT" | "BIGSERIAL" => "BIGINT".into(),
        "FLOAT4" | "REAL" => "REAL".into(),
        "FLOAT8" | "DOUBLE" | "DOUBLE PRECISION" => "REAL".into(),
        "VARCHAR" | "CHARACTER VARYING" | "NVARCHAR" | "NTEXT" => "TEXT".into(),
        "CHAR" | "CHARACTER" | "NCHAR" | "BPCHAR" => "TEXT".into(),
        "BOOL" | "BOOLEAN" => "BOOLEAN".into(),
        "NUMERIC" | "DECIMAL" | "NUMBER" => "DECIMAL".into(),
        other => other.to_string(),
    }
}

fn is_numeric_sql_type(ty: &str) -> bool {
    let base = ty
        .split('(')
        .next()
        .unwrap_or(ty)
        .trim()
        .to_ascii_uppercase();
    matches!(
        base.as_str(),
        "INTEGER"
            | "INT"
            | "INT2"
            | "INT4"
            | "INT8"
            | "SMALLINT"
            | "BIGINT"
            | "TINYINT"
            | "MEDIUMINT"
            | "SERIAL"
            | "BIGSERIAL"
            | "REAL"
            | "FLOAT"
            | "FLOAT4"
            | "FLOAT8"
            | "DOUBLE"
            | "DOUBLE PRECISION"
            | "DECIMAL"
            | "NUMERIC"
            | "NUMBER"
            | "MONEY"
            | "SMALLMONEY"
            | "BIT"
            | "YEAR"
            | "COUNTER"
    ) || (base.contains("INT") && !base.contains("INTERVAL") && !base.contains("POINT"))
}

fn infer_numeric_column(rows: &[Vec<String>], col: usize) -> bool {
    let mut seen = 0usize;
    let mut numeric = 0usize;
    for row in rows.iter().take(40) {
        let Some(v) = row.get(col) else { continue };
        let t = v.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("null") {
            continue;
        }
        seen += 1;
        if t.parse::<f64>().is_ok() {
            numeric += 1;
        }
    }
    seen > 0 && numeric * 10 >= seen * 8
}

fn setup_cjk_fonts(ctx: &egui::Context) {
    let candidates: &[(&str, u32)] = &[
        (r"C:\Windows\Fonts\msyh.ttc", 0),
        (r"C:\Windows\Fonts\simhei.ttf", 0),
        (r"C:\Windows\Fonts\Deng.ttf", 0),
        ("/System/Library/Fonts/PingFang.ttc", 0),
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
    ];
    let mut loaded = None;
    for &(path, index) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            loaded = Some((path.to_string(), bytes, index));
            break;
        }
    }
    let Some((path, bytes, index)) = loaded else {
        eprintln!("CJK font not found");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    fonts
        .font_data
        .insert("cjk".to_owned(), std::sync::Arc::new(data));
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push("cjk".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("cjk".to_owned());
    ctx.set_fonts(fonts);
    eprintln!("Loaded CJK font: {path}");
}

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = false;
    style.visuals.panel_fill = BG_WORK;
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.extreme_bg_color = Color32::from_rgb(236, 240, 244);
    style.visuals.faint_bg_color = Color32::from_rgb(245, 247, 250);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(236, 240, 244);
    style.visuals.widgets.hovered.bg_fill = SEL;
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(200, 224, 248);
    style.visuals.selection.bg_fill = SEL;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.window_stroke = Stroke::new(1.0, BORDER);
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    ctx.set_style(style);
}
