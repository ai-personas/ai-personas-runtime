# Take the next clerk’s register

[Shift-close CSV](receiving-register.csv) · [Teo’s exact instructions](next-clerk.txt) · [What happened during the outage](../../docs/shift-intake.md)

The two original submissions are [the relief volunteer’s packet](01-Relief-volunteer-packet.zip) and [the desk lead’s packet](02-Desk-lead-packet.zip). Inside either ZIP, `Dispatch.html` gives Vale’s instructions and links to the files for **that** recipient. The [opening](open.csv) and [intermediate](one-received.csv) CSVs are the exact earlier issued revisions; a missing filename or hash there was not an empty-file receipt.

Open the final download directly from the checkout: `./personas --gui --open-file examples/shift-intake/receiving-register.csv`. Click row **2** to read the lead’s fields.

To independently check the final register against both ZIPs, from the repository root:

```sh
python3 examples/shift-intake/receive.py
```

This reads the downloaded CSV, recalculates both hashes, checks each ZIP’s CRCs, and compares the actual sender, issuer, recipient and entire file listing. The [result](receiving.json) quotes the outstanding and physical-status fields. The [registered messages and scoring rule](protocol.json) preserve the meaning of those fields.

The [decision and station record](decisions.json) includes the accepted input that received each arrival, the exact staged files, subsequent stdout supply, all three issues and the interval with no decision. The [audit](audit.py) can be rerun by an operator holding the original private study directory; it does not start work:

```sh
python3 examples/shift-intake/audit.py "$STUDIO/bin/ai-personas" \
  "$PRIVATE_STUDY/Receiving shift" "$PRIVATE_STUDY/control/shop-launches.jsonl"
```

The [boundary checks](boundaries.py) read the two frozen archives and contemporaneous station logs beside that workspace. Public files contain no live database or account configuration.

[`check_receipts.py`](check_receipts.py) exercises the file-inspection links on temporary copies of `Capture/ended.sqlite`. Removing the producing call’s terminal output withholds the mount; removing only the following decision’s terminal output withholds only its stdout receipt. An uncertain operation and a second submission of the same bytes after stop gain no inspection credit. [Post-shift inspection results](inspection.json).
