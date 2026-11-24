#!/usr/bin/env python3
"""Read the issued buyer ZIP without changing it. Optional checks run on a temporary copy."""
import argparse
from collections import Counter
import io
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import zipfile
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "inverter-revision"))
from recheck import by_ref, kicad_checks, number, plot_lines, read_table, sha


def check(args):
    evidence = json.loads((HERE / "verification.json").read_bytes())
    q, p, a, fab, cad = (evidence[k] for k in ("download", "paths", "assembly", "fabrication", "kicad"))
    original = (HERE / q["filename"]).read_bytes()
    assert len(original) == q["bytes"] and sha(original) == q["sha256"]
    with zipfile.ZipFile(io.BytesIO(original)) as z, tempfile.TemporaryDirectory() as temp:
        digests = {i.filename: sha(z.read(i)) for i in z.infolist() if not i.is_dir()}
        assert len(digests) == q["members"] == sum(not i.is_dir() for i in z.infolist())
        assert sha(json.dumps(digests, sort_keys=True, separators=(",", ":")).encode()) == q["tree_sha256"]
        entries = [row.split(None, 1) for row in z.read(q["manifest"]).decode().splitlines()]
        assert len(entries) == q["manifest_entries"] == len(digests)-1
        assert {n: h for h, n in entries} == {n: h for n, h in digests.items() if n != q["manifest"]}
        print(f"{q['filename']}: {len(original):,} bytes, {len(digests)} members, {len(entries)} checksums; {q['sha256']}")
        c = p["columns"]
        bom = by_ref(read_table(z.read(p["bom"])), c["bom_ref"])
        cpl = by_ref(read_table(z.read(p["cpl"])), c["cpl_ref"])
        parts = by_ref(read_table((HERE / "parts.csv").read_bytes()), "Reference")
        fitted = set(a["fitted"])
        assert set(bom) == set(cpl) == set(parts) == fitted
        totals = Counter()
        for ref in fitted:
            m = parts[ref]
            code = bom[ref][c["code"]]
            assert int(bom[ref]["Quantity per board"]) == 1 and code == m["JLCPCB"]
            assert bom[ref][c["mpn"]] == m["Issued MPN"] == m["Public MPN"] == m["Catalog MPN"]
            assert min(int(m["Public stock"]), int(m["Catalog stock"])) >= int(m["Screened quantity with allowance (per code)"])
            totals[code] += 5
            assert cpl[ref][c["side"]].lower() == m["Side"] == "top"
            for axis in ("x", "y"):
                assert number(cpl[ref][c[axis]]) == float(m["CPL " + axis.upper() + " mm"])
            assert number(cpl[ref][c["rot"]]) == float(m["CPL CCW degrees"])
        assert totals == a["planned_lots"] and sum(totals.values()) == a["parts_per_five"]
        if args.catalog:
            db = sqlite3.connect(args.catalog.resolve().as_uri()+"?mode=ro&immutable=1", uri=True)
            try:
                for m in parts.values():
                    assert db.execute("SELECT mfr,stock FROM jlc_components WHERE lcsc=? AND present=1", (int(m["JLCPCB"][1:]),)).fetchone() == (m["Catalog MPN"], int(m["Catalog stock"]))
            finally:
                db.close()
        print(f"{len(fitted)} fitted references; {len(totals)} supplier codes; {sum(totals.values())} installed parts for five.")
        assert digests[p["schematic"]] == cad["source_hashes"][0] and digests[p["board"]] == cad["source_hashes"][1]
        assert fab["top_paste"] is None and fab["missing"] == ["Paste,Bot", "Paste,Top"]
        for item in fab["layers"].values():
            assert digests[str(Path(p["fabrication"]) / item["issued"])] == item["sha256"]
        print("The released CAM set contains 7 Gerbers and 2 drills. Neither paste plot is supplied.")
        if not (args.kicad or args.plots):
            return
        root = Path(temp)
        project = root / "project"
        z.extractall(project)  # Only the verified exact archive above.
        if args.kicad:
            kicad_checks(args.kicad, project, cad, fitted, {**fab, "regenerated_layers": fab["layers"]}, z)
            xml = ET.parse(root / "net.xml")
            pins = {n.get("ref")+":"+n.get("pin"): group.get("name").lstrip("/") for group in xml.findall("./nets/net") for n in group.findall("node")}
            assert sha(json.dumps(pins, sort_keys=True, separators=(",", ":")).encode()) == cad["schematic_pin_map_sha256"]
        if args.plots:
            from lands import project as place
            lib = json.loads((HERE / "land-comparison.json").read_bytes())
            calibration = json.loads((HERE / "placement-calibration/reference.json").read_bytes())
            assert sha((HERE / "placement-calibration/easyeda-original.csv").read_bytes()) == calibration["output_sha256"] == lib["calibration_output"]
            rows = place(project / fab["top_copper"], bom, cpl, c, lib["library"], calibration)
            assert rows == lib["references"]
            hits = sum(pin["center_on_numbered_copper"] for pads in rows.values() for pin in pads)
            print(f"Export-calibrated catalog pad centers on their numbered Gerber copper: {hits}/{sum(map(len, rows.values()))}")
            if args.kicad:
                from drill_overlay import observe
                paste, = [f for f in (root / "regenerated").iterdir() if b"TF.FileFunction,Paste,Top" in f.read_bytes()]
                assert sha(b"\n".join(plot_lines(paste.read_bytes()))) == fab["canonical_regenerated_paste_sha256"]
                overlay = observe(paste, project / fab["top_mask"], [project / f for f in fab["drills"]])
                for k in ("holes", "under_top_paste", "in_top_mask_openings"):
                    assert overlay[k] == fab[k]
                print(f"Released drills: {overlay['holes']}; intersections with regenerated top paste: {overlay['under_top_paste']}; holes in released mask openings: {overlay['in_top_mask_openings']}.")
                bottom = fab["bottom"]
                paste, = [f for f in (root / "regenerated").iterdir() if b"TF.FileFunction,Paste,Bot" in f.read_bytes()]
                assert sha(b"\n".join(plot_lines(paste.read_bytes()))) == bottom["canonical_regenerated_paste_sha256"]
                underside = observe(paste, project / bottom["mask"], [project / f for f in fab["drills"]], "Bot")
                assert underside["under_bottom_paste"] == bottom["under_regenerated_paste"]
                openings = [x["center_mm"] for x in underside["hits"] if x["mask_open_intersection_mm2"]]
                assert sorted(openings) == sorted(bottom["mask_open_drill_centers_mm"])
                print(f"Bottom: {len(openings)} holes in released mask openings; {underside['under_bottom_paste']} hits on regenerated paste.")


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Run without -O to enable the checks.")
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--plots", action="store_true", help="Requires gerbonara and shapely")
    cli.add_argument("--kicad", metavar="KICAD_CLI", help="Stock KiCad 9 CLI, with symbol and footprint libraries")
    cli.add_argument("--catalog", type=Path, help="Pinned supplier.sqlite")
    check(cli.parse_args())
