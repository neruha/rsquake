//! JMA2001 走時表。深さ × 震央距離の格子に展開し、線形補間で引く。
//! 表の最大距離を超える分は、各深さの最後の 2 点の傾きで線形に外挿する。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    P,
    S,
}

pub struct TravelTimeTable {
    depths: Vec<f64>,
    distances: Vec<f64>,
    /// `[depth_index * distances.len() + distance_index]`
    p: Vec<f64>,
    s: Vec<f64>,
}

/// 波面の状態。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Front {
    /// まだ地表に届いていない。値は真上に届くまでの進み具合 (0〜1)。
    Underground(f64),
    /// 地表での震央距離 (km)。
    Radius(f64),
}

impl TravelTimeTable {
    pub fn bundled() -> Self {
        Self::parse(include_str!("../../assets/tjma2001.csv")).expect("同梱の走時表が読めない")
    }

    pub fn parse(csv: &str) -> anyhow::Result<Self> {
        let mut rows: Vec<(f64, f64, f64, f64)> = Vec::new();
        for (n, line) in csv.lines().enumerate().skip(1) {
            if line.trim().is_empty() {
                continue;
            }
            let c: Vec<&str> = line.split(',').map(str::trim).collect();
            anyhow::ensure!(c.len() >= 6, "{} 行目の列が足りない", n + 1);
            rows.push((c[4].parse()?, c[5].parse()?, c[1].parse()?, c[3].parse()?));
        }
        let mut depths: Vec<f64> = rows.iter().map(|r| r.0).collect();
        let mut distances: Vec<f64> = rows.iter().map(|r| r.1).collect();
        for v in [&mut depths, &mut distances] {
            v.sort_by(f64::total_cmp);
            v.dedup();
        }
        let nd = distances.len();
        let mut p = vec![f64::NAN; depths.len() * nd];
        let mut s = p.clone();
        for (dep, dist, pt, st) in rows {
            let i = depths.binary_search_by(|v| v.total_cmp(&dep)).unwrap();
            let j = distances.binary_search_by(|v| v.total_cmp(&dist)).unwrap();
            p[i * nd + j] = pt;
            s[i * nd + j] = st;
        }
        anyhow::ensure!(p.iter().all(|v| v.is_finite()), "走時表の格子に欠けがある");
        Ok(Self {
            depths,
            distances,
            p,
            s,
        })
    }

    pub fn max_depth(&self) -> f64 {
        *self.depths.last().unwrap()
    }

    pub fn max_distance(&self) -> f64 {
        *self.distances.last().unwrap()
    }

    fn grid(&self, wave: Wave) -> &[f64] {
        match wave {
            Wave::P => &self.p,
            Wave::S => &self.s,
        }
    }

    /// 深さ `depth` の行 (距離ごとの走時) を補間で作る。範囲外なら `None`。
    pub fn row(&self, wave: Wave, depth: f64) -> Option<Vec<f64>> {
        let (i, t) = locate(&self.depths, depth)?;
        let nd = self.distances.len();
        let g = self.grid(wave);
        let a = &g[i * nd..(i + 1) * nd];
        if t == 0.0 {
            return Some(a.to_vec());
        }
        let b = &g[(i + 1) * nd..(i + 2) * nd];
        Some(a.iter().zip(b).map(|(x, y)| x + (y - x) * t).collect())
    }

    /// 走時 (秒)。深さと距離の双線形補間。範囲外なら `None`。
    pub fn time(&self, wave: Wave, depth: f64, distance: f64) -> Option<f64> {
        let row = self.row(wave, depth)?;
        if !distance.is_finite() || distance < self.distances[0] {
            return None;
        }
        if distance > self.max_distance() {
            let (slope, last) = self.tail_slope(&row);
            return Some(last + slope * (distance - self.max_distance()));
        }
        let (j, t) = locate(&self.distances, distance)?;
        Some(if t == 0.0 {
            row[j]
        } else {
            row[j] + (row[j + 1] - row[j]) * t
        })
    }

    /// 行の末尾 2 点から求めた走時の傾き (秒/km) と、最後の点の走時。
    fn tail_slope(&self, row: &[f64]) -> (f64, f64) {
        let n = row.len();
        let dd = self.distances[n - 1] - self.distances[n - 2];
        ((row[n - 1] - row[n - 2]) / dd, row[n - 1])
    }

