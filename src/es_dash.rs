//! Elasticsearch cluster dashboard widgets (health / nodes / shards …).

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};

use crate::models::QueryResult;

const TEXT: Color32 = Color32::from_rgb(32, 36, 42);
const MUTED: Color32 = Color32::from_rgb(110, 118, 128);
const CARD_BG: Color32 = Color32::from_rgb(248, 250, 252);
const CARD_BORDER: Color32 = Color32::from_rgb(220, 226, 234);

/// Returns true if this ES cluster action was rendered as a custom dashboard.
pub fn try_draw_cluster_dashboard(ui: &mut Ui, action: &str, result: &QueryResult) -> bool {
    match action {
        "health" | "cluster" => {
            draw_health_dashboard(ui, result);
            true
        }
        "info" => {
            draw_info_dashboard(ui, result);
            true
        }
        "nodes" => {
            draw_nodes_dashboard(ui, result);
            true
        }
        "shards" => {
            draw_shards_dashboard(ui, result);
            true
        }
        "aliases" => {
            draw_chip_list(ui, result, "alias", "别名", Color32::from_rgb(120, 80, 160));
            true
        }
        "templates" => {
            draw_chip_list(ui, result, "name", "模板", Color32::from_rgb(70, 140, 150));
            true
        }
        _ => false,
    }
}

fn kv_map(result: &QueryResult) -> Vec<(String, String)> {
    if result.columns.len() >= 2 {
        result
            .rows
            .iter()
            .filter_map(|r| {
                let k = r.first()?.clone();
                let v = r.get(1).cloned().unwrap_or_default();
                Some((k, v))
            })
            .collect()
    } else {
        Vec::new()
    }
}

fn find_kv<'a>(rows: &'a [(String, String)], key: &str) -> Option<&'a str> {
    rows.iter()
        .find(|(k, _)| k == key || k.ends_with(&format!(".{key}")))
        .map(|(_, v)| v.as_str())
}

fn health_color(status: &str) -> Color32 {
    match status.to_ascii_lowercase().as_str() {
        "green" => Color32::from_rgb(46, 160, 90),
        "yellow" => Color32::from_rgb(210, 160, 40),
        "red" => Color32::from_rgb(200, 70, 60),
        _ => Color32::from_rgb(120, 128, 140),
    }
}

fn draw_health_dashboard(ui: &mut Ui, result: &QueryResult) {
    let kv = kv_map(result);
    let status = find_kv(&kv, "status").unwrap_or("unknown");
    let color = health_color(status);
    let cluster = find_kv(&kv, "cluster_name").unwrap_or("—");

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // Status orb
                let (orb, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
                let c = orb.center();
                ui.painter().circle_filled(c, 42.0, color);
                ui.painter().circle_stroke(
                    c,
                    42.0,
                    Stroke::new(3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
                );
                ui.painter().circle_filled(
                    c,
                    28.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 40),
                );
                ui.painter().text(
                    c,
                    Align2::CENTER_CENTER,
                    status.to_ascii_uppercase(),
                    FontId::proportional(14.0),
                    Color32::WHITE,
                );

                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("集群健康").size(18.0).strong().color(TEXT));
                    ui.label(RichText::new(cluster).size(14.0).color(MUTED));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(match status.to_ascii_lowercase().as_str() {
                            "green" => "所有主分片与副本均已分配",
                            "yellow" => "所有主分片已分配，部分副本未分配",
                            "red" => "存在未分配的主分片",
                            _ => "状态未知",
                        })
                        .size(12.0)
                        .color(MUTED),
                    );
                });
            });

            ui.add_space(16.0);
            ui.label(RichText::new("关键指标").strong().color(TEXT));
            ui.add_space(8.0);

            let metrics: &[(&str, &str)] = &[
                ("number_of_nodes", "节点"),
                ("number_of_data_nodes", "数据节点"),
                ("active_primary_shards", "主分片"),
                ("active_shards", "活跃分片"),
                ("relocating_shards", "迁移中"),
                ("initializing_shards", "初始化中"),
                ("unassigned_shards", "未分配"),
                ("delayed_unassigned_shards", "延迟未分配"),
                ("active_shards_percent_as_number", "分片完成度 %"),
            ];

            egui::Grid::new("es_health_metrics")
                .num_columns(4)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    for (i, (key, label)) in metrics.iter().enumerate() {
                        let val = find_kv(&kv, key).unwrap_or("—");
                        metric_card(ui, label, val, accent_for_metric(key, val));
                        if i % 4 == 3 {
                            ui.end_row();
                        }
                    }
                    if metrics.len() % 4 != 0 {
                        ui.end_row();
                    }
                });
        });
}

