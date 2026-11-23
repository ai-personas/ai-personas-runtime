# HG-42 Rev C · receive one reader mask

**[Before the crossover: original shop ZIP](before/HG-42-C-shop.zip)** · **[During the crossover: original shop ZIP](crossover/HG-42-C-shop.zip)**. Each contains an editable 1:1 millimetre SVG, cut and installation directions, the controlling drawing and the issuer’s tolerance checks. Both passed independent digital intake unchanged.

These are two actual handoffs by Ren. Its temporary numerical edit was explicitly requested in a historical experiment. **Neither Mara nor Linn chose to adopt a numerical change** in the voluntary reviews. [Study, decision differences and limitations](../../docs/character-mask-trial.md) · [Protocol registered before the trial](protocol.json) · [Drawing](drawing-C.txt) · [Provenance and opening conditions](study.json)

| Download | Bytes | SHA-256 | Recipient’s instructions | Receiving results |
|---|---:|---|---|---|
| Before | 5,885 | `e0f2eb1cd21b5e5bc21835c483dcee3c4f78e4ec0833b004d497db0c48a90a76` | [Cut shop / installer](before/shop-dispatch.txt) | [CAM and every tolerance](before/receiving.json) |
| Crossover | 6,169 | `d8c6d433d74b2e77ac62b46dfe84bf5dd6ac604a09e3a8403ffd803a78b271d5` | [Cut shop](crossover/shop-dispatch.txt) | [CAM and every tolerance](crossover/receiving.json) |

### Repeat the receiving check

Use Python with MeerK40t **0.9.9100** installed. The commands below open the original ZIPs and plan the two windows before the perimeter, as both issuers instructed. `receive.py` reads the actual vector paths at physical scale, checks closure and rectangularity, inspects the stock CAM paths and evaluates the drawing’s independent extrema. It does not execute the issuers’ verifiers.

```sh
python3 examples/character-mask-trial/receive.py --windows-first examples/character-mask-trial/before/HG-42-C-shop.zip
python3 examples/character-mask-trial/receive.py --windows-first examples/character-mask-trial/crossover/HG-42-C-shop.zip
```

Omit `--windows-first` to see the single-operation plan used for comparison. The stock importer initially classified the three black strokes as raster. Both shop documents specify vector through-cuts, 1:1 scale, no added kerf offset and both windows first. The directional operation assignments do not alter any artwork. The receiver uses a dummy device and never spools the job.

### Inspect the choices

- [Before: every decision and effect](before/decisions.json) · [Crossover: every decision and effect](crossover/decisions.json).
- [Mara’s actual verdict](review/mara-reply.md), [her reusable clearance method](review/independent-contact-clearance-checks.md) and [review receipts](review/mara-decisions.json).
- [Linn’s actual verdict](review/linn-reply.md), [retained question](review/linn-retained-question.json) and [review receipts](review/linn-decisions.json).
- [Post-trial inspection checks](inspection.json): actual active epoch before a switch or rollback, copied shop directions, and disconnected validation of the feedback/practice link. These are separate from the decisions made in the four live activities.
- [`audit.py`](audit.py) rechecks a private source workspace: `python3 audit.py WORKSPACE WORK --cli /path/to/ai-personas --native-download HG-42-C-shop.zip`. An optional `--launch-log` adds the external host’s four command-launch observations. Omit `--native-download` for a review. [`freeze_ren.py`](freeze_ren.py) reconstructs only the registered prefixes of the specified historical archive.
