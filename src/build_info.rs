use serde::Serialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CHANNEL: &str = env!("TCP_OPTIMISER_BUILD_CHANNEL");
pub const SOURCE: &str = env!("TCP_OPTIMISER_BUILD_SOURCE");
pub const REVISION: &str = env!("TCP_OPTIMISER_BUILD_REVISION");
pub const LONG_VERSION: &str = env!("TCP_OPTIMISER_LONG_VERSION");

// Keep a searchable provenance marker in official stripped binaries. This is
// attribution evidence, not DRM: a source fork can still remove and rebuild it.
#[used]
static BUILD_WATERMARK: &str = env!("TCP_OPTIMISER_BUILD_WATERMARK");

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BuildInfo {
    pub version: &'static str,
    pub channel: &'static str,
    pub source: &'static str,
    pub revision: &'static str,
}

pub const fn current() -> BuildInfo {
    BuildInfo {
        version: VERSION,
        channel: CHANNEL,
        source: SOURCE,
        revision: REVISION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_build_has_explicit_provenance() {
        let info = current();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.channel.is_empty());
        assert!(!info.source.is_empty());
        assert!(!info.revision.is_empty());
    }
}
