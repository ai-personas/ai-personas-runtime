# EC1 Rev A · C1 stock-card screen
**Decision:** Is this sleeve worth forwarding for a later unpowered inspection against the unchanged board? Use one record per sleeve. Keep the master unchanged; copy the blank record into the shift log. A paper referral is not substitution approval or permission to install.

### Compare the transcription with this target
| Check | Issued target | What distinguishes a mismatch |
|---|---|---|
| Identity | Panasonic Industry **EEUFR1C102** | Compare the entire manufacturer code, including every suffix. Never drop trailing characters or infer a variant’s geometry from the base code. A different code is not the specified part. |
| Electrical | Polarized radial aluminum electrolytic, **1000 µF ±20%, 16 V** | Voltage alone, or a generic ‘1000 µF’ label, is insufficient. |
| Case | **10 mm diameter × 16 mm high**; can centered between holes, top side | Do not interchange diameter and height. Do not infer a clearance allowance from the nominal dimensions. |
| Termination | Two **straight radial through-hole leads, 5.0 mm center to center**; nominal Ø1.0 mm plated drill at each C1 position | Bent/formed leads with a different insertion spacing, snap-in, axial or SMD are not the unchanged target. Do not bend leads or enlarge holes. Drill size is not a guaranteed finished bore or allowable lead diameter. |
| Polarity interface | Positive lead → pad **1 / STORE**; negative (stripe side) → pad **2 / G** | No guessed polarity. A sleeve description cannot establish actual marking, orientation or condition. |

**Evidence:** EC1 Rev A workshop ZIP: `docs/BOM.csv` C1 row (ordering code, electrical rating, body and straight pitch); `docs/parts-evidence.md` C1 entry (exact-model nominal package); `docs/build-test.md` (drills and pad polarity); `EC1/EC1.pretty/Radial_D10_P5.kicad_mod` (project footprint). These describe the board/design, not what is inside a sleeve.

### Route the offer
1. Preserve the card text verbatim, especially maker, full code, suffix, dimensions, units, polarity and lead form/pitch. Separate what the card says from nominal facts supported by the issued exact-model evidence. Log absent fields as **not stated**, never as ‘same as target’. If a legible maker and complete exact code identify the documented part, note explicitly which nominal fields come from that identity rather than the card; an explicit contradictory card field still matters.
2. **NOT A PAPER MATCH — do not forward** if the offer identifies another part or explicitly calls for an incompatible electrical or mechanical interface. Record the decisive field; equal capacitance and voltage do not rescue the wrong package. An internal contradiction about identity is a **HOLD**, not a choice of whichever reading fits.
3. **HOLD — do not forward** if there is no card, the full identity is missing/ambiguous, the evidence cannot establish a required interface, or the statements conflict so the variant cannot be identified. List the exact missing/conflicting words or fields. Keep the comparison prepared; use the existing stores exchange if further evidence arrives. Do not issue repeated requests or infer availability.
4. **PAPER MATCH — refer only for unpowered inspection** if identity and nominal interfaces are supported with no conflict. The later inspector would still have to establish that the actual sleeve contents, markings, body, straight leads, fit/clearance, polarity and condition agree. No physical inspection is claimed or requested here. Do not order, contact suppliers, print, fabricate, assemble, energize or test.

### Shift-log record · copy per offer
Sleeve ID / card source / date: ___

Verbatim card (or ‘not received’): ___

Maker + entire code, including suffix: ___

Electrical / polarity: ___  Body Ø × H (mm): ___  Lead form / insertion pitch (mm): ___

Compared to: EC1 Rev A C1 · EEUFR1C102. Nominal facts derived from exact identity, if any: ___

Decision (PAPER MATCH / NOT A PAPER MATCH / HOLD): ___

Decisive evidence or specific missing/conflicting fields: ___

Next desk action / actual-part checks still unverified: ___

**If neither card arrives:** open two separate records marked HOLD / not received. No sleeve is cleared on the evidence available. Retain the prepared target comparison and blank master for the next offer.