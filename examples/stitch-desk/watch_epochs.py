"""Keep private SQLite snapshots across real epoch changes while Studio works.

Start before admission. This observer reads the live WAL; it never prepares a
decision, pauses Studio, changes a model, or decides if an adoption was voluntary.
Use a new private output directory outside the workspace and shared project.
"""
import argparse
from hashlib import sha256
import json
import os
from pathlib import Path
import sqlite3
import time


def digest(path):
    h = sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def note(path, data):
    with path.open("x") as file:
        json.dump(data, file, indent=2, ensure_ascii=False)
        file.write("\n")


class Snapshot:
    def __init__(self, uri):
        self.db = sqlite3.connect(uri, uri=True)
        self.db.execute("begin")
        self.through, = self.db.execute("select coalesce(max(seq),0) from events").fetchone()
        self.people = {id_: {"epoch": p["epoch"], "package": p["model"]} for id_, b in self.db.execute(
            "select id,bytes from heads join objects using(digest) where kind='persona'") for p in [json.loads(b)]}
        self.unsettled, = self.db.execute("""select
            exists(select id from invocations where state in ('prepared','dispatched','responded','probe_prepared','probe_dispatched','uncertain'))
            or exists(select id from outbox where state in ('pending','dispatched','uncertain'))""").fetchone()

    def archive(self, dst):
        dst.touch(mode=0o600, exist_ok=False)
        try:
            with sqlite3.connect(dst) as copy:
                self.db.backup(copy)
                copy.execute("pragma journal_mode=delete")
                if copy.execute("pragma integrity_check").fetchone() != ("ok",):
                    raise ValueError("Inconsistent archive")
                if copy.execute("select coalesce(max(seq),0) from events").fetchone() != (self.through,):
                    raise ValueError("Archive moved beyond the pinned snapshot")
            copy.close()
            with dst.open("rb") as file:
                os.fsync(file.fileno())
        except BaseException:
            dst.unlink(missing_ok=True)
            raise
        return {"archive": dst.name, "sha256": digest(dst), "through_event": self.through,
                "heads": self.people, "unsettled_calls_or_effects": bool(self.unsettled)}


class Recorder:
    def __init__(self, root, dest):
        self.uri = (root.resolve() / "neural.sqlite").as_uri() + "?mode=ro"
        self.prior = Snapshot(self.uri)
        if self.prior.unsettled:
            raise ValueError("Arm on a settled workspace before starting the observed work.")
        cfg, = self.prior.db.execute("select bytes from heads join objects using(digest) where kind='config' and id='runtime'").fetchone()
        self.dest = dest.resolve()
        if any(self.dest.is_relative_to(p) for p in (root.resolve(), Path(json.loads(cfg)["working_directory"]).resolve())):
            raise ValueError("Keep private archives outside the workspace and its shared project.")
        self.dest.mkdir(mode=0o700, exist_ok=False)
        self.seen = self.prior.through
        self.batches = []
        self.armed = self.prior.archive(self.dest / "armed.sqlite")
        note(self.dest / "armed.json", self.armed)

    def step(self):
        cur = Snapshot(self.uri)
        try:
            changes = []
            for seq, actor, work, call, digest_, blob in cur.db.execute(
                """select seq,actor,work,invocation,digest,bytes from events join objects using(digest)
                   where kind='model_changed' and seq>? order by seq""", [self.seen]):
                if sha256(blob).hexdigest() != digest_:
                    raise ValueError("Invalid switch event digest")
                e = json.loads(blob)
                if e.get("previous_package") is not None:
                    changes.append({"event": seq, "actor": actor, "work": work, "decision": call, **e})
            if changes:
                name = f"switch-{changes[0]['event']}"
                before = self.prior.archive(self.dest / f"{name}-before.sqlite")
                after = cur.archive(self.dest / f"{name}-after.sqlite")
                for e in changes:
                    actor, epoch = e["actor"], e["epoch"]
                    e["observed_both_heads"] = (
                        self.prior.people.get(actor) == {"epoch": epoch-1, "package": e["previous_package"]} and
                        cur.people.get(actor) == {"epoch": epoch, "package": e["model"]})
                note(self.dest / f"{name}.json", {"before": before, "after": after, "switches": changes,
                     "meaning": "Observed database boundaries. Check the actual choice, invitation, qualifications and intervening history. No voluntary or causal classification is automatic."})
                self.batches.append(name)
            self.seen = cur.through
            if not cur.unsettled:
                self.prior.db.close()
                self.prior, cur = cur, None
        finally:
            if cur:
                cur.db.close()

    def close(self):
        self.step()
        end = Snapshot(self.uri)
        try:
            result = {"first_event": self.armed["through_event"], "last_event": self.seen,
                      "switch_batches": self.batches, "last_snapshot": end.archive(self.dest / "ended.sqlite")}
            note(self.dest / "observation.json", result)
            return result
        finally:
            end.db.close()
            self.prior.db.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workspace", type=Path)
    parser.add_argument("private_directory", type=Path)
    parser.add_argument("--until-file", type=Path, help="End observation when this file exists (or press Ctrl+C)")
    args = parser.parse_args()
    os.umask(0o077)
    watcher = Recorder(args.workspace, args.private_directory)
    # Compare data_version on ONE separate read connection. Opening/closing
    # snapshot connections is not a signal that the workspace has progressed.
    signal = sqlite3.connect(watcher.uri, uri=True)
    version = None
    print("Armed at event", watcher.seen, flush=True)
    try:
        while not (args.until_file and args.until_file.exists()):
            current, = signal.execute("pragma data_version").fetchone()
            if current != version:
                watcher.step()
                version = current
            time.sleep(0.12)
    except KeyboardInterrupt:
        pass
    finally:
        signal.close()
    print(json.dumps(watcher.close(), ensure_ascii=False))
