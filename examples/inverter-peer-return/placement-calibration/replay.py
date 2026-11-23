#!/usr/bin/env python3
"""Read the catalog-only EasyEDA export; optionally recreate its input PCB from the pinned public footprints."""
import argparse
import csv
import gzip
import hashlib
import io
import json
import math
from pathlib import Path
import re
from urllib.request import Request, urlopen

HERE = Path(__file__).resolve().parent


def digest(data):
    return hashlib.sha256(data).hexdigest()


def xy(row, name):
    return tuple(float(row[name + " " + axis].removesuffix("mm")) for axis in ("X", "Y"))


def check_export(calibration):
    raw = (HERE / "easyeda-original.csv").read_bytes()
    assert digest(raw) == calibration["output_sha256"]
    data = list(csv.DictReader(io.StringIO(raw.decode("utf-16")), delimiter="\t"))
    rows = {r["Designator"]: r for r in data}
    assert len(rows) == len(data) and rows.keys() == calibration["source"].keys()
    for ref, part in calibration["source"].items():
        r = rows[ref]
        assert r["Footprint"] == part["land_pattern"] and r["Rotation"] == "0" and r["Layer"] == "T"
        for field, k in (("Ref", "ref_xy_mm"), ("Mid", "mid_xy_mm"), ("Pad", "pad1_xy_mm")):
            assert list(xy(r, field)) == part[k]
        origin = part["source_origin"]  # EasyEDA source coordinates: 10 mil, Y down.
        for a, b in zip(xy(r, "Ref"), (origin[0] * 0.254, -origin[1] * 0.254)):
            assert abs(a - b) <= 0.0051
        assert [round(a - b, 3) for a, b in zip(xy(r, "Mid"), xy(r, "Ref"))] == part["mid_minus_ref_xy_mm"]
        print(f"{part['code']:>8}  {r['Footprint']:<46}  Mid−Ref (mm): {part['mid_minus_ref_xy_mm']}")
    return rows


def make_pcb(calibration, rows, cache):
    parts = calibration["source"]
    cells = []
    next_id = 300000
    for ref in sorted(parts, key=lambda r: int(r[1:])):
        part = parts[ref]
        if cache:
            raw = gzip.decompress((cache / (part["code"] + "-footprint.gz")).read_bytes())
        else:
            with urlopen(Request(part["source"], headers={"User-Agent": "Placement reference reader"}), timeout=25) as response:
                raw = response.read()
        assert digest(raw) == part["source_sha256"], f"The public record for {part['code']} changed"
        package = json.loads(raw)["result"]["packageDetail"]
        footprint = package["dataStr"]
        x, y = (footprint["head"][axis] for axis in ("x", "y"))
        first, = [s.split("~") for s in footprint["shape"] if s.startswith("PAD~") and s.split("~")[8] == "1"]
        for a, b in zip(xy(rows[ref], "Pad"), (float(first[2]) * 0.254, -float(first[3]) * 0.254)):
            assert abs(a - b) <= 0.0051, f"Pin 1 of {ref} did not import at its library coordinates"
        mount = "NONE" if part["code"] == "C8269" else "yes"
        header = f"LIB~{x}~{y}~package`{package['title']}`~0~~gge889991~1~{package['uuid']}~~~~~{mount}"
        label = f"TEXT~P~{x}~{y}~0.7~0~~3~~4.5~{ref}~~none~gge889992~0~"
        ids = {}

        def unique(match):
            nonlocal next_id
            if match[0] not in ids:
                next_id += 1
                ids[match[0]] = "gge" + str(next_id)
            return ids[match[0]]

        cells.append(re.sub(r"gge[0-9a-fA-F]+", unique, "#@$".join([header, label, *footprint["shape"]])))
        if part["code"] == "C86206":
            template = footprint
    origins = [p["source_origin"] for p in parts.values()]
    x1, y1 = (math.floor(min(v[a] for v in origins) - 55) for a in (0, 1))
    x2, y2 = (math.ceil(max(v[a] for v in origins) + 55) for a in (0, 1))
    outline = f"TRACK~1~10~~{x1} {y1} {x2} {y1} {x2} {y2} {x1} {y2} {x1} {y1}~gge959111~0"
    canvas = template["canvas"].split("~")
    canvas[16:18] = ["0", "0"]
    card = {
        "head": {"docType": "3", "editorVersion": "6.5.61", "newgId": True, "x": "0", "y": "0", "c_para": {}},
        "canvas": "~".join(canvas), "layers": template["layers"], "objects": template["objects"],
        "shape": [outline, *cells], "BBox": {"x": x1, "y": y1, "width": x2-x1, "height": y2-y1}, "netColors": {},
    }
    data = json.dumps(card, ensure_ascii=False, separators=(",", ":")).encode()
    assert digest(data) == calibration["source_doc_sha256"]
    return data


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Run without -O to enable verification")
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--make-pcb", type=Path, help="Create the tested local calibration document; fetch the pinned public library data if no cache is supplied.")
    cli.add_argument("--cache", type=Path, help="Directory of original C*-footprint.gz records, for offline replay.")
    args = cli.parse_args()
    calibration = json.loads((HERE / "reference.json").read_bytes())
    rows = check_export(calibration)
    if args.make_pcb:
        data = make_pcb(calibration, rows, args.cache)
        with args.make_pcb.open("xb") as f:
            f.write(data)
        print("Exact calibration PCB:", digest(data))
