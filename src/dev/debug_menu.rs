//! デバッグメニュー (デバッグビルドだけ)。
//!
//! 設定画面を開いた状態でコナミコマンド (↑↑↓↓←→←→BA) を打つと開く。
//! 更新ポップアップの強制表示と、仮想電文 (`scenario`) の送出ができる。
//! 押されたボタンは `DebugAction` で呼び出し側に返し、実際の処理は `EewApp` が行う。

use chrono::{DateTime, Duration, Utc};
use egui::{ComboBox, Context, Id, Key, RichText, ScrollArea, Slider, Ui};
use semver::Version;

use super::scenario::{self, ID_PREFIX, PRESETS, Params};
use crate::eew::report::{EewReport, TelegramStatus};
use crate::update::{self, Notice, Release};

/// コナミコマンド。
const KONAMI: [Key; 10] = [
    Key::ArrowUp,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::B,
    Key::A,
];

/// キー入力が `KONAMI` の並びになったかを調べる。直近のキーだけを覚える。
#[derive(Default)]
struct KonamiDetector {
    recent: Vec<Key>,
}

impl KonamiDetector {
    /// 1 回のキー押下を渡す。直近のキーが並びと揃ったら真。
    fn feed(&mut self, key: Key) -> bool {
        self.recent.push(key);
        if self.recent.len() > KONAMI.len() {
            self.recent.remove(0);
        }
        let hit = self.recent == KONAMI;
        if hit {
            self.recent.clear();
        }
        hit
    }

    fn reset(&mut self) {
        self.recent.clear();
    }
}
/// メニューから呼び出し側への依頼。
pub enum DebugAction {
    /// 仮想の報を、受信した電文として流す
    Send(Box<EewReport>),
    /// 仮想イベント (ID が `DEBUG-`) をすべて消す
    ClearVirtual,
    /// 更新などのポップアップを出す
    Popup(Notice),
    ClosePopup,
}

pub struct DebugMenu {
    pub open: bool,
    konami: KonamiDetector,
    params: Params,
    /// 仮想イベントの通し番号 (ID の元)
    counter: u32,
    /// 操作の対象にしているイベントと、最後に送った報数・発生時刻
    target: Option<Target>,
    /// ポップアップに出す版 (入力中の文字)
    popup_version: String,
    window_id: Id,
}

struct Target {
    id: String,
    serial: u32,
    origin: DateTime<Utc>,
}

impl Default for DebugMenu {
    fn default() -> Self {
        Self {
            open: false,
            konami: KonamiDetector::default(),
            params: Params::for_preset(0),
            counter: 0,
            target: None,
            popup_version: String::new(),
            window_id: Id::unique("debug_menu_window"),
        }
    }
}

