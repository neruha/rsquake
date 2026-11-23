//! 効果音を鳴らすかどうかの判定。出力装置は開かない。

use crate::config::SoundConfig;
use crate::eew::store::ApplyOutcome;

/// 再生する音。並びが優先度（低い方から高い方）。
/// 揺れの音 (`assets/sounds/weak.mp3` など) は揺れの検知を戻したときのために残してある。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SoundCue {
    EewForecast,
    EewWarning,
}

impl SoundCue {
    pub fn label(self) -> &'static str {
        match self {
            Self::EewForecast => "緊急地震速報（予報）",
            Self::EewWarning => "緊急地震速報（警報）",
        }
    }

    pub const ALL: [SoundCue; 2] = [Self::EewForecast, Self::EewWarning];
}

/// 電文の採否から鳴らす音。続報・取消・破棄は鳴らさない。
pub fn cue_for_eew(outcome: &ApplyOutcome, is_warning: bool) -> Option<SoundCue> {
    match outcome {
        ApplyOutcome::New if is_warning => Some(SoundCue::EewWarning),
        ApplyOutcome::New => Some(SoundCue::EewForecast),
        ApplyOutcome::BecameWarning => Some(SoundCue::EewWarning),
        ApplyOutcome::Updated | ApplyOutcome::Canceled | ApplyOutcome::Rejected(_) => None,
    }
}

fn audible(cfg: &SoundConfig) -> bool {
    cfg.volume() > 0.0
}

fn allows(cfg: &SoundConfig, cue: SoundCue) -> bool {
    audible(cfg)
        && match cue {
            SoundCue::EewForecast => cfg.eew_forecast,
            SoundCue::EewWarning => cfg.eew_warning,
        }
}

/// 今流れている音。
#[derive(Debug, Default)]
pub struct SoundPolicy {
    playing: Option<SoundCue>,
}

impl SoundPolicy {
    /// 設定と、いま高い音が流れているかを見て、実際に再生する音を決める。
    ///
    /// `force` は試聴。オフの項目でも鳴らす。音量 0 では鳴らさない。
    pub fn request(
        &mut self,
        cue: Option<SoundCue>,
        cfg: &SoundConfig,
        still_playing: bool,
        force: bool,
    ) -> Option<SoundCue> {
        let cue = cue?;
        let allowed = if force {
            audible(cfg)
        } else {
            allows(cfg, cue)
        };
        if !allowed {
            return None;
        }
        if !still_playing {
            self.playing = None;
        }
        if !force && self.playing.is_some_and(|cur| cue < cur) {
            return None;
        }
        self.playing = Some(cue);
        Some(cue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SoundConfig {
        SoundConfig::default()
    }

    #[test]
    fn equal_or_higher_cues_replace_what_is_playing() {
        let mut p = SoundPolicy::default();
        let c = cfg();
        assert_eq!(
            p.request(Some(SoundCue::EewForecast), &c, false, false),
            Some(SoundCue::EewForecast)
        );
        assert_eq!(
            p.request(Some(SoundCue::EewForecast), &c, true, false),
            Some(SoundCue::EewForecast),
            "2 件目の予報は鳴る"
        );
        assert_eq!(
            p.request(Some(SoundCue::EewWarning), &c, true, false),
            Some(SoundCue::EewWarning)
        );
        assert_eq!(
            p.request(Some(SoundCue::EewForecast), &c, true, false),
            None,
            "警報の再生中は予報の音を捨てる"
        );
        assert_eq!(
            p.request(Some(SoundCue::EewForecast), &c, false, false),
            Some(SoundCue::EewForecast),
            "高い音が終わっていれば低い音も鳴る"
        );
    }

    #[test]
    fn a_disabled_cue_is_dropped() {
        let mut p = SoundPolicy::default();
        let mut c = cfg();
        c.eew_warning = false;
        assert_eq!(
            p.request(Some(SoundCue::EewWarning), &c, false, false),
            None
        );
        assert_eq!(
            p.request(Some(SoundCue::EewWarning), &c, false, true),
            Some(SoundCue::EewWarning),
            "試聴はオフの項目でも鳴る"
        );
    }

    #[test]
    fn zero_volume_drops_the_cue() {
        let mut p = SoundPolicy::default();
        let mut c = cfg();
        c.volume = 0.0;
        assert_eq!(
            p.request(Some(SoundCue::EewWarning), &c, false, false),
            None
        );
        assert_eq!(
            p.request(Some(SoundCue::EewWarning), &c, false, true),
            None,
            "試聴も音量 0 では無音"
        );
    }

    #[test]
    fn eew_outcomes_pick_one_cue() {
        assert_eq!(
            cue_for_eew(&ApplyOutcome::New, false),
            Some(SoundCue::EewForecast)
        );
        assert_eq!(
            cue_for_eew(&ApplyOutcome::New, true),
            Some(SoundCue::EewWarning)
        );
        assert_eq!(
            cue_for_eew(&ApplyOutcome::BecameWarning, true),
            Some(SoundCue::EewWarning)
        );
        assert_eq!(cue_for_eew(&ApplyOutcome::Updated, true), None);
        assert_eq!(cue_for_eew(&ApplyOutcome::Canceled, false), None);
        assert_eq!(
            cue_for_eew(&ApplyOutcome::Rejected("報数が古い"), true),
            None
        );
    }
}
