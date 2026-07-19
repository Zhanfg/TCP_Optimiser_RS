# TCP Optimiser

<p align="right"><strong>简体中文</strong> · <a href="./README_EN.md">English</a></p>

> 一款面向 Magisk / KernelSU 的动态 TCP 拥塞控制模块，配有 Material Design 3 WebUI。

[![版本](https://img.shields.io/badge/version-3.0.0-blue)](https://github.com/Zhanfg/TCP_Optimiser_RS/releases)
[![许可证](https://img.shields.io/badge/license-GPL--3.0-green)](LICENSE)
[![构建状态](https://github.com/Zhanfg/TCP_Optimiser_RS/actions/workflows/build.yml/badge.svg)](https://github.com/Zhanfg/TCP_Optimiser_RS/actions/workflows/build.yml)

TCP Optimiser 会识别当前使用的 Wi-Fi 或蜂窝网络接口，并依据内核实际能力应用 TCP 拥塞控制、队列调度和 pacing 参数。WebUI 用于展示模块真实运行状态、当前网络路径、代理核心、内核能力及实时网络统计。

## 界面预览

<table>
  <tr>
    <td align="center"><strong>首页</strong><br><img src="./docs/screenshots/home.png" width="280" alt="首页运行状态"></td>
    <td align="center"><strong>统计</strong><br><img src="./docs/screenshots/stats.png" width="280" alt="实时网络统计"></td>
  </tr>
  <tr>
    <td align="center"><strong>设置</strong><br><img src="./docs/screenshots/settings.png" width="280" alt="TCP 策略设置"></td>
    <td align="center"><strong>日志</strong><br><img src="./docs/screenshots/logs.png" width="280" alt="模块日志"></td>
  </tr>
</table>

## 主要功能

### Rust 核心

- 自动识别 Wi-Fi 与蜂窝网络接口并切换对应策略。
- 支持 19 种拥塞控制算法：BBR、BBR2、BBR3、CUBIC、Westwood、Westwood+、Reno、HTCP、Vegas、YeAH、Illinois、DCTCP、CDG、BIC、HighSpeed、Hybla、NV、Scalable 和 LP。
- 按算法配置 qdisc、`pacing_ca`、`pacing_ss`、`initcwnd` 与 `initrwnd`。
- 在应用 Wi-Fi 策略前检测 VoWiFi 状态。
- 接口变化后提高检测频率，网络稳定后降低轮询频率。
- 根据 2.4、5、6 GHz Wi-Fi 频段调整 pacing。

### WebUI

- 首页、统计、设置、日志四个主要页面。
- 实时显示吞吐量、重传率、拥塞窗口和 RTT 图表。
- 检测代理应用名称、包名、核心名称与版本，并分别展示 VPN、TPROXY 等依据。
- 识别系统 Hosts 及常见 systemless Hosts 模块。
- 展示每个网络接口的 DNS 和当前路由详情。
- Material Design 3 卡片、可折叠分区、页面过渡动画和可选震动反馈。
- 胶囊式悬浮导航：未选中项仅显示图标，当前项同时显示名称。
- 支持浅色、深色、跟随系统主题及可关闭的 Android 动态取色。
- 从 320 px Android WebUI 到桌面浏览器的响应式布局。

### 策略与高级控制

- 完整显示已知算法和 qdisc，并按照当前内核检测结果标记为**支持**、**不支持**或**未验证**。
- 支持 `fq`、`fq_codel`、`cake`、`pfifo_fast`、`codel`、`fq_pie`、`pfifo` 七种队列调度器。
- 提供五组内置预设，并支持通过 JSON 导入、导出自定义预设。
- 提供 29 项运行时检测的高级内核参数，覆盖连接生命周期、缓冲区、队列、丢包恢复、低延迟轮询和 conntrack。
- 支持基带分区识别与备份，并自动生成恢复脚本。
- 简体中文与英文双语界面，共 241 项经过一致性校验的翻译文本。

## 安装

1. 从 [Releases](https://github.com/Zhanfg/TCP_Optimiser_RS/releases) 下载 `TCP_Optimiser_RS-v*.zip`。
2. 在 Magisk Manager 或 KernelSU Manager 中刷入模块。
3. 重启设备。
4. 从受支持的 Root 管理器中打开模块 WebUI。

内核需要提供 `/proc/sys/net/ipv4/tcp_available_congestion_control` 以及所需的流量控制能力。发行包包含 `arm64-v8a`、`armeabi-v7a` 和 `x86_64` 三种架构；最终可用的算法和 qdisc 仍以设备内核实际支持情况为准。

## 内置预设

| 预设 | Wi-Fi | 蜂窝网络 | qdisc | Pacing | 适用场景 |
|---|---|---|---|---:|---|
| 均衡 | CUBIC | CUBIC | fq_codel | 150/200 | 日常使用 |
| 游戏 | BBR | BBR | fq | 200/300 | 低延迟连接 |
| 流媒体 | BBR | CUBIC | fq_codel | 180/250 | 持续吞吐 |
| 省电 | Vegas | Westwood | fq_codel | 120/180 | 降低后台活动 |
| 高速 | BBR3 | BBR3 | fq | 220/320 | 受支持内核上的最大吞吐 |

## 构建与发行校验

```sh
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
```

每次推送和拉取请求都会验证 Rust、WebUI、模块元数据及 Shell 脚本，并行构建三种 Android ABI。通过验证的非 PR 构建会上传模块 ZIP 和对应 SHA-256 文件；推送与 `v<module.prop 版本>` 完全匹配的标签时，工作流会创建或更新对应的 GitHub Release。

官方 CI 二进制启用 LTO、符号剥离、源码路径重映射，并嵌入仓库与提交水印。可刷入 ZIP 包含 Ed25519 签名的 SHA-256 清单；Rust 安装器会在安装前验证签名及全部受保护文件，任何缺失或被修改的内容都会导致安装终止。公开仓库构建还会生成 GitHub 构建证明。

本地构建保持为未混淆的开发版本，可通过 `tcp_optimiser build-info` 查看构建来源。

## 项目来源

本项目参考了早期 TCP Optimiser 模块所探索的功能思路与使用场景。当前 Rust 核心采用独立架构重新设计和实现，并非对早期 C++/Shell 实现进行逐行翻译或直接移植。本仓库中实际保留的任何第三方代码或资源，仍分别遵循其原有版权与许可证条款。

## 作者

- [fatalcoder524](https://github.com/fatalcoder524)：早期模块与最初的功能构想
- [axymorrsen](https://github.com/Zhanfg)：当前 Rust 实现、WebUI 重构、统计、预设和高级内核调优

## 许可证

GPL-3.0 © 2025–2026 fatalcoder524 & axymorrsen

## 相关链接

- [GitHub 仓库](https://github.com/Zhanfg/TCP_Optimiser_RS)
- [发行版本](https://github.com/Zhanfg/TCP_Optimiser_RS/releases)
- [Telegram](https://t.me/TCP_Optimiser)
- [早期模块](https://github.com/fatalcoder524/TCP_Optimiser_Module)
