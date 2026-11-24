//! チャネルの取り込みと画面の組み立て。

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use egui::{Color32, Rect, Ui, pos2, vec2};

use crate::clock::SimClock;
use crate::config::Config;
#[cfg(debug_assertions)]
use crate::dev::debug_menu::{DebugAction, DebugMenu};
#[cfg(debug_assertions)]
use crate::dev::scenario;
use crate::eew::display::{DisplayState, switch_interval};
use crate::eew::report::{EewReport, Incoming, Upstream};
use crate::eew::store::{ApplyOutcome, EventStore};
use crate::geo::stations::StationList;
use crate::geo::topo::Topo;
use crate::geo::travel_time::TravelTimeTable;
use crate::net::kmoni::KmoniEvent;
use crate::net::ws::WsSupervisor;
use crate::net::{LinkState, NetEvent};
use crate::resident::tray::{self, Tray, TrayAction};
use crate::resident::{AutoShow, Behavior, WindowState, wants_show};
use crate::sound::SoundPlayer;
use crate::ui::map::{MapEvent, MapView, Overlays, StationLayer};
use crate::ui::settings::{LinkInfo, SettingsWindow, UpdateView};
use crate::ui::status::{KmoniStatus, Status};
use crate::ui::update_popup::{PopupAction, UpdatePopup};
use crate::ui::{arrival_panel, info_panel, intensity_panel, status, theme};
use crate::update::download::Staged;
use crate::update::{self, CheckOptions, Phase, UpdateEvent, UpdateInfo};

/// この間、新しい画像が取れなければ、時刻を赤にして古い画像であることを示す。
const KMONI_STALE: chrono::Duration = chrono::Duration::seconds(5);

/// EEW 表示中の再描画間隔の範囲。動きが速いほど短くする。
const EEW_REPAINT_MIN: Duration = Duration::from_millis(33);
const EEW_REPAINT_MAX: Duration = Duration::from_millis(200);
/// EEW でウィンドウを最前面に出したあと、通常の重なり順に戻すまでの時間。
/// Windows はほかのアプリが前面にあるとフォーカスを奪えないので、一度だけ最前面にする。
const RAISE_HOLD: Duration = Duration::from_millis(500);
/// 隠れている間や、自動表示を戻すのを待つ間に `logic` を起こす間隔
const BACKGROUND_TICK: Duration = Duration::from_millis(500);

/// 1 回の再描画で、予報円などが動いてよい画面上の距離 (ポイント)
const EEW_STEP_PX: f32 = 0.5;

/// 地図の動きの速さ (ポイント/秒) から、次の再描画までの間隔を決める。
fn eew_repaint_interval(px_per_sec: f32) -> Duration {
    if px_per_sec <= 0.0 || !px_per_sec.is_finite() {
        return EEW_REPAINT_MAX;
    }
    Duration::from_secs_f32(EEW_STEP_PX / px_per_sec).clamp(EEW_REPAINT_MIN, EEW_REPAINT_MAX)
}

/// 強震モニタの最新の状態。観測点の色は、失敗しても最後に取れたものを残す。
struct Kmoni {
    enabled: bool,
    stations: Arc<StationList>,
    colors: Vec<Option<Color32>>,
    /// 画像を受け取るたびに増える番号。地図のキャッシュの鍵
    generation: u64,
    /// 最後に取れた画像の時刻と、取れた (受信した) 時刻
    last_frame: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// 最後の結果が失敗だった
    failing: bool,
}

impl Kmoni {
    fn status(&self, now: DateTime<Utc>) -> KmoniStatus {
        if !self.enabled {
            return KmoniStatus::Disabled;
        }
        match self.last_frame {
            Some((time, received)) if !self.failing && now - received <= KMONI_STALE => {
                KmoniStatus::Live(time)
            }
            last => KmoniStatus::Stale(last.map(|(time, _)| time)),
        }
    }
}

