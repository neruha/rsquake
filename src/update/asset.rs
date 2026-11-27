//! リリースの配布物 (asset) の選択。動いている環境から、アプリ内で更新できるかと、使うファイルを決める。
//!
//! 環境の判定 (`detect`) と名前の組み立て (`asset_name`) は、OS やパスを引数で受け取る純粋な関数にしてある。

use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use super::Release;

/// 配布物のダウンロード URL の先頭。アプリ内でダウンロードするのは、これで始まる URL だけ。
pub const DOWNLOAD_PREFIX: &str = "https://github.com/neruha/rsquake/releases/download/";

/// ダウンロードしてよい URL か。
/// デバッグビルドだけ、試験用に手元のモックサーバー (`http://127.0.0.1` / `http://localhost`) も許す。
pub fn is_download_url(url: &str) -> bool {
    if url.starts_with(DOWNLOAD_PREFIX) {
        return true;
    }
    cfg!(debug_assertions) && is_local_url(url)
}

/// リダイレクト先として許す URL か。GitHub の配布物は `*.githubusercontent.com` の保管先へ転送される。
pub fn is_redirect_url(url: &str) -> bool {
    const HOSTS: [&str; 4] = [
        "github.com",
        "objects.githubusercontent.com",
        "release-assets.githubusercontent.com",
        "github-releases.githubusercontent.com",
    ];
    let Some(rest) = url.strip_prefix("https://") else {
        return cfg!(debug_assertions) && is_local_url(url);
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // ユーザー情報やポートが付いた形は受け付けない
    HOSTS.contains(&host)
}

fn is_local_url(url: &str) -> bool {
    ["http://127.0.0.1:", "http://localhost:"]
        .iter()
        .any(|p| url.starts_with(p))
}

/// Release に付いているファイル 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// バイト数 (GitHub の申告値。確認には使わず、進行状況の表示だけに使う)
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
    Other,
}

/// 動いている環境。
#[derive(Clone, Debug)]
pub struct Env {
    pub os: Os,
    pub arch: Arch,
    /// 動いている実行ファイル
    pub exe: PathBuf,
    /// Windows の `%LOCALAPPDATA%`
    pub local_app_data: Option<PathBuf>,
    /// Windows の `Program Files` (64 ビット・32 ビット)
    pub program_files: Vec<PathBuf>,
}

impl Env {
    pub fn current() -> Option<Self> {
        let os = if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other
        };
        let arch = if cfg!(target_arch = "x86_64") {
            Arch::X64
        } else if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::Other
        };
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Some(Self {
            os,
            arch,
            exe: std::env::current_exe().ok()?,
            local_app_data: var("LOCALAPPDATA"),
            program_files: ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
                .iter()
                .filter_map(|k| var(k))
                .collect(),
        })
    }
}

/// どの形で入っているか。入れ替え方がこれで決まる。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    /// Windows の MSI で入れた版 (ユーザー単位のインストール先)
    WindowsInstaller { exe: PathBuf },
    /// Windows の zip を展開しただけの版
    WindowsPortable { exe: PathBuf },
    /// macOS の `.app`
    MacApp { app: PathBuf },
    /// Linux の実行ファイル (`install.sh` で入れた `~/.local/bin/rsquake` など)
    LinuxBinary { exe: PathBuf },
}

impl Target {
    /// 更新のあとに起動し直すもの (Windows と Linux は実行ファイル、macOS は `.app`)。
    pub fn relaunch_path(&self) -> &Path {
        match self {
            Self::WindowsInstaller { exe }
            | Self::WindowsPortable { exe }
            | Self::LinuxBinary { exe } => exe,
            Self::MacApp { app } => app,
        }
    }

    /// 入れ替えで書き込むフォルダ (実行ファイル / `.app` を置いているところ)。
    pub fn parent_dir(&self) -> Option<&Path> {
        self.relaunch_path().parent()
    }
}

