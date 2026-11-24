//! 配布物のダウンロードと確認。別スレッドで行い、進み具合を `UpdateEvent` で本体に送る。
//!
//! 1. `SHA256SUMS` と `SHA256SUMS.minisig` を取り、署名を確かめる
//! 2. 配布物を流しながら取り、SHA-256 を計算する
//! 3. `SHA256SUMS` の該当行と合えば、確認済みとして作業フォルダに置く
//!
//! どれかが合わなければ、取ったファイルを消して失敗にする。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};

use super::UpdateEvent;
use super::asset::{Asset, Plan, is_download_url, is_redirect_url};
use super::verify::{self, StreamHash};
use crate::config::Config;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 受信が途切れてよい時間。回線が細くても、止まらず進んでいれば待つ。
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// `SHA256SUMS` と署名の大きさの上限
const SMALL_LIMIT: u64 = 1024 * 1024;
/// 配布物の大きさの上限 (想定の数十 MB に対し、余裕を見た値)
const ASSET_LIMIT: u64 = 400 * 1024 * 1024;

/// 確認まで済んだ配布物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    pub plan: Plan,
    /// 確認済みの配布物
    pub file: PathBuf,
    /// `file` の SHA-256 (適用の直前にもう一度確かめる)
    pub sha256: String,
    /// 作業フォルダ (`file` はこの中)
    pub work_dir: PathBuf,
}

#[derive(Debug)]
pub enum DownloadEvent {
    Progress {
        done: u64,
        total: u64,
    },
    /// 署名とハッシュを確かめている
    Verifying,
    Ready(Box<Staged>),
    Failed(String),
}

/// 更新の作業フォルダ (設定フォルダの下の `update/`)。
pub fn work_dir() -> Option<PathBuf> {
    Config::dir().map(|d| d.join("update"))
}

