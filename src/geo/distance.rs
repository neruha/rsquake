//! 球面上の距離と、方位・距離から求める点。

pub const EARTH_RADIUS_KM: f64 = 6371.0;

/// 2 点間の大圏距離 (km)。引数は度。
pub fn great_circle_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().min(1.0).asin()
}

/// 始点から方位 `bearing_deg` へ `dist_km` 進んだ点 (緯度, 経度)。
pub fn destination(lat: f64, lon: f64, bearing_deg: f64, dist_km: f64) -> (f64, f64) {
    let d = dist_km / EARTH_RADIUS_KM;
    let (p1, l1, b) = (lat.to_radians(), lon.to_radians(), bearing_deg.to_radians());
    let p2 = (p1.sin() * d.cos() + p1.cos() * d.sin() * b.cos()).asin();
    let l2 = l1 + (b.sin() * d.sin() * p1.cos()).atan2(d.cos() - p1.sin() * p2.sin());
    (p2.to_degrees(), l2.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokyo_osaka() {
        let d = great_circle_km(35.681, 139.767, 34.702, 135.495);
        assert!((d - 403.0).abs() < 5.0, "{d}");
    }

    #[test]
    fn destination_round_trip() {
        for bearing in [0.0, 45.0, 135.0, 270.0] {
            let (lat, lon) = destination(38.0, 142.0, bearing, 250.0);
            let d = great_circle_km(38.0, 142.0, lat, lon);
            assert!((d - 250.0).abs() < 1e-6, "{bearing}: {d}");
        }
    }
}
