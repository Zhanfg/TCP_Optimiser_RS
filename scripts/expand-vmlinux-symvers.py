#!/usr/bin/env python3
import argparse
import re
import shutil
import subprocess
from pathlib import Path

SYMBOL = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def nm_lines(path: Path) -> list[tuple[str, str, str]]:
    nm = shutil.which("llvm-nm") or shutil.which("nm")
    if not nm:
        raise SystemExit("llvm-nm/nm not found")
    proc = subprocess.run(
        [nm, "--defined-only", str(path)],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    rows: list[tuple[str, str, str]] = []
    for raw in proc.stdout.splitlines():
        parts = raw.split()
        if len(parts) < 3:
            continue
        value, kind, name = parts[0], parts[1], parts[-1]
        rows.append((value, kind, name))
    return rows


def undefined_symbols(path: Path) -> set[str]:
    nm = shutil.which("llvm-nm") or shutil.which("nm")
    if not nm:
        raise SystemExit("llvm-nm/nm not found")
    proc = subprocess.run(
        [nm, "-u", str(path)],
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


def parse_symvers(path: Path) -> tuple[list[str], set[str]]:
    lines = []
    symbols = set()
    for raw in path.read_text(errors="replace").splitlines():
        line = raw.rstrip("\n")
        if not line.strip():
            continue
        fields = line.split()
        if len(fields) < 2:
            raise SystemExit(f"invalid symvers line: {line!r}")
        lines.append(line)
        symbols.add(fields[1])
    return lines, symbols


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("vmlinux", type=Path)
    parser.add_argument("base_symvers", type=Path)
    parser.add_argument("module_dir", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    if not args.vmlinux.is_file():
        raise SystemExit(f"missing vmlinux: {args.vmlinux}")
    if not args.base_symvers.is_file():
        raise SystemExit(f"missing base symvers: {args.base_symvers}")

    base_lines, known = parse_symvers(args.base_symvers)
    imports: set[str] = set()
    modules = sorted(args.module_dir.glob("*.ko"))
    if not modules:
        raise SystemExit("no temporary KO files found")
    for module in modules:
        imports.update(undefined_symbols(module))

    missing = sorted(imports - known)
    if not missing:
        args.output.write_text("\n".join(base_lines) + "\n")
        print(f"symvers complete: imports={len(imports)} recovered=0")
        return

    crc_values: dict[str, int] = {}
    namespaced: set[str] = set()
    for value, _kind, name in nm_lines(args.vmlinux):
        if name.startswith("__crc_"):
            symbol = name[len("__crc_"):]
            if SYMBOL.fullmatch(symbol):
                try:
                    crc_values[symbol] = int(value, 16) & 0xFFFFFFFF
                except ValueError:
                    pass
        elif name.startswith("__kstrtabns_"):
            symbol = name[len("__kstrtabns_"):]
            if SYMBOL.fullmatch(symbol):
                namespaced.add(symbol)

    unresolved = [symbol for symbol in missing if symbol not in crc_values]
    if unresolved:
        for symbol in unresolved:
            print(f"missing CRC in official vmlinux: {symbol}")
        raise SystemExit(
            "official vmlinux does not expose CRCs for all required symbols; "
            "these KOs cannot be made exact-release loadable without changing the kernel"
        )

    unsupported_ns = sorted(set(missing) & namespaced)
    if unsupported_ns:
        for symbol in unsupported_ns:
            print(f"namespaced symbol requires explicit namespace recovery: {symbol}")
        raise SystemExit("refusing to guess namespace metadata for recovered symbols")

    recovered = []
    for symbol in missing:
        crc = crc_values[symbol]
        # All bundled networking modules are GPL-compatible. Marking recovered
        # exports GPL is intentionally conservative: it never grants a module
        # broader access than the real kernel export policy.
        recovered.append(
            f"0x{crc:08x}\t{symbol}\tvmlinux\tEXPORT_SYMBOL_GPL\t"
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(base_lines + recovered) + "\n")
    print(
        f"symvers expanded from official vmlinux: imports={len(imports)} "
        f"base={len(known)} recovered={len(recovered)}"
    )


if __name__ == "__main__":
    main()
