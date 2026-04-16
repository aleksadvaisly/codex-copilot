use std::process::Command;

fn main() {
    println!("cargo:rustc-env=CARGO_PKG_VERSION={}", build_version());
}

fn build_version() -> String {
    if let Ok(version) = std::env::var("CODEX_BUILD_VERSION") {
        let version = version.trim();
        if !version.is_empty() {
            return version.to_owned();
        }
    }

    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return "0.0.0".to_string();
    }

    let output = Command::new("sh")
        .args([
            "-c",
            r#"year=$(date +%y); monthday=$(date +%m%d); monthday=${monthday#0}; hour=$(date +%H); printf 'v%s.%s.%s' "$year" "$monthday" "$hour""#,
        ])
        .output()
        .expect("failed to compute build version");

    assert!(output.status.success(), "failed to compute build version");

    String::from_utf8(output.stdout)
        .expect("version is valid utf-8")
        .trim()
        .to_owned()
}