    /// 経過時間 `elapsed` 秒での波面。`row` は [`Self::row`] の結果を使い回す。
    pub fn front(&self, row: &[f64], elapsed: f64) -> Front {
        let first = row[0];
        if elapsed < first {
            return Front::Underground(if first > 0.0 {
                (elapsed / first).max(0.0)
            } else {
                1.0
            });
        }
        let last = *row.last().unwrap();
        if elapsed > last {
            // 表の外側は最後の 2 点の傾きで外挿する
            let (slope, _) = self.tail_slope(row);
            let extra = if slope > 0.0 {
                (elapsed - last) / slope
            } else {
                0.0
            };
            return Front::Radius(self.max_distance() + extra);
        }
        // 走時は距離に対して単調に増えるので二分探索できる
        let k = row.partition_point(|&v| v <= elapsed);
        if k >= row.len() {
            return Front::Radius(self.max_distance());
        }
        let (t0, t1) = (row[k - 1], row[k]);
        let (d0, d1) = (self.distances[k - 1], self.distances[k]);
        let f = if t1 > t0 {
            (elapsed - t0) / (t1 - t0)
        } else {
            0.0
        };
        Front::Radius(d0 + (d1 - d0) * f)
    }
}

/// 昇順の格子 `axis` で `x` を挟む区間の番号と、区間内の位置 (0〜1)。範囲外は `None`。
fn locate(axis: &[f64], x: f64) -> Option<(usize, f64)> {
    if !x.is_finite() || x < axis[0] || x > *axis.last()? {
        return None;
    }
    let k = axis.partition_point(|&v| v <= x);
    if k >= axis.len() {
        return Some((axis.len() - 1, 0.0));
    }
    let i = k - 1;
    Some((i, (x - axis[i]) / (axis[i + 1] - axis[i])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TravelTimeTable {
        TravelTimeTable::bundled()
    }

    #[test]
    fn grid_shape() {
        let t = table();
        assert_eq!(t.depths.len(), 106);
        assert_eq!(t.distances.len(), 236);
        assert_eq!(t.max_depth(), 700.0);
        assert_eq!(t.max_distance(), 2000.0);
    }

    #[test]
    fn grid_points_are_exact() {
        let t = table();
        assert_eq!(t.time(Wave::P, 0.0, 2.0), Some(0.416));
        assert_eq!(t.time(Wave::S, 0.0, 2.0), Some(0.703));
    }

    #[test]
    fn linear_between_grid_points() {
        let t = table();
        let a = t.time(Wave::S, 10.0, 100.0).unwrap();
        let b = t.time(Wave::S, 10.0, 105.0).unwrap();
        let m = t.time(Wave::S, 10.0, 102.5).unwrap();
        assert!((m - (a + b) / 2.0).abs() < 1e-9);
        let c = t.time(Wave::S, 12.0, 100.0).unwrap();
        let n = t.time(Wave::S, 11.0, 100.0).unwrap();
        assert!((n - (a + c) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn out_of_range_depth_is_none() {
        let t = table();
        assert_eq!(t.time(Wave::P, 701.0, 10.0), None);
        assert_eq!(t.time(Wave::P, -1.0, 10.0), None);
        assert_eq!(t.time(Wave::P, 10.0, -1.0), None);
        assert!(t.row(Wave::S, 800.0).is_none());
    }

    #[test]
    fn extrapolates_beyond_max_distance() {
        let t = table();
        let a = t.time(Wave::S, 10.0, 1990.0).unwrap();
        let b = t.time(Wave::S, 10.0, 2000.0).unwrap();
        let slope = (b - a) / 10.0;
        let far = t.time(Wave::S, 10.0, 3000.0).unwrap();
        assert!((far - (b + slope * 1000.0)).abs() < 1e-6);
        let row = t.row(Wave::S, 10.0).unwrap();
        match t.front(&row, far) {
            Front::Radius(r) => assert!((r - 3000.0).abs() < 1e-6, "{r}"),
            f => panic!("{f:?}"),
        }
    }

    #[test]
    fn monotonic_in_distance() {
        let t = table();
        let nd = t.distances.len();
        for g in [&t.p, &t.s] {
            for row in g.chunks(nd) {
                assert!(row.windows(2).all(|w| w[1] >= w[0]));
            }
        }
    }

    #[test]
    fn front_inverts_time() {
        let t = table();
        let row = t.row(Wave::S, 30.0).unwrap();
        let tt = t.time(Wave::S, 30.0, 123.0).unwrap();
        match t.front(&row, tt) {
            Front::Radius(r) => assert!((r - 123.0).abs() < 1e-6, "{r}"),
            f => panic!("{f:?}"),
        }
        assert!(matches!(t.front(&row, 1.0), Front::Underground(_)));
    }
}
