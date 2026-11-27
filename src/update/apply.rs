//! 確認済みの配布物を入れ替えて、新しい版で起動し直す。
//!
//! 入れ替えは、アプリ自身を別の引数で起動した「適用役」に任せる。
//!
//! ```text
//! RSQuake --apply-update <作業フォルダ> --wait-pid <本体の PID>
//! ```
//!
//! 本体は `start` で作業フォルダに `job.json` を書き、実行ファイルのコピーを適用役として起動して、
//! すぐ終了する。適用役 (`run`) は本体の終了を待ってから、OS ごとの方法で入れ替える。
//! 結果は `result.json` に書き、次に起動した本体が `startup` で読んで、失敗していれば知らせる。
//! 失敗したときは、元の版のまま起動し直す。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow, bail};
use serde::{Deserialize, Serialize};

use super::asset::{Env, Target, detect};
use super::download::{Staged, work_dir};
use super::verify;

/// 起動の引数。
pub const ARG_APPLY: &str = "--apply-update";
pub const ARG_WAIT_PID: &str = "--wait-pid";

const JOB_FILE: &str = "job.json";
const RESULT_FILE: &str = "result.json";
const LOG_FILE: &str = "apply.log";
/// 本体の終了を待つ上限
const WAIT_LIMIT: Duration = Duration::from_secs(60);
/// MSI が正常終了したときの終了コード (0 のほか、再起動が要る 3010 も成功とみなす)
const MSI_OK: [i32; 2] = [0, 3010];

/// 適用役への指示。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub version: String,
    pub target: Target,
    pub file: PathBuf,
    pub sha256: String,
}

/// 適用の結果。次に起動した本体が読む。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub version: String,
    pub ok: bool,
    pub message: String,
}

/// 適用役を起動する。成功したら、呼び出し側はすぐアプリを終了する。
pub fn start(staged: &Staged) -> anyhow::Result<()> {
    let job = Job {
        version: staged.plan.version.to_string(),
        target: staged.plan.target.clone(),
        file: staged.file.clone(),
        sha256: staged.sha256.clone(),
    };
    // 古い結果が残っていると、今回の結果と取り違える
    let _ = std::fs::remove_file(staged.work_dir.join(RESULT_FILE));
    std::fs::write(
        staged.work_dir.join(JOB_FILE),
        serde_json::to_vec_pretty(&job)?,
    )?;
    // 入れ替えで本体の実行ファイルが置き換わっても動き続けられるよう、コピーから起動する
    let runner = staged.work_dir.join(if cfg!(windows) {
        "apply-runner.exe"
    } else {
        "apply-runner"
    });
    std::fs::copy(std::env::current_exe()?, &runner).context("適用役を用意できません")?;
    let mut cmd = Command::new(&runner);
    cmd.arg(ARG_APPLY)
        .arg(&staged.work_dir)
        .arg(ARG_WAIT_PID)
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut cmd);
    cmd.spawn().context("適用役を起動できません")?;
    Ok(())
}

/// 親が終了しても続くようにする (Windows のみ。unix の子は親が終わっても残る)。
fn detach(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// `--apply-update` の引数を読んで適用役を動かす。プロセスの終了コードを返す。
/// 先頭の引数が `--apply-update` でなければ `None`。
pub fn run_from_args(args: &[String]) -> Option<i32> {
    if args.first().map(String::as_str) != Some(ARG_APPLY) {
        return None;
    }
    let dir = args.get(1).map(PathBuf::from);
    let pid = (args.get(2).map(String::as_str) == Some(ARG_WAIT_PID))
        .then(|| args.get(3).and_then(|p| p.parse::<u32>().ok()))
        .flatten();
    let (Some(dir), Some(pid)) = (dir, pid) else {
        return Some(2);
    };
    Some(run(&dir, pid))
}

/// 適用役の本体。
pub fn run(dir: &Path, pid: u32) -> i32 {
    let log = |msg: &str| {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(LOG_FILE))
        {
            let _ = writeln!(f, "{msg}");
        }
    };
    let job: Job = match std::fs::read(dir.join(JOB_FILE))
        .map_err(anyhow::Error::from)
        .and_then(|b| serde_json::from_slice(&b).map_err(Into::into))
    {
        Ok(j) => j,
        Err(e) => {
            log(&format!("job.json を読めません: {e:#}"));
            return 2;
        }
    };
    log(&format!(
        "v{} への更新を始めます (本体 PID {pid})",
        job.version
    ));

    if !wait_exit(pid, WAIT_LIMIT) {
        // 本体がまだ動いているので、起動し直さない (二重に起動してしまう)
        let msg = "アプリが終了しなかったため、更新を中止しました。";
        log(msg);
        write_outcome(dir, &job, Err(msg.to_string()));
        return 1;
    }

    let result = apply(&job, &log);
    match &result {
        Ok(()) => log("入れ替えました"),
        Err(e) => log(&format!("失敗: {e}")),
    }
    let ok = result.is_ok();
    write_outcome(dir, &job, result);
    // 成功しても失敗しても、アプリを起動する (失敗のときは元の版)
    if let Err(e) = relaunch(&job.target) {
        log(&format!("起動し直せません: {e:#}"));
    }
    i32::from(!ok)
}

