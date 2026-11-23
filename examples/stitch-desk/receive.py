"""Read the ordinary job as received. Requires stock pyembroidery==1.5.1.

No workspace files, generator or issuer's verification JSON enter the measurements.
Coordinates below are viewed from the front, +X right and +Y up.
"""
from collections import Counter
from hashlib import sha256
from importlib.metadata import version
from io import BytesIO
import json
from math import dist
from pathlib import Path
import sys
from zipfile import ZipFile

import pyembroidery as emb


def section(segments, x):
    """Intersections of the sewn outline with a vertical line (mm)."""
    ys = set()
    for (xa, ya), (xb, yb) in segments:
        if min(xa, xb) < x < max(xa, xb):
            ys.add(round(ya + (yb - ya) * (x - xa) / (xb - xa), 4))
    return sorted(ys)


def receive(path):
    with ZipFile(path) as z:
        assert not z.testzip() and len(z.namelist()) == len(set(z.namelist()))
        members = {n: {"bytes": len(b), "sha256": sha256(b).hexdigest()}
                   for n in z.namelist() for b in [z.read(n)]}
        # Filename taken from the volunteer's actual dispatch/README.
        data = z.read("RIGHT_RETURN.dst")
        directions = z.read("README.txt").decode()
    # Use native machine moves: disable inferred trim insertion and jump clipping.
    p = emb.read_dst(BytesIO(data), settings={"trim_at": None, "clipping": False})
    assert data[-3:] == bytes.fromhex("0000f3") and (len(data) - 512) % 3 == 0
    counts = Counter()
    needle, moves, segments, blocks = [], [(0, 0)], [], set()
    prev, block, jump_end = None, 0, None
    for x, y, c in p.stitches:
        cmd = c & emb.COMMAND_MASK
        point = (x / 10, -y / 10)
        moves.append(point)
        counts[cmd] += 1
        if cmd == emb.STITCH:
            needle.append(point)
            blocks.add(block)
            if prev is not None:
                segments.append((prev, point))
            prev = point
        else:
            assert cmd in (emb.END, emb.JUMP, emb.COLOR_CHANGE)
            if cmd == emb.COLOR_CHANGE:
                block += 1
            if cmd == emb.JUMP and not needle:
                jump_end = point
            prev = None
    xs, ys = zip(*needle)
    l, r, b, t = min(xs), max(xs), min(ys), max(ys)
    width, height = r - l, t - b
    # Recover tail and arrowhead from actual outline crossings, including both
    # sloping flanks. Duplicate passes on the same sewn edge count once.
    tail_points = sorted(set(y for x, y in needle if x == l))
    tail_height = max(tail_points) - min(tail_points)
    tail_sections = [section(segments, l + off) for off in (0.25, 1.25, 3.25, 5.25)]
    head_sections = [section(segments, r - off) for off in (8.5, 4.5, 0.5)]
    tip = sorted(set(y for x, y in needle if x == r))
    checks = {
        "sewn_size": 31 <= width <= 33 and 15 <= height <= 17,
        "origin_center": abs((l + r) / 2) <= 0.5 and abs((t + b) / 2) <= 0.5,
        "straight_left_tail": 5 <= tail_height <= 7 and
            all(v == [min(tail_points), max(tail_points)] for v in tail_sections),
        "rightward_point": len(tip) == 1 and all(len(s) == 2 for s in head_sections) and
            head_sections[0][0] < head_sections[1][0] < head_sections[2][0] < tip[0] <
            head_sections[2][1] < head_sections[1][1] < head_sections[0][1],
        "single_sewn_block_no_changes": len(blocks) == 1 and counts[emb.COLOR_CHANGE] == 0,
        "all_moves_fit_field": all(abs(x) <= 30 and abs(y) <= 20 for x, y in moves),
    }
    return {"zip": path.name, "zip_sha256": sha256(path.read_bytes()).hexdigest(), "members": members,
            "reader": "pyembroidery " + version("pyembroidery"), "units": "mm, +X right, +Y up",
            "sewn_bounds_lbrt": [l, b, r, t], "sewn_size": [width, height],
            "center": [(l + r) / 2, (b + t) / 2], "tail_height": tail_height,
            "left_side_y_range": [min(tail_points), max(tail_points)], "tail_sections_y": tail_sections,
            "rightmost_y": tip, "head_sections_y": head_sections, "blocks_with_stitches": len(blocks),
            "stitches": counts[emb.STITCH], "jumps": counts[emb.JUMP], "changes": counts[emb.COLOR_CHANGE],
            "end": counts[emb.END], "last_lead_in_jump": jump_end, "first_needle": needle[0],
            "longest_sewn_segment": max(dist(*pair) for pair in segments), "checks": checks,
            "operator_text_sha256": sha256(directions.encode()).hexdigest()}


if __name__ == "__main__":
    result = receive(Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("right_return_operator.zip"))
    print(json.dumps(result, ensure_ascii=False, indent=2))
    sys.exit(not all(result["checks"].values()))
