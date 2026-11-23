"""Index retained numerical character changes in saved Studio databases, offline.

python3 scripts/character_history.py path/to/neural.sqlite [...]
python3 scripts/character_history.py --list paths.txt [--as-of earlier-report.json]

Reads character history, candidate records, change events, packages and authorizing decisions.
Copies count as sources, not independent adoptions. An earlier report pins both the
source set and each read boundary; new ordinary work need not be stopped to replay it.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
from collections import Counter
from contextlib import closing


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha(b):
    return hashlib.sha256(b).hexdigest()


def encoded(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def locator(p):
    # Keep operator paths out of the public index.
    name = "~/" + p.relative_to(Path.home()).as_posix() if p.is_relative_to(Path.home()) else str(p)
    return sha(name.encode())


def blob(db, digest):
    r = db.execute("SELECT bytes FROM objects WHERE digest=?", (digest,)).fetchone()
    require(r is not None and sha(r[0]) == digest, f"missing or altered object {digest}")
    return r[0]


def coordinates(pack):
    values = {(p["tensor"], p["row"], p["column"]): p["value"] for p in pack["parameters"]}
    require(len(values) == len(pack["parameters"]), "duplicate numerical coordinate")
    return values


def census_source(db, cutoff):
    db.execute("PRAGMA query_only=ON")
    db.execute("BEGIN")
    if cutoff is None:
        v = db.execute("SELECT coalesce(max(rowid),0) FROM versions WHERE kind IN ('persona','candidate')").fetchone()[0]
        e = db.execute("SELECT coalesce(max(seq),0) FROM events WHERE kind='model_changed'").fetchone()[0]
    else:
        v, e = cutoff
    previous, pairs, initial, starts, changes = {}, {}, set(), set(), {}
    ledger = hashlib.sha256()
    versions = unknown = same_epoch_config = 0
    active = False
    for owner, rev, call, digest in db.execute(
        "SELECT id,revision,invocation,digest FROM versions WHERE kind='persona' AND rowid<=? "
        "ORDER BY id,revision", (v,)
    ):
        ledger.update(encoded(["persona", owner, rev, call, digest]) + b"\n")
        versions += 1
        person = json.loads(blob(db, digest))
        ep, model = person.get("epoch"), person.get("model")
        old = previous.get(owner)
        previous[owner] = (rev, ep, model)
        if ep is None:
            require(model is None and (not old or old[2] is None), "missing epoch on active character")
            unknown += 1
            continue
        require(isinstance(ep, int) and ep >= 0 and bool(ep) == bool(model), "inconsistent active epoch")
        if ep:
            active = True
        if ep == 1:
            initial.add((owner, model))
        if old and old[1] == ep:
            require(old[2] == model, f"package changed without an epoch: {owner}@{rev}")
            if ep > 1 and call is None:
                same_epoch_config += 1
        elif ep > 1:
            require(old and old[:2] == (rev - 1, ep - 1) and old[2] and call,
                    f"missing predecessor or writer: {owner}@{rev}")
            key = (owner, ep, call)
            require(key not in pairs, f"duplicate character transition: {key}")
            pairs[key] = (rev, old[2], model)
        elif old and old[1]:
            raise ValueError(f"epoch regressed: {owner}@{rev}")

    for seq, work, owner, call, digest in db.execute(
        "SELECT seq,work,actor,invocation,digest FROM events WHERE kind='model_changed' AND seq<=? "
        "ORDER BY seq", (e,)
    ):
        ledger.update(encoded(["model_changed", seq, work, owner, call, digest]) + b"\n")
        ev = json.loads(blob(db, digest))
        ep, model = ev["epoch"], ev["model"]
        require(isinstance(ep, int) and ep > 0, f"invalid model change at event {seq}")
        if ep == 1:
            require((owner, model) in initial, f"missing initial character version: {owner}")
            starts.add((owner, model))
            continue
        rev, before, after = pairs.pop((owner, ep, call))
        require(after == model, f"event and persona disagree: {call}")
        row = db.execute("SELECT work,actor,state,snapshot,response,receipt FROM invocations WHERE id=?",
                         (call,)).fetchone()
        require(row and row[:3] == (work, owner, "accepted"), f"missing accepted owner decision: {call}")
        snap, reply, receipt = row[3:]
        s, y = (json.loads(blob(db, h)) for h in (snap, reply))
        choice = ev["choice"]
        require((s["invocation"], s["work"], s["actor"], s["epoch"], s["model"]) ==
                (call, work, owner, ep - 1, before), f"pre-change input mismatch: {call}")
        require(y["invocation"] == call and y["next_model"] == choice and
                choice["expected_model"] == before, f"decision and event disagree: {call}")
        a, b = (json.loads(blob(db, h)) for h in (before, after))
        require(a["persona"] == b["persona"] == owner, f"package owner mismatch: {call}")
        left, right = coordinates(a), coordinates(b)
        edits = [[*k, left.get(k, 0), right.get(k, 0)] for k in sorted(left.keys() | right.keys())
                 if left.get(k, 0) != right.get(k, 0)]
        shape = [[p["contract"], p["hidden_units"]] for p in (a, b)]
        require(call not in changes, f"reused change decision: {call}")
        changes[call] = dict(owner=owner, epoch=ep, event=seq, event_sha256=digest, work=work,
                             revision=rev, choice=choice["kind"], candidate=choice.get("candidate"),
                             before=before, after=after, edits=edits, shape=shape,
                             numerical_change=bool(edits or shape[0] != shape[1]),
                             snapshot=snap, response=reply, receipt=receipt)
    require(not pairs, f"character transitions with no matching change event: {list(pairs)}")
    candidates = {}
    for i, call, digest in db.execute(
        "SELECT id,invocation,digest FROM versions WHERE kind='candidate' AND revision=1 AND rowid<=? "
        "ORDER BY id", (v,)
    ):
        record = json.loads(blob(db, digest))
        if record.get("predecessor") is not None:
            ref = f"candidate:{i}@1"
            candidates[ref] = dict(owner=record["owner"], writer=call, predecessor=record["predecessor"],
                                   model=record["model"], record_sha256=digest)
            ledger.update(encoded([ref, call, digest]) + b"\n")
    for call, change in changes.items():
        if change["choice"] == "activate":
            c = candidates[change["candidate"]]
            require((c["owner"], c["predecessor"], c["model"]) ==
                    (change["owner"], change["before"], change["after"]), f"wrong candidate: {call}")
        else:
            require(change["choice"] == "rollback", f"unknown transition: {call}")
    category = ("active" if active else "no_persona_records" if not versions else
                "no_numerical_records" if unknown == versions else "not_activated")
    return dict(cutoff=[v, e], history_sha256=ledger.hexdigest(), category=category,
                persona_versions=versions, records_without_epoch=unknown,
                epoch_one_without_start_event=len(initial - starts), unchanged_later_configuration=same_epoch_config,
                changes=sorted(changes), revision_candidates=sorted(candidates)), changes, candidates


def survey(paths, frozen=None):
    sources, changes, candidates, inodes = {}, {}, {}, set()
    for p in sorted({Path(p).expanduser().resolve(strict=True) for p in paths}):
        st = p.stat()
        if (st.st_dev, st.st_ino) in inodes:
            continue
        inodes.add((st.st_dev, st.st_ino))
        key = locator(p)
        try:
            cutoff = None if frozen is None else frozen["sources"][key]["cutoff"]
            with closing(sqlite3.connect(p.as_uri() + "?mode=ro", uri=True)) as db:
                entry, found, proposed = census_source(db, cutoff)
            sources[key] = entry
            for call, change in found.items():
                require(changes.setdefault(call, change) == change, f"contradictory copies of {call}")
            for ref, proposal in proposed.items():
                require(candidates.setdefault(ref, proposal) == proposal, f"contradictory copies of {ref}")
        except (KeyError, ValueError, sqlite3.Error) as err:
            raise ValueError(f"{p}: {err}") from err
    require(sources, "no Studio databases supplied")
    totals = dict(sources=len(sources), categories=dict(Counter(x["category"] for x in sources.values())),
                  sources_with_changes=sum(bool(x["changes"]) for x in sources.values()),
                  change_occurrences=sum(len(x["changes"]) for x in sources.values()),
                  distinct_decisions=len(changes), choices=dict(Counter(x["choice"] for x in changes.values())),
                  numerical_changes=sum(x["numerical_change"] for x in changes.values()),
                  revision_candidates=len(candidates),
                  distinct_activated_candidates=len({x["candidate"] for x in changes.values() if x["candidate"]}))
    result = dict(format="studio-character-history-v1", totals=totals, sources=sources, transitions=changes,
                  revision_candidates=candidates)
    if frozen is not None:
        require(result == frozen, "replay differs from the frozen history index")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("databases", nargs="*", type=Path)
    parser.add_argument("--list", type=Path, help="text file with one database path per line")
    parser.add_argument("--as-of", type=Path, help="reproduce this report at its saved boundaries")
    args = parser.parse_args()
    paths = args.databases + ([Path(s) for s in args.list.read_text().splitlines() if s] if args.list else [])
    report = survey(paths, json.loads(args.as_of.read_bytes()) if args.as_of else None)
    print(json.dumps(report, indent=2, sort_keys=True))
