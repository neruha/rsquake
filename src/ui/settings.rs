//! 設定画面。接続、表示、指定地点、効果音、ライセンス (クレジット)。
//! ここで変えた値は `config.toml` に保存する。
//!
//! 左にページの一覧、右にそのページの項目を並べる。項目は「名前と説明 / 操作する部品」の
//! 1 行で、関係するものをカードにまとめる。
//!
//! 文字で入力する項目 (接続先、指定地点) は、入力欄を離れたとき (Enter かフォーカス移動) に
//! 検証して反映する。1 文字ごとにつなぎ直さないため。不正な値は反映せず、理由を出す。

use std::ops::RangeInclusive;

use egui::emath::GuiRounding as _;
use egui::{
    Align, Align2, Button, Color32, Context, CornerRadius, CursorIcon, FontId, Frame, Id, Label,
    Layout, Margin, Rect, Response, RichText, Role, ScrollArea, Sense, Stroke, StrokeKind,
    TextEdit, Ui, UiBuilder, WidgetInfo, pos2, vec2,
};
use lucide_icons::Icon;

use crate::config::{Config, EewSource, Location, RegionDisplay, SoundConfig};
use crate::credits::CREDITS;
use crate::eew::display::{MAX_SWITCH_SECS, MIN_SWITCH_SECS};
use crate::eew::report::Upstream;
use crate::net::LinkState;
use crate::sound::{SoundCue, SoundPlayer};
use crate::ui::theme;
use crate::update::{self, Phase, UpdateInfo};

/// 設定画面の大きさ。アプリの窓が小さいときは、それに合わせて縮める。
const WINDOW_SIZE: egui::Vec2 = vec2(820.0, 580.0);
const WINDOW_RADIUS: u8 = 10;
const SIDEBAR_W: f32 = 184.0;
const HEADER_H: f32 = 56.0;
const NAV_H: f32 = 36.0;
/// ページの左右の余白
const PAGE_PAD: i8 = 24;
/// カードとカードの間
const SECTION_GAP: f32 = 22.0;

/// 行の内側の余白と、最低限の高さ
const ROW_PAD: egui::Vec2 = vec2(16.0, 12.0);
const ROW_MIN_H: f32 = 54.0;
/// 項目名と説明の行間
const LINE_GAP: f32 = 3.0;
/// 項目名と操作部品の間
const CONTROL_GAP: f32 = 16.0;
/// 横に並ぶ操作部品どうしの間
const CONTROL_SPACING: f32 = 8.0;

const TOGGLE_SIZE: egui::Vec2 = vec2(40.0, 22.0);
const FIELD_W: f32 = 280.0;
const SLIDER_W: f32 = 170.0;
/// スライダーの右に出す値の幅 (値が変わっても位置が動かないよう固定する)
const VALUE_W: f32 = 52.0;
/// アイコンと 2 文字のボタン (試聴、消去) の幅
const BUTTON_W: f32 = 80.0;
/// 接続の状態を出す幅
const STATUS_W: f32 = 130.0;
const SEGMENT_H: f32 = 32.0;
const SEGMENT_PAD: f32 = 3.0;

const SIDEBAR: Color32 = Color32::from_rgb(0x16, 0x18, 0x1c);
const NAV_HOVER: Color32 = Color32::from_rgb(0x20, 0x23, 0x29);
const NAV_SELECTED: Color32 = Color32::from_rgb(0x2a, 0x2f, 0x37);
const CARD: Color32 = Color32::from_rgb(0x21, 0x25, 0x2b);
const DIVIDER: Color32 = Color32::from_rgb(0x2e, 0x33, 0x3a);
const BUTTON: Color32 = Color32::from_rgb(0x2c, 0x31, 0x38);
const BUTTON_HOVER: Color32 = Color32::from_rgb(0x36, 0x3c, 0x44);
const BUTTON_ACTIVE: Color32 = Color32::from_rgb(0x40, 0x47, 0x50);
const BORDER_HOVER: Color32 = Color32::from_rgb(0x4a, 0x51, 0x5b);
const TOGGLE_OFF: Color32 = Color32::from_rgb(0x45, 0x4b, 0x54);
const SEGMENT_SELECTED: Color32 = Color32::from_rgb(0x3a, 0x40, 0x49);
const TEXT_SELECTION: Color32 = Color32::from_rgb(0x26, 0x4f, 0x8f);
const LINK: Color32 = Color32::from_rgb(0x7f, 0xb2, 0xff);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Tab {
    #[default]
    Connection,
    Display,
    Location,
    Sound,
    Behavior,
    Update,
    License,
}

impl Tab {
    const ALL: [Self; 7] = [
        Self::Connection,
        Self::Display,
        Self::Location,
        Self::Sound,
        Self::Behavior,
        Self::Update,
        Self::License,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Connection => "接続",
            Self::Display => "表示",
            Self::Location => "指定地点",
            Self::Sound => "効果音",
            Self::Behavior => "動作",
            Self::Update => "アップデート",
            Self::License => "ライセンス",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Connection => Icon::Cable,
            Self::Display => Icon::Monitor,
            Self::Location => Icon::MapPin,
            Self::Sound => Icon::Volume2,
            Self::Behavior => Icon::AppWindow,
            Self::Update => Icon::RefreshCw,
            Self::License => Icon::ScrollText,
        }
    }
}

/// 接続ページに出す、いまの接続の状態。
#[derive(Clone, Copy, Debug)]
pub struct LinkInfo {
    pub link: LinkState,
    pub upstream: Upstream,
    /// リプレイ中 (受信スレッドが無く、接続先の設定は使われていない)
    pub replay: bool,
}

/// アップデートのページに出す、確認の状態。
#[derive(Clone, Copy)]
pub struct UpdateView<'a> {
    pub info: &'a UpdateInfo,
    pub current: &'a semver::Version,
}

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    /// 「今すぐ確認」が押された。呼び出し側が確認を始めて、false に戻す
    pub request_check: bool,
    /// 「更新する」が押された。呼び出し側がダウンロードを始めて、false に戻す
    pub request_install: bool,
    /// 「再起動して更新」が押された。呼び出し側が適用して、false に戻す
    pub request_apply: bool,
    tab: Tab,
    /// 全文を開いているクレジットの添字
    opened_license: Option<usize>,
    /// 入力中の文字。窓を開いたときに設定から作り、閉じたら捨てる
    buf: Option<Buffers>,
    /// 前のフレームで、どれかの部品がフォーカスを持っていた。
    /// Esc は、まず入力欄から抜け、もう一度押すと窓を閉じる
    had_focus: bool,
}

/// 文字入力の編集中の値。
struct Buffers {
    server: String,
    test_server: String,
    wolfx_url: String,
    wolfx_test_url: String,
    loc_name: String,
    lat: String,
    lon: String,
}

impl Buffers {
    fn from_config(c: &Config) -> Self {
        let loc = c.location.as_ref();
        Self {
            server: c.server.clone(),
            test_server: c.test_server.clone(),
            wolfx_url: c.wolfx.url.clone(),
            wolfx_test_url: c.wolfx.test_url.clone(),
            loc_name: loc.map(|l| l.name.clone()).unwrap_or_default(),
            lat: loc.map(|l| l.latitude.to_string()).unwrap_or_default(),
            lon: loc.map(|l| l.longitude.to_string()).unwrap_or_default(),
        }
    }
}

