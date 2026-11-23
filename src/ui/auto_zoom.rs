//! 自動ズーム。有効なイベントの予報円が収まる範囲へ、地図をなめらかに寄せる。
//!
//! - 範囲は [`RECOMPUTE_EVERY`] 秒ごとに見直す。新しいイベントが来たときは待たずにすぐ見直し、
//!   手動操作で止まっていても割り込んで寄せる
//! - 目標はヒステリシス付き。予報円が今の目標に収まっているあいだは動かさず、
//!   はみ出すときだけ余裕を持たせて引く。大きく寄れるときだけ寄り直す
//! - 引きの上限は日本全域、寄りの上限は [`MAX_ZOOM`] 倍まで
//! - 手でパンやズームをしたら止め、最後の操作から [`RESUME_AFTER`] 秒後に再開する
//! - EEW が無くなったら日本全域に戻す (自動ズームが動いていたときだけ。
//!   手で動かしただけの地図は勝手に戻さない)
//!
//! 時刻は引数で受け取る (秒)。時間を進めながらテストできる。

use std::time::Duration;

use egui::Rect;

use crate::geo::distance::destination;
use crate::geo::projection::{JAPAN_BOUNDS, View, mercator};

/// 目標を見直す間隔 (秒)
pub const RECOMPUTE_EVERY: f64 = 1.0;
/// 最後の手動操作からこの秒数で自動ズームを再開する
pub const RESUME_AFTER: f64 = 10.0;
/// 日本全域の縮尺に対する寄りの上限
pub const MAX_ZOOM: f64 = 3.0;
/// 震源の周りに最低限残す半径 (km)。予報円がまだ小さいときに寄りすぎない
const MIN_RADIUS_KM: f64 = 250.0;
/// 範囲の外側に足す余白 (範囲の幅・高さに対する割合)
const PADDING: f64 = 0.1;
/// 引き直すときに、予報円の半径へ掛けて先の成長ぶんの余裕を持たせる倍率
const GROWTH: f64 = 1.15;
/// 予報円がこの割合 (画面の幅・高さに対して) より外へ出たら、今の目標には収まらないとみなす
const FIT_MARGIN: f64 = 0.04;
/// 今の目標より縮尺がこの倍率を超えて寄れるときだけ、寄り直す
const ZOOM_IN_RATIO: f64 = 1.7;
/// 動きの時定数 (秒)。小さいほど速く追いつく
const TAU: f64 = 0.4;

/// 予報円 (中心の緯度、経度、半径 km)。
pub type Circle = (f64, f64, f64);

/// `circles` がすべて収まるビュー。縮尺は日本全域〜[`MAX_ZOOM`] 倍に収める。
/// 日本全域より広くなるときは日本全域を返す。
pub fn focus_view(rect: Rect, circles: &[Circle]) -> View {
    fit_view(rect, circles, 1.0)
}

/// 半径を `growth` 倍して余裕を持たせた [`focus_view`]。
fn fit_view(rect: Rect, circles: &[Circle], growth: f64) -> View {
    let home = View::japan(rect);
    if circles.is_empty() {
        return home;
    }
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    let mut add = |lat: f64, lon: f64| {
        let m = mercator(lat, lon);
        for k in 0..2 {
            lo[k] = lo[k].min(m[k]);
            hi[k] = hi[k].max(m[k]);
        }
    };
    for &(lat, lon, r) in circles {
        add(lat, lon);
        let r = (r * growth).max(MIN_RADIUS_KM);
        for i in 0..16 {
            let (la, lo_) = destination(lat, lon, i as f64 * 22.5, r);
            add(la, lo_);
        }
    }
    let w = (hi[0] - lo[0]) * (1.0 + 2.0 * PADDING);
    let h = (hi[1] - lo[1]) * (1.0 + 2.0 * PADDING);
    let scale = (rect.width() as f64 / w.max(1e-9)).min(rect.height() as f64 / h.max(1e-9));
    if scale <= home.scale {
        return home;
    }
    let a = mercator(JAPAN_BOUNDS.0.0, JAPAN_BOUNDS.0.1);
    let b = mercator(JAPAN_BOUNDS.1.0, JAPAN_BOUNDS.1.1);
    // 中心は日本全域の外へ出さない
    let center = [
        ((lo[0] + hi[0]) / 2.0).clamp(a[0], b[0]),
        ((lo[1] + hi[1]) / 2.0).clamp(a[1], b[1]),
    ];
    View {
        center,
        scale: scale.min(home.scale * MAX_ZOOM),
    }
}