/// 環境から、入れ替え方を決める。アプリ内で更新できない環境は、理由を `Err` で返す。
pub fn detect(env: &Env) -> Result<Target, String> {
    match env.os {
        Os::Windows => detect_windows(env),
        Os::MacOs => detect_mac(env),
        Os::Linux => detect_linux(env),
        Os::Other => Err("この OS では、アプリ内の更新に対応していません。".into()),
    }
}

fn detect_linux(env: &Env) -> Result<Target, String> {
    let exe = env.exe.clone();
    // パッケージ管理 (apt / rpm など) が入れた場所は、管理者権限なしでは書けず、
    // 書けても管理側と食い違うので、アプリ内では更新しない。
    const MANAGED: [&str; 4] = ["/usr/bin", "/usr/sbin", "/usr/lib", "/usr/libexec"];
    if MANAGED.iter().any(|d| exe.starts_with(d)) {
        return Err("パッケージ管理が入れた場所から起動しています。\
            パッケージ管理で更新するか、install.sh で入れ直してください。"
            .into());
    }
    // cargo でビルドした開発用の実行ファイル (<リポジトリ>/target/<profile>/rsquake) は上書きしない
    let in_cargo_target = exe
        .ancestors()
        .skip(1)
        .filter(|p| p.file_name().is_some_and(|n| n == "target"))
        .any(|p| p.parent().is_some_and(|r| r.join("Cargo.toml").is_file()));
    if in_cargo_target {
        return Err("開発用のビルドから起動しているため、入れ替えません。".into());
    }
    Ok(Target::LinuxBinary { exe })
}

fn detect_windows(env: &Env) -> Result<Target, String> {
    let exe = env.exe.clone();
    if is_zip_preview_path(&exe) {
        return Err("zip の中から起動しています。\
            zip を右クリックして「すべて展開」したあと、展開したフォルダの RSQuake.exe を起動してください。"
            .into());
    }
    if env
        .program_files
        .iter()
        .any(|pf| exe.starts_with(pf.join("RSQuake")))
    {
        return Err("古いインストーラーで Program Files に入った版です。\
            いちどアンインストールして、新しいインストーラーで入れ直してください。"
            .into());
    }
    let installed = env
        .local_app_data
        .as_ref()
        .is_some_and(|d| exe.starts_with(d.join("Programs").join("RSQuake")));
    Ok(if installed {
        Target::WindowsInstaller { exe }
    } else {
        Target::WindowsPortable { exe }
    })
}

/// エクスプローラーで zip を開いたまま起動したときの一時展開先か。
/// 場所は `%TEMP%\Temp<N>_<zip の名前>.zip\...` で、アプリが終わると消えることがあるため、
/// 入れ替えも再起動もできない。
fn is_zip_preview_path(exe: &Path) -> bool {
    exe.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        let Some(rest) = name.strip_prefix("temp") else {
            return false;
        };
        name.ends_with(".zip")
            && rest
                .split('_')
                .next()
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn detect_mac(env: &Env) -> Result<Target, String> {
    let exe = &env.exe;
    // .../RSQuake.app/Contents/MacOS/rsquake の、`.app` を探す
    let app = exe
        .ancestors()
        .skip(1)
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("アプリ (.app) として起動していないため、入れ替えられません。")?;
    if app.starts_with("/Volumes") {
        return Err("ディスクイメージの中から起動しています。\
            アプリを「アプリケーション」フォルダなどへ移してから起動してください。"
            .into());
    }
    if exe
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Err("macOS が一時的な場所から起動しています。\
            アプリを「アプリケーション」フォルダなどへ移してから起動してください。"
            .into());
    }
    Ok(Target::MacApp {
        app: app.to_path_buf(),
    })
}

