//! 細分区域 code (3 桁、電文の `regions[].code`) から府県と地方を引く固定表。
//!
//! 地図の `eew-region` の code と一致する。名前では結合しない。
//! 府県の表示名は接尾辞なし (宮城、東京、京都…)。細分区域の名前は電文のものをそのまま使うので、ここでは持たない。
//!
//! 新潟と伊豆諸島、小笠原は、他の府県と束ねない独立の地方として扱う。

/// 地方。北海道・新潟・伊豆諸島・小笠原・沖縄は府県が 1 つだけの地方。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Hokkaido,
    Tohoku,
    Kanto,
    Izu,
    Ogasawara,
    Niigata,
    Hokuriku,
    Koshin,
    Tokai,
    Kinki,
    Chugoku,
    Shikoku,
    Kyushu,
    Okinawa,
}

impl Region {
    pub const ALL: [Region; 14] = [
        Self::Hokkaido,
        Self::Tohoku,
        Self::Kanto,
        Self::Izu,
        Self::Ogasawara,
        Self::Niigata,
        Self::Hokuriku,
        Self::Koshin,
        Self::Tokai,
        Self::Kinki,
        Self::Chugoku,
        Self::Shikoku,
        Self::Kyushu,
        Self::Okinawa,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Hokkaido => "北海道",
            Self::Tohoku => "東北",
            Self::Kanto => "関東",
            Self::Izu => "伊豆諸島",
            Self::Ogasawara => "小笠原",
            Self::Niigata => "新潟",
            Self::Hokuriku => "北陸",
            Self::Koshin => "甲信",
            Self::Tokai => "東海",
            Self::Kinki => "近畿",
            Self::Chugoku => "中国",
            Self::Shikoku => "四国",
            Self::Kyushu => "九州",
            Self::Okinawa => "沖縄",
        }
    }

    /// 他の府県と束ねない、名前も地方名と同じ独立の単位 (伊豆諸島、小笠原)。
    pub fn is_standalone(self) -> bool {
        matches!(self, Self::Izu | Self::Ogasawara)
    }

    /// この地方に属する府県の添字。
    pub fn prefs(self) -> impl Iterator<Item = usize> {
        PREFS
            .iter()
            .enumerate()
            .filter(move |(_, p)| p.region == self)
            .map(|(i, _)| i)
    }
}

pub struct Pref {
    /// 表示名 (接尾辞なし)
    pub name: &'static str,
    pub region: Region,
    pub codes: &'static [u16],
}

/// 府県ごとの細分区域 code。並びは code 順。
pub static PREFS: &[Pref] = &[
    Pref {
        name: "北海道",
        region: Region::Hokkaido,
        codes: &[
            100, 101, 102, 105, 106, 107, 110, 115, 116, 117, 119, 120, 121, 122, 125, 126, 127,
            130, 131, 135, 136, 139, 140, 141, 142, 145, 146, 150, 151, 152, 155, 156, 157, 160,
            161, 165, 166, 167,
        ],
    },
    Pref {
        name: "青森",
        region: Region::Tohoku,
        codes: &[200, 201, 202, 203],
    },
    Pref {
        name: "岩手",
        region: Region::Tohoku,
        codes: &[210, 211, 212, 213],
    },
    Pref {
        name: "宮城",
        region: Region::Tohoku,
        codes: &[220, 221, 222],
    },
    Pref {
        name: "秋田",
        region: Region::Tohoku,
        codes: &[230, 231, 232, 233],
    },
    Pref {
        name: "山形",
        region: Region::Tohoku,
        codes: &[240, 241, 242, 243],
    },
    Pref {
        name: "福島",
        region: Region::Tohoku,
        codes: &[250, 251, 252],
    },
    Pref {
        name: "茨城",
        region: Region::Kanto,
        codes: &[300, 301],
    },
    Pref {
        name: "栃木",
        region: Region::Kanto,
        codes: &[310, 311],
    },
    Pref {
        name: "群馬",
        region: Region::Kanto,
        codes: &[320, 321],
    },
    Pref {
        name: "埼玉",
        region: Region::Kanto,
        codes: &[330, 331, 332],
    },
    Pref {
        name: "千葉",
        region: Region::Kanto,
        codes: &[340, 341, 342],
    },
    Pref {
        name: "東京",
        region: Region::Kanto,
        codes: &[350, 351, 352],
    },
    Pref {
        name: "神奈川",
        region: Region::Kanto,
        codes: &[360, 361],
    },
    Pref {
        name: "伊豆諸島",
        region: Region::Izu,
        codes: &[354, 355, 356, 357, 358],
    },
    Pref {
        name: "小笠原",
        region: Region::Ogasawara,
        codes: &[359],
    },
    Pref {
        name: "新潟",
        region: Region::Niigata,
        codes: &[370, 371, 372, 375],
    },
    Pref {
        name: "富山",
        region: Region::Hokuriku,
        codes: &[380, 381],
    },
    Pref {
        name: "石川",
        region: Region::Hokuriku,
        codes: &[390, 391],
    },
    Pref {
        name: "福井",
        region: Region::Hokuriku,
        codes: &[400, 401],
    },
    Pref {
        name: "山梨",
        region: Region::Koshin,
        codes: &[411, 412],
    },
    Pref {
        name: "長野",
        region: Region::Koshin,
        codes: &[420, 421, 422],
    },
    Pref {
        name: "岐阜",
        region: Region::Tokai,
        codes: &[430, 431, 432],
    },
    Pref {
        name: "静岡",
        region: Region::Tokai,
        codes: &[440, 441, 442, 443],
    },
    Pref {
        name: "愛知",
        region: Region::Tokai,
        codes: &[450, 451],
    },
    Pref {
        name: "三重",
        region: Region::Tokai,
        codes: &[460, 461, 462],
    },
    Pref {
        name: "滋賀",
        region: Region::Kinki,
        codes: &[500, 501],
    },
    Pref {
        name: "京都",
        region: Region::Kinki,
        codes: &[510, 511],
    },
    Pref {
        name: "大阪",
        region: Region::Kinki,
        codes: &[520, 521],
    },
    Pref {
        name: "兵庫",
        region: Region::Kinki,
        codes: &[530, 531, 532, 535],
    },
    Pref {
        name: "奈良",
        region: Region::Kinki,
        codes: &[540],
    },
    Pref {
        name: "和歌山",
        region: Region::Kinki,
        codes: &[550, 551],
    },
    Pref {
        name: "鳥取",
        region: Region::Chugoku,
        codes: &[560, 562, 563],
    },
    Pref {
        name: "島根",
        region: Region::Chugoku,
        codes: &[570, 571, 575],
    },
    Pref {
        name: "岡山",
        region: Region::Chugoku,
        codes: &[580, 581],
    },
    Pref {
        name: "広島",
        region: Region::Chugoku,
        codes: &[590, 591, 592],
    },
    Pref {
        name: "山口",
        region: Region::Chugoku,
        codes: &[700, 702, 703, 704],
    },
    Pref {
        name: "徳島",
        region: Region::Shikoku,
        codes: &[600, 601],
    },
    Pref {
        name: "香川",
        region: Region::Shikoku,
        codes: &[610, 611],
    },
    Pref {
        name: "愛媛",
        region: Region::Shikoku,
        codes: &[620, 621, 622],
    },
    Pref {
        name: "高知",
        region: Region::Shikoku,
        codes: &[630, 631, 632],
    },
    Pref {
        name: "福岡",
        region: Region::Kyushu,
        codes: &[710, 711, 712, 713],
    },
    Pref {
        name: "佐賀",
        region: Region::Kyushu,
        codes: &[720, 721],
    },
    Pref {
        name: "長崎",
        region: Region::Kyushu,
        codes: &[730, 731, 732, 735, 736, 737],
    },
    Pref {
        name: "熊本",
        region: Region::Kyushu,
        codes: &[740, 741, 742, 743],
    },
    Pref {
        name: "大分",
        region: Region::Kyushu,
        codes: &[750, 751, 752, 753],
    },
    Pref {
        name: "宮崎",
        region: Region::Kyushu,
        codes: &[760, 761, 762, 763],
    },
    Pref {
        name: "鹿児島",
        region: Region::Kyushu,
        codes: &[770, 771, 774, 775, 776, 777, 778, 779],
    },
    Pref {
        name: "沖縄",
        region: Region::Okinawa,
        codes: &[800, 801, 802, 803, 804, 805, 806, 807],
    },
];

