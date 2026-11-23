//! 地図: ベースマップ、震度塗り、予報円、震源。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use egui::epaint::{Mesh, PathShape, PathStroke, Tessellator};
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

use crate::eew::report::{HypocenterKind, Shindo};
use crate::eew::store::EewEvent;
use crate::geo::distance::{EARTH_RADIUS_KM, destination};
use crate::geo::projection::{View, mercator};
use crate::geo::stations::StationList;
use crate::geo::topo::{Topo, arc_ids};
use crate::geo::travel_time::{Front, TravelTimeTable, Wave};
use crate::ui::auto_zoom::{self, AutoZoom, Circle};
use crate::ui::theme;

/// 三角形分割済みの区域 (メルカトル座標)。
struct RegionShape {
    code: Option<String>,
    vertices: Vec<[f64; 2]>,
    indices: Vec<u32>,
}

/// ズームが止まってから地図を作り直すまでの待ち時間 (秒)。
const SETTLE_SECS: f64 = 0.15;
/// キャッシュとの縮尺比がこの範囲を出たら、待たずに作り直す。
const MAX_STRETCH: f32 = 2.0;
/// 境界線の点を間引くときの最小間隔 (物理ピクセル)。これより近い点は描いても見分けがつかない。
const LINE_MIN_STEP_PX: f32 = 1.0;

/// 直前に残した点から `min_step` 未満の点を飛ばす。始点と終点は必ず残すので、
/// 隣の arc とのつなぎ目はずれない。閉じた線 (島の輪郭) が 3 点以下に潰れるときは
/// 島ごと消えてしまうので、間引かずに返す。小さな島の点数は少ないので負担にならない。
fn thin_points(pts: Vec<Pos2>, min_step: f32) -> Vec<Pos2> {
    if pts.len() <= 2 {
        return pts;
    }
    let first = pts[0];
    let last = pts[pts.len() - 1];
    let min_sq = min_step * min_step;
    let mut out = Vec::with_capacity(pts.len());
    out.push(first);
    let mut kept = first;
    for &p in &pts[1..pts.len() - 1] {
        if (p - kept).length_sq() >= min_sq {
            out.push(p);
            kept = p;
        }
    }
    // 終点のすぐ手前に残した点があると、ごく短い線分ができる。終点を優先して外す。
    if out.len() > 1 && (last - kept).length_sq() < min_sq {
        out.pop();
    }
    out.push(last);
    if first == last && out.len() < 4 {
        return pts;
    }
    out
}

const FILL: usize = 0;
/// S 波バーの高さ (ポイント)
const S_BAR_HEIGHT: f32 = 34.0;
const REGION: usize = 1;
const PREF: usize = 2;

/// 地図メッシュのキャッシュ。`base` は作った時点の画面座標で、パンとズームは
/// `base` から `shown` へ頂点を変換して表す。作り直すのは画面が `cull` を
/// はみ出すか、縮尺が離れすぎたとき、またはズームが止まったときだけ。
struct Cache {
    rect: Rect,
    view: View,
    ppp: f32,
    /// 線の切り抜きに使った範囲 (`base` の画面座標)
    cull: Rect,
    fill_key: u64,
    base: [Mesh; 3],
    shown: [Arc<Mesh>; 3],
    /// `shown` に反映済みの (矩形, ビュー)。`None` なら `base` そのまま。
    applied: Option<(Rect, View)>,
}

impl Cache {
    fn needs_rebuild(&self, rect: Rect, view: View, ppp: f32, settled: bool) -> bool {
        if self.rect.size() != rect.size() || self.ppp != ppp {
            return true;
        }
        let k = (view.scale / self.view.scale) as f32;
        if !(1.0 / MAX_STRETCH..=MAX_STRETCH).contains(&k) {
            return true;
        }
        if view.scale != self.view.scale && settled {
            return true;
        }
        // 今の画面が、作ったときの切り抜き範囲に収まっているか
        let a = view.to_screen(rect, self.view.center);
        let o = self.rect.center();
        let to_built = |p: Pos2| o + (p - a) / k;
        let visible = Rect::from_two_pos(to_built(rect.min), to_built(rect.max));
        !self.cull.contains_rect(visible)
    }

