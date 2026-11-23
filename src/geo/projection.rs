//! メルカトル投影と、地図のビュー（中心と縮尺）。

use egui::{Pos2, Rect, Vec2, pos2};

/// 経緯度 (度) をメルカトル座標 (ラジアン単位、y は北が正) にする。
pub fn mercator(lat: f64, lon: f64) -> [f64; 2] {
    let lat = lat.clamp(-85.0, 85.0).to_radians();
    [
        lon.to_radians(),
        (std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().ln(),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// 画面中心のメルカトル座標
    pub center: [f64; 2],
    /// 1 メルカトル単位あたりの画面ポイント数
    pub scale: f64,
}

/// 日本全域 (南西端と北東端)。
pub const JAPAN_BOUNDS: ((f64, f64), (f64, f64)) = ((23.5, 122.5), (46.0, 149.0));

impl View {
    pub fn fit(rect: Rect, sw: (f64, f64), ne: (f64, f64)) -> Self {
        let a = mercator(sw.0, sw.1);
        let b = mercator(ne.0, ne.1);
        let w = (b[0] - a[0]).max(1e-9);
        let h = (b[1] - a[1]).max(1e-9);
        let scale = (rect.width() as f64 / w).min(rect.height() as f64 / h);
        Self {
            center: [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0],
            scale,
        }
    }

    pub fn japan(rect: Rect) -> Self {
        Self::fit(rect, JAPAN_BOUNDS.0, JAPAN_BOUNDS.1)
    }

    #[inline]
    pub fn to_screen(&self, rect: Rect, m: [f64; 2]) -> Pos2 {
        let c = rect.center();
        pos2(
            c.x + ((m[0] - self.center[0]) * self.scale) as f32,
            c.y - ((m[1] - self.center[1]) * self.scale) as f32,
        )
    }

    pub fn latlon_to_screen(&self, rect: Rect, lat: f64, lon: f64) -> Pos2 {
        self.to_screen(rect, mercator(lat, lon))
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.center[0] -= delta.x as f64 / self.scale;
        self.center[1] += delta.y as f64 / self.scale;
    }

    /// `anchor` (画面座標) の地点を動かさずに拡大縮小する。
    pub fn zoom_at(
        &mut self,
        rect: Rect,
        anchor: Pos2,
        factor: f64,
        min_scale: f64,
        max_scale: f64,
    ) {
        let new_scale = (self.scale * factor).clamp(min_scale, max_scale);
        let c = rect.center();
        let dx = (anchor.x - c.x) as f64;
        let dy = (anchor.y - c.y) as f64;
        // 拡大前後で anchor の下にあるメルカトル座標を一致させる
        self.center[0] += dx / self.scale - dx / new_scale;
        self.center[1] -= dy / self.scale - dy / new_scale;
        self.scale = new_scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_anchor() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let mut v = View::japan(rect);
        let m = mercator(35.0, 139.0);
        let before = v.to_screen(rect, m);
        v.zoom_at(rect, before, 2.0, 1.0, 1e6);
        let after = v.to_screen(rect, m);
        assert!((before - after).length() < 0.01);
    }
}
