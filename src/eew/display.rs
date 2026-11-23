//! 表示するイベントの選択。
//!
//! 右のパネルは 1 イベントずつ出す。有効なイベントを一定間隔で順に切り替え、
//! 新しい報が届いても切り替えを前倒しにはしない。割り込むのは次の 3 つだけ。
//!
//! - 別のイベントが新しく発生した
//! - あるイベントで初めて警報が出た
//! - 取消報が届いた
//!
//! 割り込んだイベントも同じ間隔だけ表示し、そのあとは通常の切り替えに戻る。
//! 現在時刻は引数で受け取るので、時間を進めながらテストできる。

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};

use super::store::ApplyOutcome;

/// 切り替え間隔の既定値 (秒)。
pub const DEFAULT_SWITCH_SECS: u32 = 5;
pub const MIN_SWITCH_SECS: u32 = 1;
pub const MAX_SWITCH_SECS: u32 = 60;

/// 設定の秒数から切り替え間隔を作る。範囲外は折る。
pub fn switch_interval(secs: u32) -> Duration {
    Duration::seconds(secs.clamp(MIN_SWITCH_SECS, MAX_SWITCH_SECS) as i64)
}

#[derive(Debug, Default)]
pub struct DisplayState {
    current: Option<String>,
    /// `current` を出し始めた時刻
    since: Option<DateTime<Utc>>,
    /// いま出しているのは割り込みの表示か。割り込み中は次の割り込みを待たせる
    interrupting: bool,
    /// まだ出していない割り込み
    queue: VecDeque<String>,
    /// 直近の `update` で有効だったイベントの数
    active_len: usize,
}

impl DisplayState {
    /// 報を採用した結果を受け取り、割り込みの対象なら控えておく。
    pub fn notify(&mut self, event_id: &str, outcome: &ApplyOutcome) {
        let interrupts = matches!(
            outcome,
            ApplyOutcome::New | ApplyOutcome::BecameWarning | ApplyOutcome::Canceled
        );
        if interrupts && !self.queue.iter().any(|id| id == event_id) {
            self.queue.push_back(event_id.to_string());
        }
    }

    /// 表示するイベントの ID を決める。`active` は有効なイベントを並べたもの
    /// (切り替えはこの順に進む)。
    pub fn update(
        &mut self,
        active: &[&str],
        interval: Duration,
        now: DateTime<Utc>,
    ) -> Option<&str> {
        self.active_len = active.len();
        self.queue.retain(|id| active.contains(&id.as_str()));
        if self
            .current
            .as_deref()
            .is_some_and(|c| !active.contains(&c))
        {
            self.current = None;
            self.interrupting = false;
        }
        // 時計が戻ったときに、いつまでも切り替わらないのを防ぐ
        if self.since.is_some_and(|s| s > now) {
            self.since = Some(now);
        }

        match self.current.clone() {
            None => {
                // 何も出していないときは、待たせる相手がいないので、そのまま出す。
                // 続けて届いた割り込みは、次の更新で割り込みとして出る。
                let first = self
                    .queue
                    .pop_front()
                    .or_else(|| active.first().map(|id| (*id).to_string()));
                if let Some(id) = first {
                    self.queue.retain(|q| *q != id);
                    self.show(id, false, now);
                }
            }
            Some(cur) => {
                let due = self.since.is_none_or(|s| now - s >= interval);
                if !self.interrupting && !self.queue.is_empty() {
                    let id = self.queue.pop_front().expect("空でない");
                    self.show(id, true, now);
                } else if due {
                    if let Some(id) = self.queue.pop_front() {
                        self.show(id, true, now);
                    } else {
                        let next = active
                            .iter()
                            .position(|id| *id == cur)
                            .map_or(0, |i| (i + 1) % active.len());
                        self.show(active[next].to_string(), false, now);
                    }
                }
            }
        }
        self.current.as_deref()
    }

    fn show(&mut self, id: String, interrupting: bool, now: DateTime<Utc>) {
        self.current = Some(id);
        self.interrupting = interrupting;
        self.since = Some(now);
    }

    /// 次に表示が変わりうるまでの時間。変わる見込みが無ければ `None`。
    pub fn next_switch_in(&self, interval: Duration, now: DateTime<Utc>) -> Option<Duration> {
        if self.current.is_none() || (self.active_len < 2 && self.queue.is_empty()) {
            return None;
        }
        let since = self.since?;
        Some((interval - (now - since)).max(Duration::zero()))
    }

