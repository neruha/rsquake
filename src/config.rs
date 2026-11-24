//! 設定ファイル `rsquake/config.toml` の読み書き。置き場所は OS の設定ディレクトリ
//! (Windows: `%APPDATA%`、macOS: `~/Library/Application Support`、Linux: `$XDG_CONFIG_HOME` または `~/.config`)。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// EEW の受信元
    pub source: EewSource,
    /// proxy の接続先 (`ws://host:port`)
    pub server: String,
    /// テストモードの接続先 (テストサーバー)
    pub test_server: String,
    /// 訓練報・試験報を表示する
    pub show_training: bool,
    /// 受信した envelope を JSONL に保存する
    pub log_telegrams: bool,
    pub test_mode: bool,
    /// `--test` で一時的にテストモードにしたとき、ファイルに書かれていた `test_mode`。
    /// 保存するときはこちらを書く (起動引数を設定ファイルに残さない)
    #[serde(skip)]
    pub saved_test_mode: Option<bool>,
    pub wolfx: WolfxConfig,
    pub location: Option<Location>,
    pub sound: SoundConfig,
    pub eew: EewConfig,
    pub update: UpdateConfig,
}

/// 新しいバージョンの確認 (GitHub Releases)。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UpdateConfig {
    /// 起動時に確認する
    pub check: bool,
    /// プレリリースも新版として扱う
    pub include_prerelease: bool,
    /// 新しい版を見つけたら、確認せずにダウンロードして適用する。
    /// スキップした版は適用しない。EEW が出ている間は、終わるまで適用を待つ
    pub auto_apply: bool,
    /// 通知をスキップしたバージョン (これと同じ版は、起動時にポップアップを出さない)
    pub skipped_version: Option<String>,
    /// 案内を済ませたバージョン。いまの版と違えば、起動時にリリースノート (GitHub) へ案内する。
    /// 新規インストール (設定ファイルが無い) のときは、いまの版を入れて案内しない。
    pub last_notes_version: Option<String>,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check: true,
            include_prerelease: false,
            auto_apply: false,
            skipped_version: None,
            last_notes_version: None,
        }
    }
}

/// EEW をどこから受け取るか。同時には使わない。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EewSource {
    /// DMDATA (proxy 経由)
    #[default]
    Dmdata,
    /// Wolfx Open API の `jma_eew`
    Wolfx,
}

impl EewSource {
    pub const ALL: [Self; 2] = [Self::Dmdata, Self::Wolfx];

    pub fn label(self) -> &'static str {
        match self {
            // リリース版の DMDATA はまだ作りかけ
            Self::Dmdata if !Config::DEV_FEATURES => "DMDATA (WIP)",
            Self::Dmdata => "DMDATA",
            Self::Wolfx => "Wolfx",
        }
    }

    /// 設定画面で選べるか。リリース版の DMDATA は作りかけなので選べない。
    pub fn selectable(self) -> bool {
        Config::DEV_FEATURES || self != Self::Dmdata
    }
}

/// Wolfx の接続先。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WolfxConfig {
    pub url: String,
    /// テストモードの接続先 (テストサーバーの Wolfx 用ポート)
    pub test_url: String,
}

impl Default for WolfxConfig {
    fn default() -> Self {
        Self {
            url: "wss://ws-api.wolfx.jp/jma_eew".into(),
            test_url: "ws://127.0.0.1:8081".into(),
        }
    }
}

/// EEW の表示設定。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EewConfig {
    pub region_display: RegionDisplay,
    /// 複数の EEW を切り替える間隔 (秒)
    pub switch_interval_secs: u32,
    /// 有効なイベントの予報円が収まるよう、地図を自動で寄せる
    pub auto_zoom: bool,
}

impl Default for EewConfig {
    fn default() -> Self {
        Self {
            region_display: RegionDisplay::default(),
            switch_interval_secs: crate::eew::display::DEFAULT_SWITCH_SECS,
            auto_zoom: true,
        }
    }
}

/// 細分区域パネルの表示方法。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegionDisplay {
    /// 細分区域ごとに予想震度を出す
    Detailed,
    /// 警報の地域だけを、震度なしで短くまとめて出す
    #[default]
    Summary,
}