impl DebugMenu {
    /// 設定画面が開いているフレームで呼ぶ。コナミコマンドが揃ったらメニューを開く。
    pub fn feed_keys(&mut self, ctx: &Context) {
        let keys: Vec<Key> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } => Some(*key),
                    _ => None,
                })
                .collect()
        });
        for key in keys {
            if self.konami.feed(key) {
                self.open = true;
            }
        }
    }

    /// 設定画面が閉じているあいだは、入力の途中経過を捨てる。
    pub fn settings_closed(&mut self) {
        self.konami.reset();
    }

    pub fn show(
        &mut self,
        ctx: &Context,
        now: DateTime<Utc>,
        current: &Version,
    ) -> Vec<DebugAction> {
        let mut actions = Vec::new();
        if !self.open {
            return actions;
        }
        if self.popup_version.is_empty() {
            self.popup_version = bump_patch(current).to_string();
        }
        let mut open = true;
        egui::Window::new("デバッグメニュー")
            .id(self.window_id)
            .open(&mut open)
            .default_width(360.0)
            .default_pos(egui::pos2(24.0, 64.0))
            .show(ctx, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    self.popup_section(ui, current, &mut actions);
                    ui.separator();
                    self.eew_section(ui, now, &mut actions);
                });
            });
        self.open = open;
        actions
    }

    fn popup_section(&mut self, ui: &mut Ui, current: &Version, actions: &mut Vec<DebugAction>) {
        ui.heading("バージョンのポップアップ");
        ui.label(format!("いまの版: {current}"));
        ui.horizontal(|ui| {
            ui.label("表示する版");
            ui.text_edit_singleline(&mut self.popup_version);
        });
        let parsed = Version::parse(self.popup_version.trim());
        if parsed.is_err() {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "semver として読めません (例: 0.2.0)",
            );
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(parsed.is_ok(), egui::Button::new("新しい版がある"))
                .clicked()
                && let Ok(v) = &parsed
            {
                actions.push(DebugAction::Popup(Notice::Update(Release {
                    version: v.clone(),
                    url: update::release_url(v),
                })));
            }
            if ui
                .add_enabled(parsed.is_ok(), egui::Button::new("版が上がった案内"))
                .clicked()
                && let Ok(v) = parsed
            {
                actions.push(DebugAction::Popup(Notice::Upgraded(v)));
            }
            if ui.button("閉じる").clicked() {
                actions.push(DebugAction::ClosePopup);
            }
        });
        ui.weak("EEW が表示されている間、ポップアップはすぐ閉じられます。");
    }

    fn eew_section(&mut self, ui: &mut Ui, now: DateTime<Utc>, actions: &mut Vec<DebugAction>) {
        ui.heading("仮想 EEW");
        let before = self.params.preset;
        ComboBox::from_id_salt("debug_preset")
            .width(300.0)
            .selected_text(PRESETS[self.params.preset].name)
            .show_ui(ui, |ui| {
                for (i, p) in PRESETS.iter().enumerate() {
                    ui.selectable_value(&mut self.params.preset, i, p.name);
                }
            });
        if self.params.preset != before {
            // プリセットを変えたら、値もそのプリセットのものに戻す (ステータスなどは残す)
            let keep = (
                self.params.status,
                self.params.long_period,
                self.params.elapsed_secs,
            );
            self.params = Params::for_preset(self.params.preset);
            (
                self.params.status,
                self.params.long_period,
                self.params.elapsed_secs,
            ) = keep;
        }

        let preset = self.params.preset();
        ui.add_enabled_ui(!preset.is_assumed(), |ui| {
            ui.add(Slider::new(&mut self.params.magnitude, 3.0..=9.5).text("マグニチュード"));
            ui.add(Slider::new(&mut self.params.depth, 0.0..=700.0).text("深さ (km)"));
        });
        ui.add(
            Slider::new(&mut self.params.max_index, 1..=9)
                .custom_formatter(|v, _| scenario::shindo_label(v as u8).to_string())
                .text("最大震度"),
        );
        ui.add(Slider::new(&mut self.params.elapsed_secs, 0..=90).text("発生からの経過 (秒)"));
        ui.checkbox(&mut self.params.long_period, "長周期地震動階級を付ける");
        ui.horizontal(|ui| {
            ui.label("ステータス");
            ComboBox::from_id_salt("debug_status")
                .selected_text(status_name(self.params.status))
                .show_ui(ui, |ui| {
                    for s in [
                        TelegramStatus::Normal,
                        TelegramStatus::Training,
                        TelegramStatus::Test,
                    ] {
                        ui.selectable_value(&mut self.params.status, s, status_name(s));
                    }
                });
        });

        ui.add_space(4.0);
        match &self.target {
            Some(t) => ui.label(RichText::new(format!("対象: {} (第{}報)", t.id, t.serial))),
            None => ui.label(RichText::new("対象: なし").weak()),
        };
        ui.horizontal_wrapped(|ui| {
            if ui.button("新しいイベントで第1報").clicked() {
                self.counter += 1;
                let id = format!("{ID_PREFIX}{:04}", self.counter);
                let origin = now - Duration::seconds(self.params.elapsed_secs as i64);
                self.target = Some(Target {
                    id,
                    serial: 0,
                    origin,
                });
                self.send_next(now, false, actions);
            }
            let has_target = self.target.is_some();
            if ui
                .add_enabled(has_target, egui::Button::new("次の報を送る"))
                .clicked()
            {
                self.send_next(now, false, actions);
            }
            if ui
                .add_enabled(has_target, egui::Button::new("最終報を送る"))
                .clicked()
            {
                self.send_next(now, true, actions);
            }
            if ui
                .add_enabled(has_target, egui::Button::new("取消報を送る"))
                .clicked()
                && let Some(t) = self.target.as_mut()
            {
                t.serial += 1;
                actions.push(DebugAction::Send(Box::new(scenario::build_cancel(
                    &t.id,
                    t.serial,
                    self.params.status,
                    now,
                ))));
            }
        });
        if ui.button("仮想イベントをすべて消す").clicked() {
            self.target = None;
            actions.push(DebugAction::ClearVirtual);
        }
        ui.weak("本物の受信は止めません。仮想の報は ID が DEBUG- で始まります。");
    }

    fn send_next(&mut self, now: DateTime<Utc>, is_last: bool, actions: &mut Vec<DebugAction>) {
        let Some(t) = self.target.as_mut() else {
            return;
        };
        t.serial += 1;
        actions.push(DebugAction::Send(Box::new(scenario::build_report(
            &self.params,
            &t.id,
            t.serial,
            t.origin,
            now,
            is_last,
        ))));
    }
}

fn status_name(s: TelegramStatus) -> &'static str {
    match s {
        TelegramStatus::Normal => "通常",
        TelegramStatus::Training => "訓練",
        TelegramStatus::Test => "試験",
    }
}

fn bump_patch(v: &Version) -> Version {
    Version::new(v.major, v.minor, v.patch + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(d: &mut KonamiDetector, keys: &[Key]) -> bool {
        keys.iter().fold(false, |hit, k| hit | d.feed(*k))
    }

    #[test]
    fn konami_opens() {
        let mut d = KonamiDetector::default();
        assert!(feed_all(&mut d, &KONAMI));
    }

    #[test]
    fn konami_needs_exact_order() {
        let mut d = KonamiDetector::default();
        let mut keys = KONAMI.to_vec();
        keys.swap(8, 9);
        assert!(!feed_all(&mut d, &keys));
    }

    #[test]
    fn konami_restarts_after_a_mistake() {
        let mut d = KonamiDetector::default();
        // 先頭の ↑ を余分に押しても、そこから数え直して成立する
        let mut keys = vec![Key::ArrowUp];
        keys.extend(KONAMI);
        assert!(feed_all(&mut d, &keys));
    }

    #[test]
    fn konami_can_repeat() {
        let mut d = KonamiDetector::default();
        assert!(feed_all(&mut d, &KONAMI));
        assert!(feed_all(&mut d, &KONAMI));
    }

    #[test]
    fn popup_version_is_newer() {
        let v = Version::new(0, 1, 1);
        assert!(bump_patch(&v) > v);
    }
}
