"""Disconnected timing control using Ren's genuine switch and rollback, not a new commission."""
import argparse
import json
from pathlib import Path
import sqlite3
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "character-mask-trial"))
from freeze_ren import ARCHIVE, project
from watch_epochs import Recorder, digest


def check(history, private):
    assert digest(history) == ARCHIVE
    private.mkdir(mode=0o700, parents=True, exist_ok=False)
    before, after = private / "event-63.sqlite", private / "event-65.sqlite"
    project(history, before, 1)
    project(history, after, 2)
    results = {}
    for name, target, expected in (("single", after, [64]), ("activation_and_rollback", history, [64, 86])):
        case = private / name
        workspace = case / "workspace"
        workspace.mkdir(mode=0o700, parents=True)
        db = sqlite3.connect(workspace / "neural.sqlite")
        def load(path):
            source = sqlite3.connect(path.resolve().as_uri() + "?mode=ro&immutable=1", uri=True)
            source.backup(db)
            source.close()
        load(before)
        db.execute("pragma journal_mode=wal")
        observer = Recorder(workspace, case / "capture")
        # Advance a private fixture in one commit between observations.
        # Never edit a switch or replay a decision in the historical workspace.
        load(target)
        end = observer.close()
        db.close()
        batch, = end["switch_batches"]
        report = json.loads((observer.dest / (batch + ".json")).read_bytes())
        assert [s["event"] for s in report["switches"]] == expected
        assert end["last_event"] == end["last_snapshot"]["through_event"] == report["after"]["through_event"]
        extracted = []
        for switch in report["switches"]:
            packages = {}
            for side, key in (("before", "previous_package"), ("after", "model")):
                saved = switch["package_files"][side]
                path = observer.dest / saved["file"]
                assert digest(path) == saved["sha256"] == switch[key] and saved["bytes"] == path.stat().st_size
                pkg = json.loads(path.read_bytes())
                assert pkg["persona"] == switch["actor"] == "ren"
                packages[side] = {(w["tensor"], w["row"], w["column"]): w["value"] for w in pkg["parameters"]}
            a, b = packages["before"], packages["after"]
            diff = [(*k, a.get(k, 0), b.get(k, 0)) for k in a.keys() | b.keys() if a.get(k, 0) != b.get(k, 0)]
            expected_values = (0.3, 0.9) if switch["event"] == 64 else (0.9, 0.3)
            assert diff == [("direct", 2, 2, *expected_values)]
            assert switch["observed_both_heads"] == (name == "single")
            extracted.append({k: switch[k] for k in ("event", "decision", "numerical", "prior_numerical", "package_files", "observed_both_heads")})
            extracted[-1].update(kind=switch["choice"]["kind"], reason=switch["choice"]["reason"], parameter_changes=diff)
        results[name] = {"before_through": report["before"]["through_event"], "after_through": report["after"]["through_event"],
                         "exact_packages": extracted}
    return {"source_sha256": ARCHIVE, "scope": "Historical timing controls on copies. Both switches belonged to a requested reversible experiment. No new provider calls or held-out arms.", "cases": results}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("history", type=Path)
    parser.add_argument("new_private_directory", type=Path)
    args = parser.parse_args()
    print(json.dumps(check(args.history, args.new_private_directory), ensure_ascii=False, indent=2))