pub struct EewApp {
    config: Config,
    rx: Receiver<NetEvent>,
    store: EventStore,
    display: DisplayState,
    tt: TravelTimeTable,
    map: MapView,
    link: LinkState,
    upstream: Upstream,
    replay: bool,
    kmoni: Kmoni,
    settings: SettingsWindow,
    sound: SoundPlayer,
    /// srev の再生中は、画面の「今」をこの時計にする
    clock: Option<SimClock>,
    /// EEW の受信スレッド。リプレイ中は無い。受信元を変えたらつなぎ直す
    ws: Option<WsSupervisor>,
    update: Updater,
    /// ゲームモード。常駐と EEW での自動表示を止める。保存せず、起動のたびにオフ
    game_mode: bool,
    /// トレイ。常駐の設定がオンで、この OS で作れたときだけある
    tray: Option<Tray>,
    /// 自動で表示したウィンドウを元に戻すための記憶
    auto_show: AutoShow,
    /// 閉じるボタンで隠して、まだ戻していない
    hidden: bool,
    /// トレイの「終了」から終わらせている (閉じるボタンの隠す処理を飛ばす)
    quitting: bool,
    /// EEW でウィンドウを表示する依頼 (次の `logic` で処理する)
    show_pending: bool,
    /// 最前面にしたウィンドウを通常の重なり順に戻す時刻
    lower_at: Option<Instant>,
    /// デバッグメニュー (デバッグビルドだけ)
    #[cfg(debug_assertions)]
    debug: DebugMenu,
}

/// 新しい版の確認と、その通知。リプレイ中は何もしない。
struct Updater {
    /// 確認と通知をするか (リプレイ・時計を合わせた再生では無効)
    enabled: bool,
    current: semver::Version,
    tx: Sender<UpdateEvent>,
    rx: Receiver<UpdateEvent>,
    info: UpdateInfo,
    /// 起動時の通知をまだ決めていない。確認の結果 (または失敗) が届いたときに決める
    notice_pending: bool,
    popup: Option<UpdatePopup>,
    /// 確認まで済んだ配布物 (`Install::Ready` / `WaitingForEew` のとき)
    staged: Option<Staged>,
    /// 適用を頼まれている (EEW が終わったら適用する)
    apply_requested: bool,
    /// 前回のアプリ内の更新が失敗していたときの理由 (起動時の通知で 1 回だけ出す)
    last_failure: Option<String>,
    /// 適用役を起動したので、終了する
    quit: bool,
    /// 前回の更新が失敗したので、この起動では自動で適用しない
    auto_blocked: bool,
}

