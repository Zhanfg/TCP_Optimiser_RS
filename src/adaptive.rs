use serde::Serialize;
use std::io;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathState {
    Stable,
    Bufferbloat,
    LossyWireless,
    HighBdp,
    Congested,
    ProxyConstrained,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetrySample {
    pub interval_ms: u64,
    pub avg_rtt_ms: Option<f64>,
    pub max_rtt_ms: Option<f64>,
    pub baseline_rtt_ms: Option<f64>,
    pub retrans_ratio: Option<f64>,
    pub rx_mbps: Option<f64>,
    pub tx_mbps: Option<f64>,
    pub established: u32,
    pub tcp_in_use: Option<u32>,
    pub transparent_proxy: bool,
    pub wifi_frequency_mhz: Option<u32>,
}

impl TelemetrySample {
    fn aggregate_mbps(&self) -> f64 {
        self.rx_mbps.unwrap_or(0.0) + self.tx_mbps.unwrap_or(0.0)
    }

    fn rtt_inflation(&self) -> Option<f64> {
        let baseline = self.baseline_rtt_ms.filter(|value| *value > 0.0)?;
        Some(self.avg_rtt_ms? / baseline)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub state: PathState,
    pub confidence: u8,
    pub reasons: Vec<String>,
    pub sample: TelemetrySample,
}

/// Observe a physical interface without changing any kernel/network setting.
///
/// The sampler deliberately uses two counter snapshots so cumulative kernel
/// counters become interval deltas. It performs no active traffic generation.
pub fn observe(active_iface: &str, interval: Duration) -> io::Result<Classification> {
    let interval = interval.max(Duration::from_millis(250));
    let before = crate::stats::adaptive_counters(active_iface);
    thread::sleep(interval);
    let after = crate::stats::adaptive_counters(active_iface);
    let proxy = crate::proxy::detect_proxy_snapshot();

    let seconds = interval.as_secs_f64();
    let interval_ms = interval.as_millis().min(u64::MAX as u128) as u64;

    let retrans_ratio = before
        .tcp
        .as_ref()
        .zip(after.tcp.as_ref())
        .and_then(|(a, b)| {
            let out = b.out_segs.saturating_sub(a.out_segs);
            (out > 0).then(|| {
                let retrans = b.retrans.saturating_sub(a.retrans);
                retrans as f64 / out as f64
            })
        });

    let (rx_mbps, tx_mbps) = before
        .iface
        .as_ref()
        .zip(after.iface.as_ref())
        .map(|(a, b)| {
            (
                bytes_to_mbps(b.rx_bytes.saturating_sub(a.rx_bytes), seconds),
                bytes_to_mbps(b.tx_bytes.saturating_sub(a.tx_bytes), seconds),
            )
        })
        .map_or((None, None), |(rx, tx)| (Some(rx), Some(tx)));

    let conn = after.conn_info.as_ref();
    let sample = TelemetrySample {
        interval_ms,
        avg_rtt_ms: conn.map(|value| value.avg_rtt_ms),
        max_rtt_ms: conn.map(|value| value.max_rtt_ms),
        // Baseline learning belongs to the persistent daemon controller. The
        // one-shot observer must not invent a path baseline from one sample.
        baseline_rtt_ms: None,
        retrans_ratio,
        rx_mbps,
        tx_mbps,
        established: after.established,
        tcp_in_use: after.sock.as_ref().map(|value| value.tcp_in_use),
        transparent_proxy: proxy.transparent,
        wifi_frequency_mhz: crate::network::wifi_freq(active_iface),
    };

    Ok(classify(sample))
}

fn bytes_to_mbps(bytes: u64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return 0.0;
    }
    bytes as f64 * 8.0 / seconds / 1_000_000.0
}

/// Deterministic instantaneous classifier.
///
/// This function only describes the observed path. It does not select or
/// apply congestion control, qdisc, sysctl or route changes.
pub fn classify(sample: TelemetrySample) -> Classification {
    let mut reasons = Vec::new();
    let rtt = sample.avg_rtt_ms;
    let loss = sample.retrans_ratio;
    let throughput = sample.aggregate_mbps();

    if let Some(inflation) = sample.rtt_inflation() {
        if inflation >= 2.0 && throughput >= 5.0 {
            reasons.push(format!(
                "RTT inflation {:.2}x under {:.1} Mbps load",
                inflation, throughput
            ));
            return Classification {
                state: PathState::Bufferbloat,
                confidence: 92,
                reasons,
                sample,
            };
        }
    }

    if sample.wifi_frequency_mhz.is_some() && loss.is_some_and(|value| value >= 0.03) {
        reasons.push(format!(
            "wireless retransmission ratio {:.2}%",
            loss.unwrap_or_default() * 100.0
        ));
        return Classification {
            state: PathState::LossyWireless,
            confidence: 90,
            reasons,
            sample,
        };
    }

    if loss.is_some_and(|value| value >= 0.015) && rtt.is_some_and(|value| value >= 120.0) {
        reasons.push(format!(
            "elevated RTT {:.1} ms with {:.2}% retransmissions",
            rtt.unwrap_or_default(),
            loss.unwrap_or_default() * 100.0
        ));
        return Classification {
            state: PathState::Congested,
            confidence: 87,
            reasons,
            sample,
        };
    }

    if sample.transparent_proxy
        && rtt.is_some_and(|value| value >= 150.0)
        && loss.is_some_and(|value| value >= 0.005)
    {
        reasons.push("transparent proxy path is active".to_string());
        reasons.push(format!(
            "proxied path RTT {:.1} ms and retransmissions {:.2}%",
            rtt.unwrap_or_default(),
            loss.unwrap_or_default() * 100.0
        ));
        return Classification {
            state: PathState::ProxyConstrained,
            confidence: 76,
            reasons,
            sample,
        };
    }

    if throughput >= 50.0 && rtt.is_some_and(|value| value >= 80.0) {
        reasons.push(format!(
            "high-throughput/high-RTT path: {:.1} Mbps at {:.1} ms",
            throughput,
            rtt.unwrap_or_default()
        ));
        return Classification {
            state: PathState::HighBdp,
            confidence: 72,
            reasons,
            sample,
        };
    }

    if rtt.is_some_and(|value| value < 100.0) && loss.is_some_and(|value| value < 0.005) {
        reasons.push(format!(
            "low retransmission ratio {:.2}% with RTT {:.1} ms",
            loss.unwrap_or_default() * 100.0,
            rtt.unwrap_or_default()
        ));
        return Classification {
            state: PathState::Stable,
            confidence: 82,
            reasons,
            sample,
        };
    }

    if rtt.is_none() {
        reasons.push("no established TCP RTT sample".to_string());
    }
    if loss.is_none() {
        reasons.push("no TCP segment delta in the sampling interval".to_string());
    }
    if reasons.is_empty() {
        reasons.push("signals do not meet a stable path-state threshold".to_string());
    }

    Classification {
        state: PathState::Unknown,
        confidence: 35,
        reasons,
        sample,
    }
}

/// Small state machine used by the future daemon integration to prevent a
/// transient sample from flipping the active path state.
#[derive(Debug, Clone)]
pub struct HysteresisClassifier {
    current: PathState,
    pending: Option<PathState>,
    pending_count: u8,
    required_samples: u8,
    min_confidence: u8,
}

impl Default for HysteresisClassifier {
    fn default() -> Self {
        Self {
            current: PathState::Unknown,
            pending: None,
            pending_count: 0,
            required_samples: 3,
            min_confidence: 60,
        }
    }
}

impl HysteresisClassifier {
    pub fn current(&self) -> PathState {
        self.current
    }

    pub fn update(&mut self, observation: &Classification) -> PathState {
        if observation.confidence < self.min_confidence {
            self.pending = None;
            self.pending_count = 0;
            return self.current;
        }

        if observation.state == self.current {
            self.pending = None;
            self.pending_count = 0;
            return self.current;
        }

        if self.pending == Some(observation.state) {
            self.pending_count = self.pending_count.saturating_add(1);
        } else {
            self.pending = Some(observation.state);
            self.pending_count = 1;
        }

        if self.pending_count >= self.required_samples {
            self.current = observation.state;
            self.pending = None;
            self.pending_count = 0;
        }

        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TelemetrySample {
        TelemetrySample {
            interval_ms: 1000,
            avg_rtt_ms: Some(35.0),
            max_rtt_ms: Some(45.0),
            baseline_rtt_ms: None,
            retrans_ratio: Some(0.001),
            rx_mbps: Some(10.0),
            tx_mbps: Some(2.0),
            established: 4,
            tcp_in_use: Some(8),
            transparent_proxy: false,
            wifi_frequency_mhz: Some(5180),
        }
    }

    #[test]
    fn stable_path_is_classified_without_mutation() {
        let result = classify(sample());
        assert_eq!(result.state, PathState::Stable);
        assert!(result.confidence >= 80);
    }

    #[test]
    fn bufferbloat_requires_learned_baseline_and_load() {
        let mut input = sample();
        input.baseline_rtt_ms = Some(30.0);
        input.avg_rtt_ms = Some(95.0);
        input.rx_mbps = Some(80.0);
        let result = classify(input);
        assert_eq!(result.state, PathState::Bufferbloat);
    }

    #[test]
    fn wireless_loss_wins_over_generic_congestion() {
        let mut input = sample();
        input.avg_rtt_ms = Some(180.0);
        input.retrans_ratio = Some(0.05);
        let result = classify(input);
        assert_eq!(result.state, PathState::LossyWireless);
    }

    #[test]
    fn high_bdp_path_is_detected() {
        let mut input = sample();
        input.avg_rtt_ms = Some(90.0);
        input.retrans_ratio = Some(0.006);
        input.rx_mbps = Some(120.0);
        let result = classify(input);
        assert_eq!(result.state, PathState::HighBdp);
    }

    #[test]
    fn hysteresis_requires_repeated_confident_observations() {
        let observation = classify(sample());
        let mut classifier = HysteresisClassifier::default();

        assert_eq!(classifier.update(&observation), PathState::Unknown);
        assert_eq!(classifier.update(&observation), PathState::Unknown);
        assert_eq!(classifier.update(&observation), PathState::Stable);
        assert_eq!(classifier.current(), PathState::Stable);
    }

    #[test]
    fn low_confidence_sample_does_not_flip_state() {
        let stable = classify(sample());
        let mut classifier = HysteresisClassifier::default();
        classifier.update(&stable);
        classifier.update(&stable);
        classifier.update(&stable);

        let mut unknown_sample = sample();
        unknown_sample.avg_rtt_ms = None;
        unknown_sample.retrans_ratio = None;
        let unknown = classify(unknown_sample);
        assert_eq!(unknown.state, PathState::Unknown);
        assert!(unknown.confidence < 60);
        assert_eq!(classifier.update(&unknown), PathState::Stable);
    }
}
