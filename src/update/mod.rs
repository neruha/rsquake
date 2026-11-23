//! 新しいバージョンの確認。GitHub Releases の一覧を起動時に 1 回だけ取り、
//! いまより新しい版があるかを UI に渡す。
//!
//! 通知だけで、ダウンロードや差し替えは行わない。リリースノートも取り込まず、
//! GitHub の Release のページをブラウザで開いて見てもらう。
//! 失敗しても何も出さず、設定画面の状態にだけ反映する。

use std::sync::mpsc::Sender;
use std::time::Duration;

use semver::Version;
use serde::Deserialize;

use crate::config::UpdateConfig;

/// Release のページの先頭。ブラウザで開くのは、これで始まる URL だけ。
pub const RELEASES_PAGE: &str = "https://github.com/neruha/rsquake/releases";
const API_URL: &str = "https://api.github.com/repos/neruha/rsquake/releases?per_page=30";
const TIMEOUT: Duration = Duration::from_secs(10);

/// 通知のポップアップを自動で閉じるまでの秒数。
pub const POPUP_SECS: f32 = 10.0;

/// いまのバージョン。
pub fn current_version() -> Version {
    #[cfg(debug_assertions)]
    if let Some(v) = std::env::var("RSQUAKE_UPDATE_CURRENT")
        .ok()
        .and_then(|v| Version::parse(&v).ok())
    {
        return v;
    }
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo.toml の version が semver でない")
}

/// ある版の Release のページ。
pub fn release_url(version: &Version) -> String {
    format!("{RELEASES_PAGE}/tag/v{version}")
}

/// 開いてよい URL か。リリースのページ以外は開かない。
pub fn is_release_url(url: &str) -> bool {
    url == RELEASES_PAGE || url.starts_with(&format!("{RELEASES_PAGE}/"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// ブラウザで開く URL。必ず `RELEASES_PAGE` 以下
    pub url: String,
}

/// 確認の結果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckResult {
    /// いまより新しい版のうち、いちばん新しいもの
    pub latest: Option<Release>,
}

#[derive(Debug)]
pub enum UpdateEvent {
    Checked(CheckResult),
    Failed(String),
}

/// 設定画面が見る、確認の状態。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// まだ確認していない (自動確認がオフなど)
    #[default]
    Idle,
    Checking,
    Done,
    Failed,
}

#[derive(Clone, Debug, Default)]
pub struct UpdateInfo {
    pub phase: Phase,
    pub result: CheckResult,
}

/// ポップアップで出す通知。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// 新しい版がある
    Update(Release),
    /// 版が上がった後の初めての起動。いまの版のリリースノート (GitHub) へ案内する
    Upgraded(Version),
}

/// 出す通知を決める。`result` は確認の結果 (確認しない・失敗したときは `None`)。
/// - 新しい版があり、スキップしていなければ、それを出す
/// - なければ、版が上がった後の初回だけ、いまの版のリリースノートへ案内する
/// - EEW が出ている間は出さない (あとからも出さない)
pub fn choose_notice(
    result: Option<&CheckResult>,
    config: &UpdateConfig,
    current: &Version,
    eew_active: bool,
) -> Option<Notice> {
    if eew_active {
        return None;
    }
    if let Some(latest) = result.and_then(|r| r.latest.as_ref())
        && config.skipped_version.as_deref() != Some(&latest.version.to_string())
    {
        return Some(Notice::Update(latest.clone()));
    }
    (config.last_notes_version.as_deref() != Some(&current.to_string()))
        .then(|| Notice::Upgraded(current.clone()))
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

struct Parsed {
    release: Release,
    prerelease: bool,
}

/// タグ (`v0.2.0` / `0.2.0`) をバージョンにする。
fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

fn parse_releases(json: &[u8]) -> Result<Vec<Parsed>, serde_json::Error> {
    let raw: Vec<RawRelease> = serde_json::from_slice(json)?;
    Ok(raw
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version = parse_tag(&r.tag_name)?;
            // API が返す URL は信用せず、リリースのページ以外なら自分で作る
            let url = if is_release_url(&r.html_url) {
                r.html_url
            } else {
                release_url(&version)
            };
            Some(Parsed {
                release: Release { version, url },
                prerelease: r.prerelease,
            })
        })
        .collect())
}

/// Release の一覧から、いまより新しい版を選ぶ。
/// プレリリースを含めない設定のときは、プレリリースを除く。
fn pick(releases: Vec<Parsed>, current: &Version, include_prerelease: bool) -> CheckResult {
    let latest = releases
        .into_iter()
        .filter(|p| include_prerelease || !p.prerelease)
        .filter(|p| p.release.version > *current)
        .max_by(|a, b| a.release.version.cmp(&b.release.version))
        .map(|p| p.release);
    CheckResult { latest }
}

