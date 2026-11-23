"""Independent HG-42/C receiving bench. Give it an issued ZIP, not a persona workspace.

Requires MeerK40t 0.9.9100. Only dummy-device planning runs; the issuer's
programs, if any, are never used to measure the artwork or grade tolerances.
"""
import argparse
from decimal import Decimal as D
from hashlib import sha256
from importlib.metadata import version
from itertools import product
from io import BytesIO
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
from tempfile import TemporaryDirectory
import xml.etree.ElementTree as ET
from zipfile import ZipFile


SIDES = ("left", "top", "right", "bottom")
DRAWING = "5a4de29523d4c1bffde01e02bf7d38162e44b37d090d49a8857971b6537257fc"


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def rectangle(points):
    """All edges must trace one rectangle exactly once, including closure."""
    require(len(points) >= 5 and math.dist(points[0], points[-1]) < 1e-8, "Open vector")
    xs, ys = zip(*points)
    l, t, r, b = min(xs), min(ys), max(xs), max(ys)
    eps = 1e-8
    require(r - l > eps and b - t > eps, "Degenerate rectangle")
    directions = []
    for (x1, y1), (x2, y2) in zip(points, points[1:]):
        dx, dy = x2 - x1, y2 - y1
        if max(abs(dx), abs(dy)) < eps:
            continue
        if abs(dx) < eps and min(abs(x1-l), abs(x1-r)) < eps:
            step = "d" if dy > 0 else "u"
        elif abs(dy) < eps and min(abs(y1-t), abs(y1-b)) < eps:
            step = "r" if dx > 0 else "l"
        else:
            raise ValueError("Not an axis-aligned straight side of the rectangle")
        if not directions or directions[-1] != step:
            directions.append(step)
    if directions[0] == directions[-1]:
        directions.pop()
    require(len(directions) == len(set(directions)) == 4, "Retraced or incomplete rectangle")
    return l, t, r, b


def artwork(source):
    from meerk40t.svgelements import SVG, Shape, Path as SvgPath, Move, Line, Close

    xml = ET.fromstring(source)
    # Translate SVG user space to physical millimetres. The renderer handles
    # viewBox origins, aspect ratio and transforms. Resolve the page's physical
    # size independently: this library approximates inches per millimetre.
    factors = {"mm": 1, "cm": 10, "in": 25.4, "pt": 25.4/72, "pc": 25.4/6, "px": 25.4/96, "": 25.4/96}
    def mm(name):
        m = re.fullmatch(r"\s*([+\-\d.eE]+)\s*(mm|cm|in|pt|pc|px)?\s*", xml.attrib[name])
        require(m is not None, f"Unresolved physical SVG {name}")
        return float(m[1]) * factors[m[2] or ""]
    drawing = SVG.parse(BytesIO(source), ppi=96, reify=True, on_error="raise")
    sx, sy = mm("width") / drawing.width, mm("height") / drawing.height
    require(all(v > 0 and math.isfinite(v) for v in (sx, sy)), "Invalid SVG page")
    def xy(p):
        return p.x * sx, p.y * sy
    contours = []
    for node in drawing.elements():
        if not isinstance(node, Shape):
            continue
        for path in SvgPath(node).as_subpaths():
            points = []
            for seg in path:
                require(isinstance(seg, (Move, Line, Close)), "Curved cut contour")
                if not isinstance(seg, Move):
                    require(bool(points) and math.dist(points[-1], xy(seg.start)) < 1e-8, "Broken cut contour")
                points.append(xy(seg.end))
            contours.append(rectangle(points))
    require(len(contours) == 3, f"Expected three cut contours, got {len(contours)}")
    return {"page_mm": [mm("width"), mm("height")], "paths_mm": contours}