/// 効果音。音源は同梱の 5 つに固定し、ここでは音ごとのオンオフと全体音量だけを持つ。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SoundConfig {
    /// 0.0〜1.0。0 なら試聴も含めて鳴らない
    pub volume: f32,
    pub eew_forecast: bool,
    pub eew_warning: bool,
}

impl SoundConfig {
    /// 範囲外の値は再生時に折る。
    pub fn volume(&self) -> f32 {
        self.volume.clamp(0.0, 1.0)
    }
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            volume: 0.8,
            eew_forecast: true,
            eew_warning: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Location {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source: if Config::DEV_FEATURES {
                EewSource::default()
            } else {
                EewSource::Wolfx
            },
            server: "ws://localhost:3000".into(),
            test_server: "ws://127.0.0.1:8080".into(),
            show_training: false,
            log_telegrams: false,
            test_mode: false,
            saved_test_mode: None,
            wolfx: WolfxConfig::default(),
            location: None,
            sound: SoundConfig::default(),
            eew: EewConfig::default(),
            update: UpdateConfig::default(),
        }
    }
}

/// 接続先に足すパスとクエリ。proxy とテストサーバーで共通。
pub const WS_PATH: &str = "/ws?name=rsquake&v=2&topics=eew";

impl Config {
    pub fn dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.config_dir().join("rsquake"))
    }

    pub fn path() -> Option<PathBuf> {
        Self::dir().map(|d| d.join("config.toml"))
    }

    /// テストモードとログ保存を使えるビルドか。リリースビルドでは設定画面に出さず、常に無効。
    pub const DEV_FEATURES: bool = cfg!(debug_assertions);

    /// リリースビルドでは、ファイルに書かれていてもテストモードとログ保存は無効にする。
    pub fn load() -> Self {
        let mut c = Self::load_file();
        if !Self::DEV_FEATURES {
            c.test_mode = false;
            c.log_telegrams = false;
            if !c.source.selectable() {
                c.source = EewSource::Wolfx;
            }
        }
        c
    }

    /// 読めなければ既定値。ファイルが無ければ既定値で作っておく。
    /// 読めないファイルは、あとで既定値に上書きされないよう別名で残す。
    fn load_file() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!("設定を読めないので既定値を使う: {e}");
                let backup = path.with_file_name(format!(
                    "config.broken-{}.toml",
                    chrono::Local::now().format("%Y%m%d%H%M%S")
                ));
                match std::fs::copy(&path, &backup) {
                    Ok(_) => tracing::warn!("読めない設定を {} に残した", backup.display()),
                    Err(e) => tracing::warn!("読めない設定を残せない: {e}"),
                }
                Self::default()
            }),
            Err(_) => {
                let mut c = Self::default();
                // 新規インストールには、リリースノートを案内しない
                c.update.last_notes_version = Some(env!("CARGO_PKG_VERSION").into());
                if let Err(e) = c.save() {
                    tracing::warn!("設定を保存できない: {e}");
                }
                c
            }
        }
    }

    /// 一時ファイルに書いてから置き換える。途中で落ちても元のファイルは壊れない。
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("設定の場所が分からない"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut saved = self.clone();
        if let Some(t) = self.saved_test_mode {
            saved.test_mode = t;
        }
        let tmp = path.with_extension("toml.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(toml::to_string_pretty(&saved)?.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// 起動引数 `--test`。このセッションだけテストモードにし、設定ファイルには残さない。
    pub fn force_test_mode(&mut self) {
        if !Self::DEV_FEATURES {
            return;
        }
        if self.saved_test_mode.is_none() {
            self.saved_test_mode = Some(self.test_mode);
        }
        self.test_mode = true;
    }

    /// 受信元に応じた接続先。DMDATA は proxy の `/ws?...`、Wolfx は設定の URL そのまま。
    pub fn ws_url(&self) -> String {
        match self.source {
            EewSource::Dmdata => {
                let base = if self.test_mode {
                    &self.test_server
                } else {
                    &self.server
                };
                format!("{}{}", base.trim_end_matches('/'), WS_PATH)
            }
            EewSource::Wolfx => {
                if self.test_mode {
                    self.wolfx.test_url.clone()
                } else {
                    self.wolfx.url.clone()
                }
            }
        }
    }

    pub fn accept_non_normal(&self) -> bool {
        self.show_training || self.test_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_toml_uses_defaults() {
        let c: Config = toml::from_str(
            "test_mode = true\n[location]\nname = \"東京\"\nlatitude = 35.68\nlongitude = 139.76\n",
        )
        .unwrap();
        assert!(c.test_mode);
        assert_eq!(c.server, "ws://localhost:3000");
        assert_eq!(
            c.ws_url(),
            "ws://127.0.0.1:8080/ws?name=rsquake&v=2&topics=eew"
        );
        assert_eq!(c.location.unwrap().name, "東京");
        assert_eq!(c.sound, SoundConfig::default());
    }

    #[test]
    #[cfg(debug_assertions)]
    fn source_defaults_to_dmdata_and_old_config_still_loads() {
        let c: Config = toml::from_str("server = \"ws://example:3000\"\n").unwrap();
        assert_eq!(c.source, EewSource::Dmdata);
        assert_eq!(
            c.ws_url(),
            "ws://example:3000/ws?name=rsquake&v=2&topics=eew"
        );
        assert_eq!(c.wolfx, WolfxConfig::default());
    }

    #[test]
    fn update_settings_default_on_and_old_config_still_loads() {
        let c: Config = toml::from_str("test_mode = true\n").unwrap();
        assert_eq!(c.update, UpdateConfig::default());
        assert!(c.update.check);
        assert!(!c.update.include_prerelease);
        assert!(!c.update.auto_apply);
        let c: Config =
            toml::from_str("[update]\ninclude_prerelease = true\nskipped_version = \"0.3.0\"\n")
                .unwrap();
        assert!(c.update.check);
        assert!(c.update.include_prerelease);
        assert_eq!(c.update.skipped_version.as_deref(), Some("0.3.0"));
        // 保存して読み直しても変わらない
        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back.update, c.update);
    }

    #[test]
    fn wolfx_source_roundtrip_and_urls() {
        let mut c: Config = toml::from_str("source = \"wolfx\"\n").unwrap();
        assert_eq!(c.source, EewSource::Wolfx);
        assert_eq!(c.ws_url(), "wss://ws-api.wolfx.jp/jma_eew");
        c.test_mode = true;
        assert_eq!(c.ws_url(), "ws://127.0.0.1:8081");

        let c: Config =
            toml::from_str("source = \"wolfx\"\n[wolfx]\nurl = \"wss://x.example/e\"\n").unwrap();
        assert_eq!(c.ws_url(), "wss://x.example/e");
        assert_eq!(
            c.wolfx.test_url, "ws://127.0.0.1:8081",
            "書かなかった項目は既定値"
        );

        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn sound_settings_have_defaults_and_roundtrip() {
        let c = Config::default();
        assert_eq!(c.eew.region_display, RegionDisplay::Summary);
        assert_eq!(c.eew.switch_interval_secs, 5);
        let c: Config = toml::from_str("test_mode = true\n").unwrap();
        assert_eq!(c.eew.region_display, RegionDisplay::Summary);
        let c: Config =
            toml::from_str("[eew]\nregion_display = \"detailed\"\nswitch_interval_secs = 8\n")
                .unwrap();
        assert_eq!(c.eew.region_display, RegionDisplay::Detailed);
        assert_eq!(c.eew.switch_interval_secs, 8);
        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back.eew, c.eew);

        let c = Config::default();
        assert_eq!(c.sound.volume, 0.8);
        assert!(c.sound.eew_warning);

        let c: Config = toml::from_str("[sound]\nvolume = 0.25\neew_forecast = false\n").unwrap();
        assert!((c.sound.volume - 0.25).abs() < 1e-6);
        assert!(!c.sound.eew_forecast);
        assert!(c.sound.eew_warning, "書かなかった項目は既定値");

        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back.sound, c.sound);
    }

    #[test]
    fn old_shake_sound_keys_still_load() {
        let c: Config =
            toml::from_str("[sound]\nshake_weak = false\nshake_strong = true\n").unwrap();
        assert_eq!(c.sound, SoundConfig::default());
    }

    #[test]
    fn forced_test_mode_is_not_serialized() {
        let mut c = Config::default();
        c.force_test_mode();
        assert!(c.test_mode);
        assert_eq!(c.saved_test_mode, Some(false));
        let text = toml::to_string_pretty(&c).unwrap();
        assert!(!text.contains("saved_test_mode"));
    }
}