fn accent_for_metric(key: &str, val: &str) -> Color32 {
    let n: f64 = val.parse().unwrap_or(0.0);
    if key.contains("unassigned") || key.contains("relocating") || key.contains("initializing") {
        if n > 0.0 {
            return Color32::from_rgb(210, 140, 40);
        }
    }
    Color32::from_rgb(45, 120, 200)
}

fn metric_card(ui: &mut Ui, label: &str, value: &str, accent: Color32) {
    let size = Vec2::new(140.0, 72.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), CARD_BG);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, CARD_BORDER),
        egui::StrokeKind::Outside,
    );
    // left accent bar
    ui.painter().rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height())),
        CornerRadius {
            nw: 8,
            sw: 8,
            ne: 0,
            se: 0,
        },
        accent,
    );
    ui.painter().text(
        Pos2::new(rect.min.x + 14.0, rect.min.y + 14.0),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(11.0),
        MUTED,
    );
    ui.painter().text(
        Pos2::new(rect.min.x + 14.0, rect.max.y - 14.0),
        Align2::LEFT_BOTTOM,
        value,
        FontId::proportional(22.0),
        TEXT,
    );
}

fn draw_info_dashboard(ui: &mut Ui, result: &QueryResult) {
    let kv = kv_map(result);
    let name = find_kv(&kv, "cluster_name").unwrap_or("—");
    let ver = find_kv(&kv, "number")
        .or_else(|| find_kv(&kv, "version.number"))
        .unwrap_or("—");
    let tagline = find_kv(&kv, "tagline").unwrap_or("");
    let lucene = find_kv(&kv, "lucene_version")
        .or_else(|| find_kv(&kv, "version.lucene_version"))
        .unwrap_or("—");

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            hero_banner(
                ui,
                "集群信息",
                name,
                Color32::from_rgb(45, 120, 200),
            );
            ui.add_space(12.0);
            egui::Grid::new("es_info_cards")
                .num_columns(3)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    metric_card(ui, "版本", ver, Color32::from_rgb(45, 120, 200));
                    metric_card(ui, "Lucene", lucene, Color32::from_rgb(70, 140, 150));
                    metric_card(
                        ui,
                        "节点名",
                        find_kv(&kv, "name").unwrap_or("—"),
                        Color32::from_rgb(90, 130, 180),
                    );
                    ui.end_row();
                });
            if !tagline.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new(tagline).italics().size(13.0).color(MUTED));
            }
            ui.add_space(16.0);
            ui.label(RichText::new("全部字段").strong().color(TEXT));
            ui.add_space(6.0);
            for (k, v) in &kv {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(k).size(12.0).color(MUTED).monospace());
                    ui.label(RichText::new(v).size(12.0).color(TEXT));
                });
            }
        });
}

fn hero_banner(ui: &mut Ui, title: &str, subtitle: &str, accent: Color32) {
    let w = ui.available_width().max(200.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 64.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), accent);
    ui.painter().text(
        Pos2::new(rect.min.x + 18.0, rect.center().y - 10.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        Color32::from_rgba_unmultiplied(255, 255, 255, 200),
    );
    ui.painter().text(
        Pos2::new(rect.min.x + 18.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(18.0),
        Color32::WHITE,
    );
}

fn col_index(columns: &[String], name: &str) -> Option<usize> {
    columns.iter().position(|c| c == name)
}

fn draw_nodes_dashboard(ui: &mut Ui, result: &QueryResult) {
    let cols = &result.columns;
    let name_i = col_index(cols, "name");
    let ip_i = col_index(cols, "ip");
    let heap_i = col_index(cols, "heap.percent");
    let ram_i = col_index(cols, "ram.percent");
    let cpu_i = col_index(cols, "cpu");
    let role_i = col_index(cols, "node.role").or_else(|| col_index(cols, "role"));
    let master_i = col_index(cols, "master");

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} 个节点", result.rows.len()))
                        .strong()
                        .size(16.0)
                        .color(TEXT),
                );
            });
            ui.add_space(10.0);

            let width = ui.available_width();
            let card_w = 220.0;
            let cols_n = ((width / (card_w + 12.0)).floor() as usize).clamp(1, 4);
            egui::Grid::new("es_node_cards")
                .num_columns(cols_n)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    for (i, row) in result.rows.iter().enumerate() {
                        let name = name_i
                            .and_then(|i| row.get(i))
                            .map(|s| s.as_str())
                            .unwrap_or("?");
                        let ip = ip_i
                            .and_then(|i| row.get(i))
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        let heap = heap_i
                            .and_then(|i| row.get(i))
                            .and_then(|s| s.parse::<f32>().ok())
                            .unwrap_or(0.0);
                        let ram = ram_i
                            .and_then(|i| row.get(i))
                            .and_then(|s| s.parse::<f32>().ok())
                            .unwrap_or(0.0);
                        let cpu = cpu_i
                            .and_then(|i| row.get(i))
                            .map(|s| s.as_str())
                            .unwrap_or("—");
                        let role = role_i
                            .and_then(|i| row.get(i))
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        let is_master = master_i
                            .and_then(|i| row.get(i))
                            .map(|s| s == "*" || s.eq_ignore_ascii_case("true"))
                            .unwrap_or(false);

                        node_card(ui, name, ip, heap, ram, cpu, role, is_master);
                        if (i + 1) % cols_n == 0 {
                            ui.end_row();
                        }
                    }
                    if result.rows.len() % cols_n != 0 {
                        ui.end_row();
                    }
                });
        });
}