impl SettingsWindow {
    /// 設定が変わったら `true` (呼び出し側が保存する)。
    pub fn show(
        &mut self,
        ctx: &Context,
        config: &mut Config,
        sound: &mut SoundPlayer,
        game_mode: &mut bool,
        link: LinkInfo,
        update: UpdateView<'_>,
    ) -> bool {
        if !self.open {
            // 閉じたら編集中の文字は捨てる (次に開いたとき設定から作り直す)
            self.buf = None;
            self.had_focus = false;
            return false;
        }
        let mut buf = self
            .buf
            .take()
            .unwrap_or_else(|| Buffers::from_config(config));

        let screen = ctx.content_rect();
        let size = WINDOW_SIZE
            .min(screen.size() - vec2(32.0, 32.0))
            .max(vec2(480.0, 320.0))
            .floor();
        let mut changed = false;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !self.had_focus;

        egui::Window::new("設定")
            .id(Id::unique("settings_window"))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .fixed_size(size)
            .default_pos((screen.center() - size / 2.0).round_ui())
            .frame(
                Frame::new()
                    .fill(theme::PANEL)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(WINDOW_RADIUS)
                    .shadow(egui::Shadow {
                        offset: [0, 10],
                        blur: 32,
                        spread: 0,
                        color: Color32::from_black_alpha(120),
                    }),
            )
            .show(ctx, |ui| {
                apply_style(ui);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let side = Rect::from_min_max(rect.min, pos2(rect.min.x + SIDEBAR_W, rect.max.y));
                let header = Rect::from_min_max(
                    pos2(side.max.x, rect.min.y),
                    pos2(rect.max.x, rect.min.y + HEADER_H),
                );
                let body = Rect::from_min_max(pos2(side.max.x, header.max.y), rect.max);

                self.sidebar(ui, side);
                close |= self.header(ui, header);

                let mut page = ui.new_child(
                    UiBuilder::new()
                        .max_rect(body)
                        .layout(Layout::top_down(Align::Min)),
                );
                page.set_clip_rect(body.intersect(ui.clip_rect()));
                ScrollArea::vertical()
                    .id_salt(self.tab)
                    .auto_shrink([false, false])
                    .show(&mut page, |ui| {
                        Frame::new()
                            .inner_margin(Margin {
                                left: PAGE_PAD,
                                right: PAGE_PAD,
                                top: 18,
                                bottom: 2,
                            })
                            .show(ui, |ui| {
                                ui.push_id(self.tab, |ui| match self.tab {
                                    Tab::Connection => {
                                        changed |= connection_page(ui, config, &mut buf, link);
                                    }
                                    Tab::Display => changed |= display_page(ui, config),
                                    Tab::Location => changed |= location_page(ui, config, &mut buf),
                                    Tab::Sound => changed |= sound_page(ui, config, sound),
                                    Tab::Behavior => {
                                        changed |= behavior_page(ui, config, game_mode)
                                    }
                                    Tab::Update => {
                                        changed |= update_page(
                                            ui,
                                            config,
                                            update,
                                            UpdateRequests {
                                                check: &mut self.request_check,
                                                install: &mut self.request_install,
                                                apply: &mut self.request_apply,
                                            },
                                        );
                                    }
                                    Tab::License => self.license_page(ui),
                                });
                            });
                    });
            });

        self.had_focus = ctx.memory(|m| m.focused().is_some());
        if close {
            self.open = false;
            self.had_focus = false;
        } else {
            self.buf = Some(buf);
        }
        changed
    }

    /// 「動作」のページを開く (トレイの「設定」から)。
    pub fn open_behavior(&mut self) {
        self.open = true;
        self.tab = Tab::Behavior;
    }

    /// 左のページ一覧。
    fn sidebar(&mut self, ui: &Ui, rect: Rect) {
        let p = ui.painter().clone();
        // 窓の枠線 (1px) の内側なので、角は窓より 1 小さい
        let r = WINDOW_RADIUS - 1;
        p.rect_filled(
            rect,
            CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            },
            SIDEBAR,
        );
        p.vline(rect.max.x, rect.y_range(), Stroke::new(1.0, theme::BORDER));
        p.text(
            pos2(rect.min.x + 20.0, rect.min.y + HEADER_H / 2.0),
            Align2::LEFT_CENTER,
            "設定",
            theme::bold_font(17.0),
            theme::TEXT,
        );
        let mut y = rect.min.y + HEADER_H;
        for tab in Tab::ALL {
            let item =
                Rect::from_min_size(pos2(rect.min.x + 10.0, y), vec2(rect.width() - 20.0, NAV_H));
            if nav_item(ui, item, tab, self.tab == tab) {
                self.tab = tab;
            }
            y += NAV_H + 4.0;
        }
        p.text(
            pos2(rect.min.x + 20.0, rect.max.y - 20.0),
            Align2::LEFT_CENTER,
            concat!("RSQuake ", env!("CARGO_PKG_VERSION")),
            theme::body_font(11.5),
            theme::TEXT_MUTED,
        );
    }

    /// ページの名前と閉じるボタン。閉じるボタンが押されたら `true`。
    fn header(&self, ui: &Ui, rect: Rect) -> bool {
        let p = ui.painter().clone();
        p.hline(
            rect.x_range(),
            rect.max.y - 0.5,
            Stroke::new(1.0, theme::BORDER),
        );
        p.text(
            pos2(rect.min.x + f32::from(PAGE_PAD), rect.center().y),
            Align2::LEFT_CENTER,
            self.tab.label(),
            theme::bold_font(18.0),
            theme::TEXT,
        );
        let button = Rect::from_center_size(
            pos2(rect.max.x - 16.0 - 15.0, rect.center().y),
            vec2(30.0, 30.0),
        );
        icon_button(ui, button, "settings_close", Icon::X, "閉じる")
    }

    fn license_page(&mut self, ui: &mut Ui) {
        for (i, c) in CREDITS.iter().enumerate() {
            if i > 0 {
                ui.add_space(10.0);
            }
            card_frame()
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(RichText::new(c.name).font(theme::bold_font(14.0)));
                    ui.label(RichText::new(c.usage).size(12.0).color(theme::TEXT_MUTED));
                    ui.add_space(4.0);
                    ui.label(RichText::new(c.license).size(12.5));
                    ui.label(
                        RichText::new(c.copyright)
                            .size(12.0)
                            .color(theme::TEXT_MUTED),
                    );
                    if let Some(url) = c.url {
                        ui.hyperlink_to(RichText::new(url).size(12.0), url);
                    }
                    let Some(text) = c.full_text else {
                        return;
                    };
                    ui.add_space(6.0);
                    let opened = self.opened_license == Some(i);
                    let icon = if opened {
                        Icon::ChevronUp
                    } else {
                        Icon::ChevronDown
                    };
                    if ui.add(icon_text_button(icon, "ライセンス全文")).clicked() {
                        self.opened_license = if opened { None } else { Some(i) };
                    }
                    if opened {
                        ui.add_space(4.0);
                        Frame::new()
                            .fill(theme::BG)
                            .corner_radius(6)
                            .inner_margin(12)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.add(
                                    Label::new(
                                        RichText::new(text.trim_end())
                                            .font(FontId::monospace(11.5)),
                                    )
                                    .selectable(true)
                                    .wrap(),
                                );
                            });
                    }
                });
        }
        ui.add_space(SECTION_GAP);
    }
}