/// その環境と版で使う配布物の名前 (`scripts/package-*` の命名)。
pub fn asset_name(target: &Target, arch: Arch, version: &Version) -> Result<String, String> {
    match (target, arch) {
        (Target::WindowsInstaller { .. }, Arch::X64) => {
            Ok(format!("RSQuake-{version}-windows-x64-installer.msi"))
        }
        (Target::WindowsPortable { .. }, Arch::X64) => {
            Ok(format!("RSQuake-{version}-windows-x64.zip"))
        }
        (Target::MacApp { .. }, Arch::Arm64) => Ok(format!("RSQuake-{version}-macos-arm64.dmg")),
        (Target::MacApp { .. }, Arch::X64) => Ok(format!("RSQuake-{version}-macos-x86_64.dmg")),
        (Target::LinuxBinary { .. }, Arch::X64) => {
            Ok(format!("RSQuake-{version}-linux-x86_64.tar.gz"))
        }
        (Target::LinuxBinary { .. }, Arch::Arm64) => {
            Ok(format!("RSQuake-{version}-linux-aarch64.tar.gz"))
        }
        _ => Err("この CPU 向けの配布物がありません。".into()),
    }
}

pub const SUMS_NAME: &str = "SHA256SUMS";
pub const SIG_NAME: &str = "SHA256SUMS.minisig";

/// アプリ内で更新するときに使う、3 つのファイルと入れ替え方。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub version: Version,
    pub target: Target,
    pub asset: Asset,
    pub sums: Asset,
    pub sig: Asset,
}

/// Release の中から、3 つのファイルを探して `Plan` にする。足りなければ、理由を `Err` で返す。
pub fn plan(release: &Release, target: Target, arch: Arch) -> Result<Plan, String> {
    let find = |name: &str| -> Result<Asset, String> {
        let a = release
            .assets
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| format!("リリースに {name} がありません。"))?;
        if !is_download_url(&a.url) {
            return Err(format!("{name} の URL が配布元のものではありません。"));
        }
        Ok(a.clone())
    };
    let name = asset_name(&target, arch, &release.version)?;
    Ok(Plan {
        version: release.version.clone(),
        asset: find(&name)?,
        sums: find(SUMS_NAME)?,
        sig: find(SIG_NAME)?,
        target,
    })
}

