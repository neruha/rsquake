//! 予想パネル: 指定地点までの距離と、P 波・S 波の到達・未到達、進み具合のバー。
//!
//! 予報業務に当たらないよう、到達時刻や残り秒数は数字で出さない。

use chrono::{DateTime, Utc};
use egui::{Align2, Rect, Ui, pos2};

use crate::config::Location;
use crate::eew::report::HypocenterKind;
use crate::eew::store::EewEvent;
use crate::geo::distance::great_circle_km;
use crate::geo::travel_time::{TravelTimeTable, Wave};
use crate::ui::{theme, widgets};

pub struct Arrival {
    /// 震源が確定していないときは `None`
    pub distance_km: Option<f64>,
    /// 0〜1。1 で到達。計算できないときは `None` (不明)
    pub p: Option<f64>,
    pub s: Option<f64>,
}

/// 地点が未設定、または取消報のときは `None` (パネルを出さない)。
/// 仮定震源・低精度・走時表の範囲外などで計算できない項目は、`Arrival` の中で `None` (不明) にする。
pub fn compute(
    ev: &EewEvent,
    loc: Option<&Location>,
    tt: &TravelTimeTable,
    now: DateTime<Utc>,
) -> Option<Arrival> {
    let loc = loc?;
    if ev.is_canceled() {
        return None;
    }
    let unknown = Arrival {
        distance_km: None,
        p: None,
        s: None,
    };
    let Some(q) = ev.latest.quake.as_ref() else {
        return Some(unknown);
    };
    if q.kind != HypocenterKind::Normal {
        return Some(unknown);
    }
    let Some((lat, lon)) = q.position() else {
        return Some(unknown);
    };
    let distance_km = great_circle_km(lat, lon, loc.latitude, loc.longitude);
    let progress = || -> Option<(f64, f64)> {
        let depth = q.depth_km?;
        let base = q.base_time()?;
        let tp = tt.time(Wave::P, depth, distance_km)?;
        let ts = tt.time(Wave::S, depth, distance_km)?;
        let elapsed = (now - base).num_milliseconds() as f64 / 1000.0;
        let frac = |t: f64| {
            if t <= 0.0 {
                1.0
            } else {
                (elapsed / t).clamp(0.0, 1.0)
            }
        };
        Some((frac(tp), frac(ts)))
    };
    let (p, s) = match progress() {
        Some((p, s)) => (Some(p), Some(s)),
        None => (None, None),
    };
    Some(Arrival {
        distance_km: Some(distance_km),
        p,
        s,
    })
}

pub fn show(ui: &Ui, rect: Rect, ev: &EewEvent, loc: &Location, a: &Arrival) {
    let inner = widgets::card(ui, rect);
    let painter = ui.painter_at(rect);
    widgets::caption(&painter, inner.min, "到達予想");
    let title = painter.text(
        pos2(inner.min.x, inner.min.y + 18.0),
        Align2::LEFT_TOP,
        &loc.name,
        theme::bold_font(20.0),
        theme::TEXT,
    );
    let (distance, distance_color) = match a.distance_km {
        Some(d) => (format!("震央距離 {d:.0} km"), theme::TEXT),
        None => ("震央距離 不明".to_string(), theme::TEXT_MUTED),
    };
    painter.text(
        pos2(inner.max.x, title.center().y),
        Align2::RIGHT_CENTER,
        distance,
        theme::display_font(18.0),
        distance_color,
    );

    let s_color = if ev.ever_warning {
        theme::WARNING
    } else {
        theme::FORECAST
    };
    let rows = [("P波", a.p, theme::P_WAVE), ("S波", a.s, s_color)];
    let top = title.max.y + 8.0;
    let row_h = ((inner.max.y - top) / 2.0).clamp(18.0, 36.0);
    for (i, (label, f, color)) in rows.into_iter().enumerate() {
        let cy = top + row_h * (i as f32 + 0.5);
        painter.text(
            pos2(inner.min.x, cy),
            Align2::LEFT_CENTER,
            label,
            theme::bold_font(15.0),
            theme::TEXT,
        );
        let (state, state_color, bar_color) = match f {
            None => ("不明", theme::TEXT_MUTED, theme::CANCELED),
            Some(f) if f >= 1.0 => ("到達", color, color),
            Some(_) => ("未到達", theme::TEXT_MUTED, color),
        };
        painter.text(
            pos2(inner.max.x, cy),
            Align2::RIGHT_CENTER,
            state,
            theme::bold_font(15.0),
            state_color,
        );
        let bar = Rect::from_min_max(
            pos2(inner.min.x + 44.0, cy - 5.0),
            pos2(inner.max.x - 64.0, cy + 5.0),
        );
        // 不明のときはバーを灰色で満たし、進み具合がないことを区別する
        widgets::progress_bar(&painter, bar, f.unwrap_or(1.0) as f32, bar_color);
    }
}
