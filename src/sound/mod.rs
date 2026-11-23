//! 同梱の効果音。判定は [`policy`]。出力装置は起動時に開き、開けなければ
//! 間をおいて開き直す (あとから挿したスピーカーでも鳴るように)。

mod policy;

pub use policy::{SoundCue, SoundPolicy};

use std::io::Cursor;
use std::time::{Duration, Instant};

use rodio::Source;

use crate::config::SoundConfig;
use crate::eew::store::ApplyOutcome;

const FORECAST: &[u8] = include_bytes!("../../assets/sounds/eew_forecast.mp3");
const WARNING: &[u8] = include_bytes!("../../assets/sounds/eew_warn.mp3");

/// 出力装置を開けなかったあと、次に開き直すまでの間隔
const RETRY_AFTER: Duration = Duration::from_secs(5);

struct Clip {
    channels: rodio::ChannelCount,
    sample_rate: rodio::SampleRate,
    samples: Vec<rodio::Sample>,
}

impl Clip {
    fn source(&self) -> rodio::buffer::SamplesBuffer {
        rodio::buffer::SamplesBuffer::new(self.channels, self.sample_rate, self.samples.clone())
    }
}

fn decode(bytes: &'static [u8]) -> Clip {
    let decoder = rodio::Decoder::new_mp3(Cursor::new(bytes)).expect("同梱の効果音が読めない");
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let samples = decoder.collect();
    Clip {
        channels,
        sample_rate,
        samples,
    }
}

struct Clips {
    forecast: Clip,
    warning: Clip,
}

impl Clips {
    fn load() -> Self {
        Self {
            forecast: decode(FORECAST),
            warning: decode(WARNING),
        }
    }

    fn get(&self, cue: SoundCue) -> &Clip {
        match cue {
            SoundCue::EewForecast => &self.forecast,
            SoundCue::EewWarning => &self.warning,
        }
    }
}

struct Output {
    /// 落とすと再生が止まる
    _device: rodio::MixerDeviceSink,
    player: rodio::Player,
}

/// UI スレッドが持つ再生器。装置を開けなくても画面はそのまま動く。
pub struct SoundPlayer {
    policy: SoundPolicy,
    clips: Clips,
    output: Option<Output>,
    /// 最後に開けなかった時刻。`RETRY_AFTER` たつまで開き直さない
    failed_at: Option<Instant>,
}

impl SoundPlayer {
    /// 効果音を読み込み、出力装置を開いておく (最初の EEW で UI が止まらないように)。
    pub fn new() -> Self {
        let mut p = Self {
            policy: SoundPolicy::default(),
            clips: Clips::load(),
            output: None,
            failed_at: None,
        };
        p.ensure_output();
        p
    }

    pub fn on_eew(&mut self, outcome: &ApplyOutcome, is_warning: bool, cfg: &SoundConfig) {
        self.start(policy::cue_for_eew(outcome, is_warning), cfg, false);
    }

    /// 設定画面の試聴。項目がオフでも、音量が 0 でなければ鳴らす。
    pub fn preview(&mut self, cue: SoundCue, cfg: &SoundConfig) {
        self.start(Some(cue), cfg, true);
    }

    pub fn set_volume(&mut self, volume: f32) {
        if let Some(output) = &self.output {
            output.player.set_volume(volume.clamp(0.0, 1.0));
        }
    }

    fn start(&mut self, cue: Option<SoundCue>, cfg: &SoundConfig, force: bool) {
        let Some(cue) = self.policy.request(cue, cfg, self.still_playing(), force) else {
            return;
        };
        self.play(cue, cfg.volume());
    }

    fn still_playing(&self) -> bool {
        self.output.as_ref().is_some_and(|o| !o.player.empty())
    }

    /// 出力装置が開いていなければ開く。失敗したら `RETRY_AFTER` の間は試さない。
    fn ensure_output(&mut self) {
        if self.output.is_some() || self.failed_at.is_some_and(|t| t.elapsed() < RETRY_AFTER) {
            return;
        }
        match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(device) => {
                let player = rodio::Player::connect_new(device.mixer());
                if self.failed_at.take().is_some() {
                    tracing::info!("効果音の出力を開けた");
                }
                self.output = Some(Output {
                    _device: device,
                    player,
                });
            }
            Err(e) => {
                if self.failed_at.is_none() {
                    tracing::warn!("効果音の出力を開けない: {e}");
                }
                self.failed_at = Some(Instant::now());
            }
        }
    }

    fn play(&mut self, cue: SoundCue, volume: f32) {
        self.ensure_output();
        let Some(output) = &self.output else { return };
        let source = self.clips.get(cue).source();
        // 今の音を飛ばして、新しい音を頭から再生する。`stop` は次の append が終わるまで待つので使わない。
        if !output.player.empty() {
            output.player.skip_one();
        }
        output.player.set_volume(volume);
        output.player.append(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sounds_decode() {
        let clips = Clips::load();
        for cue in SoundCue::ALL {
            assert!(!clips.get(cue).samples.is_empty(), "{}", cue.label());
        }
    }
}