    /// `base` の頂点を今のビューの画面座標に変換して `shown` に書く。
    fn apply(&mut self, rect: Rect, view: View) {
        let k = (view.scale / self.view.scale) as f32;
        let a = view.to_screen(rect, self.view.center);
        let o = self.rect.center();
        for (base, shown) in self.base.iter().zip(self.shown.iter_mut()) {
            let shown = Arc::make_mut(shown);
            for (s, b) in shown.vertices.iter_mut().zip(&base.vertices) {
                s.pos = a + (b.pos - o) * k;
            }
        }
        self.applied = Some((rect, view));
    }
}

/// 1 イベント分の予報円の材料。報が変わるまで使い回す。
pub struct WaveRows {
    pub serial: u32,
    pub p: Option<Vec<f64>>,
    pub s: Option<Vec<f64>>,
}

/// 観測点メッシュのキャッシュ。強震モニタの画像・矩形・ビューが同じ間は使い回す。
struct StationCache {
    generation: u64,
    rect: Rect,
    view: View,
    mesh: Arc<Mesh>,
}

pub struct MapView {
    regions: Vec<RegionShape>,
    region_arcs: Vec<Vec<[f64; 2]>>,
    pref_arcs: Vec<Vec<[f64; 2]>>,
    view: Option<View>,
    /// 前のフレームの地図の矩形と、手で動かしたか
    last_rect: Option<Rect>,
    user_moved: bool,
    /// 縮尺が最後に変わった時刻 (egui の `input.time`)
    scale_changed_at: f64,
    cache: Option<Cache>,
    rows: HashMap<String, WaveRows>,
    station_cache: Option<StationCache>,
    /// 直前のフレームで、予報円などが画面上で動いた速さの最大値 (ポイント/秒)
    motion: f32,
    auto: AutoZoom,
    /// 自動ズームが把握している有効なイベント ID。増えたら待たずに寄せる
    auto_seen: HashSet<String>,
}

/// 地図に描くイベントの情報。
pub struct MapEvent<'a> {
    pub event: &'a EewEvent,
    pub primary: bool,
}

/// 強震モニタの観測点。`colors` は `list.stations` と同じ並びで、`None` は描かない。
#[derive(Clone, Copy)]
pub struct StationLayer<'a> {
    pub list: &'a StationList,
    pub colors: &'a [Option<Color32>],
    /// `colors` を入れ替えるたびに増える番号。キャッシュの鍵
    pub generation: u64,
}

/// 地図の上に重ねる観測の層。
#[derive(Clone, Copy, Default)]
pub struct Overlays<'a> {
    pub stations: Option<StationLayer<'a>>,
}

impl MapView {
    pub fn new(topo: &Topo) -> Self {
        let regions_src = &topo.objects["eew-region"];
        let prefs_src = &topo.objects["eew-pref"];
        let mut regions = Vec::new();
        for f in regions_src {
            for poly in &f.polygons {
                let mut flat: Vec<f64> = Vec::new();
                let mut holes: Vec<usize> = Vec::new();
                let mut vertices: Vec<[f64; 2]> = Vec::new();
                for (k, ring) in poly.rings.iter().enumerate() {
                    let mut pts = topo.ring(ring);
                    if pts.len() > 1 && pts.first() == pts.last() {
                        pts.pop();
                    }
                    if pts.len() < 3 {
                        continue;
                    }
                    if k > 0 {
                        holes.push(vertices.len());
                    }
                    for p in pts {
                        let m = mercator(p[1], p[0]);
                        flat.extend_from_slice(&m);
                        vertices.push(m);
                    }
                }
                if vertices.len() < 3 {
                    continue;
                }
                let indices = match earcutr::earcut(&flat, &holes, 2) {
                    Ok(ix) => ix.into_iter().map(|i| i as u32).collect(),
                    Err(e) => {
                        tracing::warn!("{} を三角形分割できない: {e:?}", f.name);
                        continue;
                    }
                };
                regions.push(RegionShape {
                    code: f.code.clone(),
                    vertices,
                    indices,
                });
            }
        }

        // 境界線は arc 単位で描く。府県の境界に当たる arc は細分区域の線から外す。
        let pref_ids = arc_ids(prefs_src);
        let region_ids: HashSet<usize> = arc_ids(regions_src);
        let to_merc = |i: usize| -> Vec<[f64; 2]> {
            topo.arcs[i].iter().map(|p| mercator(p[1], p[0])).collect()
        };
        let mut pref_list: Vec<usize> = pref_ids.iter().copied().collect();
        pref_list.sort_unstable();
        let mut region_list: Vec<usize> = region_ids.difference(&pref_ids).copied().collect();
        region_list.sort_unstable();

        Self {
            regions,
            region_arcs: region_list.into_iter().map(to_merc).collect(),
            pref_arcs: pref_list.into_iter().map(to_merc).collect(),
            view: None,
            last_rect: None,
            user_moved: false,
            scale_changed_at: f64::NEG_INFINITY,
            cache: None,
            rows: HashMap::new(),
            station_cache: None,
            motion: 0.0,
            auto: AutoZoom::default(),
            auto_seen: HashSet::new(),
        }
    }

