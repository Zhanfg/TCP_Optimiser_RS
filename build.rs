use std::env;

fn main() {
    for key in [
        "TCP_OPTIMISER_OFFICIAL_BUILD",
        "TCP_OPTIMISER_BUILD_REPOSITORY",
        "TCP_OPTIMISER_BUILD_REVISION",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }

    let official = env::var("TCP_OPTIMISER_OFFICIAL_BUILD").as_deref() == Ok("1");
    let channel = if official {
        "official-github"
    } else {
        "local-development"
    };
    let repository = if official {
        env::var("TCP_OPTIMISER_BUILD_REPOSITORY")
            .unwrap_or_else(|_| "Zhanfg/TCP_Optimiser_RS".to_string())
    } else {
        "local".to_string()
    };
    let revision = if official {
        env::var("TCP_OPTIMISER_BUILD_REVISION").unwrap_or_else(|_| "unknown".to_string())
    } else {
        "uncommitted".to_string()
    };
    let source = if official {
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
