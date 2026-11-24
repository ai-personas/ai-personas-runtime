"""Project the two retained history prefixes on either side of Ren's actual activation.

Specific to the identified historical SQLite archive. Never edits its source, a
model, a persona version, a choice or an event. The new files are archival
reconstructions, not contemporaneous backups or new persona decisions.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


ARCHIVE = "272fca08c9d1efcbbe754e8dd951e4072423a83c136326e6a0b9b868f61ab40b"
CALL = "call-bdddf3b36f6a2ed4626ca2846ea29572"
MODELS = (
    "da2e9708ff20bce90b1822600b29d52acaafa048b62afa4d905511fc67d16e7a",
    "b6ae80a39f63018a7d33b515802193279900f5cee5922e00c2f148119009cecc",
)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def project(source, output, epoch):
    assert sha(source) == ARCHIVE and epoch in (1, 2)
    source = source.resolve()
    src = sqlite3.connect(source.as_uri() + "?mode=ro&immutable=1", uri=True)
    output.touch(mode=0o600, exist_ok=False)
    db = sqlite3.connect(output)
    try:
        src.backup(db)
        # Row boundaries identify *whole* transactions in this exact archive.
        # 63: admission reopened, before dispatch. 64–65: activation and reply
        # in a single acceptance. 66 belongs to the subsequent decision.
        seq, last_call, version, obj = ((63, 45, 102, 379), (65, 46, 107, 391))[epoch - 1]
        assert src.execute("select rowid,state from invocations where id=?", (CALL,)).fetchone() == (46, "accepted")
        assert list(src.execute("select seq,kind from events where invocation=? order by seq", (CALL,))) == [(64, "model_changed"), (65, "reply")]
        assert src.execute("select count(*) from asset_index").fetchone() == (0,)
        assert src.execute("select max(revision) from versions where kind='control' and rowid<=?", (version,)).fetchone() == (4,)
        db.execute("pragma foreign_keys=on")
        with db:
            db.execute("delete from outbox where invocation in (select id from invocations where rowid>?)", (last_call,))
            db.execute("delete from events where seq>?", (seq,))
            db.execute("update sqlite_sequence set seq=? where name='events'", (seq,))
            db.execute("delete from versions where rowid>?", (version,))
            db.execute("delete from heads")
            db.execute("""insert into heads select kind,id,revision,digest from versions v
                        where revision=(select max(revision) from versions t where t.kind=v.kind and t.id=v.id)""")
            db.execute("delete from invocations where rowid>?", (last_call,))
            db.execute("delete from objects where rowid>?", (obj,))
        db.execute("pragma journal_mode=delete")
        db.execute("vacuum")
        assert not list(db.execute("pragma foreign_key_check"))
        assert db.execute("pragma integrity_check").fetchone() == ("ok",)
        current, = db.execute("select bytes from heads join objects using(digest) where kind='persona' and id='ren'").fetchone()
        assert (json.loads(current)["epoch"], json.loads(current)["model"]) == (epoch, MODELS[epoch - 1])
        assert list(db.execute("select * from controls")) == list(src.execute("select * from controls"))
        for table, limit, key in (("events", seq, "seq"), ("versions", version, "rowid"),
                                  ("invocations", last_call, "rowid"), ("objects", obj, "rowid")):
            query = f"select * from {table} where {key}<=? order by {key}"
            # VACUUM may renumber physical rowids; compare every saved value.
            assert list(src.execute(query, (limit,))) == list(db.execute(f"select * from {table} order by {key}"))
        for digest, data in db.execute("select digest,bytes from objects"):
            assert hashlib.sha256(data).hexdigest() == digest
        assert all(db.execute("select 1 from objects where digest=?", (m,)).fetchone() for m in MODELS)
    finally:
        db.close()
        src.close()
    return {"source_sha256": ARCHIVE, "archive_sha256": sha(output), "through_event": seq,
            "model": MODELS[epoch - 1], "epoch": epoch, "activation_call": CALL}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("epoch", type=int, choices=(1, 2))
    args = parser.parse_args()
    print(json.dumps(project(args.source, args.output, args.epoch), indent=2))
