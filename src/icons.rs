//! Navicat-like painted icons (no external image assets).
#![allow(dead_code)]

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2,
};

use crate::kind::BackendKind;
use crate::models::NodeKind;

impl BackendKind {
    /// Brand accent used for badges (Navicat-style distinct colors).
    pub fn brand_color(self) -> Color32 {
        match self {
            Self::Mysql => Color32::from_rgb(0, 117, 143),
            Self::Mariadb => Color32::from_rgb(180, 70, 40),
            Self::Postgres => Color32::from_rgb(51, 103, 145),
            Self::Mssql => Color32::from_rgb(168, 50, 72),
            Self::Sqlite => Color32::from_rgb(14, 78, 140),
            Self::Oracle => Color32::from_rgb(195, 32, 47),
            Self::Snowflake => Color32::from_rgb(41, 181, 232),
            Self::Clickhouse => Color32::from_rgb(255, 204, 0),
            Self::Mongodb => Color32::from_rgb(77, 153, 85),
            Self::Redis => Color32::from_rgb(196, 54, 54),
            Self::Memcached => Color32::from_rgb(90, 110, 140),
            Self::Elasticsearch => Color32::from_rgb(240, 150, 40),
            Self::Etcd => Color32::from_rgb(65, 145, 175),
        }
    }

    pub fn abbrev(self) -> &'static str {
        match self {
            Self::Mysql => "My",
            Self::Mariadb => "Ma",
            Self::Postgres => "Pg",
            Self::Mssql => "MS",
            Self::Sqlite => "Sq",
            Self::Oracle => "Or",
            Self::Snowflake => "Sf",
            Self::Clickhouse => "CH",
            Self::Mongodb => "Mg",
            Self::Redis => "Rd",
            Self::Memcached => "Mc",
            Self::Elasticsearch => "ES",
            Self::Etcd => "et",
        }
    }
}