def cam(file, windows_first):
    from meerk40t.kernel import Kernel
    from meerk40t.core import core
    from meerk40t.core.units import UNITS_PER_MM
    from meerk40t.device import dummydevice

    k = Kernel("MeerK40t", version("meerk40t"), "HG42 receiver", ansi=False, ignore_settings=True)
    k.add_plugin(core.plugin)
    k.add_plugin(dummydevice.plugin)
    k(partial=True)
    try:
        k.console("service device start dummy 0\n")
        e = k.elements
        e.load(str(file))
        nodes = list(e.elems())
        require(bool(nodes), "No drawing elements imported")
        defaults = [{"operation": op.type, "references": len(op.children)} for op in e.ops()]
        for op in e.ops():
            op.output = False
        # Select all issued vectors for through cutting. No path is omitted,
        # duplicated, moved, scaled, mirrored or kerf-offset by this receiver.
        if windows_first:
            # The receiving operator can assign operations without editing any
            # vector. Do not silently depend on the optimizer's ordering.
            require(len(nodes) == 3, "Three individually selectable contours required for separate operations")
            def contains(outer, inner):
                a, b = outer.bounds, inner.bounds
                return a[0] < b[0] < b[2] < a[2] and a[1] < b[1] < b[3] < a[3]
            outlines = [n for n in nodes if all(n is m or contains(n, m) for m in nodes)]
            require(len(outlines) == 1, "No unique enclosing perimeter")
            groups = [[n for n in nodes if n is not outlines[0]], outlines]
        else:
            groups = [nodes]
        for group in groups:
            cut = e.op_branch.add(type="op cut")
            for n in group:
                cut.add_reference(n)
        k.planner.opt_merge_ops = False
        k.planner.opt_inner_first = True
        k.planner("plan copy preprocess validate blob preopt optimize\n")
        plan = k.planner.default_plan.plan
        require(plan and all(type(p).__name__ == "CutCode" for p in plan), "No stock cut plan")
        paths, points = [], []
        for code in plan:
            for seg in code.flat():
                require(type(seg).__name__ == "LineCut", "Nonlinear CAM move")
                if not points:
                    points = [seg.start]
                require(points[-1] == seg.start, "Broken CAM contour")
                points.append(seg.end)
                if points[-1] == points[0]:
                    mm = [tuple(v / UNITS_PER_MM for v in k.device.view.iposition(*p)) for p in points]
                    paths.append(rectangle(mm))
                    points = []
        require(not points and len(paths) == 3, "Expected three complete, closed CAM paths")
        if windows_first:
            a = paths[-1]
            require(all(a[0] < b[0] < b[2] < a[2] and a[1] < b[1] < b[3] < a[3] for b in paths[:2]),
                    "Planned cuts did not put the two windows before the outline")
        return {"engine": "MeerK40t " + version("meerk40t"), "device": k.device.name, "spooled": False,
                "operation_assignment": "windows, then perimeter" if windows_first else "one cut operation",
                "import_classification": defaults, "entities": [n.type for n in nodes], "path_order_mm": paths}
    finally:
        k()


def corners(center, radius):
    return D(center) - D(radius), D(center) + D(radius)


def fields():
    # Explicit corners of the original dimension chains. Keep W/H and the
    # paired edges from the same card. Do not invent independent values for
    # a derived left edge and a separately tabulated right edge on that card.
    dims_a = [corners("9", ".25"), corners("8", ".20"), corners("22", ".30"), corners("11", ".20")]
    a = [(x, y, x+w, y+h) for x, y, w, h in product(*dims_a)]
    dims_b = [corners("96", ".40"), corners("54", ".30"), corners("8.40", ".35"),
              corners("9.20", ".25"), corners("24.60", ".30"), corners("13.80", ".25")]
    b = [(w-r-i, h-b-j, w-r, h-b) for w, h, r, b, i, j in product(*dims_b)]
    za = list(product(corners("7.45", ".10"), corners("6.75", ".10"), corners("33", ".12"), corners("20.60", ".10")))
    zb = list(product(corners("60.60", ".12"), corners("29", ".10"), corners("89.75", ".10"), corners("46.65", ".12")))
    return ((a, za), (b, zb))


