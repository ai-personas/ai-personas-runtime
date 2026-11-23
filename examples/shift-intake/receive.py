"""Receive Teo's unedited register against the two submitted ZIPs. No Studio or dependencies required."""
import argparse
import csv
from hashlib import sha256
import io
import json
from pathlib import Path
from zipfile import ZipFile

HERE = Path(__file__).resolve().parent


def inspect(register, packets):
    plan = json.loads((HERE / "protocol.json").read_bytes())
    b = register.read_bytes()
    rows = list(csv.DictReader(io.StringIO(b.decode("utf-8"), newline=""), strict=True))
    receipts = []
    for name, submitter in zip(plan["attachments"], plan["roles"][1:], strict=True):
        path = packets / name
        data = path.read_bytes()
        with ZipFile(io.BytesIO(data)) as z:
            names = z.namelist()
            assert z.testzip() is None and len(names) == len(set(names))
            members = {n: sha256(z.read(n)).hexdigest() for n in names}
            slip = z.read("Dispatch.html").decode()
            assert "<dt>Issuing persona</dt><dd dir=\"auto\">Vale</dd>" in slip
        matched = [r for r in rows if r["received_packet_filename"] == name]
        r = matched[0] if len(matched) == 1 else {}
        # This reader is for the issued register's semicolon-separated file
        # lists. Compare all entries, so an extra unreceived file cannot pass.
        recipient, _, field = r.get("files_for_recipient", "").partition(": ")
        files, metadata, rest = field.partition(". Packet also contains ")
        allowed = set(members) - {"Dispatch.html"}
        asserted = files.split("; ")
        digest = sha256(data).hexdigest()
        checks = {"one_row_with_exact_filename": len(matched) == 1, "outer_sha256": r.get("packet_sha256") == digest,
                  "issuer_and_submitter": r.get("contributor") == "Vale (persona-1); submitted by " + submitter,
                  "recipient": f"<h1 dir=\"auto\">{recipient}</h1>" in slip,
                  "exact_file_list": set(asserted) == allowed and len(asserted) == len(allowed),
                  "dispatch_sheet_identified_separately": metadata != "" and rest == "Dispatch.html"}
        receipts.append({"actual_submitter": submitter, "received_file": name, "packet_bytes": len(data),
                         "outer_sha256": digest, "members": members, "checks": checks, "status_as_written":
                         {k: r.get(k) for k in ("receipt_status", "missing_or_outstanding", "physical_status")}})
    return {"register_bytes": len(b), "register_sha256": sha256(b).hexdigest(), "data_rows": len(rows),
            "packets": receipts, "inventory_matches": len(rows) == len(receipts) and
            all(all(p["checks"].values()) for p in receipts),
            "scope": "Digital file intake. Read the quoted missing and physical-status fields against the dispatches and actual shift-close message."}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--register", type=Path, default=HERE / "receiving-register.csv")
    parser.add_argument("--packets", type=Path, default=HERE)
    args = parser.parse_args()
    result = inspect(args.register, args.packets)
    print(json.dumps(result, indent=2, ensure_ascii=False))
    raise SystemExit(0 if result["inventory_matches"] else 1)
