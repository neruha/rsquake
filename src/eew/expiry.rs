//! 表示終了条件。EventID ごとに、最後に採用した報を受信してから 5 分間表示する。

use chrono::{DateTime, Duration, Utc};

/// 最後に報を受信してから表示し続ける秒数。
pub const DISPLAY_SECS: i64 = 300;

/// 受信時刻 `received` に採用した報のイベントの表示終了時刻。この時刻を過ぎたら終了。
pub fn expires_at(received: DateTime<Utc>) -> DateTime<Utc> {
    received + Duration::seconds(DISPLAY_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_minutes_from_receipt() {
        let received: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
        assert_eq!((expires_at(received) - received).num_seconds(), 300);
    }
}
