"""Independent BC-17/A dimensional intake. Read the cutter file, not the maker's calculations."""
import argparse
from hashlib import sha256
from importlib.metadata import version
from io import BytesIO
from itertools import product
import json
from math import dist, isfinite
import os
from pathlib import Path
import re
from tempfile import TemporaryDirectory
from xml.etree import ElementTree as ET
from zipfile import ZipFile

from shapely import affinity
from shapely.geometry import MultiPoint, Polygon, box
from shapely.ops import unary_union


def require(value, message):
    if not value:
        raise ValueError(message)


def artwork(data):
    from meerk40t.svgelements import SVG, Close, Line, Move, Path as SvgPath, Shape
    root = ET.fromstring(data)
    factors = {"mm": 1, "cm": 10, "in": 25.4, "pt": 25.4/72, "pc": 25.4/6, "px": 25.4/96, "": 25.4/96}
    def length(key):
        m = re.fullmatch(r"\s*([+\-\d.eE]+)\s*(mm|cm|in|pt|pc|px)?\s*", root.attrib[key])
        require(m, "No physical SVG " + key)
        return float(m[1]) * factors[m[2] or ""]
    require(not any(e.tag.rsplit("}", 1)[-1] in ("text","image","foreignObject") for e in root.iter()),
            "Unexpected text or image in the cutting file")
    doc = SVG.parse(BytesIO(data), ppi=96, reify=True, on_error="raise")
    sx, sy = length("width") / doc.width, length("height") / doc.height
    require(all(isfinite(a) and a > 0 for a in (sx, sy)), "Invalid page scale")
    paths = []
    for node in doc.elements():
        if not isinstance(node, Shape):
            continue
        for path in SvgPath(node).as_subpaths():
            points = []
            for seg in path:
                require(isinstance(seg, (Move, Line, Close)), "Nonlinear cutter path")
                if not isinstance(seg, Move):
                    require(points and dist(points[-1], (sx*seg.start.x, sy*seg.start.y)) < 1e-8, "Broken vector")
                q = (seg.end.x*sx, seg.end.y*sy)
                if not points or points[-1] != q:
                    points.append(q)
            paths.append(points)
    require(len(paths) == 1, f"Expected a single contour; found {len(paths)}")
    return polygon(paths[0]), {"declared_width": root.attrib["width"], "declared_height": root.attrib["height"],
                               "page_mm": [length("width"), length("height")], "points_mm": paths[0]}


def polygon(points):
    require(len(points) >= 4 and dist(points[0], points[-1]) < 1e-8, "Open or empty contour")
    # Use the actual path order; do not fix, union or simplify bad artwork.
    p = Polygon(points)
    require(p.is_valid and p.exterior.is_simple and p.area > 0, "Invalid or self-intersecting contour")
    return p


def at_datum(p):
    return affinity.translate(p, xoff=-p.bounds[2], yoff=-p.bounds[1])


def envelopes(p, radius):
    """Sweep every straight boundary segment with the specified axis-aligned square.

    The swept band, including the square around every vertex, contains all
    points within L-infinity radius of the boundary. Union with / subtract
    from the original set gives its exact outer / inner polygonal envelope.
    A round Euclidean buffer or a miter on a diagonal is not that kernel.
    """
    corners = list(product((-radius, radius), repeat=2))
    v = list(p.exterior.coords)
    band = unary_union([MultiPoint([(x+dx, y+dy) for x,y in (a,b) for dx,dy in corners]).convex_hull
                        for a,b in zip(v, v[1:])])
    return p.difference(band), p.union(band)