    pub fn reset_view(&mut self) {
        self.view = None;
        self.user_moved = false;
    }

    /// 直前のフレームで、予報円などが画面上で動いた速さの最大値 (ポイント/秒)。
    /// 再描画の間隔を決めるのに使う。
    pub fn motion_px_per_sec(&self) -> f32 {
        self.motion
    }

    /// 予報円の行を、報が変わったときだけ作り直す。
    fn wave_rows(&mut self, tt: &TravelTimeTable, ev: &EewEvent) -> &WaveRows {
        let id = &ev.latest.event_id;
        let stale = self
            .rows
            .get(id)
            .is_none_or(|r| r.serial != ev.latest.serial);
        if stale {
            let depth = ev.latest.quake.as_ref().and_then(|q| q.depth_km);
            let row = |w| depth.and_then(|d| tt.row(w, d));
            self.rows.insert(
                id.clone(),
                WaveRows {
                    serial: ev.latest.serial,
                    p: row(Wave::P),
                    s: row(Wave::S),
                },
            );
        }
        &self.rows[id]
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        events: &[MapEvent<'_>],
        location: Option<(f64, f64)>,
        overlays: Overlays<'_>,
        tt: &TravelTimeTable,
        now: DateTime<Utc>,
        auto_zoom: bool,
    ) -> Rect {
        let rect = ui.max_rect();
        self.motion = 0.0;
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        // 窓の大きさが変わったら、手で動かしていなければ全域に合わせ直す。
        // 動かしていれば縮尺だけ幅に合わせる。
        if let Some(last) = self.last_rect
            && last.size() != rect.size()
        {
            self.auto.invalidate();
            if self.user_moved {
                if let Some(v) = &mut self.view {
                    v.scale *= (rect.width() / last.width().max(1.0)) as f64;
                }
            } else {
                self.view = None;
            }
        }
        self.last_rect = Some(rect);
        let mut view = *self.view.get_or_insert_with(|| View::japan(rect));
        let prev_scale = view.scale;

        // 手動のパンとズーム
        let home = View::japan(rect);
        let (min_scale, max_scale) = (home.scale * 0.6, home.scale * 40.0);
        let time = ui.input(|i| i.time);
        if response.dragged() {
            view.pan(response.drag_delta());
            self.user_moved = true;
            self.auto.manual_input(time);
        }
        if response.double_clicked() {
            view = home;
            self.user_moved = false;
            self.auto.manual_input(time);
        }
        if response.hovered() {
            let (scroll, zoom, pointer) = ui.input(|i| {
                (
                    i.smooth_scroll_delta.y,
                    i.zoom_delta(),
                    i.pointer.hover_pos(),
                )
            });
            let factor = zoom as f64 * (scroll as f64 / 300.0).exp();
            if (factor - 1.0).abs() > 1e-4 {
                let anchor = pointer.unwrap_or(rect.center());
                view.zoom_at(rect, anchor, factor, min_scale, max_scale);
                self.user_moved = true;
                self.auto.manual_input(time);
            }
        }

        // 自動ズーム: 手で動かした直後の 10 秒は止まる
        if auto_zoom {
            // 新しいイベントが来たら、手動操作の停止も待ち時間も飛ばしてすぐ寄せる
            let live: HashSet<String> = events
                .iter()
                .filter(|e| !e.event.is_canceled())
                .map(|e| e.event.latest.event_id.clone())
                .collect();
            if live.iter().any(|id| !self.auto_seen.contains(id)) {
                self.auto.interrupt();
            }
            self.auto_seen = live;
            if self.auto.due(time) {
                let circles = self.focus_circles(events, tt, now);
                let target = auto_zoom::next_target(rect, self.auto.target(), &circles);
                self.auto.set_target(time, target);
            }
            let step = self.auto.step(time, rect, &mut view);
            if step.animating {
                ui.ctx().request_repaint();
            }
            if let Some(wait) = step.resume_in {
                ui.ctx().request_repaint_after(wait);
            }
            if step.animating || step.resumed {
                // 自動で動かした地図は、窓の大きさが変わったら全域から合わせ直す
                self.user_moved = false;
            }
        } else {
            self.auto = AutoZoom::default();
            self.auto_seen.clear();
        }
        if view.scale != prev_scale {
            self.scale_changed_at = ui.input(|i| i.time);
        }
        self.view = Some(view);

        // 区域ごとの最大予測震度 (有効な全イベントの最大値。取消は除く)
        let mut fills: HashMap<&str, Shindo> = HashMap::new();
        for e in events {
            if e.event.is_canceled() {
                continue;
            }
            if let Some(i) = &e.event.latest.intensity {
                for r in &i.regions {
                    if let Some(s) = r.max_int.representative() {
                        let cur = fills.entry(r.code.as_str()).or_insert(s);
                        *cur = (*cur).max(s);
                    }
                }
            }
        }
        let fill_key = {
            use std::hash::{Hash, Hasher};
            let mut v: Vec<(&str, Shindo)> = fills.iter().map(|(k, v)| (*k, *v)).collect();
            v.sort_unstable();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            v.hash(&mut h);
            h.finish()
        };

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::SEA);
        self.update_cache(ui, rect, view, fill_key, &fills);
        let c = self.cache.as_ref().unwrap();
        for m in &c.shown {
            painter.add(Shape::Mesh(m.clone()));
        }
        if let Some(layer) = overlays.stations {
            let mesh = self.station_mesh(layer, rect, &view, home.scale);
            painter.add(Shape::Mesh(mesh));
        }

        if let Some((lat, lon)) = location {
            let p = view.latlon_to_screen(rect, lat, lon);
            painter.circle(p, 5.0, theme::TEXT, Stroke::new(2.0, theme::BG));
            painter.circle_filled(p, 2.0, theme::BG);
        }

        // 主表示のイベントを最後に (上に) 描く
        let mut order: Vec<&MapEvent<'_>> = events.iter().collect();
        order.sort_by_key(|e| e.primary);
        for e in order {
            if e.event.is_canceled() {
                continue;
            }
            self.draw_event(&painter, rect, &view, tt, e.event, now);
        }
        rect
    }