fn write_outcome(dir: &Path, job: &Job, result: Result<(), String>) {
    let out = Outcome {
        version: job.version.clone(),
        ok: result.is_ok(),
        message: result.err().unwrap_or_default(),
    };
    if let Ok(b) = serde_json::to_vec_pretty(&out) {
        let _ = std::fs::write(dir.join(RESULT_FILE), b);
    }
}

/// OS ごとの入れ替え。
fn apply(job: &Job, log: &dyn Fn(&str)) -> Result<(), String> {
    // 確認してから適用までの間に、ファイルが差し替えられていないか
    let actual = verify::sha256_file(&job.file).map_err(|e| format!("配布物を読めません: {e}"))?;
    if actual != job.sha256 {
        return Err("配布物が確認のときと変わっています。".into());
    }
    let r = match &job.target {
        Target::WindowsInstaller { .. } => apply_msi(&job.file, log),
        Target::WindowsPortable { exe } => apply_portable(&job.file, exe, log),
        Target::MacApp { app } => apply_mac(&job.file, app, log),
        Target::LinuxBinary { exe } => apply_linux(&job.file, exe, log),
    };
    r.map_err(|e| format!("{e:#}"))
}

// ---- Windows (インストーラー版) ----

fn apply_msi(msi: &Path, log: &dyn Fn(&str)) -> anyhow::Result<()> {
    let msi_log = msi.with_file_name("msi.log");
    let status = Command::new("msiexec")
        .arg("/i")
        .arg(msi)
        .args(["/passive", "/norestart", "/l*v"])
        .arg(&msi_log)
        .status()
        .context("msiexec を起動できません")?;
    let code = status.code().unwrap_or(-1);
    log(&format!("msiexec の終了コード: {code}"));
    if !MSI_OK.contains(&code) {
        bail!("インストーラーが失敗しました (終了コード {code})");
    }
    Ok(())
}

// ---- Windows (ポータブル版) ----

/// zip の中から `RSQuake.exe` を取り出して `dest` に書く。
pub fn extract_exe(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    const LIMIT: u64 = 400 * 1024 * 1024;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?)?;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let is_exe = entry
            .enclosed_name()
            .and_then(|n| n.file_name().map(|f| f == "RSQuake.exe"))
            .unwrap_or(false);
        if !is_exe || entry.is_dir() {
            continue;
        }
        let mut out = std::fs::File::create(dest)
            .with_context(|| format!("{} を作れません", dest.display()))?;
        let n = std::io::copy(&mut std::io::Read::take(entry, LIMIT + 1), &mut out)?;
        if n == 0 || n > LIMIT {
            bail!("zip の中の RSQuake.exe の大きさがおかしいです");
        }
        out.sync_all()?;
        return Ok(());
    }
    bail!("zip に RSQuake.exe がありません")
}

fn apply_portable(zip: &Path, exe: &Path, log: &dyn Fn(&str)) -> anyhow::Result<()> {
    let new = sibling(exe, "", ".new");
    let old = sibling(exe, "", ".old");
    extract_exe(zip, &new)?;
    log(&format!("{} を取り出しました", new.display()));
    // 実行中のファイルは消せないが、名前は変えられる
    if let Err(e) = retry(10, Duration::from_millis(300), || swap(exe, &new, &old)) {
        let _ = std::fs::remove_file(&new);
        return Err(anyhow!("入れ替えられません: {e}"));
    }
    Ok(())
}

// ---- Linux ----