fn node_card(
    ui: &mut Ui,
    name: &str,
    ip: &str,
    heap: f32,
    ram: f32,
    cpu: &str,
    role: &str,
    is_master: bool,
) {
    let size = Vec2::new(220.0, 118.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), CARD_BG);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, CARD_BORDER),
        egui::StrokeKind::Outside,
    );

    let accent = if is_master {
        Color32::from_rgb(210, 140, 40)
    } else {
        Color32::from_rgb(45, 120, 200)
    };
    ui.painter().circle_filled(
        Pos2::new(rect.min.x + 18.0, rect.min.y + 20.0),
        7.0,
        accent,
    );
    ui.painter().text(
        Pos2::new(rect.min.x + 32.0, rect.min.y + 20.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.0),
        TEXT,
    );
    if is_master {
        let badge = Rect::from_min_size(
            Pos2::new(rect.max.x - 52.0, rect.min.y + 10.0),
            Vec2::new(40.0, 16.0),
        );
        ui.painter()
            .rect_filled(badge, CornerRadius::same(8), Color32::from_rgb(255, 236, 200));
        ui.painter().text(
            badge.center(),
            Align2::CENTER_CENTER,
            "主",
            FontId::proportional(10.0),
            Color32::from_rgb(160, 100, 20),
        );
    }
    ui.painter().text(
        Pos2::new(rect.min.x + 14.0, rect.min.y + 40.0),
        Align2::LEFT_CENTER,
        if ip.is_empty() { role } else { ip },
        FontId::proportional(11.0),
        MUTED,
    );

    // heap / ram bars (labels inside card)
    ui.painter().text(
        Pos2::new(rect.min.x + 14.0, rect.min.y + 54.0),
        Align2::LEFT_CENTER,
        format!("堆 {heap:.0}%"),
        FontId::proportional(10.0),
        MUTED,
    );
    draw_pct_bar(
        ui,
        Rect::from_min_size(
            Pos2::new(rect.min.x + 70.0, rect.min.y + 50.0),
            Vec2::new(rect.width() - 84.0, 8.0),
        ),
        heap,
    );
    ui.painter().text(
        Pos2::new(rect.min.x + 14.0, rect.min.y + 72.0),
        Align2::LEFT_CENTER,
        format!("内存 {ram:.0}%"),
        FontId::proportional(10.0),
        MUTED,
    );
    draw_pct_bar(
        ui,
        Rect::from_min_size(
            Pos2::new(rect.min.x + 70.0, rect.min.y + 68.0),
            Vec2::new(rect.width() - 84.0, 8.0),
        ),
        ram,
    );
    ui.painter().text(
        Pos2::new(rect.min.x + 14.0, rect.max.y - 12.0),
        Align2::LEFT_CENTER,
        format!("CPU {cpu}%  ·  {role}"),
        FontId::proportional(10.0),
        MUTED,
    );
}

fn draw_pct_bar(ui: &mut Ui, rect: Rect, pct: f32) {
    let pct = pct.clamp(0.0, 100.0);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(230, 234, 240));
    let fill_w = rect.width() * (pct / 100.0);
    let fill = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
    let color = if pct >= 85.0 {
        Color32::from_rgb(200, 70, 60)
    } else if pct >= 70.0 {
        Color32::from_rgb(210, 150, 40)
    } else {
        Color32::from_rgb(46, 150, 90)
    };
    ui.painter()
        .rect_filled(fill, CornerRadius::same(4), color);
}

