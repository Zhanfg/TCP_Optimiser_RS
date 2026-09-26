#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

def parse_symvers(path: Path):
    symbols = {}
    for raw in path.read_text(errors="replace").splitlines():
        fields = raw.split()
        if len(fields) < 2:
            continue
        crc, name = fields[0], fields[1]
        try:
            symbols[name] = int(crc, 16)
        except ValueError:
            continue
    return symbols

def main():
    p=argparse.ArgumentParser()
    p.add_argument("profile", type=Path)
    p.add_argument("symvers", type=Path)
    p.add_argument("--report", type=Path)
    args=p.parse_args()

    profile=json.loads(args.profile.read_text())
    actual=parse_symvers(args.symvers)
    expected={name:int(crc,16) for name,crc in profile["reference_crcs"].items()}

    missing=[]
    mismatched=[]
    matched=[]
    for name,crc in sorted(expected.items()):
        if name not in actual:
            missing.append(name)
        elif actual[name] != crc:
            mismatched.append({
                "symbol":name,
                "device":f"0x{crc:08x}",
                "source":f"0x{actual[name]:08x}",
            })
        else:
            matched.append(name)

    report={
        "schema":1,
        "device":profile["product"],
        "target_kmi":profile["target_kmi"],
        "reference_count":len(expected),
        "matched":len(matched),
        "missing":missing,
        "mismatched":mismatched,
        "compatible":not missing and not mismatched,
    }
    if args.report:
        args.report.write_text(json.dumps(report,indent=2,sort_keys=True)+"\n")
    print(json.dumps(report,indent=2,sort_keys=True))
    if not report["compatible"]:
        raise SystemExit("device ABI fingerprint does not match pinned OnePlus source")

if __name__ == "__main__":
    main()
