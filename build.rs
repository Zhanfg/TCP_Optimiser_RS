use std::env;

const OFFICIAL_CHANNEL: &str = "official-github";
const CANDIDATE_CHANNEL: &str = "pull-request-candidate";
const LOCAL_CHANNEL: &str = "local-development";

fn main() {
    for key in [
        "TCP_OPTIMISER_OFFICIAL_BUILD",
        "TCP_OPTIMISER_BUILD_CHANNEL",
        "TCP_OPTIMISER_BUILD_REPOSITORY",
        "TCP_OPTIMISER_BUILD_REVISION",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }

    let trusted_ci_source = env::var("TCP_OPTIMISER_OFFICIAL_BUILD").as_deref() == Ok("1");
    let channel = if trusted_ci_source {
        let requested = env::var("TCP_OPTIMISER_BUILD_CHANNEL")
            .unwrap_or_else(|_| OFFICIAL_CHANNEL.to_string());
        match requested.as_str() {
            OFFICIAL_CHANNEL | CANDIDATE_CHANNEL => requested,
            other => panic!("unsupported TCP_OPTIMISER_BUILD_CHANNEL: {other}"),
        }
    } else {
        LOCAL_CHANNEL.to_string()
    };
    let repository = if trusted_ci_source {
        env::var("TCP_OPTIMISER_BUILD_REPOSITORY")
            .unwrap_or_else(|_| "Zhanfg/TCP_Optimiser_RS".to_string())
    } else {
        "local".to_string()
    };
    let revision = if trusted_ci_source {
        env::var("TCP_OPTIMISER_BUILD_REVISION").unwrap_or_else(|_| "unknown".to_string())
    } else {
        "uncommitted".to_string()
    };
    let source = if trusted_ci_source {
        format!("https://github.com/{repository}")
    } else {
        "local source tree".to_string()
    };
    let version = env::var("CARGO_PKG_VERSION").expect("Cargo package version");
    let watermark =
        format!("TCP Optimiser | channel={channel} | source={source} | revision={revision}");
    let long_version =
        format!("{version}\nBuild channel: {channel}\nSource: {source}\nRevision: {revision}");

    println!("cargo:rustc-env=TCP_OPTIMISER_BUILD_CHANNEL={channel}");
    println!("cargo:rustc-env=TCP_OPTIMISER_BUILD_SOURCE={source}");
    println!("cargo:rustc-env=TCP_OPTIMISER_BUILD_REVISION={revision}");
    println!("cargo:rustc-env=TCP_OPTIMISER_BUILD_WATERMARK={watermark}");
    println!("cargo:rustc-env=TCP_OPTIMISER_LONG_VERSION={long_version}");
}