fn draw_shards_dashboard(ui: &mut Ui, result: &QueryResult) {
    let state_i = col_index(&result.columns, "state");
    let mut started = 0usize;
    let mut unassigned = 0usize;
    let mut relocating = 0usize;
    let mut initializing = 0usize;
    let mut other = 0usize;
    for row in &result.rows {
        let st = state_i
            .and_then(|i| row.get(i))
            .map(|s| s.as_str())
            .unwrap_or("")
            .to_ascii_uppercase();
        match st.as_str() {
            "STARTED" => started += 1,
            "UNASSIGNED" => unassigned += 1,
            "RELOCATING" => relocating += 1,
            "INITIALIZING" => initializing += 1,
            _ => other += 1,
        }
    }
    let total = result.rows.len().max(1) as f32;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("共 {} 个分片", result.rows.len()))
                    .strong()
                    .size(16.0)
                    .color(TEXT),
            );
            ui.add_space(12.0);

            // stacked distribution bar
            let w = ui.available_width().min(520.0);
            let (bar, _) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::hover());
            let segments = [
                (started, Color32::from_rgb(46, 160, 90), "STARTED"),
                (relocating, Color32::from_rgb(210, 150, 40), "RELOCATING"),
                (initializing, Color32::from_rgb(70, 140, 200), "INITIALIZING"),
                (unassigned, Color32::from_rgb(200, 70, 60), "UNASSIGNED"),
                (other, Color32::from_rgb(140, 148, 160), "OTHER"),
            ];
            let mut x = bar.min.x;
            for (n, color, _) in &segments {
                if *n == 0 {
                    continue;
                }
                let seg_w = bar.width() * (*n as f32 / total);
                let r = Rect::from_min_size(Pos2::new(x, bar.min.y), Vec2::new(seg_w, bar.height()));
                ui.painter().rect_filled(r, CornerRadius::ZERO, *color);
                x += seg_w;
            }
            ui.painter().rect_stroke(
                bar,
                CornerRadius::same(6),
                Stroke::new(1.0, CARD_BORDER),
                egui::StrokeKind::Outside,
            );

            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                for (n, color, label) in &segments {
                    if *n == 0 {
                        continue;
                    }
                    legend_chip(ui, *color, label, *n);
                }
            });

            ui.add_space(16.0);
            egui::Grid::new("es_shard_metrics")
                .num_columns(4)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    metric_card(ui, "已启动", &started.to_string(), Color32::from_rgb(46, 160, 90));
                    metric_card(
                        ui,
                        "未分配",
                        &unassigned.to_string(),
                        Color32::from_rgb(200, 70, 60),
                    );
                    metric_card(
                        ui,
                        "迁移中",
                        &relocating.to_string(),
                        Color32::from_rgb(210, 150, 40),
                    );
                    metric_card(
                        ui,
                        "初始化",
                        &initializing.to_string(),
                        Color32::from_rgb(70, 140, 200),
                    );
                    ui.end_row();
                });
        });
}

fn legend_chip(ui: &mut Ui, color: Color32, label: &str, n: usize) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, color);
        ui.label(
            RichText::new(format!("{label} {n}"))
                .size(11.0)
                .color(MUTED),
        );
    });
}

fn draw_chip_list(ui: &mut Ui, result: &QueryResult, name_col: &str, title: &str, accent: Color32) {
    let idx = col_index(&result.columns, name_col).unwrap_or(0);
    let secondary = result
        .columns
        .iter()
        .position(|c| c != name_col && c != "name");

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{title} · {} 项", result.rows.len()))
                    .strong()
                    .size(16.0)
                    .color(TEXT),
            );
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                for row in &result.rows {
                    let name = row.get(idx).map(|s| s.as_str()).unwrap_or("?");
                    let extra = secondary
                        .and_then(|i| row.get(i))
                        .map(|s| s.as_str())
                        .filter(|s| !s.is_empty() && *s != name)
                        .unwrap_or("");
                    let label = if extra.is_empty() {
                        name.to_string()
                    } else {
                        format!("{name}  →  {extra}")
                    };
                    let galley = ui.fonts(|f| {
                        f.layout_no_wrap(label.clone(), FontId::proportional(12.0), TEXT)
                    });
                    let pad = Vec2::new(24.0, 16.0);
                    let size = galley.size() + pad;
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
                    let bg = if resp.hovered() {
                        Color32::from_rgb(236, 242, 250)
                    } else {
                        CARD_BG
                    };
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(14), bg);
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(14),
                        Stroke::new(1.0, accent.gamma_multiply(0.35)),
                        egui::StrokeKind::Outside,
                    );
                    ui.painter().circle_filled(
                        Pos2::new(rect.min.x + 12.0, rect.center().y),
                        3.5,
                        accent,
                    );
                    ui.painter().galley(
                        Pos2::new(rect.min.x + 20.0, rect.center().y - galley.size().y * 0.5),
                        galley,
                        TEXT,
                    );
                }
            });
        });
}