def swept_regions():
    # Project each *whole* rectangle from the panel to the art datum before
    # taking extrema. This preserves the shared W in both right-land edges.
    cases = product((65.5, 66.5), (-.35, .35), (-.35, .35), (25., 27.), (3.7, 4.3),
                    (5.7, 6.3), (3.7, 4.3), (14.5, 16.5), (3.7, 4.3), (3.7, 4.3))
    subjects = {k: [] for k in ("left_land", "right_land", "transverse_land", "wire_aisle")}
    for w, dx, dy, pad_left, w1, gap, w2, pad_top, h1, h2 in cases:
        ox, oy = w - 2 + dx, 2 + dy
        boxes = [(5,6,19,17), (w-19,6,w-5,17), (5,25,w-5,27),
                 (pad_left-1,0,pad_left+w1+gap+w2+1,pad_top+max(h1,h2)+1)]
        for values, (l,t,r,b) in zip(subjects.values(), boxes):
            values.append((l-ox,t-oy,r-ox,b-oy))
    # The independent X and Y translations continuously cover intervals;
    # on each axis the component intervals overlap. Thus the union over the
    # entire continuous tolerance box is exactly this rectangle for each test.
    return {k: box(min(a[0] for a in v), min(a[1] for a in v), max(a[2] for a in v), max(a[3] for a in v))
            for k,v in subjects.items()}


def assess(p):
    p = at_datum(p)
    inner, outer = envelopes(p, .15)
    regions = swept_regions()
    l,t,r,b = outer.bounds
    left = 65.5-2-.35+l
    right = 2-.35-r
    top = 2-.35+t
    bottom_y = 2+.35+b
    # Computational tolerance only (nm or smaller), not an additional shop allowance.
    eps = 1e-9
    bounds = dict(left_inset=left, right_inset=right, top_inset=top, deepest_y=bottom_y,
                  bottom_panel_inset=112.5-bottom_y)
    checks = {"left_inset": left >= 1-eps, "right_inset": right >= 1-eps, "top_inset": top >= 1-eps,
              "depth_limit": bottom_y <= 30+eps, "inside_panel_bottom": bottom_y <= 112.5+eps,
              "connected_eroded_core": inner.geom_type == "Polygon" and not inner.is_empty,
              "minimum_area": inner.area >= 1150-eps}
    details = {}
    for k,zone in regions.items():
        if k == "wire_aisle":
            overlap = outer.intersection(zone).area
            checks[k] = overlap <= eps
            details[k] = {"sweep_at_art_datum_mm": zone.bounds, "intrusion_mm2": overlap,
                          "additional_clearance_mm": outer.distance(zone)}
        else:
            missing = zone.difference(inner)
            checks[k] = not inner.is_empty and inner.buffer(eps).covers(zone)
            details[k] = {"sweep_at_art_datum_mm": zone.bounds, "uncovered_mm2": missing.area,
                          "additional_clearance_mm": inner.boundary.distance(zone) if checks[k] else None}
    return {"centerline_span_mm": [p.bounds[2]-p.bounds[0], p.bounds[3]-p.bounds[1]],
            "nominal_area_mm2": p.area, "finished_minimum_area_mm2": inner.area,
            "outer_profile_worst_mm": bounds, "functional_regions": details, "checks": checks}


def toolpath(source):
    """Stock vector-cut planning, one operation on all imported entities. Dummy device only."""
    from meerk40t.kernel import Kernel
    from meerk40t.core import core
    from meerk40t.core.units import UNITS_PER_MM
    from meerk40t.device import dummydevice
    k = Kernel("MeerK40t", version("meerk40t"), "BC17 receiver", ansi=False, ignore_settings=True)
    k.add_plugin(core.plugin)
    k.add_plugin(dummydevice.plugin)
    k(partial=True)
    try:
        k.console("service device start dummy 0\n")
        k.elements.load(str(source))
        nodes = list(k.elements.elems())
        initial = [op.type for op in k.elements.ops() if len(op.children)]
        for op in k.elements.ops():
            op.output = False
        cut = k.elements.op_branch.add(type="op cut")
        for node in nodes:
            cut.add_reference(node)
        k.planner("plan copy preprocess validate blob preopt optimize\n")
        paths, current = [], []
        for operation in k.planner.default_plan.plan:
            require(type(operation).__name__ == "CutCode", "No vector cut code")
            for seg in operation.flat():
                require(type(seg).__name__ == "LineCut", "Nonlinear planned motion")
                if not current:
                    current.append(seg.start)
                require(current[-1] == seg.start, "Open intermediate contour in CAM")
                current.append(seg.end)
                if current[0] == current[-1]:
                    paths.append([tuple(v/UNITS_PER_MM for v in k.device.view.iposition(*q)) for q in current])
                    current = []
        require(not current and len(paths) == 1, "Not exactly one closed machine path")
        return polygon(paths[0]), {"default_operations": initial, "entities": [n.type for n in nodes],
                                  "points_mm": paths[0], "spooled": False}
    finally:
        k()