/// 離島の区域。府県が「揃った」かの判定からは外す (警報が出ていなくても揃い扱い)。
/// そうしないと、離島を持つ北海道・長崎・鹿児島・沖縄は府県名にまとまらない。
pub static ISOLATED: &[u16] = &[
    119, 139, 575, 735, 736, 737, 774, 775, 776, 777, 778, 779, 802, 803, 804, 805, 806, 807,
];

pub fn is_isolated(code: u16) -> bool {
    ISOLATED.contains(&code)
}

/// code から府県の添字を引く。表に無ければ `None`。
pub fn pref_of(code: u16) -> Option<usize> {
    PREFS.iter().position(|p| p.codes.contains(&code))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::geo::topo::Topo;

    fn map_codes() -> HashSet<u16> {
        let topo = Topo::parse(include_bytes!("../../assets/geojson/eew-prefregion.json")).unwrap();
        topo.objects["eew-region"]
            .iter()
            .filter_map(|f| f.code.as_ref())
            .map(|c| c.parse().unwrap())
            .collect()
    }

    #[test]
    fn table_matches_the_map_exactly() {
        let all: Vec<u16> = PREFS.iter().flat_map(|p| p.codes.iter().copied()).collect();
        let set: HashSet<u16> = all.iter().copied().collect();
        assert_eq!(all.len(), set.len(), "code が重複している");
        let map = map_codes();
        assert_eq!(map.len(), 188);
        let missing: Vec<_> = map.difference(&set).collect();
        let extra: Vec<_> = set.difference(&map).collect();
        assert!(missing.is_empty(), "表に無い code: {missing:?}");
        assert!(extra.is_empty(), "地図に無い code: {extra:?}");
    }

    #[test]
    fn isolated_codes_are_in_the_table() {
        for c in ISOLATED {
            assert!(pref_of(*c).is_some(), "{c} が表に無い");
        }
    }

    #[test]
    fn pref_names_have_no_suffix() {
        for p in PREFS {
            if p.name == "京都" {
                continue;
            }
            assert!(
                !p.name.ends_with(['県', '府', '都']),
                "{} に接尾辞がある",
                p.name
            );
        }
    }

    #[test]
    fn every_pref_has_a_non_isolated_area() {
        for p in PREFS {
            assert!(p.codes.iter().any(|c| !is_isolated(*c)), "{}", p.name);
        }
    }

    #[test]
    fn niigata_and_izu_are_their_own_regions() {
        assert_eq!(Region::Niigata.prefs().count(), 1);
        assert_eq!(Region::Izu.prefs().count(), 1);
        assert_eq!(Region::Ogasawara.prefs().count(), 1);
        assert_eq!(Region::Hokuriku.prefs().count(), 3);
        assert_eq!(Region::Koshin.prefs().count(), 2);
        assert_eq!(Region::Kanto.prefs().count(), 7);
    }
}