/// 確認の設定。
#[derive(Clone, Debug)]
pub struct CheckOptions {
    pub url: String,
    pub include_prerelease: bool,
    pub current: Version,
}

impl CheckOptions {
    pub fn new(include_prerelease: bool) -> Self {
        // デバッグビルドだけ、試験用に接続先を差し替えられる (手元のモックサーバー)
        #[cfg(debug_assertions)]
        let url = std::env::var("RSQUAKE_UPDATE_URL").unwrap_or_else(|_| API_URL.to_string());
        #[cfg(not(debug_assertions))]
        let url = API_URL.to_string();
        Self {
            url,
            include_prerelease,
            current: current_version(),
        }
    }
}

async fn fetch(opts: &CheckOptions) -> anyhow::Result<CheckResult> {
    crate::net::install_tls_provider();
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .user_agent(concat!("rsquake/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let res = client
        .get(&opts.url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?;
    let body = res.bytes().await?;
    let releases = parse_releases(&body)?;
    Ok(pick(releases, &opts.current, opts.include_prerelease))
}

/// 別スレッドで 1 回だけ確認し、結果を `tx` に送る。
pub fn spawn(opts: CheckOptions, tx: Sender<UpdateEvent>, ctx: egui::Context) {
    let spawned = std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let event = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => match rt.block_on(fetch(&opts)) {
                    Ok(r) => UpdateEvent::Checked(r),
                    Err(e) => UpdateEvent::Failed(format!("{e:#}")),
                },
                Err(e) => UpdateEvent::Failed(e.to_string()),
            };
            let _ = tx.send(event);
            ctx.request_repaint();
        });
    if let Err(e) = spawned {
        tracing::warn!("更新の確認を始められない: {e}");
    }
}

/// ポップアップを閉じるまでの残り時間。マウスが乗っている間は進めない。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Countdown {
    left: f32,
}

impl Default for Countdown {
    fn default() -> Self {
        Self { left: POPUP_SECS }
    }
}

impl Countdown {
    /// `dt` 秒進める。時間切れなら `true`。
    /// 描画の中断などで `dt` が大きくなっても、1 回で進めるのは 1 秒まで。
    pub fn tick(&mut self, dt: f32, paused: bool) -> bool {
        if !paused {
            self.left = (self.left - dt.clamp(0.0, 1.0)).max(0.0);
        }
        self.left <= 0.0
    }

    /// 残りの割合 (1.0 → 0.0)
    pub fn fraction(&self) -> f32 {
        (self.left / POPUP_SECS).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn rel(version: &str) -> Release {
        Release {
            version: v(version),
            url: release_url(&v(version)),
        }
    }

    const JSON: &str = r#"[
        {"tag_name":"v0.4.0-beta.1","html_url":"https://github.com/neruha/rsquake/releases/tag/v0.4.0-beta.1","draft":false,"prerelease":true},
        {"tag_name":"v0.3.0","html_url":"https://github.com/neruha/rsquake/releases/tag/v0.3.0","draft":false,"prerelease":false},
        {"tag_name":"v0.9.0","html_url":"https://github.com/neruha/rsquake/releases/tag/v0.9.0","draft":true,"prerelease":false},
        {"tag_name":"v0.2.0","html_url":"https://evil.example/x","draft":false,"prerelease":false},
        {"tag_name":"nightly","html_url":"","draft":false,"prerelease":false},
        {"tag_name":"v0.1.0","html_url":"https://github.com/neruha/rsquake/releases/tag/v0.1.0","draft":false,"prerelease":false}
    ]"#;

    fn check(current: &str, pre: bool) -> CheckResult {
        pick(parse_releases(JSON.as_bytes()).unwrap(), &v(current), pre)
    }

    #[test]
    fn tags_with_or_without_v_parse() {
        assert_eq!(parse_tag("v1.2.3"), Some(v("1.2.3")));
        assert_eq!(parse_tag("1.2.3-beta.1"), Some(v("1.2.3-beta.1")));
        assert_eq!(parse_tag("nightly"), None);
    }

    #[test]
    fn drafts_and_unparsable_tags_are_ignored() {
        let all = parse_releases(JSON.as_bytes()).unwrap();
        let versions: Vec<_> = all.iter().map(|p| p.release.version.to_string()).collect();
        assert_eq!(versions, ["0.4.0-beta.1", "0.3.0", "0.2.0", "0.1.0"]);
    }

    #[test]
    fn foreign_urls_are_replaced_with_the_tag_page() {
        let all = parse_releases(JSON.as_bytes()).unwrap();
        assert_eq!(all[2].release.url, release_url(&v("0.2.0")));
        assert!(all.iter().all(|p| is_release_url(&p.release.url)));
        assert!(!is_release_url(
            "https://github.com/neruha/rsquake-evil/releases"
        ));
        assert!(!is_release_url(
            "https://github.com/neruha/rsquake/releases.evil"
        ));
    }