    /// 自動ズームで収める予報円。S 波と P 波のうち大きい方の半径を使う。
    /// 仮定震源など円を描かないイベントは、震源の点だけを入れる。
    fn focus_circles(
        &mut self,
        events: &[MapEvent<'_>],
        tt: &TravelTimeTable,
        now: DateTime<Utc>,
    ) -> Vec<Circle> {
        let mut out = Vec::new();
        for e in events {
            if e.event.is_canceled() {
                continue;
            }
            let Some(q) = e.event.latest.quake.clone() else {
                continue;
            };
            let Some((lat, lon)) = q.position() else {
                continue;
            };
            let mut radius = 0.0_f64;
            if q.kind != HypocenterKind::Assumed
                && let Some(base) = q.base_time()
            {
                let elapsed = (now - base).num_milliseconds() as f64 / 1000.0;
                let rows = self.wave_rows(tt, e.event);
                for row in [&rows.p, &rows.s].into_iter().flatten() {
                    if let Front::Radius(r) = tt.front(row, elapsed) {
                        radius = radius.max(r);
                    }
                }
            }
            out.push((lat, lon, radius));
        }
        out
    }

    fn update_cache(
        &mut self,
        ui: &Ui,
        rect: Rect,
        view: View,
        fill_key: u64,
        fills: &HashMap<&str, Shindo>,
    ) {
        let ppp = ui.ctx().pixels_per_point();
        let now = ui.input(|i| i.time);
        let since_zoom = now - self.scale_changed_at;
        let rebuild = match &self.cache {
            None => true,
            Some(c) => c.needs_rebuild(rect, view, ppp, since_zoom >= SETTLE_SECS),
        };

        if !rebuild {
            let c = self.cache.as_mut().unwrap();
            if c.fill_key != fill_key {
                // 震度の塗り替えは塗りだけ作り直す。線は作ったときのままでよい。
                c.base[FILL] = build_fill(&self.regions, c.rect, c.view, fills);
                c.shown[FILL] = Arc::new(c.base[FILL].clone());
                c.fill_key = fill_key;
                c.applied = None;
            }
            if c.applied != Some((rect, view)) {
                c.apply(rect, view);
            }
            if view.scale != c.view.scale {
                // ズームが止まるのを待つ間に、作り直しの時刻へ再描画を呼ぶ
                let wait = (SETTLE_SECS - since_zoom).max(0.0) + 0.01;
                ui.ctx()
                    .request_repaint_after(Duration::from_secs_f64(wait));
            }
            return;
        }

        let fill = build_fill(&self.regions, rect, view, fills);

        let tex = ui.ctx().fonts(|f| f.font_image_size());
        let options = ui.ctx().tessellation_options(|o| *o);
        let mut tess = Tessellator::new(ppp, options, tex, Vec::new());
        // 画面の外にも半画面ぶん余白を取って切り抜く。パンやズームアウトで
        // 縁が見えてくるまでは作り直さずに済む。
        let cull = rect.expand2(rect.size() * 0.5);
        let mut lines = |arcs: &[Vec<[f64; 2]>], stroke: Stroke| {
            let mut mesh = Mesh::default();
            let min_step = LINE_MIN_STEP_PX / ppp;
            for arc in arcs {
                let pts: Vec<Pos2> = arc.iter().map(|m| view.to_screen(rect, *m)).collect();
                if !Rect::from_points(&pts).intersects(cull) {
                    continue;
                }
                let path = PathShape::line(thin_points(pts, min_step), PathStroke::from(stroke));
                tess.tessellate_path(&path, &mut mesh);
            }
            mesh
        };
        let region_lines = lines(&self.region_arcs, Stroke::new(0.6, theme::REGION_LINE));
        let pref_lines = lines(&self.pref_arcs, Stroke::new(1.1, theme::PREF_LINE));

        let base = [fill, region_lines, pref_lines];
        let shown = [
            Arc::new(base[FILL].clone()),
            Arc::new(base[REGION].clone()),
            Arc::new(base[PREF].clone()),
        ];
        self.cache = Some(Cache {
            rect,
            view,
            ppp,
            cull,
            fill_key,
            base,
            shown,
            applied: Some((rect, view)),
        });
    }

    /// 観測点のメッシュ。画像・矩形・ビューが前回と同じなら作り直さない。
    fn station_mesh(
        &mut self,
        layer: StationLayer<'_>,
        rect: Rect,
        view: &View,
        home_scale: f64,
    ) -> Arc<Mesh> {
        if let Some(c) = &self.station_cache
            && c.generation == layer.generation
            && c.rect == rect
            && c.view == *view
        {
            return c.mesh.clone();
        }
        let mesh = Arc::new(build_station_mesh(layer, rect, view, home_scale));
        self.station_cache = Some(StationCache {
            generation: layer.generation,
            rect,
            view: *view,
            mesh: mesh.clone(),
        });
        mesh
    }

    fn draw_event(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        view: &View,
        tt: &TravelTimeTable,
        ev: &EewEvent,
        now: DateTime<Utc>,
    ) {
        let Some(q) = ev.latest.quake.clone() else {
            return;
        };
        let Some((lat, lon)) = q.position() else {
            return;
        };
        let epicenter = view.latlon_to_screen(rect, lat, lon);
        let s_color = if ev.ever_warning {
            theme::WARNING
        } else {
            theme::FORECAST
        };

        if q.kind == HypocenterKind::Assumed {
            draw_assumed_mark(painter, epicenter, s_color);
            return;
        }

        let dashed = q.kind == HypocenterKind::LowAccuracy;
        let elapsed = q
            .base_time()
            .map(|t| (now - t).num_milliseconds() as f64 / 1000.0);
        let rows = self.wave_rows(tt, ev);
        let mut s_progress = None;
        let mut motion = 0.0_f32;
        if let Some(elapsed) = elapsed {
            // 波面が画面上で動く速さ (ポイント/秒)。1 秒先との差から求める。
            let px_per_km =
                (view.scale / (EARTH_RADIUS_KM * lat.to_radians().cos().max(0.05))) as f32;
            let front_speed =
                |row: &[f64]| match (tt.front(row, elapsed), tt.front(row, elapsed + 1.0)) {
                    (Front::Radius(a), Front::Radius(b)) => (b - a).max(0.0) as f32 * px_per_km,
                    _ => 0.0,
                };
            if let Some(row) = &rows.p
                && let Front::Radius(r) = tt.front(row, elapsed)
            {
                motion = motion.max(front_speed(row));
                let pts = circle_points(view, rect, lat, lon, r);
                stroke_ring(painter, pts, Stroke::new(1.4, theme::P_WAVE), dashed);
            }
            if let Some(row) = &rows.s {
                match tt.front(row, elapsed) {
                    Front::Radius(r) => {
                        motion = motion.max(front_speed(row));
                        let pts = circle_points(view, rect, lat, lon, r);
                        if !dashed {
                            painter.add(Shape::convex_polygon(
                                pts.clone(),
                                s_color.gamma_multiply(0.06),
                                Stroke::NONE,
                            ));
                        }
                        stroke_ring(painter, pts, Stroke::new(2.4, s_color), dashed);
                    }
                    Front::Underground(f) => {
                        s_progress = Some(f);
                        // バーは S 波が地表に届くまでの間に 34 ポイント伸びる
                        if row[0] > 0.0 {
                            motion = motion.max(S_BAR_HEIGHT / row[0] as f32);
                        }
                    }
                }
            }
        }
        self.motion = self.motion.max(motion);

        draw_epicenter(painter, epicenter);
        if let Some(f) = s_progress {
            draw_s_bar(painter, epicenter, f as f32, s_color);
        }
    }

    pub fn forget_rows(&mut self, keep: &HashSet<&str>) {
        self.rows.retain(|k, _| keep.contains(k.as_str()));
    }
}

/// 観測点を 1 枚のメッシュにまとめる。色は強震モニタの画素の色をそのまま使う。
/// 画面の外の観測点は作らない。
fn build_station_mesh(layer: StationLayer<'_>, rect: Rect, view: &View, home_scale: f64) -> Mesh {
    let half = ((view.scale / home_scale).sqrt() as f32 * 1.7).clamp(1.7, 6.0);
    let visible = rect.expand(half);
    let unit = unit_disc(half);
    let mut mesh = Mesh::default();
    for (s, color) in layer.list.stations.iter().zip(layer.colors) {
        let Some(color) = color else { continue };
        let p = view.to_screen(rect, s.merc);
        if !visible.contains(p) {
            continue;
        }
        add_colored_disc(&mut mesh, p, half, *color, unit);
    }
    mesh
}

/// 観測点の丸の外周 (半径 1 の単位円)。半径が小さいときは頂点を減らす。
fn unit_disc(radius: f32) -> &'static [Vec2] {
    static TABLES: OnceLock<[Vec<Vec2>; 2]> = OnceLock::new();
    let tables = TABLES.get_or_init(|| {
        let ring = |n: u32| {
            (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    vec2(a.cos(), a.sin())
                })
                .collect::<Vec<_>>()
        };
        [ring(8), ring(12)]
    });
    if radius < 3.0 { &tables[0] } else { &tables[1] }
}

