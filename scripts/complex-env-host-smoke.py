#!/usr/bin/env python3
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import resource
import subprocess
import tempfile
import time


def write_executable(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(0o755)


def run(binary: Path, args, env, timeout=8, expect_success=True):
    started = time.monotonic()
    proc = subprocess.run(
        [str(binary), *args],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=timeout,
    )
    elapsed = time.monotonic() - started
    if expect_success and proc.returncode != 0:
        raise AssertionError(
            f"{args} failed rc={proc.returncode}: {proc.stderr.strip()}"
        )
    if not expect_success and proc.returncode == 0:
        raise AssertionError(f"{args} unexpectedly succeeded")
    return proc, elapsed


def run_json(binary: Path, args, env, timeout=8):
    proc, elapsed = run(binary, args, env, timeout=timeout)
    try:
        payload = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise AssertionError(f"{args} returned invalid JSON: {proc.stdout!r}") from exc
    if not isinstance(payload, dict):
        raise AssertionError(f"{args} returned non-object JSON")
    return payload, elapsed


def source_audit(root: Path):
    daemon = (root / "src/daemon.rs").read_text()
    install = (root / "src/install.rs").read_text()
    kernel = (root / "src/kernel_module.rs").read_text()
    adaptive = (root / "src/adaptive.rs").read_text()
    assertions = {
        "qdisc_on_demand": daemon.count("ensure_qdisc_for_policy(&policy.qdisc)") >= 2,
        "no_eager_qdisc_preflight": "preflight_bundled_qdiscs" not in install,
        "proxy_connection_protection": "proxy_state.transparent && !force_proxy_kill" in daemon,
        "exact_release_is_strict": "Some(expected) => expected == release" in kernel,
        "adaptive_state_has_staleness_bound": "RUNTIME_STATE_MAX_AGE_SECONDS: u64 = 120" in adaptive,
        "no_forced_insmod": "insmod -f" not in root.read_text() if root.is_file() else True,
    }
    # Cross-file check for forbidden force-load text.
    forced = []
    detector = Path(__file__).resolve()
    for path in root.rglob("*"):
        if (
            path.is_file()
            and path.resolve() != detector
            and path.suffix in {".rs", ".sh", ".py", ".yml", ".yaml"}
        ):
            try:
                if "insmod -f" in path.read_text(errors="ignore"):
                    forced.append(str(path.relative_to(root)))
            except OSError:
                pass
    assertions["no_forced_insmod"] = not forced
    failed = [name for name, passed in assertions.items() if not passed]
    if failed:
        raise AssertionError(f"source invariant failure: {failed}; forced={forced}")
    return assertions


def build_mock_android(mockbin: Path):
    write_executable(
        mockbin / "iptables-save",
        """#!/bin/sh
if [ "$SIM_PROXY" = "tproxy" ]; then
  echo '-A PREROUTING -p tcp -j TPROXY --on-port 7893 --tproxy-mark 0x1/0x1'
fi
exit 0
""",
    )
    write_executable(mockbin / "ip6tables-save", "#!/bin/sh\nexit 0\n")
    write_executable(
        mockbin / "ps",
        """#!/bin/sh
case "$SIM_PROXY" in
  tproxy) echo 'root 123 1 0 00:00 ? 00:00:00 mihomo -d /data/adb/mihomo' ;;
  process) echo 'root 321 1 0 00:00 ? 00:00:00 sing-box run -c /data/adb/sing-box/config.json' ;;
  *) echo 'root 1 0 0 00:00 ? 00:00:00 init' ;;
esac
""",
    )
    write_executable(
        mockbin / "getprop",
        """#!/bin/sh
cat <<'EOF'
[net.dns1]: [1.1.1.1]
[net.dns2]: [2606:4700:4700::1111]
[ro.product.cpu.abi]: [arm64-v8a]
EOF
""",
    )
    write_executable(
        mockbin / "ss",
        """#!/bin/sh
if [ "$1" = "-K" ]; then exit 0; fi
cat <<'EOF'
ESTAB 0 0 127.0.0.1:40000 127.0.0.1:443 cubic wscale:7,7 rto:204 rtt:31.5/3.1 ato:40 mss:1448 cwnd:18
EOF
""",
    )
    write_executable(
        mockbin / "tc",
        """#!/bin/sh
case "$*" in
  *"-s qdisc show"*)
    cat <<'EOF'
qdisc fq_codel 0: dev lo root refcnt 2 limit 10240p
 Sent 4096 bytes 32 pkt (dropped 0, overlimits 0 requeues 0)
 backlog 0b 0p requeues 0
EOF
    ;;
  *"qdisc show"*) echo 'qdisc fq_codel 0: dev lo root refcnt 2 limit 10240p' ;;
esac
exit 0
""",
    )
    write_executable(
        mockbin / "ip",
        """#!/bin/sh
case "$*" in
  "route get 1.1.1.1"*) echo '1.1.1.1 dev lo src 127.0.0.1' ;;
  "route show"*) echo 'default dev lo metric 100' ;;
  "link show dev lo"*) echo '1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 state UNKNOWN' ;;
esac
exit 0
""",
    )
    write_executable(mockbin / "iw", "#!/bin/sh\nexit 1\n")
    write_executable(mockbin / "dumpsys", "#!/bin/sh\nexit 0\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--iterations", type=int, default=180)
    parser.add_argument("--source-root", default=".")
    args = parser.parse_args()

    binary = Path(args.binary).resolve()
    report_path = Path(args.report)
    report = {"passed": False, "checks": {}, "metrics": {}}

    try:
        report["checks"]["source_invariants"] = source_audit(Path(args.source_root).resolve())

        with tempfile.TemporaryDirectory(prefix="tcp-opt-complex-") as temp:
            root = Path(temp)
            module = root / "module"
            mockbin = root / "mockbin"
            module.mkdir()
            mockbin.mkdir()
            (module / "module.prop").write_text(
                "id=tcp_optimiser\nname=TCP Optimiser\nversion=3.0.0\nversionCode=300\ndescription=test\n"
            )
            (module / "wlan_cubic").touch()
            (module / "rmnet_data_cubic").touch()
            build_mock_android(mockbin)

            env = os.environ.copy()
            env["TCP_OPTIMISER_MODULE_DIR"] = str(module)
            env["PATH"] = str(mockbin) + os.pathsep + env.get("PATH", "")
            env["SIM_PROXY"] = "direct"

            build, _ = run_json(binary, ["build-info"], env)
            assert build.get("version"), build
            report["checks"]["build_info"] = build

            direct, _ = run_json(binary, ["proxy"], env)
            assert direct.get("transparent") is False, direct
            env_tproxy = env.copy()
            env_tproxy["SIM_PROXY"] = "tproxy"
            tproxy, _ = run_json(binary, ["proxy"], env_tproxy)
            assert tproxy.get("transparent") is True, tproxy
            assert tproxy.get("tproxy") is True, tproxy
            report["checks"]["proxy_modes"] = {
                "direct": direct.get("mode"),
                "tproxy": tproxy.get("mode"),
                "family": tproxy.get("family"),
            }

            sample, _ = run_json(binary, ["sample", "--iface", "lo", "--details"], env_tproxy)
            assert sample.get("active_iface") == "lo", sample
            assert isinstance(sample.get("dns"), list), sample
            assert sample.get("conn_info") is not None, sample
            report["checks"]["stats_sample"] = {
                "dns": len(sample.get("dns") or []),
                "conn_samples": (sample.get("conn_info") or {}).get("samples"),
            }

            status_auto, _ = run_json(binary, ["status", "--iface", "lo", "--runtime-only"], env)
            assert status_auto.get("qdisc_policy") == "per_algorithm", status_auto
            assert status_auto.get("auto_tuning_enabled") is True, status_auto

            (module / "qdisc").write_text("fq_codel\n")
            (module / "disable_auto_tuning").touch()
            status_manual, _ = run_json(binary, ["status", "--iface", "lo", "--runtime-only"], env)
            assert status_manual.get("qdisc_policy") == "manual", status_manual
            assert status_manual.get("auto_tuning_enabled") is False, status_manual
            (module / "qdisc").unlink()
            (module / "disable_auto_tuning").unlink()
            report["checks"]["config_churn"] = {
                "auto_qdisc": status_auto.get("qdisc_policy"),
                "manual_qdisc": status_manual.get("qdisc_policy"),
                "auto_tuning_on": status_auto.get("auto_tuning_enabled"),
                "auto_tuning_off": status_manual.get("auto_tuning_enabled"),
            }

            profile_direct, _ = run_json(binary, ["profile", "--refresh"], env)
            profile_proxy, _ = run_json(binary, ["profile", "--refresh"], env_tproxy)
            assert profile_direct.get("schema") == 1, profile_direct
            assert profile_proxy.get("schema") == 1, profile_proxy
            assert (profile_proxy.get("recommendations") or {}).get("nf_conntrack_max") is not None
            report["checks"]["profile_proxy_awareness"] = {
                "direct_conntrack": (profile_direct.get("recommendations") or {}).get("nf_conntrack_max"),
                "tproxy_conntrack": (profile_proxy.get("recommendations") or {}).get("nf_conntrack_max"),
            }

            adaptive, adaptive_elapsed = run_json(
                binary,
                ["adaptive", "--iface", "lo", "--sample-ms", "250", "--samples", "2"],
                env_tproxy,
                timeout=5,
            )
            assert len(adaptive.get("observations") or []) == 2, adaptive
            report["checks"]["adaptive_active_probe"] = {
                "stable_state": adaptive.get("stable_state"),
                "observations": len(adaptive.get("observations") or []),
            }
            report["metrics"]["adaptive_probe_seconds"] = round(adaptive_elapsed, 3)

            repair, _ = run_json(binary, ["repair", "--iface", "lo"], env_tproxy)
            assert isinstance(repair.get("success"), bool), repair
            assert isinstance(repair.get("errors"), list), repair
            report["checks"]["repair_degrades_safely"] = {
                "success": repair.get("success"),
                "errors": len(repair.get("errors") or []),
            }

            unsigned = root / "unsigned"
            unsigned.mkdir()
            (unsigned / "module.prop").write_text("id=tcp_optimiser\n")
            run(binary, ["verify-module", str(unsigned)], env, expect_success=False)
            report["checks"]["unsigned_payload_rejected"] = True

            commands = [
                ["status", "--iface", "lo", "--runtime-only"],
                ["sample", "--iface", "lo"],
                ["proxy"],
                ["build-info"],
            ]
            started = time.monotonic()

            def worker(index):
                local_env = env_tproxy if index % 3 == 0 else env
                payload, elapsed = run_json(binary, commands[index % len(commands)], local_env, timeout=5)
                return elapsed, payload

            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                futures = [pool.submit(worker, i) for i in range(args.iterations)]
                results = [future.result() for future in futures]

            elapsed = time.monotonic() - started
            max_command = max(item[0] for item in results)
            rss_mib = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss / 1024.0
            assert max_command < 5.0, max_command
            assert elapsed < 60.0, elapsed
            assert rss_mib < 512.0, rss_mib
            report["checks"]["concurrent_runtime_stress"] = {
                "iterations": args.iterations,
                "workers": 8,
            }
            report["metrics"].update({
                "stress_seconds": round(elapsed, 3),
                "slowest_command_seconds": round(max_command, 3),
                "max_child_rss_mib": round(rss_mib, 2),
            })

        report["passed"] = True
    except Exception as exc:
        report["error"] = f"{type(exc).__name__}: {exc}"
        raise
    finally:
        report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
        print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
