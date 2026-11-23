//! テスト間で共有するヘルパー。
//!
//! 暫定措置: `tests/fixtures` は .gitignore で公開リポジトリに含めていないので、
//! 手元に無いときはそのテストを飛ばす。公開できるフィクスチャを用意したら外す。

use std::path::Path;

/// `tests/fixtures/<path>` (フォルダかファイル) があるか。無ければ理由を出して `false`。
pub fn has_fixture(path: &str) -> bool {
    let ok = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
        .exists();
    if !ok {
        eprintln!("skip: tests/fixtures/{path} が無い");
    }
    ok
}

/// 挙げたフィクスチャが 1 つでも無ければ、テストをそこで終える。
#[macro_export]
macro_rules! require_fixtures {
    ($($path:expr),+ $(,)?) => {
        $(
            if !common::has_fixture($path) {
                return;
            }
        )+
    };
}
