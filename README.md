# TCP Optimiser

> Magisk / KernelSU module — dynamic TCP congestion control with Material Design 3 WebUI

[![Version](https://img.shields.io/badge/version-2.5-blue)](https://github.com/Zhanfg/TCP_Optimiser/releases)
[![License](https://img.shields.io/badge/license-GPL--3.0-green)](LICENSE)

TCP Optimiser dynamically switches TCP congestion control algorithms based on your active network interface (Wi‑Fi vs Cellular), with per‑algorithm qdisc tuning, pacing scaling, and initcwnd/initrwnd optimisation.

---

## Features

### Core Engine
- **Dynamic interface detection** — switches between Wi‑Fi and Cellular automatically
- **18 algorithms** — BBR / BBR2 / BBR3 / CUBIC / Westwood / Reno / HTCP / Vegas / YeAH / Illinois / DCTCP / CDG / BIC / HighSpeed / Hybla / NV / Scalable / LP
- **Per‑algorithm tuning** — each algorithm has a bespoke qdisc + pacing_ca + pacing_ss configuration
- **VoWiFi‑aware** — waits for Wi‑Fi Calling detection before applying Wi‑Fi settings
- **Adaptive polling** — 2 s after interface change, 5 s when stable
- **Frequency‑sensing pacing** — scales TCP pacing by Wi‑Fi band (2.4 / 5 / 6 GHz)

### WebUI (KernelSU)
- **Tabbed interface** — Home · Stats · Settings · Logs · Advanced
- **Real‑time SVG charts** — throughput (↓↑), retransmission rate, congestion window, RTT
- **Proxy detection** — system VPN vs transparent proxy (Clash / V2Ray / sing‑box / Surfing / Shadowsocks)
- **Hosts status** — Systemless / AdAway / BirdHost / blocked‑count
- **DNS servers** — per‑interface DNS display
- **Long‑press cards** — touch & hold any dashboard card for detailed explanation
- **Overscroll About** — pull down at the bottom of Home to open the About dialog
- **Predictive back gesture** — history.pushState support for Android 14+ back‑to‑previous‑page animation
- **Floating pill nav bar** — icon‑only, auto‑hides on scroll, safe‑area aware
- **Responsive layout** — 5 breakpoints from ldpi (320 px) to desktop

### Settings & Presets
- **Wi‑Fi / Cellular algorithm selectors** — only kernel‑supported algorithms shown
- **Toggle switches** — kill connections on change, set max initcwnd/initrwnd
- **Global qdisc selector** — fq / fq_codel / cake / pfifo_fast / codel / fq_pie / pfifo
- **5 built‑in presets** — Balanced · Gaming · Streaming · Battery Saver · High‑Speed — one‑tap apply
- **Custom preset import/export** — JSON format, paste to share

### Advanced (hidden by default, 5 s warning countdown)
- **10 kernel parameters** — keepalive (time/intvl/probes), busy_poll/read, somaxconn, netdev_max_backlog, MTU probing, slow‑start after idle, TCP Fast Open, conntrack_max
- **Baseband backup** — detect modem partitions, select & dump with `dd`, auto‑generated restore.sh script
- **Stealth mode** — rewrite module.prop to generic "Android System Component", suppress logs, hide from root detection apps

### Security & Cleanup
- **Uninstall script** — restores cubic/fq_codel, disables TFO/ECN, removes all traces
- **Disk space + write‑permission pre‑install validation**

### i18n
- English / 简体中文 (135 keys, full coverage)

---

## Installation

1. Download `TCP_Optimiser-v*.zip` from [Releases](https://github.com/Zhanfg/TCP_Optimiser/releases)
2. Flash in Magisk Manager or KernelSU Manager
3. Reboot
4. Open the WebUI from the module page (KernelSU) or visit the module directory

---

## Screenshots

| Home | Stats | Settings | Logs |
|------|-------|----------|------|
| *(dashboard with interface / algo / proxy / hosts)* | *(SVG charts + DNS)* | *(theme / algo / presets)* | *(live service log)* |

---

## Presets

| Preset | Wi‑Fi | Cellular | qdisc | Pacing | Use case |
|--------|-------|----------|-------|--------|----------|
| **Balanced** | CUBIC | CUBIC | fq_codel | 150/200 | Daily default |
| **Gaming** | BBR | BBR | fq | 200/300 | Low latency |
| **Streaming** | BBR | CUBIC | fq_codel | 180/250 | High throughput |
| **Battery Saver** | Vegas | Westwood | fq_codel | 120/180 | Power efficient |
| **High‑Speed** | BBR3 | BBR3 | fq | 220/320 | Maximum speed |

---

## Supported Kernels

Any kernel with `/proc/sys/net/ipv4/tcp_available_congestion_control` and `tc` (traffic control) available.  
Tested on: Qualcomm SD 8xx series, MediaTek Dimensity, Pixel Tensor.

---

## Authors

- [fatalcoder524](https://github.com/fatalcoder524) — original author
- [axymorrsen](https://github.com/Zhanfg) — WebUI redesign, stats, presets, stealth mode, advanced kernel tuning, Rust rewrite

---

## License

GPL-3.0 © 2025–2026 fatalcoder524 & axymorrsen

---

## Links

- [GitHub Repository](https://github.com/Zhanfg/TCP_Optimiser)
- [Telegram](https://t.me/TCP_Optimiser)
- [Original Module](https://github.com/fatalcoder524/TCP_Optimiser_Module)
