"""Interactive terminal for the same native commands and workspace as Personas Desktop."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import uuid


LAUNCHER = Path(__file__).resolve().parents[1] / "personas"


def ask(label, default=""):
    suffix = f" [{default}]" if default else ""
    return input(f"{label}{suffix}\n› ").strip() or default


def call(root, operator, args, capture=False):
    cmd = [str(LAUNCHER), "--neural", "--state-root", str(root), "--operator", operator, *args]
    if not capture:
        return subprocess.run(cmd).returncode
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or "The command failed.")
    return json.loads(result.stdout)


def choose_workspace():
    home = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")
    library = home / "ai-personas/editable-workspaces"
    saved = {p.parent for p in library.glob("*/neural.sqlite")}
    for entry in (library.parent / "recent-workspaces").glob("*.json"):
        try:
            path = Path(json.loads(entry.read_text(encoding="utf-8")))
            if (path / "neural.sqlite").is_file():
                saved.add(path)
        except (ValueError, TypeError, OSError):
            continue
    saved = sorted(saved)
    for i, path in enumerate(saved, 1):
        print(f"  {i}  {path.name}")
    choice = ask("Choose a saved number, enter a folder, or enter new", "new")
    if choice == "new":
        return library / f"workspace-{uuid.uuid4().hex}"
    if choice.isdecimal() and 1 <= int(choice) <= len(saved):
        return saved[int(choice) - 1]
    return Path(choice).expanduser()

def remember_workspace(root):
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")
    recent = state / "ai-personas/recent-workspaces"
    recent.mkdir(mode=0o700, parents=True, exist_ok=True)
    root = root.resolve()
    key = hashlib.sha256(os.fsencode(root)).hexdigest()
    with tempfile.NamedTemporaryFile(dir=recent, delete=False) as stage:
        temporary = Path(stage.name)
        try:
            stage.write(json.dumps(str(root)).encode("utf-8"))
            stage.flush()
            os.replace(temporary, recent / (key + ".json"))
        finally:
            temporary.unlink(missing_ok=True)


def setup(root, operator):
    print(f"New workspace at {root}")
    working = Path(ask("Existing shared working directory", str(Path.cwd()))).expanduser()
    if not working.is_dir():
        raise ValueError("Choose an existing working directory.")
    profiles = []
    while True:
        name = ask("Persona display name")
        seed = ask("Initial character and temperament")
        if not name or not seed:
            raise ValueError("Each persona needs a display name and starting character.")
        profiles.append({"id": f"persona-{len(profiles) + 1}", "label": name, "seed": seed})
        if ask("Add another persona? (y/n)", "n").lower() not in ("y", "yes"):
            break
    mode = ask("Use an OpenAI key file or existing provider configuration? (key/config)", "key")
    if mode == "key":
        credential = Path(ask("Private OpenAI key file")).expanduser()
        args = ["--platform-key-file", str(credential), "--model", ask("Exact model"),
                "--effort", ask("Reasoning effort", "low")]
    elif mode == "config":
        args = ["--provider-config", str(Path(ask("Provider configuration file")).expanduser())]
    else:
        raise ValueError("Choose key or config.")
    with tempfile.TemporaryDirectory(prefix="personas-seeds-") as temporary:
        seeds = Path(temporary) / "profiles.json"
        seeds.write_text(json.dumps({"personas": profiles}), encoding="utf-8")
        call(root, operator, ["init", "--seeds", str(seeds), "--working-directory", str(working), *args], True)


def summary(state):
    for item in state["personas"]:
        p = item["body"]
        print(f"  {p['label']} · {p['id']} · {p['lifecycle']} · epoch {p['epoch']}")
    for item in state["work"]:
        w = item["body"]
        title = w["original_need"] or "Open conversation"
        print(f"  {w['id']}  {title}")
    print(f"{len(state['deliveries'])} deliveries · {state['invocations'].get('accepted', 0)} accepted decisions")


def session(root, operator):
    if not (root / "neural.sqlite").is_file():
        if (root / "progress.json").is_file():
            raise ValueError("This folder contains a historical journal. Open it with ./personas --journal-terminal.")
        setup(root, operator)
    summary(call(root, operator, ["status"], True))
    try:
        remember_workspace(root)
    except OSError as error:
        print(f"Could not save this folder in the workspace library. {error}", file=sys.stderr)
    print("Enter task, social, summary, help or a native workspace command. Enter quit to leave.")
    while True:
        try:
            line = input("studio › ").strip()
            if line in ("q", "quit", "exit"):
                return
            if not line:
                continue
            if line == "summary":
                summary(call(root, operator, ["status"], True))
                continue
            if line in ("task", "social"):
                text = ask("Desired result" if line == "task" else "Opening topic (optional)")
                invited = ask("Invite persona IDs, separated by commas")
                args = ["request", text] if line == "task" else ["social", "--topic", text]
                call(root, operator, [*args, "--invite", invited])
            else:
                args = ["--help"] if line == "help" else shlex.split(line)
                call(root, operator, args)
        except ValueError as error:
            print(error, file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description="Open a numerical Personas Studio workspace.")
    parser.add_argument("--progress", "--state-root", dest="root", type=Path)
    parser.add_argument("--operator", default="user")
    options = parser.parse_args()
    print("PERSONAS  /  STUDIO")
    root = options.root.expanduser() if options.root else choose_workspace()
    session(root, options.operator)


if __name__ == "__main__":
    try:
        main()
    except (EOFError, KeyboardInterrupt):
        print()
    except (ValueError, OSError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
