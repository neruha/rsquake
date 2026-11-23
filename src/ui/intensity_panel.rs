//! 震度パネル: 細分区域ごとの最大予測震度と長周期地震動階級。警報と予報でグループ分けする。

use egui::{Align2, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};

use crate::config::RegionDisplay;
use crate::eew::report::RegionForecast;
use crate::eew::store::EewEvent;
use crate::eew::summary;
use crate::ui::{theme, widgets};

const ROW_H: f32 = 34.0;
const HEADER_H: f32 = 30.0;

pub fn show(ui: &mut Ui, rect: Rect, event: Option<&EewEvent>, mode: RegionDisplay) {
    if mode == RegionDisplay::Summary {
        show_summary(ui, rect, event);
        return;
    }
    let inner = widgets::card(ui, rect);
    let regions: &[RegionForecast] = event
        .filter(|e| !e.is_canceled())
        .and_then(|e| e.latest.intensity.as_ref())
        .map(|i| i.regions.as_slice())
        .unwrap_or(&[]);
    let painter = ui.painter_at(rect);
    widgets::caption(&painter, inner.min, "細分区域の予想");
    let list_rect = Rect::from_min_max(inner.min + vec2(0.0, 22.0), inner.max);
    if regions.is_empty() {
        painter.text(
            list_rect.center(),
            Align2::CENTER_CENTER,
            "予想される区域はありません",
            theme::body_font(14.0),
            theme::TEXT_MUTED,
        );
        return;
    }

    let warnings: Vec<&RegionForecast> = regions.iter().filter(|r| r.is_warning).collect();
    let forecasts: Vec<&RegionForecast> = regions.iter().filter(|r| !r.is_warning).collect();
    let has_lg = regions.iter().any(|r| r.max_lg.is_some());

    let mut child = ui.new_child(UiBuilder::new().max_rect(list_rect));
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            for (title, color, list) in [
                ("緊急地震速報（警報）", theme::WARNING, &warnings),
                ("緊急地震速報（予報）", theme::FORECAST, &forecasts),
            ] {
                if list.is_empty() {
                    continue;
                }
                let h = HEADER_H + ROW_H * list.len() as f32 + 6.0;
                let (group, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
                let p = ui.painter_at(group);
                // 左端の帯
                let strip = Rect::from_min_size(group.min, vec2(4.0, h - 6.0));
                p.rect_filled(strip, 2.0, color);
                let x0 = group.min.x + 14.0;
                p.text(
                    pos2(x0, group.min.y + HEADER_H / 2.0),
                    Align2::LEFT_CENTER,
                    title,
                    theme::bold_font(15.0),
                    theme::TEXT,
                );
                if has_lg {
                    p.text(
                        pos2(group.max.x - 4.0, group.min.y + HEADER_H / 2.0),
                        Align2::RIGHT_CENTER,
                        "震度 / 長周期",
                        theme::body_font(12.0),
                        theme::TEXT_MUTED,
                    );
                }
                for (i, r) in list.iter().enumerate() {
                    let y = group.min.y + HEADER_H + ROW_H * i as f32;
                    let row = Rect::from_min_max(pos2(x0, y), pos2(group.max.x, y + ROW_H));
                    if i % 2 == 0 {
                        p.rect_filled(
                            row.shrink2(vec2(-6.0, 1.0)),
                            theme::RADIUS_SMALL,
                            theme::PANEL_RAISED,
                        );
                    }
                    let name = p.text(
                        row.left_center(),
                        Align2::LEFT_CENTER,
                        &r.name,
                        theme::body_font(15.0),
                        theme::TEXT,
                    );
                    if r.reached {
                        p.text(
                            name.right_center() + vec2(8.0, 0.0),
                            Align2::LEFT_CENTER,
                            "主要動到達",
                            theme::body_font(11.0),
                            theme::TEXT_MUTED,
                        );
                    }
                    let b = ROW_H - 8.0;
                    let mut x = row.max.x - 4.0;
                    if has_lg {
                        let w = 76.0;
                        x -= w;
                        if let Some(lg) = &r.max_lg {
                            draw_right(&p, pos2(x, row.min.y + 4.0), w, |p, min| {
                                widgets::lg_badge(p, min, b, lg)
                            });
                        }
                        x -= 8.0;
                    }
                    let w = 112.0;
                    x -= w;
                    draw_right(&p, pos2(x, row.min.y + 4.0), w, |p, min| {
                        widgets::shindo_badge(p, min, b, &r.max_int)
                    });
                }
            }
        });
}

/// まとめ表示: 警報の地域名だけを、警報色の帯の横に折り返して並べる。
fn show_summary(ui: &mut Ui, rect: Rect, event: Option<&EewEvent>) {
    let regions: &[RegionForecast] = event
        .filter(|e| !e.is_canceled())
        .and_then(|e| e.latest.intensity.as_ref())
        .map(|i| i.regions.as_slice())
        .unwrap_or(&[]);
    let items = summary::summarize(regions);
    // 表示する地域が無いときは、他のパネル (到達予想など) と同じ背景にする
    let inner = if items.is_empty() {
        widgets::card(ui, rect)
    } else {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::WARNING_BG);
        rect.shrink2(vec2(14.0, 12.0))
    };
    let painter = ui.painter_at(rect);
    widgets::caption(&painter, inner.min, "警報対象地域");
    let list_rect = Rect::from_min_max(inner.min + vec2(0.0, 22.0), inner.max);
    if items.is_empty() {
        painter.text(
            list_rect.center(),
            Align2::CENTER_CENTER,
            "警報対象地域なし",
            theme::body_font(14.0),
            theme::TEXT_MUTED,
        );
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(list_rect));
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            let width = ui.available_width();
            let font = theme::bold_font(26.0);
            // 項目の間隔は全角スペース 1 つ分。行頭には間隔を置かず、行末の間隔は捨てる。
            let gap = ui
                .painter()
                .layout_no_wrap("\u{3000}".into(), font.clone(), theme::TEXT)
                .size()
                .x;
            let galleys: Vec<_> = items
                .iter()
                .map(|i| {
                    ui.painter()
                        .layout_no_wrap(i.name.clone(), font.clone(), theme::TEXT)
                })
                .collect();
            let line_h = galleys.iter().map(|g| g.size().y).fold(0.0, f32::max);
            let max_x = (width - 8.0).max(40.0);

            // 各項目の左上の位置 (グループ左上からの相対)
            let mut pos = Vec::with_capacity(galleys.len());
            let (mut x, mut y) = (0.0_f32, 0.0_f32);
            for g in &galleys {
                let w = g.size().x;
                if x > 0.0 && x + w > max_x {
                    x = 0.0;
                    y += line_h;
                }
                pos.push(vec2(x, y));
                x += w + gap;
            }
            let h = y + line_h + 16.0;
            let (group, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
            let p = ui.painter_at(group);
            for (g, offset) in galleys.into_iter().zip(pos) {
                p.galley(group.min + vec2(4.0, 8.0) + offset, g, theme::TEXT);
            }
        });
}

/// 幅 `w` の枠の右端にそろえて描く。見えない painter で一度測ってから描く。
fn draw_right(
    p: &egui::Painter,
    min: egui::Pos2,
    w: f32,
    draw: impl Fn(&egui::Painter, egui::Pos2) -> f32,
) {
    let probe = p.with_clip_rect(Rect::NOTHING);
    let used = draw(&probe, min);
    draw(p, pos2(min.x + w - used, min.y));
}