/// `view` に `circles` の予報円がすべて収まっているか。
fn fits(rect: Rect, view: &View, circles: &[Circle]) -> bool {
    let inner = rect.shrink2(egui::vec2(
        rect.width() * FIT_MARGIN as f32,
        rect.height() * FIT_MARGIN as f32,
    ));
    circles.iter().all(|&(lat, lon, r)| {
        if !inner.contains(view.latlon_to_screen(rect, lat, lon)) {
            return false;
        }
        (0..16).all(|i| {
            let (la, lo) = destination(lat, lon, i as f64 * 22.5, r);
            inner.contains(view.latlon_to_screen(rect, la, lo))
        })
    })
}

/// いまの目標 `current` と予報円から、次の目標を決める。
///
/// 予報円が増えたり大きくなったりして目標からはみ出すときは、余裕を持たせて引き直す。
/// 収まっているときは動かさず、大きく寄れるようになったときだけ寄り直す
/// (予報円が育つたびに少しずつ動くのを避ける)。
pub fn next_target(rect: Rect, current: Option<View>, circles: &[Circle]) -> Option<View> {
    if circles.is_empty() {
        return None;
    }
    let Some(cur) = current else {
        return Some(fit_view(rect, circles, GROWTH));
    };
    if !fits(rect, &cur, circles) {
        return Some(fit_view(rect, circles, GROWTH));
    }
    let tight = fit_view(rect, circles, 1.0);
    if tight.scale > cur.scale * ZOOM_IN_RATIO {
        return Some(fit_view(rect, circles, GROWTH));
    }
    Some(cur)
}

/// [`AutoZoom::step`] の結果。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Step {
    /// 動いている最中。続けて再描画する
    pub animating: bool,
    /// 手動操作のあと、いま自動ズームに戻った
    pub resumed: bool,
    /// 手動操作で止まっている間、再開までの時間
    pub resume_in: Option<Duration>,
}

#[derive(Debug, Default)]
pub struct AutoZoom {
    last_manual: Option<f64>,
    last_computed: Option<f64>,
    /// `None` は日本全域 (`engaged` のときだけ意味を持つ)
    target: Option<View>,
    /// 自動ズームが地図を動かしている (戻し終わるまで真)
    engaged: bool,
    /// 手動操作で止まっていた
    was_paused: bool,
    last_step: Option<f64>,
}

impl AutoZoom {
    /// パン、ズーム、ダブルクリックなど、手で地図を動かした。
    pub fn manual_input(&mut self, t: f64) {
        self.last_manual = Some(t);
        // 再開したらすぐ目標を計算し直す
        self.last_computed = None;
        self.last_step = None;
    }

    pub fn paused(&self, t: f64) -> bool {
        self.last_manual.is_some_and(|m| t - m < RESUME_AFTER)
    }

    /// 窓の大きさが変わったなど、目標を作り直したい。
    pub fn invalidate(&mut self) {
        self.last_computed = None;
        self.target = None;
    }

    /// いまの目標。`None` は日本全域 (またはまだ目標なし)。
    pub fn target(&self) -> Option<View> {
        self.target
    }

    /// 新しいイベントが来た。手動操作の停止を解いて、すぐ目標を作り直す。
    pub fn interrupt(&mut self) {
        self.last_manual = None;
        self.last_computed = None;
        self.last_step = None;
    }

    /// 目標を見直す時刻か。手動操作で止まっている間は計算しない。
    pub fn due(&self, t: f64) -> bool {
        !self.paused(t) && self.last_computed.is_none_or(|c| t - c >= RECOMPUTE_EVERY)
    }

    /// 計算した目標を受け取る。`None` はイベントが無いこと。
    pub fn set_target(&mut self, t: f64, target: Option<View>) {
        self.last_computed = Some(t);
        match target {
            Some(v) => {
                self.target = Some(v);
                self.engaged = true;
            }
            None => self.target = None,
        }
    }