def receive(zip_path, member=None):
    data = zip_path.read_bytes()
    result = {"download": zip_path.name, "bytes": len(data), "sha256": sha256(data).hexdigest(),
              "svg_reader_and_cam": "MeerK40t " + version("meerk40t"), "geometry_reader": "Shapely " + version("shapely")}
    with ZipFile(BytesIO(data)) as z:
        require(z.testzip() is None and len(z.namelist()) == len(set(z.namelist())), "Ambiguous or damaged ZIP")
        svgs = [n for n in z.namelist() if n.lower().endswith(".svg")]
        if member is None:
            require(len(svgs) == 1, "Select the SVG named in the operator's instructions")
            member = svgs[0]
        source = z.read(member)
        result.update(member=member, members={n: sha256(z.read(n)).hexdigest() for n in z.namelist()})
    p, result["artwork"] = artwork(source)
    result["artwork"].update(assess(p))
    with TemporaryDirectory() as folder:
        file = Path(folder) / "received.svg"
        file.write_bytes(source)
        q, result["cam"] = toolpath(file)
    result["cam"].update(assess(q))
    result["cam"]["boundary_hausdorff_mm"] = at_datum(p).boundary.hausdorff_distance(at_datum(q).boundary)
    # Explain the two marginal sides of this *issued* rectangular notch.
    # Relative to the specified upper-right datum, its left/top opening
    # vertex and its floor determine the outer-left-pad and lower-pad cases.
    for key in ("artwork","cam"):
        pts = result[key]["points_mm"]
        xs = sorted({x for x,y in pts})
        ys = sorted({y for x,y in pts})
        if len(xs) == 4 and len(ys) == 3:
            slot = box(xs[1], ys[0], xs[2], ys[1])
            if polygon(pts).symmetric_difference(box(xs[0],ys[0],xs[-1],ys[-1]).difference(slot)).area < 1e-9:
                left, right = xs[1]-xs[-1], xs[2]-xs[-1]
                floor = ys[1]-ys[0]
                # Evaluate opposite extremes separately, not as one panel pose.
                result[key]["worst_pad_clearance_mm"] = {
                    "left": 25-(66.5-2+.35+left+.15),
                    "right": (65.5-2-.35+right-.15)-(27+4.3+6.3+4.3),
                    "below": (2-.35+floor-.15)-(16.5+4.3)}
    result["specified_artwork_pass"] = all(result["artwork"]["checks"].values())
    result["additional_full_allowance_about_planned_path_pass"] = all(result["cam"]["checks"].values())
    result["combined_pass"] = result["specified_artwork_pass"] and result["additional_full_allowance_about_planned_path_pass"]
    result["allowance_basis"] = ("BC-17 specifies +/-0.15 mm about the issued ideal profile, inclusive of cutting. "
        "Applying that full allowance again about a rounded machine path is a stricter shop stress test. "
        "Area of intrusion into the swept aisle is a union over poses, not a physical contact area.")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("zip", type=Path)
    parser.add_argument("--member")
    args = parser.parse_args()
    with TemporaryDirectory() as settings:
        os.environ.update(HOME=settings, XDG_CONFIG_HOME=settings)
        verdict = receive(args.zip, args.member)
    print(json.dumps(verdict, indent=2, ensure_ascii=False))
    raise SystemExit(0 if verdict["combined_pass"] else 1)
