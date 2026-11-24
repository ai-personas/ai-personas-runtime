"""Offline operator audit of Teo's one service shift. No call or command is replayed."""
import argparse
import csv
import io
import json
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
from zipfile import ZipFile
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "stitch-desk"))
from receipt_audit import History, h

WORK = "work-c93d4dcd517c7f00e75e7d7ca9746f9c"


def trace(cli, root, host_log):
    x = History(cli, root)
    plan = json.loads((HERE / "protocol.json").read_bytes())
    unseen = json.loads((HERE.parent / "stitch-desk/protocol.json").read_bytes())["held_out"]
    intake = x.run("intake-history", WORK)
    assert intake["opening"]["need"] == plan["brief"] and intake["opening"]["requester"] == plan["owner"]
    assert intake["event_kinds"] == {"invitation": 1, "user_message": 2} and not intake["opening"]["files"]
    events = [dict(e, body=x.obj(e["digest"])) for e in x.db.execute("select * from events where work=? order by seq", [WORK])]
    owner = intake["events"][0]["target"]
    arrivals = {}
    for phase, name, submitter in zip("AB", plan["attachments"], plan["roles"][1:]):
        e, = [e for e in events if e["kind"] == "user_message" and e["actor"] == submitter]
        mat, = e["body"]["attachments"]
        content = (HERE / name).read_bytes()
        assert e["target"] == owner and e["body"]["text"] == plan["arrival_" + phase] and e["invocation"] is None
        assert mat["name"] == name and mat["sha256"] == h(content) and x.raw(mat["sha256"]) == content
        with ZipFile(io.BytesIO(content)) as z:
            assert not z.testzip()
            files = sorted(n for n in z.namelist() if n.startswith("Files/"))
        arrivals[e["seq"]] = {"from": submitter, "time": e["created"], "attachment": mat, "files": files}
    usage = x.run("usage", WORK)
    measured = {v["id"]: v for v in usage["calls"]}
    launches = [json.loads(line) for line in host_log.read_text().splitlines()]
    calls, commands, editions, from_provider = [], [], [], {}
    stdout_inputs = {}
    for row in x.db.execute("select * from invocations where work=? order by rowid", [WORK]):
        call = row["id"]
        snap, wire, choice, output = x.completion(row, measured[call], WORK, owner, plan)
        assert snap["data"]["work"]["record"]["original_need"] == plan["brief"] and unseen not in x.raw(row["snapshot"]).decode()
        assert row["state"] == "accepted" and choice["next_model"]["kind"] == "preserve"
        from_provider[call] = choice
        cu = x.record(f"character_use:{call}@1")
        cp = x.run("character-paths", f"experience:{call}@1")
        assert cu["snapshot"] == "object:" + row["snapshot"] and cu["response"] == "object:" + row["response"]
        assert cu["provider_receipt"] == "object:" + row["receipt"] and cu["wire_sha256"] == wire["request_sha256"]
        assert cp["supplied"]["numerical"] == cu["numerical"] and cp["basis"] == "baseline" and cp["epoch"] == cu["epoch"] == 1
        assert cp["supplied"]["paths"]["outputs"] == snap["data"]["baseline_character"]["outputs"] and len(cp["supplied"]["paths"]["outputs"]) == 12
        incoming = []
        for ev in snap["data"]["new_events"]:
            n = ev["event"]
            if n in arrivals:
                orig, = [e for e in events if e["seq"] == n]
                assert ev["body"] == orig["body"] and ev["actor"] == orig["actor"] and ev["target"] == owner and ev["digest"] == orig["digest"]
                incoming.append(n)
                arrivals[n].setdefault("first_accepted_input", call)
            if ev["kind"] == "tool_result" and ev["body"]["tool"] == "command.local":
                original, = [e for e in events if e["seq"] == n]
                assert ev["body"] == original["body"] and ev["digest"] == original["digest"]
                supplied = ev["supplied_result"]
                retained = x.obj(original["body"]["result"].removeprefix("object:"))
                stream = supplied["result"]["stdout"]
                source = retained["result"]["stdout"]
                assert supplied["receipt"] == retained["receipt"] and stream["reference"] == source["object"]
                b = x.raw(source["object"].removeprefix("object:"))
                assert stream["range"] == [0, len(b)] and stream["text"].encode() == b
                stdout_inputs[n] = {"decision": call, "bytes": len(b), "sha256": h(b)}
        calls.append({"id": call, "snapshot": row["snapshot"], "request": wire["request_sha256"], "terminal_output": output,
                      "receipt": row["receipt"], "response": row["response"], "new_human_returns": incoming, "admitted": row["created"],
                      "accepted": row["updated"], "observed_through": snap["observation_through"],
                      "epoch": cp["epoch"], "package": cu["package"], "numerical": cu["numerical"], "basis": cp["basis"],
                      "choice": choice["work"]["reason"], "maintenance": choice["persona_maintenance"]["kind"],
                      "next_model": choice["next_model"]["kind"], "continuation": choice["continuation"],
                      "operations": [o["tool"] for o in choice["operations"]]})
        for e in (v for v in events if v["invocation"] == call):
            if e["kind"] == "reply":
                assert e["body"]["text"] == choice["reply"] and e["target"] == "user"
                calls[-1]["reply"] = {"event": e["seq"], "text": e["body"]["text"]}
            if e["kind"] == "delivery" and "record" in e["body"]:
                ref = e["body"]["reference"]
                package = x.record(ref)
                assert package == e["body"]["record"] and package["owner"] == e["actor"] == owner and e["target"] == "user"
                asset, = package["artifacts"]
                f = x.record(asset)
                op, = [o for o in choice["operations"] if o["tool"] == "result.deliver"]
                assert all(op["input"][k] == package[k] for k in ("scope", "title", "supported_use", "remaining"))
                csv_bytes = x.raw(f["blob"])
                author = f["origin_decision"]
                save, = [o for o in from_provider[author]["operations"] if o["id"] == f["origin_operation"]]
                assert save["tool"] == "asset.save" and save["input"]["text"].encode() == csv_bytes
                assert save["input"]["path"] == f["path"] and f["last_content_author"] == owner and f["work"] == WORK
                handle, = op["input"]["artifacts"]
                if handle.startswith("$"):
                    assert handle[1:] == save["id"] and author == call
                    notice, = [v for v in events if v["invocation"] == call and v["kind"] == "tool_result" and
                               v["body"]["operation"] == save["id"] and v["body"]["tool"] == "asset.save" and v["seq"] < e["seq"]]
                    assert x.obj(notice["body"]["result"].removeprefix("object:"))["reference"] == asset
                else:
                    assert handle == asset
                index = len(editions)
                local = HERE / ("open.csv", "one-received.csv", "receiving-register.csv")[index]
                assert local.read_bytes() == csv_bytes
                editions.append({"event": e["seq"], "issue": call, "content_author_decision": author,
                                 "reference": ref, "file": local.name, "asset": asset,
                                 "bytes": len(csv_bytes), "sha256": f["blob"], "scope": package["scope"]})
                if op["input"].get("dispatches"):
                    slip, = op["input"]["dispatches"]
                    assert slip["recipient"] == package["dispatches"][0]["recipient"] == "Next clerk"
                    assert slip["instructions"].encode() == (HERE / "next-clerk.txt").read_bytes()
                    assert package["dispatches"][0]["instructions"] == slip["instructions"]
                    assert slip["artifacts"] == package["dispatches"][0]["artifacts"] == [asset]
                    editions[-1]["dispatch"] = slip
                    with TemporaryDirectory() as temp:
                        f = Path(temp) / "independent.csv"
                        x.run("export-dispatch", ref, "1", f)
                        assert f.read_bytes() == csv_bytes
        for out in x.db.execute("select * from outbox where invocation=? order by ordinal", [call]):
            op = x.obj(out["operation"])
            assert out["state"] == "completed" and op == choice["operations"][out["ordinal"]] and op["tool"] == "command.local"
            result = x.obj(out["result"])
            v = result["result"]
            notice, = [e for e in events if e["kind"] == "tool_result" and e["body"].get("effect") == out["id"]]
            assert notice["invocation"] == call and notice["actor"] == notice["target"] == owner
            assert notice["body"]["result"] == "object:" + out["result"] and notice["body"]["operation"] == op["id"]
            assert x.record(result["receipt"]) == v and v["process_started"] and v["origin"] == "persona"
            assert v["invocation"] == call and v["actor"] == owner and v["work"] == WORK and v["operation"] == op["id"]
            host = v["host"]
            assert v["argv0"] == op["input"]["program"] and v["arguments"] == op["input"]["arguments"]
            sha = h("\0".join([host["executable"], *v["arguments"]]).encode())
            entry, = [l for l in launches if l["argv_sha256"] == sha and l["cwd"] == host["working_directory"] and v["started"]-1 <= l["stamp"] <= v["finished"]+1]
            launches.remove(entry)
            for path, item in v["files_supplied"].items():
                assert item["reference"] == op["input"]["files"][path]
                assert item["bytes"] == len(x.raw(item["object"].removeprefix("object:")))
            commands.append({"decision": call, "event": notice["seq"], "receipt": result["receipt"], "operation": v["operation"],
                             "argv_sha256": sha, "exit": v["exit_code"], "mounts": v["files_supplied"]})
    assert not launches and len(measured) == len(calls) == usage["verified_completed_inputs"]
    for command in commands:
        command["stdout_in_subsequent_input"] = stdout_inputs.pop(command["event"])
    assert not stdout_inputs
    for n, a in arrivals.items():
        receipt = x.run("message-receipt", str(n))
        assert receipt["sender"] == a["from"] and receipt["received"]["decision"] == f"experience:{a['first_accepted_input']}@1"
        assert receipt["received"]["binding"] == "provider_output" and receipt["unverified_command_inputs"] == 0
        file, = receipt["attachments"]
        assert file["supplied"] is None and file["whole"] is None
        a["accepted_at"] = receipt["received"]["accepted_at"]
        a["command_inputs"] = file["local_command_inputs"]
        expected = [(c["event"], p, c["receipt"]) for c in commands for p, m in c["mounts"].items() if m["reference"] == a["attachment"]["reference"]]
        assert [(m["event"], m["path"], m["receipt"]) for m in a["command_inputs"]] == [
            (f"experience:event-{e}@1", p, r) for e, p, r in expected]
        for m in a["command_inputs"]:
            c, = [c for c in commands if c["receipt"] == m["receipt"]]
            out = c["stdout_in_subsequent_input"]
            assert m["stdout_supplied"]["decision"] == f"experience:{out['decision']}@1"
            assert m["stdout_supplied"]["binding"] == "provider_output" and m["stdout_supplied"]["range"] == [0, out["bytes"]]
            assert m["stdout_supplied"]["reference"] == "object:" + out["sha256"] and m["stdout_bytes"] == out["bytes"]
    def rows(e):
        return list(csv.DictReader(io.StringIO((HERE / e["file"]).read_text())))
    assert len(editions) == 3
    assert [r["receipt_status"] for r in rows(editions[0])] == ["Not received", "Not received"]
    intermediate = rows(editions[1])
    assert intermediate[0]["packet_sha256"] == arrivals[86]["attachment"]["sha256"] and intermediate[1]["receipt_status"] == "Not received"
    for row, a in zip(rows(editions[-1]), arrivals.values(), strict=True):
        assert a["from"] in row["contributor"] and "Vale" in row["contributor"]
        assert row["packet_sha256"] == a["attachment"]["sha256"] and row["received_packet_filename"] == a["attachment"]["name"]
        assert all(n in row["files_for_recipient"] for n in a["files"])
        assert all(n not in row["files_for_recipient"] for other in arrivals.values() if other != a for n in other["files"])
    # Waiting, eligibility and a decision are different observations. Compare the
    # actual participation and append-only call rows with the operator's station log.
    from boundaries import check
    intervals = check(root.parent, x, calls, events, arrivals)
    x.db.close()
    return {"work": WORK, "design": plan["design"], "live_runtime": plan["runtime"], "registration": "49fa173",
            "model": plan["model"], "effort": plan["effort"], "connection": plan["connection"],
            "intake_events": intake["events"], "service_observations": intervals, "arrivals": arrivals, "editions": editions,
            "verified_inputs_and_outputs": len(calls), "calls": calls, "commands": commands, "totals":
            {k: usage["totals"][k]["sum"] for k in ("input_tokens", "cached_input_tokens", "output_tokens", "actual_cost_microusd")}}


if __name__ == "__main__":
    args = argparse.ArgumentParser(description=__doc__)
    for key in ("cli","root","host_log"):
        args.add_argument(key, type=Path)
    print(json.dumps(trace(**vars(args.parse_args())), ensure_ascii=False, indent=2))
