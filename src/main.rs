#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Duration;

use rsquake::app::EewApp;
use rsquake::config::{Config, EewSource};
use rsquake::dev::replay::{self, ReplayOptions};
#[cfg(feature = "srev")]
use rsquake::dev::srev;
use rsquake::geo::stations::StationList;
use rsquake::geo::topo::Topo;
use rsquake::graphics;
use rsquake::net::kmoni::{self, KmoniOptions};
use rsquake::net::ws::WsSupervisor;
use std::sync::Arc;

struct Args {
    test: bool,
    replay: Vec<PathBuf>,
    interval_ms: Option<u64>,
    no_kmoni: bool,
    /// `--source dmdata|wolfx`。このセッションだけ EEW の受信元を替える (設定ファイルは書き換えない)
    source: Option<EewSource>,
    kmoni_record: Option<PathBuf>,
    kmoni_replay: Option<PathBuf>,
    #[cfg(feature = "srev")]
    srev: Option<PathBuf>,
    #[cfg(feature = "srev")]
    sim_start: Option<String>,
    #[cfg(feature = "srev")]
    sim_anchor_ms: Option<i64>,
}

fn parse_args() -> Args {
    let mut args = Args {
        test: false,
        replay: Vec::new(),
        interval_ms: None,
        no_kmoni: false,
        source: None,
        kmoni_record: None,
        kmoni_replay: None,
        #[cfg(feature = "srev")]
        srev: None,
        #[cfg(feature = "srev")]
        sim_start: None,
        #[cfg(feature = "srev")]
        sim_anchor_ms: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--test" => args.test = true,
            "--no-kmoni" => args.no_kmoni = true,
            "--source" => {
                args.source = match it.next().as_deref() {
                    Some("dmdata") => Some(EewSource::Dmdata),
                    Some("wolfx") => Some(EewSource::Wolfx),
                    other => {
                        eprintln!("--source は dmdata か wolfx: {other:?}");
                        None
                    }
                }
            }
            "--replay" => args.replay.extend(it.next().map(PathBuf::from)),
            "--interval" => args.interval_ms = it.next().and_then(|v| v.parse().ok()),
            "--kmoni-record" => args.kmoni_record = it.next().map(PathBuf::from),
            "--kmoni-replay" => args.kmoni_replay = it.next().map(PathBuf::from),
            #[cfg(feature = "srev")]
            "--srev" => args.srev = it.next().map(PathBuf::from),
            #[cfg(feature = "srev")]
            "--sim-start" => args.sim_start = it.next(),
            #[cfg(feature = "srev")]
            "--sim-anchor" => args.sim_anchor_ms = it.next().and_then(|v| v.parse().ok()),
            other => eprintln!("不明な引数: {other}"),
        }
    }
    args
}

#[cfg(feature = "srev")]
fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2)
}

fn main() -> eframe::Result {
    // 更新の適用役として起動されたときは、画面を出さずに入れ替えだけして終わる
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = rsquake::update::apply::run_from_args(&raw_args) {
        std::process::exit(code);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rsquake=info".into()),
        )
        .init();

    let args = parse_args();
    let mut config = Config::load();
    if let Some(source) = args.source
        && source.selectable()
    {
        config.source = source;
    }
    if args.test {
        config.force_test_mode();
    }
    let launch = graphics::prepare(&mut config);
    let backend = launch.backend;
    let replaying = !args.replay.is_empty();
    let kmoni_enabled = !args.no_kmoni;
    let stations = Arc::new(StationList::bundled());
    // Wolfx の区域名 → code。地図データから 1 回だけ作る
    let region_codes = Arc::new(
        Topo::parse(include_bytes!("../assets/geojson/eew-prefregion.json"))
            .expect("同梱の地図が読めない")
            .region_name_to_code(),
    );

    // srev の再生は、強震モニタの取得と電文のリプレイの代わりには使えない (時間軸が別になる)
    #[cfg(feature = "srev")]
    let srev = args.srev.as_ref().map(|path| {
        if replaying || args.no_kmoni || args.kmoni_replay.is_some() {
            fail("--srev は --replay / --no-kmoni / --kmoni-replay と同時に使えない");
        }
        srev::setup(
            path,
            args.sim_start.as_deref(),
            args.sim_anchor_ms,
            &stations,
        )
        .unwrap_or_else(|e| fail(&e))
    });
    #[cfg(feature = "srev")]
    let clock = srev.as_ref().map(|(_, c)| *c);
    #[cfg(not(feature = "srev"))]
    let clock: Option<rsquake::clock::SimClock> = None;

    let (tx, rx) = std::sync::mpsc::channel();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("RSQuake")
        .with_inner_size([1440.0, 860.0])
        .with_min_inner_size([1000.0, 640.0]);
    // タスクバー / Dock 以外 (Windows のタイトルバー、Linux) で使われるウィンドウアイコン
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    graphics::configure(&mut options, backend);

    let result = eframe::run_native(
        "rsquake",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            #[cfg(feature = "srev")]
            let srev_active = if let Some((replay, clock)) = srev.clone() {
                srev::spawn(replay, clock, stations.clone(), tx.clone(), ctx.clone());
                true
            } else {
                false
            };
            #[cfg(not(feature = "srev"))]
            let srev_active = false;
            if !srev_active && kmoni_enabled {
                if let Some(dir) = args.kmoni_replay.clone() {
                    kmoni::spawn_replay(dir, stations.clone(), tx.clone(), ctx.clone());
                } else {
                    let mut opts = KmoniOptions::new(stations.clone());
                    opts.record_dir = args.kmoni_record.clone();
                    kmoni::spawn(opts, tx.clone(), ctx.clone());
                }
            }
            if replaying {
                replay::spawn(
                    ReplayOptions {
                        paths: args.replay,
                        interval: args.interval_ms.map(Duration::from_millis),
                        max_gap: Duration::from_secs(5),
                        start_delay: Duration::from_secs(1),
                        region_codes: region_codes.clone(),
                    },
                    tx.clone(),
                    ctx.clone(),
                );
            }
            let supervisor = (!replaying).then(|| {
                WsSupervisor::start(&config, tx.clone(), ctx.clone(), region_codes.clone())
            });
            Ok(Box::new(EewApp::new(
                cc,
                config,
                rx,
                replaying,
                stations,
                kmoni_enabled,
                clock,
                supervisor,
                launch,
            )))
        }),
    );
    if let Err(e) = &result
        && backend == rsquake::config::GraphicsBackend::Dx12
    {
        // 落ちずにエラーで返ってきたときは、次の起動を待たずにここで Vulkan に戻す
        tracing::error!("DirectX 12 で起動できない ({e})。Vulkan で起動し直す");
        graphics::clear_marker();
        let mut c = Config::load();
        c.graphics.backend = rsquake::config::GraphicsBackend::Vulkan;
        c.graphics.dx12_failed = true;
        if let Err(e) = c.save() {
            tracing::warn!("設定を保存できない: {e}");
        } else if graphics::restart().is_ok() {
            return Ok(());
        }
    }
    result
}