impl EewApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Config,
        rx: Receiver<NetEvent>,
        replay: bool,
        stations: Arc<StationList>,
        kmoni_enabled: bool,
        clock: Option<SimClock>,
        ws: Option<WsSupervisor>,
    ) -> Self {
        theme::install(&cc.egui_ctx);
        let (update_tx, update_rx) = channel();
        let update_enabled = !replay && clock.is_none();
        let mut update = Updater {
            enabled: update_enabled,
            current: update::current_version(),
            tx: update_tx,
            rx: update_rx,
            info: UpdateInfo::default(),
            notice_pending: update_enabled,
            popup: None,
            staged: None,
            apply_requested: false,
            last_failure: None,
            quit: false,
            auto_blocked: false,
        };
        if update_enabled {
            update.last_failure = update::apply::startup().failure;
            update.auto_blocked = update.last_failure.is_some();
        }
        if update_enabled && config.update.check {
            update.start_check(&config, &cc.egui_ctx);
        }
        let topo = Topo::parse(include_bytes!("../assets/geojson/eew-prefregion.json"))
            .expect("同梱の地図が読めない");
        let map = MapView::new(&topo);
        let tray = (config.window.resident && tray::SUPPORTED)
            .then(|| Tray::new(&cc.egui_ctx, false))
            .flatten();
        Self {
            // 時計を合わせて再生するときは、電文の時刻が今に合っているので補正しない
            store: EventStore::new((config.test_mode || replay) && clock.is_none()),
            config,
            rx,
            display: DisplayState::default(),
            tt: TravelTimeTable::bundled(),
            map,
            link: LinkState::Connecting,
            upstream: Upstream::Unknown,
            replay,
            kmoni: Kmoni {
                enabled: kmoni_enabled,
                colors: vec![None; stations.stations.len()],
                generation: 0,
                stations,
                last_frame: None,
                failing: false,
            },
            settings: SettingsWindow::default(),
            sound: SoundPlayer::new(),
            clock,
            ws,
            update,
            game_mode: false,
            tray,
            auto_show: AutoShow::default(),
            hidden: false,
            quitting: false,
            show_pending: false,
            lower_at: None,
            #[cfg(debug_assertions)]
            debug: DebugMenu::default(),
        }
    }

    /// いま効いている常駐と自動表示の動作。
    fn behavior(&self) -> Behavior {
        Behavior::new(&self.config.window, self.game_mode, self.tray.is_some())
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.map_or_else(Utc::now, |c| c.now())
    }

    fn drain(&mut self) {
        let now = self.now();
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                NetEvent::Link(l) => {
                    self.link = l;
                    if l != LinkState::Connected {
                        self.upstream = Upstream::Unknown;
                    }
                }
                NetEvent::Upstream(u) => self.upstream = u,
                NetEvent::Kmoni(KmoniEvent::Frame(f)) => {
                    // 古い画像が追い越して届いても戻らない
                    if self.kmoni.last_frame.is_none_or(|(t, _)| f.time >= t) {
                        self.kmoni.colors = f.colors;
                        self.kmoni.last_frame = Some((f.time, now));
                        self.kmoni.generation += 1;
                    }
                    self.kmoni.failing = false;
                }
                NetEvent::Kmoni(KmoniEvent::Failed(why)) => {
                    if !self.kmoni.failing {
                        tracing::warn!("強震モニタ: {why}");
                    }
                    self.kmoni.failing = true;
                }
                NetEvent::Message(Incoming::Report(r)) => self.apply_report(*r, now),
                NetEvent::Message(_) => {}
            }
        }
        self.store.prune(now);
    }

    /// 受信した 1 報を store に入れ、音と表示の切り替えに伝える。
    fn apply_report(&mut self, r: EewReport, now: DateTime<Utc>) {
        let (id, serial) = (r.event_id.clone(), r.serial);
        let is_warning = r.is_warning;
        let outcome = self.store.apply(r, now);
        self.sound.on_eew(&outcome, is_warning, &self.config.sound);
        if wants_show(
            &self.behavior(),
            &self.config.window.popup,
            &outcome,
            is_warning,
        ) {
            self.show_pending = true;
        }
        self.display.notify(&id, &outcome);
        match outcome {
            ApplyOutcome::Rejected(why) => {
                tracing::info!("{id} 第{serial}報を捨てた: {why}")
            }
            o => tracing::info!("{id} 第{serial}報: {o:?}"),
        }
    }

    /// デバッグメニュー: コナミコマンドの受付と、メニューからの依頼の処理。
    #[cfg(debug_assertions)]
    fn debug_step(&mut self, ctx: &egui::Context, now: DateTime<Utc>) {
        if self.settings.open {
            self.debug.feed_keys(ctx);
        } else {
            self.debug.settings_closed();
        }
        for action in self.debug.show(ctx, now, &self.update.current) {
            match action {
                DebugAction::Send(r) => self.apply_report(*r, now),
                DebugAction::ClearVirtual => {
                    self.store.remove_where(scenario::is_debug_id);
                }
                DebugAction::Popup(notice) => {
                    self.update.show_popup(notice);
                }
                DebugAction::ClosePopup => self.update.popup = None,
                DebugAction::FakeInstall(state) => self.update.info.install = state,
            }
        }
    }

    /// 更新の確認・ダウンロードの結果を受け取り、通知を出すか、適用するかを決める。
    /// 通知は EEW が出ている間は出さず、出ている通知は EEW が出たらすぐ閉じる。
    /// 適用 (再起動) は、EEW が出ている間はしない。
    fn update_step(&mut self, ctx: &egui::Context, eew_active: bool) {
        while let Ok(ev) = self.update.rx.try_recv() {
            match ev {
                UpdateEvent::Checked(r) => self.update.on_checked(r),
                UpdateEvent::Failed(why) => {
                    tracing::info!("更新を確認できない: {why}");
                    self.update.info.phase = Phase::Failed;
                }
                UpdateEvent::Download(ev) => self.update.on_download(ev),
            }
        }
        if eew_active {
            self.update.popup = None;
        }
        self.update.advance(&self.config, ctx, eew_active);

        // 起動時の通知: 確認が終わった (確認しないときは最初の 1 フレーム) ところで 1 回だけ決める
        if self.update.notice_pending && self.update.info.phase != Phase::Checking {
            self.update.notice_pending = false;
            let result =
                (self.update.info.phase == Phase::Done).then_some(&self.update.info.result);
            let failed = self.update.last_failure.take();
            let notice = update::choose_notice(
                result,
                &self.config.update,
                &self.update.current,
                eew_active,
                failed.as_deref(),
            );
            if let Some(notice) = notice {
                // 版が上がった案内は、出したときに見せた版として残す (出せなかったら次の起動でもう一度)
                if matches!(notice, update::Notice::Upgraded(_)) {
                    self.config.update.last_notes_version = Some(self.update.current.to_string());
                    self.save_config();
                }
                self.update.show_popup(notice);
            }
        }
        if self.update.quit {
            // 更新の適用で終了するときは、常駐でもウィンドウを隠さず終わる
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// 設定に合わせて、トレイを作る・消す。メインスレッドの `logic` から呼ぶ。
    fn sync_tray(&mut self, ctx: &egui::Context) {
        let want = self.config.window.resident && tray::SUPPORTED;
        if want && self.tray.is_none() {
            self.tray = Tray::new(ctx, self.game_mode);
        } else if !want {
            self.tray = None;
        }
        if let Some(t) = self.tray.as_mut() {
            t.set_game_mode(self.game_mode);
        }
    }

    fn window_state(&self, ctx: &egui::Context) -> WindowState {
        if self.hidden {
            WindowState::Hidden
        } else if ctx.input(|i| i.viewport().minimized) == Some(true) {
            WindowState::Minimized
        } else {
            WindowState::Shown
        }
    }

    /// ウィンドウを前に出す。`raise` なら、一度だけ最前面にしてフォーカスを求める。
    fn show_window(&mut self, ctx: &egui::Context, raise: bool) {
        use egui::{ViewportCommand, WindowLevel};
        if self.hidden {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            self.hidden = false;
        }
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        if raise {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
            self.lower_at = Some(Instant::now() + RAISE_HOLD);
        }
        ctx.request_repaint();
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        let actions = self.tray.as_ref().map(Tray::poll).unwrap_or_default();
        for action in actions {
            match action {
                TrayAction::Show => {
                    self.auto_show.cancel();
                    self.show_window(ctx, false);
                }
                TrayAction::Settings => {
                    self.auto_show.cancel();
                    self.settings.open_behavior();
                    self.show_window(ctx, false);
                }
                TrayAction::ToggleGameMode => self.game_mode = !self.game_mode,
                TrayAction::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// 閉じるボタン。常駐が効いているなら終了せず隠す。
    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quitting
            && self.behavior().hide_on_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden = true;
            self.auto_show.cancel();
        }
    }

    /// EEW でのウィンドウ表示と、終了後に元へ戻す処理。
    fn auto_show_step(&mut self, ctx: &egui::Context, eew_active: bool) {
        use egui::ViewportCommand;
        if self.game_mode {
            self.auto_show.cancel();
            self.show_pending = false;
        }
        if std::mem::take(&mut self.show_pending) {
            let before = self.window_state(ctx);
            self.auto_show.on_show(before);
            self.show_window(ctx, true);
        }
        if let Some(at) = self.lower_at
            && Instant::now() >= at
        {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            self.lower_at = None;
        }
        if !eew_active
            && self.auto_show.pending()
            && let Some(state) = self
                .auto_show
                .on_eew_ended(self.config.window.popup.hide_after)
        {
            match state {
                WindowState::Hidden => {
                    ctx.send_viewport_cmd(ViewportCommand::Visible(false));
                    self.hidden = true;
                }
                WindowState::Minimized => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                WindowState::Shown => {}
            }
        }
        // トレイが無いのに隠れたままだと戻せない
        if self.hidden && self.tray.is_none() {
            self.show_window(ctx, false);
        }
        // 隠れている間も、期限切れや元に戻す時刻に気づけるよう起こす
        if self.hidden || self.auto_show.pending() {
            ctx.request_repaint_after(BACKGROUND_TICK);
        }
        if let Some(at) = self.lower_at {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
    }

    fn save_config(&self) {
        if let Err(e) = self.config.save() {
            tracing::warn!("設定を保存できない: {e}");
        }
    }

    /// 表示中のイベントに、デバッグメニューの仮想イベントがあるか。リリースでは常に偽。
    fn has_debug_events(&self, active: &[&crate::eew::store::EewEvent]) -> bool {
        #[cfg(debug_assertions)]
        return active
            .iter()
            .any(|e| scenario::is_debug_id(&e.latest.event_id));
        #[cfg(not(debug_assertions))]
        {
            let _ = active;
            false
        }
    }
}

impl Updater {
    fn start_check(&mut self, config: &Config, ctx: &egui::Context) {
        self.info.phase = Phase::Checking;
        let mut opts = CheckOptions::new(config.update.include_prerelease);
        opts.current = self.current.clone();
        update::spawn(opts, self.tx.clone(), ctx.clone());
    }

    fn show_popup(&mut self, notice: update::Notice) {
        let installable = self.info.installable() && self.info.install.can_start();
        self.popup = Some(UpdatePopup::new(notice, self.current.clone(), installable));
    }

    /// 確認の結果を受け取る。進行中のダウンロードや確認済みのファイルは、確認し直しても変えない。
    fn on_checked(&mut self, result: update::CheckResult) {
        self.info.phase = Phase::Done;
        let busy = !self.info.install.can_start();
        self.info.plan = result.latest.as_ref().map(update::asset::plan_current);
        if !busy {
            self.info.install = update::Install::None;
        }
        self.info.result = result;
    }

    /// 自動で適用してよい新しい版があるか。スキップした版は適用しない。
    /// 前回の更新が失敗していたときは、その起動では自動で試さない (失敗をくり返さない)。
    fn auto_target(&self, config: &Config) -> bool {
        if !config.update.check || !config.update.auto_apply || self.auto_blocked {
            return false;
        }
        let Some(Ok(plan)) = &self.info.plan else {
            return false;
        };
        config.update.skipped_version.as_deref() != Some(&plan.version.to_string())
    }

    /// 「更新する」: ダウンロードして確認を始める。
    fn start_download(&mut self, ctx: &egui::Context) {
        if !self.info.install.can_start() {
            return;
        }
        let Some(Ok(plan)) = self.info.plan.clone() else {
            return;
        };
        self.info.install = update::Install::Downloading {
            done: 0,
            total: plan.asset.size,
        };
        update::download::spawn(plan, self.tx.clone(), ctx.clone());
    }

    fn on_download(&mut self, ev: update::download::DownloadEvent) {
        use update::Install;
        use update::download::DownloadEvent as D;
        match ev {
            D::Progress { done, total } => {
                if matches!(self.info.install, Install::Downloading { .. }) {
                    self.info.install = Install::Downloading { done, total };
                }
            }
            D::Verifying => self.info.install = Install::Verifying,
            D::Ready(staged) => {
                self.staged = Some(*staged);
                self.info.install = Install::Ready;
            }
            D::Failed(why) => {
                tracing::warn!("更新のダウンロードに失敗: {why}");
                self.staged = None;
                self.apply_requested = false;
                self.info.install = Install::Failed(why);
            }
        }
    }

    /// 毎フレーム: 適用を頼まれていて、確認も済んでいて、EEW が出ていなければ、適用役を起動する。
    fn advance(&mut self, config: &Config, ctx: &egui::Context, eew_active: bool) {
        use update::Install;
        // 自動で適用する設定なら、見つかった版のダウンロードを始める (設定を後からオンにしたときも)。
        // 始めるのは `None` のときだけで、失敗した (`Failed`) あとは自動でやり直さない。
        if self.enabled
            && self.info.install == Install::None
            && self.info.phase == Phase::Done
            && self.auto_target(config)
        {
            self.start_download(ctx);
            self.apply_requested = true;
        }
        if !self.apply_requested {
            return;
        }
        match self.info.install {
            Install::Ready | Install::WaitingForEew => {
                if eew_active {
                    self.info.install = Install::WaitingForEew;
                    return;
                }
                self.apply(ctx);
            }
            _ => {}
        }
    }

    /// 適用役を起動して、終了を頼む。
    fn apply(&mut self, ctx: &egui::Context) {
        use update::Install;
        let Some(staged) = self.staged.clone() else {
            return;
        };
        self.apply_requested = false;
        self.info.install = Install::Applying;
        match update::apply::start(&staged) {
            Ok(()) => {
                self.quit = true;
                ctx.request_repaint();
            }
            Err(e) => {
                tracing::warn!("更新を適用できない: {e:#}");
                self.info.install = Install::Failed(format!("{e:#}"));
            }
        }
    }

    /// 「再起動して更新」が押された。EEW が出ていれば、終わるまで待つ。
    fn request_apply(&mut self) {
        if matches!(self.info.install, update::Install::Ready) {
            self.apply_requested = true;
        }
    }
}

impl eframe::App for EewApp {
    /// ウィンドウが隠れていても呼ばれる。受信の取り込みなど、止めてはいけない処理はここに置く。
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_tray(ctx);
        self.drain();
        let now = self.now();
        let eew_now = self.store.active(now).iter().any(|e| !e.is_canceled());
        self.update_step(ctx, eew_now);
        self.handle_tray(ctx);
        self.handle_close(ctx);
        self.auto_show_step(ctx, eew_now);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // ウィンドウ内の操作があったら、自動表示したウィンドウは元に戻さない
        if ui.ctx().input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::PointerButton { pressed: true, .. }
                        | egui::Event::Key { pressed: true, .. }
                )
            })
        }) {
            self.auto_show.cancel();
        }
        let now = self.now();
        let active = self.store.active(now);
        // 右のパネルに出す 1 イベント。一定間隔で切り替える
        let interval = switch_interval(self.config.eew.switch_interval_secs);
        let ids: Vec<&str> = active.iter().map(|e| e.latest.event_id.as_str()).collect();
        let primary_id = self.display.update(&ids, interval, now).map(str::to_string);
        let position = self.display.position(&ids);
        let next_switch = self.display.next_switch_in(interval, now);
        let primary = primary_id
            .as_deref()
            .and_then(|id| active.iter().copied().find(|e| e.latest.event_id == id));

        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, theme::BG);
        let gap = 8.0;
        let map_rect =
            Rect::from_min_max(full.min, pos2(full.min.x + full.width() * 0.6, full.max.y));
        let side = Rect::from_min_max(
            pos2(map_rect.max.x + gap, full.min.y + gap),
            full.max - vec2(gap, gap),
        );

        // 地図
        let events: Vec<MapEvent<'_>> = active
            .iter()
            .map(|e| MapEvent {
                event: e,
                primary: primary.is_some_and(|p| std::ptr::eq(p, *e)),
            })
            .collect();
        let location = self
            .config
            .location
            .as_ref()
            .map(|l| (l.latitude, l.longitude));
        let mut map_ui = ui.new_child(egui::UiBuilder::new().max_rect(map_rect));
        let layer =
            (self.kmoni.enabled && self.kmoni.last_frame.is_some()).then_some(StationLayer {
                list: &self.kmoni.stations,
                colors: &self.kmoni.colors,
                generation: self.kmoni.generation,
            });

        let kmoni_status = self.kmoni.status(now);
        let eew_active = active.iter().any(|e| !e.is_canceled());
        self.map.show(
            &mut map_ui,
            &events,
            location,
            Overlays { stations: layer },
            &self.tt,
            now,
            self.config.eew.auto_zoom,
        );
        let keep: HashSet<&str> = active.iter().map(|e| e.latest.event_id.as_str()).collect();
        self.map.forget_rows(&keep);
        status::show(
            ui,
            map_rect,
            now,
            &Status {
                link: self.link,
                upstream: self.upstream,
                source: self.config.source,
                test_mode: self.config.test_mode,
                replay: self.replay || self.clock.is_some(),
                debug_events: self.has_debug_events(&active),
                game_mode: self.game_mode,
                kmoni: kmoni_status,
            },
        );

        // 設定画面を開くボタン (地図の右上)
        let gear = Rect::from_min_size(
            pos2(map_rect.max.x - 44.0, map_rect.min.y + 12.0),
            vec2(32.0, 32.0),
        );
        let resp = ui
            .put(
                gear,
                egui::Button::new(
                    egui::RichText::new(lucide_icons::Icon::Settings.unicode())
                        .font(theme::icon_font(17.0)),
                )
                .frame(true),
            )
            .on_hover_text("設定");
        if resp.clicked() {
            self.settings.open = !self.settings.open;
        }

        // 更新の通知 (地図の右下)
        if let Some(popup) = self.update.popup.as_mut() {
            match popup.show(ui.ctx(), map_rect) {
                PopupAction::None => {}
                PopupAction::Close => self.update.popup = None,
                PopupAction::Open(url) => {
                    if update::is_release_url(&url) {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                    self.update.popup = None;
                }
                PopupAction::Install => {
                    self.update.start_download(ui.ctx());
                    self.update.apply_requested = true;
                    self.update.popup = None;
                }
                PopupAction::Skip(version) => {
                    self.config.update.skipped_version = Some(version.to_string());
                    self.save_config();
                    self.update.popup = None;
                }
            }
        }

        // 右パネル: 情報 / 予想 / 震度
        let info_h = info_panel::height_for(side.width()).min(side.height() * 0.6);
        let info = Rect::from_min_size(side.min, vec2(side.width(), info_h));
        info_panel::show(ui, info, primary, position);
        let mut y = info.max.y + gap;

        if let Some(ev) = primary
            && let (Some(loc), Some(a)) = (
                self.config.location.as_ref(),
                arrival_panel::compute(ev, self.config.location.as_ref(), &self.tt, now),
            )
        {
            let h = (side.height() * 0.2).clamp(130.0, 170.0);
            let r = Rect::from_min_size(pos2(side.min.x, y), vec2(side.width(), h));
            arrival_panel::show(ui, r, ev, loc, &a);
            y = r.max.y + gap;
        }
        let rest = Rect::from_min_max(pos2(side.min.x, y), side.max);
        intensity_panel::show(ui, rest, primary, self.config.eew.region_display);

        let link = LinkInfo {
            link: self.link,
            upstream: self.upstream,
            replay: self.ws.is_none(),
        };
        let update_view = UpdateView {
            info: &self.update.info,
            current: &self.update.current,
        };

        let settings_changed = self.settings.show(
            ui.ctx(),
            &mut self.config,
            &mut self.sound,
            &mut self.game_mode,
            link,
            update_view,
        );
        if std::mem::take(&mut self.settings.request_install) {
            self.update.start_download(ui.ctx());
            // 設定画面からは、確認まで済んだところで止めて「再起動して更新」を待つ
            // (自動で適用する設定のときは、確認が済んだらそのまま適用する)
            self.update.apply_requested = self.config.update.auto_apply;
        }
        if std::mem::take(&mut self.settings.request_apply) {
            self.update.request_apply();
        }
        if std::mem::take(&mut self.settings.request_check)
            && self.update.enabled
            && self.update.info.phase != Phase::Checking
        {
            self.update.start_check(&self.config, ui.ctx());
        }
        if settings_changed {
            if let Err(e) = self.config.save() {
                tracing::warn!("設定を保存できない: {e}");
            }
            // テストモードの時刻補正は、切り替えたらすぐ効かせる
            self.store.shift_times = (self.config.test_mode || self.replay) && self.clock.is_none();
            // 受信元を変えたら、再起動せずにその場でつなぎ直す。進行中のイベントは残す
            if let Some(ws) = self.ws.as_mut()
                && ws.reconnect_if_changed(&self.config)
            {
                self.link = LinkState::Connecting;
                self.upstream = Upstream::Unknown;
            }
        }

        #[cfg(debug_assertions)]
        self.debug_step(ui.ctx(), now);
        // デバッグメニューや設定から出た表示の依頼を、次の `logic` で処理する
        if self.show_pending {
            ui.ctx().request_repaint();
        }

        // 再描画: EEW 表示中は予報円が動く速さに合わせる (時計のために毎秒は必ず描く)。
        // 平常時は時計のために毎秒。
        let ctx = ui.ctx();
        // 表示の切り替わりに合わせて描く (秒数などは画面に出さないので、切り替え時だけでよい)
        if let Some(wait) = next_switch {
            ctx.request_repaint_after(wait.to_std().unwrap_or_default() + Duration::from_millis(5));
        }
        if eew_active {
            let next_second =
                Duration::from_millis(1000 - now.timestamp_subsec_millis().min(999) as u64 + 5);
            ctx.request_repaint_after(
                eew_repaint_interval(self.map.motion_px_per_sec()).min(next_second),
            );
        } else {
            let ms = 1000 - now.timestamp_subsec_millis().min(999) as u64;
            ctx.request_repaint_after(Duration::from_millis(ms + 5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repaint_interval_follows_motion() {
        assert_eq!(eew_repaint_interval(0.0), EEW_REPAINT_MAX);
        assert_eq!(eew_repaint_interval(1.6), EEW_REPAINT_MAX);
        assert_eq!(eew_repaint_interval(1000.0), EEW_REPAINT_MIN);
        let mid = eew_repaint_interval(10.0);
        assert!(mid > EEW_REPAINT_MIN && mid < EEW_REPAINT_MAX, "{mid:?}");
    }
}