    /// `view` を目標へ少し動かす。
    pub fn step(&mut self, t: f64, rect: Rect, view: &mut View) -> Step {
        if self.paused(t) {
            self.was_paused = true;
            self.last_step = None;
            let left = RESUME_AFTER - (t - self.last_manual.unwrap_or(t));
            return Step {
                resume_in: self
                    .engaged
                    .then(|| Duration::from_secs_f64(left.max(0.0) + 0.02)),
                ..Step::default()
            };
        }
        let resumed = std::mem::take(&mut self.was_paused) && self.engaged;
        if !self.engaged {
            self.last_step = None;
            return Step::default();
        }
        let goal = self.target.unwrap_or_else(|| View::japan(rect));
        // 休んでいたあとの最初の 1 フレームは、時間が飛んでいるので決まった刻みにする
        let dt = self
            .last_step
            .map_or(1.0 / 60.0, |p| (t - p).clamp(0.0, 0.1));
        self.last_step = Some(t);
        let k = 1.0 - (-dt / TAU).exp();

        let dx = (goal.center[0] - view.center[0]) * view.scale;
        let dy = (goal.center[1] - view.center[1]) * view.scale;
        let ds = (goal.scale / view.scale).ln();
        if dx.abs() < 0.5 && dy.abs() < 0.5 && ds.abs() < 1e-3 {
            *view = goal;
            if self.target.is_none() {
                // 日本全域に戻り終えた
                self.engaged = false;
            }
            self.last_step = None;
            return Step {
                resumed,
                ..Step::default()
            };
        }
        view.center[0] += (goal.center[0] - view.center[0]) * k;
        view.center[1] += (goal.center[1] - view.center[1]) * k;
        view.scale *= (ds * k).exp();
        Step {
            animating: true,
            resumed,
            resume_in: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn rect() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(860.0, 860.0))
    }

    #[test]
    fn small_circle_zooms_in_on_the_epicenter() {
        let home = View::japan(rect());
        let v = focus_view(rect(), &[(35.0, 139.0, 20.0)]);
        assert!(v.scale > home.scale * 2.0, "{} vs {}", v.scale, home.scale);
        assert!(v.scale <= home.scale * MAX_ZOOM + 1e-6);
        let m = mercator(35.0, 139.0);
        // 中心は円の外接矩形の中心 (メルカトル上で南北が非対称なぶん、数 km ずれる)
        assert!((v.center[0] - m[0]).abs() < 1e-3 && (v.center[1] - m[1]).abs() < 1e-3);
    }

    #[test]
    fn zoom_in_is_capped() {
        let home = View::japan(rect());
        let v = focus_view(rect(), &[(35.0, 139.0, 0.0)]);
        assert!(v.scale <= home.scale * MAX_ZOOM + 1e-6);
    }

    #[test]
    fn growing_circle_zooms_out_and_everything_fits() {
        let r = rect();
        let small = focus_view(r, &[(35.0, 139.0, 150.0)]);
        let big = focus_view(r, &[(35.0, 139.0, 400.0)]);
        assert!(big.scale < small.scale);
        // 円の東西南北の端が画面に収まる
        for bearing in [0.0, 90.0, 180.0, 270.0] {
            let (la, lo) = destination(35.0, 139.0, bearing, 400.0);
            let p = big.latlon_to_screen(r, la, lo);
            assert!(r.contains(p), "{bearing}: {p:?}");
        }
    }

    #[test]
    fn huge_or_empty_falls_back_to_japan() {
        let r = rect();
        assert_eq!(focus_view(r, &[]), View::japan(r));
        assert_eq!(focus_view(r, &[(35.0, 139.0, 3000.0)]), View::japan(r));
    }

    #[test]
    fn two_events_are_both_inside() {
        let r = rect();
        let v = focus_view(r, &[(43.0, 143.0, 50.0), (33.0, 131.0, 50.0)]);
        for (la, lo) in [(43.0, 143.0), (33.0, 131.0)] {
            assert!(r.contains(v.latlon_to_screen(r, la, lo)));
        }
    }

    #[test]
    fn epicenter_outside_japan_keeps_center_inside_japan() {
        let r = rect();
        let v = focus_view(r, &[(52.0, 160.0, 10.0)]);
        let ne = mercator(JAPAN_BOUNDS.1.0, JAPAN_BOUNDS.1.1);
        assert!(v.center[0] <= ne[0] + 1e-9 && v.center[1] <= ne[1] + 1e-9);
    }

    #[test]
    fn recompute_every_second() {
        let mut a = AutoZoom::default();
        assert!(a.due(0.0));
        a.set_target(0.0, None);
        assert!(!a.due(0.9));
        assert!(a.due(1.0));
        a.invalidate();
        assert!(a.due(0.5));
    }

