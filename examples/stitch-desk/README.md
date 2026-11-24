# A machine operator can take Linn’s original job

- **[Original operator ZIP](right_return_operator.zip)** — the unchanged 9,113 bytes issued by Linn and saved from the volunteer’s slip in Studio. Read `README.txt` inside and use `RIGHT_RETURN.dst` for the machine. [Exact dispatch wording](dispatch.txt).
- **[Volunteer packet](volunteer-packet.zip)** — unpack and open `Dispatch.html`. Its linked `Files/embroidery/right_return_operator.zip` is the same original ZIP. This is the later Studio packaging feature, not a new edition by Linn.
- [Study and limitations](../../docs/stitch-desk.md) · [Registration before invitation](protocol.json) · [Actual decisions, tool runs and issue](decisions.json) · [Independent measurements](receiving.json) · [Native measurement clipboard](studio-measurements.txt) · [Three recipient packet checks](recipient-packets.json).

To reproduce the ordinary digital geometry checks, install `pyembroidery==1.5.1` and run `python3 receive.py right_return_operator.zip`. It measures needle coordinates, not the header, preview or issuer’s verifier. As a second implementation, [libembroidery `7aaaee4`](https://github.com/Embroidermodder/libembroidery/tree/7aaaee4e7fd363b79eddcf212c9c0f53f278bbef) read the unmodified DST: `sew RIGHT_RETURN.dst moves.csv`. [Its actual output](independent-moves.csv) includes its own initial origin jump and illustrative spool color. Compare rows marked `*` after the first origin jump to the stock Python reader, with positive Y up. All 113 agree in order.

`audit.py` uses the frozen live database, native clipboard text, download and host launch log; its usage and character-path readers make no inference calls. Only the evidence summary, deliverables and independent intake are published. Neither reader operated a machine.

### Preserving an actual change during future work

Arm the [epoch observer](watch_epochs.py) on a settled numerical workspace before inviting anyone. Then work normally in Studio:

```sh
python3 watch_epochs.py "$WORKSPACE" "$PRIVATE_PARENT/new-observation"
```

Stop the observer after the activity settles using Ctrl+C (or pass `--until-file PATH`). It keeps a read transaction on the latest observed boundary without outstanding calls or queued effects and copies it and the newly observed state when it sees a post-initialization switch. It also saves both exact owner-checked model objects under `packages/`, even when a rapid activation and rollback fell between samples. It records heads, event ranges and archive digests. It never selects an action, opens an activity or switches a character. The SQLite archives are private and do not include the shared project directory.

Inspect the actual invitation, accepted choice, previous and new numerical fingerprints and intervening events. Initial activation, rollback, a directed change, and a freely adopted different numerical model are distinct. If several commits arrived between observations, these may be bracketing states, not adjacent transactions. `observed_both_heads: false` means that pair of archives does **not** preserve both required generations. A read pin can retain WAL pages during a long call. Use Studio’s verified restore on private copies and match the project, tools and evidence separately before any live comparison.

[Disconnected positive control](observer-control.json): the observer retained Ren’s genuine epoch 1 at event 63 and epoch 2 at 65, including the original choice explaining that the activation was requested. [Teo’s later live observation](../shift-intake/decisions.json) contains no switch.

[Two timing controls](observer-packages.json) also verified the private package files. When events 64 and 86 arrived between the same two observations, the observer preserved the exact **0.3 → 0.9 → 0.3** coefficient changes and reported `observed_both_heads: false` for both. Neither an extracted package nor the later restored weights substitute for the missing intermediate active workspace. [`check_observer.py`](check_observer.py) reproduces this on copies of the pinned historical archive.
