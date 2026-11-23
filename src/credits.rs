//! クレジット (出典とライセンス)。設定画面の「ライセンス」欄に出す。
//!
//! MIT License は「著作権表示と許諾表示を、ソフトウェアの複製または重要な部分に含める」ことが
//! 条件なので、名前だけでなくライセンス全文を埋め込み、画面から読めるようにする。

/// 1 件の出典。
pub struct Credit {
    pub name: &'static str,
    pub url: Option<&'static str>,
    /// 何に使っているか
    pub usage: &'static str,
    pub license: &'static str,
    pub copyright: &'static str,
    /// ライセンス全文。利用規約に従うものは `None`
    pub full_text: Option<&'static str>,
}

pub const CREDITS: &[Credit] = &[
    Credit {
        name: "kyoshin-monitor-observation-points",
        url: Some("https://github.com/ingen084/kyoshin-monitor-observation-points"),
        usage: "強震モニタの観測点リスト",
        license: "MIT License",
        copyright: "Copyright (c) 2025 ingen084",
        full_text: Some(include_str!("../assets/kmoni/LICENSE")),
    },
    Credit {
        name: "強震モニタ（防災科学技術研究所）",
        url: Some("http://www.kmoni.bosai.go.jp"),
        usage: "リアルタイム震度の画像",
        license: "提供元の利用規約に従う",
        copyright: "国立研究開発法人防災科学技術研究所",
        full_text: None,
    },
    Credit {
        name: "Wolfx Project",
        url: Some("https://wolfx.jp/"),
        usage: "緊急地震速報 (jma_eew) の受信元の 1 つ (設定で選ぶ)",
        license: "提供元の利用規約に従う",
        copyright: "Wolfx Project",
        full_text: None,
    },
    Credit {
        name: "Inter",
        url: Some("https://github.com/rsms/inter"),
        usage: "フォント",
        license: "SIL Open Font License 1.1",
        copyright: "Copyright (c) 2016 The Inter Project Authors",
        full_text: Some(include_str!("../assets/fonts/OFL-Inter.txt")),
    },
    Credit {
        name: "Notification (IENBA)",
        url: Some("https://freesound.org/s/545495/"),
        usage: "効果音 (予報)。加工して使用",
        license: "Creative Commons Attribution 4.0 International (CC BY 4.0)",
        copyright: "IENBA",
        full_text: Some(include_str!("../assets/sounds/LICENSE-sounds.txt")),
    },
    Credit {
        name: "Text Message (LoudKevin)",
        url: Some("https://freesound.org/s/761464/"),
        usage: "効果音 (警報)。加工して使用",
        license: "Creative Commons 0 (CC0 1.0)",
        copyright: "LoudKevin",
        full_text: Some(include_str!("../assets/sounds/LICENSE-sounds.txt")),
    },
    Credit {
        name: "Gen Interface JP",
        url: None,
        usage: "フォント",
        license: "SIL Open Font License 1.1（配布元のビルド用ファイルは MIT License）",
        copyright: "Copyright (c) 2026 Yamato Iizuka",
        full_text: Some(include_str!("../assets/fonts/OFL-GenInterfaceJP.txt")),
    },
    Credit {
        name: "BIZ UDGothic",
        url: Some("https://github.com/googlefonts/morisawa-biz-ud-gothic"),
        usage: "フォント",
        license: "SIL Open Font License 1.1",
        copyright: "Copyright 2022 The BIZ UDGothic Project Authors",
        full_text: Some(include_str!("../assets/fonts/OFL-BIZUDGothic.txt")),
    },
    Credit {
        name: "LINE Seed JP",
        url: None,
        usage: "フォント",
        license: "SIL Open Font License 1.1",
        copyright: "(c) LY Corporation",
        full_text: Some(include_str!("../assets/fonts/OFL-LINESeedJP.txt")),
    },
    Credit {
        name: "M PLUS 1p",
        url: None,
        usage: "フォント",
        license: "SIL Open Font License 1.1",
        copyright: "Copyright 2016 The M+ Project Authors",
        full_text: Some(include_str!("../assets/fonts/OFL-MPLUS1p.txt")),
    },
    Credit {
        name: "Lucide",
        url: Some("https://lucide.dev"),
        usage: "アイコン (フォント)",
        license: "ISC License（Feather 由来のアイコンは MIT License）",
        copyright: "Copyright (c) 2026 Lucide Icons and Contributors",
        full_text: Some(include_str!("../assets/fonts/LICENSE-Lucide.txt")),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_observation_points_list_is_credited_with_full_text() {
        for (name, year) in [("kyoshin-monitor-observation-points", "2025")] {
            let c = CREDITS
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{name} が無い"));
            assert_eq!(c.license, "MIT License");
            assert!(
                c.copyright.contains(year) && c.copyright.contains("ingen084"),
                "{}",
                c.copyright
            );
            let text = c.full_text.expect("MIT は全文を同梱する");
            assert!(text.contains("Permission is hereby granted"), "{name}");
            assert!(
                text.contains(c.copyright),
                "{name}: 全文の著作権表示と合っていない"
            );
        }
    }

    #[test]
    fn every_entry_with_a_text_has_it_and_the_kmoni_one_has_none() {
        for c in CREDITS {
            match c.full_text {
                Some(t) => assert!(!t.trim().is_empty(), "{}", c.name),
                None => assert!(c.license.contains("利用規約"), "{}", c.name),
            }
        }
    }
}
