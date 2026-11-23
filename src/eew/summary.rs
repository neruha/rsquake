//! 警報地域のまとめ表示 (NHK 風)。警報が出ている区域だけを、震度なしで短い名前の並びにする。
//!
//! 細分区域は常に府県名にまとめる (区域名は出さない)。地方の全府県で区域がそろっていれば地方名にする。
//! それでも文字数が `NAME_BUDGET` を超えるときに限り、府県が半数以上出ている地方を地方名にし、
//! なお超えるなら震度の高い順に残して「ほか N 地域」にする。

use std::collections::BTreeMap;

use super::area_table::{self, PREFS, Region};
use super::report::{RegionForecast, Shindo};

/// 項目名の文字数の合計の上限
pub const NAME_BUDGET: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryLevel {
    /// 細分区域 (電文の名前のまま)
    Area,
    Prefecture,
    Region,
    /// 伊豆諸島、小笠原
    Standalone,
    /// 「ほか N 地域」
    More,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryItem {
    pub name: String,
    pub level: SummaryLevel,
    /// 含まれる区域の最大予想震度。並べ替えにだけ使い、画面には出さない。
    pub max_int: Option<Shindo>,
    /// 含まれる区域の最小 code (同じ震度のときの並び順)
    sort_code: u16,
}

struct Warned<'a> {
    code: u16,
    name: &'a str,
    int: Option<Shindo>,
    pref: Option<usize>,
}

/// 地方名にまとめる条件
#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionRule {
    /// 地方の全府県で、(離島を除く) 全区域が警報
    AllPrefs,
    /// 府県の半数以上が出ている
    HalfPrefs,
}

const STAGES: [RegionRule; 2] = [RegionRule::AllPrefs, RegionRule::HalfPrefs];

pub fn summarize(regions: &[RegionForecast]) -> Vec<SummaryItem> {
    let warned: Vec<Warned> = regions
        .iter()
        .filter(|r| r.is_warning)
        .map(|r| {
            let code = r.code.parse::<u16>().ok();
            Warned {
                code: code.unwrap_or(u16::MAX),
                name: &r.name,
                int: r.max_int.representative(),
                pref: code.and_then(area_table::pref_of),
            }
        })
        .collect();
    if warned.is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();
    for rule in STAGES {
        items = build(&warned, rule);
        if total_chars(&items) <= NAME_BUDGET {
            sort(&mut items);
            return items;
        }
    }
    sort(&mut items);
    truncate(items)
}

fn build(warned: &[Warned], rule: RegionRule) -> Vec<SummaryItem> {
    let mut items = Vec::new();
    let mut by_pref: BTreeMap<usize, Vec<&Warned>> = BTreeMap::new();
    for w in warned {
        match w.pref {
            Some(p) => by_pref.entry(p).or_default().push(w),
            // 表に無い区域はまとめられない
            None => items.push(area_item(w)),
        }
    }

    // 離島を除く全区域が警報なら、その府県は「そろった」
    let complete = |p: usize| {
        let list = &by_pref[&p];
        PREFS[p]
            .codes
            .iter()
            .filter(|c| !area_table::is_isolated(**c))
            .all(|c| list.iter().any(|w| w.code == *c))
    };

    for region in Region::ALL {
        let prefs: Vec<usize> = region.prefs().filter(|p| by_pref.contains_key(p)).collect();
        if prefs.is_empty() {
            continue;
        }
        let total = region.prefs().count();

        // 府県が 1 つだけの地方は、地方の段を飛ばして府県名で出す
        if total > 1 {
            let enough = match rule {
                RegionRule::AllPrefs => prefs.len() == total && prefs.iter().all(|p| complete(*p)),
                RegionRule::HalfPrefs => prefs.len() * 2 >= total,
            };
            if enough {
                let list: Vec<&Warned> = prefs
                    .iter()
                    .flat_map(|p| by_pref[p].iter().copied())
                    .collect();
                items.push(group_item(region.name(), SummaryLevel::Region, &list));
                continue;
            }
        }

        let level = if region.is_standalone() {
            SummaryLevel::Standalone
        } else {
            SummaryLevel::Prefecture
        };
        for p in prefs {
            items.push(group_item(PREFS[p].name, level, &by_pref[&p]));
        }
    }
    items
}

fn area_item(w: &Warned) -> SummaryItem {
    SummaryItem {
        name: w.name.to_string(),
        level: SummaryLevel::Area,
        max_int: w.int,
        sort_code: w.code,
    }
}

fn group_item(name: &str, level: SummaryLevel, list: &[&Warned]) -> SummaryItem {
    SummaryItem {
        name: name.to_string(),
        level,
        max_int: list.iter().filter_map(|w| w.int).max(),
        sort_code: list.iter().map(|w| w.code).min().unwrap_or(u16::MAX),
    }
}

fn total_chars(items: &[SummaryItem]) -> usize {
    items.iter().map(|i| i.name.chars().count()).sum()
}

