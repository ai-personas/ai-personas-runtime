#!/usr/bin/env python3
"""Recheck the unedited shop-return edition against its receiving record."""
import argparse
from collections import Counter, defaultdict
from contextlib import ExitStack
import csv
import hashlib
import io
import json
import math
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile

HERE = Path(__file__).resolve().parent


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read_table(data):
    text = data.decode("utf-8-sig")
    delim = "\t" if "\t" in text.splitlines()[0] else ","
    return list(csv.DictReader(io.StringIO(text), delimiter=delim))


def by_ref(rows, column):
    result = {}
    for row in rows:
        for ref in re.split(r"[\s,;]+", row[column].strip()):
            assert ref and ref not in result, f"Empty or duplicate reference: {ref}"
            result[ref] = row
    return result


def number(value):
    return float(re.sub(r"\s*(mm|deg|°)\s*$", "", value, flags=re.I))


def pad_coverage(copper, cpl, bom, columns, libraries):
    from gerbonara import GerberFile
    from gerbonara.utils import MM
    from shapely.geometry import Point, GeometryCollection
    from drill_overlay import shape
    plotted = defaultdict(GeometryCollection)
    for ob in GerberFile.open(copper).objects:
        attrs = getattr(ob, "attrs", None)
        if not isinstance(attrs, dict) or ".P" not in attrs:
            continue
        ref, pin, *_ = attrs[".P"]
        aperture = GeometryCollection()
        for prim in ob.to_primitives(MM):
            s = shape(prim)
            aperture = aperture.union(s) if prim.polarity_dark == ob.polarity_dark else aperture.difference(s)
        k = (ref, pin)
        plotted[k] = plotted[k].union(aperture) if ob.polarity_dark else plotted[k].difference(aperture)
    counts = {}
    for ref, row in cpl.items():
        assert row[columns["side"]].strip().lower() in ("top", "t")
        lib = libraries[bom[ref][columns["code"]].strip()]
        a = math.radians(number(row[columns["rot"]]))
        xx, yy = number(row[columns["x"]]), number(row[columns["y"]])
        counts[ref] = {}
        for name, datum in [("origin", [0., 0.]), ("model_center", lib["model_center_xy_up_mm"])]:
            if datum is None:
                counts[ref][name] = None
                continue
            hits = 0
            for pin in lib["numbered_pads"]:
                x = pin["xy_mm"][0] - datum[0]
                y = -pin["xy_mm"][1] - datum[1]
                pt = Point(xx + x * math.cos(a) - y * math.sin(a), yy + y * math.cos(a) + x * math.sin(a))
                hits += plotted[ref, pin["number"]].covers(pt)
            counts[ref][name] = [hits, len(lib["numbered_pads"])]
    return counts


def plot_lines(data):
    for line in data.splitlines():
        if line.startswith((b"%TF.CreationDate,", b"; #@! TF.CreationDate,")):
            continue
        if b" date " in line and line.startswith((b"G04 Created by KiCad (PCBNEW ", b"; DRILL file {KiCad ")):
            continue
        yield line


def kicad_checks(cli, project, config, fitted, fab, issued_plots):
    for label, cmd, name in [
        ("erc", ["sch", "erc"], config["schematic"]),
        ("drc", ["pcb", "drc"], config["board"]),
        ("parity", ["pcb", "drc", "--schematic-parity"], config["board"]),
    ]:
        dest = project.parent / (label + ".json")
        proc = subprocess.run([cli, *cmd, "--severity-all", "--format", "json", "--exit-code-violations",
                               "-o", dest, project / name], capture_output=True)
        if not dest.is_file():
            raise RuntimeError(proc.stderr.decode(errors="replace"))
        result = json.loads(dest.read_bytes())
        findings = [(k, e) for k in ("violations", "unconnected_items", "schematic_parity") for e in result.get(k, [])]
        findings += [("sheets", e) for sheet in result.get("sheets", []) for e in sheet.get("violations", [])]
        counts = Counter(f"{k}/{e['severity']}/{e['type']}" for k, e in findings)
        assert proc.returncode == config[label]["exit"] and counts == config[label]["counts"], (label, counts)
        print(f"{label}: exit {proc.returncode}, {sum(counts.values())} findings")
    net = project.parent / "net.xml"
    pos = project.parent / "pos.csv"
    cam = project.parent / "regenerated"
    cam.mkdir()
    def run(*args):
        subprocess.run([cli, *args], check=True, capture_output=True)
    run("sch", "export", "netlist", "--format", "kicadxml", "-o", net, project / config["schematic"])
    run("pcb", "export", "pos", "--format", "csv", "--units", "mm", "--side", "both", "-o", pos, project / config["board"])
    xml = ET.parse(net)
    assert {c.get("ref") for c in xml.findall("./components/comp")} == fitted == by_ref(read_table(pos.read_bytes()), "Ref").keys()
    pins = {(n.get("ref"), n.get("pin")): group.get("name").lstrip("/") for group in xml.findall("./nets/net") for n in group.findall("node")}
    assert len(pins) == config["numbered_pins"] and {pins["R4", "1"], pins["R4", "2"]} == {"+5V", "GND"}
    print(f"Regenerated netlist and placement: {len(fitted)} references; {len(pins)} schematic pins, R4 across +5V and GND.")
    run("pcb", "export", "gerbers", "--layers", "F.Cu,B.Cu,F.Paste,B.Paste,F.Silkscreen,B.Silkscreen,F.Mask,B.Mask,Edge.Cuts",
        "-o", str(cam) + "/", project / config["board"])
    run("pcb", "export", "drill", "-o", str(cam) + "/", project / config["board"])
    folder = Path(fab["top_copper"]).parent
    for name, item in fab["regenerated_layers"].items():
        before = issued_plots.read((folder / item["issued"]).as_posix())
        after = (cam / item["regenerated"]).read_bytes()
        assert sha(before) == item["sha256"] and list(plot_lines(after)) == list(plot_lines(before)), name
    print(f"Fabrication: all {len(fab['regenerated_layers'])} issued plots/drill files match regeneration apart from dates.")


