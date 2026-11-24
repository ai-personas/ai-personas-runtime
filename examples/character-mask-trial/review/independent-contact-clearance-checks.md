# Independent contact-clearance checks

Use when a fixture, nest or support must avoid a fragile or forbidden region of a toleranced part.

1. Map each intended contact and each forbidden region to its own side and datum. Distinguish explicit drawing dimensions from inferred dimensions and assumptions. Do not mirror a setback labeled on only one side.
2. Derive the available border on each side independently from the stated constraints. Include overall dimensions, feature sizes, gaps, offsets and their allowed combinations. Keep correlated dimensions correlated; do not independently stack combinations that cannot coexist.
3. Compare the minimum available border with the maximum inward reach of the support at the same datum. Record the signed clearance and the governing combination. A negative clearance indicates possible overlap, even when the nominal assembly fits.
4. Separate drawing-based clearance from manufacturing, placement, warpage and load allowances. If an allowance or datum relationship is unknown, label it unresolved rather than treating it as zero or declaring a guaranteed safe fit.
5. Check all contact sides, including asymmetric cases, against an independently derived table or geometric construction. Mesh validity, correct scale, successful slicing and nominal fit do not establish worst-case contact clearance.
6. Preserve exact-package checks, source regeneration checks, manifold/scale checks and clear statements of unperformed physical validation. These remain useful but answer different questions.

## Motivating observation and scope
The receiving desk reported that the original P124 nest sliced at full scale and fitted the nominal module digitally. It also reported that the drawing's width-side cell setback was labeled on the right only: an independent allowed stack could leave a 3.20 mm left border against 3.25 mm of seat, permitting 0.05 mm overlap. No print or damage was observed. This resource retains a checking method, not a repaired design, independently reproduced measurement, physical validation or demonstrated improvement on a later job. The original issue and disposition remain unchanged.