/// 中心と外周の頂点でつくる三角形の扇形で、塗りつぶしの丸を足す。`unit` は [`unit_disc`] の外周。
fn add_colored_disc(mesh: &mut Mesh, center: Pos2, radius: f32, color: Color32, unit: &[Vec2]) {
    let segments = unit.len() as u32;
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(center, color);
    for u in unit {
        mesh.colored_vertex(center + *u * radius, color);
    }
    for i in 0..segments {
        mesh.add_triangle(base, base + 1 + i, base + 1 + (i + 1) % segments);
    }
}

/// 区域の塗りメッシュを画面座標で作る。
fn build_fill(
    regions: &[RegionShape],
    rect: Rect,
    view: View,
    fills: &HashMap<&str, Shindo>,
) -> Mesh {
    let mut fill = Mesh::default();
    for r in regions {
        let color = r
            .code
            .as_deref()
            .and_then(|code| fills.get(code))
            .map(|s| theme::shindo_color(*s))
            .unwrap_or(theme::LAND);
        let base = fill.vertices.len() as u32;
        for v in &r.vertices {
            fill.colored_vertex(view.to_screen(rect, *v), color);
        }
        fill.indices.extend(r.indices.iter().map(|i| base + i));
    }
    fill
}

fn circle_points(view: &View, rect: Rect, lat: f64, lon: f64, radius_km: f64) -> Vec<Pos2> {
    const N: usize = 96;
    (0..N)
        .map(|i| {
            let (la, lo) = destination(lat, lon, i as f64 * 360.0 / N as f64, radius_km);
            view.latlon_to_screen(rect, la, lo)
        })
        .collect()
}