    #[test]
    fn latest_skips_prereleases_unless_asked() {
        assert_eq!(check("0.1.0", false).latest, Some(rel("0.3.0")));
        assert_eq!(check("0.1.0", true).latest, Some(rel("0.4.0-beta.1")));
    }

    #[test]
    fn no_update_when_current_is_newest() {
        assert_eq!(check("0.3.0", false).latest, None);
        // 公開されていない版 (開発中) でも、より新しい版が無ければ何も出ない
        assert_eq!(check("0.3.5", false).latest, None);
    }

    #[test]
    fn prerelease_current_is_older_than_its_release() {
        // 0.4.0-alpha.1 は 0.3.0 より新しく、0.4.0-beta.1 はプレリリースなので除かれる
        assert_eq!(check("0.4.0-alpha.1", false).latest, None);
        assert_eq!(
            check("0.4.0-alpha.1", true).latest,
            Some(rel("0.4.0-beta.1"))
        );
        let releases = vec![Parsed {
            release: rel("0.2.0"),
            prerelease: false,
        }];
        assert_eq!(
            pick(releases, &v("0.2.0-beta.1"), false).latest,
            Some(rel("0.2.0"))
        );
    }

    fn cfg(skipped: Option<&str>, notes: Option<&str>) -> UpdateConfig {
        UpdateConfig {
            skipped_version: skipped.map(String::from),
            last_notes_version: notes.map(String::from),
            ..UpdateConfig::default()
        }
    }

    fn found(latest: &str) -> CheckResult {
        CheckResult {
            latest: Some(rel(latest)),
        }
    }

    #[test]
    fn notice_prefers_update_then_upgraded() {
        let cur = v("0.1.0");
        let res = found("0.3.0");
        assert_eq!(
            choose_notice(Some(&res), &cfg(None, Some("0.1.0")), &cur, false),
            Some(Notice::Update(rel("0.3.0")))
        );
        // スキップした版は出さず、案内も済んでいれば何も出ない
        assert_eq!(
            choose_notice(Some(&res), &cfg(Some("0.3.0"), Some("0.1.0")), &cur, false),
            None
        );
        // スキップしていても、案内がまだならそちらを出す
        assert_eq!(
            choose_notice(Some(&res), &cfg(Some("0.3.0"), Some("0.0.9")), &cur, false),
            Some(Notice::Upgraded(cur.clone()))
        );
        // さらに新しい版が出たら、スキップしていても出す
        assert_eq!(
            choose_notice(
                Some(&found("0.4.0")),
                &cfg(Some("0.3.0"), Some("0.1.0")),
                &cur,
                false
            ),
            Some(Notice::Update(rel("0.4.0")))
        );
    }

    #[test]
    fn upgrade_notice_works_without_a_check_result() {
        let cur = v("0.2.0");
        // 確認しない・失敗したときも、版が上がっていれば案内する (ネットワーク不要)
        assert_eq!(
            choose_notice(None, &cfg(None, None), &cur, false),
            Some(Notice::Upgraded(cur.clone()))
        );
        assert_eq!(
            choose_notice(None, &cfg(None, Some("0.1.0")), &cur, false),
            Some(Notice::Upgraded(cur.clone()))
        );
        assert_eq!(
            choose_notice(None, &cfg(None, Some("0.2.0")), &cur, false),
            None
        );
    }

    #[test]
    fn notice_is_suppressed_during_eew() {
        assert_eq!(
            choose_notice(Some(&found("0.3.0")), &cfg(None, None), &v("0.1.0"), true),
            None
        );
        assert_eq!(
            choose_notice(None, &cfg(None, None), &v("0.1.0"), true),
            None
        );
    }

    /// 実際の GitHub API に https でつなぐ (TLS の構成の確認用)。`cargo test -- --ignored`
    #[test]
    #[ignore = "network"]
    fn live_github_api_over_https() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let opts = CheckOptions::new(false);
        let r = rt
            .block_on(fetch(&opts))
            .expect("GitHub API に https でつなげない");
        println!("{r:?}");
    }

    #[test]
    fn countdown_runs_pauses_and_expires() {
        let mut c = Countdown::default();
        assert!(!c.tick(0.5, false));
        // 止めている間は進まない
        assert!(!c.tick(1.0, true));
        assert!((c.fraction() - 0.95).abs() < 1e-6);
        // 1 回で進めるのは 1 秒まで
        let mut c = Countdown::default();
        assert!(!c.tick(100.0, false));
        assert!((c.fraction() - 0.9).abs() < 1e-6);
        for _ in 0..8 {
            assert!(!c.tick(1.0, false));
        }
        assert!(c.tick(1.0, false));
        assert_eq!(c.fraction(), 0.0);
    }
}