/// tar.gz の中から実行ファイル `rsquake` を取り出して `dest` に書き、実行権限を付ける。
pub fn extract_linux_binary(tarball: &Path, dest: &Path) -> anyhow::Result<()> {
    const LIMIT: u64 = 400 * 1024 * 1024;
    let gz = flate2::read::GzDecoder::new(std::fs::File::open(tarball)?);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries()? {
        let entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let is_bin = entry
            .path()
            .ok()
            .and_then(|p| p.file_name().map(|f| f == "rsquake"))
            .unwrap_or(false);
        if !is_bin {
            continue;
        }
        let mut out = std::fs::File::create(dest)
            .with_context(|| format!("{} を作れません", dest.display()))?;
        let n = std::io::copy(&mut std::io::Read::take(entry, LIMIT + 1), &mut out)?;
        if n == 0 || n > LIMIT {
            bail!("tar.gz の中の rsquake の大きさがおかしいです");
        }
        out.sync_all()?;
        drop(out);
        // 実行ファイル (ELF) でないものは置かない
        let mut magic = [0u8; 4];
        std::io::Read::read_exact(&mut std::fs::File::open(dest)?, &mut magic)?;
        if &magic != b"\x7fELF" {
            let _ = std::fs::remove_file(dest);
            bail!("tar.gz の中の rsquake が実行ファイルではありません");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))?;
        }
        return Ok(());
    }
    bail!("tar.gz に rsquake がありません")
}

fn apply_linux(tarball: &Path, exe: &Path, log: &dyn Fn(&str)) -> anyhow::Result<()> {
    let new = sibling(exe, "", ".new");
    let old = sibling(exe, "", ".old");
    if let Err(e) = extract_linux_binary(tarball, &new) {
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }
    log(&format!("{} を取り出しました", new.display()));
    // 実行中のファイルでも、名前の変更なら置き換えられる
    if let Err(e) = swap(exe, &new, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(anyhow!("入れ替えられません: {e}"));
    }
    Ok(())
}

// ---- macOS ----

fn apply_mac(dmg: &Path, app: &Path, log: &dyn Fn(&str)) -> anyhow::Result<()> {
    let work = dmg
        .parent()
        .ok_or_else(|| anyhow!("作業フォルダが分かりません"))?;
    let mount = work.join("mnt");
    let new = sibling(app, ".", ".new");
    let old = sibling(app, ".", ".old");
    let _ = std::fs::remove_dir_all(&new);
    std::fs::create_dir_all(&mount)?;

    let attach = run_cmd(
        Command::new("hdiutil")
            .args([
                "attach",
                "-nobrowse",
                "-readonly",
                "-noverify",
                "-mountpoint",
            ])
            .arg(&mount)
            .arg(dmg),
    );
    if let Err(e) = attach {
        // 途中までマウントされたかもしれない
        detach_dmg(&mount);
        return Err(e.context("ディスクイメージを開けません"));
    }
    let copied = (|| -> anyhow::Result<()> {
        let src = find_app(&mount)?;
        // 同じボリュームに置いて、名前の変更だけで入れ替えられるようにする
        run_cmd(Command::new("ditto").arg(&src).arg(&new)).context("アプリを複製できません")
    })();
    detach_dmg(&mount);
    copied?;
    log("複製しました");

    let prepared = (|| -> anyhow::Result<()> {
        run_cmd(
            Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&new),
        )
        .context("複製したアプリの署名が壊れています")?;
        // 外れていれば何も起きない。付いていれば、初回起動の警告を出さないよう外す
        let _ = Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&new)
            .status();
        Ok(())
    })();
    if let Err(e) = prepared {
        let _ = std::fs::remove_dir_all(&new);
        return Err(e);
    }

    if let Err(e) = swap(app, &new, &old) {
        let _ = std::fs::remove_dir_all(&new);
        return Err(anyhow!("入れ替えられません: {e}"));
    }
    Ok(())
}

fn find_app(mount: &Path) -> anyhow::Result<PathBuf> {
    let direct = mount.join("RSQuake.app");
    if direct.is_dir() {
        return Ok(direct);
    }
    bail!("ディスクイメージに RSQuake.app がありません")
}

fn detach_dmg(mount: &Path) {
    let ok = Command::new("hdiutil")
        .arg("detach")
        .arg(mount)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        let _ = Command::new("hdiutil")
            .args(["detach", "-force"])
            .arg(mount)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn run_cmd(cmd: &mut Command) -> anyhow::Result<()> {
    let out = cmd
        .output()
        .with_context(|| format!("{:?} を起動できません", cmd.get_program()))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    bail!(
        "{:?} が失敗しました ({}): {}",
        cmd.get_program(),
        out.status,
        stderr.trim()
    )
}

// ---- 共通 ----

/// `path` と同じフォルダの、`<prefix><名前><suffix>`。入れ替えの一時的な名前に使う。
fn sibling(path: &Path, prefix: &str, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!("{prefix}{name}{suffix}"))
}

