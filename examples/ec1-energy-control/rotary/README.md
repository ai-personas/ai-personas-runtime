# Does a force on a turning rotor pay back its charging energy?

**[Download Lumen’s editable workbook](Turn-ledger.xlsx) · [Print the one-page A4 briefing](Rotor-brief.pdf) · [Read the desk’s original question](commission.txt)**

Open a copy of **Turn-ledger.xlsx** in a recalculating spreadsheet. On **Inputs**, change the six yellow cells B5:B10: radius in mm, force in mN, speed in rpm, number of complete turns, conversion efficiency (enter `60%`, not `60`), and initial charge cost in mJ. Read **Full turns**. The macro-free workbook needs no other file or service. The PDF is the briefing for the **issued** settings; editing the workbook does not change it.

**In the specified 20 turns, neither force law repays the 25 mJ charge.** A force fixed in the room starts forward-tangent, then opposes the motion for part of each revolution. Its positive and negative work cancel. A force that *stays* forward-tangent supplies positive shaft work throughout. That second law is a different physical assumption, not a consequence of putting the first device on a rotor.

| At 100 mm, 2 mN, 60 rpm and 60% conversion | Room-fixed force | Forward-tangent force |
|---|---:|---:|
| Signed shaft work per complete turn | +0.400 − 0.400 = **0 mJ** | **1.256637 mJ** |
| Electrical energy from signed work over 20 turns | 0 mJ | 15.079645 mJ |
| Load energy minus **only** the initial charge cost | −25 mJ | −9.920355 mJ |
| First whole turn strictly over the charge cost | Never | **34**, beyond the run |

The room-fixed case could deliver **4.80 mJ gross** in 20 turns by engaging the load only on favorable arcs, but then the constant-speed drive must supply at least **8.00 mJ of shaft work** on the opposing arcs. In the forward-tangent case the hypothetical torque is 0.200 mN·m throughout; at one turn per second the mean load power is 0.753982 mW. Neither column measures the energy needed to sustain the force or restore the device. Exceeding a one-time charge is **not** a measured COP above one. A closed-cycle test needs synchronized angle, signed torque, speed, electrical input and load energy, and equal initial and final stored and kinetic energy or a metered reset. No rig was built or tested.

### What Lumen did in Studio

Rotor desk opened a new invitation from [Arden’s moving-plate edition](../motion/) in a separate copy of its settled history. Only Lumen was invited. The desk supplied the question, not a formula, layout, implementation or verdict. Lumen accepted, checked the tools available, wrote the workbook and briefing, and selected a render of the briefing for its issuing decision. No collaborator or further operator message was needed. The earlier edition was the starting reference, not evidence that its file contents were read on this task.

Lumen checked both files after issue. Its first receiving command read the ledger and PDF properties but failed when `pdftotext` was unavailable. The failure reached its next decision. Lumen used an available PDF reader, explained the full-turn result and stopped. Its earlier command had checked the formula cases with its own method; it reported no stock recalculation engine in that workspace. There were **eight accepted `gpt-6.2-astra` `xhigh` decisions** via direct Codex OAuth, with 171,163 input and 15,461 output tokens reported, no price reported and no change of numerical character. The original prompts, selected image, finals and command receipts matched. This was a different commission from the [five-board reflow handoff](../reflow/); neither restarted the other.

### What a separate receiver checked

The **Save selected files ZIP…** slip in Studio and an independent export yielded the same archive. Each file below also matched the retained command output registered by Lumen and the file at the public issue. Neither was regenerated for publication.

| Original file | Bytes | SHA-256 |
|---|---:|---|
| `Turn-ledger.xlsx` | 9,533 | `02cc468de649817d3eba39a3f09c695a06fc21945ddc46b0df738d6891dec85b` |
| `Rotor-brief.pdf` | 10,972 | `54360f76a8e653e6b220c05dea82169f887cf2d57ebe381bf47b9f705cb31929` |

After checking the workbook parts, relationships, names and formula functions, the receiver used stock **Gnumeric 1.12.51** offline. All **37** original cached results and **18 sets of 37 recalculated results** matched an independent calculation or its expected input refusal. No output formula was repaired. Examples:

- At **50 mm, 3 mN, 120 rpm, 45 turns, 70%, 15 mJ**, the follower delivered **29.688051 mJ** in 22.5 s; the first whole turn over the bill was **23**. Entering all six changes in the graphical spreadsheet produced the same result. Doubling only rpm doubled mean power, not energy per turn.
- With the initial bill exactly equal to one turn’s yield, the first turn **tied** and the second exceeded it. Zero force or zero efficiency did not earn back a positive bill. At zero turns the run delivered nothing even though a future crossover could be calculated.
- The editor refused a fractional turn count and a negative radius. Clearing the force made **Input check** change to **Check inputs**. Blank, stopped-rpm, fractional, negative and out-of-range efficiency cases suppressed numerical results on recalculation. The importer reported an unhandled empty `workbookProtection` element; it did not prevent the edits or checks above.

The received PDF rendered as one **595.276 × 841.890 pt** A4 page. Its table, threshold, drive qualification and measurement instructions were legible and agreed with the workbook’s original settings. Studio can [inspect the saved expression at `Full turns!D22`](../../../docs/desktop.md#inspect-a-workbook-formula) without executing it. That view is not the independent recalculation.

Edition `delivery:delivery-3b7b0673412b4686094fcb80d350408a@1`, issued as **claimed complete**. The addressed copy and Lumen’s own checks are not a user verdict, a force measurement or acceptance of a generator design.
