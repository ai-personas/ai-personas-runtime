"""Project documented supplier land shapes using the exported EDA datum and the buyer's actual placement rows.

Overlap describes two CAD land patterns, not the solder joint or a machine preview.
Plated-hole pads are compared as Gerber flashes; connector fit is checked separately.
"""
import math
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "inverter-revision"))
from recheck import numbered_copper, number


def land_outline(pad):
    from shapely.affinity import scale
    from shapely.geometry import LineString, Point, box

    w, h = pad["size_mm"]
    if pad["shape"] == "RECT":
        shape = box(-w / 2, -h / 2, w / 2, h / 2)
    elif pad["shape"] == "ELLIPSE" or (pad["shape"] == "OVAL" and w == h):
        shape = scale(Point(0, 0).buffer(1, resolution=96), w / 2, h / 2)
    elif pad["shape"] == "OVAL":
        end = (max(w-h, 0) / 2, max(h-w, 0) / 2)
        shape = LineString([(-end[0], -end[1]), end]).buffer(min(w, h) / 2, resolution=96)
    else:
        raise ValueError("Unsupported public land shape: " + pad["shape"])
    assert pad["rot"] == 0, "These source cells were measured at zero intrinsic pad rotation"
    return shape


def project(copper, bom, cpl, columns, libraries, calibration):
    from shapely.affinity import rotate, translate
    from shapely.geometry import Point

    actual = numbered_copper(copper)
    cells = {part["code"]: part for part in calibration["source"].values()}
    results = {}
    for ref, row in sorted(cpl.items()):
        code = bom[ref][columns["code"]].strip()
        lib = libraries[code]
        cell = cells[code]
        assert lib["sha256"] == cell["source_sha256"] and lib["package"] == cell["land_pattern"]
        assert row[columns["side"]].strip().lower() in ("top", "t")
        midpoint = [number(row[columns[a]]) for a in ("x", "y")]
        theta = number(row[columns["rot"]])
        dx, dy = cell["mid_minus_ref_xy_mm"]
        c, s = math.cos(math.radians(theta)), math.sin(math.radians(theta))
        coverage = []
        for pad in lib["pads"]:
            pin = pad["number"]
            if not pin:
                continue
            copper_pad = actual[ref, pin]
            x = pad["xy_mm"][0] - dx
            y = -pad["xy_mm"][1] - dy
            p = Point(midpoint[0] + c*x - s*y, midpoint[1] + s*x + c*y)
            outline = translate(rotate(land_outline(pad), theta, origin=(0, 0)), p.x, p.y)
            coverage.append({
                "pin": pin, "xy_mm": [round(p.x, 5), round(p.y, 5)],
                "center_on_numbered_copper": bool(copper_pad.covers(p)),
                "center_to_plotted_centroid_mm": None if copper_pad.is_empty else round(p.distance(copper_pad.centroid), 5),
                "fraction_of_catalog_land_on_numbered_copper": round(outline.intersection(copper_pad).area / outline.area, 5),
            })
        results[ref] = coverage
    return results
