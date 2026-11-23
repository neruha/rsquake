// Windows: exe にアイコンとバージョン情報を埋め込む (エクスプローラーやタスクバーで使われる)。
// 他の OS では何もしない。
fn main() {
    println!("cargo:rerun-if-changed=assets/icon/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/icon.ico");
        res.set("ProductName", "RSQuake");
        res.set("FileDescription", "RSQuake");
        if let Err(e) = res.compile() {
            // リソースが無くても動くので、ビルドは止めない
            println!("cargo:warning=アイコンの埋め込みに失敗: {e}");
        }
    }
}
