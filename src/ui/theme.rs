//! 色、フォント、角丸。

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Visuals};
use std::sync::Arc;

use crate::eew::report::{LgClass, Shindo};

pub const BG: Color32 = Color32::from_rgb(0x12, 0x14, 0x17);
pub const PANEL: Color32 = Color32::from_rgb(0x1b, 0x1e, 0x23);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(0x24, 0x28, 0x2e);
pub const BORDER: Color32 = Color32::from_rgb(0x30, 0x35, 0x3c);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xea, 0xed);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x95, 0x9c, 0xa6);

pub const SEA: Color32 = Color32::from_rgb(0x0c, 0x10, 0x14);
pub const LAND: Color32 = Color32::from_rgb(0x2b, 0x2f, 0x33);
pub const REGION_LINE: Color32 = Color32::from_rgb(0x46, 0x4b, 0x52);
pub const PREF_LINE: Color32 = Color32::from_rgb(0x8d, 0x94, 0x9c);

pub const WARNING: Color32 = Color32::from_rgb(0xe0, 0x32, 0x2b);
/// 警報対象地域 (まとめ表示) の背景。パネル色に赤を少し足した暗い赤。
pub const WARNING_BG: Color32 = Color32::from_rgb(0x3a, 0x1c, 0x1e);
pub const FORECAST: Color32 = Color32::from_rgb(0xf2, 0xc1, 0x2e);
pub const ON_FORECAST: Color32 = Color32::from_rgb(0x1a, 0x16, 0x05);
pub const CANCELED: Color32 = Color32::from_rgb(0x5a, 0x60, 0x68);
pub const P_WAVE: Color32 = Color32::from_rgb(0x5f, 0xb4, 0xff);
pub const OK: Color32 = Color32::from_rgb(0x3d, 0xc4, 0x7e);
pub const TRAINING: Color32 = Color32::from_rgb(0x2f, 0xb8, 0xc9);
/// 設定画面で、選んでいるもの・オンのものに付ける色。
pub const ACCENT: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);

pub const RADIUS: CornerRadius = CornerRadius::same(8);
pub const RADIUS_SMALL: CornerRadius = CornerRadius::same(5);

/// 気象庁の震度色。
pub fn shindo_color(s: Shindo) -> Color32 {
    match s.index() {
        0 => Color32::from_rgb(0x50, 0x56, 0x5e),
        1 => Color32::from_rgb(0xf2, 0xf2, 0xff),
        2 => Color32::from_rgb(0x00, 0xaa, 0xff),
        3 => Color32::from_rgb(0x00, 0x41, 0xff),
        4 => Color32::from_rgb(0xfa, 0xe6, 0x96),
        5 => Color32::from_rgb(0xff, 0xe6, 0x00),
        6 => Color32::from_rgb(0xff, 0x99, 0x00),
        7 => Color32::from_rgb(0xff, 0x28, 0x00),
        8 => Color32::from_rgb(0xa5, 0x00, 0x21),
        _ => Color32::from_rgb(0xb4, 0x00, 0x68),
    }
}

pub fn shindo_text_color(s: Shindo) -> Color32 {
    match s.index() {
        1 | 4 | 5 | 6 => Color32::from_rgb(0x15, 0x15, 0x18),
        _ => Color32::WHITE,
    }
}

/// 長周期地震動階級の色。
pub fn lg_color(c: LgClass) -> Color32 {
    match c.index() {
        0 => Color32::from_rgb(0x50, 0x56, 0x5e),
        1 => Color32::from_rgb(0x00, 0xaa, 0xff),
        2 => Color32::from_rgb(0xff, 0xe6, 0x00),
        3 => Color32::from_rgb(0xff, 0x28, 0x00),
        _ => Color32::from_rgb(0xb4, 0x00, 0x68),
    }
}

pub fn lg_text_color(c: LgClass) -> Color32 {
    match c.index() {
        2 => Color32::from_rgb(0x15, 0x15, 0x18),
        _ => Color32::WHITE,
    }
}

/// 大きく出す数字用 (BIZ UDGothic Bold)。
pub fn display_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

pub fn bold_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn body_font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// 情報パネルのタイトル (M+ 1p Medium)。
pub fn title_font(size: f32) -> FontId {
    mplus_medium_font(size)
}

/// 報数 (Gen Interface JP Bold)。
pub fn serial_font(size: f32) -> FontId {
    bold_font(size)
}

pub fn seed_bold_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("seed-bold".into()))
}

pub fn seed_regular_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("seed-regular".into()))
}

pub fn mplus_light_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("mplus-light".into()))
}

pub fn mplus_medium_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("mplus-medium".into()))
}

/// アイコン (Lucide)。文字は `lucide_icons::Icon::unicode` で得る。
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut fonts,
        "gen-regular",
        include_bytes!("../../assets/fonts/GenInterfaceJP-Regular.ttf"),
    );
    add(
        &mut fonts,
        "gen-bold",
        include_bytes!("../../assets/fonts/GenInterfaceJP-Bold.ttf"),
    );
    add(
        &mut fonts,
        "biz-bold",
        include_bytes!("../../assets/fonts/BIZUDGothic-Bold.ttf"),
    );
    add(
        &mut fonts,
        "seed-bold",
        include_bytes!("../../assets/fonts/LINESeedJP-Bold.ttf"),
    );
    add(
        &mut fonts,
        "seed-regular",
        include_bytes!("../../assets/fonts/LINESeedJP-Regular.ttf"),
    );
    add(
        &mut fonts,
        "mplus-light",
        include_bytes!("../../assets/fonts/MPLUS1p-Light.ttf"),
    );
    add(
        &mut fonts,
        "mplus-medium",
        include_bytes!("../../assets/fonts/MPLUS1p-Medium.ttf"),
    );
    add(&mut fonts, "lucide", lucide_icons::LUCIDE_FONT_BYTES);

    let fallback: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    let with = |first: &str| {
        let mut v = vec![first.to_string()];
        v.extend(fallback.iter().cloned());
        v
    };
    // 日本語を持たないフォントは源ノ角ゴ系の GenInterface に落とす
    let with_jp = |first: &str| {
        let mut v = vec![first.to_string(), "gen-regular".to_string()];
        v.extend(fallback.iter().cloned());
        v
    };
    fonts
        .families
        .insert(FontFamily::Proportional, with("gen-regular"));
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), with("gen-bold"));
    fonts
        .families
        .insert(FontFamily::Name("display".into()), with("biz-bold"));
    fonts
        .families
        .insert(FontFamily::Name("seed-bold".into()), with_jp("seed-bold"));
    fonts.families.insert(
        FontFamily::Name("seed-regular".into()),
        with_jp("seed-regular"),
    );
    fonts.families.insert(
        FontFamily::Name("mplus-light".into()),
        with_jp("mplus-light"),
    );
    fonts.families.insert(
        FontFamily::Name("mplus-medium".into()),
        with_jp("mplus-medium"),
    );
    fonts
        .families
        .insert(FontFamily::Name("icons".into()), vec!["lucide".to_string()]);
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .push("gen-regular".into());
    ctx.set_fonts(fonts);

    let mut v = Visuals::dark();
    // ヒンティングはグリッドに揃えるぶん、125% のような小数倍率や、情報パネルの
    // 拡縮で出る小数サイズだと縦線の太さが揃わずギザつく。アウトラインのまま描く。
    v.text_options.font_hinting = false;
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.override_text_color = Some(TEXT);
    v.window_corner_radius = RADIUS;
    v.widgets.noninteractive.bg_stroke.color = BORDER;
    ctx.set_visuals(v);
}
