# Receive Cleo’s successor

**Hold for engineering review. Do not order the five assemblies from this edition.** [Download the exact ZIP](successor-evidence-hold.zip) · [Cleo’s buyer instructions](buyer-dispatch.txt) · [Linn’s independent report](independent-receiving-disposition.md) · [Measured results and limitations](../../docs/inverter-peer-return.md).

The return fixed the three exposed vias and U2’s geometry. The buyer received the exact archive Arden issued. No buyer files were repaired during inspection. Supplier-specific placement clearance remains unestablished; the four connector holes also differ from the manufacturer’s recommended diameter, and no paste layer was issued. Neither supplier acceptance nor measured 5 W performance is claimed.

| Evidence | What it contains |
|---|---|
| [Protocol](protocol.json) · [Cleo’s brief for Arden](cleo-to-arden.md) | Starting edition, original finding, autonomous exchange, stopping conditions and independent receiving brief |
| [Decision and run evidence](decisions-and-runs.json) | Exact request, receipt, returned editions, 42 exchanges, 16 receiving runs, held and rejected decisions, provider-input/output digests |
| [Peer history checks](peer-history-checks.json) | Actual prior checks on the reissued ZIP, sender/receiver visibility, disconnected negative controls for changed files and altered receipts |
| [Cleo’s reply](cleo-to-buyer.md) · [Linn’s reply](linn-to-buyer.md) · [Linn’s actual saved KiCad check output](linn-kicad-output.txt) | The two buyer dispositions and the later successful saved reports overlooked in Linn’s final account |
| [Independent verification](verification.json) · [18-part table](parts.csv) | Exact bytes, stock observations and dates, source parity, plot regeneration, actual holes and design operating conditions |
| [Calibration](placement-calibration/) · [land comparison](land-comparison.json) | EasyEDA’s actual per-part zero export, placement transforms and overlap against the issued top copper. A [second exporter](placement-calibration/toolkit-comparison.json) corroborates centers but disagrees on two rotation rules. |

From the repository root:

```sh
python3 examples/inverter-peer-return/receive.py
python3 examples/inverter-peer-return/receive.py --plots --kicad kicad-cli --catalog /path/to/supplier.sqlite
```

The first command needs only Python. The second was exercised with stock KiCad 9.0.9, `gerbonara` 1.5.0, `shapely` and the [pinned supplier catalog](../inverter-successor/README.md). `--catalog` is optional. Gerber and drill comparisons ignore creation dates only. The whole-board paste test explicitly regenerates the **missing** paste layer from the unaltered board; it never adds files to the buyer’s ZIP.
