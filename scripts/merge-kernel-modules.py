#!/usr/bin/env python3
import argparse
import hashlib
import json
import shutil
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def safe_relative(value: str) -> Path:
    path = Path(value)
    if not value or path.is_absolute() or ".." in path.parts:
        raise ValueError(f"unsafe module path: {value!r}")
    return path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("input_root", type=Path)
    parser.add_argument("output_root", type=Path)
    args = parser.parse_args()

    manifests = sorted(args.input_root.glob("**/manifest.json"))
    if not manifests:
        raise SystemExit("no kernel-module manifests found")

    args.output_root.mkdir(parents=True, exist_ok=True)
    modules = []
    builds = []
    seen = set()

    for manifest_path in manifests:
        source_root = manifest_path.parent
        data = json.loads(manifest_path.read_text())
        if data.get("schema") != 1:
            raise SystemExit(f"unsupported manifest schema: {manifest_path}")

        builds.append({
            key: data[key]
            for key in (
                "kmi",
                "kernel_release",
                "kernel_source",
                "bbr3_source",
                "builtin_capabilities",
                "unavailable_capabilities",
            )
            if key in data
        })

        for entry in data.get("modules", []):
            relative = safe_relative(entry["file"])
            source = source_root / relative
            if not source.is_file():
                raise SystemExit(f"missing module referenced by {manifest_path}: {relative}")
            actual = sha256(source)
            if actual.lower() != entry["sha256"].lower():
                raise SystemExit(f"source module hash mismatch: {source}")

            identity = (entry["name"], entry["kmi"], entry["arch"])
            if identity in seen:
                raise SystemExit(f"duplicate kernel module identity: {identity}")
            seen.add(identity)

            destination = args.output_root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)

            copied = dict(entry)
            copied["sha256"] = actual
            modules.append(copied)

    modules.sort(key=lambda item: (item["kmi"], item["arch"], item["name"]))
    builds.sort(key=lambda item: item.get("kmi", ""))

    merged = {
        "schema": 1,
        "builds": builds,
        "modules": modules,
    }
    (args.output_root / "manifest.json").write_text(
        json.dumps(merged, indent=2, sort_keys=True) + "\n"
    )
    print(f"merged {len(modules)} modules from {len(manifests)} builds")


if __name__ == "__main__":
    main()
