//! 常駐とウィンドウの自動表示の判定。egui にもトレイにも依存しない。
//!
//! ゲームモードは設定を書き換えず、[`Behavior::new`] で打ち消すだけにする。
//! 切ると、設定した値がそのまま効き直す。

pub mod tray;

use crate::config::{PopupConfig, WindowConfig};
use crate::eew::store::ApplyOutcome;

/// いま実際に効いている動作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Behavior {
    /// 閉じるボタンでウィンドウを隠す (偽なら終了する)
    pub hide_on_close: bool,
    /// EEW でウィンドウを表示する
    pub show_on_eew: bool,
}

impl Behavior {
    /// `tray_available` は、トレイのアイコンを実際に出せていること。
    /// 出せていないと隠したウィンドウを戻せないので、隠さない。
    pub fn new(cfg: &WindowConfig, game_mode: bool, tray_available: bool) -> Self {
        Self {
            hide_on_close: cfg.resident && !game_mode && tray_available,
            show_on_eew: cfg.popup.enabled && !game_mode,
        }
    }
}

/// この報でウィンドウを表示するか。効果音と同じく、新しいイベントと初めての警報だけが対象。
pub fn wants_show(
    behavior: &Behavior,
    popup: &PopupConfig,
    outcome: &ApplyOutcome,
    is_warning: bool,
) -> bool {
    if !behavior.show_on_eew {
        return false;
    }
    match outcome {
        ApplyOutcome::New if is_warning => popup.warning,
        ApplyOutcome::New => popup.forecast,
        ApplyOutcome::BecameWarning => popup.warning,
        ApplyOutcome::Updated | ApplyOutcome::Canceled | ApplyOutcome::Rejected(_) => false,
    }
}

/// ウィンドウのいまの状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    /// 常駐中で、隠れている
    Hidden,
    Minimized,
    Shown,
}

/// 自動で表示したウィンドウを、EEW が終わったら元に戻すための記憶。
#[derive(Debug, Default)]
pub struct AutoShow {
    /// 自動で表示したときの元の状態 (`Shown` にはならない)。`None` なら戻すものが無い
    restore_to: Option<WindowState>,
}

impl AutoShow {
    /// EEW でウィンドウを表示するとき、表示する前の状態を渡す。
    /// すでに自動で表示しているなら、最初の状態を保つ。
    pub fn on_show(&mut self, before: WindowState) {
        if self.restore_to.is_none() && before != WindowState::Shown {
            self.restore_to = Some(before);
        }
    }

    /// EEW が終わった。元に戻す状態があれば返す。
    pub fn on_eew_ended(&mut self, hide_after: bool) -> Option<WindowState> {
        let restore = self.restore_to.take();
        if hide_after { restore } else { None }
    }

    /// ユーザーがウィンドウを操作した、またはゲームモードに入った。元には戻さない。
    pub fn cancel(&mut self) {
        self.restore_to = None;
    }

    /// 戻す状態を覚えているか
    pub fn pending(&self) -> bool {
        self.restore_to.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(resident: bool) -> WindowConfig {
        WindowConfig {
            resident,
            popup: PopupConfig::default(),
        }
    }

    #[test]
    fn behavior_combines_settings_game_mode_and_tray() {
        let b = Behavior::new(&window(true), false, true);
        assert!(b.hide_on_close && b.show_on_eew);
        // ゲームモードは両方を打ち消す
        let b = Behavior::new(&window(true), true, true);
        assert!(!b.hide_on_close && !b.show_on_eew);
        // 常駐オフ。表示は常駐と関係なく効く
        let b = Behavior::new(&window(false), false, true);
        assert!(!b.hide_on_close && b.show_on_eew);
        // トレイを出せないなら隠さない
        let b = Behavior::new(&window(true), false, false);
        assert!(!b.hide_on_close && b.show_on_eew);
        // 表示のオフ
        let mut w = window(true);
        w.popup.enabled = false;
        assert!(!Behavior::new(&w, false, true).show_on_eew);
    }

    #[test]
    fn wants_show_follows_outcome_and_kind() {
        let b = Behavior::new(&window(true), false, true);
        let p = PopupConfig::default();
        assert!(wants_show(&b, &p, &ApplyOutcome::New, false));
        assert!(wants_show(&b, &p, &ApplyOutcome::New, true));
        assert!(wants_show(&b, &p, &ApplyOutcome::BecameWarning, true));
        assert!(!wants_show(&b, &p, &ApplyOutcome::Updated, true));
        assert!(!wants_show(&b, &p, &ApplyOutcome::Canceled, false));
        assert!(!wants_show(&b, &p, &ApplyOutcome::Rejected("x"), false));
    }

    #[test]
    fn wants_show_respects_forecast_warning_and_game_mode() {
        let b = Behavior::new(&window(true), false, true);
        let forecast_off = PopupConfig {
            forecast: false,
            ..PopupConfig::default()
        };
        assert!(!wants_show(&b, &forecast_off, &ApplyOutcome::New, false));
        assert!(wants_show(&b, &forecast_off, &ApplyOutcome::New, true));
        let warning_off = PopupConfig {
            warning: false,
            ..PopupConfig::default()
        };
        assert!(wants_show(&b, &warning_off, &ApplyOutcome::New, false));
        assert!(!wants_show(&b, &warning_off, &ApplyOutcome::New, true));
        assert!(!wants_show(
            &b,
            &warning_off,
            &ApplyOutcome::BecameWarning,
            true
        ));
        let game = Behavior::new(&window(true), true, true);
        assert!(!wants_show(
            &game,
            &PopupConfig::default(),
            &ApplyOutcome::New,
            true
        ));
    }

    #[test]
    fn auto_show_restores_hidden_and_minimized() {
        for before in [WindowState::Hidden, WindowState::Minimized] {
            let mut a = AutoShow::default();
            a.on_show(before);
            assert!(a.pending());
            assert_eq!(a.on_eew_ended(true), Some(before));
            assert!(!a.pending());
            assert_eq!(a.on_eew_ended(true), None);
        }
    }

    #[test]
    fn auto_show_does_not_hide_a_visible_window() {
        let mut a = AutoShow::default();
        a.on_show(WindowState::Shown);
        assert!(!a.pending());
        assert_eq!(a.on_eew_ended(true), None);
    }

    #[test]
    fn auto_show_keeps_the_first_state() {
        let mut a = AutoShow::default();
        a.on_show(WindowState::Hidden);
        // 表示したあとの 2 つ目の EEW。ウィンドウはもう表示中
        a.on_show(WindowState::Shown);
        assert_eq!(a.on_eew_ended(true), Some(WindowState::Hidden));
    }

    #[test]
    fn auto_show_stops_after_user_action_or_when_hide_after_is_off() {
        let mut a = AutoShow::default();
        a.on_show(WindowState::Minimized);
        a.cancel();
        assert_eq!(a.on_eew_ended(true), None);

        a.on_show(WindowState::Minimized);
        assert_eq!(a.on_eew_ended(false), None);
        assert!(!a.pending());
    }
}
