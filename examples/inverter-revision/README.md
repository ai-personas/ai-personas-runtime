# Receive the revised bench inverter

**Manufacturing hold.** [Download Arden’s exact edition](inverter-revision-manufacturing-hold.zip) for engineering review. The editable sources and placement data improved, but three vias still open into solder paste on a board specified for ordinary tented vias. [Read Cleo’s exact buyer reply](cleo-receiving-verdict.md) or the [full comparison](../../docs/inverter-shop-return.md).

| Evidence | Contents |
|---|---|
| [Protocol](protocol.json) | New supplier brief, process, starting history, independent receiver brief and stopping boundaries |
| [Receiving measurements](verification.json) | Native download digest, member tree digest, regenerated fabrication file digests, drill hits, source parity, process and editor observations |
| [Parts](parts.csv) · [placement references](placement-reference.json) | Per-designator supplier and MPN matches, stock, placement coordinates and numbered public land-pattern centers with dated source links |
| [Operating basis](operating-basis.json) | Calculation from the released board, not physical bench data |
| [Decisions and receiving runs](decisions-and-runs.json) | Input, output and receipt digests for all 121 calls; the seven exact-edition runs and their issuing decision |

From the checkout, the first command checks the original download and order-file references with Python alone. For the second, use stock KiCad 9.0.9 with its symbol and footprint libraries; `--plots` also requires `gerbonara` (verified with 1.5.0) and `shapely`. The optional `--catalog` takes the [pinned supplier database](../inverter-successor/README.md).

```sh
python3 examples/inverter-revision/recheck.py
python3 examples/inverter-revision/recheck.py --plots --kicad kicad-cli --catalog /path/to/supplier.sqlite
```

Observed on the separate receiving toolchain:

```text
Original download: 1,814,885 bytes; 868 exact files; 10690706a5f7cb5e01869db3fdd1ed377eb234e2f31e2c121b95dc72022df9af
PCB: 18 fitted references; BOM: 18; placement: 18
Drills: 23; hits intersecting top paste: 3
Part-specific pad-center projections match the recorded top-copper observations.
erc: exit 0, 0 findings
drc: exit 0, 0 findings
parity: exit 0, 0 findings
Regenerated netlist and placement: 18 references; 62 schematic pins, R4 across +5V and GND.
Fabrication: all 10 issued plots/drill files match regeneration apart from dates.
```

`drill_overlay.py` also runs directly on the issued `.gtp`, `.gts` and `.drl` files. It reports the area of each drill’s intersection with top paste and mask openings. Coordinates in these Gerbers have Y increasing upward; Cleo’s reply gives PCB Editor coordinates with Y increasing downward. The measured pad-center projections do not supply JLCPCB’s machine zero, a live factory review or a measured 5 W rating.

The plot comparison drops creation-date lines only. A different KiCad build can also write different generator metadata.