fn client() -> anyhow::Result<reqwest::Client> {
    crate::net::install_tls_provider();
    let redirect = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error("リダイレクトが多すぎます")
        } else if is_redirect_url(attempt.url().as_str()) {
            attempt.follow()
        } else {
            attempt.error("配布元以外へのリダイレクトは受け付けません")
        }
    });
    Ok(reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .redirect(redirect)
        .user_agent(concat!("rsquake/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// 1 つのファイルを `dest` に取る。`limit` を超えたら止める。`on_chunk` に受け取った量を渡す。
async fn get_to_file(
    client: &reqwest::Client,
    asset: &Asset,
    dest: &Path,
    limit: u64,
    mut on_chunk: impl FnMut(&[u8], u64, u64),
) -> anyhow::Result<()> {
    if !is_download_url(&asset.url) {
        bail!("{} の URL が配布元のものではありません", asset.name);
    }
    let mut res = client
        .get(&asset.url)
        .send()
        .await
        .with_context(|| format!("{} を取得できません", asset.name))?
        .error_for_status()
        .with_context(|| format!("{} を取得できません", asset.name))?;
    let total = res.content_length().unwrap_or(asset.size);
    if total > limit {
        bail!("{} が大きすぎます", asset.name);
    }
    let mut file = std::fs::File::create(dest)?;
    let mut done = 0u64;
    while let Some(chunk) = res
        .chunk()
        .await
        .with_context(|| format!("{} の受信が途切れました", asset.name))?
    {
        done += chunk.len() as u64;
        if done > limit {
            bail!("{} が大きすぎます", asset.name);
        }
        file.write_all(&chunk)?;
        on_chunk(&chunk, done, total);
    }
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

/// ダウンロードから確認までを行う。`progress` には (受け取った量, 全体) を渡す。
pub async fn fetch(
    plan: &Plan,
    work_dir: &Path,
    public_key: &str,
    mut progress: impl FnMut(u64, u64),
    mut verifying: impl FnMut(),
) -> anyhow::Result<Staged> {
    // 前回の残りは捨てて、空の作業フォルダから始める
    let _ = std::fs::remove_dir_all(work_dir);
    std::fs::create_dir_all(work_dir)?;
    let client = client()?;

    let sums_path = work_dir.join(&plan.sums.name);
    let sig_path = work_dir.join(&plan.sig.name);
    get_to_file(&client, &plan.sums, &sums_path, SMALL_LIMIT, |_, _, _| {}).await?;
    get_to_file(&client, &plan.sig, &sig_path, SMALL_LIMIT, |_, _, _| {}).await?;

    // 先に署名を確かめる。配布物は、署名された一覧に載っている物だけ取る
    let sums = std::fs::read(&sums_path)?;
    let sig = std::fs::read_to_string(&sig_path).context("署名を読めません")?;
    verify::verify_sums(&sums, &sig, public_key).map_err(|e| anyhow!(e))?;
    let sums = String::from_utf8(sums).context("SHA256SUMS が文字列ではありません")?;
    let expected = verify::expected_hash(&sums, &plan.asset.name)
        .ok_or_else(|| anyhow!("SHA256SUMS に {} がありません", plan.asset.name))?;

    let part = work_dir.join(format!("{}.part", plan.asset.name));
    let mut hash = StreamHash::default();
    get_to_file(
        &client,
        &plan.asset,
        &part,
        ASSET_LIMIT,
        |chunk, done, total| {
            hash.update(chunk);
            progress(done, total);
        },
    )
    .await?;

    verifying();
    let actual = hash.finish();
    if actual != expected {
        let _ = std::fs::remove_file(&part);
        bail!("{} のハッシュが SHA256SUMS と合いません", plan.asset.name);
    }
    let file = work_dir.join(&plan.asset.name);
    std::fs::rename(&part, &file)?;
    Ok(Staged {
        plan: plan.clone(),
        file,
        sha256: actual,
        work_dir: work_dir.to_path_buf(),
    })
}

/// 別スレッドでダウンロードして確認し、進み具合を `tx` に送る。
pub fn spawn(plan: Plan, tx: Sender<UpdateEvent>, ctx: egui::Context) {
    let send = move |tx: &Sender<UpdateEvent>, ctx: &egui::Context, ev: DownloadEvent| {
        let _ = tx.send(UpdateEvent::Download(ev));
        ctx.request_repaint();
    };
    let spawned = std::thread::Builder::new()
        .name("update-download".into())
        .spawn({
            let (tx, ctx) = (tx.clone(), ctx.clone());
            move || {
                let result = (|| -> anyhow::Result<Staged> {
                    let dir =
                        work_dir().ok_or_else(|| anyhow!("作業フォルダの場所が分かりません"))?;
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    // 進行状況は、1% 進むごとにだけ送る (毎チャンクだと画面の再描画が多すぎる)
                    let mut last_pct = u64::MAX;
                    let (tx2, ctx2) = (tx.clone(), ctx.clone());
                    let (tx3, ctx3) = (tx.clone(), ctx.clone());
                    rt.block_on(fetch(
                        &plan,
                        &dir,
                        &verify::public_key(),
                        move |done, total| {
                            let pct = (done * 100).checked_div(total).unwrap_or(done);
                            if pct != last_pct {
                                last_pct = pct;
                                send(&tx2, &ctx2, DownloadEvent::Progress { done, total });
                            }
                        },
                        move || send(&tx3, &ctx3, DownloadEvent::Verifying),
                    ))
                })();
                let ev = match result {
                    Ok(staged) => DownloadEvent::Ready(Box::new(staged)),
                    Err(e) => DownloadEvent::Failed(format!("{e:#}")),
                };
                send(&tx, &ctx, ev);
            }
        });
    if let Err(e) = spawned {
        let _ = tx.send(UpdateEvent::Download(DownloadEvent::Failed(format!(
            "スレッドを作れません: {e}"
        ))));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use semver::Version;

    use super::*;
    use crate::update::asset::{SIG_NAME, SUMS_NAME, Target};

    const TEST_KEY: &str = "RWRRS95L2SSVpPjPdK8xSg6CzI3rrNyG8zvOLLP8J22P25qPh7oys6PB";
    /// `hello` の SHA-256 を載せた SHA256SUMS (試験用の鍵で署名)
    const SUMS: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  RSQuake-0.2.0-macos-arm64.dmg\n";
    const SIG: &str = "untrusted comment: test
RURRS95L2SSVpDCz+pYrf7v1Ox+dpgoLApfQFKXNUHnhgYM1yub6fut1h1Ac/VI653SD/xhdZ1hjHfBxxugNu7Xe0srIaQ27XQs=
trusted comment: timestamp:1795487346\tfile:SHA256SUMS\thashed
A1359ZXvOkD3Zuh6PRbtZUgW1y9bNaAMdCa/F+5yI6sRcOp6JKID1ID9NtPvMPjFS8qfcOVTN0xg8HSD2ZEoDg==
";

    /// 決まったパスに決まった中身を返すだけの HTTP サーバー (1 接続 1 リクエスト)。
    fn serve(files: Vec<(&'static str, Vec<u8>)>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let body = files.iter().find(|(p, _)| *p == path).map(|(_, b)| b);
                let head = match body {
                    Some(b) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        b.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_string()
                    }
                };
                let _ = s.write_all(head.as_bytes());
                if let Some(b) = body {
                    let _ = s.write_all(b);
                }
            }
        });
        port
    }

    fn plan(port: u16) -> Plan {
        let a = |name: &str, size: u64| Asset {
            name: name.into(),
            url: format!("http://127.0.0.1:{port}/{name}"),
            size,
        };
        Plan {
            version: Version::parse("0.2.0").unwrap(),
            target: Target::MacApp {
                app: "/A/RSQuake.app".into(),
            },
            asset: a("RSQuake-0.2.0-macos-arm64.dmg", 5),
            sums: a(SUMS_NAME, SUMS.len() as u64),
            sig: a(SIG_NAME, SIG.len() as u64),
        }
    }

    fn run(
        files: Vec<(&'static str, Vec<u8>)>,
        key: &str,
    ) -> (anyhow::Result<Staged>, tempfile::TempDir, Vec<u64>) {
        let port = serve(files);
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("update");
        let mut seen = Vec::new();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let r = rt.block_on(fetch(
            &plan(port),
            &work,
            key,
            |done, _| seen.push(done),
            || {},
        ));
        (r, dir, seen)
    }

    fn good_files() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("/RSQuake-0.2.0-macos-arm64.dmg", b"hello".to_vec()),
            ("/SHA256SUMS", SUMS.as_bytes().to_vec()),
            ("/SHA256SUMS.minisig", SIG.as_bytes().to_vec()),
        ]
    }

    #[test]
    fn good_download_is_staged() {
        let (r, dir, seen) = run(good_files(), TEST_KEY);
        let staged = r.unwrap();
        assert_eq!(std::fs::read(&staged.file).unwrap(), b"hello");
        assert!(staged.file.starts_with(dir.path()));
        assert_eq!(seen.last(), Some(&5));
        // 確認前の一時ファイルは残らない
        assert!(
            !staged
                .work_dir
                .join("RSQuake-0.2.0-macos-arm64.dmg.part")
                .exists()
        );
    }

    #[test]
    fn tampered_asset_is_rejected_and_removed() {
        let mut files = good_files();
        files[0].1 = b"HELLO".to_vec();
        let (r, dir, _) = run(files, TEST_KEY);
        assert!(r.unwrap_err().to_string().contains("ハッシュ"));
        let work = dir.path().join("update");
        assert!(!work.join("RSQuake-0.2.0-macos-arm64.dmg").exists());
        assert!(!work.join("RSQuake-0.2.0-macos-arm64.dmg.part").exists());
    }

    #[test]
    fn unsigned_or_wrongly_signed_sums_are_rejected_before_the_asset_is_fetched() {
        // 署名が別の鍵のもの
        let (r, dir, seen) = run(good_files(), crate::update::verify::PUBLIC_KEY);
        assert!(r.unwrap_err().to_string().contains("署名"));
        assert!(seen.is_empty(), "署名が通らないのに配布物を取った");
        assert!(
            !dir.path()
                .join("update/RSQuake-0.2.0-macos-arm64.dmg.part")
                .exists()
        );
        // SHA256SUMS が書き換えられている
        let mut files = good_files();
        files[1].1 = SUMS.replace("2cf2", "0000").into_bytes();
        assert!(run(files, TEST_KEY).0.is_err());
        // 署名ファイルが無い
        let files = good_files().into_iter().take(2).collect();
        assert!(run(files, TEST_KEY).0.is_err());
    }

    #[test]
    fn asset_missing_from_sums_is_rejected() {
        let mut files = good_files();
        files[1].1 =
            b"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  other.dmg\n"
                .to_vec();
        // 一覧が変わると署名も合わないので、どちらにしても失敗する
        assert!(run(files, TEST_KEY).0.is_err());
    }

    #[test]
    fn foreign_asset_url_is_refused() {
        let mut p = plan(1);
        p.asset.url = "https://evil.example/x.dmg".into();
        let dir = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let port = serve(good_files());
        p.sums.url = format!("http://127.0.0.1:{port}/SHA256SUMS");
        p.sig.url = format!("http://127.0.0.1:{port}/SHA256SUMS.minisig");
        let r = rt.block_on(fetch(&p, &dir.path().join("u"), TEST_KEY, |_, _| {}, || {}));
        assert!(r.unwrap_err().to_string().contains("配布元"));
    }
}
