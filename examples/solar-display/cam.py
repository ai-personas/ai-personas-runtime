"""Import Ada's unchanged SVG and inspect the actual MeerK40t CAM plan.

Run with MeerK40t 0.9.9100 installed. Only the core CAM and dummy-device
plugins are loaded; computing the plan sends nothing to a device.
"""
import hashlib
import json
import os
from pathlib import Path
import sys
from tempfile import TemporaryDirectory

import numpy as np


def intake(source):
    from importlib.metadata import version
    from meerk40t.kernel import Kernel
    from meerk40t.core import core
    from meerk40t.core.units import UNITS_PER_MM
    from meerk40t.device import dummydevice

    before = source.read_bytes()
    digest = hashlib.sha256(before).hexdigest()
    assert digest == "4c44ea41fadea48555314d50fdec58c06a238768983b687c93c020234f22275f"
    kernel = Kernel("MeerK40t", version("meerk40t"), "Field-packet receiver", ansi=False, ignore_settings=True)
    kernel.add_plugin(core.plugin)
    kernel.add_plugin(dummydevice.plugin)
    kernel(partial=True)
    try:
        kernel.console("service device start dummy 0\n")
        elements = kernel.elements
        elements.load(str(source))
        artwork = list(elements.elems())
        original = [[float(v) / UNITS_PER_MM for v in n.bounds] for n in artwork]
        defaults = [{"operation": op.type, "references": len(op.children)} for op in elements.ops()]
        # The actual field packet calls for vector cutting, holes first, then
        # the outline. This is operation assignment, not drawing manipulation.
        for op in elements.ops():
            op.output = False
        for kind, count in [("elem ellipse", 2), ("elem rect", 1)]:
            drawings = [n for n in artwork if n.type == kind]
            assert len(drawings) == count
            cut = elements.op_branch.add(type="op cut")
            for n in drawings:
                cut.add_reference(n)
        kernel.planner.opt_merge_ops = False
        kernel.planner.opt_inner_first = True
        kernel.planner("plan copy preprocess validate blob preopt optimize\n")
        operations = kernel.planner.default_plan.plan
        assert operations and all(type(c).__name__ == "CutCode" for c in operations)
        paths = []
        current = []
        for code in operations:
            for segment in code.flat():
                assert type(segment).__name__ in ("LineCut", "CubicCut")
                assert not current or current[-1].end == segment.start, "Open CAM path"
                current.append(segment)
                if current[0].start == current[-1].end:
                    paths.append(current)
                    current = []
        assert not current and len(paths) == 3
        rows = []
        for path in paths:
            # Sample the curves *after* the import, plan and integer coordinate
            # conversion. Do not derive toolpath dimensions from the SVG tags.
            def points(s):
                a, b = np.array(s.start), np.array(s.end)
                if type(s).__name__ == "LineCut":
                    return [a, b]
                t = np.linspace(0., 1., 257)[:, None]
                r = 1 - t
                return r**3 * a + 3*r*r*t * np.array(s.c1()) + 3*r*t*t * np.array(s.c2()) + t**3 * b
            pts = np.array([kernel.device.view.iposition(*xy)
                            for s in path for xy in points(s)]) / UNITS_PER_MM
            lo, hi = pts.min(axis=0), pts.max(axis=0)
            circle = all(type(s).__name__ == "CubicCut" for s in path)
            center = (lo + hi) / 2
            record = {"closed": True, "kind": "hole" if circle else "outline", "segments": len(path),
                      "center_mm": center.round(6).tolist(), "span_mm": (hi - lo).round(6).tolist(),
                      "bounds_mm": [lo.round(6).tolist(), hi.round(6).tolist()]}
            if circle:
                record["max_sampled_radius_error_mm"] = round(float(np.max(np.abs(
                    np.linalg.norm(pts - center, axis=1) - 2))), 6)
            rows.append(record)
        assert [r["kind"] for r in rows] == ["hole", "hole", "outline"]
        holes = sorted(rows[:2], key=lambda r: r["center_mm"])
        for r, cx in zip(holes, (8, 82)):
            assert np.allclose(r["span_mm"], [4, 4], atol=.001, rtol=0)
            assert np.allclose(r["center_mm"], [cx, 20], atol=.001, rtol=0)
            assert r["max_sampled_radius_error_mm"] < .001
        assert np.allclose(rows[2]["bounds_mm"], [[0, 0], [90, 40]], atol=.001, rtol=0), rows[2]
        assert np.allclose(original, [[v / UNITS_PER_MM for v in n.bounds] for n in artwork])
        assert source.read_bytes() == before
        return {"cam": "MeerK40t " + version("meerk40t"), "input_sha256": digest,
                "unmodified_input_bytes": len(before), "automatic_import_classification": defaults,
                "operations_selected_from_issuer_instructions": ["cut both holes", "cut outline"],
                "plan": rows, "device": kernel.device.name, "spooled": False, "physical_cut": False}
    finally:
        kernel()


if __name__ == "__main__":
    file = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).with_name("solar-garden-plate.svg")
    with TemporaryDirectory() as home:
        os.environ.update(HOME=home, XDG_CONFIG_HOME=home, XDG_STATE_HOME=home)
        print(json.dumps(intake(file), indent=2))