fn connection_page(ui: &mut Ui, config: &mut Config, buf: &mut Buffers, link: LinkInfo) -> bool {
    let mut changed = false;

    section(ui, "受信元", "", |rows| {
        let sources = EewSource::ALL.map(|s| (s, s.label()));
        let disabled: Vec<EewSource> = EewSource::ALL
            .into_iter()
            .filter(|s| !s.selectable())
            .collect();
        changed |= rows.segmented_row(
            "EEW の受信元",
            "切り替えるとその場でつなぎ直します。同時には使いません。",
            &mut config.source,
            &sources,
            &disabled,
        );
        if config.source == EewSource::Wolfx {
            rows.note(
                "Wolfx には長周期地震動階級と「以上」の表記が無く、細分区域が DMDATA より少ないことがあります。",
            );
        }
    });

    section(
        ui,
        "接続先",
        "入力欄を離れる (Enter) と反映して、つなぎ直します。",
        |rows| {
            match config.source {
                EewSource::Dmdata => {
                    changed |= rows.text_row(
                        "接続先",
                        "ws://host:port の形で入力します。",
                        &mut buf.server,
                        &mut config.server,
                        parse_proxy_url,
                    );
                    if Config::DEV_FEATURES {
                        changed |= rows.text_row(
                            "テストモードの接続先",
                            "テストサーバーの ws://host:port です。",
                            &mut buf.test_server,
                            &mut config.test_server,
                            parse_proxy_url,
                        );
                    }
                }
                EewSource::Wolfx => {
                    changed |= rows.text_row(
                        "接続先",
                        "ws:// または wss:// の URL です。",
                        &mut buf.wolfx_url,
                        &mut config.wolfx.url,
                        parse_wolfx_url,
                    );
                    if Config::DEV_FEATURES {
                        changed |= rows.text_row(
                            "テストモードの接続先",
                            "テストサーバーの URL です。",
                            &mut buf.wolfx_test_url,
                            &mut config.wolfx.test_url,
                            parse_wolfx_url,
                        );
                    }
                }
            }
            rows.current_row(&config.ws_url(), link);
        },
    );

    if Config::DEV_FEATURES {
        section(ui, "動作", "", |rows| {
            if rows.toggle_row(
                "テストモード",
                "テストサーバーにつなぎ、電文の時刻を今に合わせます。訓練報・試験報も表示し、画面の左上に「テスト」と出ます。",
                &mut config.test_mode,
            ) {
                // 手で切り替えたら、--test で入った一時的な状態ではなくその値を保存する
                config.saved_test_mode = None;
                changed = true;
            }
            changed |= rows.toggle_row(
                "受信した電文をログに保存",
                "保存した JSONL は --replay でそのまま再生できます。",
                &mut config.log_telegrams,
            );
        });
    }
    changed
}

fn display_page(ui: &mut Ui, config: &mut Config) -> bool {
    let mut changed = false;

    section(ui, "予想震度パネル", "", |rows| {
        let e = &mut config.eew;
        changed |= rows.segmented_row(
            "細分区域の表示",
            match e.region_display {
                RegionDisplay::Detailed => "区域ごとに予想震度を出します。",
                RegionDisplay::Summary => {
                    "警報の地域だけを、震度なしで出します。区域がそろった府県や地方は短い名前にまとめます。"
                }
            },
            &mut e.region_display,
            &[
                (RegionDisplay::Detailed, "詳細"),
                (RegionDisplay::Summary, "簡易"),
            ],
            &[],
        );
        let mut secs = f64::from(e.switch_interval_secs);
        if rows.slider_row(
            "複数の EEW を切り替える間隔",
            "新しいイベント、初めての警報、取消が出たときは、間隔を待たずに切り替えます。",
            &mut secs,
            f64::from(MIN_SWITCH_SECS)..=f64::from(MAX_SWITCH_SECS),
            |v| format!("{v:.0} 秒"),
        ) {
            e.switch_interval_secs = secs as u32;
            changed = true;
        }
    });

    section(ui, "地図", "", |rows| {
        changed |= rows.toggle_row(
            "自動でズームする",
            "予報円が収まる範囲へ寄せます。手で動かすと止まり、10 秒後に再開します。",
            &mut config.eew.auto_zoom,
        );
    });

    section(ui, "電文", "", |rows| {
        changed |= rows.toggle_row(
            "訓練報・試験報を表示",
            "オンにすると、情報パネルの帯に「訓練」「試験」と出ます。",
            &mut config.show_training,
        );
    });
    changed
}

/// 指定地点の入力欄。3 欄とも空なら設定なし。緯度経度が数字で、範囲に収まったときに反映する。
fn location_page(ui: &mut Ui, config: &mut Config, buf: &mut Buffers) -> bool {
    let mut edited = false;
    let mut clear = false;

    section(
        ui,
        "地点",
        "予想パネルに、この地点までの距離と P 波・S 波の到達状況を出します。空にすると出しません。",
        |rows| {
            edited |= rows.row(
                "名前",
                "空のときは「指定地点」と出します。",
                FIELD_W,
                |ui| field(ui, "name", &mut buf.loc_name, FIELD_W, "", false).lost_focus(),
            );
            edited |= rows.row(
                "緯度・経度",
                "度の小数で入力します。北緯と東経が正です。",
                FIELD_W,
                |ui| {
                    // 行の部品は右から並ぶ。Tab で緯度→経度の順に移れるよう、ここだけ左から並べる
                    ui.allocate_ui_with_layout(
                        vec2(FIELD_W, ui.spacing().interact_size.y),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            let w = (FIELD_W - CONTROL_SPACING) / 2.0;
                            let lat = field(ui, "lat", &mut buf.lat, w, "緯度", true).lost_focus();
                            let lon = field(ui, "lon", &mut buf.lon, w, "経度", true).lost_focus();
                            lat | lon
                        },
                    )
                    .inner
                },
            );

            let empty = buf.loc_name.trim().is_empty()
                && buf.lat.trim().is_empty()
                && buf.lon.trim().is_empty();
            let (icon, color, text) = match parse_location(&buf.loc_name, &buf.lat, &buf.lon) {
                Err(why) => (Icon::TriangleAlert, theme::WARNING, why.to_string()),
                Ok(Some(l)) => (
                    Icon::MapPin,
                    theme::OK,
                    format!("{} ({:.4}, {:.4})", l.name, l.latitude, l.longitude),
                ),
                Ok(None) => (Icon::MapPin, theme::TEXT_MUTED, "設定なし".to_string()),
            };
            clear = rows.status_row(icon, color, &text, BUTTON_W, |ui| {
                ui.add_enabled(!empty, icon_text_button(Icon::Trash2, "消去"))
                    .clicked()
            });
        },
    );

    if clear {
        buf.loc_name.clear();
        buf.lat.clear();
        buf.lon.clear();
        edited = true;
    }
    if edited
        && let Ok(loc) = parse_location(&buf.loc_name, &buf.lat, &buf.lon)
        && config.location != loc
    {
        config.location = loc;
        return true;
    }
    false
}

