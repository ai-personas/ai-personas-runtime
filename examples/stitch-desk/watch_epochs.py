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

    def configured_reasoning(self):
        # Saved selections at this read pin, not model/effort reports from calls.
        # Do not copy credential references or account data into the summary.
        selections = {}
        for actor, revision, address, blob in self.db.execute(
                "select id,revision,digest,bytes from heads join objects using(digest) where kind='persona' order by id"):
            person = json.loads(blob)
            if sha256(blob).hexdigest() != address or person['id'] != actor:
                raise ValueError("Cannot bind the configured reasoning selection to its resident")
            provider = person['provider']
            selections[actor] = {"edition": f"persona:{actor}@{revision}",
                                 **{k: provider.get(k) for k in ('mode', 'model', 'effort')}}
        return selections

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
        self.database = root.resolve() / 'neural.sqlite'
        self.source = self.database.stat()
        self.uri = self.database.as_uri() + '?mode=ro'
        self.prior = self.pin()
        if self.prior.unsettled:
            raise ValueError("Arm on a settled workspace before starting the observed work.")
        cfg, = self.prior.db.execute("select bytes from heads join objects using(digest) where kind='config' and id='runtime'").fetchone()
        self.dest = dest.resolve()
        if any(self.dest.is_relative_to(p) for p in (root.resolve(), Path(json.loads(cfg)["working_directory"]).resolve())):
            raise ValueError("Keep private archives outside the workspace and its shared project.")
        selections = self.prior.configured_reasoning()
        self.dest.mkdir(mode=0o700, exist_ok=False)
        self.seen = self.prior.through
        self.batches = []
        self.armed = self.prior.archive(self.dest / "armed.sqlite")
        self.armed['configured_reasoning'] = selections
        self.check_database()
        note(self.dest / "armed.json", self.armed)

    def check_database(self):
        try:
            same = os.path.samestat(self.source, self.database.stat())
        except FileNotFoundError:
            same = False
        if not same:
            raise ValueError('The observed workspace database was moved or replaced. Keep the capture as incomplete.')

    def pin(self):
        self.check_database()
        snapshot = Snapshot(self.uri)
        try:
            self.check_database()
            return snapshot
        except BaseException:
            snapshot.db.close()
            raise

    def keep_package(self, snapshot, actor, handle):
        row = snapshot.db.execute("select bytes from objects where digest=?", [handle]).fetchone()
        if not row or sha256(row[0]).hexdigest() != handle or json.loads(row[0])["persona"] != actor:
            raise ValueError("Switch does not identify an intact package owned by its actor")
        directory = self.dest / "packages"
        directory.mkdir(mode=0o700, exist_ok=True)
        path = directory / (handle + ".json")
        try:
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError:
            if path.read_bytes() != row[0]:
                raise ValueError("Existing private package differs from the switch object")
        else:
            with os.fdopen(fd, "wb") as file:
                file.write(row[0])
                file.flush()
                os.fsync(file.fileno())
        return {"file": str(path.relative_to(self.dest)), "sha256": handle, "bytes": len(row[0])}

    def step(self, final_archive=None):
        cur = self.pin()
        try:
            if cur.through < self.seen:
                raise ValueError('The observed journal moved backwards. Keep the capture as incomplete.')
            changes = []
            for seq, actor, work, call, digest_, blob in cur.db.execute(
                """select seq,actor,work,invocation,digest,bytes from events join objects using(digest)
                   where kind='model_changed' and seq>? order by seq""", [self.seen]):
                if sha256(blob).hexdigest() != digest_:
                    raise ValueError("Invalid switch event digest")
                e = json.loads(blob)
                if e.get("previous_package") is not None:
                    changes.append({**e, "event": seq, "actor": actor, "work": work, "decision": call})
            if changes:
                name = f"switch-{changes[0]['event']}"
                before = self.prior.archive(self.dest / f"{name}-before.sqlite")
                after = cur.archive(self.dest / f"{name}-after.sqlite")
                for e in changes:
                    actor, epoch = e["actor"], e["epoch"]
                    # Even if two commits landed between samples, the event
                    # retains the exact immutable models. These files alone are
                    # not two runnable histories with their original active heads.
                    e["package_files"] = {side: self.keep_package(cur, actor, e[key])
                                          for side, key in (("before", "previous_package"), ("after", "model"))}
                    e["observed_both_heads"] = (
                        self.prior.people.get(actor) == {"epoch": epoch-1, "package": e["previous_package"]} and
                        cur.people.get(actor) == {"epoch": epoch, "package": e["model"]})
                note(self.dest / f"{name}.json", {"before": before, "after": after, "switches": changes,
                     "meaning": "Observed database boundaries. Check the actual choice, invitation, qualifications and intervening history. No voluntary or causal classification is automatic."})
                self.batches.append(name)
            self.seen = cur.through
            end = cur.archive(final_archive) if final_archive is not None else None
            if not cur.unsettled:
                self.prior.db.close()
                self.prior, cur = cur, None
            return end
        finally:
            if cur:
                cur.db.close()

    def close(self):
        try:
            # The final event scan and the final archive use the same read pin.
            # A concurrent commit after it belongs to a later observation.
            end = self.step(self.dest / "ended.sqlite")
            result = {"first_event": self.armed["through_event"], "last_event": self.seen,
                      "switch_batches": self.batches, "last_snapshot": end}
            note(self.dest / "observation.json", result)
            return result
        finally:
            self.prior.db.close()


