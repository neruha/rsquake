//! TopoJSON (量子化あり) のデコード。地図の描画に要る分だけを扱う。

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Topology {
    transform: Option<Transform>,
    arcs: Vec<Vec<[f64; 2]>>,
    objects: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
struct Transform {
    scale: [f64; 2],
    translate: [f64; 2],
}

#[derive(Deserialize)]
struct GeometryCollection {
    geometries: Vec<RawGeometry>,
}

#[derive(Deserialize)]
struct RawGeometry {
    #[serde(rename = "type")]
    kind: Option<String>,
    arcs: Option<Value>,
    properties: Option<Properties>,
}

#[derive(Deserialize, Default, Clone)]
struct Properties {
    code: Option<String>,
    name: Option<String>,
}

/// 1 本のポリゴン。`rings[0]` が外周、残りが穴。リングは arc 番号の列。
#[derive(Clone, Debug)]
pub struct Polygon {
    pub rings: Vec<Vec<i64>>,
}

#[derive(Clone, Debug)]
pub struct Feature {
    pub code: Option<String>,
    pub name: String,
    pub polygons: Vec<Polygon>,
}

#[derive(Debug)]
pub struct Topo {
    /// 経緯度 (経度, 緯度) に戻した arc
    pub arcs: Vec<Vec<[f64; 2]>>,
    pub objects: std::collections::HashMap<String, Vec<Feature>>,
}

impl Topo {
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        let t: Topology = serde_json::from_slice(bytes)?;
        let arcs = t
            .arcs
            .iter()
            .map(|arc| decode_arc(arc, t.transform.as_ref()))
            .collect();
        let mut objects = std::collections::HashMap::new();
        for (name, obj) in t.objects {
            let gc: GeometryCollection = serde_json::from_value(obj)?;
            let features = gc.geometries.into_iter().map(convert_geometry).collect();
            objects.insert(name, features);
        }
        Ok(Self { arcs, objects })
    }

    /// arc 番号 (負なら逆向き) を経緯度の列にして `out` へ足す。
    /// リングを組むとき、arc どうしの継ぎ目の点が重複しないよう先頭を飛ばす。
    pub fn push_arc(&self, index: i64, out: &mut Vec<[f64; 2]>) {
        let (arc, reversed) = if index >= 0 {
            (&self.arcs[index as usize], false)
        } else {
            (&self.arcs[(!index) as usize], true)
        };
        let skip = usize::from(!out.is_empty());
        if reversed {
            out.extend(arc.iter().rev().skip(skip));
        } else {
            out.extend(arc.iter().skip(skip));
        }
    }

    pub fn ring(&self, ring: &[i64]) -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        for &i in ring {
            self.push_arc(i, &mut out);
        }
        out
    }

    /// 細分区域 (`eew-region`) の 名前 → code の表。code が無い地物は含まない。
    pub fn region_name_to_code(&self) -> std::collections::HashMap<String, String> {
        crate::eew::wolfx::region_codes(
            self.objects
                .get("eew-region")
                .into_iter()
                .flatten()
                .map(|f| (f.code.as_deref(), f.name.as_str())),
        )
    }
}

/// arc 番号 (負の値を正規化した値) の集合を集める。
pub fn arc_ids(features: &[Feature]) -> std::collections::HashSet<usize> {
    features
        .iter()
        .flat_map(|f| f.polygons.iter())
        .flat_map(|p| p.rings.iter())
        .flatten()
        .map(|&i| if i >= 0 { i as usize } else { (!i) as usize })
        .collect()
}

fn decode_arc(arc: &[[f64; 2]], tr: Option<&Transform>) -> Vec<[f64; 2]> {
    match tr {
        Some(tr) => {
            let (mut x, mut y) = (0.0, 0.0);
            arc.iter()
                .map(|p| {
                    x += p[0];
                    y += p[1];
                    [
                        x * tr.scale[0] + tr.translate[0],
                        y * tr.scale[1] + tr.translate[1],
                    ]
                })
                .collect()
        }
        None => arc.to_vec(),
    }
}

fn parse_rings(v: &Value) -> Vec<Vec<i64>> {
    v.as_array()
        .map(|rings| {
            rings
                .iter()
                .filter_map(|r| r.as_array())
                .map(|r| r.iter().filter_map(Value::as_i64).collect())
                .collect()
        })
        .unwrap_or_default()
}

fn convert_geometry(g: RawGeometry) -> Feature {
    let props = g.properties.unwrap_or_default();
    let polygons = match (g.kind.as_deref(), &g.arcs) {
        (Some("Polygon"), Some(a)) => vec![Polygon {
            rings: parse_rings(a),
        }],
        (Some("MultiPolygon"), Some(a)) => a
            .as_array()
            .map(|ps| {
                ps.iter()
                    .map(|p| Polygon {
                        rings: parse_rings(p),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    Feature {
        code: props.code,
        name: props.name.unwrap_or_default(),
        polygons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "type": "Topology",
        "transform": {"scale": [0.5, 2.0], "translate": [100.0, 10.0]},
        "arcs": [[[0, 0], [2, 0], [0, 1]], [[2, 1], [-2, 0], [0, -1]]],
        "objects": {"a": {"type": "GeometryCollection", "geometries": [
            {"type": "Polygon", "arcs": [[0, 1]], "properties": {"code": "100", "name": "x"}},
            {"type": null, "properties": {"name": "empty"}},
            {"type": "Polygon", "arcs": [[-2, -1]], "properties": {"name": "rev"}}
        ]}}
    }"#;

    #[test]
    fn delta_decoding() {
        let t = Topo::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(t.arcs[0], vec![[100.0, 10.0], [101.0, 10.0], [101.0, 12.0]]);
        assert_eq!(t.arcs[1], vec![[101.0, 12.0], [100.0, 12.0], [100.0, 10.0]]);
    }

    #[test]
    fn ring_joins_and_reverses() {
        let t = Topo::parse(SAMPLE.as_bytes()).unwrap();
        let fs = &t.objects["a"];
        let ring = t.ring(&fs[0].polygons[0].rings[0]);
        assert_eq!(ring.len(), 5);
        assert_eq!(ring.first(), ring.last());
        let rev = t.ring(&fs[2].polygons[0].rings[0]);
        assert_eq!(rev.len(), 5);
        assert_eq!(rev[1], [100.0, 12.0]);
    }

    #[test]
    fn null_geometry_has_no_polygons() {
        let t = Topo::parse(SAMPLE.as_bytes()).unwrap();
        assert!(t.objects["a"][1].polygons.is_empty());
    }

    #[test]
    fn bundled_map_loads() {
        let t = Topo::parse(include_bytes!("../../assets/geojson/eew-prefregion.json")).unwrap();
        assert_eq!(t.objects["eew-pref"].len(), 56);
        assert_eq!(t.objects["eew-region"].len(), 194);
        assert!(
            t.objects["eew-region"]
                .iter()
                .any(|f| f.code.as_deref() == Some("100"))
        );
    }

    #[test]
    fn region_name_table_from_bundled_map() {
        let t = Topo::parse(include_bytes!("../../assets/geojson/eew-prefregion.json")).unwrap();
        let m = t.region_name_to_code();
        // 地図の 188 区域 (code あり) と別名 2 つ
        assert_eq!(m.len(), 190);
        assert_eq!(m["熊本県天草・芦北"], "743");
        assert_eq!(m["大東島"], "803");
        assert_eq!(m["沖縄県大東島"], "803");
        assert_eq!(m["宮古島"], "804");
    }
}
