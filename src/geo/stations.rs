//! 強震モニタの観測点リスト (ingen084/kyoshin-monitor-observation-points, MIT)。

use serde::Deserialize;

use super::projection::mercator;

#[derive(Clone, Debug)]
pub struct Station {
    pub code: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// 地図に描くためのメルカトル座標
    pub merc: [f64; 2],
    /// `jma_s` 画像上の画素位置 (`center_point`)。画像に載らない観測点は `None`。
    pub pixel: Option<(i32, i32)>,
    pub suspended: bool,
    /// 観測網の種類 (`type`。例: `kiK_net`)
    pub kind: String,
    /// 広域名 (例: `北海道`)
    pub region: String,
    /// 細分名 (例: `紋別地方`)。無い観測点もある
    pub sub_region: Option<String>,
}

#[derive(Debug)]
pub struct StationList {
    pub stations: Vec<Station>,
}

#[derive(Deserialize)]
struct Raw {
    code: String,
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    sub_region: Option<String>,
    #[serde(default)]
    is_suspended: bool,
    location: Location,
    point: Option<Point>,
}

#[derive(Deserialize)]
struct Location {
    latitude: f64,
    longitude: f64,
}

#[derive(Deserialize)]
struct Point {
    center_point: Xy,
}

#[derive(Deserialize)]
struct Xy {
    x: i32,
    y: i32,
}

impl StationList {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        // 配布元の JSON は BOM 付き
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let raw: Vec<Raw> = serde_json::from_slice(bytes)?;
        let stations = raw
            .into_iter()
            .map(|r| Station {
                merc: mercator(r.location.latitude, r.location.longitude),
                lat: r.location.latitude,
                lon: r.location.longitude,
                pixel: r.point.map(|p| (p.center_point.x, p.center_point.y)),
                code: r.code,
                name: r.name,
                suspended: r.is_suspended,
                kind: r.kind,
                region: r.region,
                sub_region: r.sub_region,
            })
            .collect();
        Ok(Self { stations })
    }

    pub fn bundled() -> Self {
        Self::parse(include_bytes!("../../assets/kmoni/intensity-points.json"))
            .expect("同梱の観測点リストが読めない")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_list_loads() {
        let l = StationList::bundled();
        assert_eq!(l.stations.len(), 1749);
        assert_eq!(l.stations.iter().filter(|s| s.pixel.is_none()).count(), 115);
        assert_eq!(l.stations.iter().filter(|s| s.suspended).count(), 14);
        let first = &l.stations[0];
        assert_eq!(
            (first.code.as_str(), first.pixel),
            ("ABSH01", Some((289, 42)))
        );
        assert_eq!(first.region, "北海道");
        assert_eq!(first.sub_region.as_deref(), Some("紋別地方"));
        assert_eq!(first.kind, "kiK_net");
    }

    #[test]
    fn every_station_has_a_region() {
        let l = StationList::bundled();
        assert!(l.stations.iter().all(|s| !s.region.is_empty()));
    }
}
