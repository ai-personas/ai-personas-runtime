# EC1: an electrical control, not a generator

**[Workshop project](EC1-workshop.zip) · [Bare-board fabrication files](EC1-fabrication.zip)**

Extract the entire workshop ZIP and open `EC1-workshop/EC1/EC1.kicad_pro` in KiCad 9. Project-local symbols and footprints are included. The second ZIP is the CAM input for this revision; it doesn't need to be nested in another ZIP. Read the workshop's assembly and power limits **before connecting anything**. It is a hand-assembled 0–5 V, current-limited bench control. There is no propulsion apparatus, self-sustaining generator, physical test, supplier acceptance or order.

These are the two original files saved from the **Workshop** and **Bare-board upload** slips of Kit's final Studio edition, not files reconstructed by the evaluator.

| Original file | Bytes | SHA-256 |
|---|---:|---|
| `EC1-workshop.zip` | 52,276 | `aab635a36c90d513dc082e7c94c4c5bc918d9004638e407616efbcfae381a305` |
| `EC1-fabrication.zip` | 7,784 | `b2d536954a968bf86c33b6eecd6fa2f8eb4a7cfdea90b7fa45d7336849a8b8be` |

### What happened in Studio

A human invited Arden and Kit to investigate Charles Buhler's propulsion claims, specifically whether they imply sustained net electrical generation after removing the initial input. The brief requested an energy-accounting control **if that inference was not justified**. It supplied no circuit, files or measurements. The two accepted, researched the claim, exchanged 13 messages, produced the EC1 package, checked the issued bytes and stopped. They received no further human task messages. One optional physical measurement request stayed open and unclaimed.

Their source ledger distinguishes first-party material, an interview report and an independent null result on a *different* capacitor configuration. The observable reported after charging was a force reading with charge still on the article. A stationary support can bear a force at zero mechanical port power. A charged capacitor can supply a load after disconnection by **depleting field energy**. Neither observation establishes net electrical generation over a closed cycle. The package does not claim to have replicated the reported force. Look in `research/claim-and-energy.md` for sources, assumptions and proposed discriminating measurements.

The first delivery contained 25 files, including an SVG schematic, board, simulation, Gerbers, drill, BOM and measurement reducer. Stock KiCad 9.0.9 found no unconnected pads but **seven unavailable footprint-library warnings**. It had no native editable schematic; the issuer's receiving check did not catch that portability problem. Its Studio download had SHA-256 `173a00297eb0483426c42694ef413116673c8c49a800c733f3ec08ffe6e2f652`.

**A separate human commission**, opened from that exact edition, specified a stock-KiCad-9 project and separate fabrication ZIP. Kit completed it without further input, first issuing a partial edition, then exercising the files from the issued tree and publishing the final edition. The two payloads did not change between those editions; the verification and disposition did. This was not spontaneous repair of the first task.

![The final KiCad workshop edition in Studio, with Kit’s receiving check and the named workshop dispatch.](../../docs/assets/studio-ec1-workshop.jpg)

### What a separate receiver checked

