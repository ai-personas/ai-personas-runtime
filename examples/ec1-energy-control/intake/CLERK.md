# EC1 | receiving desk

**Keep the originals unchanged.** Retain the workshop and fabrication ZIPs separately (see `ARCHIVES.md`). Keep each logger export and its metadata as received. The CSVs under `examples/` are synthetic exercises, not measurements. Never decide from a filename.

## 1. Run the check offline

Extract the whole kit. In a terminal, change into the `intake-kit` folder. Use Python 3 (standard library; no installation of add-on packages). On Windows use `py -3` in place of `python3`.

```
python3 check_logs.py examples
python3 check_logs.py "/path/to/new logs" > batch-report.json
python3 check_archives.py issued
```

The logger check reads direct-child `.csv` files, not subfolders. Write the report to a new file outside the input directory. Retain it alongside the batch identification. Exit 0 = all assessed files usable, 1 = one or more HOLD (read the JSON; the examples intentionally return 1), 2 = invocation/directory problem. No manual selection of time windows is needed.

## 2. Decide what may be quoted

Check `sha256`, `disposition`, `reasons`, `runs` and `breaks` **before** quoting energies. A usable trace shows a near-empty STORE, a rise with LOAD off, charged isolation, then discharge into LOAD ending near the starting state. These are voltage signatures, not direct observations of switches. The common sample spacing is 5 ms; simultaneous ideal switch rows do not add elapsed time. In this exercise a transition must have rows from both phase signatures at the same timestamp. If the signature changes across a positive-duration step, HOLD even at exactly 5 ms: neither the switching instant nor that interval’s energy is supported. Quote only the separate covered spans. `USABLE_COMPLETE_SYNTHETIC_CYCLE` is a coverage decision under the exercise assumptions, not authentication of a physical run.

| Example | Desk action | Meaningful numbers (mJ, rounded) |
|---|---|---|
| log-27.csv | Usable complete cycle, 0–16 s | Signed input 24.93808; R1 heat 12.50003; load 12.43821; final store ≈0.0000014. No excess. |
| log-14.csv | HOLD: starts charged; neither empty start nor charging observed | Within the 0–9 s window: initial store 12.43811; input 0; load 12.43821. Prior charging cost unknown, not zero. |
| log-83.csv | HOLD: 0.25→2.75 s charging gap, 499 missing interior samples | Signed input 5.52999 over 0–0.25 s and 1.53623 over 2.75–16 s. Later load 12.43821. No whole-cycle input or efficiency. |

See `example-report.json` for the runnable output; `interval-ledger.md` gives an independent calculation. On HOLD, preserve the original and report; ask for the continuous original export, acquisition settings and run notes. Do not repair, fabricate, interpolate or silently substitute a simulation.

## 3. Interpret the boundary

Each `covered_intervals` entry stands on its own. For nominal R1 = R2 = 1000 Ω, C1 = 0.001 F, integrate sampled powers by trapezoids over supported time steps: IN-port power = IN·(IN−STORE)/R1 (positive into EC1); R1 heat rate = (IN−STORE)²/R1; load heat rate = LOAD²/R2. Stored field energy at an observed endpoint is C1·STORE²/2. Compare signed input with R1 heat + load heat + final minus initial store over the **same** interval. The residual is a closure diagnostic, not output. Load minus inward energy ignores initial storage; neither quantity is upstream wall-plug energy. Nominal component values, sampling and rounding limit precision.

The workshop note describes reported scale force while charge remained. A stationary force can coexist with zero mechanical port power (force × velocity). Mechanical work requires force integrated along displacement, including reset. These voltage channels measure no force or displacement; the RC control neither verifies nor refutes the reported effect. Disconnecting the charger does not empty a charged store. Positive load energy after isolation is not net generation. A generation claim would require metering all relevant energy boundaries, returning stores to initial conditions, and reproducible excess beyond uncertainty.
