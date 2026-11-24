"""Test an aligned vertical loading path against the *issued* STL, in mm."""
import numpy as np
from shapely.geometry import box, LineString

from shop import bearing_faces, material, projection_above


def high_point(mesh, rect):
    """Highest facet point over an XY rectangle, including clipped sloping faces."""
    x0, y0, x1, y1 = rect.bounds
    found = float(mesh.bounds[0, 2])  # also allow for the flat bench below an open frame
    for triangle in mesh.triangles:
        poly = list(triangle)
        for axis, bound, sign in [(0, x0, 1), (1, y0, 1), (0, x1, -1), (1, y1, -1)]:
            clipped = []
            for i, a in enumerate(poly):
                b = poly[(i + 1) % len(poly)]
                ina = sign * (a[axis] - bound) >= 0
                inb = sign * (b[axis] - bound) >= 0
                if ina:
                    clipped.append(a)
                if ina != inb:
                    clipped.append(a + (b - a) * ((bound - a[axis]) / (b[axis] - a[axis])))
            poly = clipped
        if poly:
            found = max(found, max(p[2] for p in poly))
    return found


def footprint(mesh, pocket, seat, short_axis="x", stop_height=0.5, face_margins=(3.2, 3.95)):
    """The four nominal stop coordinates must also be witnessed on a mesh section.

    Support is tested over the entire translation range, not just with a
    nominal panel drawn at the middle. This does not model sag, probe forces,
    printed dimensions or the operator tilting the panel during loading.
    """
    left, bottom, right, top = pocket
    dims_min = (65.5, 112.5)
    dims_max = (66.5, 113.5)
    # Sheet 1: right edge 5.1 +/-0.5; a conservative bound for the
    # opposite margin also stacks the two 27.5 cells and 0.8 gap (+/-0.3).
    # Both short-end borders are at least 3.95 under their drawing callouts.
    borders_min = face_margins
    if short_axis == "y":
        dims_min, dims_max, borders_min = dims_min[::-1], dims_max[::-1], borders_min[::-1]
    width, length = right - left, top - bottom
    x, y = dims_min
    pocket_poly = box(*pocket)
    common = box(right - x, top - y, left + x, bottom + y)
    centers = box(left + x / 2, bottom + y / 2, right - x / 2, top - y / 2)
    # A vertical line from above must miss every polygon above the seating
    # plane throughout the pocket, including any overhanging lips.
    shadow = projection_above(mesh, seat + 1e-5)
    guides = material(mesh, seat + stop_height)
    supports = bearing_faces(mesh, seat)
    shared_support = supports.intersection(common)
    active_sweep = box(left + borders_min[0], bottom + borders_min[1],
                       right - borders_min[0], top - borders_min[1])
    relief = seat - high_point(mesh, active_sweep)
    guide_spans = [
        [(left, top - y), (left, bottom + y)], [(right, top - y), (right, bottom + y)],
        [(right - x, bottom), (left + x, bottom)], [(right - x, top), (left + x, top)],
    ]
    return {
        "pocket_xyxy_mm": pocket, "seat_z_mm": seat, "stop_witness_z_mm": seat + stop_height,
        "opening_mm": [width, length], "panel_min_xy_mm": list(dims_min), "panel_max_xy_mm": list(dims_max),
        "diametral_clearance_largest_mm": [round(float(v), 6) for v in np.subtract([width, length], dims_max)],
        "shadow_in_opening_mm2": round(shadow.intersection(pocket_poly).area, 6),
        "nearest_guide_to_each_stop_mm": [round(guides.distance(LineString(line)), 6) for line in guide_spans],
        "support_common_to_all_poses_mm2": round(shared_support.area, 6),
        "support_hull_contains_all_panel_centers": shared_support.convex_hull.contains(centers),
        "swept_cell_border_min_xy_mm": list(borders_min),
        "cell_field_intersects_contact": active_sweep.intersects(supports),
        "contact_to_swept_cell_field_mm": round(active_sweep.distance(supports), 6),
        "relief_to_fixture_or_flat_bench_under_swept_cells_mm": round(float(relief), 6),
        "remaining_after_0_8_mm_thickness_difference_mm": round(float(relief - 0.8), 6),
    }


def left_cell_case(mesh):
    """An explicit width stack, seated at the left stop (a mirrored pose is equivalent)."""
    seat = bearing_faces(mesh, 12)
    panel_width = 65.5
    cell_width = 27.5 + 0.3
    column_gap = 0.8 + 0.3
    right_border = 5.1 + 0.5
    left_border = panel_width - right_border - 2 * cell_width - column_gap
    x0 = -33.75 + left_border
    y1 = 113 / 2 - 4.45
    cell = box(x0, y1 - 20.18, x0 + cell_width, y1)
    overlap = seat.intersection(cell)
    return {"panel_width_mm": panel_width, "each_cell_width_mm": cell_width, "column_gap_mm": column_gap,
            "dimensioned_right_margin_mm": right_border, "derived_left_margin_mm": round(left_border, 6),
            "seat_overlap_depth_mm": round(max(0, -30.5 - x0), 6),
            "first_cell_projected_on_bearing_face_mm2": round(overlap.area, 6)}