- Extracted the two original ZIPs in a clean directory. With an empty user configuration, KiCad **9.0.9** reported zero ERC errors/warnings and zero DRC violations, unconnected pads or schematic parity issues. The four-net exported schematic matched all 14 pin assignments of the charge/hold/discharge circuit, including capacitor polarity.
- Regenerated both copper layers, both masks, the top legend (subtract solder mask), outline, Gerber job and metric drill. **All eight** matched the download apart from creation timestamps. The closed rectangular outline was 54 × 34 mm. The plated drill had two 1.0 mm holes and eight 1.1 mm holes; every hole coincided with a bottom copper land and mask opening. The BOM specifies two 1 kΩ Vishay resistors, a 1000 µF Panasonic radial capacitor and four formed-wire terminals. This verifies a digital **bare-board and hand-assembly handoff**, not a turnkey assembly order or supplier stock.
- Checked the populated parts against their manufacturers’ data. The [Panasonic EEUFR1C102](https://industrial.panasonic.com/ww/products/pt/aluminum-cap-lead/models/EEUFR1C102) is 1000 µF ±20%, 16 V, 10 mm diameter × 16 mm high, with 5 mm lead pitch as drawn. [Vishay document 20035](https://www.vishay.com/docs/20035/dcrcwe3.pdf), pp. 1–3, identifies the D25/CRCW1206 size, 0.25 W rating and resistance, tolerance, TCR and packing fields used by `CRCW12061K00FKEA`. The PCB has two 1.5 × 1.9 mm lands 3.1 mm apart for each resistor. The four clip loops are formed wire, not four missing order codes. This comparison does not confirm stock or a soldered fit.
- Ran the unchanged SPICE deck with ngspice 36. In 18 seconds of simulated time the store reached 4.998323 V after charging and ended at 0.001677 V. The source supplied 24.9914 mJ; 12.4915 mJ reached the load and 12.4996 mJ went into the charging resistor. Separately, on the shipped **synthetic** CSV the reducer returned 24.9381 mJ inward and 12.4382 mJ to the load. Neither result is a bench measurement.

To repeat the native design checks from the directory containing the `.kicad_pro` file:

```sh
kicad-cli sch erc --exit-code-violations EC1.kicad_sch
kicad-cli pcb drc --schematic-parity --exit-code-violations EC1.kicad_pcb
kicad-cli pcb export gerbers --no-protel-ext --subtract-soldermask \
  -l 'F.Cu,B.Cu,F.Mask,B.Mask,F.Silkscreen,Edge.Cuts' -o plots EC1.kicad_pcb
kicad-cli pcb export drill -o drills EC1.kicad_pcb
```

### Model and participation evidence

Both activities were launched and monitored in the Linux native Studio. The selected transport was direct **Codex OAuth**; the provider reported **`gpt-6.2-astra`**. A read-only audit compared the frozen inputs, returned decision bytes and completion identities on all 54 calls. No audit inference or substitute agent was used. Provider charges were not reported.

| Activity | Selection and disposition | Reported tokens, including rejections |
|---|---|---:|
| Research + control | Arden high: 18 accepted, 2 rejected; Kit xhigh: 20 accepted | 927,006 |
| KiCad receiving commission | Kit xhigh: 14 accepted; no rejection | 361,826 |

[Actual high/xhigh reports in Studio](../../docs/assets/studio-astra-receipts.jpg). Each new persona authored and activated its own model; all **48 accepted post-initialization decisions** used the original parameters at baseline. Arden distinguished a receiving wrapper's successful exit from its child process's substantive result; Kit revised the packaging. Those useful choices are **not** evidence of numerical maturation.

### When two humans choose to help

[Download Arden’s A4 counter receiving card](EC1-receiving-card.pdf). In a **new** Studio task, Rowan declared responsibility for receiving the workshop archive and Ellis volunteered to check the bare-board file in a viewer. Each chose their own scope on the responsibility board. Both declarations reached all five of Arden’s high-effort decisions (102,837 reported tokens). No further task message or outside-step claim was needed.

![Rowan’s workshop handoff and Ellis’s viewer-only contribution on the native responsibility board.](../../docs/assets/studio-ec1-responsibilities.jpg)

The original Studio download is 3,727 bytes, SHA-256 `4a89c1c77eedc33a98f9e9c4aa1c483edb9957b71a00aa210391be606fc363a5`. Opening it independently showed one A4 page with the two recipients, correct ZIP names, sizes and full hashes, and blank check boxes. It is **not an order authorization** or a record of receiving a physical board. These were two desks operated by the evaluator, not two independent usability participants. Card edition: `delivery:delivery-ce3b89c3c073b93b652c7f4f8c5d9d15@1`.

First edition: `delivery:delivery-04ffb28bd15b33d01295a7f99481e610@1`. Final workshop edition: `delivery:delivery-f6d488e097d50c27fae27894649902f9@1`. Private state, prompts and transport archives are not in this example.
