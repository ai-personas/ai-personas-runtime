#!/usr/bin/env python3
"""Recheck the original downloads. A reproduced warning is not assembly approval."""
import argparse
from collections import Counter
import csv
import hashlib
import io
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import zipfile

HERE = Path(__file__).resolve().parent


def sha(data):
    return hashlib.sha256(data).hexdigest()


def rows(data):
    return list(csv.DictReader(io.StringIO(data.decode("utf-8-sig"))))


def check(arm, cli, catalog):
    folder = HERE / arm
    report = json.loads((folder / "receiving.json").read_text())
    archive = folder / report["original_download"]
    actual = archive.read_bytes()
    assert len(actual) == report["bytes"] and sha(actual) == report["sha256"], archive
    prefix = "bench-inverter-v5/" if arm == "frozen" else ""
    with zipfile.ZipFile(io.BytesIO(actual)) as z, tempfile.TemporaryDirectory() as tmp:
        assert z.testzip() is None and len(z.namelist()) == len(set(z.namelist())) == report["members"]
        if prefix:
            manifest = prefix + "MANIFEST.json"
            expected = {prefix + name: entry["sha256"] for name, entry in json.loads(z.read(manifest)).items()}
            bom = rows(z.read(prefix + "assembly/JLCPCB-BOM.csv"))
            cpl = rows(z.read(prefix + "assembly/JLCPCB-CPL.csv"))
            identities = {r["Reference"]: r["ManufacturerPart"] for r in rows(z.read(prefix + "assembly/engineering-bom.csv"))}
            assert all(row["Comment"] == identities[row["Designator"]] for row in bom)
        else:
            manifest = "SHA256SUMS.txt"
            expected = {}
            for line in z.read(manifest).decode().splitlines():
                digest, name = line.split(None, 1)
                expected[name.strip().lstrip("*")] = digest
            bom = rows(z.read("Assembly/BOM.csv"))
            cpl = rows(z.read("Assembly/CPL.csv"))
            identities = {r["Designator"]: r["Manufacturer Part Number"] for r in bom}
            assert all(r["Quantity per board"] == "1" for r in bom)
        assert expected.keys() == set(z.namelist()) - {manifest}
        assert all(sha(z.read(name)) == digest for name, digest in expected.items())
        references = set(identities)
        assert len(bom) == len(cpl) == len(references) == report["procurement"]["parts_per_board"]
        assert {r["Designator"] for r in bom} == {r["Designator"] for r in cpl} == references
        observed = {r["Reference"]: r for r in rows((folder / "parts.csv").read_bytes())}
        for row in bom:
            ref = row["Designator"]
            code = row["LCSC Part #"]
            assert code == observed[ref]["JLCPCB"] and identities[ref] == observed[ref]["Catalog MPN"]
            if catalog:
                assert catalog.execute("SELECT mfr FROM jlc_components WHERE lcsc=? AND present=1",
                                       (int(code[1:]),)).fetchone() == (identities[ref],)
        print(f"{arm}: {len(actual):,} original bytes, {len(expected)} manifest entries, {len(references)} fitted references")
        if cli:
            work = Path(tmp)
            z.extractall(work)  # Exact hash-identified edition, verified above.
            cad = report["kicad"]
            board = work / cad["board"].removeprefix("issued/")
            schematic = work / cad["schematic"].removeprefix("issued/")
            for label, args, source in (("erc", ["sch", "erc"], schematic), ("drc", ["pcb", "drc"], board),
                                        ("parity", ["pcb", "drc", "--schematic-parity"], board)):
                dest = work / f"receiver-{label}.json"
                run = subprocess.run([cli, *args, "--severity-all", "--format", "json",
                                      "--exit-code-violations", "-o", dest, source], capture_output=True)
                if not dest.is_file():
                    raise RuntimeError(run.stderr.decode(errors="replace"))
                data = json.loads(dest.read_bytes())
                findings = [(k, v) for k in ("violations", "unconnected_items", "schematic_parity")
                            for v in data.get(k, [])]
                findings += [("sheets", v) for s in data.get("sheets", []) for v in s.get("violations", [])]
                counts = Counter(f"{k}/{v['severity']}/{v['type']}" for k, v in findings)
                expected_run = cad["trials"][label]
                assert run.returncode == expected_run["exit"] and dict(counts) == expected_run["counts"], (label, counts)
                print(f"  {label}: exit {run.returncode}, {sum(counts.values())} findings")


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Run without Python's -O flag to enable the receiving checks.")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kicad", metavar="KICAD_CLI")
    parser.add_argument("--catalog", type=Path, metavar="COMPONENTS_SQLITE")
    args = parser.parse_args()
    db = None
    if args.catalog:
        digest = hashlib.sha256()
        with args.catalog.open("rb") as source:
            for block in iter(lambda: source.read(4 << 20), b""):
                digest.update(block)
        assert digest.hexdigest() == "080b01786a73850ce546a049053dbcf2c15fc41b029bd69b6c942d1848b56632"
        db = sqlite3.connect(args.catalog.resolve().as_uri() + "?mode=ro&immutable=1", uri=True)
    for name in ("frozen", "experienced"):
        check(name, args.kicad, db)