def check(args):
    result = json.loads((HERE / "verification.json").read_text())
    issue, assembly, fab = (result[k] for k in ("download", "assembly", "fabrication"))
    original = (HERE / issue["filename"]).read_bytes()
    assert sha(original) == issue["sha256"] and len(original) == issue["bytes"]
    with ExitStack() as owned:
        z = owned.enter_context(zipfile.ZipFile(io.BytesIO(original)))
        names = [item.filename for item in z.infolist() if not item.is_dir()]
        digests = {n: sha(z.read(n)) for n in names}
        assert len(names) == len(digests) == issue["members"]
        assert sha(json.dumps(digests, sort_keys=True, separators=(",", ":")).encode()) == issue["member_tree_sha256"]
        manifest = issue["manifest"]
        checks = [s.split(None, 1) for s in z.read(manifest).decode().splitlines()]
        assert len(checks) == issue["manifest_entries"] == len(digests) - 1
        assert {(Path(manifest).parent / name).as_posix(): digest for digest, name in checks} == {
            name: digest for name, digest in digests.items() if name != manifest
        }
        print(f"Original download: {len(original):,} bytes; {len(names)} exact files; {issue['sha256']}")
        def archive(path):
            return owned.enter_context(zipfile.ZipFile(io.BytesIO(z.read(path)))) if path else z
        order = archive(assembly.get("archive"))
        columns = assembly["columns"]
        bom = by_ref(read_table(order.read(assembly["bom"])), columns["bom_ref"])
        cpl = by_ref(read_table(order.read(assembly["cpl"])), columns["cpl_ref"])
        identities = by_ref(read_table(order.read(assembly["identity"])), columns["identity_ref"]) if assembly.get("identity") else bom
        measured = by_ref(read_table((HERE / "parts.csv").read_bytes()), "Reference")
        fitted = set(assembly["fitted_references"])
        assert set(bom) == set(assembly["bom_references"]) and set(cpl) == set(assembly["cpl_references"])
        assert measured.keys() == fitted
        if args.catalog:
            db = sqlite3.connect(args.catalog.resolve().as_uri() + "?mode=ro&immutable=1", uri=True)
            owned.callback(db.close)
        for ref in fitted & bom.keys():
            code = bom[ref][columns["code"]].strip()
            assert identities[ref][columns["mpn"]].strip() == measured[ref]["Issued MPN"] == measured[ref]["Catalog MPN"] == measured[ref]["Public MPN"]
            assert code == measured[ref]["JLCPCB"]
            assert min(int(measured[ref]["Catalog stock"]), int(measured[ref]["Public stock"])) >= int(measured[ref]["Quantity screened with allowance (code)"])
            if args.catalog:
                assert db.execute("select mfr, stock from jlc_components where lcsc=? and present=1",
                                  (int(code[1:]),)).fetchone() == (measured[ref]["Catalog MPN"], int(measured[ref]["Catalog stock"]))
        print(f"PCB: {len(fitted)} fitted references; BOM: {len(bom)}; placement: {len(cpl)}")
        work = Path(owned.enter_context(tempfile.TemporaryDirectory()))
        if args.plots:
            from drill_overlay import observe
            plots = archive(fab.get("archive"))
            def write(name):
                path = work / "cam" / name
                path.parent.mkdir(exist_ok=True, parents=True)
                path.write_bytes(plots.read(name))
                return path
            overlay = observe(write(fab["top_paste"]), write(fab["top_mask"]), [write(p) for p in fab["drills"]])
            for key in ("holes", "under_top_paste", "in_top_mask_openings"):
                assert overlay[key] == fab[key], (key, overlay[key])
            print(f"Drills: {overlay['holes']}; hits intersecting top paste: {overlay['under_top_paste']}")
            library = json.loads((HERE / "placement-reference.json").read_text())
            counts = pad_coverage(write(fab["top_copper"]), cpl, bom, columns, library)
            assert counts == assembly["numbered_centers_on_corresponding_copper"], counts
            print("Part-specific pad-center projections match the recorded top-copper observations.")
        if args.kicad:
            project = work / "project"
            archive(result["kicad"].get("archive")).extractall(project)  # Only after exact edition validation.
            kicad_checks(args.kicad, project, result["kicad"], fitted, fab, archive(fab.get("archive")))


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Run without -O to enable the checks")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kicad", metavar="KICAD_CLI")
    parser.add_argument("--catalog", type=Path, metavar="SUPPLIER_SQLITE")
    parser.add_argument("--plots", action="store_true", help="Requires gerbonara and shapely")
    check(parser.parse_args())
