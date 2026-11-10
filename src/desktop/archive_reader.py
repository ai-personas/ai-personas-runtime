"""Read TAR metadata or one regular member, without filesystem extraction."""
import json
import shutil
import sys
import tarfile


def main():
    source, destination = sys.argv[1:3]
    wanted = int(sys.argv[3]) if len(sys.argv) == 5 else None
    entries = []
    with tarfile.open(source, mode="r:*") as archive:
        for index, entry in enumerate(archive):
            if wanted is not None:
                if index != wanted:
                    continue
                if entry.name != sys.argv[4]:
                    raise ValueError("The package changed since it was opened. Return to the file list and select it again.")
                if not entry.isfile():
                    raise ValueError("Only regular archive members have a file preview; links are not followed.")
                with archive.extractfile(entry) as member, open(destination, "wb") as output:
                    shutil.copyfileobj(member, output)
                return
            if entry.isdir():
                continue
            note = None
            if entry.issym():
                note = f"Symbolic link to {entry.linkname} (not followed)"
            elif entry.islnk():
                note = f"Hard link to {entry.linkname} (not followed)"
            elif not entry.isfile():
                note = "Special archive entry; no file preview"
            entries.append(dict(index=index, name=entry.name, size=entry.size, note=note))
    if wanted is not None:
        raise ValueError("The selected member is no longer present in this package.")
    with open(destination, "w", encoding="utf-8") as output:
        json.dump(entries, output, ensure_ascii=True)


if __name__ == "__main__":
    main()
