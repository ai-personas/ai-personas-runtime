# A rear adhesive cut file tested at the drawing limits

**[Take Mara’s original BC-17 cutting package](../examples/solar-bond-strip/BC17-A-cut-package.zip).** The 1:1 SVG for a P124 rear mounting adhesive meets the drawing without edits, including the placement and cut tolerances. It leaves **zero spare clearance** at two sides of the wire exit. A receiving CAM rounded both sides inward: that difference matters if the cutter needs the whole tolerance budget *after* planning.

This is a new adhesive on the **back** of the module for a different exhibit bracket. The [earlier front-face inspection nest](solar-inspection-nest.md#the-cell-edge-qualification) and its possible 0.05 mm cell/ledge overlap remain as issued.

## One invitation in native Studio

The operator restored Mara after her earlier shop review and armed the private epoch observer at event **85**, before the new invitation. The station had two files: the [released shop requirements](../examples/solar-bond-strip/BC-17-A.txt) and Voltaic’s R4G drawing. A fresh download of the [manufacturer PDF](https://voltaicsystems.com/content/P124_dwg.pdf) matched the original 820,539 bytes. The registered two-thread comparison brief was outside the project.

At **86**, Exhibit shop invited Mara once with the actual BC-17 attachment. Oren was available; Mara worked alone. She chose the U-profile, ran the build and checks, and prepared the operator instructions. No further task messages were sent.

| Event | What actually happened |
|---|---|
| **93–95** | The first render could not write its requested temporary PNG and exited 1. Mara selected a writable project path. The resulting **288,869-byte** image of page 1 reached three verified provider inputs. An independent PDF render reproduced its exact digest. |
| **96–100** | One response was rejected for an omitted next-appraisal choice; its proposed operations did not run. Mara proceeded, built the ZIP at **97**, and checked its geometry at **99**. |
| **101–105** | A second rejected response attempted to register a ZIP whose original capture was no longer on that input. Mara found the producing receipt using `command.history`, selected it on the issuing input, and registered the **original** ZIP at **105**. No rebuild, operator correction or recapture was needed. |
| **107–114** | Mara issued the ZIP to **Exhibit shop cutter operator** at **107**. Her receiving check staged that exact file, verified the archive, SVG topology and scale, and exited 0 at **111**. She chose to stop at **114**, about 5 min 21 s from the native run start. |

[All 15 input and terminal-output bindings](../examples/solar-bond-strip/decisions.json) verified against direct `gpt-6.1-sol` / medium / Codex OAuth; **13** responses were accepted. The independent launcher log matched seven persona commands, including the failed render. Reported usage was **303,501 input** (125,952 cached) and **12,914 output** tokens; actual cost unreported. The 273 bytes from **Copy instructions** match the accepted issue, and the native download, original capture, registered asset and separate CLI export all match byte for byte.

## Full tolerance result on the unchanged delivery

The [receiver](../examples/solar-bond-strip/receive.py) reads the actual SVG’s physical dimensions, view box and transformed vectors. In its calculations the artwork datum is the **upper-right** of the centerline bounds. It projects the complete lands and forbidden aisle into this frame over every allowed placement and component dimension. It erodes and dilates the actual polygon with a **square** of half-width 0.15 mm; it does not measure ink thickness or use the issuer’s checker.

| Requirement | Worst result on the issued artwork |
|---|---|
| Manufacturable geometry | One simple, closed, eight-segment profile. Connected core after maximum recession. |
| Outside limits | Left inset **1.00**, right **1.50**, top **1.50**, deepest Y **30.00 mm**. Even the shortest module clears its bottom edge by 82.50 mm. |
| Pad pair and wire exit | Left/right/below-pad clearances **1.00 / 1.10 / 1.00 mm**, including the full gap between pads. Required ≥1.00 mm. |
| Bracket lands | All three covered for all positions; extra clearance to the eroded boundary: **2.00 / 2.50 / 2.00 mm** (left/right/transverse). |
| Area | **1,278.70 mm²** nominal; minimum finished **1,245.85 mm²**, **95.85 mm²** above the requirement. |

For example, the rightmost pad edge can be `27 + 4.3 + 6.3 + 4.3 = 41.9 mm`. Its opening must reach at least 42.9 mm. The left pad can start at 25.0 mm, so the opposite side of that opening must reach at most 24.0 mm. The lower edge of either pad can be `16.5 + 4.3 = 20.8 mm`; the notch must remain open through 21.8 mm. The right bracket land shares the taping datum’s dependence on finished panel width; treating those as independent would test an impossible combination. Front-cell setbacks, electrical ratings and the sheet-2 wafer layout are not datums for this back-only part.

**CAM disposition.** Using the issuer’s instructions (vector centerline, no additional offset, 100%, no mirror), stock MeerK40t produced one closed path on its dummy device. The digital plan remained inside the specified ±0.15 mm process envelope. At a common upper-right datum, its notch’s left wall moved **0.000327 mm** inward and its floor **0.000154 mm** upward. Spending another full ±0.15 mm about *that* path would reduce two clearances to **0.999673** and **0.999846 mm**. The strict post-planning test failed; the drawing specifies **one total** allowance relative to the artwork. [Both readings, all path coordinates and margins](../examples/solar-bond-strip/receiving.json). No knife motion, adhesion, installation or factory acceptance was observed.

## Did a numerical revision help?

**No eligible change.** Both boundary archives survived, and the complete interval **85–114** contained no post-initialization switch. Every response chose `preserve`; all 13 accepted choices used Mara’s epoch-1 baseline. Neither a read of her earlier clearance-method file nor its reference was found in the actual provider inputs. Her checks are evidence of work, not evidence of a numerical benefit. Neither arm of the [registered unseen comparison](../examples/stitch-desk/protocol.json) ran.

Design `7d86efb`; live desktop `96981bf`; [registration](../examples/solar-bond-strip/protocol.json) `6dd2989`. [Full acceptance map](acceptance-map.md)
