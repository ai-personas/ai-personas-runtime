"""Recheck a settled study activity against its original Studio database, offline.

The usage reader reconstructs provider inputs; the terminal output is compared
separately below. A local projection on another package never executes a choice.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
from tempfile import TemporaryDirectory

from freeze_ren import MODELS


def digest(b):
    return hashlib.sha256(b).hexdigest()


def audit(root, cli, work, native=None, launches=None):
    db = sqlite3.connect((root.resolve() / "neural.sqlite").as_uri() + "?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    def run(*args):
        return json.loads(subprocess.check_output([cli, "--state-root", root, "neural", *args]))
    def raw(h):
        b, = db.execute("select bytes from objects where digest=?", (h,)).fetchone()
        assert digest(b) == h
        return b
    def obj(h):
        return json.loads(raw(h))
    def record(ref):
        kind, tail = ref.split(":", 1)
        id_, rev = tail.rsplit("@", 1)
        h, = db.execute("select digest from versions where kind=? and id=? and revision=?", (kind, id_, int(rev))).fetchone()
        return obj(h)
    def has_text(v, text):
        if isinstance(v, dict):
            return any(has_text(value, text) for value in v.values())
        if isinstance(v, list):
            return any(has_text(value, text) for value in v)
        return v == text
    plan = json.loads(Path(__file__).with_name("protocol.json").read_bytes())
    drawing = Path(__file__).with_name("drawing-C.txt").read_bytes()
    trace = run("intake-history", work)
    opening = trace["opening"]
    assert trace["event_count"] == 1 and trace["events"][0]["kind"] == "invitation"
    assert opening["need"] == plan["manufacturing_brief" if native else "review"]
    assert opening["requester"] == plan["operator"] and opening["source"] == "operator"
    actor = trace["events"][0]["target"]
    returned = None if native else plan["mara_return" if actor == "mara" else "linn_return"]
    if native:
        file, = opening["files"]
        assert file["name"] == "drawing-C.txt" and raw(file["sha256"]) == drawing
    usage = run("usage", work)
    measured = {r["id"]: r for r in usage["calls"]}
    report = {"work": work, "opening": opening, "invitation": trace["events"][0], "states": usage["states"],
              "verified_inputs": usage["verified_completed_inputs"], "call_span_s": usage["recorded_call_span_seconds"],
              "totals": {k: usage["totals"][k]["sum"] for k in ("input_tokens", "cached_input_tokens", "output_tokens", "actual_cost_microusd")},
              "calls": [], "editions": [], "commands": [], "terminal_replies": [], "changes": []}
    events = [dict(e, body=obj(e["digest"])) for e in db.execute("select * from events where work=? order by seq", (work,))]
    response = {}
    for row in db.execute("select * from invocations where work=? order by rowid", (work,)):
        call = row["id"]
        snap, receipt = obj(row["snapshot"]), obj(row["receipt"])
        data, wire = snap["data"], receipt["audit"]
        assert snap["work"] == work and snap["actor"] == actor and snap["invocation"] == call
        assert measured[call]["input_evidence"] == "verified_completed_input"
        assert receipt["response_completed"] and receipt["stream_eof"] and wire["invocation_id"] == call
        assert receipt["reported_model"] == plan["model"] and receipt["reported_effort"] == plan["effort"]
        assert wire["transport_profile"] == "traditional_chatgpt_responses_v6"
        carrier = db.execute("select digest,bytes from provider_outputs where receipt=?", (row["receipt"],)).fetchone()
        assert digest(carrier["bytes"]) == carrier["digest"] == receipt["output_items_sha256"]
        msg, = [v for v in json.loads(carrier["bytes"])["terminal_output"] if v["type"] == "message" and v.get("phase") != "commentary"]
        text = "".join(v["text"] for v in msg["content"] if v["type"] == "output_text").encode()
        assert text == raw(row["response"])
        choice = json.loads(text)
        assert choice["invocation"] == call and data["work"]["record"]["original_need"] == opening["need"]
        current = [e for e in events if e["invocation"] == call]
        r = {k: row[k] for k in ("id", "state", "actor", "snapshot", "receipt", "response")}
        r.update(request_sha256=wire["request_sha256"], terminal_output_sha256=carrier["digest"],
                 schema_sha256=wire["schema_sha256"], configuration_sha256=wire["configuration_sha256"],
                 instructions_sha256=wire["instructions_sha256"], contract=snap["contract_object"],
                 selected=[v["reference"] for v in data["selected_resources"]],
                 working_directory=data["host_working_directory"], tool_names=snap["tool_names"],
                 full_drawing_supplied=has_text(data, drawing.decode()), full_return_supplied=bool(returned and has_text(data, returned)))
        report["calls"].append(r)
        if len(report["calls"]) == 1:
            report["opening_input"] = {"tools": data["available_tools"], "peers": data["peers"],
                                       "development": data["development"], "materials": data["work"]["record"].get("materials")}
        if row["state"] != "accepted":
            e, = [e for e in current if e["kind"] == "decision_rejected"]
            assert not e["body"]["effects_adopted"]
            r.update(rejection=e["body"]["reason"], attempted_behavior=choice.get("behavior"),
                     attempted_maintenance=choice.get("persona_maintenance"))
            continue
        response[call] = choice
        r.update(maintenance=choice["persona_maintenance"], next_model=choice["next_model"]["kind"],
                 next_appraisal=choice["next_context"]["appraisal"]["kind"], behavior=(choice.get("behavior") or {}).get("kind"),
                 judgment=choice["work"], continuation=choice["continuation"], operations=[])
        cu = record("character_use:" + call + "@1")
        assert cu["owner"] == actor and cu["work"] == work and cu["wire_sha256"] == wire["request_sha256"]
        assert all(cu[key] == "object:" + row[col] for key, col in (("snapshot", "snapshot"), ("response", "response"), ("provider_receipt", "receipt")))
        alternate = ["--compare", "object:" + MODELS[2 - snap["epoch"]]] if native else []
        cp = run("character-paths", "experience:" + call + "@1", *alternate)
        assert cp["supplied"]["numerical"] == cu["numerical"] and cp["supplied"]["package"] == "object:" + cu["package"]
        assert cp["epoch"] == snap["epoch"] and len(cp["supplied"]["paths"]["outputs"]) == 12
        r.update(package=cu["package"], epoch=cu["epoch"], numerical=cu["numerical"], appraisal=cp["appraisal"],
                 basis=cp["basis"], supplied=cp["supplied"]["paths"]["outputs"])
        if native:
            assert cu["package"] == MODELS[snap["epoch"] - 1] and cp["alternate"]["reference"] == alternate[1]
            r.update(alternate_package=cp["alternate"]["package"], changed_outputs=cp["alternate"]["changed_outputs"])
        for op in choice["operations"]:
            e, = [e for e in current if e["kind"] == "tool_result" and e["actor"] == e["target"] == actor
                  and e["body"]["operation"] == op["id"] and e["body"]["tool"] == op["tool"]]
            r["operations"].append({"tool": op["tool"], "local_id": op["id"], "event": e["seq"], "result": e["body"]["result"]})
        if choice.get("reply"):
            e, = [e for e in current if e["kind"] == "reply" and e["target"] == "user" and e["body"]["text"] == choice["reply"]]
            report["terminal_replies"].append({"event": e["seq"], "call": call, "text": choice["reply"]})
    assert len(measured) == len(report["calls"]) == report["verified_inputs"]
    for e in events:
        if e["kind"] == "delivery" and e["body"].get("record"):
            body = e["body"]["record"]
            ref = e["body"]["reference"]
            assert body == record(ref) and body["owner"] == actor and e["target"] == "user"
            op, = [x for x in response[e["invocation"]]["operations"] if x["tool"] == "result.deliver"]
            assert all(op["input"][k] == body[k] for k in ("title", "scope", "remaining", "supported_use"))
            artifact, = body["artifacts"]
            slip, = body["dispatches"]
            authored_slip, = op["input"]["dispatches"]
            assert slip["artifacts"] == [artifact] and all(authored_slip[k] == slip[k] for k in ("instructions", "recipient"))
            asset = record(artifact)
            before = raw(asset["blob"])
            listing = run("file-deliveries", artifact)
            assert listing["unverified"] == 0 and any(z["reference"] == ref for z in listing["rows"])
            with TemporaryDirectory() as folder:
                dest = Path(folder) / "receiver.zip"
                run("export-dispatch", ref, "1", dest)
                assert before == dest.read_bytes() == native.read_bytes()
            report["editions"].append({"reference": ref, "event": e["seq"], "call": e["invocation"],
                                      "asset": artifact, "file": asset["path"], "bytes": len(before), "sha256": asset["blob"],
                                      "instructions": slip["instructions"], "recipient": slip["recipient"], "remaining": body["remaining"]})
        elif e["kind"] in ("model_changed", "assessment", "personal_trial_proposed", "question_use", "human_request", "work_request"):
            report["changes"].append({"kind": e["kind"], "event": e["seq"], "call": e["invocation"]})
    for t in db.execute("select * from outbox where work=? order by rowid", (work,)):
        queued = obj(t["operation"])
        op, = [o for o in response[t["invocation"]]["operations"] if o["id"] == queued["id"]]
        assert op["tool"] == queued["tool"] == "command.local" and t["state"] == "completed"
        result = obj(t["result"])
        v = result["result"]
        host = v["host"]
        assert v["origin"] == "persona" and v["invocation"] == t["invocation"] and v["actor"] == actor
        assert v["operation"] == op["id"] and v["arguments"] == op["input"]["arguments"] and v["argv0"] == op["input"]["program"]
        argv = [v["argv0"] if host.get("receiving_launcher") else host["executable"], *v["arguments"]]
        argv_sha = digest("\0".join(argv).encode())
        if launches:
            assert any(z["cwd"] == host["working_directory"] and v["started"]-1 <= z["stamp"] <= v["finished"]+1
                       and argv_sha == z["argv_sha256"] for z in launches)
        if v.get("receiving"):
            assert v["receiving"]["delivery"] == ref
            assert v["receiving"]["purpose"] == op["input"]["receiving"]["purpose"]
            check, = [e["body"] for e in events if e["kind"] == "delivery_check" and e["body"]["effect"] == t["id"]]
            assert check["delivery"] == ref and check["files_staged"] == 1 and check["exit_code"] == v["exit_code"]
        report["commands"].append({"call": t["invocation"], "receipt": result["receipt"], "result": t["result"],
            "origin": v["origin"], "launched": v["process_started"], "exit": v["exit_code"], "argv_sha256": argv_sha,
            "host_log_matched": bool(launches), "launcher_sha256": host["launcher"]["sha256_before_launch"],
            "executable_sha256": host["executable_sha256_before_launch"], "receiving": v.get("receiving"),
            "directory": host["directory_mode"], "outputs": v["outputs"], "stdout": v["stdout"]["object"], "stderr": v["stderr"]["object"]})
    assert bool(report["editions"]) == bool(native)
    db.close()
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workspace", type=Path)
    parser.add_argument("work")
    parser.add_argument("--cli", default="ai-personas")
    parser.add_argument("--native-download", type=Path)
    parser.add_argument("--launch-log", type=Path)
    args = parser.parse_args()
    log = [json.loads(l) for l in args.launch_log.read_text().splitlines()] if args.launch_log else None
    print(json.dumps(audit(args.workspace, args.cli, args.work, args.native_download, log), indent=2))
