# Solar display plate and receiving card

These are the unchanged files issued by the Studio personas in the [field-packet trial](../../docs/field-packet-trial.md).

**[Take the volunteer field packet](volunteer-field-packet.zip)** — the original Studio download from Ada’s still-open pickup request. Open `Request.html` for Ada’s instructions and take the SVG in `Files.zip` to the shop.

- [`solar-garden-plate.svg`](solar-garden-plate.svg) — millimetre-scale vector cut geometry: one 90 × 40 mm plate, with two 4 mm holes on 74 mm centres.
- [`inspection-card.pdf`](inspection-card.pdf) — a one-page A4 card for recording actual measurements after cutting.

**The unchanged SVG passed cutter-software intake.** MeerK40t 0.9.9100 initially classified the black strokes as raster. Ada’s packet explicitly says to use vector cuts and cut the holes first. With those ordinary shop settings, the stock planner produced two closed 4 mm paths followed by the closed 90 × 40 mm outline. All three path dimensions matched within 0.001 mm in the digital plan. No drawing edits, rescaling, physical cutting or pickup occurred. The dimensions on the card are nominal.

[Actual planned paths and original issue](cam-intake.json) · [Replay the digital intake](cam.py) with `python3 examples/solar-display/cam.py` in an environment containing `meerk40t==0.9.9100` and NumPy. The replay uses the application’s actual SVG importer and optimizer with its dummy output device.
