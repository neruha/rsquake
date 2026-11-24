//! 情報パネル: 種別と報数、震源、時刻、規模、深さ、最大予測震度、最大予測長周期地震動階級。
//!
//! デザインは 550 x 414 の座標系で組み、表示幅に合わせて拡縮する。

use chrono::{DateTime, Local, Utc};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Ui, pos2, vec2};

use crate::eew::report::{HypocenterKind, Shindo};
use crate::eew::store::EewEvent;
use crate::ui::scale_icons::ScaleIcons;
use crate::ui::{theme, widgets};

/// デザイン上のパネル全体の大きさ。
const DESIGN_W: f32 = 550.0;
const DESIGN_H: f32 = 414.0;

const BAND_H: f32 = 56.0;
/// タイトルを帯の中央から下げる量 (デザイン座標)。M+ は余白がほぼ対称なので 0。
const TITLE_DY: f32 = 0.0;
const CARD_TOP: f32 = 69.0;

const CARD_FILL: Color32 = Color32::from_rgb(0x29, 0x29, 0x29);
const FORECAST_BAND: Color32 = Color32::from_rgb(0xe6, 0xcb, 0x00);
const TEXT_SOFT: Color32 = Color32::from_rgb(0xd4, 0xd4, 0xd4);
const LABEL: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
/// 幅に対して必要な高さ。
pub fn height_for(width: f32) -> f32 {
    width * DESIGN_H / DESIGN_W
}