fn remove_any(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// `current` を `old` に、`new` を `current` に名前を変えて入れ替える。
/// 2 つ目が失敗したら、`current` を元に戻す。
pub fn swap(current: &Path, new: &Path, old: &Path) -> std::io::Result<()> {
    // どの操作で、どのパスが失敗したか分かるようにする
    let ctx = |op: &str, from: &Path, to: &Path, e: std::io::Error| {
        std::io::Error::new(
            e.kind(),
            format!("{op} ({} -> {}): {e}", from.display(), to.display()),
        )
    };
    remove_any(old);
    std::fs::rename(current, old).map_err(|e| ctx("現在の版を .old へ", current, old, e))?;
    if let Err(e) = std::fs::rename(new, current) {
        let _ = std::fs::rename(old, current);
        return Err(ctx("新しい版を置く", new, current, e));
    }
    Ok(())
}

fn retry<T, E>(times: u32, delay: Duration, mut f: impl FnMut() -> Result<T, E>) -> Result<T, E> {
    let mut n = 1;
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if n >= times => return Err(e),
            Err(_) => {
                n += 1;
                std::thread::sleep(delay);
            }
        }
    }
}

/// 更新した (または更新に失敗した) アプリを起動し直す。
fn relaunch(target: &Target) -> anyhow::Result<()> {
    let path = target.relaunch_path();
    let mut cmd = match target {
        Target::MacApp { .. } => {
            let mut c = Command::new("open");
            c.arg("-n").arg(path);
            c
        }
        _ => Command::new(path),
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut cmd);
    cmd.spawn()
        .with_context(|| format!("{} を起動できません", path.display()))?;
    Ok(())
}

/// プロセスが終わるまで待つ。上限までに終われば真。
pub fn wait_exit(pid: u32, limit: Duration) -> bool {
    #[cfg(unix)]
    {
        let start = Instant::now();
        loop {
            // 信号は送らず、存在だけ調べる。権限が無いときも、存在はしている
            let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
            if !alive {
                return true;
            }
            if start.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
        const SYNCHRONIZE: u32 = 0x0010_0000;
        let _ = Instant::now();
        // SAFETY: ハンドルは自分で開いて、このブロックの中で閉じる。
        unsafe {
            let h = OpenProcess(SYNCHRONIZE, 0, pid);
            if h.is_null() {
                // 開けない = もう無い (権限が無い場合も、同じユーザーの本体なので起きない)
                return true;
            }
            let r = WaitForSingleObject(h, limit.as_millis().min(u128::from(u32::MAX - 1)) as u32);
            CloseHandle(h);
            r == WAIT_OBJECT_0
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, limit, Instant::now());
        true
    }
}

// ---- 起動時 ----

/// 起動時の後始末と、前回の更新の結果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Startup {
    /// 前回のアプリ内の更新が失敗していたときの理由
    pub failure: Option<String>,
}

/// 前回の更新の結果を読み、入れ替えで残った古いファイルと作業フォルダを片付ける。
pub fn startup() -> Startup {
    let mut s = Startup::default();
    if let Some(dir) = work_dir() {
        if let Some(out) = read_outcome(&dir)
            && !out.ok
        {
            s.failure = Some(out.message);
            // 調査用に、作業フォルダを消す前に適用役のログを残す
            if let Some(parent) = dir.parent() {
                let _ = std::fs::copy(dir.join(LOG_FILE), parent.join("apply-failed.log"));
            }
        }
        // 成功でも失敗でも、結果を読んだら作業フォルダは要らない
        let _ = std::fs::remove_dir_all(&dir);
    }
    if let Some(env) = Env::current()
        && let Ok(target) = detect(&env)
    {
        cleanup_old(&target);
    }
    s
}

fn read_outcome(dir: &Path) -> Option<Outcome> {
    let b = std::fs::read(dir.join(RESULT_FILE)).ok()?;
    serde_json::from_slice(&b).ok()
}

