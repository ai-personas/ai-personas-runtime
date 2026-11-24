"""Bind this one native P124 job to the opening archive, provider completions, command log and shop download."""
import argparse
import base64
import json
from pathlib import Path
import sqlite3
import sys
from tempfile import TemporaryDirectory

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "stitch-desk"))
from receipt_audit import History, h
from watch_epochs import digest


def read(p):
    return json.loads(p.read_bytes())


def contains_value(node, wanted):
    if isinstance(node, list):
        return any(contains_value(v, wanted) for v in node)
    if isinstance(node, dict):
        return any(contains_value(v, wanted) for v in node.values())
    return node == wanted


def audit(cli, station):
    w = read(station / "invitation.json")["work"]
    plan = read(HERE / "protocol.json")
    held = read(HERE.parent / "stitch-desk/protocol.json")["held_out"]
    x = History(cli, station / "Exhibit bench")
    intake = x.run("intake-history", w)
    assert intake["event_kinds"] == {"invitation": 1} and intake["opening"]["need"] == plan["brief"]
    assert intake["opening"]["requester"] == plan["owner"] and intake["events"][0]["target"] == plan["persona"]
    spec = (HERE / "BC-17-A.txt").read_bytes()
    attached, = intake["opening"]["files"]
    assert attached["name"] == "BC-17-A.txt" and x.raw(attached["sha256"]) == spec
    origin = read(station / "origin.json")
    pdf = station / "shop/project/reference/P124_dwg.pdf"
    assert origin["inputs"] == {attached["name"]: h(spec), pdf.name: digest(pdf)}
    assert digest(station / "origin.sqlite") == origin["source"]
    usage = x.run("usage", w)
    calls = {v["id"]: v for v in usage["calls"]}
    events = [dict(e, body=x.obj(e["digest"])) for e in x.db.execute("select * from events where work=? order by seq", [w])]
    choices, records, editions, commands = {}, [], [], []
    log = [json.loads(s) for s in (station / "control/shop-launches.jsonl").read_text().splitlines()]
    previous_method = "asset:asset-0043250054ccece696efd2ebda075d4e@1"
    method_text = x.raw(x.record(previous_method)["blob"]).decode()
    for row in x.db.execute("select * from invocations where work=? order by rowid", [w]):
        call, actor = row["id"], row["actor"]
        snap, wire, choice, out = x.completion(row, calls[call], w, actor, plan)
        data = snap["data"]
        assert data["work"]["record"]["original_need"] == plan["brief"] and held not in x.raw(row["snapshot"]).decode()
        item = {k: row[k] for k in ("id", "state", "actor", "snapshot", "response", "receipt")}
        item.update(request_sha256=wire["request_sha256"], terminal_output_sha256=out, package=snap["model"],
                    epoch=snap["epoch"], selected=[v["reference"] for v in data["selected_resources"]],
                    held_out_supplied=False, full_shop_spec=contains_value(data,spec.decode()),
                    prior_method_full_text=contains_value(data,method_text), prior_method_reference=previous_method in json.dumps(data),
                    reason=choice["work"]["reason"], continuation=choice["continuation"],
                    maintenance=choice["persona_maintenance"]["kind"], next_model=choice["next_model"]["kind"],
                    operations=[v["tool"] for v in choice["operations"]], image_inputs=[])
        for img, proof in zip(calls[call]["images"], wire.get("image_inputs", []), strict=True):
            blob = x.raw(img["reference"].removeprefix("object:"))
            encoded = b"data:"+img["mime"].encode()+b";base64,"+base64.b64encode(blob)
            assert h(encoded) == proof["image_url_sha256"] and len(encoded) == proof["image_url_bytes"]
            item["image_inputs"].append({"reference":img["reference"], "bytes":len(blob), "url_sha256":h(encoded)})
        records.append(item)
        current = [e for e in events if e["invocation"] == call]
        if row["state"] == "rejected":
            e, = current
            assert e["kind"] == "decision_rejected" and not e["body"]["effects_adopted"]
            assert not list(x.db.execute("select id from outbox where invocation=?", [call]))
            item["rejection"] = {"event":e["seq"], "reason":e["body"]["reason"]}
            continue
        assert row["state"] == "accepted"
        choices[call] = choice
        cu = x.record("character_use:"+call+"@1")
        cp = x.run("character-paths", "experience:"+call+"@1")
        assert cu["snapshot"] == "object:"+row["snapshot"] and cu["response"] == "object:"+row["response"]
        assert cu["provider_receipt"] == "object:"+row["receipt"] and cu["wire_sha256"] == wire["request_sha256"]
        assert cp["supplied"]["package"] == "object:"+cu["package"] and cp["supplied"]["numerical"] == cu["numerical"]
        assert snap["epoch"] == cp["epoch"] == cu["epoch"] and len(cp["supplied"]["paths"]["outputs"]) == 12
        item.update(basis=cp["basis"], numerical=cu["numerical"], inclinations=cp["supplied"]["paths"]["outputs"])
        for op in choice["operations"]:
            e, = [v for v in current if v["kind"] == "tool_result" and v["body"]["operation"] == op["id"]]
            assert e["actor"] == e["target"] == actor and e["body"]["tool"] == op["tool"]
        if choice.get("reply"):
            e, = [v for v in current if v["kind"] == "reply" and v["target"] == "user" and v["body"]["text"] == choice["reply"]]
            item["reply"] = {"event":e["seq"], "text":choice["reply"]}
        for e in current:
            if e["kind"] != "delivery" or "record" not in e["body"]:
                continue
            ref = e["body"]["reference"]
            body = x.record(ref)
            assert e["actor"] == body["owner"] == actor and body == e["body"]["record"]
            op, = [op for op in choice["operations"] if op["tool"] == "result.deliver"]
            assert all(op["input"][k] == body[k] for k in ("title","scope","supported_use","remaining"))
            slip, = body["dispatches"]
            author_slip, = op["input"]["dispatches"]
            assert all(author_slip[k] == slip[k] for k in ("instructions","recipient"))
            asset, = body["artifacts"]
            assert slip["artifacts"] == [asset]
            f = x.record(asset)
            name = Path(f["path"]).name
            download = station/"Native export"/name
            assert x.raw(f["blob"]) == download.read_bytes() == (HERE/name).read_bytes()
            assert (station/"Native export/shop-dispatch.txt").read_bytes() == (HERE/"shop-dispatch.txt").read_bytes() == slip["instructions"].encode()
            with TemporaryDirectory() as folder:
                dest = Path(folder)/"unchanged.zip"
                x.run("export-dispatch", ref, "1", dest)
                assert dest.read_bytes() == download.read_bytes()
            editions.append({"event":e["seq"], "issuing_choice":call, "reference":ref, "asset":asset, "file":name,
                             "bytes":download.stat().st_size, "sha256":f["blob"], "dispatch":slip,
                             "instructions_sha256":h(slip["instructions"].encode()), "remaining":body["remaining"]})
    assert len(calls) == usage["verified_completed_inputs"] == len(records)
    e_by_seq = {e["seq"]:e for e in events}
    def result(seq):
        return x.obj(e_by_seq[seq]["body"]["result"].removeprefix("object:"))
    for row in x.db.execute("select * from outbox where work=? order by rowid", [w]):
        operation = x.obj(row["operation"])
        selected = choices[row["invocation"]]["operations"][row["ordinal"]]
        assert selected["id"] == operation["id"] and selected["tool"] == operation["tool"]
        e, = [e for e in events if e["kind"] == "tool_result" and e["body"].get("effect") == row["id"]]
        assert row["state"] == "completed" and e["body"]["result"] == "object:"+row["result"]
        if operation["tool"] == "image.read":
            assert x.obj(row["result"])["image_blob"] == result(94)["result"]["outputs"]["p124.png"]["object"][7:]
            continue
        assert operation["tool"] == "command.local"
        output = x.obj(row["result"])
        r = output["result"]
        assert r == x.record(output["receipt"]) and r["actor"] == row["actor"] and r["invocation"] == row["invocation"]
        assert r["work"] == w and r["origin"] == "persona" and r["process_started"]
        assert r["arguments"] == selected["input"]["arguments"] and r["argv0"] == selected["input"]["program"]
        host = r["host"]
        program = r["argv0"] if host.get("receiving_launcher") else host["executable"]
        argv_sha = h("\0".join([program,*r["arguments"]]).encode())
        logged, = [v for v in log if v["argv_sha256"] == argv_sha and v["cwd"] == host["working_directory"] and
                   r["started"]-1 <= v["stamp"] <= r["finished"]+1]
        log.remove(logged)
        for file in [r["stdout"], r["stderr"], *r["outputs"].values()]:
            assert len(x.raw(file["object"][7:])) == file["bytes"]
        commands.append({"event":e["seq"], "call":row["invocation"], "id":operation["id"], "receipt":output["receipt"],
                         "program":r["argv0"], "argv_sha256":argv_sha,"host_log_matches":True,"exit":r["exit_code"],
                         "stdout":r["stdout"]["object"], "stderr":r["stderr"]["object"], "outputs":r["outputs"]})
    assert not log
    edition, = editions
    build = result(97)
    assert build["result"]["outputs"][edition["file"]]["object"] == "object:"+edition["sha256"]
    assert result(103)["runs"][0]["receipt"] == build["receipt"] and build["receipt"] in next(r for r in records if r["id"]==edition["issuing_choice"])["selected"]
    assert result(105)["registered_from"] == "object:"+edition["sha256"] and result(105)["reference"] == edition["asset"]
    assert e_by_seq[111]["body"]["delivery"] == edition["reference"] and result(110)["result"]["files_supplied"][edition["file"]]["object"] == "object:"+edition["sha256"]
    render = result(94)["result"]["outputs"]["p124.png"]
    assert all(im["reference"] == render["object"] for r in records for im in r["image_inputs"])
    armed = read(station/"Capture/armed.json")
    end = read(station/"Capture/observation.json")
    for observation in (armed, end["last_snapshot"]):
        archive = station/"Capture"/observation["archive"]
        assert digest(archive) == observation["sha256"]
        db = sqlite3.connect(archive.as_uri()+"?mode=ro&immutable=1",uri=True)
        assert db.execute("select max(seq) from events").fetchone() == (observation["through_event"],)
        db.close()
    switches = [e["seq"] for e in x.db.execute("select seq from events where kind='model_changed' and seq>? and seq<=?",
                                             [armed["through_event"],end["last_event"]])]
    assert not switches and not end["switch_batches"] and armed["heads"] == end["last_snapshot"]["heads"]
    x.db.close()
    return {"design":plan["design"], "live_runtime":plan["runtime"], "registered_before_invitation":"6dd2989",
            "source":origin,"work":w,"opening":intake["opening"],"operator_intake":intake["events"],"model":plan["model"],
            "connection":plan["connection"],"effort":plan["effort"],"states":usage["states"],"verified_inputs_and_outputs":len(records),
            "calls":records,"commands":commands,"edition":edition,"receiving_check":e_by_seq[111]["body"],
            "totals":{k:usage["totals"][k]["sum"] for k in ("input_tokens","cached_input_tokens","output_tokens","actual_cost_microusd")},
            "stop":read(station/"boundary.json"),"armed":armed,"observation":end,"eligible_switch_events":switches,"paired_arms_run":0}


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("cli",type=Path)
    p.add_argument("station",type=Path)
    args = p.parse_args()
    print(json.dumps(audit(args.cli,args.station.resolve()),ensure_ascii=False,indent=2))