    /// 表示中のイベントの位置 (1 から) と、有効なイベントの数。
    pub fn position(&self, active: &[&str]) -> Option<(usize, usize)> {
        let cur = self.current.as_deref()?;
        let i = active.iter().position(|id| *id == cur)?;
        Some((i + 1, active.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: i64) -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap() + Duration::seconds(s)
    }

    const IV: i64 = 5;

    fn iv() -> Duration {
        Duration::seconds(IV)
    }

    fn shown(d: &mut DisplayState, active: &[&str], now: i64) -> Option<String> {
        d.update(active, iv(), t(now)).map(str::to_string)
    }

    #[test]
    fn nothing_active_shows_nothing() {
        let mut d = DisplayState::default();
        assert_eq!(shown(&mut d, &[], 0), None);
        assert_eq!(d.next_switch_in(iv(), t(0)), None);
    }

    #[test]
    fn single_event_stays() {
        let mut d = DisplayState::default();
        d.notify("a", &ApplyOutcome::New);
        for s in 0..20 {
            assert_eq!(shown(&mut d, &["a"], s).as_deref(), Some("a"));
        }
        assert_eq!(d.next_switch_in(iv(), t(20)), None);
    }

    #[test]
    fn rotates_in_order_at_the_interval() {
        let mut d = DisplayState::default();
        let all = ["a", "b", "c"];
        // 最初のイベントは割り込み扱いで先頭が出る
        d.notify("a", &ApplyOutcome::New);
        assert_eq!(shown(&mut d, &all, 0).as_deref(), Some("a"));
        // 後から来た b と c は割り込みになるので、それぞれ 1 間隔ずつ出る
        d.notify("b", &ApplyOutcome::New);
        d.notify("c", &ApplyOutcome::New);
        assert_eq!(shown(&mut d, &all, 1).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 5).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 6).as_deref(), Some("c"));
        // 割り込みが尽きたら通常の切り替え (c の次は先頭に戻る)
        assert_eq!(shown(&mut d, &all, 10).as_deref(), Some("c"));
        assert_eq!(shown(&mut d, &all, 11).as_deref(), Some("a"));
        assert_eq!(shown(&mut d, &all, 15).as_deref(), Some("a"));
        assert_eq!(shown(&mut d, &all, 16).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 21).as_deref(), Some("c"));
        assert_eq!(shown(&mut d, &all, 26).as_deref(), Some("a"));
    }

    #[test]
    fn updates_do_not_interrupt() {
        let mut d = DisplayState::default();
        let all = ["a", "b"];
        d.notify("a", &ApplyOutcome::New);
        shown(&mut d, &["a"], 0);
        d.notify("b", &ApplyOutcome::New);
        assert_eq!(shown(&mut d, &all, 1).as_deref(), Some("b"));
        // 続報は表示を動かさない
        d.notify("a", &ApplyOutcome::Updated);
        d.notify("b", &ApplyOutcome::Updated);
        for s in 2..6 {
            assert_eq!(shown(&mut d, &all, s).as_deref(), Some("b"), "{s}");
        }
        assert_eq!(shown(&mut d, &all, 6).as_deref(), Some("a"));
        d.notify("b", &ApplyOutcome::Updated);
        assert_eq!(shown(&mut d, &all, 10).as_deref(), Some("a"));
        assert_eq!(shown(&mut d, &all, 11).as_deref(), Some("b"));
    }

    #[test]
    fn rejected_reports_do_not_interrupt() {
        let mut d = DisplayState::default();
        let all = ["a", "b"];
        d.notify("a", &ApplyOutcome::New);
        shown(&mut d, &["a"], 0);
        d.notify("b", &ApplyOutcome::Rejected("報数が古い"));
        assert_eq!(shown(&mut d, &all, 1).as_deref(), Some("a"));
        assert_eq!(shown(&mut d, &all, 5).as_deref(), Some("b"));
    }

    #[test]
    fn first_warning_interrupts_then_normal_rotation_resumes() {
        let mut d = DisplayState::default();
        let all = ["a", "b", "c"];
        d.notify("a", &ApplyOutcome::New);
        shown(&mut d, &["a"], 0);
        d.notify("b", &ApplyOutcome::New);
        d.notify("c", &ApplyOutcome::New);
        assert_eq!(shown(&mut d, &all, 1).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 6).as_deref(), Some("c"));
        assert_eq!(shown(&mut d, &all, 11).as_deref(), Some("a"));
        // a が出ている最中に、b で初めて警報が出る
        d.notify("b", &ApplyOutcome::BecameWarning);
        assert_eq!(shown(&mut d, &all, 12).as_deref(), Some("b"));
        // b は割り込み時刻から 1 間隔 (12 + 5 = 17) 出る
        assert_eq!(shown(&mut d, &all, 16).as_deref(), Some("b"));
        // そのあとは、割り込まれた b の次 (c) から通常の切り替え
        assert_eq!(shown(&mut d, &all, 17).as_deref(), Some("c"));
        assert_eq!(shown(&mut d, &all, 22).as_deref(), Some("a"));
    }

    #[test]
    fn cancel_on_the_shown_event_restarts_its_interval() {
        let mut d = DisplayState::default();
        let all = ["a", "b"];
        d.notify("a", &ApplyOutcome::New);
        shown(&mut d, &["a"], 0);
        d.notify("b", &ApplyOutcome::New);
        shown(&mut d, &all, 1); // b
        assert_eq!(shown(&mut d, &all, 4).as_deref(), Some("b"));
        // 切り替えの直前に b の取消が届く。b をもう 1 間隔出す
        d.notify("b", &ApplyOutcome::Canceled);
        // 割り込み中の表示なので、いまの間隔が終わるまで待つ
        assert_eq!(shown(&mut d, &all, 5).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 6).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 10).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 11).as_deref(), Some("a"));
    }

    #[test]
    fn cancel_on_another_event_interrupts() {
        let mut d = DisplayState::default();
        let all = ["a", "b"];
        d.notify("a", &ApplyOutcome::New);
        shown(&mut d, &["a"], 0);
        d.notify("b", &ApplyOutcome::New);
        shown(&mut d, &all, 1); // b (割り込み)
        assert_eq!(shown(&mut d, &all, 6).as_deref(), Some("a"));
        d.notify("b", &ApplyOutcome::Canceled);
        assert_eq!(shown(&mut d, &all, 7).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 11).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 12).as_deref(), Some("a"));
    }

    #[test]
    fn interrupts_queue_up_while_one_is_shown() {
        let mut d = DisplayState::default();
        let all = ["a", "b", "c"];
        d.notify("a", &ApplyOutcome::New);
        shown(&mut d, &["a"], 0);
        d.notify("b", &ApplyOutcome::New);
        assert_eq!(shown(&mut d, &["a", "b"], 1).as_deref(), Some("b"));
        // b の表示中に c が発生。b を途中で切らず、b のあとに c を出す
        d.notify("c", &ApplyOutcome::New);
        assert_eq!(shown(&mut d, &all, 3).as_deref(), Some("b"));
        assert_eq!(shown(&mut d, &all, 6).as_deref(), Some("c"));
        assert_eq!(shown(&mut d, &all, 10).as_deref(), Some("c"));
        assert_eq!(shown(&mut d, &all, 11).as_deref(), Some("a"));
    }

    #[test]
    fn expired_event_is_replaced_at_once() {
        let mut d = DisplayState::default();
        d.notify("a", &ApplyOutcome::New);
        shown(&mut d, &["a"], 0);
        d.notify("b", &ApplyOutcome::New);
        shown(&mut d, &["a", "b"], 1); // b
        // 表示中の b が終了した。間隔を待たずに残りを出す
        assert_eq!(shown(&mut d, &["a"], 2).as_deref(), Some("a"));
        // 割り込み待ちのイベントが終了したら、割り込みは捨てる
        d.notify("c", &ApplyOutcome::New);
        assert_eq!(shown(&mut d, &["a"], 3).as_deref(), Some("a"));
        assert_eq!(shown(&mut d, &["a"], 4).as_deref(), Some("a"));
        assert_eq!(shown(&mut d, &[], 5), None);
    }

    #[test]
    fn interval_is_read_each_time() {
        let mut d = DisplayState::default();
        let all = ["a", "b"];
        d.notify("a", &ApplyOutcome::New);
        d.update(&["a"], iv(), t(0));
        d.notify("b", &ApplyOutcome::New);
        d.update(&all, iv(), t(1)); // b
        assert_eq!(
            d.update(&all, Duration::seconds(10), t(6)),
            Some("b"),
            "間隔を伸ばしたので 6 秒では切り替わらない"
        );
        assert_eq!(d.update(&all, Duration::seconds(10), t(11)), Some("a"));
    }

    #[test]
    fn next_switch_and_position() {
        let mut d = DisplayState::default();
        let all = ["a", "b"];
        d.notify("a", &ApplyOutcome::New);
        d.update(&["a"], iv(), t(0));
        d.notify("b", &ApplyOutcome::New);
        d.update(&all, iv(), t(1)); // b
        assert_eq!(d.next_switch_in(iv(), t(3)), Some(Duration::seconds(3)));
        assert_eq!(d.next_switch_in(iv(), t(30)), Some(Duration::zero()));
        assert_eq!(d.position(&all), Some((2, 2)));
        assert_eq!(d.position(&["a"]), None);
    }

    #[test]
    fn clock_moving_backwards_does_not_freeze() {
        let mut d = DisplayState::default();
        let all = ["a", "b"];
        d.notify("a", &ApplyOutcome::New);
        d.update(&["a"], iv(), t(100));
        d.update(&all, iv(), t(0));
        assert_eq!(d.update(&all, iv(), t(5)), Some("b"));
    }

    #[test]
    fn switch_interval_is_clamped() {
        assert_eq!(switch_interval(0), Duration::seconds(1));
        assert_eq!(switch_interval(5), Duration::seconds(5));
        assert_eq!(switch_interval(9999), Duration::seconds(60));
    }
}
