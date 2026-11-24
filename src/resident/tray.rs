//! トレイ (Windows のタスクトレイ、macOS のメニューバー) のアイコンとメニュー。
//! 配布している Windows と macOS だけで動き、ほかの OS では何もしない。

/// トレイから来る操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// ウィンドウを表示する
    Show,
    /// ウィンドウを表示して設定画面を開く
    Settings,
    ToggleGameMode,
    Quit,
}

/// この OS でトレイを使えるか。
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

#[cfg(any(windows, target_os = "macos"))]
pub use imp::Tray;
#[cfg(not(any(windows, target_os = "macos")))]
pub use stub::Tray;

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use std::sync::mpsc::{Receiver, Sender, channel};

    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    use super::TrayAction;

    const ID_SHOW: &str = "rsquake.show";
    const ID_SETTINGS: &str = "rsquake.settings";
    const ID_GAME: &str = "rsquake.game";
    const ID_QUIT: &str = "rsquake.quit";

    const ICON_PNG: &[u8] = include_bytes!("../../assets/icon/icon.png");
    /// トレイに渡す画像の一辺。元の画像 (1024) をそのまま渡さず、縮める
    const ICON_SIZE: u32 = 64;

    pub struct Tray {
        icon: TrayIcon,
        game: CheckMenuItem,
        normal_icon: Icon,
        game_icon: Icon,
        game_mode: bool,
        rx: Receiver<TrayAction>,
    }

    impl Tray {
        /// メインスレッドで呼ぶ。作れなかったら `None`。
        pub fn new(ctx: &egui::Context, game_mode: bool) -> Option<Self> {
            match Self::build(ctx, game_mode) {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::warn!("トレイを作れない: {e}");
                    None
                }
            }
        }

        fn build(ctx: &egui::Context, game_mode: bool) -> anyhow::Result<Self> {
            let (normal_icon, game_icon) = icons()?;
            let show = MenuItem::with_id(ID_SHOW, "ウィンドウを表示", true, None);
            let settings = MenuItem::with_id(ID_SETTINGS, "設定", true, None);
            let game = CheckMenuItem::with_id(ID_GAME, "ゲームモード", true, game_mode, None);
            let quit = MenuItem::with_id(ID_QUIT, "終了", true, None);
            let menu = Menu::new();
            menu.append_items(&[
                &show,
                &settings,
                &game,
                &PredefinedMenuItem::separator(),
                &quit,
            ])?;

            let (tx, rx) = channel();
            install_handlers(tx, ctx.clone());

            let icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                // macOS はメニューバーのクリックでメニューを開く。Windows は左クリックで表示、右クリックでメニュー
                .with_menu_on_left_click(cfg!(target_os = "macos"))
                .with_icon(if game_mode {
                    game_icon.clone()
                } else {
                    normal_icon.clone()
                })
                .with_tooltip(tooltip(game_mode))
                .build()?;
            Ok(Self {
                icon,
                game,
                normal_icon,
                game_icon,
                game_mode,
                rx,
            })
        }

        /// 溜まった操作を取り出す。
        pub fn poll(&self) -> Vec<TrayAction> {
            self.rx.try_iter().collect()
        }

        /// ゲームモードの表示 (チェック、アイコン、ツールチップ) を合わせる。
        pub fn set_game_mode(&mut self, on: bool) {
            if self.game_mode == on {
                return;
            }
            self.game_mode = on;
            self.game.set_checked(on);
            let icon = if on {
                &self.game_icon
            } else {
                &self.normal_icon
            };
            if let Err(e) = self.icon.set_icon(Some(icon.clone())) {
                tracing::warn!("トレイのアイコンを替えられない: {e}");
            }
            let _ = self.icon.set_tooltip(Some(tooltip(on)));
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
            MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
        }
    }

    fn tooltip(game_mode: bool) -> &'static str {
        if game_mode {
            "RSQuake (ゲームモード)"
        } else {
            "RSQuake"
        }
    }

    /// トレイのイベントを受けて、`logic` を起こす。
    fn install_handlers(tx: Sender<TrayAction>, ctx: egui::Context) {
        let (tx_menu, ctx_menu) = (tx.clone(), ctx.clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let action = match e.id.0.as_str() {
                ID_SHOW => TrayAction::Show,
                ID_SETTINGS => TrayAction::Settings,
                ID_GAME => TrayAction::ToggleGameMode,
                ID_QUIT => TrayAction::Quit,
                _ => return,
            };
            let _ = tx_menu.send(action);
            ctx_menu.request_repaint();
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                let _ = tx.send(TrayAction::Show);
                ctx.request_repaint();
            }
        }));
    }

    /// 通常のアイコンと、ゲームモードの印 (右下の点) を付けたアイコン。
    fn icons() -> anyhow::Result<(Icon, Icon)> {
        let img =
            image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)?.into_rgba8();
        let mut img = image::imageops::resize(
            &img,
            ICON_SIZE,
            ICON_SIZE,
            image::imageops::FilterType::Triangle,
        );
        let normal = Icon::from_rgba(img.clone().into_raw(), ICON_SIZE, ICON_SIZE)?;

        let s = ICON_SIZE as f32;
        let (cx, cy, r) = (s * 0.74, s * 0.74, s * 0.24);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d <= r + 1.5 {
                // 内側は橙、縁は白
                *px = if d <= r - 1.0 {
                    image::Rgba([0xff, 0x8a, 0x00, 0xff])
                } else {
                    image::Rgba([0xff, 0xff, 0xff, 0xff])
                };
            }
        }
        let game = Icon::from_rgba(img.into_raw(), ICON_SIZE, ICON_SIZE)?;
        Ok((normal, game))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod stub {
    use super::TrayAction;

    /// トレイの無い OS 用。作れないので常に `None` になる。
    pub struct Tray;

    impl Tray {
        pub fn new(_ctx: &egui::Context, _game_mode: bool) -> Option<Self> {
            None
        }

        pub fn poll(&self) -> Vec<TrayAction> {
            Vec::new()
        }

        pub fn set_game_mode(&mut self, _on: bool) {}
    }
}