fn stroke_ring(painter: &egui::Painter, mut pts: Vec<Pos2>, stroke: Stroke, dashed: bool) {
    if dashed {
        pts.push(pts[0]);
        painter.extend(Shape::dashed_line(&pts, stroke, 8.0, 6.0));
    } else {
        painter.add(Shape::closed_line(pts, stroke));
    }
}

fn draw_epicenter(painter: &egui::Painter, p: Pos2) {
    let r = 9.0;
    for (w, c) in [(5.5, Color32::WHITE), (3.0, theme::WARNING)] {
        let s = Stroke::new(w, c);
        painter.line_segment([p + vec2(-r, -r), p + vec2(r, r)], s);
        painter.line_segment([p + vec2(-r, r), p + vec2(r, -r)], s);
    }
}

/// 仮定震源: 震源の位置が仮のものとわかるよう、×ではなく中空の菱形にする。
fn draw_assumed_mark(painter: &egui::Painter, p: Pos2, color: Color32) {
    let r = 10.0;
    let pts = vec![
        p + vec2(0.0, -r),
        p + vec2(r, 0.0),
        p + vec2(0.0, r),
        p + vec2(-r, 0.0),
    ];
    painter.add(Shape::closed_line(
        pts.clone(),
        Stroke::new(5.0, Color32::WHITE),
    ));
    painter.add(Shape::closed_line(pts, Stroke::new(2.5, color)));
    painter.circle_filled(p, 2.5, color);
}