/// Colored rounded badge with brand glyph (connection / type picker).
pub fn kind_badge(ui: &mut Ui, kind: BackendKind, size: f32) {
    let (rect, _resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_kind_badge(ui, rect, kind);
}

pub fn paint_kind_badge(ui: &mut Ui, rect: Rect, kind: BackendKind) {
    paint_backend_icon(ui, rect, kind);
}

/// Distinctive geometric brand mark for each backend (no external assets).
pub fn paint_backend_icon(ui: &mut Ui, rect: Rect, kind: BackendKind) {
    let p = ui.painter();
    let c = rect.center();
    let s = size_for(rect);
    let radius = (s * 0.22).clamp(2.0, 10.0);
    let fill = kind.brand_color();
    p.rect_filled(rect, CornerRadius::same(radius as u8), fill);
    // subtle top highlight
    let hi = Rect::from_min_max(
        rect.min,
        Pos2::new(rect.max.x, rect.min.y + rect.height() * 0.36),
    );
    p.rect_filled(
        hi,
        CornerRadius {
            nw: radius as u8,
            ne: radius as u8,
            sw: 0,
            se: 0,
        },
        Color32::from_rgba_unmultiplied(255, 255, 255, 28),
    );

    let ink = Color32::WHITE;
    let stroke = Stroke::new((s * 0.08).clamp(1.0, 2.0), ink);

    // Small badges: letter abbrev stays readable in the connection tree.
    if s < 20.0 {
        p.text(
            c,
            Align2::CENTER_CENTER,
            kind.abbrev(),
            FontId::proportional((s * 0.42).clamp(7.0, 10.0)),
            ink,
        );
        return;
    }

    match kind {
        BackendKind::Mysql | BackendKind::Mariadb => {
            // database cylinder
            let w = s * 0.42;
            let body = Rect::from_min_max(
                Pos2::new(c.x - w * 0.5, c.y - s * 0.08),
                Pos2::new(c.x + w * 0.5, c.y + s * 0.28),
            );
            p.rect_filled(body, CornerRadius::ZERO, ink.gamma_multiply(0.92));
            p.circle_filled(Pos2::new(c.x, c.y - s * 0.12), w * 0.5, ink);
            p.circle_filled(Pos2::new(c.x, c.y + s * 0.28), w * 0.5, ink.gamma_multiply(0.85));
        }
        BackendKind::Postgres => {
            // elephant-like: round head + ears
            p.circle_filled(c, s * 0.28, ink);
            p.circle_filled(Pos2::new(c.x - s * 0.28, c.y - s * 0.02), s * 0.16, ink);
            p.circle_filled(Pos2::new(c.x + s * 0.28, c.y - s * 0.02), s * 0.16, ink);
            p.circle_filled(Pos2::new(c.x - s * 0.1, c.y - s * 0.04), s * 0.05, fill);
            p.circle_filled(Pos2::new(c.x + s * 0.1, c.y - s * 0.04), s * 0.05, fill);
            // trunk
            p.line_segment(
                [Pos2::new(c.x, c.y + s * 0.08), Pos2::new(c.x + s * 0.06, c.y + s * 0.28)],
                Stroke::new((s * 0.1).clamp(1.5, 2.5), ink),
            );
        }
        BackendKind::Mssql => {
            // stacked server plates
            let w = s * 0.55;
            let h = s * 0.16;
            for (i, mult) in [(0_i32, 1.0_f32), (1, 0.9), (2, 0.8)] {
                let y = c.y - s * 0.22 + i as f32 * (h + s * 0.06);
                let r = Rect::from_center_size(Pos2::new(c.x, y), Vec2::new(w, h));
                p.rect_filled(r, CornerRadius::same(2), ink.gamma_multiply(mult));
                p.circle_filled(Pos2::new(r.min.x + s * 0.1, y), s * 0.035, fill);
            }
        }
        BackendKind::Oracle => {
            // red seal / O ring
            p.circle_stroke(c, s * 0.28, Stroke::new((s * 0.1).clamp(1.5, 2.8), ink));
            p.circle_filled(c, s * 0.12, ink);
        }
        BackendKind::Sqlite => {
            // feather / leaf
            let tip = Pos2::new(c.x + s * 0.22, c.y - s * 0.3);
            let base = Pos2::new(c.x - s * 0.2, c.y + s * 0.28);
            p.line_segment([tip, base], Stroke::new((s * 0.1).clamp(1.5, 2.4), ink));
            for t in [0.25_f32, 0.45, 0.65] {
                let along = Pos2::new(
                    tip.x + (base.x - tip.x) * t,
                    tip.y + (base.y - tip.y) * t,
                );
                let side = Pos2::new(along.x - s * 0.18, along.y - s * 0.02);
                p.line_segment([along, side], stroke);
            }
        }
        BackendKind::Snowflake => {
            // 6-point asterisk
            for a in 0..6 {
                let ang = std::f32::consts::PI * a as f32 / 3.0;
                let x = ang.cos() * s * 0.28;
                let y = ang.sin() * s * 0.28;
                p.line_segment(
                    [c, Pos2::new(c.x + x, c.y + y)],
                    Stroke::new((s * 0.09).clamp(1.4, 2.4), ink),
                );
            }
            p.circle_filled(c, s * 0.06, ink);
        }
        BackendKind::Clickhouse => {
            // house / chevron
            let pts = [
                Pos2::new(c.x, c.y - s * 0.3),
                Pos2::new(c.x + s * 0.28, c.y - s * 0.02),
                Pos2::new(c.x + s * 0.28, c.y + s * 0.28),
                Pos2::new(c.x - s * 0.28, c.y + s * 0.28),
                Pos2::new(c.x - s * 0.28, c.y - s * 0.02),
            ];
            p.add(Shape::convex_polygon(pts.to_vec(), ink, Stroke::NONE));
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x, c.y + s * 0.1), Vec2::new(s * 0.16, s * 0.22)),
                CornerRadius::same(1),
                fill,
            );
        }
        BackendKind::Mongodb => {
            // leaf / shield
            let pts = [
                Pos2::new(c.x, c.y - s * 0.32),
                Pos2::new(c.x + s * 0.26, c.y + s * 0.05),
                Pos2::new(c.x, c.y + s * 0.34),
                Pos2::new(c.x - s * 0.26, c.y + s * 0.05),
            ];
            p.add(Shape::convex_polygon(pts.to_vec(), ink, Stroke::NONE));
            p.line_segment(
                [Pos2::new(c.x, c.y - s * 0.22), Pos2::new(c.x, c.y + s * 0.28)],
                Stroke::new((s * 0.07).clamp(1.0, 2.0), fill),
            );
        }
        BackendKind::Redis => {
            // stacked cubes / diamond stack
            let d = s * 0.2;
            for (dy, mult) in [(-0.18_f32, 1.0_f32), (0.0, 0.9), (0.18, 0.8)] {
                let cy = c.y + dy * s;
                let pts = [
                    Pos2::new(c.x, cy - d * 0.7),
                    Pos2::new(c.x + d, cy),
                    Pos2::new(c.x, cy + d * 0.7),
                    Pos2::new(c.x - d, cy),
                ];
                p.add(Shape::convex_polygon(
                    pts.to_vec(),
                    ink.gamma_multiply(mult),
                    Stroke::NONE,
                ));
            }
        }
        BackendKind::Memcached => {
            // memory bars
            let w = s * 0.14;
            let gap = s * 0.08;
            for i in 0..3 {
                let x = c.x + (i as f32 - 1.0) * (w + gap);
                let h = s * (0.35 + i as f32 * 0.08);
                let r = Rect::from_center_size(Pos2::new(x, c.y), Vec2::new(w, h));
                p.rect_filled(r, CornerRadius::same(2), ink);
            }
        }
        BackendKind::Elasticsearch => {
            // magnifier over bars
            p.circle_stroke(Pos2::new(c.x - s * 0.06, c.y - s * 0.04), s * 0.2, stroke);
            p.line_segment(
                [
                    Pos2::new(c.x + s * 0.08, c.y + s * 0.1),
                    Pos2::new(c.x + s * 0.26, c.y + s * 0.28),
                ],
                Stroke::new((s * 0.1).clamp(1.5, 2.5), ink),
            );
            for (i, h) in [(0_i32, 0.2_f32), (1, 0.32), (2, 0.14)] {
                let x = c.x - s * 0.28 + i as f32 * s * 0.12;
                let top = c.y + s * 0.28 - h * s;
                p.line_segment(
                    [Pos2::new(x, c.y + s * 0.28), Pos2::new(x, top)],
                    Stroke::new(1.4, ink.gamma_multiply(0.85)),
                );
            }
        }
        BackendKind::Etcd => {
            // 3-node cluster
            let r = s * 0.1;
            let nodes = [
                Pos2::new(c.x, c.y - s * 0.2),
                Pos2::new(c.x - s * 0.22, c.y + s * 0.16),
                Pos2::new(c.x + s * 0.22, c.y + s * 0.16),
            ];
            for a in 0..3 {
                for b in (a + 1)..3 {
                    p.line_segment([nodes[a], nodes[b]], stroke);
                }
            }
            for n in nodes {
                p.circle_filled(n, r, ink);
            }
        }
    }
}

