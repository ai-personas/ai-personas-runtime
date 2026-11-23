"""Read exact indexed Studio records and check a completion against its stored final carrier."""
from hashlib import sha256
import json
import sqlite3
import subprocess


def h(b):
    return sha256(b).hexdigest()


class History:
    def __init__(self, cli, root):
        self.cli, self.root = cli, root
        self.db = sqlite3.connect((root.resolve() / "neural.sqlite").as_uri() + "?mode=ro", uri=True)
        self.db.row_factory = sqlite3.Row

    def run(self, *args):
        return json.loads(subprocess.check_output([self.cli, "--state-root", self.root, "neural", *args]))

    def raw(self, digest):
        b, = self.db.execute("select bytes from objects where digest=?", [digest]).fetchone()
        assert h(b) == digest
        return b

    def obj(self, digest):
        return json.loads(self.raw(digest))

    def record(self, ref):
        kind, addr = ref.split(":", 1)
        id_, rev = addr.rsplit("@", 1)
        d, = self.db.execute("select digest from versions where kind=? and id=? and revision=?",
                             (kind, id_, int(rev))).fetchone()
        return self.obj(d)

    def completion(self, row, measured, work, owner, plan):
        call = row["id"]
        snap, receipt = self.obj(row["snapshot"]), self.obj(row["receipt"])
        wire = receipt["audit"]
        assert call == snap["invocation"] == wire["invocation_id"] and owner == snap["actor"] and snap["work"] == work
        assert row["state"] == measured["state"] and measured["input_evidence"] == "verified_completed_input"
        assert measured["snapshot"] == "object:" + row["snapshot"] and measured["receipt"] == "object:" + row["receipt"]
        assert receipt["reported_model"] == plan["model"] and receipt["reported_effort"] == plan["effort"]
        assert self.record(f"persona:{owner}@{snap['persona_revision']}")["provider"]["mode"] == "chatgpt_oauth"
        assert wire["transport_profile"] == "traditional_chatgpt_responses_v6" and receipt["response_completed"] and receipt["stream_eof"]
        output = self.db.execute("select digest,bytes from provider_outputs where receipt=?", [row["receipt"]]).fetchone()
        assert h(output["bytes"]) == output["digest"] == receipt["output_items_sha256"]
        msg, = [v for v in json.loads(output["bytes"])["terminal_output"]
                if v["type"] == "message" and v.get("phase") != "commentary"]
        text = "".join(v["text"] for v in msg["content"] if v["type"] == "output_text")
        assert text.encode() == self.raw(row["response"])
        choice = json.loads(text)
        assert choice["invocation"] == call
        return snap, wire, choice, output["digest"]