fn sound_page(ui: &mut Ui, config: &mut Config, sound: &mut SoundPlayer) -> bool {
    let mut changed = false;

    section(ui, "音量", "", |rows| {
        let mut percent = f64::from((config.sound.volume() * 100.0).round());
        if rows.slider_row(
            "全体の音量",
            "0% にすると、試聴も含めて鳴りません。",
            &mut percent,
            0.0..=100.0,
            |v| format!("{v:.0}%"),
        ) {
            config.sound.volume = ((percent / 100.0) as f32).clamp(0.0, 1.0);
            sound.set_volume(config.sound.volume);
            changed = true;
        }
    });

    section(ui, "鳴らす音", "", |rows| {
        for cue in SoundCue::ALL {
            let audible = config.sound.volume() > 0.0;
            let mut play = false;
            changed |= rows.toggle_row_with(
                cue.label(),
                cue_description(cue),
                flag(&mut config.sound, cue),
                BUTTON_W,
                |ui| {
                    play = ui
                        .add_enabled(audible, icon_text_button(Icon::Play, "試聴"))
                        .clicked();
                },
            );
            if play {
                sound.preview(cue, &config.sound);
            }
        }
    });
    changed
}

/// 常駐、EEW 発生時のウィンドウ表示、ゲームモード。ゲームモードは設定ファイルに保存しないので、
/// 戻り値 (設定が変わったか) には含めない。
fn behavior_page(ui: &mut Ui, config: &mut Config, game_mode: &mut bool) -> bool {
    let mut changed = false;
    let w = &mut config.window;
    let note = if *game_mode {
        " (ゲームモード中は無効)"
    } else {
        ""
    };

    if crate::resident::tray::SUPPORTED {
        section(ui, "常駐", "", |rows| {
            changed |= rows.toggle_row(
                "閉じたときにトレイに常駐する",
                &format!(
                    "ウィンドウを閉じても終了せず、トレイ (メニューバー) から開き直せます。EEW の受信は続きます。{note}"
                ),
                &mut w.resident,
            );
        });
    }

    section(ui, "EEW 発生時の表示", "", |rows| {
        changed |= rows.toggle_row(
            "ウィンドウを表示する",
            &format!(
                "EEW が出たら、隠れているウィンドウや最小化したウィンドウを前に出します。{note}"
            ),
            &mut w.popup.enabled,
        );
        let on = w.popup.enabled;
        rows.disabled_unless(on, |rows| {
            changed |= rows.toggle_row("予報で表示", "", &mut w.popup.forecast);
            changed |= rows.toggle_row("警報で表示", "", &mut w.popup.warning);
            changed |= rows.toggle_row(
                "EEW が終わったら元に戻す",
                "自動で表示したウィンドウを、隠れていた状態、または最小化した状態に戻します。ウィンドウを操作すると戻しません。",
                &mut w.popup.hide_after,
            );
        });
    });

    section(ui, "ゲームモード", "", |rows| {
        rows.toggle_row(
            "ゲームモード",
            "有効の間は、常駐と EEW 発生時の表示を止めます。起動するたびに無効に戻ります。",
            game_mode,
        );
    });
    changed
}

/// アップデートのページで押されたボタン。
struct UpdateRequests<'a> {
    check: &'a mut bool,
    install: &'a mut bool,
    apply: &'a mut bool,
}

/// ダウンロードの進行状況の文。
fn progress_text(done: u64, total: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if total > 0 {
        format!(
            "ダウンロードしています ({:.1} / {:.1} MB)",
            done as f64 / MB,
            total as f64 / MB
        )
    } else {
        format!("ダウンロードしています ({:.1} MB)", done as f64 / MB)
    }
}

/// アップデートの設定と、確認・更新の状態。
/// アプリ内で更新できない環境では、ダウンロードとリリースノートは GitHub のページで見てもらう。
fn update_page(
    ui: &mut Ui,
    config: &mut Config,
    view: UpdateView<'_>,
    mut req: UpdateRequests<'_>,
) -> bool {
    let mut changed = false;

    section(
        ui,
        "確認",
        "新しい版があれば、起動時にポップアップで知らせます。EEW が出ている間は出しません。",
        |rows| {
            changed |= rows.toggle_row(
                "起動時に新しい版を確認",
                "GitHub Releases に問い合わせます。",
                &mut config.update.check,
            );
            changed |= rows.toggle_row(
                "プレリリースを含める",
                "ベータ版などのプレリリースも、新しい版として扱います。",
                &mut config.update.include_prerelease,
            );
            changed |= rows.toggle_row(
                "自動で更新",
                "新しい版を見つけたら、確認せずにダウンロードして更新します。EEW が出ている間は、終わるまで再起動を待ちます。",
                &mut config.update.auto_apply,
            );
        },
    );

    section(ui, "状態", "", |rows| {
        let checking = view.info.phase == Phase::Checking;
        let latest = view.info.result.latest.as_ref();
        let (icon, color, text) = match (&view.info.phase, latest) {
            (Phase::Checking, _) => (
                Icon::RefreshCw,
                theme::TEXT_MUTED,
                "確認しています".to_string(),
            ),
            (Phase::Failed, _) => (
                Icon::TriangleAlert,
                theme::WARNING,
                "確認できませんでした。ネットワークを確認してください。".to_string(),
            ),
            (Phase::Done, Some(r)) => (
                Icon::Download,
                theme::FORECAST,
                format!("新しいバージョン v{} が公開されています", r.version),
            ),
            (Phase::Done, None) => (
                Icon::Check,
                theme::OK,
                format!("最新です (v{})", view.current),
            ),
            (Phase::Idle, _) => (
                Icon::Info,
                theme::TEXT_MUTED,
                if config.update.check {
                    format!("まだ確認していません (v{})", view.current)
                } else {
                    format!("起動時の確認はオフです (v{})", view.current)
                },
            ),
        };
        rows.status_row(icon, color, &text, 120.0, |ui| {
            if ui
                .add_enabled(!checking, icon_text_button(Icon::RefreshCw, "今すぐ確認"))
                .clicked()
            {
                *req.check = true;
            }
        });
        if let Some(r) = latest {
            update_rows(rows, r, &view, &mut req);
        }
        rows.row(
            "リリースノート",
            "変更点は GitHub の Release のページに書かれています。",
            140.0,
            |ui| {
                if ui
                    .add(icon_text_button(Icon::ExternalLink, "GitHub で開く"))
                    .clicked()
                {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(update::RELEASES_PAGE));
                }
            },
        );
    });
    changed
}