fn size_for(rect: Rect) -> f32 {
    rect.height().min(rect.width())
}

/// Large type card used in connection wizard.
pub fn kind_type_card(ui: &mut Ui, kind: BackendKind, selected: bool, size: Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let bg = if selected {
        Color32::from_rgb(230, 242, 255)
    } else if resp.hovered() {
        Color32::from_rgb(245, 248, 252)
    } else {
        Color32::from_rgb(252, 253, 255)
    };
    let border = if selected {
        kind.brand_color()
    } else {
        Color32::from_rgb(210, 216, 224)
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), bg);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(if selected { 2.0 } else { 1.0 }, border),
        StrokeKind::Inside,
    );

    let badge = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.min.y + 30.0),
        Vec2::splat(40.0),
    );
    paint_backend_icon(ui, badge, kind);

    ui.painter().text(
        Pos2::new(rect.center().x, rect.max.y - 18.0),
        Align2::CENTER_CENTER,
        kind.display_name(),
        FontId::proportional(12.5),
        if selected {
            kind.brand_color()
        } else {
            Color32::from_rgb(40, 48, 56)
        },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Small tree / object icon painted geometrically.
pub fn node_icon(ui: &mut Ui, kind: NodeKind, accent: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_node_icon(ui, rect, kind, accent);
}

pub fn paint_node_icon(ui: &mut Ui, rect: Rect, kind: NodeKind, accent: Color32) {
    let p = ui.painter();
    let c = rect.center();
    let s = size_for(rect);
    match kind {
        NodeKind::Connection => {
            paint_kind_style_dot(p, rect, accent);
        }
        NodeKind::Database => {
            // cylinder approximated with rect + circles
            let w = s * 0.72;
            let h = s * 0.55;
            let body = Rect::from_min_max(
                Pos2::new(c.x - w * 0.5, c.y - h * 0.2),
                Pos2::new(c.x + w * 0.5, c.y + h * 0.4),
            );
            p.rect_filled(body, CornerRadius::ZERO, accent.gamma_multiply(0.85));
            p.circle_filled(Pos2::new(c.x, c.y - h * 0.28), w * 0.42, accent);
            p.circle_filled(
                Pos2::new(c.x, c.y + h * 0.4),
                w * 0.42,
                accent.gamma_multiply(0.75),
            );
        }
        NodeKind::Schema => {
            let r = Rect::from_center_size(c, Vec2::splat(s * 0.7));
            p.rect_filled(r, CornerRadius::same(3), accent);
            p.line_segment(
                [Pos2::new(r.min.x + 3.0, r.center().y), Pos2::new(r.max.x - 3.0, r.center().y)],
                Stroke::new(1.2, Color32::WHITE),
            );
        }
        NodeKind::Table => {
            let r = Rect::from_center_size(c, Vec2::new(s * 0.78, s * 0.62));
            p.rect_filled(r, CornerRadius::same(2), accent);
            // grid lines
            let mid_y = r.min.y + r.height() * 0.38;
            p.line_segment(
                [Pos2::new(r.min.x, mid_y), Pos2::new(r.max.x, mid_y)],
                Stroke::new(1.0, Color32::WHITE),
            );
            let mid_x = r.center().x;
            p.line_segment(
                [Pos2::new(mid_x, mid_y), Pos2::new(mid_x, r.max.y)],
                Stroke::new(1.0, Color32::WHITE),
            );
        }
        NodeKind::Collection => {
            let r1 = Rect::from_center_size(Pos2::new(c.x - 1.0, c.y - 2.0), Vec2::new(s * 0.62, s * 0.48));
            let r2 = Rect::from_center_size(Pos2::new(c.x + 1.5, c.y + 2.0), Vec2::new(s * 0.62, s * 0.48));
            p.rect_filled(r2, CornerRadius::same(2), accent.gamma_multiply(0.7));
            p.rect_filled(r1, CornerRadius::same(2), accent);
        }
        NodeKind::Index => {
            // stacked "index cards"
            let w = s * 0.7;
            let h = s * 0.22;
            for (mult, dy) in [(0.55_f32, -3.0_f32), (0.75, -1.0), (1.0, 1.5)] {
                let r = Rect::from_center_size(Pos2::new(c.x, c.y + dy), Vec2::new(w, h));
                p.rect_filled(r, CornerRadius::same(2), accent.gamma_multiply(mult));
            }
        }
        NodeKind::Key => {
            // diamond
            let d = s * 0.38;
            let pts = [
                Pos2::new(c.x, c.y - d),
                Pos2::new(c.x + d, c.y),
                Pos2::new(c.x, c.y + d),
                Pos2::new(c.x - d, c.y),
            ];
            p.add(Shape::convex_polygon(pts.to_vec(), accent, Stroke::NONE));
        }
        NodeKind::Folder => {
            let tab = Rect::from_min_size(
                Pos2::new(c.x - s * 0.36, c.y - s * 0.28),
                Vec2::new(s * 0.34, s * 0.18),
            );
            let body = Rect::from_center_size(Pos2::new(c.x, c.y + 1.0), Vec2::new(s * 0.72, s * 0.48));
            p.rect_filled(tab, CornerRadius::same(2), accent.gamma_multiply(0.85));
            p.rect_filled(body, CornerRadius::same(2), accent);
        }
        NodeKind::Column => {
            p.circle_filled(c, s * 0.22, accent);
        }
    }
}

/// Tree-row icon: ES actions (`schema`), Navicat folders (`path`), views (`status`).
pub fn paint_tree_icon(
    ui: &mut Ui,
    rect: Rect,
    kind: NodeKind,
    schema: Option<&str>,
    path: Option<&str>,
    status: Option<&str>,
    accent: Color32,
) {
    if let Some(action) = schema {
        if paint_action_icon(ui, rect, action, accent) {
            return;
        }
    }
    if let Some(p) = path {
        if paint_nav_icon(ui, rect, p, accent) {
            return;
        }
    }
    if status == Some("view") {
        paint_view_icon(ui, rect, accent);
        return;
    }
    paint_node_icon(ui, rect, kind, accent);
}

/// Navicat-style category / leaf glyphs keyed by `meta.path`.
fn paint_nav_icon(ui: &mut Ui, rect: Rect, path: &str, accent: Color32) -> bool {
    let p = ui.painter();
    let c = rect.center();
    let s = size_for(rect);
    match path {
        "nav:tables" => {
            paint_node_icon(ui, rect, NodeKind::Table, Color32::from_rgb(30, 90, 160));
            true
        }
        "nav:views" => {
            paint_view_icon(ui, rect, Color32::from_rgb(40, 110, 170));
            true
        }
        "nav:indexes" | "nav:tbl_indexes" | "nav:index" => {
            // A-Z index badge
            let fill = Color32::from_rgb(45, 120, 200);
            let bg = Rect::from_center_size(c, Vec2::new(s * 0.88, s * 0.7));
            p.rect_filled(bg, CornerRadius::same(2), fill);
            p.text(
                c,
                Align2::CENTER_CENTER,
                "A↓",
                FontId::proportional(s * 0.42),
                Color32::WHITE,
            );
            true
        }
        "nav:triggers" | "nav:tbl_triggers" | "nav:trigger" => {
            let fill = Color32::from_rgb(220, 130, 40);
            p.line_segment(
                [Pos2::new(c.x - s * 0.05, c.y - s * 0.36), Pos2::new(c.x + s * 0.18, c.y - s * 0.02)],
                Stroke::new(2.0, fill),
            );
            p.line_segment(
                [Pos2::new(c.x + s * 0.18, c.y - s * 0.02), Pos2::new(c.x - s * 0.08, c.y - s * 0.02)],
                Stroke::new(2.0, fill),
            );
            p.line_segment(
                [Pos2::new(c.x - s * 0.08, c.y - s * 0.02), Pos2::new(c.x + s * 0.12, c.y + s * 0.36)],
                Stroke::new(2.0, fill),
            );
            true
        }
        "nav:columns" => {
            let fill = Color32::from_rgb(50, 130, 190);
            let base_y = c.y + s * 0.28;
            for (i, h) in [(0_i32, 0.42_f32), (1, 0.62), (2, 0.5)] {
                let x = c.x + (i as f32 - 1.0) * s * 0.22;
                let top = base_y - h * s;
                let r = Rect::from_min_max(Pos2::new(x - s * 0.08, top), Pos2::new(x + s * 0.08, base_y));
                p.rect_filled(r, CornerRadius::same(1), fill);
            }
            true
        }
        "nav:fks" | "nav:fk" => {
            let fill = Color32::from_rgb(50, 120, 180);
            p.circle_stroke(Pos2::new(c.x - s * 0.14, c.y), s * 0.18, Stroke::new(1.6, fill));
            p.circle_stroke(Pos2::new(c.x + s * 0.14, c.y), s * 0.18, Stroke::new(1.6, fill));
            p.line_segment(
                [Pos2::new(c.x - s * 0.02, c.y), Pos2::new(c.x + s * 0.02, c.y)],
                Stroke::new(1.6, fill),
            );
            true
        }
        "nav:uniques" | "nav:unique" => {
            let fill = Color32::from_rgb(200, 120, 50);
            let r = Rect::from_center_size(c, Vec2::splat(s * 0.55));
            p.rect_filled(r, CornerRadius::same(2), fill);
            p.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(s * 0.28)),
                CornerRadius::same(1),
                Stroke::new(1.4, Color32::WHITE),
                StrokeKind::Inside,
            );
            true
        }
        "nav:checks" | "nav:check" => {
            let fill = Color32::from_rgb(46, 140, 90);
            p.circle_filled(c, s * 0.36, fill);
            p.line_segment(
                [
                    Pos2::new(c.x - s * 0.16, c.y),
                    Pos2::new(c.x - s * 0.02, c.y + s * 0.14),
                ],
                Stroke::new(1.8, Color32::WHITE),
            );
            p.line_segment(
                [
                    Pos2::new(c.x - s * 0.02, c.y + s * 0.14),
                    Pos2::new(c.x + s * 0.18, c.y - s * 0.14),
                ],
                Stroke::new(1.8, Color32::WHITE),
            );
            true
        }
        _ if path.starts_with("nav:") => {
            paint_node_icon(ui, rect, NodeKind::Folder, accent);
            true
        }
        _ => false,
    }
}

