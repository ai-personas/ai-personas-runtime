"""Offline receipt audit of the one ordinary Studio activity. No provider calls.

Give the CLI, the retained workspace, the native ZIP, the native clipboard
text, and the independent host launch log. Private histories are not published.
"""
import argparse
import base64
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from zipfile import ZipFile
from receipt_audit import History, h

WORK = "work-1e1c84185c4f4a626d844dd8911d162e"
QUESTION = "development:question-8cf226255d8de1b6af8f65ebe66599b4@1"


def study(cli, root, download, copied, log):
    history = History(cli, root)
    db, run, raw, obj, record = history.db, history.run, history.raw, history.obj, history.record

    plan = json.loads(Path(__file__).with_name("protocol.json").read_bytes())
    intake = run("intake-history", WORK)
    opening = intake["opening"]
    assert intake["event_kinds"] == {"invitation": 1} and opening["files"] == []
    assert opening["need"] == plan["ordinary"] and opening["requester"] == plan["operator"]
    owner = intake["events"][0]["target"]
    edition = None
    events = [dict(e, body=obj(e["digest"])) for e in db.execute(
        "select * from events where work=? order by seq", [WORK])]
    usage = run("usage", WORK)
    measured = {v["id"]: v for v in usage["calls"]}
    accepted, calls, tools = {}, [], []
    launches = [json.loads(line) for line in log.read_text().splitlines()]
    first = None
    for r in db.execute("select * from invocations where work=? order by rowid", [WORK]):
        call = r["id"]
        snap, wire, choice, output = history.completion(r, measured[call], WORK, owner, plan)
        data = snap["data"]
        assert data["work"]["record"]["original_need"] == plan["ordinary"] and plan["held_out"] not in raw(r["snapshot"]).decode()
        current = [e for e in events if e["invocation"] == call]
        if first is None:
            first = {"available_peers": [p["label"] for p in data["peers"] if p["id"] != owner],
                     "package": snap["model"], "epoch": snap["epoch"], "baseline": data["baseline_character"]["outputs"]}
        row = {k: r[k] for k in ("id", "state", "snapshot", "receipt", "response")}
        row.update(request_sha256=wire["request_sha256"], prompt_sha256=wire["prompt_sha256"],
                   terminal_output_sha256=output, supplied_package=snap["model"], epoch=snap["epoch"],
                   question_reminder=any(item["reference"] == QUESTION for item in data["development"]["personal_reminders"]),
                   selected_resources=[v["reference"] for v in data["selected_resources"]],
                   images=measured[call]["images"], operations=[v["tool"] for v in choice["operations"]],
                   judgment=choice["work"]["reason"], next_model=choice["next_model"]["kind"],
                   next_appraisal=choice["next_context"]["appraisal"]["kind"], behavioral_scope=choice.get("behavior"),
                   maintenance=choice["persona_maintenance"], continuation=choice["continuation"]["kind"])
        calls.append(row)
        if r["state"] == "rejected":
            rejected, = [e for e in current if e["kind"] == "decision_rejected"]
            assert not rejected["body"]["effects_adopted"] and not list(db.execute("select id from outbox where invocation=?", [call]))
            row["rejection"] = rejected["body"]["reason"]
            continue
        assert r["state"] == "accepted"
        accepted[call] = choice
        cu = record(f"character_use:{call}@1")
        cp = run("character-paths", f"experience:{call}@1")
        assert cu["snapshot"] == "object:" + r["snapshot"] and cu["response"] == "object:" + r["response"]
        assert cu["provider_receipt"] == "object:" + r["receipt"] and cu["wire_sha256"] == wire["request_sha256"]
        assert cp["basis"] == "baseline" and cp["supplied"]["package"] == "object:" + first["package"]
        assert cp["supplied"]["paths"]["outputs"] == first["baseline"] and cp["appraisal"] == {"features": {}}
        assert cp["supplied"]["numerical"] == cu["numerical"] and len(first["baseline"]) == 12
        row.update(numerical=cu["numerical"], basis=cp["basis"], applied_question=choice.get("examining"))
        for image, audit in zip(row["images"], cu["provider_image_audit"], strict=True):
            blob = raw(image["reference"].removeprefix("object:"))
            wire_image = b"data:" + image["mime"].encode() + b";base64," + base64.b64encode(blob)
            assert h(wire_image) == audit["image_url_sha256"] and len(wire_image) == audit["image_url_bytes"]
            with ZipFile(download) as z:
                assert blob == z.read("front_preview.png")
        for op in choice["operations"]:
            event, = [e for e in current if e["kind"] == "tool_result" and
                      e["body"]["operation"] == op["id"] and e["body"]["tool"] == op["tool"]]
            assert event["actor"] == event["target"] == owner
        if choice.get("reply"):
            reply, = [e for e in current if e["kind"] == "reply" and e["target"] == "user"]
            assert choice["reply"] == reply["body"]["text"]
            row["reply"] = {"event": reply["seq"], "text": choice["reply"]}
        for e in current:
            if e["kind"] != "delivery" or not e["body"].get("record"):
                continue
            assert edition is None and e["actor"] == owner and e["target"] == "user"
            ref = e["body"]["reference"]
            body = record(ref)
            assert body == e["body"]["record"]
            op, = [o for o in choice["operations"] if o["tool"] == "result.deliver"]
            slip, = body["dispatches"]
            issued_slip, = op["input"]["dispatches"]
            assert all(slip[k] == issued_slip[k] for k in ("instructions", "recipient"))
            assert copied.read_bytes() == slip["instructions"].encode()
            asset, = slip["artifacts"]
            assert body["artifacts"] == [asset]
            f = record(asset)
            assert raw(f["blob"]) == download.read_bytes()
            with TemporaryDirectory() as folder:
                dest = Path(folder) / "out.zip"
                run("export-dispatch", ref, "1", dest)
                assert dest.read_bytes() == download.read_bytes()
            edition = {"event": e["seq"], "call": call, "reference": ref, "asset": asset, "path": f["path"],
                       "sha256": f["blob"], "bytes": len(download.read_bytes()), "slip": slip, "remaining": body["remaining"]}
    assert len(calls) == len(measured) == usage["verified_completed_inputs"]
    for t in db.execute("select * from outbox where work=? order by rowid", [WORK]):
        op = obj(t["operation"])
        chosen, = [v for v in accepted[t["invocation"]]["operations"] if v["id"] == op["id"]]
        v = obj(t["result"])
        run_ = v["result"]
        host = run_["host"]
        assert t["state"] == "completed" and record(v["receipt"]) == run_
        assert run_["origin"] == "persona" and run_["invocation"] == t["invocation"] and run_["actor"] == owner
        assert run_["argv0"] == chosen["input"]["program"] and run_["arguments"] == chosen["input"]["arguments"]
        argv = [run_["argv0"] if host.get("receiving_launcher") else host["executable"], *run_["arguments"]]
        argv_sha = h("\0".join(argv).encode())
        logged, = [l for l in launches if l["argv_sha256"] == argv_sha and l["cwd"] == host["working_directory"]
                   and run_["started"] - 1 <= l["stamp"] <= run_["finished"] + 1]
        launches.remove(logged)
        info = {"call": t["invocation"], "id": op["id"], "receipt": v["receipt"], "argv_sha256": argv_sha,
                "environment": host["directory_mode"], "exit": run_["exit_code"], "outputs": run_["outputs"]}
        if run_.get("receiving"):
            check, = [e for e in events if e["kind"] == "delivery_check" and e["body"]["effect"] == t["id"]]
            assert check["body"]["delivery"] == run_["receiving"]["delivery"] == edition["reference"]
            assert run_["files_supplied"][edition["path"]]["object"] == "object:" + edition["sha256"]
            info["receiving"] = check["body"]
        tools.append(info)
    assert not launches
    authored = dict(db.execute("select kind,count(*) from versions where invocation in (select id from invocations where work=?) group by kind", [WORK]))
    switches = [e["seq"] for e in events if e["kind"] == "model_changed"]
    assert not switches and not any(authored.get(k) for k in ("candidate", "assessment", "persona", "question_use", "development"))
    db.close()
    return {"work": WORK, "intake": opening, "invitation": intake["events"][0], "opening_character": first,
            "states": usage["states"], "verified_inputs_and_outputs": len(calls), "calls": calls, "commands": tools,
            "edition": edition, "switch_events": switches, "authored_versions": authored,
            "totals": {k: usage["totals"][k]["sum"] for k in ("input_tokens", "cached_input_tokens", "output_tokens", "actual_cost_microusd")}}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ("cli", "root", "download", "copied", "log"):
        parser.add_argument(key, type=Path)
    print(json.dumps(study(**vars(parser.parse_args())), ensure_ascii=False, indent=2))