    #[test]
    fn target_stays_while_circle_fits() {
        let r = rect();
        let first = next_target(r, None, &[(35.0, 139.0, 10.0)]).unwrap();
        // 少し育っても、収まっていれば目標は動かない
        for radius in [20.0, 60.0, 120.0, 180.0] {
            let t = next_target(r, Some(first), &[(35.0, 139.0, radius)]).unwrap();
            assert_eq!(t, first, "radius {radius}");
        }
    }

    #[test]
    fn target_zooms_out_with_headroom_when_circle_escapes() {
        let r = rect();
        let first = next_target(r, None, &[(35.0, 139.0, 10.0)]).unwrap();
        let t = next_target(r, Some(first), &[(35.0, 139.0, 400.0)]).unwrap();
        assert!(t.scale < first.scale);
        // 余裕を持たせるので、少し大きくなってもまた引き直さない
        let again = next_target(r, Some(t), &[(35.0, 139.0, 440.0)]).unwrap();
        assert_eq!(again, t);
    }

    #[test]
    fn target_zooms_in_only_when_much_closer() {
        let r = rect();
        let wide = next_target(r, None, &[(35.0, 139.0, 900.0)]).unwrap();
        // 円が消えて小さくなっても、わずかな差では寄り直さない
        let same = next_target(r, Some(wide), &[(35.0, 139.0, 850.0)]).unwrap();
        assert_eq!(same, wide);
        let close = next_target(r, Some(wide), &[(35.0, 139.0, 10.0)]).unwrap();
        assert!(close.scale > wide.scale);
    }

    #[test]
    fn interrupt_cancels_manual_pause() {
        let mut a = AutoZoom::default();
        a.manual_input(1.0);
        assert!(a.paused(2.0) && !a.due(2.0));
        a.interrupt();
        assert!(!a.paused(2.0) && a.due(2.0));
    }

    #[test]
    fn moves_toward_the_target_and_settles() {
        let r = rect();
        let mut view = View::japan(r);
        let target = focus_view(r, &[(35.0, 139.0, 50.0)]);
        let mut a = AutoZoom::default();
        a.set_target(0.0, Some(target));
        let first = a.step(0.0, r, &mut view);
        assert!(first.animating);
        assert!(view.scale > View::japan(r).scale && view.scale < target.scale);
        let mut t = 0.0;
        for _ in 0..600 {
            t += 1.0 / 60.0;
            if !a.step(t, r, &mut view).animating {
                break;
            }
        }
        assert_eq!(view, target);
    }

    #[test]
    fn manual_input_pauses_then_resumes_after_ten_seconds() {
        let r = rect();
        let mut view = View::japan(r);
        let target = focus_view(r, &[(35.0, 139.0, 50.0)]);
        let mut a = AutoZoom::default();
        a.set_target(0.0, Some(target));
        a.manual_input(1.0);
        let before = view;
        let s = a.step(5.0, r, &mut view);
        assert_eq!(view, before, "止まっている間は動かさない");
        assert!(!s.animating && s.resume_in.is_some());
        assert!(!a.due(5.0), "止まっている間は計算もしない");
        assert!(a.paused(10.9) && !a.paused(11.0));

        assert!(a.due(11.0), "再開したらすぐ計算し直す");
        let s = a.step(11.0, r, &mut view);
        assert!(s.resumed && s.animating);
        assert_ne!(view, before);
    }

    #[test]
    fn returns_to_japan_when_events_end_then_stops() {
        let r = rect();
        let mut view = focus_view(r, &[(35.0, 139.0, 50.0)]);
        let mut a = AutoZoom::default();
        a.set_target(0.0, Some(view));
        a.step(0.0, r, &mut view);
        a.set_target(3.0, None);
        let mut t = 3.0;
        for _ in 0..1200 {
            t += 1.0 / 60.0;
            if !a.step(t, r, &mut view).animating {
                break;
            }
        }
        assert_eq!(view, View::japan(r));
        // 戻り終えたら、手で動かした地図には触らない
        let mut manual = View {
            center: [0.1, 0.1],
            scale: 1000.0,
        };
        let s = a.step(t + 1.0, r, &mut manual);
        assert_eq!(s, Step::default());
        assert_eq!(manual.scale, 1000.0);
    }

    #[test]
    fn never_engaged_without_events_leaves_manual_view_alone() {
        let r = rect();
        let mut a = AutoZoom::default();
        a.set_target(0.0, None);
        let mut v = View {
            center: [0.5, 0.5],
            scale: 777.0,
        };
        assert_eq!(a.step(1.0, r, &mut v), Step::default());
        assert_eq!(v.scale, 777.0);
    }
}