fn paint_view_icon(ui: &mut Ui, rect: Rect, accent: Color32) {
    let p = ui.painter();
    let c = rect.center();
    let s = size_for(rect);
    // table body
    let r = Rect::from_center_size(c, Vec2::new(s * 0.78, s * 0.55));
    p.rect_filled(r, CornerRadius::same(2), accent);
    let mid_y = r.min.y + r.height() * 0.4;
    p.line_segment(
        [Pos2::new(r.min.x, mid_y), Pos2::new(r.max.x, mid_y)],
        Stroke::new(1.0, Color32::WHITE),
    );
    // eye overlay
    p.circle_filled(Pos2::new(c.x + s * 0.22, c.y - s * 0.22), s * 0.16, Color32::from_rgb(40, 140, 200));
    p.circle_filled(Pos2::new(c.x + s * 0.22, c.y - s * 0.22), s * 0.07, Color32::WHITE);
}

/// Returns true if a specialized action glyph was drawn.
fn paint_action_icon(ui: &mut Ui, rect: Rect, action: &str, accent: Color32) -> bool {
    let p = ui.painter();
    let c = rect.center();
    let s = size_for(rect);
    let fill = match action {
        "health" => Color32::from_rgb(46, 140, 90),
        "info" => Color32::from_rgb(45, 120, 200),
        "nodes" => Color32::from_rgb(70, 100, 150),
        "shards" => Color32::from_rgb(180, 120, 40),
        "aliases" => Color32::from_rgb(120, 80, 160),
        "templates" => Color32::from_rgb(70, 140, 150),
        "docs" => accent,
        "mapping" => Color32::from_rgb(50, 130, 170),
        "settings" => Color32::from_rgb(110, 118, 128),
        "stats" => Color32::from_rgb(200, 100, 50),
        "cluster" => Color32::from_rgb(45, 120, 200),
        _ => return false,
    };
    let bg = Rect::from_center_size(c, Vec2::splat(s * 0.92));
    p.rect_filled(bg, CornerRadius::same(3), fill);

    match action {
        "health" => {
            // soft heart-like circle pulse
            p.circle_filled(c, s * 0.22, Color32::WHITE);
            p.circle_stroke(c, s * 0.32, Stroke::new(1.2, Color32::WHITE));
        }
        "info" => {
            p.circle_stroke(c, s * 0.28, Stroke::new(1.4, Color32::WHITE));
            p.circle_filled(Pos2::new(c.x, c.y - s * 0.14), s * 0.06, Color32::WHITE);
            p.line_segment(
                [Pos2::new(c.x, c.y - s * 0.02), Pos2::new(c.x, c.y + s * 0.18)],
                Stroke::new(1.6, Color32::WHITE),
            );
        }
        "nodes" => {
            let r = s * 0.12;
            p.circle_filled(Pos2::new(c.x, c.y - s * 0.18), r, Color32::WHITE);
            p.circle_filled(Pos2::new(c.x - s * 0.18, c.y + s * 0.12), r, Color32::WHITE);
            p.circle_filled(Pos2::new(c.x + s * 0.18, c.y + s * 0.12), r, Color32::WHITE);
        }
        "shards" => {
            let cell = s * 0.22;
            let gap = s * 0.06;
            for dx in [-1.0_f32, 1.0] {
                for dy in [-1.0_f32, 1.0] {
                    let r = Rect::from_center_size(
                        Pos2::new(c.x + dx * (cell + gap) * 0.5, c.y + dy * (cell + gap) * 0.5),
                        Vec2::splat(cell),
                    );
                    p.rect_filled(r, CornerRadius::same(1), Color32::WHITE);
                }
            }
        }
        "aliases" => {
            let a = Rect::from_center_size(
                Pos2::new(c.x - s * 0.08, c.y),
                Vec2::new(s * 0.42, s * 0.32),
            );
            let b = Rect::from_center_size(
                Pos2::new(c.x + s * 0.08, c.y),
                Vec2::new(s * 0.42, s * 0.32),
            );
            p.rect_stroke(a, CornerRadius::same(2), Stroke::new(1.4, Color32::WHITE), StrokeKind::Inside);
            p.rect_stroke(b, CornerRadius::same(2), Stroke::new(1.4, Color32::WHITE), StrokeKind::Inside);
        }
        "templates" => {
            let r = Rect::from_center_size(c, Vec2::new(s * 0.48, s * 0.58));
            p.rect_filled(r, CornerRadius::same(2), Color32::WHITE);
            p.line_segment(
                [Pos2::new(r.min.x + 2.0, r.min.y + s * 0.18), Pos2::new(r.max.x - 2.0, r.min.y + s * 0.18)],
                Stroke::new(1.0, fill),
            );
            p.line_segment(
                [Pos2::new(r.min.x + 2.0, r.min.y + s * 0.3), Pos2::new(r.max.x - 2.0, r.min.y + s * 0.3)],
                Stroke::new(1.0, fill),
            );
        }
        "docs" => {
            let r = Rect::from_center_size(c, Vec2::new(s * 0.55, s * 0.45));
            p.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.3, Color32::WHITE), StrokeKind::Inside);
            p.line_segment(
                [Pos2::new(r.min.x, r.min.y + r.height() * 0.35), Pos2::new(r.max.x, r.min.y + r.height() * 0.35)],
                Stroke::new(1.0, Color32::WHITE),
            );
            p.line_segment(
                [Pos2::new(r.center().x, r.min.y + r.height() * 0.35), Pos2::new(r.center().x, r.max.y)],
                Stroke::new(1.0, Color32::WHITE),
            );
        }
        "mapping" => {
            p.circle_filled(Pos2::new(c.x, c.y - s * 0.16), s * 0.08, Color32::WHITE);
            p.circle_filled(Pos2::new(c.x - s * 0.18, c.y + s * 0.16), s * 0.08, Color32::WHITE);
            p.circle_filled(Pos2::new(c.x + s * 0.18, c.y + s * 0.16), s * 0.08, Color32::WHITE);
            p.line_segment(
                [Pos2::new(c.x, c.y - s * 0.08), Pos2::new(c.x - s * 0.18, c.y + s * 0.08)],
                Stroke::new(1.2, Color32::WHITE),
            );
            p.line_segment(
                [Pos2::new(c.x, c.y - s * 0.08), Pos2::new(c.x + s * 0.18, c.y + s * 0.08)],
                Stroke::new(1.2, Color32::WHITE),
            );
        }
        "settings" => {
            p.circle_stroke(c, s * 0.22, Stroke::new(1.6, Color32::WHITE));
            p.circle_filled(c, s * 0.08, Color32::WHITE);
            for ang in [0.0_f32, 60.0, 120.0, 180.0, 240.0, 300.0] {
                let rad = ang.to_radians();
                let outer = Pos2::new(c.x + rad.cos() * s * 0.34, c.y + rad.sin() * s * 0.34);
                let inner = Pos2::new(c.x + rad.cos() * s * 0.24, c.y + rad.sin() * s * 0.24);
                p.line_segment([inner, outer], Stroke::new(1.6, Color32::WHITE));
            }
        }
        "stats" => {
            let base_y = c.y + s * 0.2;
            for (i, h) in [(0_i32, 0.18_f32), (1, 0.32), (2, 0.24)] {
                let x = c.x + (i as f32 - 1.0) * s * 0.18;
                let top = base_y - h * s;
                p.line_segment(
                    [Pos2::new(x, base_y), Pos2::new(x, top)],
                    Stroke::new(2.0, Color32::WHITE),
                );
            }
        }
        "cluster" => {
            p.circle_filled(c, s * 0.12, Color32::WHITE);
            p.circle_stroke(c, s * 0.28, Stroke::new(1.3, Color32::WHITE));
        }
        _ => {}
    }
    true
}