/// 新しい版があるときの、更新の行。アプリ内で更新できるときは、進み具合とボタンを出す。
fn update_rows(
    rows: &mut Rows<'_>,
    r: &update::Release,
    view: &UpdateView<'_>,
    req: &mut UpdateRequests<'_>,
) {
    use update::Install;
    let open_github = |rows: &mut Rows<'_>, desc: &str| {
        rows.row("ダウンロード", desc, 140.0, |ui| {
            if ui
                .add(icon_text_button(Icon::ExternalLink, "GitHub で開く"))
                .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(&r.url));
            }
        });
    };
    match &view.info.plan {
        Some(Ok(_)) => {}
        Some(Err(why)) => {
            // アプリ内では更新できない。理由を出して、GitHub のページへ案内する
            open_github(rows, &format!("このアプリでは自動で更新できません。{why}"));
            return;
        }
        None => {
            open_github(rows, "GitHub の Release のページが開きます。");
            return;
        }
    }
    match &view.info.install {
        Install::None => {
            rows.row(
                "更新",
                "ダウンロードして、署名を確かめてから更新します。",
                140.0,
                |ui| {
                    if ui
                        .add(icon_text_button(Icon::Download, "更新する"))
                        .clicked()
                    {
                        *req.install = true;
                    }
                },
            );
        }
        Install::Downloading { done, total } => {
            rows.status_row(
                Icon::Download,
                theme::FORECAST,
                &progress_text(*done, *total),
                0.0,
                |_| {},
            );
            let frac = if *total > 0 {
                *done as f32 / *total as f32
            } else {
                0.0
            };
            rows.progress_row(frac);
        }
        Install::Verifying => {
            rows.status_row(
                Icon::RefreshCw,
                theme::TEXT_MUTED,
                "署名とハッシュを確認しています",
                0.0,
                |_| {},
            );
        }
        Install::Ready => {
            rows.row(
                "更新",
                "確認が済みました。再起動して、新しい版にします。",
                170.0,
                |ui| {
                    if ui
                        .add(icon_text_button(Icon::RotateCw, "再起動して更新"))
                        .clicked()
                    {
                        *req.apply = true;
                    }
                },
            );
        }
        Install::WaitingForEew => {
            rows.status_row(
                Icon::Clock,
                theme::WARNING,
                "緊急地震速報が終わったら更新します",
                0.0,
                |_| {},
            );
        }
        Install::Applying => {
            rows.status_row(
                Icon::RotateCw,
                theme::TEXT_MUTED,
                "更新しています。まもなく再起動します",
                0.0,
                |_| {},
            );
        }
        Install::Failed(why) => {
            rows.status_row(
                Icon::TriangleAlert,
                theme::WARNING,
                &format!("更新できませんでした。{why}"),
                140.0,
                |ui| {
                    if ui
                        .add(icon_text_button(Icon::ExternalLink, "GitHub で開く"))
                        .clicked()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&r.url));
                    }
                },
            );
            rows.row(
                "更新",
                "もう一度ダウンロードします。",
                140.0,
                |ui| {
                    if ui
                        .add(icon_text_button(Icon::Download, "やり直す"))
                        .clicked()
                    {
                        *req.install = true;
                    }
                },
            );
        }
    }
}

fn flag(cfg: &mut SoundConfig, cue: SoundCue) -> &mut bool {
    match cue {
        SoundCue::EewForecast => &mut cfg.eew_forecast,
        SoundCue::EewWarning => &mut cfg.eew_warning,
    }
}

/// いつ鳴るか (`sound::policy::cue_for_eew` に合わせる)。
fn cue_description(cue: SoundCue) -> &'static str {
    match cue {
        SoundCue::EewForecast => "新しい地震の予報を受信したときに鳴らします。",
        SoundCue::EewWarning => "警報を受信したとき、予報が警報に変わったときに鳴らします。",
    }
}

/// 接続の状態を表す言葉と色。
fn link_label(l: LinkInfo) -> (&'static str, Color32) {
    if l.replay {
        return ("リプレイ中", theme::TRAINING);
    }
    match (l.link, l.upstream) {
        (LinkState::Disconnected, _) => ("接続不可", theme::WARNING),
        (LinkState::Connecting, _) => ("接続中", theme::FORECAST),
        (LinkState::Connected, Upstream::Disconnected) => ("上流に接続不可", theme::WARNING),
        (LinkState::Connected, _) => ("接続済み", theme::OK),
    }
}

/// DMDATA (proxy) の接続先。`ws://host:port` まで。パスとクエリは固定なので入れさせない。
pub fn parse_proxy_url(text: &str) -> Result<String, &'static str> {
    let t = text.trim().trim_end_matches('/');
    let host = t
        .strip_prefix("ws://")
        .ok_or("ws://host:port の形で入力してください (wss:// は使えません)")?;
    if host.is_empty() || host.contains(['/', '?', '#', ' ']) {
        return Err("ホストとポートだけを入力してください (パスは固定です)");
    }
    if let Some((h, port)) = host.rsplit_once(':')
        && (h.is_empty() || port.parse::<u16>().is_err())
    {
        return Err("ポート番号が正しくありません");
    }
    Ok(t.to_string())
}

/// Wolfx の接続先。`ws://` か `wss://` の URL そのまま (パスも含む)。
pub fn parse_wolfx_url(text: &str) -> Result<String, &'static str> {
    let t = text.trim();
    let rest = t
        .strip_prefix("wss://")
        .or_else(|| t.strip_prefix("ws://"))
        .ok_or("ws:// か wss:// で始まる URL を入力してください")?;
    if rest.is_empty() || rest.starts_with('/') || t.contains(' ') {
        return Err("URL が正しくありません");
    }
    Ok(t.to_string())
}

/// 指定地点。3 欄とも空なら「設定なし」。緯度経度は度 (小数)。
pub fn parse_location(name: &str, lat: &str, lon: &str) -> Result<Option<Location>, &'static str> {
    let (name, lat, lon) = (name.trim(), lat.trim(), lon.trim());
    if name.is_empty() && lat.is_empty() && lon.is_empty() {
        return Ok(None);
    }
    let latitude: f64 = lat.parse().map_err(|_| "緯度を数字で入力してください")?;
    let longitude: f64 = lon.parse().map_err(|_| "経度を数字で入力してください")?;
    if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
        return Err("緯度は -90〜90 の範囲です");
    }
    if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
        return Err("経度は -180〜180 の範囲です");
    }
    Ok(Some(Location {
        name: if name.is_empty() {
            "指定地点"
        } else {
            name
        }
        .to_string(),
        latitude,
        longitude,
    }))
}

/// 設定画面の中だけで使う見た目 (余白、ボタン、入力欄)。
fn apply_style(ui: &mut Ui) {
    let style = ui.style_mut();
    style.spacing.item_spacing = vec2(CONTROL_SPACING, 6.0);
    style.spacing.button_padding = vec2(12.0, 6.0);
    style.spacing.interact_size = vec2(40.0, 30.0);

    let v = &mut style.visuals;
    v.selection.bg_fill = TEXT_SELECTION;
    v.selection.stroke = Stroke::new(1.0, theme::ACCENT);
    v.hyperlink_color = LINK;
    v.text_edit_bg_color = Some(theme::BG);
    let w = &mut v.widgets;
    for (state, fill, border) in [
        (&mut w.inactive, BUTTON, theme::BORDER),
        (&mut w.hovered, BUTTON_HOVER, BORDER_HOVER),
        (&mut w.active, BUTTON_ACTIVE, BORDER_HOVER),
        (&mut w.open, BUTTON_ACTIVE, BORDER_HOVER),
    ] {
        state.weak_bg_fill = fill;
        state.bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, border);
        state.fg_stroke = Stroke::new(1.0, theme::TEXT);
        state.corner_radius = CornerRadius::same(6);
        state.expansion = 0.0;
    }
    w.noninteractive.corner_radius = CornerRadius::same(6);
}

