//! DMDATA JSON (eew-information v1.0.0) の serde 型。
//!
//! 受信側で壊れにくいよう、型定義で必須になっている項目もなるべく `Option` で受ける。

use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EewTelegram {
    #[serde(rename = "_schema")]
    pub schema: Option<Schema>,
    #[serde(rename = "type")]
    pub title_type: Option<String>,
    pub status: String,
    pub info_type: String,
    pub report_date_time: DateTime<Utc>,
    pub event_id: String,
    pub serial_no: String,
    pub body: Body,
}

#[derive(Debug, Deserialize)]
pub struct Schema {
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    #[serde(default)]
    pub is_last_info: bool,
    #[serde(default)]
    pub is_canceled: bool,
    #[serde(default)]
    pub is_warning: bool,
    pub earthquake: Option<Earthquake>,
    pub intensity: Option<Intensity>,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Earthquake {
    pub origin_time: Option<DateTime<Utc>>,
    pub arrival_time: Option<DateTime<Utc>>,
    pub condition: Option<String>,
    pub hypocenter: Hypocenter,
    pub magnitude: Option<Magnitude>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hypocenter {
    pub name: String,
    pub coordinate: Option<Coordinate>,
    pub depth: Option<UnitValue>,
    pub land_or_sea: Option<String>,
    pub accuracy: Option<Accuracy>,
}

#[derive(Debug, Deserialize)]
pub struct Coordinate {
    pub latitude: Option<UnitValue>,
    pub longitude: Option<UnitValue>,
}

#[derive(Debug, Deserialize)]
pub struct UnitValue {
    pub value: Option<String>,
    pub condition: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Magnitude {
    pub value: Option<String>,
    pub condition: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Accuracy {
    #[serde(default)]
    pub epicenters: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Intensity {
    pub forecast_max_int: Option<MaxInt>,
    pub forecast_max_lg_int: Option<MaxInt>,
    #[serde(default)]
    pub regions: Vec<Region>,
}

#[derive(Debug, Deserialize)]
pub struct MaxInt {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub code: String,
    pub name: String,
    pub forecast_max_int: MaxInt,
    pub forecast_max_lg_int: Option<MaxInt>,
    #[serde(default)]
    pub is_plum: bool,
    #[serde(default)]
    pub is_warning: bool,
    pub condition: Option<String>,
    pub arrival_time: Option<DateTime<Utc>>,
}