fn paint_kind_style_dot(p: &egui::Painter, rect: Rect, color: Color32) {
    p.circle_filled(rect.center(), size_for(rect) * 0.32, color);
}

/// Toolbar button: large icon on top, caption below (Navicat ribbon style).
pub fn toolbar_icon_btn(
    ui: &mut Ui,
    icon: ToolbarIcon,
    caption: &str,
    accent: Color32,
) -> egui::Response {
    let desired = Vec2::new(68.0, 62.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let bg = if resp.is_pointer_button_down_on() {
        Color32::from_rgb(196, 220, 242)
    } else if resp.hovered() {
        Color32::from_rgb(220, 234, 248)
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), bg);
    }
    let icon_c = Pos2::new(rect.center().x, rect.min.y + 22.0);
    paint_toolbar_icon(ui, icon_c, icon, accent, 28.0);
    ui.painter().text(
        Pos2::new(rect.center().x, rect.max.y - 11.0),
        Align2::CENTER_CENTER,
        caption,
        FontId::proportional(11.0),
        Color32::from_rgb(48, 52, 58),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Thin vertical divider between toolbar groups.
pub fn toolbar_separator(ui: &mut Ui) {
    let h = 44.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 62.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        egui::Rangef::new(rect.center().y - h * 0.5, rect.center().y + h * 0.5),
        Stroke::new(1.0, Color32::from_rgb(210, 216, 224)),
    );
}

