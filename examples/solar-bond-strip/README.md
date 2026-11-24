# P124 rear adhesive · BC-17 Rev A

**[Original cutter ZIP](BC17-A-cut-package.zip)** · [Mara’s dispatch](shop-dispatch.txt) · [Shop drawing](BC-17-A.txt) · [Study and operator findings](../../docs/solar-bond-strip.md).

The one-piece SVG meets the shop drawing on the full specified tolerance stack. Inside the ZIP, `PLACEMENT.txt` gives the orientation and datum; `DIMENSIONAL-CHECK.txt` is the issuer’s calculation. The two limit conditions at the wire aisle leave no extra margin for errors **outside** the stated allowance.

| Independent reading | Evidence |
|---|---|
| Exact SVG, 62.00 × 27.50 mm | One closed contour, three lands completely covered, minimum finished area 1,245.85 mm². Worst left / right / lower pad clearances: 1.000 / 1.100 / 1.000 mm. |
| Stock vector plan | The direct import initially classified black as raster. Selecting the specified vector cut yielded one closed eight-segment path without editing the SVG. If the full ±0.15 mm is still spent **after** coordinate rounding, left / lower clearances are 0.999673 / 0.999846 mm. That stricter test fails. No machine was driven. |
| Native issue | 2,676 bytes, SHA-256 `d18b1b85e90867741368b7809827a8d197079c5a24d42a33504204bb7d8e0758`; [verified provider inputs, outputs, commands and epoch archives](decisions.json). |

To reproduce the [receiver output](receiving.json), install `meerk40t==0.9.9100` and `shapely==2.1.2` and run:

```sh
python3 examples/solar-bond-strip/receive.py examples/solar-bond-strip/BC17-A-cut-package.zip
```

It reads the unedited SVG and stock planner coordinates independently of the issuer’s calculations. **Expected exit: 1** because `specified_artwork_pass` is true and `additional_full_allowance_about_planned_path_pass` is false. The reported intrusion area refers to the *union* of permitted aisle locations, not a measured collision on one panel. Neither calculation measures adhesion or an actual cut.

[Protocol registered before admission](protocol.json). The original [P124 component drawing](https://voltaicsystems.com/content/P124_dwg.pdf) is pinned by SHA-256 in the decision audit. Applicable rear-view features are on sheet 1. The shop drawing defines the bracket and process. An operator holding the private station can run the [offline audit](audit.py): `python3 audit.py /path/to/ai-personas /path/to/station`. It starts no inference or machining.