fn card_frame() -> Frame {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, DIVIDER))
        .corner_radius(theme::RADIUS)
}

/// 見出しと、項目をまとめたカード。`caption` はカードの下に出す補足 (空なら出さない)。
fn section(ui: &mut Ui, title: &str, caption: &str, add: impl FnOnce(&mut Rows<'_>)) {
    ui.label(
        RichText::new(title)
            .font(theme::bold_font(12.5))
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(8.0);
    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        add(&mut Rows { ui, first: true });
    });
    if !caption.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new(caption).size(12.0).color(theme::TEXT_MUTED));
    }
    ui.add_space(SECTION_GAP);
}

/// 行ごとの部品の ID。行の出し入れ (Wolfx の注記など) で他の行の ID がずれないよう、名前から作る。
fn row_id(title: &str) -> Id {
    Id::unique("settings_row").with(title)
}

/// カードの中身。行を上から積み、行の間に線を引く。
struct Rows<'a> {
    ui: &'a mut Ui,
    first: bool,
}

impl Rows<'_> {
    fn divider(&mut self) {
        if std::mem::take(&mut self.first) {
            return;
        }
        let (rect, _) = self
            .ui
            .allocate_exact_size(vec2(self.ui.available_width(), 1.0), Sense::hover());
        let line = Rect::from_min_max(pos2(rect.min.x + ROW_PAD.x, rect.min.y), rect.max);
        self.ui.painter().rect_filled(line, 0.0, DIVIDER);
    }

    /// 左に項目名と説明、右に操作する部品 (幅 `control_w`) を置く 1 行。
    /// 部品は右から並ぶ。
    fn row<R>(
        &mut self,
        title: &str,
        desc: &str,
        control_w: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.row_with(
            title,
            (desc, theme::body_font(12.0), theme::TEXT_MUTED),
            control_w,
            add,
        )
    }

    /// `row` の、説明の書体と色を変えられる版。
    fn row_with<R>(
        &mut self,
        title: &str,
        (desc, desc_font, desc_color): (&str, FontId, Color32),
        control_w: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.divider();
        let ui = &mut *self.ui;
        let width = ui.available_width();
        let text_w = (width - ROW_PAD.x * 2.0 - control_w - CONTROL_GAP).max(80.0);
        let p = ui.painter().clone();
        let title_g = p.layout(
            title.to_owned(),
            theme::body_font(14.0),
            theme::TEXT,
            text_w,
        );
        let desc_g =
            (!desc.is_empty()).then(|| p.layout(desc.to_owned(), desc_font, desc_color, text_w));
        let title_h = title_g.size().y;
        let text_h = title_h + desc_g.as_ref().map_or(0.0, |g| LINE_GAP + g.size().y);
        let h = (text_h + ROW_PAD.y * 2.0).max(ROW_MIN_H).ceil();
        let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
        if ui.is_rect_visible(rect) {
            let pos = pos2(rect.min.x + ROW_PAD.x, rect.center().y - text_h / 2.0);
            p.galley(pos, title_g, theme::TEXT);
            if let Some(g) = desc_g {
                p.galley(pos + vec2(0.0, title_h + LINE_GAP), g, desc_color);
            }
        }
        let control = Rect::from_min_max(
            pos2(rect.max.x - ROW_PAD.x - control_w, rect.min.y),
            pos2(rect.max.x - ROW_PAD.x, rect.max.y),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .scope_id(row_id(title))
                .max_rect(control)
                .layout(Layout::right_to_left(Align::Center)),
        );
        add(&mut child)
    }

    /// `enabled` が偽の間、中の行を操作できなくする。
    fn disabled_unless(&mut self, enabled: bool, add: impl FnOnce(&mut Rows<'_>)) {
        let first = std::mem::replace(&mut self.first, false);
        self.ui.add_enabled_ui(enabled, |ui| {
            add(&mut Rows { ui, first });
        });
    }

    fn toggle_row(&mut self, title: &str, desc: &str, on: &mut bool) -> bool {
        self.toggle_row_with(title, desc, on, 0.0, |_| {})
    }

    /// スイッチの左に、もう 1 つ部品 (幅 `extra_w`) を置く行。
    fn toggle_row_with(
        &mut self,
        title: &str,
        desc: &str,
        on: &mut bool,
        extra_w: f32,
        extra: impl FnOnce(&mut Ui),
    ) -> bool {
        let id = row_id(title).with("toggle");
        let extra_w = if extra_w > 0.0 {
            extra_w + CONTROL_SPACING
        } else {
            0.0
        };
        self.row(title, desc, TOGGLE_SIZE.x + extra_w, |ui| {
            let changed = toggle(ui, id, title, on).changed();
            extra(ui);
            changed
        })
    }

    /// いくつかの中から 1 つを選ぶ行。
    fn segmented_row<T: Copy + PartialEq>(
        &mut self,
        title: &str,
        desc: &str,
        value: &mut T,
        options: &[(T, &str)],
        disabled: &[T],
    ) -> bool {
        let p = self.ui.painter().clone();
        let widest = options
            .iter()
            .map(|(_, label)| {
                p.layout_no_wrap(label.to_string(), segment_font(), theme::TEXT)
                    .size()
                    .x
            })
            .fold(0.0, f32::max);
        // どの選択肢も同じ幅にする
        let segment_w = (widest + 32.0).max(72.0).ceil();
        let id = row_id(title).with("segment");
        let w = segment_w * options.len() as f32 + SEGMENT_PAD * 2.0;
        self.row(title, desc, w, |ui| {
            segmented(ui, id, value, options, disabled, segment_w)
        })
    }

    /// 整数きざみのスライダーと、その値。
    fn slider_row(
        &mut self,
        title: &str,
        desc: &str,
        value: &mut f64,
        range: RangeInclusive<f64>,
        format: impl Fn(f64) -> String,
    ) -> bool {
        let id = row_id(title).with("slider");
        self.row(title, desc, SLIDER_W + CONTROL_SPACING + VALUE_W, |ui| {
            let (_, value_rect) = ui.allocate_space(vec2(VALUE_W, 20.0));
            let changed = slider(ui, id, title, value, range).changed();
            ui.painter().text(
                value_rect.right_center(),
                Align2::RIGHT_CENTER,
                format(*value),
                theme::body_font(13.0),
                theme::TEXT,
            );
            changed
        })
    }

    /// 文字を入れる行。入力欄を離れたときに `parse` で検証し、通れば `target` へ反映する。
    /// 不正なら説明の代わりに理由を赤く出し、設定は変えない。
    fn text_row(
        &mut self,
        title: &str,
        desc: &str,
        text: &mut String,
        target: &mut String,
        parse: fn(&str) -> Result<String, &'static str>,
    ) -> bool {
        let (desc, color) = match parse(text) {
            Ok(_) => (desc, theme::TEXT_MUTED),
            Err(why) => (why, theme::WARNING),
        };
        let resp = self.row_with(
            title,
            (desc, theme::body_font(12.0), color),
            FIELD_W,
            |ui| field(ui, "field", text, FIELD_W, "", true),
        );
        if resp.lost_focus()
            && let Ok(v) = parse(text)
        {
            // 前後の空白などを落とした形に直す
            text.clone_from(&v);
            if *target != v {
                *target = v;
                return true;
            }
        }
        false
    }

    /// いまの接続先と、その状態。
    fn current_row(&mut self, url: &str, link: LinkInfo) {
        let (label, color) = link_label(link);
        self.row_with(
            "現在の接続先",
            (url, FontId::monospace(12.0), theme::TEXT_MUTED),
            STATUS_W,
            |ui| {
                let p = ui.painter().clone();
                let g = p.layout_no_wrap(label.to_owned(), theme::bold_font(12.5), color);
                let (_, rect) = ui.allocate_space(vec2(g.size().x + 14.0, 20.0));
                p.circle_filled(pos2(rect.min.x + 4.0, rect.center().y), 4.0, color);
                p.galley(
                    pos2(rect.min.x + 14.0, rect.center().y - g.size().y / 2.0),
                    g,
                    color,
                );
            },
        );
    }

    /// 補足の行 (操作する部品なし)。
    fn note(&mut self, text: &str) {
        self.message_row(
            "note",
            Icon::Info,
            theme::TEXT_MUTED,
            theme::TEXT_MUTED,
            text,
            0.0,
            |_| {},
        );
    }

    /// アイコンと文で状態を出し、右に部品 (幅 `control_w`) を置く行。
    /// 進み具合のバーだけの行。
    fn progress_row(&mut self, fraction: f32) {
        self.divider();
        let ui = &mut *self.ui;
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
        let bar = Rect::from_min_max(
            pos2(rect.min.x + ROW_PAD.x, rect.center().y - 2.0),
            pos2(rect.max.x - ROW_PAD.x, rect.center().y + 2.0),
        );
        let p = ui.painter();
        p.rect_filled(bar, 2.0, DIVIDER);
        let fill = Rect::from_min_size(
            bar.min,
            vec2(bar.width() * fraction.clamp(0.0, 1.0), bar.height()),
        );
        p.rect_filled(fill, 2.0, theme::ACCENT);
    }

    fn status_row<R>(
        &mut self,
        icon: Icon,
        color: Color32,
        text: &str,
        control_w: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.message_row("status", icon, color, theme::TEXT, text, control_w, add)
    }

    #[allow(clippy::too_many_arguments)]
    fn message_row<R>(
        &mut self,
        key: &str,
        icon: Icon,
        icon_color: Color32,
        text_color: Color32,
        text: &str,
        control_w: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        const ICON_W: f32 = 24.0;
        self.divider();
        let ui = &mut *self.ui;
        let width = ui.available_width();
        let gap = if control_w > 0.0 { CONTROL_GAP } else { 0.0 };
        let text_w = (width - ROW_PAD.x * 2.0 - ICON_W - control_w - gap).max(80.0);
        let p = ui.painter().clone();
        let font = theme::body_font(12.5);
        let line_h = p.fonts_mut(|f| f.row_height(&font));
        let g = p.layout(text.to_owned(), font, text_color, text_w);
        let text_h = g.size().y;
        let min_h = if control_w > 0.0 { ROW_MIN_H } else { 0.0 };
        let h = (text_h + ROW_PAD.y * 2.0).max(min_h).ceil();
        let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
        if ui.is_rect_visible(rect) {
            let top = rect.center().y - text_h / 2.0;
            // アイコンは 1 行目の高さに合わせる
            p.text(
                pos2(rect.min.x + ROW_PAD.x, top + line_h / 2.0),
                Align2::LEFT_CENTER,
                icon.unicode(),
                theme::icon_font(15.0),
                icon_color,
            );
            p.galley(pos2(rect.min.x + ROW_PAD.x + ICON_W, top), g, text_color);
        }
        let control = Rect::from_min_max(
            pos2(rect.max.x - ROW_PAD.x - control_w, rect.min.y),
            pos2(rect.max.x - ROW_PAD.x, rect.max.y),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .scope_id(row_id(key))
                .max_rect(control)
                .layout(Layout::right_to_left(Align::Center)),
        );
        add(&mut child)
    }
}

