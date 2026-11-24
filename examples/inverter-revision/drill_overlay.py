#!/usr/bin/env python3
"""Measure each issued Excellon hit against the assembly-side paste and mask openings (mm).

Usage: python3 drill_overlay.py --paste TOP_PASTE --mask TOP_MASK DRILL [DRILL ...]
Requires gerbonara and shapely. Does not change the files. Counts geometric
intersections, including partial overlap; it does not decide factory acceptance.
"""
import argparse
import hashlib
import json
from pathlib import Path

from gerbonara import ExcellonFile, GerberFile, graphic_primitives as gp
from gerbonara.utils import MM, approximate_arc
from shapely.affinity import rotate
from shapely.geometry import GeometryCollection, LineString, Point, Polygon, box
from shapely.ops import unary_union


def shape(p):
    if isinstance(p, gp.Circle):
        return Point(p.x, p.y).buffer(p.r, resolution=96)
    if isinstance(p, gp.Rectangle):
        rect = box(p.x - p.w / 2, p.y - p.h / 2, p.x + p.w / 2, p.y + p.h / 2)
        return rotate(rect, p.rotation, origin=(p.x, p.y), use_radians=True)
    if isinstance(p, gp.Line):
        return LineString([(p.x1, p.y1), (p.x2, p.y2)]).buffer(p.width / 2, resolution=96)
    if isinstance(p, gp.Arc):
        arc = list(approximate_arc(p.cx, p.cy, p.x1, p.y1, p.x2, p.y2, p.clockwise, max_error=0.0001))
        return LineString(arc).buffer(p.width / 2, resolution=96)
    if isinstance(p, gp.ArcPoly):
        pts = []
        for a, b, (cw, center) in p.segments:
            if cw is None:
                pts.append(a)
            else:
                pts.extend(list(approximate_arc(*center, *a, *b, cw, max_error=0.0001))[:-1])
        poly = Polygon(pts)
        if not poly.is_valid:
            raise ValueError("Unsupported self-intersecting region")
        return poly
    raise ValueError(f"Unsupported plot primitive: {type(p).__name__}")


def openings(path, function, polarity, side="Top"):
    layer = GerberFile.open(path)
    if side not in ("Top", "Bot") or layer.file_attrs.get(".FileFunction") != (function, side):
        raise ValueError(f"{path.name}: expected {function},{side}")
    if layer.file_attrs.get(".FilePolarity") != (polarity,):
        raise ValueError(f"{path.name}: unexpected {function} file polarity")
    image = GeometryCollection()
    for obj in layer.objects:
        # A compound aperture is composed locally before it darkens or clears
        # the layer. Compound-aperture gaps don't erase an earlier flash.
        aperture = GeometryCollection()
        for p in obj.to_primitives(MM):
            surface = shape(p)
            aperture = aperture.union(surface) if p.polarity_dark == obj.polarity_dark else aperture.difference(surface)
        image = image.union(aperture) if obj.polarity_dark else image.difference(aperture)
    return image


def observe(paste, mask, drills, side="Top"):
    a = openings(paste, "Paste", "Positive", side)
    m = openings(mask, "Soldermask", "Negative", side)
    hits = []
    for path in drills:
        for obj in ExcellonFile.open(path).objects:
            hole = unary_union([shape(p) for p in obj.to_primitives(MM)])
            if hole.is_empty or hole.area == 0:
                raise ValueError("Unsupported drill object")
            hits.append({
                "drill": path.name,
                "center_mm": [round(z, 5) for z in hole.centroid.coords[0]],
                "bounds_mm": [round(v, 5) for v in hole.bounds],
                "paste_intersection_mm2": round(hole.intersection(a).area, 6),
                "mask_open_intersection_mm2": round(hole.intersection(m).area, 6),
                "paste_clearance_mm": None if a.is_empty else round(hole.distance(a), 5),
                "mask_open_clearance_mm": None if m.is_empty else round(hole.distance(m), 5),
            })
    label = "top" if side == "Top" else "bottom"
    return {
        "sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [paste, mask, *drills]},
        "holes": len(hits),
        f"under_{label}_paste": sum(h["paste_intersection_mm2"] > 0 for h in hits),
        f"in_{label}_mask_openings": sum(h["mask_open_intersection_mm2"] > 0 for h in hits),
        "hits": hits,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--paste", type=Path, required=True)
    parser.add_argument("--mask", type=Path, required=True)
    parser.add_argument("--side", choices=("Top", "Bot"), default="Top")
    parser.add_argument("drills", type=Path, nargs="+")
    args = parser.parse_args()
    print(json.dumps(observe(args.paste, args.mask, args.drills, args.side), indent=2))