#[derive(Clone, Copy)]
pub enum ToolbarIcon {
    NewConnection,
    OpenQuery,
    Run,
    Refresh,
    Save,
    OpenFile,
}

fn paint_plus_badge(p: &egui::Painter, at: Pos2) {
    let r = 6.0;
    p.circle_filled(at, r, Color32::from_rgb(46, 160, 80));
    p.circle_stroke(at, r, Stroke::new(1.0, Color32::WHITE));
    let ink = Color32::WHITE;
    p.line_segment(
        [Pos2::new(at.x - 3.0, at.y), Pos2::new(at.x + 3.0, at.y)],
        Stroke::new(1.6, ink),
    );
    p.line_segment(
        [Pos2::new(at.x, at.y - 3.0), Pos2::new(at.x, at.y + 3.0)],
        Stroke::new(1.6, ink),
    );
}

fn paint_toolbar_icon(ui: &mut Ui, c: Pos2, icon: ToolbarIcon, accent: Color32, s: f32) {
    let p = ui.painter();
    match icon {
        ToolbarIcon::NewConnection => {
            // Two plugs linked
            let left = Pos2::new(c.x - 7.0, c.y);
            let right = Pos2::new(c.x + 5.0, c.y);
            p.circle_filled(left, 6.5, Color32::from_rgb(70, 140, 210));
            p.circle_filled(right, 6.5, Color32::from_rgb(45, 120, 200));
            p.line_segment(
                [Pos2::new(left.x + 5.0, left.y), Pos2::new(right.x - 5.0, right.y)],
                Stroke::new(2.5, Color32::from_rgb(30, 90, 160)),
            );
            // prongs
            p.line_segment(
                [Pos2::new(left.x - 6.5, left.y - 3.0), Pos2::new(left.x - 10.0, left.y - 3.0)],
                Stroke::new(2.0, Color32::from_rgb(30, 90, 160)),
            );
            p.line_segment(
                [Pos2::new(left.x - 6.5, left.y + 3.0), Pos2::new(left.x - 10.0, left.y + 3.0)],
                Stroke::new(2.0, Color32::from_rgb(30, 90, 160)),
            );
            paint_plus_badge(p, Pos2::new(c.x + 12.0, c.y + 10.0));
        }
        ToolbarIcon::OpenQuery => {
            // Stacked table sheets
            let back = Rect::from_center_size(
                Pos2::new(c.x + 2.0, c.y - 2.0),
                Vec2::new(s * 0.72, s * 0.58),
            );
            p.rect_filled(back, CornerRadius::same(2), Color32::from_rgb(160, 200, 235));
            let front = Rect::from_center_size(c, Vec2::new(s * 0.72, s * 0.58));
            p.rect_filled(front, CornerRadius::same(2), Color32::from_rgb(45, 120, 200));
            // grid lines
            let mid_y = front.center().y;
            p.hline(
                front.x_range(),
                mid_y,
                Stroke::new(1.2, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
            );
            p.vline(
                front.center().x,
                front.y_range(),
                Stroke::new(1.2, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
            );
            paint_plus_badge(p, Pos2::new(c.x + 12.0, c.y + 10.0));
        }
        ToolbarIcon::Run => {
            // Filled play in circle
            p.circle_filled(c, s * 0.42, Color32::from_rgb(46, 150, 90));
            let pts = vec![
                Pos2::new(c.x - 4.0, c.y - 7.0),
                Pos2::new(c.x + 8.0, c.y),
                Pos2::new(c.x - 4.0, c.y + 7.0),
            ];
            p.add(Shape::convex_polygon(pts, Color32::WHITE, Stroke::NONE));
        }
        ToolbarIcon::Refresh => {
            let r = s * 0.36;
            p.circle_stroke(c, r, Stroke::new(2.4, accent));
            // gap + arrow head
            let tip = Pos2::new(c.x + r * 0.55, c.y - r * 0.85);
            p.add(Shape::convex_polygon(
                vec![
                    tip,
                    Pos2::new(tip.x + 5.0, tip.y + 2.0),
                    Pos2::new(tip.x - 1.0, tip.y + 6.0),
                ],
                accent,
                Stroke::NONE,
            ));
        }
        ToolbarIcon::Save => {
            let body = Rect::from_center_size(c, Vec2::new(s * 0.7, s * 0.78));
            p.rect_filled(body, CornerRadius::same(2), Color32::from_rgb(55, 125, 195));
            // label slot
            let slot = Rect::from_min_max(
                Pos2::new(body.min.x + 4.0, body.min.y + 3.0),
                Pos2::new(body.max.x - 4.0, body.min.y + 11.0),
            );
            p.rect_filled(slot, CornerRadius::same(1), Color32::from_rgb(230, 240, 250));
            // bottom media
            let media = Rect::from_min_max(
                Pos2::new(body.min.x + 5.0, body.max.y - 12.0),
                Pos2::new(body.max.x - 5.0, body.max.y - 3.0),
            );
            p.rect_filled(media, CornerRadius::same(1), Color32::WHITE);
        }
        ToolbarIcon::OpenFile => {
            // Folder
            let tab = Rect::from_min_max(
                Pos2::new(c.x - s * 0.38, c.y - s * 0.28),
                Pos2::new(c.x - s * 0.05, c.y - s * 0.12),
            );
            p.rect_filled(tab, CornerRadius::same(1), Color32::from_rgb(230, 180, 60));
            let body = Rect::from_center_size(
                Pos2::new(c.x, c.y + 2.0),
                Vec2::new(s * 0.78, s * 0.52),
            );
            p.rect_filled(body, CornerRadius::same(2), Color32::from_rgb(240, 195, 70));
            p.rect_stroke(
                body,
                CornerRadius::same(2),
                Stroke::new(1.0, Color32::from_rgb(180, 130, 30)),
                StrokeKind::Outside,
            );
        }
    }
}

/// Status LED for connection online/offline.
pub fn status_led(ui: &mut Ui, online: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    let color = if online {
        Color32::from_rgb(46, 160, 90)
    } else {
        Color32::from_rgb(170, 176, 184)
    };
    ui.painter().circle_filled(rect.center(), 4.0, color);
    if online {
        ui.painter().circle_stroke(
            rect.center(),
            5.5,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(46, 160, 90, 90)),
        );
    }
}

/// Brand mark for top-left.
pub fn brand_mark(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(30, 90, 160));
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "A",
        FontId::proportional(size * 0.55),
        Color32::WHITE,
    );
}
