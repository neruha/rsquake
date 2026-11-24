//! 地図左上の時刻と接続状態。通常時は目立たせず、異常時だけ文字で出す。

use chrono::{DateTime, Local, Utc};
use egui::{Color32, Rect, Ui, pos2, vec2};

use crate::config::EewSource;
use crate::eew::report::Upstream;
use crate::net::LinkState;
use crate::ui::theme;

pub struct Status {
    pub link: LinkState,
    pub upstream: Upstream,
    pub source: EewSource,
    pub test_mode: bool,
    pub replay: bool,
    /// 仮想イベント (デバッグメニュー) を表示している
    pub debug_events: bool,
    pub kmoni: KmoniStatus,
}

/// 左上の時刻の元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KmoniStatus {
    /// 強震モニタを使わない。パソコンの時計を出す
    Disabled,
    /// 最新の画像を取れている。画像の時刻を出す
    Live(DateTime<Utc>),
    /// まだ取れていない、または取れなくなった。最後に取れた画像の時刻で止めて赤にする
    Stale(Option<DateTime<Utc>>),
}

pub fn show(ui: &Ui, map: Rect, now: DateTime<Utc>, s: &Status) {
    let painter = ui.painter_at(map);
    let origin = map.min + vec2(12.0, 12.0);
    let (shown, time_color) = match s.kmoni {
        KmoniStatus::Live(t) => (Some(t), theme::TEXT),
        KmoniStatus::Disabled => (Some(now), theme::TEXT),
        KmoniStatus::Stale(t) => (t, theme::WARNING),
    };
    let time = shown.map_or_else(
        || "----/--/-- --:--:--".to_string(),
        |t| {
            t.with_timezone(&Local)
                .format("%Y/%m/%d %H:%M:%S")
                .to_string()
        },
    );
    let galley = painter.layout_no_wrap(time, theme::display_font(20.0), time_color);
    let pad = vec2(12.0, 8.0);
    let dot_w = 18.0;

    let (problem, dot) = problem_text(s.source, s.link, s.upstream);
    let mut tags: Vec<(&str, Color32, Color32)> = Vec::new();
    if s.replay {
        tags.push(("リプレイ", theme::TRAINING, theme::BG));
    } else if s.test_mode {
        tags.push(("テスト", theme::TRAINING, theme::BG));
    }
    if s.debug_events {
        tags.push(("DEBUG", theme::TRAINING, theme::BG));
    }
    if let Some(p) = problem {
        tags.push((
            p,
            dot,
            if dot == theme::FORECAST {
                theme::ON_FORECAST
            } else {
                Color32::WHITE
            },
        ));
    }

    let size = vec2(
        galley.size().x + dot_w + pad.x * 2.0,
        galley.size().y + pad.y * 2.0,
    );
    let rect = Rect::from_min_size(origin, size);
    painter.rect_filled(rect, theme::RADIUS, theme::PANEL.gamma_multiply(0.92));
    let dot_pos = pos2(rect.min.x + pad.x + 5.0, rect.center().y);
    painter.circle_filled(dot_pos, 5.0, dot);
    painter.galley(
        pos2(rect.min.x + pad.x + dot_w, rect.min.y + pad.y),
        galley,
        time_color,
    );

    let mut x = rect.max.x + 8.0;
    for (text, fill, color) in tags {
        let g = painter.layout_no_wrap(text.to_string(), theme::bold_font(14.0), color);
        let r = Rect::from_min_size(pos2(x, rect.min.y), vec2(g.size().x + 20.0, rect.height()));
        painter.rect_filled(r, theme::RADIUS, fill);
        painter.galley(r.center() - g.size() / 2.0, g, color);
        x = r.max.x + 8.0;
    }
}

/// 異常時の文言と点の色。通常時は `None`。Wolfx のときだけ受信元の名前を入れる。
fn problem_text(
    source: EewSource,
    link: LinkState,
    upstream: Upstream,
) -> (Option<&'static str>, Color32) {
    let wolfx = source == EewSource::Wolfx;
    match (link, upstream) {
        (LinkState::Disconnected, _) => (
            Some(if wolfx {
                "Wolfx 接続不可"
            } else {
                "EEW 接続不可"
            }),
            theme::WARNING,
        ),
        (LinkState::Connecting, _) => (
            Some(if wolfx {
                "Wolfx 接続中"
            } else {
                "EEW 接続中"
            }),
            theme::FORECAST,
        ),
        (LinkState::Connected, Upstream::Disconnected) => {
            (Some("EEW 接続不可（上流）"), theme::WARNING)
        }
        (LinkState::Connected, _) => (None, theme::OK),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_text_names_wolfx_only() {
        let (t, _) = problem_text(EewSource::Wolfx, LinkState::Disconnected, Upstream::Unknown);
        assert_eq!(t, Some("Wolfx 接続不可"));
        let (t, _) = problem_text(
            EewSource::Dmdata,
            LinkState::Disconnected,
            Upstream::Unknown,
        );
        assert_eq!(t, Some("EEW 接続不可"));
        let (t, _) = problem_text(EewSource::Wolfx, LinkState::Connected, Upstream::Unknown);
        assert_eq!(t, None);
    }
}
