#!/usr/bin/env python3
"""Check two original EC1 archive byte identities, without extracting or writing."""
import argparse
import hashlib
from pathlib import Path

ISSUED = (
    ("EC1-workshop.zip", 52276, "aab635a36c90d513dc082e7c94c4c5bc918d9004638e407616efbcfae381a305"),
    ("EC1-fabrication.zip", 7784, "b2d536954a968bf86c33b6eecd6fa2f8eb4a7cfdea90b7fa45d7336849a8b8be"),
)

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("directory", nargs="?", type=Path, default=Path(__file__).resolve().parent / "issued", help="directory containing both original ZIPs (default: bundled issued/)")
    root = ap.parse_args().directory
    ok = True
    for name, size, expected in ISSUED:
        path = root / name
        try:
            if not path.is_file():
                raise OSError("missing or not a regular file")
            n = 0
            h = hashlib.sha256()
            with path.open("rb") as src:
                for chunk in iter(lambda: src.read(65536), b""):
                    n += len(chunk)
                    h.update(chunk)
            match = n == size and h.hexdigest() == expected
            print(("BYTE MATCH" if match else "HOLD: MISMATCH"), name, n, "bytes; SHA-256", h.hexdigest())
            ok = ok and match
        except OSError as e:
            print("HOLD:", name, str(e))
            ok = False
    print("Byte test only. Check source issue and original writing evidence separately (ARCHIVES.md).")
    return 0 if ok else 1

if __name__ == "__main__":
    raise SystemExit(main())
