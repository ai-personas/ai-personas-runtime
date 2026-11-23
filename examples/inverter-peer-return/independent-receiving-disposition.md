# Independent receiving disposition

## Decision
**HOLD: unchanged-file ordering of five complete assembled prototypes is not substantiated.** This is a qualification of the evidence, not a demonstrated assertion that the placement files are wrong. Do not interpret this report as manufacturing release or order authorization.

## Exact edition
Starting receiving reference: delivery:delivery-f6d161232b743040623be9c737f0b6e3@1.
Received ZIP SHA256: `3712d5b34b3995b32a65ecf1f1565c0736dca8b20023ed23c5a98fe6ea0af24b`.
The edition itself is titled a corrected successor with a placement-compatibility hold; that label does not replace independent checks.

## Findings
### Placement interface — release blocker
The package's hold concerns U1/U3/U4 rotations of 270 degrees and U2's placement origin. U2's CPL coordinate is 53.7475, -46; agreement with its body/library center does not independently establish the supplier machine datum. The public component/footprint references do not by themselves prove JLCPCB's component-specific machine origin and pin-one interpretation. I therefore cannot certify that ordering would require only confirmation, rather than a prohibited manual placement correction. No definitive wrong-rotation or wrong-origin finding is asserted here.

### Part identity and complete assembly
The inspected public supplier snapshots identify C75864 as DRV8871DDAR, C86206 as L7805ABD2T-TR, C6987 as TLC555IDR and C13527 as CD4013BM96. These are independent identity evidence, not merely nonempty BOM codes. Public snapshots are not a reservation of live stock or a supplier acceptance of the complete order. The order plan calls for five boards with 18 fitted parts per board, including C8269 through-hole connectors intended for supplier installation. Do not omit the connectors or substitute buyer soldering while calling this the requested complete assembly. Supplier acceptance of that installation remains an order-interface condition.

### Physical process
The intended route is two-layer 1.6 mm FR-4, 1 oz copper, lead-free HASL, ordinary tented vias and Standard top-side PCBA. Supplier-added preparation to a processing envelope of at least 70 by 70 mm is contemplated; rails, fiducials and tooling are allowed by the brief. This review does not establish an independent exhaustive fabrication-tolerance certification or supplier CAM acceptance. Native DRC alone would not establish those facts. No special filled/capped-via process is certified by this report, and ordinary tenting must not be represented as filling.

### Editable stock KiCad 9 project
The received native files include `KiCad/inverter.kicad_pcb` and `KiCad/inverter.kicad_sch`, and were readable in the receiving toolchain. The PCB command printed 'Found 0 violations' and 'Found 0 unconnected items'. The schematic command printed 'Found 0 violations'. Both exited 4 because the requested /tmp report destinations could not be written (permission denied); no saved reports were produced. These observations support native parse/check access, not an unqualified clean execution or a complete interactive editing round-trip certification. No design repair was made.

### Buyer instructions
The package explicitly retains a placement-compatibility hold. Its instructions do not support proceeding directly to ordering while that hold is open. Routine selections, supplier preparation and confirmation of correct placements are allowed by the brief; buyer file edits and manual placement corrections are not. The available evidence does not establish that only the allowed actions will suffice.

## What would change the disposition
Independent component-specific substantiation that JLCPCB interprets the unchanged CPL origins and orientations correctly, especially U1/U3/U4 and U2, is required before clearance. If the supplier interpretation instead requires a buyer correction, this edition fails the unchanged-file condition. This report neither requests designer follow-up nor authorizes upload to obtain that information.

## Limits and actions
No upload, order, payment, physical build or file repair was performed. Stock, supplier processing acceptance and actual 5 W electrical/thermal performance were not physically validated. This is a completed independent receiving disposition with unresolved release conditions, not a completed manufacture or a claim of buyer acceptance.

## Retained evidence
- delivery:delivery-f6d161232b743040623be9c737f0b6e3@1
- Inspection stdout: object:33a505e1ec267ca65b06aa2ef68e2446cc42e1c00eadeedc7d2c1e56f29e2dba
- Observed inspection tail: experience:event-1364@1