/// フォルダに、ファイルを作って消せるか。
pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".rsquake-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 動いている環境で、その Release をアプリ内で更新するための `Plan` を作る。
pub fn plan_current(release: &Release) -> Result<Plan, String> {
    let env = Env::current().ok_or("実行ファイルの場所が分かりません。")?;
    let target = detect(&env)?;
    // macOS は `.app` の名前を変えるので、`.app` を置いているフォルダに書けることが要る
    // (Windows は実行ファイルを置いているフォルダ。インストーラー版は msiexec が書くので見ない)
    if !matches!(target, Target::WindowsInstaller { .. })
        && !target.parent_dir().is_some_and(dir_writable)
    {
        return Err("アプリのあるフォルダに書き込めません。".into());
    }
    plan(release, target, env.arch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn env(os: Os, exe: &str) -> Env {
        Env {
            os,
            arch: Arch::X64,
            exe: PathBuf::from(exe),
            local_app_data: Some(PathBuf::from("/Users/u/AppData/Local")),
            program_files: vec![PathBuf::from("/Program Files")],
        }
    }

    #[test]
    fn download_urls_are_limited_to_this_repository() {
        assert!(is_download_url(
            "https://github.com/neruha/rsquake/releases/download/v0.2.0/SHA256SUMS"
        ));
        assert!(!is_download_url(
            "https://github.com/neruha/rsquake-evil/releases/download/v0.2.0/x"
        ));
        assert!(!is_download_url("https://evil.example/x"));
        assert!(!is_download_url(
            "https://github.com.evil.example/neruha/rsquake/releases/download/x"
        ));
    }

    #[test]
    fn redirects_only_go_to_github_hosts() {
        assert!(is_redirect_url(
            "https://objects.githubusercontent.com/github-production-release-asset/1?x=y"
        ));
        assert!(is_redirect_url("https://github.com/neruha/rsquake/x"));
        assert!(!is_redirect_url("https://evil.example/x"));
        assert!(!is_redirect_url("https://github.com@evil.example/x"));
        assert!(!is_redirect_url(
            "https://objects.githubusercontent.com.evil.example/x"
        ));
        assert!(!is_redirect_url("http://github.com/x"));
    }

    #[test]
    fn windows_install_kinds_are_told_apart() {
        let e = env(
            Os::Windows,
            "/Users/u/AppData/Local/Programs/RSQuake/RSQuake.exe",
        );
        assert!(matches!(detect(&e), Ok(Target::WindowsInstaller { .. })));
        let e = env(Os::Windows, "/Users/u/Downloads/RSQuake/RSQuake.exe");
        assert!(matches!(detect(&e), Ok(Target::WindowsPortable { .. })));
        // Program Files の古いインストーラー版は、アプリ内では更新しない
        let e = env(Os::Windows, "/Program Files/RSQuake/RSQuake.exe");
        assert!(detect(&e).unwrap_err().contains("アンインストール"));
        // Program Files の下でも、RSQuake 以外のフォルダは古いインストーラー版ではない
        let e = env(Os::Windows, "/Program Files/Other/RSQuake.exe");
        assert!(matches!(detect(&e), Ok(Target::WindowsPortable { .. })));
    }

    #[test]
    fn running_from_inside_a_zip_is_refused() {
        let e = env(
            Os::Windows,
            "/Users/u/AppData/Local/Temp/Temp2_RSQuake-0.2.0-windows-x64.zip/RSQuake-0.2.0-windows-x64/RSQuake.exe",
        );
        assert!(detect(&e).unwrap_err().contains("zip"));
        // 名前が似ているだけの通常のフォルダは対象外
        let e = env(Os::Windows, "/Users/u/Downloads/Temp_files/RSQuake.exe");
        assert!(matches!(detect(&e), Ok(Target::WindowsPortable { .. })));
        let e = env(Os::Windows, "/Users/u/Downloads/Temp2_x/RSQuake.exe");
        assert!(matches!(detect(&e), Ok(Target::WindowsPortable { .. })));
    }

    #[test]
    fn mac_app_is_found_from_the_executable() {
        let e = env(
            Os::MacOs,
            "/Applications/RSQuake.app/Contents/MacOS/rsquake",
        );
        assert_eq!(
            detect(&e),
            Ok(Target::MacApp {
                app: PathBuf::from("/Applications/RSQuake.app")
            })
        );
        // 名前を変えていても .app なら見つかる
        let e = env(
            Os::MacOs,
            "/Users/u/Apps/RSQuake 2.app/Contents/MacOS/rsquake",
        );
        assert!(detect(&e).is_ok());
    }

    #[test]
    fn mac_unsupported_launch_places_are_refused() {
        assert!(
            detect(&env(
                Os::MacOs,
                "/Users/u/dev/rsquake/target/release/rsquake"
            ))
            .is_err()
        );
        let dmg = env(
            Os::MacOs,
            "/Volumes/RSQuake/RSQuake.app/Contents/MacOS/rsquake",
        );
        assert!(detect(&dmg).unwrap_err().contains("ディスクイメージ"));
        let t = env(
            Os::MacOs,
            "/private/var/folders/x/AppTranslocation/ABC/d/RSQuake.app/Contents/MacOS/rsquake",
        );
        assert!(detect(&t).unwrap_err().contains("一時的な場所"));
        assert!(detect(&env(Os::Other, "/usr/bin/rsquake")).is_err());
    }

    #[test]
    fn asset_names_follow_the_packaging_scripts() {
        let ver = v("0.2.0-beta.1");
        let inst = Target::WindowsInstaller { exe: "x".into() };
        let port = Target::WindowsPortable { exe: "x".into() };
        let mac = Target::MacApp { app: "x".into() };
        assert_eq!(
            asset_name(&inst, Arch::X64, &ver).unwrap(),
            "RSQuake-0.2.0-beta.1-windows-x64-installer.msi"
        );
        assert_eq!(
            asset_name(&port, Arch::X64, &ver).unwrap(),
            "RSQuake-0.2.0-beta.1-windows-x64.zip"
        );
        assert_eq!(
            asset_name(&mac, Arch::Arm64, &ver).unwrap(),
            "RSQuake-0.2.0-beta.1-macos-arm64.dmg"
        );
        assert_eq!(
            asset_name(&mac, Arch::X64, &ver).unwrap(),
            "RSQuake-0.2.0-beta.1-macos-x86_64.dmg"
        );
        assert!(asset_name(&port, Arch::Arm64, &ver).is_err());
        let lin = Target::LinuxBinary { exe: "x".into() };
        assert_eq!(
            asset_name(&lin, Arch::X64, &ver).unwrap(),
            "RSQuake-0.2.0-beta.1-linux-x86_64.tar.gz"
        );
        assert_eq!(
            asset_name(&lin, Arch::Arm64, &ver).unwrap(),
            "RSQuake-0.2.0-beta.1-linux-aarch64.tar.gz"
        );
        assert!(asset_name(&lin, Arch::Other, &ver).is_err());
    }

    #[test]
    fn linux_user_installs_are_updatable_but_managed_and_dev_builds_are_not() {
        let e = env(Os::Linux, "/home/u/.local/bin/rsquake");
        assert_eq!(
            detect(&e),
            Ok(Target::LinuxBinary {
                exe: PathBuf::from("/home/u/.local/bin/rsquake")
            })
        );
        assert!(detect(&env(Os::Linux, "/opt/rsquake/bin/rsquake")).is_ok());
        // パッケージ管理の場所は対象外
        let e = env(Os::Linux, "/usr/bin/rsquake");
        assert!(detect(&e).unwrap_err().contains("パッケージ管理"));
        // 開発用のビルド (target の親に Cargo.toml がある) は上書きしない
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join("Cargo.toml"), "").unwrap();
        let exe = repo.path().join("target/release/rsquake");
        assert!(
            detect(&env(Os::Linux, exe.to_str().unwrap()))
                .unwrap_err()
                .contains("開発用")
        );
    }

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.into(),
            url: format!("{DOWNLOAD_PREFIX}v0.2.0/{name}"),
            size: 1,
        }
    }

    fn release(names: &[&str]) -> Release {
        Release {
            version: v("0.2.0"),
            url: super::super::release_url(&v("0.2.0")),
            assets: names.iter().map(|n| asset(n)).collect(),
        }
    }

    #[test]
    fn plan_needs_the_asset_the_sums_and_the_signature() {
        let mac = Target::MacApp {
            app: "/A/R.app".into(),
        };
        let all = ["RSQuake-0.2.0-macos-arm64.dmg", SUMS_NAME, SIG_NAME];
        let p = plan(&release(&all), mac.clone(), Arch::Arm64).unwrap();
        assert_eq!(p.asset.name, "RSQuake-0.2.0-macos-arm64.dmg");
        assert_eq!(p.sums.name, SUMS_NAME);
        // 署名が無いリリースは、アプリ内で更新しない
        let no_sig = ["RSQuake-0.2.0-macos-arm64.dmg", SUMS_NAME];
        assert!(
            plan(&release(&no_sig), mac.clone(), Arch::Arm64)
                .unwrap_err()
                .contains(SIG_NAME)
        );
        // 別の CPU 向けしか無いとき
        assert!(plan(&release(&all), mac, Arch::X64).is_err());
    }

    #[test]
    fn plan_refuses_foreign_download_urls() {
        let mac = Target::MacApp {
            app: "/A/R.app".into(),
        };
        let mut r = release(&["RSQuake-0.2.0-macos-arm64.dmg", SUMS_NAME, SIG_NAME]);
        r.assets[0].url = "https://evil.example/RSQuake.dmg".into();
        assert!(plan(&r, mac, Arch::Arm64).is_err());
    }
}
