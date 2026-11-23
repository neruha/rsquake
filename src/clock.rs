//! 画面の「今」。リプレイでは、現実の時計を過去の時刻に対応づけた時計を使う。

use chrono::{DateTime, Utc};

/// 再生時刻 = `start` + (現実の時刻 − `anchor`)。1 倍速。
///
/// rsquake とテストサーバーは別のプロセスなので、同じ `start` と `anchor` を渡して時間軸をそろえる。
#[derive(Clone, Copy, Debug)]
pub struct SimClock {
    /// 再生を始める時刻 (リプレイ上の時刻)
    pub start: DateTime<Utc>,
    /// `start` に当たる現実の時刻
    pub anchor: DateTime<Utc>,
}

impl SimClock {
    pub fn new(start: DateTime<Utc>, anchor: DateTime<Utc>) -> Self {
        Self { start, anchor }
    }

    pub fn at(&self, wall: DateTime<Utc>) -> DateTime<Utc> {
        self.start + (wall - self.anchor)
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.at(Utc::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_wall_time_onto_the_replay_time() {
        let start: DateTime<Utc> = "2026-01-01T00:00:30Z".parse().unwrap();
        let anchor: DateTime<Utc> = "2030-01-01T00:00:00Z".parse().unwrap();
        let c = SimClock::new(start, anchor);
        assert_eq!(c.at(anchor), start);
        assert_eq!(
            c.at(anchor + chrono::Duration::seconds(51)).to_rfc3339(),
            "2026-01-01T00:01:21+00:00"
        );
        // 起点より前の現実の時刻は、起点より前の再生時刻になる
        assert!(c.at(anchor - chrono::Duration::seconds(1)) < start);
    }
}
