"""Check the native listener log against the append-only history and both observer archives."""
from hashlib import sha256
import json
import sqlite3


def read(path):
    return json.loads(path.read_bytes())


def check(station, history, calls, events, arrivals):
    points = {}
    def boundary(label, logfile, member=None):
        state = read(station / (logfile + ".json"))
        state = state[member] if member else state
        listener = state["runner"]
        obs = {k: state[k] for k in ("stamp", "seq", "calls", "turn", "cursor", "continuation")}
        obs["listener"] = listener["state"]
        if "session" in listener:
            session = listener["session"]
            assert session["scope"] == {"kind": "activity_tree", "work": events[0]["work"]} and session["watch"]
            obs["session"] = session["id"]
        assert not state["busy"] and obs["calls"] == obs["turn"]
        # Calls within the same integer second as a native click may or may not
        # have been admitted yet; do not invent a fractional DB timestamp.
        sec = int(obs["stamp"])
        assert sum(c["admitted"] < sec for c in calls) <= obs["calls"] <= sum(c["admitted"] <= sec for c in calls)
        if obs["turn"]:
            decision = calls[obs["turn"]-1]["continuation"].copy()
            if decision["kind"] == "wait":
                decision.setdefault("wake_at_unix", None)
            assert decision == obs["continuation"]
        points[label] = obs
        return obs
    start = boundary("before_run", "initial-run")
    waiting = boundary("first_wait", "initial-boundary")
    first = boundary("before_relief_send", "arrival-A", "prior")
    updated = boundary("second_wait", "A-boundary")
    stopping = boundary("before_finish_listener", "offline", "prior")
    offline = boundary("listener_offline", "offline", "ended")
    submitting = boundary("before_lead_send", "arrival-B", "prior")
    reconnect = boundary("before_reconnect", "reconnect")
    final = boundary("chosen_stop", "final-boundary")
    ended = boundary("listener_released", "released", "ended")
    assert start["listener"] == ended["listener"] == "unobserved"
    assert waiting["session"] == first["session"] == updated["session"] == stopping["session"] != final["session"]
    assert [p["stamp"] for p in points.values()] == sorted(p["stamp"] for p in points.values())
    assert waiting["calls"] == 5 and waiting["continuation"]["kind"] == updated["continuation"]["kind"] == "wait"
    for p in (updated, stopping, offline, submitting, reconnect):
        assert p["calls"] == 8 and p["cursor"] == 89 and "user_message" in p["continuation"]["on_events"]
    assert all(p["listener"] == "unobserved" for p in (offline, submitting, reconnect))
    assert offline["seq"] == submitting["seq"] == 94 and reconnect["seq"] == 95
    between = [e for e in events if offline["seq"] < e["seq"] <= reconnect["seq"]]
    assert len(between) == 1 and between[0]["kind"] == "user_message" and between[0]["invocation"] is None
    for phase, prior, after in (("A", first, updated), ("B", submitting, reconnect)):
        log = read(station / ("arrival-" + phase + ".json"))
        assert prior["stamp"] <= log["stamp"] <= after["stamp"] and log["event"] == prior["seq"]+1
        assert log["body"]["attachments"] == [arrivals[log["event"]]["attachment"]]
    assert not any(e["kind"] in ("model_changed", "explicit_resume") for e in events)
    assert calls[8]["id"] == arrivals[95]["first_accepted_input"] and calls[8]["observed_through"] == 95
    assert final["calls"] == ended["calls"] == 13 and final["continuation"]["kind"] == "stop"
    assert final["stamp"]-start["stamp"] < 25*60

    armed = read(station / "Capture/armed.json")
    observation = read(station / "Capture/observation.json")
    last = observation["last_snapshot"]
    assert armed["heads"] == last["heads"] and not observation["switch_batches"]
    assert all(p["epoch"] == 1 for p in last["heads"].values())
    for point in (armed, last):
        archive = station / "Capture" / point["archive"]
        assert sha256(archive.read_bytes()).hexdigest() == point["sha256"]
        db = sqlite3.connect(archive.resolve().as_uri() + "?mode=ro&immutable=1", uri=True)
        assert db.execute("select max(seq) from events").fetchone() == (point["through_event"],)
        heads = {id_: {k: json.loads(b)[k2] for k, k2 in (("epoch","epoch"),("package","model"))}
                 for id_, b in db.execute("select id,bytes from heads join objects using(digest) where kind='persona'")}
        assert point["heads"] == heads and not point["unsettled_calls_or_effects"]
        db.close()
    assert armed["through_event"] == observation["first_event"] == 71 and last["through_event"] == observation["last_event"] == 106
    assert not history.db.execute("select 1 from events where seq>? and seq<=? and kind='model_changed'",
                                  [armed["through_event"], last["through_event"]]).fetchone()
    return {"boundaries": points, "gap_between_unobserved_readings_s": round(reconnect["stamp"] - offline["stamp"], 3),
            "turns_before_and_after_gap": [offline["turn"], reconnect["turn"]], "events_in_gap": [e["seq"] for e in between],
            "seconds_to_chosen_stop": round(final["stamp"]-start["stamp"], 3),
            "observer": {"before_invitation": armed, "end": last, "switches": [], "held_out_arms_run": 0}}
