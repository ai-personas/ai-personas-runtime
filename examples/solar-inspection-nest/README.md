# Receive the P124 inspection nest

**[Download Mara’s original shop ZIP](P124_inspection_nest.zip).** Unpack it and follow its `README.txt`. The one-piece STL slices unchanged at 100% in millimetres. The STEP and Python source are for editing or regeneration; neither is needed to slice it.

**Scope:** digital first-print readiness and nominal module fit passed. A conservative tolerance combination leaves a possible **0.05 mm overlap** of one cell footprint with the seat. Cell-free contact over the entire production range and physical fit are unverified. [Shop findings and drawing interpretation](../../docs/solar-inspection-nest.md)

SHA-256: `8cbb0399d222bf7166da88bf2f8d08182e496b30cdcdbb6bf79a5e6cb4f6252c` · 20,402 bytes · six files. [Exact shop dispatch](shop-dispatch.txt) · [Original reply](reply.md) · [Trial protocol](protocol.json) · [Verified decisions](decisions-and-runs.json) · [Independent measurements](verification.json)

### Replay the independent intake

```sh
python3 examples/solar-inspection-nest/receive.py
python3 examples/solar-inspection-nest/receive.py --mesh
python3 examples/solar-inspection-nest/receive.py --rebuild --panel-step /path/to/P124_3D.stp
python3 examples/solar-inspection-nest/receive.py --cura /path/to/CuraEngine \
  --definition /path/to/fdmprinter.def.json
```

The first command needs only Python. Mesh analysis used trimesh 4.12.2, Shapely 2.1.2, NumPy 2.2.6, SciPy, networkx and rtree. `--rebuild` explicitly runs the original `model.py` in a temporary directory and compares both CAD outputs; use CadQuery 2.6.1 / OCP 7.8.1.1.post1 for the recorded run. `--panel-step` uses the pinned [Voltaic model](https://voltaicsystems.com/content/P124_3D.stp). `--cura` used stock CuraEngine 4.13.0 with matching `fdmprinter.def.json` and `fdmextruder.def.json` in the same directory. [Exact slicer sources](slicer-sources.json)

`verification.json` beside this README contains the independent intake. The different file of the same name **inside the ZIP** is Mara’s original verification. [Studio’s archive-member measurements](mesh-receipts.json) are from a separate, stopped copy.
