#!/usr/bin/env python3
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess


SYMBOL = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def load_kmi_symbols(kernel: Path) -> set[str]:
    symbols: set[str] = set()
    android = kernel / "android"
    for path in sorted(android.glob("abi_gki_aarch64*")):
        if not path.is_file() or path.suffix in {".xml", ".stg"}:
            continue
        try:
            for raw in path.read_text(errors="ignore").splitlines():
                line = raw.strip()
                if not line or line.startswith("#") or line.startswith("["):
                    continue
                if SYMBOL.fullmatch(line):
                    symbols.add(line)
        except OSError:
            continue
    if not symbols:
        raise SystemExit("no arm64 GKI KMI symbol lists found")
    return symbols


def undefined_symbols(ko: Path) -> set[str]:
    nm = shutil.which("llvm-nm") or shutil.which("nm")
    if not nm:
        raise SystemExit("llvm-nm/nm not found")
    proc = subprocess.run(
        [nm, "-u", str(ko)],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    result: set[str] = set()
    for raw in proc.stdout.splitlines():
        fields = raw.split()
        if not fields:
            continue
        symbol = fields[-1]
        if SYMBOL.fullmatch(symbol):
            result.add(symbol)
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("kernel", type=Path)
    parser.add_argument("module_dir", type=Path)
    parser.add_argument("--json", dest="json_path", type=Path)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()

    allowed = load_kmi_symbols(args.kernel)
    results = []
    failed = False

    for ko in sorted(args.module_dir.glob("*.ko")):
        imported = undefined_symbols(ko)
        missing = sorted(imported - allowed)
        compatible = not missing
        failed |= not compatible
        results.append({
            "file": ko.name,
            "compatible": compatible,
            "undefined_symbols": len(imported),
            "non_kmi_symbols": missing,
        })
        state = "OK" if compatible else "FAIL"
        print(f"{state}: {ko.name}: {len(imported)} imports, {len(missing)} outside GKI KMI")
        for symbol in missing:
            print(f"  {symbol}")

    if not results:
        raise SystemExit("no .ko files found for GKI KMI audit")

    report = {
        "schema": 1,
        "kmi_symbol_count": len(allowed),
        "modules": results,
    }
    if args.json_path:
        args.json_path.parent.mkdir(parents=True, exist_ok=True)
        args.json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

    if args.strict and failed:
        raise SystemExit("one or more modules depend on symbols outside the Android GKI KMI")


if __name__ == "__main__":
    main()