/// 震度の降順、同じ震度なら code の昇順。
fn sort(items: &mut [SummaryItem]) {
    items.sort_by(|a, b| {
        b.max_int
            .cmp(&a.max_int)
            .then(a.sort_code.cmp(&b.sort_code))
    });
}

fn more_label(n: usize) -> String {
    format!("ほか{n}地域")
}

/// 予算に収まるまで、震度の高い順に残して残りを「ほか N 地域」にする。1 件は必ず残す。
fn truncate(mut items: Vec<SummaryItem>) -> Vec<SummaryItem> {
    let len = items.len();
    let mut keep = 1;
    for k in (1..len).rev() {
        let used = total_chars(&items[..k]) + more_label(len - k).chars().count();
        if used <= NAME_BUDGET {
            keep = k;
            break;
        }
    }
    if keep >= len {
        return items;
    }
    let dropped = len - keep;
    items.truncate(keep);
    items.push(SummaryItem {
        name: more_label(dropped),
        level: SummaryLevel::More,
        max_int: None,
        sort_code: u16::MAX,
    });
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eew::report::{Forecast, Upper};

    fn rf(code: u16, name: &str, int: &str, warning: bool) -> RegionForecast {
        let s = Shindo::parse(int).unwrap();
        RegionForecast {
            code: code.to_string(),
            name: name.to_string(),
            max_int: Forecast {
                from: Some(s),
                to: Upper::Value(s),
            },
            max_lg: None,
            is_warning: warning,
            is_plum: false,
            reached: false,
            arrival_time: None,
        }
    }

    fn w(code: u16, name: &str) -> RegionForecast {
        rf(code, name, "5-", true)
    }

    /// 名前は問わない (まとめ後の名前だけ見るテスト用)
    fn codes(list: &[u16]) -> Vec<RegionForecast> {
        list.iter().map(|c| w(*c, &format!("区域{c}"))).collect()
    }

    fn pref_codes(name: &str) -> Vec<u16> {
        PREFS
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .codes
            .to_vec()
    }

    fn region_codes(r: Region) -> Vec<u16> {
        r.prefs()
            .flat_map(|p| PREFS[p].codes.iter().copied())
            .collect()
    }

    fn names(items: &[SummaryItem]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn no_warnings_no_items() {
        assert!(summarize(&[]).is_empty());
        assert!(summarize(&[rf(220, "宮城県北部", "4", false)]).is_empty());
    }

    #[test]
    fn complete_prefecture_becomes_short_name() {
        let r = summarize(&codes(&pref_codes("宮城")));
        assert_eq!(names(&r), ["宮城"]);
        assert_eq!(r[0].level, SummaryLevel::Prefecture);
        assert_eq!(names(&summarize(&codes(&pref_codes("東京")))), ["東京"]);
        assert_eq!(names(&summarize(&codes(&pref_codes("京都")))), ["京都"]);
        assert_eq!(names(&summarize(&codes(&pref_codes("大阪")))), ["大阪"]);
    }

    #[test]
    fn areas_are_always_collapsed_to_prefectures() {
        let r = summarize(&[
            w(220, "宮城県北部"),
            w(221, "宮城県南部"),
            w(350, "東京都２３区"),
            w(510, "京都府北部"),
        ]);
        assert_eq!(names(&r), ["宮城", "東京", "京都"]);
        assert!(r.iter().all(|i| i.level == SummaryLevel::Prefecture));
    }

    #[test]
    fn isolated_islands_do_not_block_a_prefecture() {
        // 北海道: 離島 (119, 139) 以外がそろえば「北海道」
        let list: Vec<u16> = pref_codes("北海道")
            .into_iter()
            .filter(|c| !area_table::is_isolated(*c))
            .collect();
        assert_eq!(names(&summarize(&codes(&list))), ["北海道"]);
        // 離島だけでも、府県名で出る
        assert_eq!(names(&summarize(&codes(&[119]))), ["北海道"]);
    }

    #[test]
    fn one_area_per_prefecture_does_not_make_a_region() {
        // 東北 6 県に 1 区域ずつ。予算内なので、地方名にはせず府県名のまま。
        let r = summarize(&codes(&[200, 210, 220, 230, 240, 250]));
        assert_eq!(names(&r), ["青森", "岩手", "宮城", "秋田", "山形", "福島"]);
    }

    #[test]
    fn all_prefectures_of_a_region_become_the_region() {
        let r = summarize(&codes(&region_codes(Region::Tohoku)));
        assert_eq!(names(&r), ["東北"]);
        assert_eq!(r[0].level, SummaryLevel::Region);
    }

    #[test]
    fn kanto_does_not_need_izu_or_ogasawara() {
        let r = summarize(&codes(&region_codes(Region::Kanto)));
        assert_eq!(names(&r), ["関東"]);
    }

    #[test]
    fn niigata_is_not_part_of_hokuriku() {
        let mut list = region_codes(Region::Hokuriku);
        list.extend(pref_codes("新潟"));
        let r = summarize(&codes(&list));
        let n = names(&r);
        assert!(n.contains(&"北陸") && n.contains(&"新潟"), "{n:?}");
        // 新潟だけ揃っても地方にはならない
        let r = summarize(&codes(&pref_codes("新潟")));
        assert_eq!(names(&r), ["新潟"]);
        assert_eq!(r[0].level, SummaryLevel::Prefecture);
        // 新潟が欠けても北陸はそろう
        let r = summarize(&codes(&region_codes(Region::Hokuriku)));
        assert_eq!(names(&r), ["北陸"]);
    }

    #[test]
    fn hokuriku_and_koshin_are_separate_regions() {
        let r = summarize(&codes(&region_codes(Region::Koshin)));
        assert_eq!(names(&r), ["甲信"]);
        let mut list = region_codes(Region::Hokuriku);
        list.extend(region_codes(Region::Koshin));
        let r = summarize(&codes(&list));
        let n = names(&r);
        assert!(n.contains(&"北陸") && n.contains(&"甲信"), "{n:?}");
        assert_eq!(n.len(), 2);
    }

    #[test]
    fn izu_and_ogasawara_are_standalone() {
        let r = summarize(&codes(&pref_codes("伊豆諸島")));
        assert_eq!(names(&r), ["伊豆諸島"]);
        assert_eq!(r[0].level, SummaryLevel::Standalone);
        // 東京の区域とは混ざらない
        let mut list = pref_codes("伊豆諸島");
        list.extend(pref_codes("東京"));
        list.extend(pref_codes("小笠原"));
        let r = summarize(&codes(&list));
        assert_eq!(r.len(), 3);
        for n in ["伊豆諸島", "東京", "小笠原"] {
            assert!(names(&r).contains(&n), "{n}");
        }
    }

    #[test]
    fn within_budget_prefectures_stay_prefectures() {
        let r = summarize(&codes(&[210, 230, 250, 300, 370, 440, 450]));
        assert_eq!(
            names(&r),
            ["岩手", "秋田", "福島", "茨城", "新潟", "静岡", "愛知"]
        );
    }

    #[test]
    fn over_budget_collapses_to_region_when_half_of_prefectures_appear() {
        // 府県名のままでは 24 文字を超える数
        let r = summarize(&codes(&[
            200, 210, 220, 300, 310, 320, 330, 340, 350, 430, 440, 450, 460, 500, 510,
        ]));
        let n = names(&r);
        // 東北 3/6、関東 6/7、東海 4/4 は地方名に、近畿 2/6 は府県名のまま
        for expected in ["東北", "関東", "東海", "滋賀", "京都"] {
            assert!(n.contains(&expected), "{expected} が無い: {n:?}");
        }
        assert_eq!(n.len(), 5, "{n:?}");
    }

    #[test]
    fn extreme_cases_end_with_a_more_item() {
        let all: Vec<u16> = PREFS
            .iter()
            .flat_map(|p| p.codes.iter().copied())
            .filter(|c| c % 2 == 0)
            .collect();
        let r = summarize(&codes(&all));
        let last = r.last().unwrap();
        assert!(total_chars(&r) <= NAME_BUDGET, "{:?}", names(&r));
        if last.level == SummaryLevel::More {
            assert!(last.name.starts_with("ほか"));
        }
    }

    #[test]
    fn truncation_keeps_highest_intensity_and_counts_the_rest() {
        let items: Vec<SummaryItem> = (0..10)
            .map(|i| SummaryItem {
                name: "あいうえお".into(),
                level: SummaryLevel::Prefecture,
                max_int: Shindo::parse("4"),
                sort_code: i,
            })
            .collect();
        let r = truncate(items);
        // 5 文字 × 3 + 「ほか7地域」(6 文字) = 21 <= 24、4 件だと 26 で超える
        assert_eq!(r.len(), 4);
        assert_eq!(r.last().unwrap().name, "ほか7地域");
        assert_eq!(r.last().unwrap().level, SummaryLevel::More);
    }

    #[test]
    fn sorted_by_intensity_then_code() {
        let r = summarize(&[
            rf(250, "福島県中通り", "5-", true),
            rf(220, "宮城県北部", "6+", true),
            rf(210, "岩手県沿岸北部", "5-", true),
        ]);
        assert_eq!(names(&r), ["宮城", "岩手", "福島"]);
    }

    #[test]
    fn unknown_code_uses_telegram_name() {
        let r = summarize(&[w(999, "未知の区域")]);
        assert_eq!(names(&r), ["未知の区域"]);
        assert_eq!(r[0].level, SummaryLevel::Area);
        let bad = RegionForecast {
            code: "abc".into(),
            ..w(1, "変な区域")
        };
        assert_eq!(names(&summarize(&[bad])), ["変な区域"]);
    }
}
