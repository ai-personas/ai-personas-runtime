# Arden’s two inverter successors

Both ZIPs are original Studio downloads for engineering review. **Neither cleared unchanged ordering for five assembled boards.** The personas fixed the missing parts list, but their PCB and assembly checks left different problems.

| Download | Independent receiving evidence |
|---|---|
| [Frozen Arden: qualified v5 r3 ZIP](frozen/bench-inverter-v5-qualified-r3.zip) | [18 fitted references](frozen/parts.csv) · [KiCad and CAM results](frozen/receiving.json) · [5 W basis and paste/drill check](frozen/operating-basis.json) |
| [Experienced Arden: successor review ZIP](experienced/inverter-successor-review.zip) | [18 fitted references](experienced/parts.csv) · [KiCad and CAM results](experienced/receiving.json) · [5 W basis and paste/drill check](experienced/operating-basis.json) |

The CSVs and JSON files alongside the ZIPs are the independent receiver’s observations. The ZIP contents have not been corrected, normalized or repacked. [Read the comparison and its limitations](../../docs/inverter-supplier-successor.md).

From this directory, `python3 recheck.py` verifies the two archives, the issuers’ internal manifests, and the fitted-reference and purchasing identities in the BOMs and placement lists. To repeat the ERC, DRC and strict schematic-parity counts, run `python3 recheck.py --kicad /path/to/kicad-cli` on a stock KiCad 9 workstation. Optionally pass `--catalog /path/to/components.sqlite` to check the original pinned catalog. The check writes only to a temporary directory.

The [separate shop note from Cleo](receiving-shop/receiving-note.md) concerns the **earlier v4**, not either successor. It is also an unedited native download.

[Matched supplier protocol](protocol.json) · [Separate receiving-shop protocol](receiving-shop/protocol.json) · [Cleo’s decisions and receiving receipts](receiving-shop/observations.json)
