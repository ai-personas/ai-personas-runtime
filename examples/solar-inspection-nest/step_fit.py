"""Place Voltaic's *nominal* reference assembly on the issued STEP nest."""
from pathlib import Path
from hashlib import sha256
import cadquery as cq
from shapely.geometry import box
from shapely.ops import unary_union

from shop import bearing_faces


def nominal_fit(nest_step, panel_step, mesh):
    manufacturer = "4df76ee10f0e8657e4fa3d43d5c0deefb3c840cc11203657527cf07ac666287b"
    panel_step = Path(panel_step)
    assert sha256(panel_step.read_bytes()).hexdigest() == manufacturer
    nest, = cq.importers.importStep(str(nest_step)).solids().vals()
    panel = cq.importers.importStep(str(panel_step)).val().rotate((0, 0, 0), (1, 0, 0), 90)
    body = panel.BoundingBox()
    cx = (body.xmin + body.xmax) / 2
    cy = (body.ymin + body.ymax) / 2
    panel = panel.translate((-cx, -cy, 12 - body.zmin))
    # In this reference assembly the ten thin rectangular cell solids are
    # distinguishable from the 64 mm PCB and the two encapsulation solids.
    cells = [s for s in panel.Solids() if s.BoundingBox().zlen < 0.11]
    assert len(cells) == 10
    bearings = bearing_faces(mesh, 12)
    poses = []
    for dx in (-0.75, 0, 0.75):
        for dy in (-0.75, 0, 0.75):
            outlines = []
            for s in cells:
                r = s.BoundingBox()
                outlines.append(box(r.xmin + dx, r.ymin + dy, r.xmax + dx, r.ymax + dy))
            sample = panel.translate((dx, dy, 0))
            poses.append({"offset_xy_mm": [dx, dy],
                          "intersecting_volume_mm3": round(nest.intersect(sample).Volume(), 6),
                          "cell_to_seat_xy_mm": round(unary_union(outlines).distance(bearings), 6)})
    return {"source_sha256": manufacturer, "step_timestamp": "2023-09-12T17:12:54Z",
            "panel_extent_xy_thickness_mm": [round(v, 6) for v in (body.xlen, body.ylen, body.zlen)],
            "panel_solids": len(panel.Solids()), "cell_solids": len(cells), "poses": poses}