def score(rects):
    require(len(rects) == 3, "Need the three measured rectangles")
    ordered = sorted(rects, key=lambda r: -(r[2]-r[0])*(r[3]-r[1]))
    outline = ordered[0]
    # SVG and machine origins may be somewhere else on the page: the drawing
    # datum is the *finished blank's* top left. Keep the issued front view.
    def decimal(x):
        return D(format(x, ".12g"))
    l, t, r, b = map(decimal, outline)
    dim = (r-l, b-t)
    windows = [tuple(decimal(v) - (l, t)[i%2] for i, v in enumerate(rect)) for rect in sorted(ordered[1:])]
    blank, webs, functional = [], [], []
    for name, size, nominal in zip(("width", "height"), dim, (D(96), D(54))):
        lo, hi = corners(size, ".12")
        blank.append({"axis": name, "finished_range": [lo, hi], "permitted": [nominal-D(".2"), nominal+D(".2")],
                      "pass": lo >= nominal-D(".2") and hi <= nominal+D(".2")})
    for name, window, (prints, zones) in zip(("A", "B"), windows, fields()):
        for side, edge in enumerate(window):
            a = side % 2
            reg = corners("0", (".20", ".15")[a])
            # All inequalities are affine; corners give exact extrema over
            # the full continuous drawing box. No Monte Carlo sampling.
            openings = [edge + cut + slip for cut, slip in product(corners("0", ".10"), reg)]
            for title, boxes, sense in (("visible field", prints, 1), ("inside clear zone", zones, -1)):
                direction = sense if side < 2 else -sense
                val = min(direction*(box[side]-opening) for box, opening in product(boxes, openings))
                functional.append({"window": name, "side": SIDES[side], "test": title,
                                   "minimum_mm": val, "spare_mm": val-D(".15"), "pass": val >= D(".15")})
            web = edge-D(".10") if side < 2 else dim[a]-D(".12")-edge-D(".10")
            webs.append({"window": name, "towards": SIDES[side], "minimum_mm": web, "pass": web >= 3})
    wa, wb = windows
    def gap(axis):
        return max(D(0), max(wa[axis]-wb[axis+2], wb[axis]-wa[axis+2])-D(".20"))
    dx, dy = gap(0), gap(1)
    sep2 = dx*dx + dy*dy
    webs.append({"towards": "between windows", "axis_gaps_mm": [dx, dy], "minimum_mm": sep2.sqrt(), "pass": sep2 >= 9})
    return {"window_edges_mm": dict(zip(("A", "B"), windows)), "blank": blank, "webs": webs, "functional": functional,
            "minimum_functional_mm": min(item["minimum_mm"] for item in functional),
            "pass": all(item["pass"] for item in blank + webs + functional)}


def measure(archive, svg_name=None, windows_first=False):
    data = archive.read_bytes()
    with ZipFile(BytesIO(data)) as z:
        members = {}
        for info in z.infolist():
            name = info.filename
            require(not PurePosixPath(name).is_absolute() and ".." not in PurePosixPath(name).parts and "\\" not in name, "Invalid ZIP name")
            if info.is_dir():
                continue
            require(name not in members, "Duplicate ZIP member")
            members[name] = z.read(info)
    names = [n for n in members if n.lower().endswith(".svg")]
    if svg_name is None:
        require(len(names) == 1, "Select the SVG named in the shop directions")
        svg_name = names[0]
    source = members[svg_name]
    art = artwork(source)
    with TemporaryDirectory() as folder:
        file = Path(folder) / "as-issued.svg"
        file.write_bytes(source)
        plan = cam(file, windows_first)
        require(file.read_bytes() == source, "CAM altered input")
    paths, actual = sorted(art["paths_mm"]), sorted(plan["path_order_mm"])
    deviation = max(abs(a-b) for ra, rb in zip(paths, actual) for a, b in zip(ra, rb))
    geometry = score(art["paths_mm"])
    machine = score(plan["path_order_mm"])
    return {"input_bytes": len(data), "input_sha256": sha256(data).hexdigest(),
            "members": {k: sha256(v).hexdigest() for k, v in members.items()}, "selected_svg": svg_name,
            "artwork": art, "cam": plan, "maximum_cam_edge_difference_mm": deviation,
            "geometry": geometry, "cam_geometry": machine,
            "digital_geometry_pass": deviation < .001 and geometry["pass"] and machine["pass"]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--svg", help="Exact member named by the shop when multiple SVGs were issued")
    parser.add_argument("--windows-first", action="store_true",
                        help="Follow directions requiring two inner cuts before the perimeter")
    args = parser.parse_args()
    require(sha256(Path(__file__).with_name("drawing-C.txt").read_bytes()).hexdigest() == DRAWING, "Different drawing")
    with TemporaryDirectory() as home:
        os.environ.update(HOME=home, XDG_CONFIG_HOME=home, XDG_STATE_HOME=home)
        result = measure(args.archive, args.svg, args.windows_first)
        print(json.dumps(result, indent=2, default=str))
        if not result["digital_geometry_pass"]:
            raise SystemExit(1)
