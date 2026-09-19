#!/usr/bin/env python3
"""Audit a built kernel module against the target kernel's Module.symvers.

This complements (not replaces) the Android GKI KMI symbol-list audit:
- Module.symvers answers: "does this exact target build export every symbol?"
- audit-gki-symbols.py answers: "is every imported symbol part of the GKI KMI?"

Using Module.symvers avoids false negatives from grepping EXPORT_SYMBOL source
macros, which can be generated, namespaced, or parsed incorrectly.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path


IGNORED_UNDEFINED = {
    "_GLOBAL_OFFSET_TABLE_",
    "__this_module",
}


def find_nm() -> str:
    for candidate in ("llvm-nm", "nm"):
        path = shutil.which(candidate)
        if path:
            return path
    raise SystemExit("error: neither llvm-nm nor nm is available")


def exported_symbols(symvers: Path) -> set[str]:
    if not symvers.is_file():
        raise SystemExit(f"error: Module.symvers not found: {symvers}")

    symbols: set[str] = set()
    for line in symvers.read_text(errors="replace").splitlines():
        fields = line.split()
        if len(fields) >= 2:
            symbols.add(fields[1])
    if not symbols:
        raise SystemExit(f"error: no exported symbols parsed from {symvers}")
    return symbols


def undefined_symbols(nm: str, module: Path) -> set[str]:
    result = subprocess.run(
        [nm, "-u", str(module)],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    symbols: set[str] = set()
    for line in result.stdout.splitlines():
        fields = line.split()
        if not fields:
            continue
        symbol = fields[-1]
        if symbol not in IGNORED_UNDEFINED:
            symbols.add(symbol)
    return symbols


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("kernel_tree", type=Path)
    parser.add_argument("modules", nargs="+", type=Path)
    parser.add_argument("--json", dest="json_path", type=Path)
    args = parser.parse_args()

    symvers = args.kernel_tree / "Module.symvers"
    exports = exported_symbols(symvers)
    nm = find_nm()

    report = {
        "schema": 1,
        "kernel_tree": str(args.kernel_tree),
        "module_symvers": str(symvers),
        "modules": [],
    }
    failed = False

    for module in args.modules:
        if not module.is_file():
            raise SystemExit(f"error: module not found: {module}")
        imports = undefined_symbols(nm, module)
        missing = sorted(imports - exports)
        report["modules"].append(
            {
                "file": str(module),
                "undefined_symbols": sorted(imports),
                "missing_exports": missing,
            }
        )
        if missing:
            failed = True
            print(f"FAIL: {module}: symbols absent from Module.symvers:", file=sys.stderr)
            for symbol in missing:
                print(symbol, file=sys.stderr)
        else:
            print(
                f"OK: {module}: all {len(imports)} undefined symbols are exported "
                "by the target kernel build"
            )

    if args.json_path:
        args.json_path.parent.mkdir(parents=True, exist_ok=True)
        args.json_path.write_text(json.dumps(report, indent=2) + "\n")

    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