/// S 波が地表に届くまでの進み具合を、震源の右に縦のバーで出す。
fn draw_s_bar(painter: &egui::Painter, epicenter: Pos2, f: f32, color: Color32) {
    let size = vec2(7.0, S_BAR_HEIGHT);
    let outer = Rect::from_min_size(epicenter + vec2(16.0, -size.y / 2.0), size);
    painter.rect_filled(outer.expand(1.5), 3.0, theme::BG);
    painter.rect_filled(outer, 2.0, theme::PANEL_RAISED);
    let h = size.y * f.clamp(0.0, 1.0);
    let filled = Rect::from_min_max(pos2(outer.min.x, outer.max.y - h), outer.max);
    painter.rect_filled(filled, 2.0, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_for(rect: Rect, view: View) -> Cache {
        Cache {
            rect,
            view,
            ppp: 1.0,
            cull: rect.expand2(rect.size() * 0.5),
            fill_key: 0,
            base: Default::default(),
            shown: Default::default(),
            applied: None,
        }
    }

    fn setup() -> (Rect, View, Cache) {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let view = View::japan(rect);
        (rect, view, cache_for(rect, view))
    }

    #[test]
    fn unchanged_view_keeps_cache() {
        let (rect, view, c) = setup();
        assert!(!c.needs_rebuild(rect, view, 1.0, true));
    }

    #[test]
    fn small_pan_keeps_cache_and_large_pan_rebuilds() {
        let (rect, view, c) = setup();
        let mut v = view;
        v.pan(vec2(200.0, 100.0));
        assert!(!c.needs_rebuild(rect, v, 1.0, true));
        v.pan(vec2(400.0, 0.0));
        assert!(c.needs_rebuild(rect, v, 1.0, true));
    }

    #[test]
    fn zoom_waits_until_settled() {
        let (rect, view, c) = setup();
        let mut v = view;
        v.zoom_at(rect, rect.center(), 1.3, 0.0, 1e9);
        assert!(!c.needs_rebuild(rect, v, 1.0, false));
        assert!(c.needs_rebuild(rect, v, 1.0, true));
    }

    #[test]
    fn extreme_zoom_rebuilds_immediately() {
        let (rect, view, c) = setup();
        let mut v = view;
        v.zoom_at(rect, rect.center(), 2.5, 0.0, 1e9);
        assert!(c.needs_rebuild(rect, v, 1.0, false));
    }

    #[test]
    fn zoom_out_within_stretch_keeps_cache_while_moving() {
        let (rect, view, c) = setup();
        let mut v = view;
        v.zoom_at(rect, rect.center(), 0.6, 0.0, 1e9);
        assert!(!c.needs_rebuild(rect, v, 1.0, false));
        v.zoom_at(rect, rect.center(), 0.7, 0.0, 1e9);
        assert!(c.needs_rebuild(rect, v, 1.0, false));
    }

    #[test]
    fn resize_or_dpi_change_rebuilds() {
        let (rect, view, c) = setup();
        let bigger = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 600.0));
        assert!(c.needs_rebuild(bigger, view, 1.0, false));
        assert!(c.needs_rebuild(rect, view, 1.5, false));
    }

    #[test]
    fn apply_transforms_vertices() {
        let (rect, view, mut c) = setup();
        let mut mesh = Mesh::default();
        let m = mercator(35.0, 139.0);
        mesh.colored_vertex(view.to_screen(rect, m), Color32::WHITE);
        c.base[FILL] = mesh.clone();
        c.shown[FILL] = Arc::new(mesh);
        let mut v = view;
        v.pan(vec2(30.0, -20.0));
        v.zoom_at(rect, pos2(200.0, 150.0), 1.4, 0.0, 1e9);
        c.apply(rect, v);
        let want = v.to_screen(rect, m);
        assert!((c.shown[FILL].vertices[0].pos - want).length() < 0.05);
    }

    #[test]
    fn thin_points_keeps_ends_and_drops_close_points() {
        let pts = vec![
            pos2(0.0, 0.0),
            pos2(0.3, 0.0),
            pos2(0.6, 0.0),
            pos2(1.2, 0.0),
            pos2(1.5, 0.0),
            pos2(1.8, 0.0),
            pos2(5.0, 0.0),
        ];
        let out = thin_points(pts.clone(), 1.0);
        assert_eq!(out.first(), pts.first());
        assert_eq!(out.last(), pts.last());
        assert_eq!(out, vec![pos2(0.0, 0.0), pos2(1.2, 0.0), pos2(5.0, 0.0)]);
    }

    #[test]
    fn thin_points_collapses_tiny_arc_to_its_ends() {
        let pts = vec![
            pos2(0.0, 0.0),
            pos2(0.2, 0.0),
            pos2(0.4, 0.0),
            pos2(0.5, 0.0),
        ];
        assert_eq!(thin_points(pts, 1.0), vec![pos2(0.0, 0.0), pos2(0.5, 0.0)]);
        let two = vec![pos2(0.0, 0.0), pos2(0.1, 0.0)];
        assert_eq!(thin_points(two.clone(), 1.0), two);
    }

    #[test]
    fn thin_points_keeps_tiny_closed_ring() {
        let ring = vec![
            pos2(0.0, 0.0),
            pos2(0.3, 0.0),
            pos2(0.3, 0.3),
            pos2(0.0, 0.3),
            pos2(0.0, 0.0),
        ];
        assert_eq!(thin_points(ring.clone(), 1.0), ring);
    }
}
