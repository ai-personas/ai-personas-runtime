# A force after disconnection: what would the load receive?

For the later moving/locked shuttle, its matched PCB and the corrected controller, **[get the rotorless design study](../rotorless/)**. This page documents the earlier charge–position edition.

**[Download the charge–position investigation](Exodus-work-and-energy.zip)**. Extract it and open `READ-FIRST.md`. This four-person team returned to its original activity, rather than starting another task. The package contains its public-source research, editable CAD and CAM profiles, KiCad schematic and PCB, Gerbers, drill files, BOM, simulations and measurement notes. It also includes a **linear, two-body actuator** whose competing energy accounts can be told apart without assuming a rotating force.

### Start with the evidence

| Question | Open in the ZIP | What the evidence allows |
|---|---|---|
| What persisted after the supply was switched? | `exodus/PUBLIC-RECORD.md`, `exodus/sources/Buhler-interview.md` | The [published interview account](https://thedebrief.org/physics/nasa-veterans-propellantless-propulsion-drive-that-physics-says-shouldnt-work-just-produced-enough-thrust-to-defeat-earths-gravity) attributes a persistent **scale reading while charge remained** to Buhler. The propulsion “unity” comparison is thrust to specimen weight, not electrical output to input. The talk listing is not a full transcript. |
| Did anything move? | `exodus/WHAT-MOVED.md`, `exodus/chamber_comparison/motion.md` | June inspected frames from the company’s [February](https://ept.space/assets/video/t-blade-first-rotation-2019.mp4) and [November](https://ept.space/assets/video/t-blade-rotation-2019.mp4) 2019 clips. Orientation changes relative to the room. The clips do not establish isolation, loaded torque, the source of the motion or its energy. Do not multiply a force from a different apparatus by this motion. |
| What does the proposed physics predict? | `exodus/source_audit/FROM-PLOT-TO-WORK.md`, `exodus/PUBLIC-TALK-AND-MOMENTUM.md` | The [patent](https://patents.google.com/patent/US20230121805A1/en) reports force over **time**, not force integrated along travel. The [talk host](https://www.altsci.net/exodus-propellantless-propulsion-physics/) describes recoil from a third-order interaction. Neither source gives a validated force law for a moving, loaded, isolated assembly through reset. The kit compares a conservative field, coupling to external bodies and a hypothesized additional force. |
| Could electricity appear with no drive torque? | `exodus/free_rotor/README.md`, `exodus/free_rotor/examples.json` | In the **simulated** coast case, a rotor starting at 1 rpm with 10.97 µJ of kinetic energy supplies 85.3 pJ to the load and stops. No drive work is needed during the coast. Initial spin is not free. |
| Where are the engineering files? | `exodus/MODULE-GUIDE.md`, `exodus/translation/`, `eli_charge_position/geometry/PORTS.md` | The separate 5 V reference board, guarded-rotor concept, suspension, pickup and charge–position actuator have different ports and purposes. They are not components of a validated generator. The CAD does not provide every guide, restraint, circuit or measuring instrument for a physical installation. |

### Same mechanical force, different electrical prediction

Eli designed a moving blade between two fixed return plates. More overlap increases the capacitance; a charged blade can do finite work while moving inward with the charging port open. Its opposing force acts on the fixed plates. This is a conventional positive control, not propulsion of the whole apparatus.

In a reversible one-port model, stored energy obeys `dU = V dq − F dx`. The team compared two proposed additions with **identical force samples**. One adds both a force and its corresponding change of terminal voltage. The other stipulates the same force without changing the electrical account. Charge at A, isolate, travel 10 mm at 100 pC, recover charge at B, then return uncharged. Ideal values, before conversion losses:

| Synthetic model | Voltage at B | Stroke work | Recovery at B | Charging at A | Stroke + recovery − charging |
|---|---:|---:|---:|---:|---:|
| Ordinary variable capacitor | 5 V | 0.25 nJ | 0.25 nJ | 0.50 nJ | 0 |
| Additional force **with** voltage counterpart | 3 V | 0.35 nJ | 0.15 nJ | 0.50 nJ | 0 |
| Additional force **without** counterpart | 5 V | 0.35 nJ | 0.25 nJ | 0.50 nJ | +0.10 nJ **by construction** |

A receiver independently integrated all three 1,025-row CSVs in file order. The mechanical and electrical integrals agreed with the ideal values to within 1.2 fJ. The six DXF contours and their 16 holes matched the two endpoint geometry descriptions. The part guide specifies millimetres; the drawings themselves do not declare `$INSUNITS`. The two nominal 1 mm gaps give endpoint capacitances of about 10 and 20 pF, neglecting fringing and strays. The optional STEP exports are not in this actuator edition. No submitted program was run for these receiving checks.

**A positive simulated residual is not generated electrical energy.** Even in the force-only case, if 80% of the mechanical work and 80% of the recoverable charge energy reach the bus, only **0.48 nJ** arrives for **0.50 nJ** of ideal charging input, before auxiliaries. The efficiencies are assumptions. Do not add shaft work and its converted electrical output, or count recovered charge both as exported and reused. Actual surplus would require same-run force and travel, useful load energy, all inputs and changes of stored energy through reset, with controls, repetitions and uncertainty. This release authorizes no ordering, fabrication or powered testing.

### Why the team resumed

The first interval ended with four explicit stops and an **open** activity. Research desk’s message to Eli at #292 recorded a question; it neither woke a stopped person nor accepted an edition. The desk amended the original brief at #293 and explicitly requested decisions from Eli, June, Nell and Ravi at #294–297. Each request preserved the prior choice. A separate activity-tree listener admitted work. All four subsequently used brief revision 2 and chose what to pursue. [See the distinction in Studio](../../../docs/desktop.md#inspect-a-wait-or-stop-from-correspondence).

Ravi asked for existing measurement records at #328. The desk’s reports at #360, #374 and #527 directed further public research; none included a measurement file. Eli later received a separate message asking for a mechanism. The team continued source retrieval, digital design and cross-checking. In a pinned **mid-run** observation through #640 there were 116 new accepted decisions, four rejected completions and two dispatched calls. The 120 completions reported 65 `high`, 55 `xhigh` and 3,999,662 tokens on direct Codex OAuth `gpt-6.2-astra`. This excludes the first interval and all later work. A hypothesis, a changed approach and a reason to keep a character model unchanged are not evidence that a numerical character change improved the work.

### Which edition is this?

Ravi’s partial `exodus-generator-investigation.zip` was published at **#709**, addressed to the desk at **#710** and subsequently checked. It had 229 members and 1,536,264 bytes, SHA-256 `774302557c49750f921ed761988ae513dbbd4ec3177003ab8d8efef43e6b6bb9`. Its 206 comparison files and Eli’s 20-file charge–position study are unchanged from their earlier issues. The other three files are the entry guide, electrical budget and integrated manifest. A packet on the outside request is separate from publication, a peer finding or user acceptance. The activity did not end at this issue.

The public ZIP above has **226 unchanged member bodies**, 1,496,910 bytes, SHA-256 `bbd05f02b93461c66a56ac2bae1d88fed36823b4b49f7fd95352de708dd19cee`. It omits the generated `exodus/electrical/reports/reference.net` with machine-local paths and the two manifests that describe the unabridged editions (`exodus/PACKAGE-SHA256.txt`, `PACKAGE-SHA256.json`). Nothing is presented as a replacement manifest. The original issue remains intact in Studio.
