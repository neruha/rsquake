//! 新しい版などの通知ポップアップ。地図の右下に出し、10 秒で自動で消える。
//! マウスが乗っている間は消えるまでの時間を止める。EEW が出たら呼び出し側がすぐに閉じる。

use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Area, Button, Context, Frame, Id, Label, Layout, Margin, Order, Rect, RichText,
    Sense, Stroke, vec2,
};
use lucide_icons::Icon;
use semver::Version;

use crate::ui::theme;
use crate::update::{Countdown, Notice, RELEASES_PAGE, release_url};

const WIDTH: f32 = 340.0;
/// 地図の隅からの距離
const EDGE: f32 = 16.0;
/// 表示中の再描画の間隔。バーが約 1px 刻みで滑らかに進む程度 (10 秒で 340px)。
const REPAINT: Duration = Duration::from_millis(33);

/// ポップアップで押された操作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupAction {
    None,
    /// ブラウザで開く
    Open(String),
    /// アプリ内で更新する (ダウンロードを始める)
    Install,
    /// この版の通知をスキップする
    Skip(Version),
    Close,
}

pub struct UpdatePopup {
    notice: Notice,
    current: Version,
    /// アプリ内で更新できる (「更新する」ボタンを出す)
    installable: bool,
    countdown: Countdown,
}

impl UpdatePopup {
    pub fn new(notice: Notice, current: Version, installable: bool) -> Self {
        Self {
            notice,
            current,
            installable,
            countdown: Countdown::default(),
        }
    }

    pub fn show(&mut self, ctx: &Context, map: Rect) -> PopupAction {
        let (icon, title, body, open_label, open_url, skip) = match &self.notice {
            Notice::Update(r) => (
                Icon::Download,
                "新しいバージョンがあります".to_string(),
                if self.installable {
                    format!(
                        "RSQuake v{} が公開されています (現在 v{})。ダウンロードして、そのまま更新できます。",
                        r.version, self.current
                    )
                } else {
                    format!(
                        "RSQuake v{} が公開されています (現在 v{})。ダウンロードとリリースノートは GitHub にあります。",
                        r.version, self.current
                    )
                },
                "GitHub で開く",
                r.url.clone(),
                Some(r.version.clone()),
            ),
            Notice::Upgraded(v) => (
                Icon::Sparkles,
                format!("RSQuake v{v} になりました"),
                "変更点は GitHub のリリースノートで見られます。".to_string(),
                "リリースノートを見る",
                release_url(v),
                None,
            ),
            Notice::UpdateFailed(why) => (
                Icon::TriangleAlert,
                "更新できませんでした".to_string(),
                format!(
                    "{why} 元のバージョンのまま起動しました。手動で更新するときは GitHub から入手できます。"
                ),
                "GitHub で開く",
                RELEASES_PAGE.to_string(),
                None,
            ),
        };
        let install = self.installable && matches!(self.notice, Notice::Update(_));

        let mut action = PopupAction::None;
        let resp = Area::new(Id::unique("update_popup"))
            .order(Order::Foreground)
            .pivot(Align2::RIGHT_BOTTOM)
            .fixed_pos(map.right_bottom() - vec2(EDGE, EDGE))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::PANEL_RAISED)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(theme::RADIUS)
                    .inner_margin(Margin::same(14))
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 20,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(120),
                    })
                    .show(ui, |ui| {
                        ui.set_width(WIDTH);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(icon.unicode())
                                    .font(theme::icon_font(18.0))
                                    .color(theme::ACCENT),
                            );
                            ui.label(RichText::new(title).font(theme::bold_font(14.5)));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let close = Button::new(
                                    RichText::new(Icon::X.unicode())
                                        .font(theme::icon_font(14.0))
                                        .color(theme::TEXT_MUTED),
                                )
                                .frame(false);
                                if ui.add(close).on_hover_text("閉じる").clicked() {
                                    action = PopupAction::Close;
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ui.add(
                            Label::new(RichText::new(body).size(12.5).color(theme::TEXT_MUTED))
                                .wrap(),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if install {
                                let go = Button::new((
                                    RichText::new(Icon::Download.unicode())
                                        .font(theme::icon_font(14.0)),
                                    RichText::new("更新する").size(13.0),
                                ));
                                if ui.add(go).clicked() {
                                    action = PopupAction::Install;
                                }
                            }
                            let open = Button::new((
                                RichText::new(Icon::ExternalLink.unicode())
                                    .font(theme::icon_font(14.0)),
                                RichText::new(open_label).size(13.0),
                            ));
                            if ui.add(open).clicked() {
                                action = PopupAction::Open(open_url);
                            }
                            if let Some(version) = skip {
                                let skip_button = Button::new((
                                    RichText::new(Icon::SkipForward.unicode())
                                        .font(theme::icon_font(14.0)),
                                    RichText::new("この版はスキップ").size(13.0),
                                ));
                                if ui.add(skip_button).clicked() {
                                    action = PopupAction::Skip(version);
                                }
                            }
                        });
                        ui.add_space(10.0);
                        // 消えるまでの残り
                        let (bar, _) = ui.allocate_exact_size(vec2(WIDTH, 2.0), Sense::hover());
                        let p = ui.painter();
                        p.rect_filled(bar, 1.0, theme::BORDER);
                        p.rect_filled(
                            Rect::from_min_size(
                                bar.min,
                                vec2(bar.width() * self.countdown.fraction(), bar.height()),
                            ),
                            1.0,
                            theme::ACCENT,
                        );
                    });
            });

        if action == PopupAction::None
            && self
                .countdown
                .tick(Instant::now(), resp.response.contains_pointer())
        {
            action = PopupAction::Close;
        }
        // 表示中は毎フレーム次の再描画を予約し直す。入力が無くてもバーが止まらない。
        ctx.request_repaint_after(REPAINT);
        action
    }
}