/// フォーカスを持っている部品の枠。
fn focus_ring(ui: &Ui, rect: Rect, radius: f32) {
    ui.painter().rect_stroke(
        rect.expand(2.0),
        radius + 2.0,
        Stroke::new(1.5, LINK),
        StrokeKind::Outside,
    );
}

/// 左のページ一覧の 1 項目。押されたら `true`。
fn nav_item(ui: &Ui, rect: Rect, tab: Tab, selected: bool) -> bool {
    let resp = ui
        .interact(rect, Id::unique("settings_nav").with(tab), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    resp.widget_info(|| WidgetInfo::selected(Role::Tab, true, selected, tab.label()));
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 6.0, NAV_SELECTED);
        let bar = Rect::from_center_size(pos2(rect.min.x + 1.5, rect.center().y), vec2(3.0, 16.0));
        p.rect_filled(bar, 1.5, theme::ACCENT);
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, NAV_HOVER);
    }
    if resp.has_focus() {
        focus_ring(ui, rect, 6.0);
    }
    let color = if selected || resp.hovered() {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    p.text(
        pos2(rect.min.x + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        tab.icon().unicode(),
        theme::icon_font(16.0),
        color,
    );
    p.text(
        pos2(rect.min.x + 40.0, rect.center().y),
        Align2::LEFT_CENTER,
        tab.label(),
        if selected {
            theme::bold_font(13.5)
        } else {
            theme::body_font(13.5)
        },
        color,
    );
    resp.clicked()
}

/// アイコンだけのボタン。押されたら `true`。
fn icon_button(ui: &Ui, rect: Rect, id: &str, icon: Icon, label: &str) -> bool {
    let resp = ui
        .interact(rect, Id::unique(id), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(rect, 6.0, BUTTON_ACTIVE);
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, BUTTON_HOVER);
    }
    if resp.has_focus() {
        focus_ring(ui, rect, 6.0);
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon.unicode(),
        theme::icon_font(17.0),
        if resp.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        },
    );
    resp.on_hover_text(label).clicked()
}

/// アイコンと文字のボタン。
fn icon_text_button(icon: Icon, text: &str) -> Button<'static> {
    Button::new((
        RichText::new(icon.unicode()).font(theme::icon_font(14.0)),
        RichText::new(text).size(13.0),
    ))
}

/// オン・オフのスイッチ。
fn toggle(ui: &mut Ui, id: Id, label: &str, on: &mut bool) -> Response {
    let (_, rect) = ui.allocate_space(TOGGLE_SIZE);
    let mut resp = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let state = *on;
    resp.widget_info(|| WidgetInfo::selected(Role::Switch, true, state, label));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(id, state);
        let mut track = TOGGLE_OFF.lerp_to_gamma(theme::ACCENT, t);
        if resp.hovered() {
            track = track.lerp_to_gamma(Color32::WHITE, 0.1);
        }
        let radius = rect.height() / 2.0;
        ui.painter().rect_filled(rect, radius, track);
        let knob = radius - 3.0;
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        ui.painter()
            .circle_filled(pos2(x, rect.center().y), knob, Color32::WHITE);
        if resp.has_focus() {
            focus_ring(ui, rect, radius);
        }
    }
    resp
}

fn segment_font() -> FontId {
    theme::body_font(13.0)
}

