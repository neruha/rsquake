//! ダウンロードしたファイルの確認。
//!
//! `SHA256SUMS` を minisign で署名し、アプリに埋め込んだ公開鍵で確かめる。
//! そのうえで、配布物の SHA-256 が `SHA256SUMS` の該当行と合うことを確かめる。

use std::io::Read;
use std::path::Path;

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

/// 署名の公開鍵 (minisign)。対になる秘密鍵は、リリースの CI (`release.yml`) の secret にある。
/// 鍵を作り直すと、古い版のアプリが新しい版を受け付けなくなる。
pub const PUBLIC_KEY: &str = "RWRBln5NwYcW1hoogap/HJk+sLfxlI4AosFEC0u69cf+42jXpLzstaIO";

/// 使う公開鍵。デバッグビルドだけ、試験用に `RSQUAKE_UPDATE_PUBKEY` で差し替えられる。
pub fn public_key() -> String {
    #[cfg(debug_assertions)]
    if let Ok(k) = std::env::var("RSQUAKE_UPDATE_PUBKEY") {
        return k;
    }
    PUBLIC_KEY.to_string()
}

/// `SHA256SUMS` の中身が、公開鍵に対応する秘密鍵で署名されたものかを確かめる。
pub fn verify_sums(sums: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    let key = PublicKey::from_base64(public_key).map_err(|e| format!("公開鍵を読めません: {e}"))?;
    let sig = Signature::decode(signature).map_err(|e| format!("署名を読めません: {e}"))?;
    key.verify(sums, &sig, false)
        .map_err(|e| format!("SHA256SUMS の署名が合いません: {e}"))
}

/// `SHA256SUMS` (`sha256sum` の出力) から、ファイル名に対応するハッシュを取る。
/// 行は `<ハッシュ>  <名前>` (バイナリモードでは `<ハッシュ> *<名前>`)。
pub fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, rest) = line.trim().split_once(char::is_whitespace)?;
        let file = rest.trim_start().trim_start_matches('*');
        (file == name && is_sha256_hex(hash)).then(|| hash.to_ascii_lowercase())
    })
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// ファイルの SHA-256 (16 進、小文字)。
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// 流しながらハッシュを取る。
#[derive(Default)]
pub struct StreamHash(Sha256);

impl StreamHash {
    pub fn update(&mut self, chunk: &[u8]) {
        self.0.update(chunk);
    }

    pub fn finish(self) -> String {
        hex(&self.0.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // minisign で作った試験用の鍵と、その鍵で署名した SHA256SUMS (本番の鍵とは別)
    const TEST_KEY: &str = "RWRRS95L2SSVpPjPdK8xSg6CzI3rrNyG8zvOLLP8J22P25qPh7oys6PB";
    const SUMS: &str = "aaaa11  RSQuake-0.2.0-macos-arm64.dmg\n";
    const SIG: &str = "untrusted comment: test
RURRS95L2SSVpOjRk2XglB1iPBdqsj6ADMXl1aVPTQ2L2P/WbBSj5Vq+ggr11/ytk3qVH0P9xgrNEeEWnehHuUPW4kEdLY3Epwk=
trusted comment: timestamp:1795487074\tfile:SHA256SUMS\thashed
ddu4S1hA2Q/vVSqu0GDQ/N3Hq1u4OOATeqfNDxD6XxFLl5BnXmWMWkd1XH3sTiEstolwFpn8KNgIjaUkN1hpCg==
";

    #[test]
    fn valid_signature_passes() {
        verify_sums(SUMS.as_bytes(), SIG, TEST_KEY).unwrap();
    }

    #[test]
    fn tampered_sums_fail() {
        let bad = SUMS.replace("aaaa11", "bbbb22");
        assert!(verify_sums(bad.as_bytes(), SIG, TEST_KEY).is_err());
    }

    #[test]
    fn signature_by_another_key_fails() {
        // 本番の公開鍵では、試験用の鍵の署名は通らない
        assert!(verify_sums(SUMS.as_bytes(), SIG, PUBLIC_KEY).is_err());
    }

    #[test]
    fn garbage_signature_or_key_fail() {
        assert!(verify_sums(SUMS.as_bytes(), "x", TEST_KEY).is_err());
        assert!(verify_sums(SUMS.as_bytes(), SIG, "x").is_err());
    }

    #[test]
    fn embedded_public_key_is_valid() {
        PublicKey::from_base64(PUBLIC_KEY).expect("埋め込んだ公開鍵が読めない");
    }

    const H1: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const H2: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn sums_lines_are_looked_up_by_name() {
        let sums = format!("{H1}  a.dmg\n{H2} *b.msi\nnot-a-hash  c.zip\n");
        assert_eq!(expected_hash(&sums, "a.dmg").as_deref(), Some(H1));
        assert_eq!(expected_hash(&sums, "b.msi").as_deref(), Some(H2));
        assert_eq!(expected_hash(&sums, "c.zip"), None);
        assert_eq!(expected_hash(&sums, "missing"), None);
        // 名前の一部だけ合っても拾わない
        assert_eq!(expected_hash(&sums, "a.dm"), None);
    }

    #[test]
    fn hashes_are_compared_in_lowercase() {
        let sums = format!("{}  a.dmg\n", H1.to_ascii_uppercase());
        assert_eq!(expected_hash(&sums, "a.dmg").as_deref(), Some(H1));
    }

    #[test]
    fn stream_and_file_hashes_match_known_value() {
        let mut s = StreamHash::default();
        s.update(b"hel");
        s.update(b"lo");
        assert_eq!(s.finish(), H2);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"hello").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), H2);
    }
}