def observing(root, dest, until_file):
    """Linux desk preflight, not a guarantee that observation will continue."""
    root, dest = root.resolve(), dest.resolve()
    if (dest / 'observation.json').exists() or (until_file and until_file.exists()):
        return False
    try:
        # The observer holds BOTH the private marker and the database open.
        # A dead/reused PID, copied marker or replaced workspace is not enough.
        with (dest / 'observer.json').open('rb') as file:
            ready = json.load(file)
            marker = os.fstat(file.fileno())
        armed = json.loads((dest / 'armed.json').read_bytes())
        if (ready['workspace'] != str(root) or ready['capture'] != str(dest)
                or ready['until_file'] != (str(until_file.resolve()) if until_file else None)
                or ready['armed_sha256'] != armed['sha256'] or armed['unsettled_calls_or_effects'] is not False
                or armed['archive'] != 'armed.sqlite' or not (dest / 'armed.sqlite').is_file()):
            return False
        pid = ready['pid']
        if type(pid) is not int or pid <= 0:
            return False
        proc = Path('/proc') / str(pid)
        if (proc / 'stat').read_bytes().rsplit(b') ', 1)[1][:1] in (b'T', b't', b'Z', b'X'):
            return False
        source = (root / 'neural.sqlite').stat()
        if ready['database_file'] != [source.st_dev, source.st_ino]:
            return False
        needed = {(s.st_dev, s.st_ino) for s in (marker, source)}
        for fd in (proc / 'fd').iterdir():
            try:
                s = fd.stat()
                needed.discard((s.st_dev, s.st_ino))
            except FileNotFoundError:  # SQLite may have finished a read.
                continue
        return not needed
    except (OSError, ValueError, KeyError, IndexError, TypeError):
        return False


def watch(workspace, private_directory, until_file):
    watcher = Recorder(workspace, private_directory)
    # Compare data_version on ONE separate read connection. Opening/closing
    # snapshot connections is not a signal that the workspace has progressed.
    signal = sqlite3.connect(watcher.uri, uri=True)
    version = None
    marker = watcher.dest / "observer.json"
    with marker.open('x') as file:
        json.dump({"pid": os.getpid(), "workspace": str(workspace.resolve()), "capture": str(watcher.dest),
                   "until_file": str(until_file.resolve()) if until_file else None,
                   "armed_sha256": watcher.armed['sha256'],
                   "database_file": [watcher.source.st_dev, watcher.source.st_ino]}, file)
        file.flush()
        try:
            print("Selected for future calls (not provider receipts):", json.dumps(watcher.armed['configured_reasoning']))
            print("Armed at event", watcher.seen, flush=True)
            while not (until_file and until_file.exists()):
                watcher.check_database()
                current, = signal.execute("pragma data_version").fetchone()
                if current != version:
                    watcher.step()
                    version = current
                time.sleep(0.12)
        except KeyboardInterrupt:
            pass
        finally:
            marker.unlink(missing_ok=True)
            signal.close()
    print(json.dumps(watcher.close(), ensure_ascii=False))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workspace", type=Path)
    parser.add_argument("private_directory", type=Path)
    parser.add_argument("--until-file", type=Path, help="End observation when this file exists (or press Ctrl+C)")
    parser.add_argument("--check-live", action="store_true", help="Linux: check this exact live observation without starting one")
    args = parser.parse_args()
    if args.check_live:
        if not observing(args.workspace, args.private_directory, args.until_file):
            parser.exit(1, "No live observer verified for this workspace and capture. Ask the facilitator before opening a desk.\n")
        parser.exit(0)
    os.umask(0o077)
    try:
        watch(args.workspace, args.private_directory, args.until_file)
    except (ValueError, OSError, sqlite3.Error) as err:
        parser.exit(1, f"Observation incomplete: {err}\n")