/// 横に並べた選択肢。選び直したら `true`。
fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: Id,
    value: &mut T,
    options: &[(T, &str)],
    disabled: &[T],
    segment_w: f32,
) -> bool {
    let size = vec2(
        segment_w * options.len() as f32 + SEGMENT_PAD * 2.0,
        SEGMENT_H,
    );
    let (_, rect) = ui.allocate_space(size);
    let segment = |i: usize| {
        Rect::from_min_size(
            pos2(
                rect.min.x + SEGMENT_PAD + segment_w * i as f32,
                rect.min.y + SEGMENT_PAD,
            ),
            vec2(segment_w, SEGMENT_H - SEGMENT_PAD * 2.0),
        )
    };

    // 先に操作を全部見てから描く (押した瞬間に 2 つが選ばれて見えないように)
    let mut changed = false;
    let responses: Vec<Response> = options
        .iter()
        .enumerate()
        .map(|(i, (v, _))| {
            if disabled.contains(v) {
                return ui.interact(segment(i), id.with(i), Sense::hover());
            }
            let resp = ui
                .interact(segment(i), id.with(i), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            if resp.clicked() && *value != *v {
                *value = *v;
                changed = true;
            }
            resp
        })
        .collect();

    let p = ui.painter();
    p.rect_filled(rect, 7.0, theme::BG);
    for (i, ((v, label), resp)) in options.iter().zip(&responses).enumerate() {
        let is_disabled = disabled.contains(v);
        let selected = *value == *v && !is_disabled;
        resp.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, *label));
        let r = segment(i);
        if selected {
            p.rect_filled(r, 5.0, SEGMENT_SELECTED);
        } else if resp.hovered() && !is_disabled {
            p.rect_filled(r, 5.0, NAV_HOVER);
        }
        if resp.has_focus() && !is_disabled {
            focus_ring(ui, r, 5.0);
        }
        p.text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            segment_font(),
            if is_disabled {
                theme::TEXT_MUTED.gamma_multiply(0.5)
            } else if selected || resp.hovered() {
                theme::TEXT
            } else {
                theme::TEXT_MUTED
            },
        );
    }
    changed
}

/// 整数きざみのスライダー。ドラッグするか、フォーカスして左右キーで動かす。
fn slider(
    ui: &mut Ui,
    id: Id,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> Response {
    const HANDLE_R: f32 = 8.0;
    let (_, rect) = ui.allocate_space(vec2(SLIDER_W, 24.0));
    let mut resp = ui
        .interact(rect, id, Sense::drag())
        .on_hover_cursor(CursorIcon::PointingHand);
    let (lo, hi) = (*range.start(), *range.end());
    // つまみの中心が動く範囲
    let xs = (rect.left() + HANDLE_R)..=(rect.right() - HANDLE_R);

    let old = *value;
    if let Some(pointer) = resp.interact_pointer_pos() {
        let t = egui::remap_clamp(pointer.x, xs.clone(), 0.0..=1.0);
        *value = (lo + (hi - lo) * f64::from(t)).round();
    }
    if resp.has_focus() {
        // 左右キーをフォーカス移動に取られないようにする
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    ..Default::default()
                },
            );
        });
        let steps = ui.input(|i| {
            i.num_presses(egui::Key::ArrowRight) as f64 - i.num_presses(egui::Key::ArrowLeft) as f64
        });
        *value += steps;
    }
    *value = value.clamp(lo, hi);
    if *value != old {
        resp.mark_changed();
    }
    let shown = *value;
    resp.widget_info(|| WidgetInfo::slider(true, shown, label));

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let t = if hi > lo {
            ((shown - lo) / (hi - lo)) as f32
        } else {
            0.0
        };
        let center = pos2(egui::lerp(xs, t), rect.center().y);
        let rail = Rect::from_center_size(rect.center(), vec2(rect.width(), 6.0));
        p.rect_filled(rail, 3.0, TOGGLE_OFF);
        p.rect_filled(
            Rect::from_min_max(rail.min, pos2(center.x, rail.max.y)),
            3.0,
            theme::ACCENT,
        );
        if resp.has_focus() {
            p.circle_stroke(center, HANDLE_R + 3.0, Stroke::new(1.5, LINK));
        }
        let grow = if resp.hovered() || resp.dragged() {
            1.0
        } else {
            0.0
        };
        p.circle_filled(center, HANDLE_R + grow, Color32::WHITE);
    }
    resp
}

/// 1 行の入力欄。`mono` は等幅 (URL や数字)。
fn field(
    ui: &mut Ui,
    id_salt: &str,
    text: &mut String,
    width: f32,
    hint: &'static str,
    mono: bool,
) -> Response {
    ui.add(
        TextEdit::singleline(text)
            .id_salt(id_salt)
            .desired_width(width)
            .margin(Margin::symmetric(10, 6))
            .hint_text(hint)
            .font(if mono {
                FontId::monospace(13.0)
            } else {
                theme::body_font(13.0)
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_url_accepts_host_and_port_only() {
        assert_eq!(
            parse_proxy_url(" ws://localhost:3000/ "),
            Ok("ws://localhost:3000".into())
        );
        assert_eq!(
            parse_proxy_url("ws://127.0.0.1:8080"),
            Ok("ws://127.0.0.1:8080".into())
        );
        assert_eq!(
            parse_proxy_url("ws://example.com"),
            Ok("ws://example.com".into())
        );
        assert!(parse_proxy_url("http://x:1").is_err());
        assert!(parse_proxy_url("wss://x:1").is_err());
        assert!(parse_proxy_url("ws://").is_err());
        assert!(parse_proxy_url("ws://x:3000/ws?v=2").is_err());
        assert!(parse_proxy_url("ws://x:99999").is_err());
        assert!(parse_proxy_url("ws://x:abc").is_err());
        assert!(parse_proxy_url("ws://:3000").is_err());
        assert!(parse_proxy_url("").is_err());
    }

    #[test]
    fn wolfx_url_keeps_the_path() {
        assert_eq!(
            parse_wolfx_url("wss://ws-api.wolfx.jp/jma_eew"),
            Ok("wss://ws-api.wolfx.jp/jma_eew".into())
        );
        assert_eq!(
            parse_wolfx_url("ws://127.0.0.1:8081"),
            Ok("ws://127.0.0.1:8081".into())
        );
        assert!(parse_wolfx_url("https://x").is_err());
        assert!(parse_wolfx_url("wss://").is_err());
        assert!(parse_wolfx_url("wss:///x").is_err());
    }

    #[test]
    fn location_parsing() {
        assert_eq!(parse_location("", "", " "), Ok(None));
        let l = parse_location("東京", "35.68", "139.76").unwrap().unwrap();
        assert_eq!(
            (l.name.as_str(), l.latitude, l.longitude),
            ("東京", 35.68, 139.76)
        );
        let l = parse_location("", "35", "139").unwrap().unwrap();
        assert_eq!(l.name, "指定地点");
        assert!(parse_location("a", "", "139").is_err());
        assert!(parse_location("a", "35", "x").is_err());
        assert!(parse_location("a", "91", "139").is_err());
        assert!(parse_location("a", "35", "181").is_err());
        assert!(parse_location("a", "NaN", "139").is_err());
        assert!(parse_location("a", "35", "inf").is_err());
    }
}