/// `position` は、複数のイベントがあるときの (表示中の番号, 全体の数)。
pub fn show(ui: &Ui, rect: Rect, event: Option<&EewEvent>, position: Option<(usize, usize)>) {
    let s = (rect.width() / DESIGN_W).min(rect.height() / DESIGN_H);
    let origin = rect.min;
    // デザイン座標 (パネル左上が原点) を画面の座標にする。
    let p = |x: f32, y: f32| pos2(origin.x + x * s, origin.y + y * s);
    let r = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(p(x, y), vec2(w * s, h * s));
    let painter = ui.painter_at(rect);

    let card = r(0.0, CARD_TOP, DESIGN_W, DESIGN_H - CARD_TOP);
    painter.rect_filled(card, 12.0 * s, CARD_FILL);

    let Some(ev) = event else {
        let band = r(0.0, 0.0, DESIGN_W, BAND_H);
        painter.rect_filled(band, 12.0 * s, theme::CANCELED);
        painter.text(
            card.center(),
            Align2::CENTER_CENTER,
            "緊急地震速報は発表されていません",
            theme::seed_regular_font(20.0 * s),
            theme::TEXT_MUTED,
        );
        return;
    };
    let rep = &ev.latest;

    // 種別と報数の帯
    let (fill, color, kind) = if rep.is_cancel {
        (theme::CANCELED, theme::TEXT, "緊急地震速報（取消）")
    } else if ev.ever_warning {
        (theme::WARNING, Color32::WHITE, "緊急地震速報（警報）")
    } else {
        (FORECAST_BAND, Color32::BLACK, "緊急地震速報（予報）")
    };
    let band = r(0.0, 0.0, DESIGN_W, BAND_H);
    painter.rect_filled(band, 12.0 * s, fill);
    painter.text(
        p(18.0, BAND_H / 2.0 + TITLE_DY),
        Align2::LEFT_CENTER,
        kind,
        theme::title_font(30.0 * s),
        color,
    );
    let serial_rect = painter.text(
        p(DESIGN_W - 17.0, BAND_H / 2.0),
        Align2::RIGHT_CENTER,
        format!("#{}", rep.serial),
        theme::serial_font(32.0 * s),
        color,
    );
    // 最終報と訓練・試験のバッジは報数の左に並べる
    let mut right = serial_rect.min.x - 12.0 * s;
    let mut tag = |text: &str, fill: Color32, fg: Color32| {
        let h = 28.0 * s;
        let galley = painter.layout_no_wrap(text.to_string(), theme::seed_bold_font(16.0 * s), fg);
        let w = galley.size().x + 16.0 * s;
        let rect = Rect::from_min_size(pos2(right - w, band.center().y - h / 2.0), vec2(w, h));
        painter.rect_filled(rect, 6.0 * s, fill);
        painter.galley(rect.center() - galley.size() / 2.0, galley, fg);
        right = rect.min.x - 8.0 * s;
    };
    if rep.is_last && !rep.is_cancel {
        tag("最終", Color32::BLACK, Color32::WHITE);
    }
    if let Some(label) = rep.status.label() {
        tag(label, theme::TRAINING, theme::BG);
    }
    if let Some((i, n)) = position.filter(|&(_, n)| n > 1) {
        tag(&format!("{i}/{n}"), Color32::BLACK, Color32::WHITE);
    }

    let Some(q) = &rep.quake else {
        painter.text(
            p(18.0, CARD_TOP + 36.0),
            Align2::LEFT_CENTER,
            "この緊急地震速報は取り消されました",
            theme::seed_bold_font(24.0 * s),
            theme::TEXT,
        );
        return;
    };

    // 震源地名 + 「で地震」
    let suffix = match q.kind {
        HypocenterKind::Assumed => "（仮定震源）",
        HypocenterKind::LowAccuracy => "（精度低）で地震",
        HypocenterKind::Normal => "で地震",
    };
    let max_w = (DESIGN_W - 36.0) * s;
    let mut name_size = 40.0 * s;
    let measure = |text: &str, font: FontId| {
        painter
            .layout_no_wrap(text.to_string(), font, Color32::WHITE)
            .size()
            .x
    };
    let total = measure(&q.name, theme::seed_bold_font(name_size))
        + 12.0 * s
        + measure(suffix, theme::seed_regular_font(name_size * 0.5));
    if total > max_w {
        name_size *= max_w / total;
    }
    let name_rect = painter.text(
        p(18.0, CARD_TOP + 38.0),
        Align2::LEFT_CENTER,
        &q.name,
        theme::seed_bold_font(name_size),
        Color32::WHITE,
    );
    painter.text(
        pos2(name_rect.max.x + 12.0 * s, name_rect.max.y - 4.0 * s),
        Align2::LEFT_BOTTOM,
        suffix,
        theme::seed_regular_font(name_size * 0.5),
        Color32::WHITE,
    );

    // 発生時刻 (なければ検知時刻)
    let (time_label, time) = match (q.origin_time, q.arrival_time) {
        (Some(t), _) => ("発生", Some(t)),
        (None, Some(t)) => ("検知", Some(t)),
        _ => ("発生", None),
    };
    let time_text = match time {
        Some(t) => format!("{} {time_label}", fmt_time(t - ev.time_offset)),
        None => "不明".to_string(),
    };
    painter.text(
        p(18.0, CARD_TOP + 75.0),
        Align2::LEFT_CENTER,
        time_text,
        theme::mplus_light_font(20.0 * s),
        TEXT_SOFT,
    );

    // 最大予測震度
    let icons = ScaleIcons::get(ui.ctx());
    let intensity = rep.intensity.as_ref();
    let max_int = intensity.map(|i| &i.max_int);
    let shindo_icon = icons.shindo(max_int.and_then(|f| f.representative()));
    let box_y = CARD_TOP + 103.0;
    painter.rect_filled(r(18.0, box_y, 270.0, 106.0), 6.9 * s, shindo_icon.box_color);
    // 「5弱以上」「4〜5弱」のように幅がある時は、ラベルの下に補足する
    let range_label = max_int
        .map(|f| widgets::forecast_label(f, Shindo::label).0)
        .filter(|l| l.contains('以') || l.contains('〜'));
    let label_y = if range_label.is_some() {
        box_y + 46.0
    } else {
        box_y + 53.0
    };
    two_lines(
        &painter,
        p(100.0, label_y),
        ("最大", "予測震度"),
        27.0 * s,
        32.0 * s,
    );
    if let Some(label) = range_label {
        painter.text(
            p(100.0, box_y + 98.0),
            Align2::CENTER_BOTTOM,
            label,
            theme::seed_bold_font(15.0 * s),
            TEXT_SOFT,
        );
    }
    painter.image(
        shindo_icon.texture.id(),
        r(190.0, box_y + 8.0, 90.0, 90.0),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );

    // 最大予測長周期地震動階級。予測が無ければ none、階級が読めなければ不明
    let lg_y = CARD_TOP + 221.0;
    let lg_icon_data = match intensity.and_then(|i| i.max_lg.as_ref()) {
        Some(f) => icons.lg(f.representative()),
        None => icons.none(),
    };
    painter.rect_filled(r(18.0, lg_y, 270.0, 106.0), 6.9 * s, lg_icon_data.box_color);
    two_lines(
        &painter,
        p(100.0, lg_y + 53.0),
        ("最大予測長周期", "地震動階級"),
        18.5 * s,
        22.0 * s,
    );
    painter.image(
        lg_icon_data.texture.id(),
        r(190.0, lg_y + 8.0, 90.0, 90.0),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    // 規模と深さ
    let mag = match (q.kind, q.magnitude) {
        (HypocenterKind::Assumed, _) => "-".to_string(),
        (_, None) if q.magnitude_over_8 => "M8超".to_string(),
        (_, Some(m)) => format!("M{m:.1}"),
        (_, None) => "不明".to_string(),
    };
    let depth = match (q.kind, q.depth_km) {
        (HypocenterKind::Assumed, _) => "-".to_string(),
        (_, Some(_)) if q.depth_over_700 => "700km以上".to_string(),
        (_, Some(d)) if d <= 0.0 => "ごく浅い".to_string(),
        (_, Some(d)) => format!("{d:.0}km"),
        (_, None) => "不明".to_string(),
    };
    let col_w = (DESIGN_W - 18.0 - 313.0) * s;
    value_block(&painter, p(313.0, CARD_TOP + 122.0), "規模", &mag, col_w, s);
    value_block(
        &painter,
        p(315.0, CARD_TOP + 239.0),
        "深さ",
        &depth,
        col_w,
        s,
    );
}

/// 小さな見出しと大きな数値の組。
fn value_block(painter: &Painter, pos: Pos2, title: &str, value: &str, max_w: f32, s: f32) {
    painter.text(
        pos,
        Align2::LEFT_CENTER,
        title,
        theme::seed_regular_font(20.0 * s),
        TEXT_SOFT,
    );
    let mut size = 54.0 * s;
    let w = painter
        .layout_no_wrap(
            value.to_string(),
            theme::mplus_medium_font(size),
            Color32::WHITE,
        )
        .size()
        .x;
    if w > max_w {
        size *= max_w / w;
    }
    painter.text(
        pos + vec2(-2.0 * s, 42.0 * s),
        Align2::LEFT_CENTER,
        value,
        theme::mplus_medium_font(size),
        Color32::WHITE,
    );
}

/// 中心を基準に 2 行の文字を中央揃えで描く。
fn two_lines(painter: &Painter, center: Pos2, lines: (&str, &str), size: f32, gap: f32) {
    for (i, text) in [lines.0, lines.1].into_iter().enumerate() {
        let dy = if i == 0 { -gap / 2.0 } else { gap / 2.0 };
        painter.text(
            center + vec2(0.0, dy),
            Align2::CENTER_CENTER,
            text,
            theme::seed_bold_font(size),
            LABEL,
        );
    }
}

pub fn fmt_time(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local)
        .format("%Y/%m/%d %H:%M:%S")
        .to_string()
}