/// 入れ替えで名前を変えた古い版を消す。
pub fn cleanup_old(target: &Target) {
    match target {
        Target::WindowsInstaller { .. } => {}
        Target::WindowsPortable { exe } | Target::LinuxBinary { exe } => {
            // 起動した直後は、適用役がまだ終わっていないことがあるので、少し待ちながら試す
            let old = sibling(exe, "", ".old");
            let new = sibling(exe, "", ".new");
            std::thread::spawn(move || {
                for p in [old, new] {
                    let _ = retry(5, Duration::from_secs(1), || {
                        if p.exists() {
                            std::fs::remove_file(&p)
                        } else {
                            Ok(())
                        }
                    });
                }
            });
        }
        Target::MacApp { app } => {
            remove_any(&sibling(app, ".", ".old"));
            remove_any(&sibling(app, ".", ".new"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_replaces_and_keeps_the_old_one() {
        let d = tempfile::tempdir().unwrap();
        let (cur, new, old) = (
            d.path().join("a"),
            d.path().join("a.new"),
            d.path().join("a.old"),
        );
        std::fs::write(&cur, "v1").unwrap();
        std::fs::write(&new, "v2").unwrap();
        std::fs::write(&old, "stale").unwrap();
        swap(&cur, &new, &old).unwrap();
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "v2");
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "v1");
        assert!(!new.exists());
    }

    #[test]
    fn swap_works_on_directories() {
        let d = tempfile::tempdir().unwrap();
        let (cur, new, old) = (
            d.path().join("R.app"),
            d.path().join(".R.app.new"),
            d.path().join(".R.app.old"),
        );
        for (p, v) in [(&cur, "v1"), (&new, "v2")] {
            std::fs::create_dir_all(p.join("Contents")).unwrap();
            std::fs::write(p.join("Contents/v"), v).unwrap();
        }
        swap(&cur, &new, &old).unwrap();
        assert_eq!(
            std::fs::read_to_string(cur.join("Contents/v")).unwrap(),
            "v2"
        );
        assert_eq!(
            std::fs::read_to_string(old.join("Contents/v")).unwrap(),
            "v1"
        );
    }

    #[test]
    fn failed_swap_restores_the_current_one() {
        let d = tempfile::tempdir().unwrap();
        let (cur, new, old) = (
            d.path().join("a"),
            d.path().join("missing"),
            d.path().join("a.old"),
        );
        std::fs::write(&cur, "v1").unwrap();
        assert!(swap(&cur, &new, &old).is_err());
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "v1");
        assert!(!old.exists());
    }

    #[test]
    fn sibling_names() {
        let p = Path::new("/A/RSQuake.app");
        assert_eq!(sibling(p, ".", ".new"), Path::new("/A/.RSQuake.app.new"));
        assert_eq!(
            sibling(Path::new("/A/RSQuake.exe"), "", ".old"),
            Path::new("/A/RSQuake.exe.old")
        );
    }

    fn make_zip(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        use zip::write::SimpleFileOptions;
        let path = dir.join("t.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, body) in entries {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap();
        path
    }

    #[test]
    fn exe_is_extracted_from_the_nested_folder() {
        let d = tempfile::tempdir().unwrap();
        let zip = make_zip(
            d.path(),
            &[
                ("RSQuake-0.2.0-windows-x64/README.txt", b"x"),
                ("RSQuake-0.2.0-windows-x64/RSQuake.exe", b"MZ-new"),
            ],
        );
        let out = d.path().join("out.exe");
        extract_exe(&zip, &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"MZ-new");
    }

    #[test]
    fn zip_without_the_exe_or_with_path_escape_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("out.exe");
        let zip = make_zip(d.path(), &[("other.txt", b"x")]);
        assert!(extract_exe(&zip, &out).is_err());
        // 上位フォルダへ出る名前は無視される
        let zip = make_zip(d.path(), &[("../RSQuake.exe", b"evil")]);
        assert!(extract_exe(&zip, &out).is_err());
    }

    #[test]
    fn portable_update_swaps_the_exe_and_rolls_back_on_a_bad_zip() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("RSQuake.exe");
        std::fs::write(&exe, "old-exe").unwrap();
        let zip = make_zip(
            d.path(),
            &[("RSQuake-0.2.0-windows-x64/RSQuake.exe", b"new-exe")],
        );
        apply_portable(&zip, &exe, &|_| {}).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new-exe");
        assert_eq!(
            std::fs::read(d.path().join("RSQuake.exe.old")).unwrap(),
            b"old-exe"
        );

        let bad = make_zip(d.path(), &[("nothing.txt", b"x")]);
        assert!(apply_portable(&bad, &exe, &|_| {}).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"new-exe");
    }

    fn job(dir: &Path, sha: &str) -> Job {
        let file = dir.join("f.zip");
        std::fs::write(&file, "payload").unwrap();
        Job {
            version: "0.2.0".into(),
            target: Target::WindowsPortable {
                exe: dir.join("RSQuake.exe"),
            },
            file,
            sha256: sha.into(),
        }
    }

    #[test]
    fn apply_refuses_a_file_that_changed_after_verification() {
        let d = tempfile::tempdir().unwrap();
        let j = job(d.path(), "0000");
        assert!(apply(&j, &|_| {}).unwrap_err().contains("変わっています"));
    }

    #[test]
    fn job_and_outcome_roundtrip_as_json() {
        let d = tempfile::tempdir().unwrap();
        let j = job(d.path(), "abcd");
        let s = serde_json::to_string(&j).unwrap();
        assert_eq!(serde_json::from_str::<Job>(&s).unwrap(), j);
        write_outcome(d.path(), &j, Err("だめ".into()));
        let o = read_outcome(d.path()).unwrap();
        assert!(!o.ok);
        assert_eq!(o.message, "だめ");
        write_outcome(d.path(), &j, Ok(()));
        assert!(read_outcome(d.path()).unwrap().ok);
    }

    #[test]
    fn args_are_parsed_only_when_they_start_with_apply_update() {
        assert_eq!(run_from_args(&["--test".into()]), None);
        assert_eq!(run_from_args(&[]), None);
        // 引数が足りなければ、何もせず失敗で終わる
        assert_eq!(run_from_args(&[ARG_APPLY.into()]), Some(2));
        assert_eq!(
            run_from_args(&[
                ARG_APPLY.into(),
                "/x".into(),
                ARG_WAIT_PID.into(),
                "abc".into()
            ]),
            Some(2)
        );
    }

    #[cfg(unix)]
    #[test]
    fn wait_exit_sees_a_finished_process() {
        let mut child = Command::new("sleep").arg("0.3").spawn().unwrap();
        let pid = child.id();
        assert!(!wait_exit(pid, Duration::from_millis(50)));
        child.wait().unwrap(); // 子は終了を回収しないと残るので、回収してから見る
        assert!(wait_exit(pid, Duration::from_secs(5)));
    }

    #[test]
    fn retry_gives_up_after_the_limit() {
        let mut n = 0;
        let r: Result<(), &str> = retry(3, Duration::from_millis(1), || {
            n += 1;
            Err("no")
        });
        assert!(r.is_err());
        assert_eq!(n, 3);
        let mut n = 0;
        let r: Result<i32, &str> = retry(5, Duration::from_millis(1), || {
            n += 1;
            if n < 3 { Err("no") } else { Ok(7) }
        });
        assert_eq!(r, Ok(7));
    }

    fn make_tar_gz(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join("t.tar.gz");
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        for (name, body) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, name, *body).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
        path
    }

    #[test]
    fn linux_binary_is_extracted_and_made_executable() {
        let d = tempfile::tempdir().unwrap();
        let t = make_tar_gz(
            d.path(),
            &[
                ("RSQuake-0.2.0-linux-x86_64/LICENSE", b"x"),
                ("RSQuake-0.2.0-linux-x86_64/rsquake", b"\x7fELF-new"),
            ],
        );
        let out = d.path().join("rsquake.new");
        extract_linux_binary(&t, &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"\x7fELF-new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&out).unwrap().permissions().mode() & 0o111, 0o111);
        }
    }

    #[test]
    fn linux_tarball_without_a_binary_or_with_a_non_elf_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("rsquake.new");
        let t = make_tar_gz(d.path(), &[("a/other", b"x")]);
        assert!(extract_linux_binary(&t, &out).is_err());
        let t = make_tar_gz(d.path(), &[("a/rsquake", b"#!/bin/sh\n")]);
        assert!(extract_linux_binary(&t, &out).is_err());
        assert!(!out.exists());
    }

    #[test]
    fn linux_update_swaps_the_binary_and_keeps_the_old_one() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("rsquake");
        std::fs::write(&exe, "old").unwrap();
        let t = make_tar_gz(d.path(), &[("a/rsquake", b"\x7fELF-new")]);
        apply_linux(&t, &exe, &|_| {}).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"\x7fELF-new");
        assert_eq!(std::fs::read(d.path().join("rsquake.old")).unwrap(), b"old");

        let bad = make_tar_gz(d.path(), &[("a/nothing", b"x")]);
        assert!(apply_linux(&bad, &exe, &|_| {}).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"\x7fELF-new");
    }
}
