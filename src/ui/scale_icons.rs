//! 震度・長周期地震動階級のアイコン (assets/scale)。初回に PNG を読んでテクスチャにする。

use std::sync::Arc;

use egui::{Color32, ColorImage, Context, TextureHandle, TextureOptions};

use crate::eew::report::{LgClass, Shindo};

/// 震度の序数 (0〜9) の並び。
const SHINDO: [(&str, &[u8]); 10] = [
    ("0", include_bytes!("../../assets/scale/0.png")),
    ("1", include_bytes!("../../assets/scale/1.png")),
    ("2", include_bytes!("../../assets/scale/2.png")),
    ("3", include_bytes!("../../assets/scale/3.png")),
    ("4", include_bytes!("../../assets/scale/4.png")),
    ("5low", include_bytes!("../../assets/scale/5low.png")),
    ("5high", include_bytes!("../../assets/scale/5high.png")),
    ("6low", include_bytes!("../../assets/scale/6low.png")),
    ("6high", include_bytes!("../../assets/scale/6high.png")),
    ("7", include_bytes!("../../assets/scale/7.png")),
];

/// 長周期地震動階級 0〜4。階級 0 は震度 0 と同じアイコンを使う。
const LG: [(&str, &[u8]); 5] = [
    ("lg0", include_bytes!("../../assets/scale/0.png")),
    ("lg1", include_bytes!("../../assets/scale/lg1.png")),
    ("lg2", include_bytes!("../../assets/scale/lg2.png")),
    ("lg3", include_bytes!("../../assets/scale/lg3.png")),
    ("lg4", include_bytes!("../../assets/scale/lg4.png")),
];

/// アイコンの塗り色に掛けて、箱の背景色にする係数。
const BOX_DARKEN: f32 = 0.58;

pub struct Icon {
    pub texture: TextureHandle,
    /// 箱の背景色 (アイコンの塗り色を暗くしたもの)
    pub box_color: Color32,
}

pub struct ScaleIcons {
    shindo: Vec<Icon>,
    lg: Vec<Icon>,
    unknown: Icon,
    lg_unknown: Icon,
    /// 長周期地震動の予測が電文に無いとき
    none: Icon,
}

impl ScaleIcons {
    /// 読み込み済みならそれを返し、まだなら読む。
    pub fn get(ctx: &Context) -> Arc<Self> {
        let id = egui::Id::NULL.with("scale-icons");
        if let Some(icons) = ctx.data(|d| d.get_temp::<Arc<Self>>(id)) {
            return icons;
        }
        let load = |list: &[(&str, &[u8])]| -> Vec<Icon> {
            list.iter().map(|(n, b)| load_icon(ctx, n, b)).collect()
        };
        let icons = Arc::new(Self {
            shindo: load(&SHINDO),
            lg: load(&LG),
            unknown: load_icon(
                ctx,
                "unknown",
                include_bytes!("../../assets/scale/unknown.png"),
            ),
            lg_unknown: load_icon(
                ctx,
                "lgunknown",
                include_bytes!("../../assets/scale/lgunknown.png"),
            ),
            none: load_icon(ctx, "none", include_bytes!("../../assets/scale/none.png")),
        });
        ctx.data_mut(|d| d.insert_temp(id, icons.clone()));
        icons
    }

    /// 震度のアイコン。`None` は不明。
    pub fn shindo(&self, s: Option<Shindo>) -> &Icon {
        match s {
            Some(s) => &self.shindo[(s.index() as usize).min(self.shindo.len() - 1)],
            None => &self.unknown,
        }
    }

    /// 長周期地震動階級のアイコン。`None` は階級が読めない (不明)。
    pub fn lg(&self, c: Option<LgClass>) -> &Icon {
        match c {
            Some(c) => &self.lg[(c.index() as usize).min(self.lg.len() - 1)],
            None => &self.lg_unknown,
        }
    }

    /// 長周期地震動の予測が無いとき。
    pub fn none(&self) -> &Icon {
        &self.none
    }
}

fn load_icon(ctx: &Context, name: &str, bytes: &[u8]) -> Icon {
    let img = image::load_from_memory(bytes)
        .expect("同梱のアイコンが読めない")
        .to_rgba8();
    // 角は透明なので、少し内側の画素を塗り色とする
    let p = img.get_pixel(img.width() / 10, img.height() / 10);
    let darken = |v: u8| (v as f32 * BOX_DARKEN) as u8;
    let box_color = Color32::from_rgb(darken(p[0]), darken(p[1]), darken(p[2]));
    let size = [img.width() as usize, img.height() as usize];
    let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let texture = ctx.load_texture(format!("scale-{name}"), color, TextureOptions::LINEAR);
    Icon { texture, box_color }
}
