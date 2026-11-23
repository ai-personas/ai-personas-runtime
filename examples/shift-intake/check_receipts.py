"""Check attachment attribution using temporary copies of the settled shift. No inference or command replay."""
import argparse
import json
from pathlib import Path
import shutil
import sqlite3
import subprocess
from tempfile import TemporaryDirectory

HERE = Path(__file__).resolve().parent


def verify(cli, archive):
    record = json.loads((HERE / "decisions.json").read_bytes())
    lead = record["arrivals"]["95"]
    mount, = lead["command_inputs"]
    def call(ref):
        return ref.removeprefix("experience:").removesuffix("@1")
    results = {}
    for case in ("original", "missing_producer_output", "missing_following_output", "uncertain_effect", "later_identical_file"):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            dbfile = root / "neural.sqlite"
            shutil.copyfile(archive, dbfile)
            db = sqlite3.connect(dbfile)
            def run(*args):
                return json.loads(subprocess.check_output([cli, "--state-root", root, "--operator", "Offline fixture", "neural", *args]))
            first = db.execute("select count(*) from invocations").fetchone()[0]
            assert db.execute("select max(seq) from events").fetchone() == (106,)
            n = 95
            if case.startswith("missing_"):
                lost = mount["decision"] if case == "missing_producer_output" else mount["stdout_supplied"]["decision"]
                q = db.execute("delete from provider_outputs where receipt=(select receipt from invocations where id=?)", [call(lost)])
                assert q.rowcount == 1
            elif case == "uncertain_effect":
                q = db.execute("update outbox set state='uncertain' where invocation=? and state='completed'", [call(mount["decision"])])
                assert q.rowcount == 1
            db.commit()
            if case == "later_identical_file":
                sent = run("message", record["work"], record["intake_events"][0]["target"],
                           "Receipt inspection on a disconnected copy.", "--attach", HERE / lead["attachment"]["name"])
                n = sent["event"]
                assert n > 106 and not sent["stopped_participation_resumed"]
            actual = run("message-receipt", str(n))
            file, = actual["attachments"]
            assert file["reference"] == lead["attachment"]["reference"] and file["supplied"] is None
            rows = file.get("local_command_inputs", [])
            assert bool(actual["received"]) == (case != "later_identical_file")
            assert len(rows) == int(case in ("original", "missing_following_output"))
            assert bool(rows and rows[0].get("stdout_supplied")) == (case == "original")
            assert actual["unverified_command_inputs"] == int(case == "missing_producer_output")
            assert db.execute("select count(*) from invocations").fetchone() == (first,)
            results[case] = {"message_received": bool(actual["received"]), "verified_mounts": len(rows),
                             "verified_stdout_supply": bool(rows and rows[0].get("stdout_supplied")),
                             "unverified_command_inputs": actual["unverified_command_inputs"], "new_calls": 0}
            db.close()
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli", type=Path)
    parser.add_argument("archive", type=Path, help="The study's unmodified Capture/ended.sqlite")
    print(json.dumps(verify(**vars(parser.parse_args())), indent=2))
