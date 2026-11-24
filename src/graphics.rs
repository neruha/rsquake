//! 描画バックエンド (wgpu) の選び方と、選んだバックエンドで起動できなかったときの戻し方。
//!
//! Windows では Vulkan (従来の既定) と DirectX 12 を選べる。DirectX 12 はメモリが少なくて済むが、
//! ドライバによっては起動直後に落ちうる。落ちたかどうかは目印のファイルで見る。
//! DirectX 12 で起動するときに書き、しばらく動いたら消す。次の起動で残っていれば、
//! 前回は起動の途中で落ちたとみなして Vulkan に戻す。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui_wgpu::{WgpuSetup, wgpu};

use crate::config::{Config, GraphicsBackend};

/// 描画方式を選べる OS か。
pub const SELECTABLE: bool = cfg!(windows);

/// これだけ動けば、起動は済んだとみなして目印を消す。
const SETTLE: Duration = Duration::from_secs(5);
const MARKER_FILE: &str = "graphics-starting";

/// この起動の描画の状態。
pub struct Launch {
    /// この起動で使うバックエンド (設定画面で変えても、再起動までは変わらない)
    pub backend: GraphicsBackend,
    /// DirectX 12 を使える GPU がある
    pub dx12_available: bool,
    pub guard: Option<StartupGuard>,
}

/// 起動の前に、使うバックエンドを決める。前回 DirectX 12 で落ちていた、
/// または DirectX 12 を使えないときは、設定を Vulkan に戻して保存する。
pub fn prepare(config: &mut Config) -> Launch {
    if !SELECTABLE {
        return Launch {
            backend: GraphicsBackend::Vulkan,
            dx12_available: false,
            guard: None,
        };
    }
    let dir = Config::dir();
    let g = &mut config.graphics;
    let mut changed = false;
    if dir.as_deref().is_some_and(take_crash_marker) && g.backend == GraphicsBackend::Dx12 {
        tracing::warn!("前回は DirectX 12 で起動できなかったので、Vulkan に戻す");
        g.backend = GraphicsBackend::Vulkan;
        g.dx12_failed = true;
        changed = true;
    }
    let dx12_available = dx12_available();
    if g.backend == GraphicsBackend::Dx12 && !dx12_available {
        tracing::warn!("DirectX 12 を使える GPU が無いので、Vulkan で起動する");
        g.backend = GraphicsBackend::Vulkan;
        changed = true;
    }
    if changed && let Err(e) = config.save() {
        tracing::warn!("設定を保存できない: {e}");
    }
    let backend = config.graphics.backend;
    let guard = (backend == GraphicsBackend::Dx12)
        .then(|| dir.and_then(|d| StartupGuard::arm(&d)))
        .flatten();
    Launch {
        backend,
        dx12_available,
        guard,
    }
}

/// eframe の wgpu の設定に、バックエンドとメモリの確保方法を入れる。
pub fn configure(options: &mut eframe::NativeOptions, backend: GraphicsBackend) {
    let WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup else {
        return;
    };
    // 既定 (Performance) は GPU 用のメモリを 128MB 単位で先に取る。
    // egui のテクスチャは数 MB なので、細かく取ってメモリを抑える
    let base = setup.device_descriptor.clone();
    setup.device_descriptor = Arc::new(move |adapter| wgpu::DeviceDescriptor {
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..base(adapter)
    });
    // 環境変数 WGPU_BACKEND があれば、そちらを優先する (切り分け用)
    if SELECTABLE && backend == GraphicsBackend::Dx12 && wgpu::Backends::from_env().is_none() {
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
    }
}

/// DirectX 12 で動く GPU があるか。ソフトウェア描画 (WARP) は数えない。
pub fn dx12_available() -> bool {
    if !SELECTABLE {
        return false;
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12))
        .iter()
        .any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
}

/// 起動中の目印。落ちずに `SETTLE` だけ動いたら `disarm` で消す。
/// 落ちたときに残すため、`Drop` では消さない (パニックの巻き戻しでも残す)。
pub struct StartupGuard {
    path: PathBuf,
    armed_at: Instant,
}

impl StartupGuard {
    /// 目印を書く。中身はこのプロセスの PID。
    pub fn arm(dir: &Path) -> Option<Self> {
        let path = dir.join(MARKER_FILE);
        let written = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&path, std::process::id().to_string()));
        if let Err(e) = written {
            tracing::warn!("描画の目印を書けない: {e}");
            return None;
        }
        Some(Self {
            path,
            armed_at: Instant::now(),
        })
    }

    pub fn settled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.armed_at) >= SETTLE
    }

    pub fn disarm(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 前回の起動の目印が残っていたら消して `true` (落ちていた)。
/// 目印を書いたプロセスがまだ動いているなら、落ちたのではないので残す (多重起動)。
pub fn take_crash_marker(dir: &Path) -> bool {
    let path = dir.join(MARKER_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    if let Ok(pid) = text.trim().parse::<u32>()
        && pid != std::process::id()
        && process_alive(pid)
    {
        return false;
    }
    let _ = std::fs::remove_file(&path);
    true
}

/// 目印を消す (落ちたのではなく、エラーで止まったときに、自分で戻してから使う)。
pub fn clear_marker() {
    if let Some(dir) = Config::dir() {
        let _ = std::fs::remove_file(dir.join(MARKER_FILE));
    }
}

/// 同じ引数で起動し直す。成功したら、呼び出し側はすぐ終了する。
pub fn restart() -> std::io::Result<()> {
    std::process::Command::new(std::env::current_exe()?)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map(drop)
}

/// 設定画面に出す名前。
pub fn backend_label(b: wgpu::Backend) -> String {
    match b {
        wgpu::Backend::Dx12 => "DirectX 12".into(),
        wgpu::Backend::Vulkan => "Vulkan".into(),
        wgpu::Backend::Gl => "OpenGL".into(),
        wgpu::Backend::Metal => "Metal".into(),
        other => format!("{other:?}"),
    }
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: ハンドルは自分で開いて、このブロックの中で閉じる。
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

#[cfg(not(windows))]
fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_marker_means_no_crash() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!take_crash_marker(dir.path()));
    }

    #[test]
    fn leftover_marker_of_dead_process_is_a_crash_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        // 使われていない PID (Windows の PID は 4 の倍数なので、奇数は使われない)
        std::fs::write(dir.path().join(MARKER_FILE), "4294967295").unwrap();
        assert!(take_crash_marker(dir.path()));
        assert!(!dir.path().join(MARKER_FILE).exists());
        assert!(!take_crash_marker(dir.path()));
    }

    #[test]
    fn disarmed_guard_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let g = StartupGuard::arm(dir.path()).unwrap();
        assert!(dir.path().join(MARKER_FILE).exists());
        assert!(!g.settled(Instant::now()));
        assert!(g.settled(Instant::now() + SETTLE));
        g.disarm();
        assert!(!take_crash_marker(dir.path()));
    }

    #[test]
    #[cfg(windows)]
    fn marker_of_running_process_is_not_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "ping -n 3 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        std::fs::write(dir.path().join(MARKER_FILE), child.id().to_string()).unwrap();
        assert!(!take_crash_marker(dir.path()));
        assert!(dir.path().join(MARKER_FILE).exists());
        child.kill().ok();
        child.wait().ok();
    }
}
