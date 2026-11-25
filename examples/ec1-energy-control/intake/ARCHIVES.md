# Which EC1 ZIPs to retain

Keep these two **unchanged ZIP byte streams**, separately. Do not substitute a re-ZIP of the same members. These are issue copies for identification, not an instruction to place an order or test hardware.

|File (use)|Bytes|SHA-256 of the whole ZIP|Original issued asset|
|---|---:|---|---|
|EC1-workshop.zip (complete workshop; native KiCad project, notes, recorder/reducer)|52276|`aab635a36c90d513dc082e7c94c4c5bc918d9004638e407616efbcfae381a305`|`asset:asset-8797737d2fc0d298ef1f08075e151bda@1`|
|EC1-fabrication.zip (bare board only; Gerbers, job and plated drill)|7784|`b2d536954a968bf86c33b6eecd6fa2f8eb4a7cfdea90b7fa45d7336849a8b8be`|`asset:asset-ee0a4ed7a2a3ea8ef1256917d1fc31e8@1`|

**Two checks, different questions.** Compare the received ZIPs with this table using check_archives.py. A match answers “same bytes?” only. Also associate the copies with the original issue and original writing receipt, not just a filename, later delivery, extract, CRC, screenshot, or someone reporting a match.

Original issuing activity: `work-25838e21a17391caa992a0fc8b32770e`. See the offline retained records in `archive-evidence/`:

1. `source-issue.json`: source edition `delivery:delivery-f6d488e097d50c27fae27894649902f9@1`, naming both exact assets and their separate dispatches.
2. It cites `first-issue.json`: `delivery:delivery-5205523f8cbfb6363cadfa57c946d605@1`, same two assets, citing `experience:event-124@1` (freeze).
3. `zip-write.json`: that event’s underlying `command_run:local-ae9b3d9c263b5bda1c3a566e1910ba8396bef9ef9344fda08f18b5f7beea6683@1`, invocation `call-56a7b5f284c16e14aca28f0a957a76c0`, operation `freeze`. Its Python command writes the two named ZIPs; successful named outputs give the two byte counts and object digests above. A later read-only check is not the write receipt. These copies support a record comparison; they are not cryptographic signatures. Compare against your trusted issue records if the packet itself is in doubt.

**Exception route.** If either file differs or is absent, HOLD it; preserve what arrived and obtain the unchanged issued copy. If the bytes match but either original issue or writer evidence is absent, mark **BYTE MATCH / PROVENANCE HOLD**, not verified or reissued. Keep a read-only copy with name, size, digest, source, and which evidence is missing. Request the exact original issue and writing record that ties those bytes to that issue from the issuer/custodian. Do not create a new ZIP, infer authorship from a digest, or use a later check as a substitute. Clear the hold only after checking those records. This is a digital provenance check, not supplier acceptance, fabrication, test, or measurement. Keep the logger record verdict separate.
