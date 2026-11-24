"""Independent shop measurements. Treat STL coordinates as mm only when the issuer says so."""
from collections import Counter
from hashlib import sha256
from pathlib import Path
import io
import os
import re
import subprocess

import numpy as np
import trimesh
from shapely.geometry import Polygon
from shapely.ops import unary_union


def read_stl(data):
    # STL repeats its vertices for every triangle. Index *equal* triples only;
    # do not weld by distance, fill holes, change winding, scale or rotate.
    source = trimesh.load_mesh(io.BytesIO(data), file_type="stl", process=False)
    corners = np.asarray(source.triangles)
    assert corners.size and np.isfinite(corners).all()
    points, index = np.unique(corners.reshape(-1, 3), axis=0, return_inverse=True)
    return trimesh.Trimesh(vertices=points, faces=index.reshape(-1, 3), process=False)


def inspect_solid(mesh):
    pairs, counts = np.unique(mesh.edges_sorted, axis=0, return_counts=True)
    labels = trimesh.graph.connected_component_labels(mesh.face_adjacency, node_count=len(mesh.faces))
    return {
        "triangles": len(mesh.faces), "vertices": len(mesh.vertices), "edges": len(pairs),
        "bounds_mm": mesh.bounds.tolist(), "extent_mm": mesh.extents.tolist(),
        "zero_area_faces": int((mesh.area_faces == 0).sum()),
        "duplicate_faces": len(mesh.faces) - len(np.unique(np.sort(mesh.faces, axis=1), axis=0)),
        "edges_without_two_incident_faces": int((counts != 2).sum()),
        "edge_connected_shells": int(len(np.unique(labels))),
        "consistent_winding": bool(mesh.is_winding_consistent),
        "watertight": bool(mesh.is_watertight), "signed_volume_mm3": float(mesh.volume),
    }


def facet_fingerprint(mesh):
    """Ignore STL headers, facet order and cyclic starts, never winding or scale."""
    rows = []
    for triangle in mesh.triangles:
        pts = [tuple(0.0 if v == 0 else float(v) for v in p) for p in triangle]
        rows.append(min(tuple(pts[i:] + pts[:i]) for i in range(3)))
    return sha256(np.asarray(sorted(rows), dtype="<f8").tobytes()).hexdigest()


def material(mesh, z):
    """Even–odd section in the STL's XY frame, at a plane away from vertices."""
    path = mesh.section(plane_origin=[0, 0, z], plane_normal=[0, 0, 1])
    result = Polygon()
    if path is not None:
        for loop in path.discrete:
            assert np.allclose(loop[0], loop[-1], atol=1e-8, rtol=0)
            region = Polygon(loop[:, :2])
            assert region.is_valid, f"Non-simple section at z={z}"
            result = result.symmetric_difference(region)
    return result


def projection_above(mesh, z):
    """XY shadow of original facets clipped to Z >= z (vertical approach test)."""
    polys = []
    for triangle in mesh.triangles:
        clipped = []
        for i, a in enumerate(triangle):
            b = triangle[(i + 1) % 3]
            if a[2] >= z:
                clipped.append(a[:2])
            if (a[2] >= z) != (b[2] >= z):
                clipped.append((a + (b - a) * ((z - a[2]) / (b[2] - a[2])))[:2])
        if len(clipped) >= 3:
            poly = Polygon(clipped)
            if poly.area:
                polys.append(poly)
    return unary_union(polys)


def bearing_faces(mesh, z):
    """Horizontal upward faces at the seating plane; excludes any lower floor."""
    mask = (mesh.face_normals[:, 2] > 0.999999) & np.all(
        np.isclose(mesh.triangles[:, :, 2], z, atol=1e-5, rtol=0), axis=1)
    return unary_union([Polygon(t[:, :2]) for t in mesh.triangles[mask]])


