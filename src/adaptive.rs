use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetrySample {
    pub interval_ms: u64,
    pub avg_rtt_ms: Option<f64>,
    pub max_rtt_ms: Option<f64>,
    pub baseline_rtt_ms: Option<f64>,
    pub retrans_ratio: Option<f64>,
    pub rx_mbps: Option<f64>,
    pub tx_mbps: Option<f64>,
    pub qdisc_name: Option<String>,
    pub qdisc_backlog_bytes: Option<u64>,
    pub qdisc_backlog_packets: Option<u64>,
    pub qdisc_drop_delta: Option<u64>,
    pub qdisc_overlimit_delta: Option<u64>,
    pub qdisc_requeue_delta: Option<u64>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Classification {
    pub state: PathState,
    pub confidence: u8,
    pub reasons: Vec<String>,
    pub sample: TelemetrySample,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdaptiveReport {
    pub stable_state: PathState,
    pub baseline_rtt_ms: Option<f64>,
    pub baseline_samples: u8,
    pub observations: Vec<Classification>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdaptiveRuntimeState {
    pub schema: u32,
    pub updated_epoch: u64,
    pub iface: String,
    pub stable_state: PathState,
    pub baseline_rtt_ms: Option<f64>,
    pub baseline_samples: u8,
    pub latest: Classification,
}

#[derive(Debug, Default)]
pub struct RuntimeObserver {
    iface: String,
    previous: Option<(Instant, crate::stats::AdaptiveCounters)>,
    baseline: BaselineEstimator,
    hysteresis: HysteresisClassifier,
}

impl RuntimeObserver {
    pub fn clear(&mut self) {
        self.iface.clear();
        self.previous = None;
        self.baseline = BaselineEstimator::default();
        self.hysteresis = HysteresisClassifier::default();
    }

    /// Consume one daemon-loop observation without sleeping or mutating the
    /// network. The first observation for an interface only establishes the
    /// counter baseline; later observations produce interval deltas.
    pub fn tick(&mut self, active_iface: &str) -> Option<AdaptiveRuntimeState> {
        if self.iface != active_iface {
            self.clear();
            self.iface = active_iface.to_string();
            self.previous = Some((
                Instant::now(),
                crate::stats::adaptive_counters(active_iface),
            ));
            return None;
        }

        let (sampled_at, before) = self.previous.as_ref()?;
        let elapsed = sampled_at.elapsed();
        if elapsed < Duration::from_secs(2) {
            return None;
        }

        let after = crate::stats::adaptive_counters(active_iface);
        let proxy = crate::proxy::detect_proxy_snapshot();
        let mut sample =
            telemetry_between(active_iface, before, &after, elapsed, proxy.transparent);
        self.baseline.apply(&mut sample);
        let latest = classify(sample);
        self.hysteresis.update(&latest);
        self.previous = Some((Instant::now(), after));

        Some(AdaptiveRuntimeState {
            schema: 1,
            updated_epoch: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            iface: active_iface.to_string(),
            stable_state: self.hysteresis.current(),
            baseline_rtt_ms: self.baseline.rtt_ms,
            baseline_samples: self.baseline.samples,
            latest,
        })
    }
}

pub fn persist_runtime_state(state: &AdaptiveRuntimeState) -> io::Result<()> {
    let path = crate::config::module_dir().join("adaptive_state.json");
    let temporary = path.with_extension("json.tmp");
    let payload = serde_json::to_vec(state).map_err(io::Error::other)?;
    fs::write(&temporary, payload)?;
    fs::rename(temporary, path)
}

pub fn clear_runtime_state() {
    let _ = fs::remove_file(crate::config::module_dir().join("adaptive_state.json"));
}

const RUNTIME_STATE_MAX_AGE_SECONDS: u64 = 120;

pub fn load_runtime_state(active_iface: &str) -> Option<AdaptiveRuntimeState> {
    let payload = fs::read(crate::config::module_dir().join("adaptive_state.json")).ok()?;
    let state = serde_json::from_slice::<AdaptiveRuntimeState>(&payload).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    runtime_state_is_current(&state, active_iface, now).then_some(state)
}

fn runtime_state_is_current(
    state: &AdaptiveRuntimeState,
    active_iface: &str,
    now_epoch: u64,
) -> bool {
    state.schema == 1
        && state.iface == active_iface
        && now_epoch >= state.updated_epoch
        && now_epoch - state.updated_epoch <= RUNTIME_STATE_MAX_AGE_SECONDS
}

#[derive(Debug, Default)]
struct BaselineEstimator {
    rtt_ms: Option<f64>,
    samples: u8,
}

impl BaselineEstimator {
    fn apply(&mut self, sample: &mut TelemetrySample) {
        sample.baseline_rtt_ms = self.rtt_ms;

        let Some(rtt) = sample.avg_rtt_ms else {
            return;
        };
        let quiet = sample.aggregate_mbps() <= 5.0
            && sample.retrans_ratio.unwrap_or(0.0) <= 0.005
            && sample.qdisc_backlog_bytes.unwrap_or(0) <= 8_000
            && sample.qdisc_drop_delta.unwrap_or(0) == 0;
        if !quiet {
            return;
        }

        self.rtt_ms = Some(self.rtt_ms.map_or(rtt, |baseline| baseline.min(rtt)));
        self.samples = self.samples.saturating_add(1);
        sample.baseline_rtt_ms = self.rtt_ms;
    }
}

/// Observe a physical interface without changing any kernel/network setting.
///
/// Consecutive snapshots reuse the previous sample as the next interval's
/// starting point. This keeps the observer passive while producing real
/// counter deltas for retransmissions, throughput and qdisc pressure.
pub fn observe_series(
    active_iface: &str,
    interval: Duration,
    samples: u8,
) -> io::Result<AdaptiveReport> {
    let interval = interval.max(Duration::from_millis(250));
    let samples = samples.clamp(1, 12);
    let mut before = crate::stats::adaptive_counters(active_iface);
    let mut baseline = BaselineEstimator::default();
    let mut hysteresis = HysteresisClassifier::default();
    let mut observations = Vec::with_capacity(usize::from(samples));

    for _ in 0..samples {
        thread::sleep(interval);
        let after = crate::stats::adaptive_counters(active_iface);
        let proxy = crate::proxy::detect_proxy_snapshot();
        let mut sample =
            telemetry_between(active_iface, &before, &after, interval, proxy.transparent);
        baseline.apply(&mut sample);
        let classification = classify(sample);
        hysteresis.update(&classification);
        observations.push(classification);
        before = after;
    }

    Ok(AdaptiveReport {
        stable_state: hysteresis.current(),
        baseline_rtt_ms: baseline.rtt_ms,
        baseline_samples: baseline.samples,
        observations,
    })
}

fn telemetry_between(
    active_iface: &str,
    before: &crate::stats::AdaptiveCounters,
    after: &crate::stats::AdaptiveCounters,
    interval: Duration,
    transparent_proxy: bool,
) -> TelemetrySample {
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

    let qdisc_name = after.qdisc.as_ref().map(|value| value.name.clone());
    let qdisc_backlog_bytes = after.qdisc.as_ref().map(|value| value.backlog_bytes);
    let qdisc_backlog_packets = after.qdisc.as_ref().map(|value| value.backlog_packets);
    let qdisc_drop_delta = before
        .qdisc
        .as_ref()
        .zip(after.qdisc.as_ref())
        .filter(|(a, b)| a.name == b.name)
        .map(|(a, b)| b.drops.saturating_sub(a.drops));
    let qdisc_overlimit_delta = before
        .qdisc
        .as_ref()
        .zip(after.qdisc.as_ref())
        .filter(|(a, b)| a.name == b.name)
        .map(|(a, b)| b.overlimits.saturating_sub(a.overlimits));
    let qdisc_requeue_delta = before
        .qdisc
        .as_ref()
        .zip(after.qdisc.as_ref())
        .filter(|(a, b)| a.name == b.name)
        .map(|(a, b)| b.requeues.saturating_sub(a.requeues));

    let conn = after.conn_info.as_ref();
    TelemetrySample {
        interval_ms,
        avg_rtt_ms: conn.map(|value| value.avg_rtt_ms),
        max_rtt_ms: conn.map(|value| value.max_rtt_ms),
        baseline_rtt_ms: None,
        retrans_ratio,
        rx_mbps,
        tx_mbps,
        qdisc_name,
        qdisc_backlog_bytes,
        qdisc_backlog_packets,
        qdisc_drop_delta,
        qdisc_overlimit_delta,
        qdisc_requeue_delta,
        established: after.established,
        tcp_in_use: after.sock.as_ref().map(|value| value.tcp_in_use),
        transparent_proxy,
        wifi_frequency_mhz: crate::network::wifi_freq(active_iface),
    }
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
            let queue_evidence = sample.qdisc_backlog_bytes.unwrap_or(0) >= 16_000
                || sample.qdisc_drop_delta.unwrap_or(0) > 0;
            if queue_evidence {
                reasons.push(format!(
                    "qdisc backlog={} bytes, drop delta={}",
                    sample.qdisc_backlog_bytes.unwrap_or(0),
                    sample.qdisc_drop_delta.unwrap_or(0)
                ));
            }
            return Classification {
                state: PathState::Bufferbloat,
                confidence: if queue_evidence { 96 } else { 86 },
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

    if (loss.is_some_and(|value| value >= 0.015)
        || sample.qdisc_drop_delta.is_some_and(|value| value >= 4)
        || sample.qdisc_overlimit_delta.is_some_and(|value| value >= 8))
        && rtt.is_some_and(|value| value >= 120.0)
    {
        reasons.push(format!(
            "elevated RTT {:.1} ms with {:.2}% retransmissions, {} qdisc drops and {} overlimits",
            rtt.unwrap_or_default(),
            loss.unwrap_or_default() * 100.0,
            sample.qdisc_drop_delta.unwrap_or(0),
            sample.qdisc_overlimit_delta.unwrap_or(0)
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

    if rtt.is_some_and(|value| value < 100.0)
        && loss.is_some_and(|value| value < 0.005)
        && sample.qdisc_drop_delta.unwrap_or(0) == 0
    {
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

/// Small state machine that prevents a transient sample from flipping the
/// reported path state.
#[derive(Debug, Clone)]
struct HysteresisClassifier {
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
    fn current(&self) -> PathState {
        self.current
    }

    fn update(&mut self, observation: &Classification) -> PathState {
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
            qdisc_name: Some("fq_codel".to_string()),
            qdisc_backlog_bytes: Some(0),
            qdisc_backlog_packets: Some(0),
            qdisc_drop_delta: Some(0),
            qdisc_overlimit_delta: Some(0),
            qdisc_requeue_delta: Some(0),
            established: 4,
            tcp_in_use: Some(8),
            transparent_proxy: false,
            wifi_frequency_mhz: Some(5180),
        }
    }

    #[test]
    fn runtime_state_rejects_stale_or_wrong_interface() {
        let state = AdaptiveRuntimeState {
            schema: 1,
            updated_epoch: 1_000,
            iface: "wlan0".to_string(),
            stable_state: PathState::Stable,
            baseline_rtt_ms: Some(20.0),
            baseline_samples: 3,
            latest: classify(sample()),
        };
        assert!(runtime_state_is_current(&state, "wlan0", 1_120));
        assert!(!runtime_state_is_current(&state, "wlan0", 1_121));
        assert!(!runtime_state_is_current(&state, "rmnet_data0", 1_100));
        assert!(!runtime_state_is_current(&state, "wlan0", 999));
    }

    #[test]
    fn runtime_observer_clear_resets_learned_state() {
        let mut observer = RuntimeObserver {
            iface: "wlan0".to_string(),
            previous: None,
            baseline: BaselineEstimator {
                rtt_ms: Some(25.0),
                samples: 4,
            },
            hysteresis: HysteresisClassifier {
                current: PathState::Stable,
                ..HysteresisClassifier::default()
            },
        };
        observer.clear();
        assert!(observer.iface.is_empty());
        assert!(observer.previous.is_none());
        assert_eq!(observer.baseline.rtt_ms, None);
        assert_eq!(observer.hysteresis.current(), PathState::Unknown);
    }

    #[test]
    fn baseline_learns_only_from_quiet_samples() {
        let mut estimator = BaselineEstimator::default();

        let mut quiet = sample();
        quiet.rx_mbps = Some(1.0);
        quiet.tx_mbps = Some(0.2);
        estimator.apply(&mut quiet);
        assert_eq!(estimator.rtt_ms, Some(35.0));
        assert_eq!(estimator.samples, 1);

        let mut loaded = sample();
        loaded.avg_rtt_ms = Some(120.0);
        loaded.rx_mbps = Some(80.0);
        estimator.apply(&mut loaded);
        assert_eq!(loaded.baseline_rtt_ms, Some(35.0));
        assert_eq!(estimator.rtt_ms, Some(35.0));
        assert_eq!(estimator.samples, 1);
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
    fn qdisc_evidence_strengthens_bufferbloat_confidence() {
        let mut input = sample();
        input.baseline_rtt_ms = Some(30.0);
        input.avg_rtt_ms = Some(95.0);
        input.rx_mbps = Some(80.0);
        input.qdisc_backlog_bytes = Some(64_000);
        input.qdisc_drop_delta = Some(3);
        let result = classify(input);
        assert_eq!(result.state, PathState::Bufferbloat);
        assert_eq!(result.confidence, 96);
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

    #[test]
    fn compressed_week_survives_repeated_path_transitions_without_flapping() {
        // One loop iteration represents one minute: 10,080 virtual minutes = 7 days.
        // Every two hours the environment changes. Hysteresis must require exactly
        // three confident observations before following the new path state.
        let states = [
            PathState::Stable,
            PathState::Bufferbloat,
            PathState::LossyWireless,
            PathState::HighBdp,
            PathState::ProxyConstrained,
            PathState::Congested,
        ];
        let mut classifier = HysteresisClassifier::default();
        let mut committed_transitions = 0usize;
        let mut previous = classifier.current();

        for virtual_minute in 0..10_080usize {
            let phase = virtual_minute / 120;
            let expected = states[phase % states.len()];
            let observation = Classification {
                state: expected,
                confidence: 90,
                reasons: vec!["compressed-week scenario".to_string()],
                sample: sample(),
            };
            let before = classifier.current();
            let after = classifier.update(&observation);
            if after != previous {
                committed_transitions += 1;
                previous = after;
            }

            let minute_in_phase = virtual_minute % 120;
            if minute_in_phase < 2 && before != expected {
                assert_eq!(after, before, "state flipped before hysteresis completed");
            }
            if minute_in_phase >= 2 {
                assert_eq!(after, expected, "state failed to converge within three samples");
            }
        }

        assert!(committed_transitions >= 80);
        assert!(committed_transitions <= 84);
    }

    #[test]
    fn compressed_week_baseline_is_bounded_under_long_mixed_load() {
        let mut estimator = BaselineEstimator::default();

        for virtual_minute in 0..10_080usize {
            let mut input = sample();
            if virtual_minute % 10 == 0 {
                // Periodic heavy traffic must not poison the quiet RTT baseline.
                input.avg_rtt_ms = Some(280.0);
                input.rx_mbps = Some(120.0);
                input.tx_mbps = Some(18.0);
                input.retrans_ratio = Some(0.025);
                input.qdisc_backlog_bytes = Some(96_000);
                input.qdisc_drop_delta = Some(6);
            } else {
                input.avg_rtt_ms = Some(25.0 + (virtual_minute % 7) as f64);
                input.rx_mbps = Some(1.5);
                input.tx_mbps = Some(0.2);
                input.retrans_ratio = Some(0.001);
                input.qdisc_backlog_bytes = Some(0);
                input.qdisc_drop_delta = Some(0);
            }
            estimator.apply(&mut input);
        }

        assert_eq!(estimator.rtt_ms, Some(25.0));
        assert_eq!(estimator.samples, u8::MAX, "sample counter must saturate, not wrap");
    }
}
