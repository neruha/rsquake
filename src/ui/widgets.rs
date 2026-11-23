//! パネルで共通に使う小さな部品。

use egui::{Align2, Color32, FontId, Pos2, Rect, Ui, vec2};

use crate::eew::report::{Forecast, LgClass, Shindo, Upper};
use crate::ui::theme;

/// 角丸の箱に文字を中央寄せで描き、箱の幅を返す。
#[allow(clippy::too_many_arguments)]
pub fn badge(
    painter: &egui::Painter,
    min: Pos2,
    height: f32,
    min_width: f32,
    text: &str,
    font: FontId,
    fill: Color32,
    color: Color32,
) -> f32 {
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let w = (galley.size().x + height * 0.5).max(min_width);
    let rect = Rect::from_min_size(min, vec2(w, height));
    painter.rect_filled(rect, theme::RADIUS_SMALL, fill);
    let pos = rect.center() - galley.size() / 2.0;
    painter.galley(pos, galley, color);
    w
}

/// 震度の箱。`over` は「5弱以上」のように出す。
pub fn shindo_badge(painter: &egui::Painter, min: Pos2, size: f32, f: &Forecast<Shindo>) -> f32 {
    let (label, color) = forecast_label(f, Shindo::label);
    let font = theme::display_font((size * label_scale(&label)).max(13.0));
    match f.representative() {
        Some(s) => badge(
            painter,
            min,
            size,
            size,
            &label,
            font,
            theme::shindo_color(s),
            theme::shindo_text_color(s),
        ),
        None => badge(
            painter,
            min,
            size,
            size,
            &label,
            font,
            theme::PANEL_RAISED,
            color,
        ),
    }
}

pub fn lg_badge(painter: &egui::Painter, min: Pos2, size: f32, f: &Forecast<LgClass>) -> f32 {
    let (label, color) = forecast_label(f, LgClass::label);
    let font = theme::display_font((size * label_scale(&label)).max(13.0));
    match f.representative() {
        Some(c) => badge(
            painter,
            min,
            size,
            size,
            &label,
            font,
            theme::lg_color(c),
            theme::lg_text_color(c),
        ),
        None => badge(
            painter,
            min,
            size,
            size,
            &label,
            font,
            theme::PANEL_RAISED,
            color,
        ),
    }
}

fn label_scale(label: &str) -> f32 {
    match label.chars().count() {
        0..=1 => 0.62,
        2 => 0.5,
        _ => 0.4,
    }
}

/// 電文の `from` / `to` を表示用の文字列にする。値は電文のまま。
pub fn forecast_label<T: Copy + PartialEq>(
    f: &Forecast<T>,
    label: fn(T) -> &'static str,
) -> (String, Color32) {
    match (f.from, f.to) {
        (Some(a), Upper::Over) => (format!("{}以上", label(a)), theme::TEXT),
        (Some(a), Upper::Value(b)) if a == b => (label(a).to_string(), theme::TEXT),
        (Some(a), Upper::Value(b)) => (format!("{}〜{}", label(a), label(b)), theme::TEXT),
        (None, Upper::Value(b)) => (label(b).to_string(), theme::TEXT),
        _ => ("不明".to_string(), theme::TEXT_MUTED),
    }
}

/// パネルの見出し (小さく、薄く)。
pub fn caption(painter: &egui::Painter, pos: Pos2, text: &str) {
    painter.text(
        pos,
        Align2::LEFT_TOP,
        text,
        theme::body_font(12.0),
        theme::TEXT_MUTED,
    );
}

/// 横向きのプログレスバー。
pub fn progress_bar(painter: &egui::Painter, rect: Rect, f: f32, color: Color32) {
    painter.rect_filled(rect, rect.height() / 2.0, theme::BG);
    let w = rect.width() * f.clamp(0.0, 1.0);
    if w > 0.5 {
        let filled = Rect::from_min_size(rect.min, vec2(w.max(rect.height()), rect.height()));
        painter.rect_filled(filled, rect.height() / 2.0, color);
    }
}

/// 角丸のカード領域を描き、内側の矩形を返す。
pub fn card(ui: &Ui, rect: Rect) -> Rect {
    ui.painter().rect_filled(rect, theme::RADIUS, theme::PANEL);
    rect.shrink2(vec2(14.0, 12.0))
}
