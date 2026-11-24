"""Replay the shop intake on the original ZIP. Does not write to it or send a job."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile
import zipfile


HERE = Path(__file__).resolve().parent


def digest(data):
    return hashlib.sha256(data).hexdigest()


def members(archive):
    result = {}
    for info in archive.infolist():
        if info.is_dir():
            continue
        name = info.filename
        path = PurePosixPath(name)
        if path.is_absolute() or ".." in path.parts or "\\" in name or name in result:
            raise ValueError(f"Ambiguous package path {name!r}")
        result[name] = archive.read(info)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--zip", type=Path, help="An original download to compare")
    parser.add_argument("--mesh", action="store_true", help="Measure the actual solid and panel fit")
    parser.add_argument("--cura", type=Path, help="Also slice with this CuraEngine executable")
    parser.add_argument("--definition", type=Path, help="Path to Cura's stock fdmprinter.def.json")
    parser.add_argument("--panel-step", type=Path, help="Also place Voltaic's pinned nominal STEP on the issued nest")
    parser.add_argument("--rebuild", action="store_true", help="Execute the verified original model.py in a new directory and compare CAD outputs")
    args = parser.parse_args()
    if bool(args.cura) != bool(args.definition):
        parser.error("Supply --cura and --definition together")

    record = json.loads((HERE / "verification.json").read_bytes())
    issued = record["original_zip"]
    path = args.zip or HERE / issued["file"]
    data = path.read_bytes()
    assert len(data) == issued["bytes"] and digest(data) == issued["sha256"]
    with zipfile.ZipFile(path) as z:
        files = members(z)
    assert {k: digest(v) for k, v in files.items()} == issued["member_sha256"]
    print(f"{len(data):,} original ZIP bytes; {len(files)} unchanged files.")
    if args.mesh or args.cura or args.panel_step or args.rebuild:
        from fit import footprint, left_cell_case
        from shop import read_stl, inspect_solid, facet_fingerprint, upward_growth
        g = record["geometry"]
        mesh = read_stl(files[g["path"]])
        assert inspect_solid(mesh) == g["solid"]
        assert facet_fingerprint(mesh) == g["oriented_facets_sha256"]
        growth = upward_growth(mesh, record["layer_height_mm"])
        assert growth == g["support_free_sampling"]
        fit = footprint(mesh, **record["fixture_parameters"])
        assert fit == record["aligned_fit"]
        conditional = footprint(mesh, **record["fixture_parameters"], face_margins=(4.6, 3.95))
        assert conditional == record["if_first_margin_is_bilateral"]
        edge_case = left_cell_case(mesh)
        assert edge_case == record["one_sided_width_stack"]
        print("Mesh dimensions (mm):", mesh.extents.tolist())
        print("Clearance for maximum panel, by axis (mm):", fit["diametral_clearance_largest_mm"])
        print("Giving both long edges the stated side setback, minimum gap (mm):",
              conditional["contact_to_swept_cell_field_mm"])
        print("Without assuming bilateral cell centering, possible projected overlap (mm):",
              edge_case["seat_overlap_depth_mm"])
    if args.cura:
        from shop import slice_file
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            model = root / PurePosixPath(record["geometry"]["path"]).name
            model.write_bytes(files[record["geometry"]["path"]])
            run = slice_file(args.cura.resolve(), args.definition.resolve(), model, root / "measured.gcode",
                             record["slicer"]["settings"])
            for k in ("slicer", "input_sha256", "nonempty_layers", "layer_numbers", "continuous_layers",
                      "toolpath_center_bounds_mm", "deposition_features", "deposition_moves"):
                assert run[k] == record["slicer"][k], (k, run[k], record["slicer"][k])
            print(f"{run['slicer']}: {run['nonempty_layers']} deposited layers, {run['deposition_moves']} extrusion moves.")
    if args.rebuild or args.panel_step:
        import cadquery as cq
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            step = root / "as-issued.step"
            step.write_bytes(files["P124_nest.step"])
            if args.panel_step:
                from step_fit import nominal_fit
                panel = nominal_fit(step, args.panel_step, mesh)
                assert panel == record["manufacturer_nominal_step"]
                print(f"Nominal manufacturer model: {len(panel['poses'])} aligned positions; minimum cell-to-seat gap "
                      f"{min(x['cell_to_seat_xy_mm'] for x in panel['poses'])} mm.")
            if args.rebuild:
                script = root / "model.py"
                script.write_bytes(files["model.py"])
                subprocess.run([sys.executable, script], cwd=root, check=True, timeout=120)
                regen = (root / "P124_nest.stl").read_bytes()
                assert digest(regen) == issued["member_sha256"]["P124_nest.stl"]
                a, = cq.importers.importStep(str(step)).solids().vals()
                b, = cq.importers.importStep(str(root / "P124_nest.step")).solids().vals()
                assert a.isValid() and b.isValid() and a.cut(b).Volume() + b.cut(a).Volume() < 1e-7
                print("Relocated source regenerated the identical STL and an equivalent single STEP solid.")
    print(record["disposition"])


if __name__ == "__main__":
    main()