def upward_growth(mesh, layer_height):
    """Material outside the previous sample, allowing a 1:1 overhang slope.

    A positive value calls for interpretation (e.g. a deliberate short bridge).
    This is a sampled shape comparison, not a strength or adhesion test.
    """
    low, top = mesh.bounds[:, 2]
    heights = np.arange(low + layer_height * 0.4999, top, layer_height)
    last = None
    worst = 0.0
    count = 0
    for z in heights:
        shape = material(mesh, z)
        if last is not None:
            area = shape.difference(last.buffer(layer_height, join_style="mitre")).area
            worst = max(worst, area)
            count += area > 0.0001
        last = shape
    return {"compared_pairs": len(heights) - 1, "pairs_over_0_0001_mm2": count,
            "max_area_beyond_45_degree_step_mm2": round(worst, 6)}


def summarize_gcode(text):
    """Read actual Marlin moves, rather than CuraEngine's placeholder file header."""
    absolute_xyz = absolute_e = True
    xyz = np.zeros(3)
    e = 0.0
    layer = None
    feature = ""
    by_layer = Counter()
    by_type = Counter()
    low = np.full(3, np.inf)
    high = -low.copy()
    extruded = 0.0
    printed_layer_z = {}
    for line in text.splitlines():
        if line.startswith(";LAYER:"):
            layer = int(line[7:])
            feature = ""
        elif line.startswith(";TYPE:"):
            feature = line[6:].strip()
        cmd, *rest = line.partition(";")[0].split() or [""]
        if cmd == "G90":
            absolute_xyz = True
        elif cmd == "G91":
            absolute_xyz = False
        elif cmd == "M82":
            absolute_e = True
        elif cmd == "M83":
            absolute_e = False
        elif cmd in ("G0", "G1", "G92"):
            args = {x[0]: float(x[1:]) for x in rest if x[0] in "XYZE" and len(x) > 1}
            old = xyz.copy()
            old_e = e
            for i, axis in enumerate("XYZ"):
                if axis in args:
                    xyz[i] = args[axis] if absolute_xyz or cmd == "G92" else xyz[i] + args[axis]
            if "E" in args:
                e = args["E"] if absolute_e or cmd == "G92" else e + args["E"]
            if cmd != "G92" and layer is not None and e > old_e and np.any(xyz[:2] != old[:2]):
                low = np.minimum(low, np.minimum(old, xyz))
                high = np.maximum(high, np.maximum(old, xyz))
                by_layer[layer] += 1
                by_type[feature] += 1
                extruded += e - old_e
                printed_layer_z.setdefault(layer, set()).add(xyz[2])
    assert by_layer and all(len(z) == 1 for z in printed_layer_z.values())
    return {"deposition_moves": sum(by_layer.values()), "nonempty_layers": len(by_layer),
            "layer_numbers": [min(by_layer), max(by_layer)], "continuous_layers": len(by_layer) == max(by_layer) - min(by_layer) + 1,
            "toolpath_center_bounds_mm": [low.tolist(), high.tolist()],
            "deposition_features": dict(sorted(by_type.items())), "extruded_filament_mm": round(extruded, 3)}


def slice_file(binary, definition, stl, gcode, settings):
    # No model transformation or mesh repair. Centering translates the input
    # onto the bed; the STL coordinate scale and orientation stay unchanged.
    args = [str(binary), "slice", "-m2", "-j", str(definition)]
    for key, value in settings.items():
        args += ["-s", f"{key}={value}"]
    args += ["-e0", "-s", "machine_nozzle_size=0.4", "-s", "material_diameter=1.75",
             "-l", str(stl), "-o", str(gcode)]
    env = dict(os.environ, CURA_ENGINE_SEARCH_PATH=str(Path(definition).parent))
    original = stl.read_bytes()
    proc = subprocess.run(args, env=env, capture_output=True, text=True, timeout=150)
    if proc.returncode:
        raise RuntimeError((proc.stderr + proc.stdout)[-2400:])
    assert stl.read_bytes() == original
    text = gcode.read_text()
    version = re.search(r";Generated with (Cura_SteamEngine [^\n]+)", text).group(1)
    return {"slicer": version, "exit": proc.returncode, "input_sha256": sha256(original).hexdigest(),
            "settings": settings, **summarize_gcode(text)}
