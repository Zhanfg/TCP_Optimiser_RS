# Adaptive Network Engine Roadmap

Branch: `future/adaptive-network-engine`

This branch explores the next major evolution of TCP Optimiser after the GKI KO build pipeline. The goal is not to add algorithms for the sake of feature count. The goal is a capability-gated, closed-loop network controller that can adapt safely to real path conditions while preserving existing connections.

## Design principles

1. Never bypass GKI/KMI/module ABI validation.
2. Never require connection killing for normal policy changes.
3. Every active tuning action must have readback verification and a safe fallback.
4. Prefer passive telemetry. Active probes are optional and bounded.
5. Avoid oscillation with EWMA, hysteresis, hold-down timers and minimum dwell times.
6. Proxy/VPN/TUN/TPROXY awareness is mandatory; do not treat virtual interfaces as physical uplinks.
7. Unsupported kernel features degrade to observation-only instead of failing installation.
8. Do not store plaintext SSID/BSSID/network identifiers by default.
9. Keep battery overhead low: netlink/event-driven wakeups first, slow polling as fallback.
10. Do not market QUIC/UDP as using TCP congestion control; optimize only the parts the kernel can actually influence.

## Phase A — closed-loop path classifier

Add a lightweight runtime model fed by data already available in the project:

- RTT and cwnd from `ss -tino`
- retransmission deltas from `/proc/net/snmp`
- interface RX/TX deltas from `/proc/net/dev`
- socket pressure from `/proc/net/sockstat`
- qdisc state from `tc -s qdisc`
- Wi-Fi band/frequency and physical interface type
- proxy mode: none / process / TUN / TPROXY / mixed

Derived rolling signals:

- smoothed RTT and RTT inflation
- retransmission ratio
- throughput trend
- qdisc backlog/drop trend when available
- connection count / socket pressure
- link transition age

Initial path states:

- `Stable`
- `LatencySensitive`
- `Bufferbloat`
- `LossyWireless`
- `HighBdp`
- `Congested`
- `ProxyConstrained`
- `Unknown`

State transitions must use hysteresis and minimum dwell time. A single bad sample must never cause a policy switch.

## Phase B — adaptive policy planner

Separate **classification** from **actuation**.

The planner should produce a proposed policy:

- congestion-control algorithm
- qdisc
- pacing values
- optional ECN behavior where supported
- optional buffer/backlog adjustments
- reason and confidence
- rollback target

Default behavior should remain conservative:

- preserve the current policy if confidence is low
- prefer changing qdisc/pacing before changing congestion control
- never switch repeatedly during an active transient
- preserve existing TCP sessions
- verify every write and revert failed changes

Add three modes:

- `off`: existing static behavior
- `observe`: classify and show recommendations, never write
- `adaptive`: apply verified policy changes

`observe` should be the initial default for development builds.

## Phase C — bufferbloat guard

Add optional capacity-aware qdisc tuning.

Candidate behavior:

- detect persistent RTT inflation under sustained TX load
- estimate a conservative egress capacity from passive samples
- if CAKE is available, optionally set a bounded bandwidth ceiling
- otherwise tune FQ-CoDel/FQ within supported portable parameters
- automatically relax shaping when the bottleneck disappears

Do not guess a permanent bandwidth from one short burst.

Ingress shaping through IFB may be explored later, but must be separately capability-gated because Android kernels and vendor networking stacks vary widely.

## Phase D — proxy-aware physical path model

Current proxy detection should become part of the controller instead of only UI/profile context.

Goals:

- maintain a physical-uplink view even when the default route points to TUN
- detect double-queue situations
- distinguish TUN, TPROXY, mixed and process-only proxy paths
- detect MTU/MSS risk around VPN/proxy paths
- avoid rewriting proxy policy routes, fwmarks, nftables/iptables rules
- preserve established connections unless the user explicitly requests disruption

Add diagnostics for:

- physical MTU vs virtual MTU
- suspected PMTU black-hole conditions
- route-table / policy-routing inconsistencies
- proxy interface changes

## Phase E — per-network learned profiles

Persist bounded historical observations so the engine does not relearn every network from zero.

Network identity must be privacy-preserving:

- hash any stable network identifier before storing it
- do not store plaintext SSID/BSSID by default
- cellular profiles should avoid storing subscriber identifiers

Persist only compact metrics such as:

- typical RTT range
- typical throughput envelope
- loss/retransmission baseline
- previously stable policy
- confidence and last-seen time

Use decay/expiry so stale profiles naturally disappear.

## Phase F — UDP / QUIC visibility

Modern Android traffic is often QUIC, so the module needs visibility even though QUIC congestion control lives in user space.

Useful work:

- UDP socket counts and pressure
- RX/TX/drop counters where kernel interfaces expose them
- qdisc behavior for UDP-heavy workloads
- path MTU diagnostics
- buffer sizing within kernel-supported limits

Non-goal:

- pretending BBR/CUBIC sysctls control QUIC congestion control.

## Phase G — optional traffic classes

Explore an opt-in QoS layer for latency-sensitive and background traffic.

Possible backends, in order of compatibility:

1. existing skb marks / DSCP when already present
2. Android UID-aware firewall classification
3. cgroup/eBPF backend only on devices with proven support

Potential classes:

- interactive / gaming
- voice / VoIP
- normal
- streaming
- background bulk

This must be disabled by default because packet marking can conflict with VPN/proxy/firewall setups.

## Phase H — power and thermal awareness

Add a low-overhead guard that can reduce tuning activity when:

- screen is off
- Android power saver is enabled
- device is thermally constrained

This should change controller cadence and aggressiveness, not silently cripple throughput.

## Phase I — flight recorder and diagnostic export

Create a compact rolling event log containing:

- route/interface transitions
- classifier state transitions
- policy decisions and reasons
- failed writes/readback mismatches
- qdisc repairs
- proxy-mode changes
- KMI/module-load outcomes

Provide one-command/WebUI export of a sanitized diagnostic bundle for bug reports.

## Optional research

These are useful but should not block the core engine:

- MPTCP capability detection and observation
- dual-uplink awareness when Wi-Fi and cellular coexist
- optional IFB ingress shaping
- BPF-based telemetry on kernels with BTF and required hooks
- per-flow diagnostics without packet payload capture

## Explicit non-goals

- unsafe `insmod -f` / modversion bypasses
- global packet interception just for statistics
- DNS hijacking
- hard-coded public benchmark servers
- aggressive connection resets
- writing unsupported sysctls
- storing packet payloads
- storing plaintext network identifiers by default
- adding congestion algorithms only to increase the advertised count

## Implementation order

1. telemetry sampler + deterministic state classifier
2. observe-only WebUI exposure
3. policy planner with dry-run output
4. verified adaptive actuation + rollback
5. bufferbloat guard
6. proxy/MTU diagnostics
7. learned profiles
8. UDP/QUIC visibility
9. optional QoS and BPF backends

## Validation

Each state transition and policy decision must be unit-testable from synthetic telemetry fixtures.

CI should add:

- classifier fixture tests
- oscillation/hysteresis tests
- fallback tests when metrics are missing
- proxy-mode matrix tests
- Android 12/5.10, 13/5.15, 14/6.1 and 15/6.6 capability smoke
- no-session-kill regression checks

Real-device validation should compare the adaptive engine against the current static policy using RTT under load, retransmission rate, throughput stability, battery overhead and policy-switch count.
