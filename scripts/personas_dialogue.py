#!/usr/bin/env python3
"""Foreground UI only. Rust owns authentication, evidence, setup, intake and runs.

No secrets are accepted here. Durable keys precede helper calls; uncertainty is
never converted into a replacement operation. Requires Python 3.9+ on Unix.
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import time
import uuid

NAMES = ("ai-personas", "personas-auth", "personas-provider", "personas-session")
DOC = "docs/interactive-launch.md"
PARTICIPANT_RECOVERY_REASONS = {
    "complete_response_rejected": "complete response rejected before actions",
    "provider_prelaunch_rejection": "call rejected before provider launch",
    "provider_terminal_failure": "provider reported failed or incomplete response-only inference",
    "response_only_transport_failure": "response-only transport ended; remote outcome remains unresolved",
    "response_only_http_failure": "temporary HTTP service failure; remote outcome remains unresolved",
    "provider_context_rejection": "provider rejected context size; pending host text is available by exact reference",
}


class Halt(Exception):
    """User chose to stop. Progress is retained."""


class Blocked(Exception):
    def __init__(self, code):
        self.code = code if re.fullmatch(r"[a-z_]{1,96}", code) else "helper_rejected"
        super().__init__(self.code)


def require(ok, code="invalid_private_progress"):
    if not ok:
        raise Blocked(code)


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def decode(raw):
    try:
        return json.loads(raw, object_pairs_hook=unique,
                          parse_constant=lambda _: (_ for _ in ()).throw(Blocked("invalid_json")))
    except (ValueError, UnicodeError, RecursionError):
        raise Blocked("invalid_json") from None


def encoded(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def identifier(value):
    return isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]+", value) is not None


def path_text(value):
    return (isinstance(value, str) and value.startswith("/")
            and "\x00" not in value)


def safe_text(value):
    return (isinstance(value, str) and bool(value)
            and bool(value.strip()))


def native_identifier(value):
    # Native references can contain punctuation, including length-prefixed
    # composite observation IDs. Match the runtime's identity grammar without
    # rewriting the exact reference or imposing the UI's filename grammar.
    return (isinstance(value, str) and bool(value)
            and not any(c.isspace() or ord(c) < 32 or 0x7f <= ord(c) <= 0x9f for c in value))


def provider_label(value):
    return native_identifier(value)


def terminal_text(value):
    # Show control characters as visible escapes; retain the original text in
    # native records and the journal instead of rejecting or replacing it.
    return "".join(c if c in "\n\t" or c.isprintable()
                   else json.dumps(c, ensure_ascii=True)[1:-1] for c in str(value))


def feedback_mandate_summary(item):
    context = item.get("mandate_context") or {}
    received = context.get("received_mandate")
    if received is None:
        return "Task revision at feedback receipt was not recorded."
    original = received["revision"]
    current = context["current_mandate_revision"]
    if original == current:
        return f"Received during the current task revision ({current})."
    return (f"Received during task revision {original}; current task revision is {current}. "
            "Review whether the direction still applies; only an explicit replacement removes it.")


def readable_messages(messages, show_invocation=False):
    """Render already-permitted messages, preserving citations and copy counts.

    Group exact copies only within an attributed decision. Keep every distinct
    subject; references identify files or history without supplying their bodies.
    """
    groups, positions = [], {}
    for message in messages:
        # Retain every supplied scope field, including any future metadata.
        # Empty citations were omitted by historical/native serialization.
        scope = {key: value for key, value in message.items() if key not in ("id", "subject")}
        scope.setdefault("file_references", [])
        scope.setdefault("history_references", [])
        attributed = all(isinstance(message.get(field), str) and message[field]
                         for field in ("sender", "recipient", "work", "invocation"))
        key = encoded(scope) if attributed else None
        index = positions.get(key) if key is not None else None
        subject = message.get("subject", "")
        if index is None:
            if key is not None:
                positions[key] = len(groups)
            groups.append({"message": message, "count": 1, "subjects": [subject]})
        else:
            group = groups[index]
            group["count"] += 1
            if subject not in group["subjects"]:
                group["subjects"].append(subject)
    parts = []
    for group in groups:
        message = group["message"]
        author = message.get('sender', 'unknown author')
        if show_invocation:
            author += f" ({message.get('invocation', 'unavailable call')})"
        parts.append(f"{author}: {message.get('text', '')}")
        if group["count"] > 1:
            parts.append(f"Recorded {group['count']} times in the same decision.")
            parts.extend(f"Subject: {subject}" for subject in group["subjects"])
        references = message.get("file_references", [])
        if references:
            parts.append("Attached exact file references (metadata, not supplied content):")
            parts.extend(json.dumps(reference, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
                         for reference in references)
        history = message.get("history_references", [])
        if history:
            parts.append("Attached exact history references (metadata, not supplied records or images):")
            parts.extend(json.dumps(reference, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
                         for reference in history)
    return parts


def readable_delivery(delivery, summary=False):
    """Render a file list or the complete qualified text of a bound delivery.

    The full view falls back to the original JSON for unrecognized packages.
    The file list explicitly links to the complete view; it supplies no bodies.
    """
    try:
        artifacts = delivery["artifacts"]
        if not isinstance(artifacts, list) or not artifacts:
            return None
        if summary:
            parts = [f"Release {delivery['release']} from {delivery['actor']} "
                     f"to {delivery['recipient']} ({delivery['invocation']})."]
        else:
            parts = [json.dumps({k: v for k, v in delivery.items() if k != "artifacts"},
                                ensure_ascii=False, indent=2)]
        for artifact in artifacts:
            serialized = artifact["qualified_bundle_json"]
            if hashlib.sha256(serialized.encode("utf-8")).hexdigest() != artifact["qualified_bundle_sha256"]:
                return None
            bundle = decode(serialized)
            if bundle.get("profile") != "qualified-exact-context-json-v1":
                return None
            entries = bundle["entries"]
            raw = artifact["bytes"]
            if (not isinstance(entries, list) or not entries or not isinstance(raw, list)
                    or any(type(byte) is not int or not 0 <= byte <= 255 for byte in raw)):
                return None
            source = artifact["source"]
            roots = [entry for entry in entries if all(
                entry["reference"].get(key) == source.get(key)
                for key in ("source", "revision", "representation", "start", "end"))]
            if len(roots) != 1 or not isinstance(roots[0]["text"], str):
                return None
            if any(not isinstance(entry.get("text"), str) for entry in entries):
                return None
            if source.get("representation") == "bytes_metadata":
                metadata = decode(roots[0]["text"])
                if (metadata.get("profile") != "retained-file-bytes-metadata-v1"
                        or metadata.get("body_supplied") is not False
                        or metadata.get("bytes") != len(raw)
                        or metadata.get("sha256") != hashlib.sha256(bytes(raw)).hexdigest()):
                    return None
                if not summary:
                    parts.append("Artifact bytes are retained exactly. Choose e to export the file and its qualifications.")
            elif roots[0]["text"].encode("utf-8") != bytes(raw):
                return None
            if summary:
                parts.append(f"File: {roots[0].get('path', 'unnamed')} ({len(raw):,} bytes).")
                if roots[0].get("retired"):
                    parts.append("This recorded file is retired.")
                for entry in entries:
                    if entry is not roots[0]:
                        parts.append(f"Accompanying file: {entry.get('path', 'unnamed')}.")
                if bundle.get("withheld"):
                    parts.append("Some associated content is unavailable; inspect the full qualifications with v.")
                continue
            parts.append("Artifact record:")
            parts.append(json.dumps({k: v for k, v in artifact.items()
                                     if k not in ("bytes", "qualified_bundle_json")},
                                    ensure_ascii=False, indent=2))
            parts.append("Package qualifications and source attribution:")
            parts.append(json.dumps({k: v for k, v in bundle.items() if k != "entries"},
                                    ensure_ascii=False, indent=2))
            for number, entry in enumerate(entries, 1):
                if not isinstance(entry["text"], str):
                    return None
                parts.append(f"Document {number}/{len(entries)}: {entry.get('path', 'unnamed')}")
                parts.append(json.dumps({k: v for k, v in entry.items() if k != "text"},
                                        ensure_ascii=False, indent=2))
                parts.extend(("Document text:", entry["text"], "End of document text."))
        if summary:
            parts.append("This is a file list. Choose v for full contents and qualifications. "
                         "The work menu also offers e to export files and j for exact receipts.")
        return parts
    except (Blocked, KeyError, TypeError, ValueError, AttributeError, UnicodeError):
        return None


class PrivateDirectory:
    """Descriptor-relative local state with private creation defaults.

    Existing ownership, permission bits and repository placement add no access
    gates. Resolve directory aliases before traversal; missing or invalid saved
    progress is never silently reset.
    """
    def __init__(self, path, create=False):
        self.path = Path(path).resolve(strict=False)
        self.fd = self.open_path(self.path, create=create)

    @staticmethod
    def open_path(path, create=False, missing_ok=False):
        """Check all existing components; missing_ok is a read-only preflight."""
        path = Path(path).resolve(strict=False)
        require(path_text(str(path)), "invalid_state_path")
        require(not (create and missing_ok), "invalid_private_path_check")
        fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        try:
            for part in Path(path).parts[1:]:
                try:
                    child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                except FileNotFoundError:
                    if missing_ok:
                        os.close(fd)
                        return None
                    require(create, "private_directory_missing")
                    os.mkdir(part, 0o700, dir_fd=fd)
                    os.fsync(fd)
                    child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                os.close(fd)
                fd = child
            return fd
        except BaseException:
            os.close(fd)
            raise

    @staticmethod
    def file_ok(info, maximum=None):
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, "invalid_journal_file")

    def read(self, name, maximum=None):
        require(re.fullmatch(r"[a-zA-Z0-9_.-]+", name) is not None)
        try:
            fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=self.fd)
        except FileNotFoundError:
            return None
        with os.fdopen(fd, "rb") as source:
            self.file_ok(os.fstat(source.fileno()), maximum)
            raw = source.read()
        return raw

    def write(self, name, raw):
        self.read(name)  # Validate existing journal identity before atomic replacement.
        temporary = ".write-" + uuid.uuid4().hex
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=self.fd)
        try:
            with os.fdopen(fd, "wb") as out:
                out.write(raw)
                out.flush()
                os.fsync(out.fileno())
            os.replace(temporary, name, src_dir_fd=self.fd, dst_dir_fd=self.fd)
            os.fsync(self.fd)
        finally:
            try:
                os.unlink(temporary, dir_fd=self.fd)
            except FileNotFoundError:
                pass

    def close(self):
        os.close(self.fd)


def valid_codex_paths(value):
    return (isinstance(value, dict) and set(value) == {"executable", "host_home", "codex_home"}
            and all(path_text(path) for path in value.values()))


def valid_launch_reference(value):
    return (isinstance(value, dict) and set(value) == {"id", "revision", "sha256"}
            and value["id"] == "runtime-launch-v1" and type(value["revision"]) is int
            and value["revision"] > 0 and isinstance(value["sha256"], str)
            and re.fullmatch(r"[a-f0-9]{64}", value["sha256"]) is not None)


def valid_session_route(route):
    if not isinstance(route, dict):
        return False
    if route.get("kind") == "native_siwc":
        return set(route) == {"kind", "vault"} and path_text(route["vault"])
    return (set(route) == {"kind", "executable", "host_home", "codex_home", "authorize_codex_auth_only"}
            and route["kind"] == "codex_auth_only" and type(route["authorize_codex_auth_only"]) is bool
            and valid_codex_paths({k: route[k] for k in ("executable", "host_home", "codex_home")}))


def new_progress():
    return {"version": 1, "operator": None, "installation": None, "runtime": None,
            "vault": None, "needs": [], "session": None, "session_history": [], "probes": [],
            "settings": None, "bootstrap": None, "setup_authorized": False,
            "work_input": None, "codex_auth": None}


def validate_progress(p):
    fields = set(new_progress())
    require(isinstance(p, dict) and set(p) | {"session_history"} == fields and p["version"] == 1)
    require(p["codex_auth"] is None or valid_codex_paths(p["codex_auth"]))
    require(type(p["setup_authorized"]) is bool)
    require(not p["setup_authorized"] or p["bootstrap"] is not None)
    require(p["operator"] is None or identifier(p["operator"]))
    require(p["vault"] is None or path_text(p["vault"]))
    if p["runtime"] is not None:
        require(isinstance(p["runtime"], dict) and set(p["runtime"]) == {"path", "owned"}
                and path_text(p["runtime"]["path"]) and type(p["runtime"]["owned"]) is bool)
    if p["installation"] is not None:
        require(isinstance(p["installation"], dict) and set(p["installation"]) == set(NAMES))
        for item in p["installation"].values():
            require(isinstance(item, dict) and set(item) == {"path", "sha256"} and path_text(item["path"])
                    and re.fullmatch(r"[a-f0-9]{64}", item["sha256"]) is not None)
    require(isinstance(p["needs"], list))
    seen = set()
    for n in p["needs"]:
        if isinstance(n, dict) and n.get("kind") == "optional_activity":
            require(set(n) == {"id", "kind", "work"}
                    and identifier(n["id"]) and n["id"] not in seen
                    and (n["work"] is None or identifier(n["work"])))
            seen.add(n["id"])
            continue
        require(isinstance(n, dict) and set(n) == {"id", "text", "require_acceptance", "shared", "work"})
        require(identifier(n["id"]) and n["id"] not in seen and safe_text(n["text"])
                and type(n["require_acceptance"]) is bool and type(n["shared"]) is bool
                and (n["work"] is None or identifier(n["work"])))
        seen.add(n["id"])
    history = p.get("session_history", [])
    require(isinstance(history, list))
    session_ids = set()
    for s in history + ([] if p["session"] is None else [p["session"]]):
        require(isinstance(s, dict)
                and set(s) == {"id", "work", "route", "expected_launch"}
                and identifier(s["id"]) and identifier(s["work"]))
        require(s["id"] not in session_ids)
        session_ids.add(s["id"])
        require(valid_session_route(s["route"]))
        require(valid_launch_reference(s["expected_launch"]))
    require(isinstance(p["probes"], list))
    seen = set()
    for probe in p["probes"]:
        require(isinstance(probe, dict) and set(probe) == {"request", "view", "attempted"})
        q = probe["request"]
        require(isinstance(q, dict) and set(q) == {"id", "catalogue_sha256", "model", "effort"})
        require(identifier(q["id"]) and q["id"] not in seen and provider_label(q["model"])
                and provider_label(q["effort"])
                and type(probe["attempted"]) is bool
                and (q["catalogue_sha256"] is None
                     or (isinstance(q["catalogue_sha256"], str)
                         and re.fullmatch(r"[a-f0-9]{64}", q["catalogue_sha256"]) is not None)))
        seen.add(q["id"])
        v = probe["view"]
        if v is not None:
            require(isinstance(v, dict) and isinstance(v.get("plan"), dict)
                    and v["plan"].get("request") == q
                    and re.fullmatch(r"[a-f0-9]{64}", v.get("plan_sha256", "")) is not None)
    for key in ("settings", "bootstrap"):
        ref = p[key]
        if ref is not None:
            require(isinstance(ref, dict) and set(ref) == {"file", "sha256"}
                    and re.fullmatch(r"[a-z-]+[a-z0-9-]*\.json", ref["file"]) is not None
                    and re.fullmatch(r"[a-f0-9]{64}", ref["sha256"]) is not None)
    item = p["work_input"]
    if item is not None:
        require(isinstance(item, dict) and set(item) == {"kind", "request", "delivered"}
                and item["kind"] in ("feedback", "answer-question", "resume-participant", "amend-work")
                and type(item["delivered"]) is bool)
        request = item["request"]
        require(isinstance(request, dict) and identifier(request.get("work")))
        if item["kind"] == "feedback":
            require({"id", "work", "persona", "text"} <= set(request)
                    <= {"id", "work", "persona", "text", "supersedes"}
                    and identifier(request["id"]) and identifier(request["persona"])
                    and safe_text(request["text"]))
            replaced = request.get("supersedes", [])
            require(isinstance(replaced, list) and all(native_identifier(value) for value in replaced)
                    and len(set(replaced)) == len(replaced))
        elif item["kind"] == "answer-question":
            require(set(request) == {"work", "reply"} and isinstance(request["reply"], dict))
            reply = request["reply"]
            require(set(reply) == {"id", "question", "question_revision", "text", "basis"}
                    and identifier(reply["id"]) and safe_text(reply["question"])
                    and re.fullmatch(r"[a-f0-9]{64}", reply["question_revision"]) is not None
                    and safe_text(reply["text"]) and safe_text(reply["basis"]))
        elif item["kind"] == "amend-work":
            require(set(request) == {"id", "operator", "work", "scope", "invitations", "resume",
                                     "expected_runtime", "expected_launch", "authorize_amend"}
                    and identifier(request["id"]) and request["operator"] == p["operator"]
                    and type(request["authorize_amend"]) is bool
                    and isinstance(request["scope"], dict)
                    and safe_text(request["scope"].get("request"))
                    and isinstance(request["invitations"], list)
                    and isinstance(request["resume"], list)
                    and all(identifier(actor) for actor in request["resume"]))
            for key, name in (("expected_runtime", "runtime-state-v1"),
                              ("expected_launch", "runtime-launch-v1")):
                ref = request[key]
                require(isinstance(ref, dict) and set(ref) == {"id", "revision", "sha256"}
                        and ref["id"] == name and type(ref["revision"]) is int and ref["revision"] > 0
                        and re.fullmatch(r"[a-f0-9]{64}", ref["sha256"]) is not None)
        else:
            require(set(request) == {"id", "operator", "work", "persona", "expected_runtime",
                                     "expected_launch", "authorize_resume"}
                    and identifier(request["id"]) and identifier(request["persona"])
                    and request["operator"] == p["operator"] and type(request["authorize_resume"]) is bool)
            for key, name in (("expected_runtime", "runtime-state-v1"),
                              ("expected_launch", "runtime-launch-v1")):
                ref = request[key]
                require(isinstance(ref, dict) and set(ref) == {"id", "revision", "sha256"}
                        and ref["id"] == name and type(ref["revision"]) is int and ref["revision"] > 0
                        and re.fullmatch(r"[a-f0-9]{64}", ref["sha256"]) is not None)


class Journal:
    def __init__(self, directory, create=False):
        self.directory = directory
        flags = os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC
        try:
            self.lock = os.open("interaction.lock", flags | (os.O_CREAT if create else 0),
                                0o600, dir_fd=directory.fd)
        except FileNotFoundError:
            raise Blocked("progress_missing_do_not_reset") from None
        try:
            directory.file_ok(os.fstat(self.lock), 0)
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            raw = directory.read("progress.json")
            if create:
                require(raw is None, "progress_already_exists")
                require(set(os.listdir(directory.fd)) <= {"interaction.lock"}, "progress_missing_do_not_reset")
                self.data = new_progress()
                self.save()
            else:
                require(raw is not None, "progress_missing_do_not_reset")
                self.data = decode(raw)
                validate_progress(self.data)
        except BaseException:
            os.close(self.lock)
            raise

    def save(self):
        validate_progress(self.data)
        self.directory.write("progress.json", encoded(self.data))

    def keep(self, kind, value, identity):
        name = kind + "-" + identity + ".json"
        raw = encoded(value)
        previous = self.directory.read(name)
        require(previous is None or previous == raw, "immutable_configuration_changed")
        self.directory.write(name, raw)
        ref = {"file": name, "sha256": hashlib.sha256(raw).hexdigest()}
        self.data[kind] = ref
        self.save()
        return str(self.directory.path / name)

    def document(self, kind):
        ref = self.data[kind]
        require(ref is not None)
        raw = self.directory.read(ref["file"], 262144 if kind == "bootstrap" else 65536)
        require(raw is not None and hashlib.sha256(raw).hexdigest() == ref["sha256"], "saved_configuration_changed")
        return str(self.directory.path / ref["file"]), decode(raw)

    def close(self):
        os.close(self.lock)


class Terminal:
    def say(self, text):
        print(terminal_text(text), flush=True)

    def ask(self, text):
        print(terminal_text(text), end=" ", flush=True)
        line = sys.stdin.buffer.readline()
        if not line:
            raise Halt()
        require(line.endswith(b"\n"), "incomplete_input")
        return line[:-1].decode("utf-8")

    def number(self, text, default, low, high):
        while True:
            raw = self.ask(f"{text} [{default}]").strip() or str(default)
            if raw.isdecimal() and low <= int(raw) and (high is None or int(raw) <= high):
                return int(raw)
            self.say(f"Enter an integer of at least {low}." if high is None else f"Enter {low}..{high}.")

    def bounded_text(self, text, maximum, default=None):
        # The old maximum remains a call-site hint, not an input quota.
        while True:
            raw = self.ask(text + (f" [{default}]" if default is not None else "")).strip()
            value = raw or default
            if value:
                return value
            self.say("Enter nonempty text.")


def _open_regular(path):
    """Resolve the selected alias and anchor reads to its actual regular file."""
    require(all(type(getattr(os, name, None)) is int for name in
                ("O_DIRECTORY", "O_CLOEXEC", "O_NOFOLLOW", "O_NONBLOCK")),
            "verified_exec_unavailable")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW
    directory = None
    leaf = None
    try:
        p = Path(path).resolve(strict=True)
        directory = os.open("/", flags)
        for part in p.parts[1:-1]:
            child = None
            try:
                child = os.open(part, flags, dir_fd=directory)
                info = os.fstat(child)
                if not stat.S_ISDIR(info.st_mode):
                    raise Blocked("unsafe_installation_path")
                os.close(directory)
                directory = child
                child = None  # ownership transferred to directory
            finally:
                if child is not None:
                    os.close(child)
        leaf = os.open(p.name, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC | os.O_NOFOLLOW,
                       dir_fd=directory)
        info = os.fstat(leaf)
        require(stat.S_ISREG(info.st_mode), "invalid_installation")
        result, leaf = (leaf, None)
        return result, info
    except (OSError, RuntimeError):
        raise Blocked("unsafe_installation_path") from None
    finally:
        if leaf is not None:
            os.close(leaf)
        if directory is not None:
            os.close(directory)

def _digest_fd(fd):
    h = hashlib.sha256()
    os.lseek(fd, 0, os.SEEK_SET)
    while True:
        part = os.read(fd, 1024 * 1024)
        if not part:
            break
        h.update(part)
    os.lseek(fd, 0, os.SEEK_SET)
    return h.hexdigest()


def _stable_stat(before, after):
    return (before.st_dev, before.st_ino, before.st_mode, before.st_size,
            before.st_mtime_ns, before.st_ctime_ns) == (
            after.st_dev, after.st_ino, after.st_mode, after.st_size,
            after.st_mtime_ns, after.st_ctime_ns)


def binary_digest(path):
    fd, before = _open_regular(path)
    try:
        digest = _digest_fd(fd)
        require(_stable_stat(before, os.fstat(fd)), "installation_changed_during_hash")
        return digest
    except OSError:
        raise Blocked("installation_unavailable") from None
    finally:
        os.close(fd)


class VerifiedExecutable:
    """Linux-only immutable snapshot of a pinned ELF helper.

    Pins the main ELF bytes only. The kernel loader and shared libraries remain
    trusted host dependencies. Same-UID hostile processes and root are outside
    this snapshot's threat boundary.
    """
    @staticmethod
    def capabilities_available():
        os_names = ("MFD_CLOEXEC", "MFD_ALLOW_SEALING", "O_CLOEXEC", "O_NOFOLLOW", "O_NONBLOCK", "O_DIRECTORY")
        fcntl_names = ("F_ADD_SEALS", "F_GET_SEALS", "F_SEAL_SEAL", "F_SEAL_SHRINK", "F_SEAL_GROW", "F_SEAL_WRITE")
        return (sys.platform.startswith("linux")
                and callable(getattr(os, "memfd_create", None))
                and callable(getattr(os, "fchmod", None))
                and callable(getattr(fcntl, "fcntl", None))
                and all(type(getattr(os, name, None)) is int for name in os_names)
                and all(type(getattr(fcntl, name, None)) is int for name in fcntl_names)
                and Path("/proc/self/fd").is_dir())

    @staticmethod
    def supported():
        return VerifiedExecutable.capabilities_available()

    def __init__(self, path, expected_digest):
        require(sys.platform.startswith("linux"), "verified_exec_unsupported")
        require(isinstance(expected_digest, str) and re.fullmatch(r"[a-f0-9]{64}", expected_digest),
                "approved_binary_digest_required")
        require(self.capabilities_available(), "verified_exec_unavailable")
        source_fd, before = _open_regular(path)
        self.fd = None
        try:
            # Only ELF executables are supported: no shebang/interpreter chain.
            magic = os.read(source_fd, 4)
            require(magic == b"\x7fELF", "verified_exec_format_unsupported")
            os.lseek(source_fd, 0, os.SEEK_SET)
            self.fd = os.memfd_create("personas-helper", os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
            digest = hashlib.sha256()
            total = 0
            while True:
                chunk = os.read(source_fd, 1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                digest.update(chunk)
                view = memoryview(chunk)
                while view:
                    written = os.write(self.fd, view)
                    require(written > 0, "verified_exec_unavailable")
                    view = view[written:]
            require(_stable_stat(before, os.fstat(source_fd)), "installation_changed_during_snapshot")
            actual = digest.hexdigest()
            require(actual == expected_digest, "approved_binary_changed")
            require(total == before.st_size, "installation_changed_during_snapshot")
            os.fchmod(self.fd, 0o500)
            os.lseek(self.fd, 0, os.SEEK_SET)
            seals = fcntl.F_SEAL_SEAL | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_GROW | fcntl.F_SEAL_WRITE
            fcntl.fcntl(self.fd, fcntl.F_ADD_SEALS, seals)
            require(fcntl.fcntl(self.fd, getattr(fcntl, "F_GET_SEALS", -1)) & seals == seals,
                    "verified_exec_unavailable")
            self.path = f"/proc/self/fd/{self.fd}"
            self.digest = actual
        except Blocked:
            if self.fd is not None:
                os.close(self.fd)
                self.fd = None
            raise
        except (OSError, AttributeError, TypeError, ValueError):
            if self.fd is not None:
                os.close(self.fd)
                self.fd = None
            raise Blocked("verified_exec_unavailable") from None
        except BaseException:
            if self.fd is not None:
                os.close(self.fd)
                self.fd = None
            raise
        finally:
            os.close(source_fd)

    def close(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None


def _toolchain_order(name):
    """Order recognized rustup directory names; names never prove compiler version."""
    if re.fullmatch(r"stable(?:-[A-Za-z0-9_.-]+)?", name):
        return (0, 0, 0, name)
    numeric = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:-[A-Za-z0-9_.-]+)?", name)
    if numeric:
        major, minor, patch = (int(part) for part in numeric.groups())
        return (1, -major, -minor, -patch, name)
    if re.fullmatch(r"beta(?:-[A-Za-z0-9_.-]+)?", name):
        return (2, 0, 0, name)
    if re.fullmatch(r"nightly(?:-[A-Za-z0-9_.-]+)?", name):
        return (3, 0, 0, name)
    return None


def discover_cargo():
    """Discover every candidate from PATH and the selected Rustup root."""
    candidates = []
    cargo = shutil.which("cargo")
    if cargo:
        candidates.append((cargo, None))

    rustup_home = os.environ.get("RUSTUP_HOME")
    if rustup_home is not None:
        roots = [Path(rustup_home) / "toolchains"] if path_text(rustup_home) else []
    else:
        home = os.environ.get("HOME")
        roots = [Path(home) / ".rustup" / "toolchains"] if home and path_text(home) else []

    discovered = []
    for root in roots:
        try:
            with os.scandir(root) as entries:
                names = []
                for entry in entries:
                    names.append(entry.name)
        except (FileNotFoundError, NotADirectoryError):
            continue
        except PermissionError:
            continue
        for name in sorted(names):
            order = _toolchain_order(name)
            if order is None:
                continue
            directory = root / name
            if not directory.is_dir():
                continue
            bindir = directory / "bin"
            cargo_path, rustc = bindir / "cargo", bindir / "rustc"
            if (cargo_path.is_file() and os.access(cargo_path, os.X_OK)
                    and rustc.is_file() and os.access(rustc, os.X_OK)):
                discovered.append((order, str(cargo_path), str(bindir)))

    for _, candidate, bindir in sorted(discovered):
        if all(existing[0] != candidate for existing in candidates):
            candidates.append((candidate, bindir))
    return candidates


def _parse_rustc_version(output):
    if not isinstance(output, str):
        return False
    match = re.match(r"^rustc (\d+)\.(\d+)\.(\d+)(?:-([^\s]+))?(?:\s|$)", output)
    if not match:
        return False
    version = tuple(int(part) for part in match.groups()[:3])
    return version > (1, 88, 0) or (version == (1, 88, 0) and match.group(4) is None)


def _stop_probe(proc):
    # Always target the isolated group: the parent may have exited while a
    # descendant still owns stdout and keeps the pipe open.
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except OSError:
        try:
            if proc.poll() is None:
                proc.kill()
        except OSError:
            pass
    try:
        proc.wait()
    except (OSError, subprocess.SubprocessError):
        pass


def _supported_rustc(rustc, env, cwd):
    """Read one compiler version without application duration or byte quotas."""
    try:
        proc = subprocess.Popen([rustc, "--version"], stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                env=env, cwd=cwd, start_new_session=True)
    except (OSError, subprocess.SubprocessError, ValueError):
        return False
    output = bytearray()
    accepted = False
    try:
        fd = proc.stdout.fileno()
        os.set_blocking(fd, False)
        with selectors.DefaultSelector() as poll:
            poll.register(proc.stdout, selectors.EVENT_READ)
            while poll.get_map():
                events = poll.select(0.1)
                if not events:
                    continue
                try:
                    part = os.read(fd, 8192)
                except BlockingIOError:
                    continue
                if not part:
                    poll.unregister(proc.stdout)
                    continue
                output.extend(part)
        code = proc.wait()
        if code != 0:
            return False
        try:
            text = output.decode("utf-8")
        except UnicodeDecodeError:
            return False
        accepted = _parse_rustc_version(text)
        return accepted
    except (OSError, subprocess.SubprocessError, ValueError):
        return False
    finally:
        # Version probes must not leave descendants behind, even if the parent
        # printed a valid version and exited successfully.
        _stop_probe(proc)
        if proc.stdout is not None:
            proc.stdout.close()


def select_supported_cargo(candidates, base_env, cwd):
    """Select the first supported compiler using the actual build environment."""
    for cargo, toolchain_bin in candidates:
        env = dict(base_env)
        if toolchain_bin:
            env["PATH"] = toolchain_bin + os.pathsep + env.get("PATH", "")
            rustc = str(Path(toolchain_bin) / "rustc")
        else:
            rustc = shutil.which("rustc", path=env.get("PATH"))
            if not rustc:
                continue
        if _supported_rustc(rustc, env, cwd):
            return cargo, toolchain_bin, env
    return None


class Helpers:
    def __init__(self, journal, checkout, ui):
        self.j = journal
        self.checkout = checkout
        self.ui = ui
        self.executables = {}
        self.pinned_installation = None

    def close(self):
        for executable in self.executables.values():
            executable.close()
        self.executables = {}
        self.pinned_installation = None

    def pin_installation(self, mapping):
        # Retain one coherent installation for this UI lifetime. A later build
        # must not replace inspection helpers while an earlier session runs.
        snapshots = {}
        try:
            for name in NAMES:
                item = mapping[name]
                if sys.platform.startswith("linux"):
                    snapshots[name] = VerifiedExecutable(item["path"], item["sha256"])
                else:
                    require(binary_digest(item["path"]) == item["sha256"], "approved_binary_changed")
        except BaseException:
            for executable in snapshots.values():
                executable.close()
            raise
        self.close()
        self.executables = snapshots
        self.pinned_installation = {name: dict(mapping[name]) for name in NAMES}

    def candidates(self):
        paths = []
        explicit = os.environ.get("PERSONAS_BIN")
        if explicit is not None:
            require(bool(explicit) and "\0" not in explicit, "invalid_explicit_installation")
            paths.append(Path(explicit).parent.resolve(strict=True))
        installed = shutil.which("ai-personas")
        if installed:
            paths.append(Path(installed).parent)
        paths += [self.checkout / "target/release", self.checkout / "target/debug"]
        found = []
        for folder in paths:
            if folder not in found and all((folder / n).is_file() for n in NAMES):
                found.append(folder)
        return found

    def adopt(self, folder):
        mapping = {}
        selected_folder = Path(folder).resolve(strict=True)
        for name in NAMES:
            # Keep the selected name; reads resolve its target before binding
            # complete bytes to the saved digest and execution snapshot.
            path = selected_folder / name
            mapping[name] = {"path": str(path), "sha256": binary_digest(path)}
        self.pin_installation(mapping)
        self.j.data["installation"] = mapping
        self.j.save()

    def install(self):
        explicit = os.environ.get("PERSONAS_BIN")
        if explicit is not None:
            require(bool(explicit) and "\0" not in explicit, "invalid_explicit_installation")
            selected = Path(explicit).parent.resolve(strict=True)
            # The explicit choice selects current bytes even if this directory
            # also held a previously selected build. Other progress is retained.
            self.ui.say(f"PERSONAS_BIN selects native helpers in {selected}.")
            self.adopt(selected)
            return
        if self.j.data["installation"] is not None:
            try:
                self.pin_installation(self.j.data["installation"])
                return
            except (Blocked, OSError):
                self.ui.say("A selected helper is missing or changed. Choose the installation to use.")
                self.j.data["installation"] = None
                self.j.save()
        candidates = self.candidates()
        for i, folder in enumerate(candidates, 1):
            self.ui.say(f"{i}: existing helpers in {folder}")
        self.ui.say("b: build this checkout with Cargo; p: select an existing helper directory; q: leave")
        prompt = "Installation choice [1]:" if candidates else "Installation choice:"
        choice = self.ui.ask(prompt).strip().lower() or ("1" if candidates else "")
        if choice.isdecimal() and 1 <= int(choice) <= len(candidates):
            self.adopt(candidates[int(choice) - 1])
        elif choice == "p":
            folder = self.ui.ask("Directory containing all four current helpers:").strip()
            require(bool(folder) and "\0" not in folder, "invalid_installation")
            self.adopt(Path(folder))
        elif choice == "b":
            candidates = discover_cargo()
            if not candidates:
                self.ui.say("No Cargo candidate was found on PATH or in the configured Rustup toolchain root. "
                            "Install or repair Rust 1.88+ and the native C toolchain/linker under host policy, "
                            "or choose an existing helper directory. No installer was run.")
                return self.install()
            target = self.j.directory.path / "build"
            env = self.environment()
            for k in ("CARGO_HOME", "RUSTUP_HOME"):
                if k in os.environ:
                    env[k] = os.environ[k]
            env.update(CARGO_PROFILE_RELEASE_DEBUG="0", CARGO_INCREMENTAL="0",
                       CARGO_TARGET_DIR=str(target))
            selected = select_supported_cargo(candidates, env, self.checkout)
            if selected is None:
                self.ui.say("No discovered candidate reported Rust 1.88+. "
                            "Repair the configured toolchain or choose an approved existing helper directory. "
                            "No installer or provider operation was run.")
                return self.install()
            cargo, toolchain_bin, env = selected
            self.ui.say("Building local helpers; Cargo progress follows. Interrupting never starts provider work.")
            # Child-only (Python 3.9+): keep new build paths private even when
            # the caller uses umask 0002. Never chmod an existing installation.
            proc = subprocess.Popen([cargo, "build", "--locked", "--release", "--bins"],
                                    cwd=self.checkout, env=env, start_new_session=True, umask=0o077)
            try:
                code = proc.wait()
            except BaseException:
                self.stop(proc)
                raise
            require(code == 0, "build_failed")
            self.adopt(target / "release")
        else:
            raise Halt()

    @staticmethod
    def environment():
        # Default host commands need the actual launch environment, including
        # package-manager, proxy and tool settings. Model authentication still
        # uses the explicitly selected vault or auth-only route.
        return dict(os.environ)

    @staticmethod
    def stop(proc):
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()

    def call(self, name, args, timeout=None, auth_display=False, on_wait=None, on_input=None):
        require(self.pinned_installation == self.j.data["installation"], "installation_not_pinned")
        item = self.j.data["installation"][name]
        executable = item["path"]
        pass_fds = ()
        if sys.platform.startswith("linux"):
            verified = self.executables[name]
            executable = verified.path
            pass_fds = (verified.fd,)
        else:
            require(binary_digest(item["path"]) == item["sha256"], "approved_binary_changed")
        proc = subprocess.Popen([executable, *args], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=None if auth_display else subprocess.PIPE,
                                cwd=self.checkout, env=self.environment(), start_new_session=True,
                                pass_fds=pass_fds)
        out, err = bytearray(), bytearray()
        next_update = time.monotonic() + 15
        try:
            with selectors.DefaultSelector() as poll:
                poll.register(proc.stdout, selectors.EVENT_READ, out)
                streams = 1
                if proc.stderr:
                    poll.register(proc.stderr, selectors.EVENT_READ, err)
                    streams += 1
                if on_input is not None:
                    poll.register(sys.stdin, selectors.EVENT_READ, None)
                while streams:
                    for key, _ in poll.select(0.2):
                        if key.data is None:
                            if proc.poll() is not None or on_input() is False:
                                poll.unregister(key.fileobj)
                            continue
                        part = os.read(key.fd, 8192)
                        if not part:
                            poll.unregister(key.fileobj)
                            streams -= 1
                        else:
                            key.data.extend(part)
                    if on_wait is not None and proc.poll() is None and time.monotonic() >= next_update:
                        on_wait()
                        next_update = time.monotonic() + 15
                code = proc.wait()
            if code != 0:
                try:
                    # Decode the structured error without echoing provider output.
                    error = decode(err.rstrip().split(b"\n")[-1] if name == "personas-session" else err)
                    require(isinstance(error, dict), "invalid_helper_error")
                    if name == "personas-session":
                        require(error.get("version") == 1 and error.get("ok") is False,
                                "invalid_session_error")
                    label = error.get("code", "helper_rejected")
                    require(isinstance(label, str), "invalid_helper_error")
                except Blocked:
                    label = "helper_outcome_unknown"
                raise Blocked(label)
            result = decode(out)
            require(isinstance(result, dict), "invalid_helper_response")
            if name == "personas-session":
                require(result.get("ok") is True, "session_rejected")
                return result["result"]
            if name == "personas-provider":
                require(result.get("status") == "ok", "provider_rejected")
                return result["data"]
            return result
        except BaseException:
            self.stop(proc)
            raise
        finally:
            proc.stdout.close()
            if proc.stderr:
                proc.stderr.close()


class Dialogue:
    def __init__(self, journal, helpers, ui, checkout):
        self.j, self.h, self.ui, self.checkout = journal, helpers, ui, checkout
        self.p = journal.data

    def session(self, *args, timeout=None, on_wait=None, on_input=None):
        return self.h.call("personas-session", ["--state-root", self.p["runtime"]["path"],
                            "--operator", self.p["operator"], *args], timeout=timeout,
                           on_wait=on_wait, on_input=on_input)

    def provider(self, *args, timeout=None):
        return self.h.call("personas-provider", ["--vault", self.p["vault"], *args], timeout=timeout)

    def capture_need(self):
        text = self.ui.ask("What would you like Personas to work on? Do not paste credentials. (Enter alone to inspect existing work)")
        if not text.strip():
            return
        require(safe_text(text), "invalid_need")
        # Preserve the exact text BEFORE any other prompt, build or helper call.
        n = {"id": "need-" + uuid.uuid4().hex, "text": text, "require_acceptance": False,
             "shared": False, "work": None}
        self.p["needs"].append(n)
        self.j.save()
        self.ui.say("Your request is saved privately. It has not been shared or submitted.")

    def capture_optional_activity(self):
        activity = {"id": "activity-" + uuid.uuid4().hex, "kind": "optional_activity", "work": None}
        self.p["needs"].append(activity)
        self.j.save()
        return activity

    def choose_runtime(self):
        if self.p["runtime"] is not None:
            return
        self.ui.say("n: create a new private runtime here; e: use an existing saved runtime")
        choice = self.ui.ask("Runtime choice [n]:").strip().lower() or "n"
        if choice == "n":
            self.p["runtime"] = {"path": str(self.j.directory.path / "runtime"), "owned": True}
        elif choice == "e":
            default = Path(os.environ.get("XDG_STATE_HOME", str(Path(os.environ["HOME"]) / ".local/state"))) / "ai-personas"
            path = self.ui.ask(f"Exact private runtime directory [{default}]:").strip() or str(default)
            path = str(Path(path).resolve(strict=False))
            require(path_text(path), "unsafe_private_path")
            directory = PrivateDirectory(path)
            directory.close()
            self.p["runtime"] = {"path": path, "owned": False}
        else:
            raise Halt()
        self.j.save()

    def account(self, expected=None):
        if self.p["vault"] is None:
            # Keep the native credential vault adjacent to the private journal by
            # default; Rust still validates the actual vault parent and account.
            # This does not import CLI credentials or authorize inference.
            path = str(self.j.directory.path / "personas-account.json")
            self.ui.say(f"Native Personas credential vault location: {path}")
            self.p["vault"] = path
            self.j.save()
        while True:
            try:
                status = self.h.call("personas-auth", ["--vault", self.p["vault"], "status"])
            except Blocked as error:
                if error.code in ("protected_vault_unavailable", "vault_unavailable", "unsafe_vault_path", "native_vault_location_unsupported"):
                    raise Blocked("native_vault_location_unsupported") from None
                self.ui.say(f"Selected vault status unavailable ({error.code}). The secure auth helper controls path/access checks.")
                status = {"grant_connected": False}
            if status.get("grant_connected") is True:
                account = status.get("account_sha256")
                require(isinstance(account, str) and re.fullmatch(r"[a-f0-9]{64}", account), "invalid_account_status")
                self.ui.say(f"Selected account fingerprint: {account}; expiry: {status.get('expires_at_unix')}")
                if expected is not None:
                    require(account == expected.get("account_sha256"), "saved_account_mismatch")
                    if status.get("expires_at_unix") != expected.get("saved_expires_at_unix"):
                        self.ui.say("This is the same account but the runtime pins another expiry. It was NOT rebound. "
                                    "Existing work, holds and budgets remain unchanged. Explicit reverification and "
                                    "reviewed provider migration are required; see docs/activation.md.")
                        raise Blocked("explicit_provider_migration_required")
                return status
            self.ui.say("Native SIWC sign-in requires the browser to reach the loopback callback on this same host. "
                        "Opening the authorization URL on another computer alone will not complete this login.")
            self.ui.say("For a remote VM, see the official SIWC sign-in and self-hosted VM guidance:")
            self.ui.say("https://developers.openai.com/siwc/token-sharing-open-source/sign-in")
            self.ui.say("https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms")
            self.ui.say("The documented same-application credential-session transfer route exists, but this launcher "
                        "does not implement credential import or transfer. Do not copy Codex credentials or edit host IDs.")
            self.ui.say("If this host cannot provide the supported callback and protected vault, choose q and stop; "
                        "no sign-in, refresh, or inference is required to leave safely.")
            self.ui.say("Account access is absent, expired, or fenced. s: browser sign-in; r: explicit same-account refresh; q: leave")
            choice = self.ui.ask("Access action:").strip().lower()
            if choice not in ("s", "r"):
                raise Halt()
            command = "sign-in" if choice == "s" else "refresh"
            # The auth helper alone displays its generated URL on stderr, never in
            # a journal or returned UI log. Never retry exchange/refresh on error.
            self.h.call("personas-auth", ["--vault", self.p["vault"], command],
                        timeout=240, auth_display=True)

    def select_provider(self, catalog=None):
        models = [] if catalog is None else catalog.get("models")
        require(isinstance(models, list), "catalogue_unavailable")
        for i, model in enumerate(models, 1):
            self.ui.say(f"{i}: {model['display_name']} ({model['slug']})")
        prompt = "Choose a listed number or enter an exact model slug (no default):" if models else "Exact model slug (no default):"
        raw = self.ui.ask(prompt).strip()
        model = {str(i): m["slug"] for i, m in enumerate(models, 1)}.get(raw, raw)
        require(provider_label(model), "model_selection_required")
        if catalog is not None:
            efforts = catalog.get("effort_candidates_unverified", [])
            self.ui.say("Example effort labels, with no advertised support: " + ", ".join(efforts))
        effort = self.ui.ask("Exact effort label (no default):").strip()
        require(provider_label(effort), "effort_selection_required")
        return model, effort

    def new_probe(self):
        model, effort = self.select_provider()
        request = {"id": "probe-" + uuid.uuid4().hex, "catalogue_sha256": None,
                   "model": model, "effort": effort}
        probe = {"request": request, "view": None, "attempted": False}
        self.p["probes"].append(probe)
        self.j.save()
        return probe

    def selected_settings(self, model, effort):
        settings = self.provider("configure", "--model", model, "--effort", effort)
        key = "selected-" + hashlib.sha256(encoded(settings)).hexdigest()
        self.j.keep("settings", settings, key)
        self.ui.say("Model settings saved with capability unverified. Configuration performed no inference.")
        return settings

    def settings(self):
        if self.p["settings"] is not None:
            path, settings = self.j.document("settings")
            renewal = self.provider("renewal-status", "--saved-config", path)
            if renewal["explicit_provider_migration_required"]:
                # Bootstrap has not been authored yet. Save a new configuration
                # for the same choices and current account/expiry, preserving the
                # previous document. No existing runtime is migrated here.
                return self.selected_settings(settings["model"], settings["effort"])
            return settings
        if self.p["probes"]:
            # A settings file can be durable even if the following progress write
            # was interrupted. Reattach only those exact bytes after the backend
            # confirms their current account/expiry; never invent them
            # or repeat the paid probe just because its UI pointer was lost.
            key = self.p["probes"][-1]["request"]["id"]
            name = "settings-" + key + ".json"
            raw = self.j.directory.read(name, 65536)
            if raw is not None:
                saved = decode(raw)
                path = str(self.j.directory.path / name)
                renewal = self.provider("renewal-status", "--saved-config", path)
                if renewal["explicit_provider_migration_required"]:
                    return self.selected_settings(saved["model"], saved["effort"])
                self.j.keep("settings", saved, key)
                return saved
        while True:
            self.ui.say("Model setup: d: enter model and effort (default); c: browse models; p: optional capability check; q: leave.")
            if self.p["probes"]:
                self.ui.say("Previous check: r: inspect/continue it; u: use its model and effort without requiring proof.")
            choice = self.ui.ask("Model setup:").strip().lower() or "d"
            if choice in ("d", "c", "u"):
                if choice == "u":
                    require(bool(self.p["probes"]), "probe_unavailable")
                    q = self.p["probes"][-1]["request"]
                    model, effort = q["model"], q["effort"]
                else:
                    catalog = self.provider("catalogue") if choice == "c" else None
                    model, effort = self.select_provider(catalog)
                return self.selected_settings(model, effort)
            if choice == "p":
                probe = self.new_probe()
            elif choice == "r" and self.p["probes"]:
                probe = self.p["probes"][-1]
            else:
                raise Halt()
            q = probe["request"]
            if probe["view"] is None:
                args = ["plan", "--id", q["id"],
                        "--model", q["model"], "--effort", q["effort"]]
                if q["catalogue_sha256"] is not None:
                    args += ["--catalogue-sha256", q["catalogue_sha256"]]
                try:
                    probe["view"] = self.provider(*args)
                    self.j.save()
                except Blocked as error:
                    if error.code not in ("catalogue_stale", "credential_changed"):
                        raise
                    self.ui.say("The selected catalogue no longer matches this credential. No request was dispatched. "
                                "The retained selection remains available without requiring that check.")
                    continue
            v = probe["view"]
            args = ["--id", q["id"], "--plan-sha256", v["plan_sha256"]]
            status = self.provider("probe-status", *args)
            phase = status.get("phase")
            if phase == "planned":
                self.ui.say(f"One capability check: model={q['model']}, effort={q['effort']}, "
                            f"account={v['plan']['account_sha256']}; application duration, byte and token quotas are disabled.")
                self.ui.say(v["cost_notice"])
                probe["attempted"] = True
                self.j.save()
                try:
                    status = self.provider("probe", *args)
                except Blocked as error:
                    if error.code != "credential_changed":
                        raise
                    self.ui.say("The check names an earlier credential. Its model and effort remain available without requiring proof.")
                    continue
                phase = status.get("phase")
            if phase == "completed_verified":
                try:
                    # Save the exact completed proof and configuration together.
                    settings = self.provider("configuration", *args)
                    self.j.keep("settings", settings, q["id"])
                    self.ui.say("This check verified its strict carrier and reported effort. Settings and exact evidence were saved privately.")
                    return settings
                except Blocked as error:
                    if error.code != "credential_changed":
                        raise
                    self.ui.say("Completed proof cannot be bound to configuration now. No replacement probe was started.")
            elif phase in ("outcome_unrecorded", "failed_or_uncertain") or status.get("remote_usage_may_remain"):
                self.ui.say("This probe outcome/usage remains unresolved and its identity is never replayed. "
                            "A distinct selected probe does not settle or replace that outcome.")
            else:
                self.ui.say(f"Capability remains unestablished: {phase}, {status.get('code')}. "
                            "No model/effort substitution or fabricated evidence is permitted.")

    def bootstrap(self):
        if self.p["bootstrap"] is None:
            if self.p["codex_auth"] is None and self.p["vault"] is None:
                defaults = self.codex_auth_defaults()
                self.ui.say("Sign in: c: use the existing Codex ChatGPT login shown below; "
                            "p: choose other Codex locations; n: native browser sign-in.")
                self.ui.say(f"Codex: {defaults['executable'] or 'not found on PATH'}; "
                            f"home: {defaults['host_home']}; state: {defaults['codex_home']}.")
                choice = self.ui.ask("Account route [c]:").strip().lower() or "c"
                if choice == "c":
                    self.codex_auth_source(use_defaults=True)
                elif choice == "p":
                    self.codex_auth_source(change=True)
                elif choice == "n":
                    self.account()
                else:
                    raise Halt()
            if self.p["codex_auth"] is not None:
                if self.p["settings"] is None:
                    route = self.codex_auth_source()
                    model, effort = self.select_provider()
                    self.ui.say("Reading the selected Codex login for direct provider requests. No model call is started.")
                    try:
                        settings = self.h.call("personas-provider", ["configure-codex",
                            "--codex-executable", route["executable"], "--host-home", route["host_home"],
                            "--codex-home", route["codex_home"], "--model", model, "--effort", effort])
                    except Blocked as error:
                        if error.code in ("credential_unavailable", "credential_expired"):
                            self.ui.say("The selected Codex login is unavailable. Sign in to ChatGPT using that Codex installation, then reopen ./personas.")
                        raise
                    key = "selected-" + hashlib.sha256(encoded(settings)).hexdigest()
                    self.j.keep("settings", settings, key)
                _, settings = self.j.document("settings")
                require(settings.get("mode") == "chatgpt_oauth", "saved_provider_binding_unsupported")
            else:
                self.account()
                settings = self.settings()
            cohort = self.ui.number("Number of continuing personas", 2, 1, None)
            noun = "persona" if cohort == 1 else "personas"
            self.ui.say(f"Proposed local cohort: {cohort} continuing {noun} and you, without assigned owner/reviewer roles. "
                        "They can maintain their own character and supporting resources, work together and communicate. "
                        "Host command access is enabled by default: personas can install and use tools with this host account and environment.")
            self.ui.say("Give each persona a name and starting profile. Describe openness, conscientiousness, "
                        "extraversion, agreeableness and sensitivity to stress (OCEAN), plus initial mood, "
                        "energy and assertiveness (VAD). Use words; scores are optional. These are starting "
                        "tendencies, not assigned jobs or claimed experience. The first model call authors "
                        "that persona's own characteristic fragments.")
            self.ui.say(f"Default model: {settings['model']} / {settings['effort']}.")
            per_persona = self.ui.ask("Choose a model for each persona? [yes/NO]").strip().lower() == "yes"
            seeds = {}
            for index in range(1, cohort + 1):
                persona = f"persona-{index:04d}"
                label = self.ui.bounded_text(f"Name for {persona}", 256, f"Persona {index}")
                profile = self.ui.bounded_text(f"Starting profile for {label}:", 16384)
                seeds[persona] = {"label": label, "text": profile,
                                  "provenance": f"Starting profile supplied by {self.p['operator']} during setup; no prior experience asserted."}
                if per_persona:
                    model = self.ui.ask(f"Model for {label} [{settings['model']}]:").strip() or settings["model"]
                    effort = self.ui.ask(f"Effort for {label} [{settings['effort']}]:").strip() or settings["effort"]
                    require(provider_label(model) and provider_label(effort), "invalid_model_selection")
                    seeds[persona].update(model=model, effort=effort)
            default_directory = self.j.directory.path / "workspace"
            directory = self.ui.ask(f"Task working directory [{default_directory}]:").strip() or str(default_directory)
            directory = Path(directory).resolve(strict=False)
            directory.mkdir(parents=True, exist_ok=True)
            request = {"id": "bootstrap-" + uuid.uuid4().hex, "operator": self.p["operator"],
                       "persona_seeds": seeds,
                       "setup": {"cohort": cohort, "destination": "openai", "provider": settings,
                                 "working_directory": str(directory)}}
            self.j.keep("bootstrap", request, request["id"])
        path, request = self.j.document("bootstrap")
        preview = self.session("bootstrap", path, "--preview")
        setup = request["setup"]
        for persona, seed in preview["persona_seeds"].items():
            selection = preview["persona_models"][persona]
            self.ui.say(f"Starting profile — {seed['label']} ({persona}): {seed['text']}\n"
                        f"Source: {seed['provenance']}\nModel: {selection['model']} / {selection['effort']}")
        noun = "persona" if setup["cohort"] == 1 else "personas"
        self.ui.say(f"Runtime: {self.p['runtime']['path']}\n"
                    f"Operator: {self.p['operator']}; working directory: {setup['working_directory']}\n"
                    f"Share needs, persona context and outputs with OpenAI, {setup['cohort']} {noun} and this requester. "
                    "The runtime imposes no work/session/lifetime call or cumulative storage/navigation quotas. "
                    "Usage is recorded. Explicit stops, pauses and actual execution results remain effective.")
        if self.ui.ask("Show saved configuration and accounting estimates? [yes/NO]").strip().lower() == "yes":
            self.ui.say(json.dumps(preview, ensure_ascii=False, indent=2))
        self.p["setup_authorized"] = True
        self.j.save()
        self.session("bootstrap", path)
        # No UI 'setup complete' flag: every return/restart consults Runtime.

    def inspect(self, after=None):
        args = ["inspect", "--limit", "8"]
        if after:
            args += ["--after", after]
        return self.session(*args)

    def show_call_failures(self, work):
        attempts = [work.get("last_attempt")]
        attempts.extend(person.get("active_invocation") for person in work["participants"])
        seen, groups = set(), {}
        for attempt in attempts:
            if not attempt or attempt["id"] in seen or not attempt.get("failure"):
                continue
            seen.add(attempt["id"])
            failure = {key: value for key, value in attempt["failure"].items() if key != "terminal"}
            key = encoded(failure)
            if key not in groups:
                groups[key] = (failure, [])
            groups[key][1].append(attempt["id"])
        for failure, calls in groups.values():
            detail = failure.get("failure") or failure.get("kind") or "retained failure"
            if failure.get("http_status") is not None:
                detail += f"; HTTP {failure['http_status']}"
            if failure.get("provider_error_code"):
                detail += f"; {failure['provider_error_code']}"
            if failure.get("unsupported_parameter"):
                detail += f"; parameter {failure['unsupported_parameter']}"
            self.ui.say(f"   Call failure ({', '.join(calls)}): {detail}.")
            if failure.get("provider_message"):
                label = "Provider explanation (redacted)" if failure.get("provider_message_redacted") else "Provider explanation"
                self.ui.say(f"   {label}: {failure['provider_message']}")

    def show_messages(self, messages, show_invocation=False, shown_messages=None):
        if shown_messages is not None:
            messages = [message for message in messages if message["id"] not in shown_messages]
        for part in readable_messages(messages, show_invocation=show_invocation):
            self.ui.say(part)
        displayed = {message["id"] for message in messages}
        if shown_messages is not None:
            shown_messages.update(displayed)
        return displayed

    def show_persona_activity(self, result, shown_activity=None):
        descriptions = {"ongoing": "continuing", "blocked": "reports a blocker",
                        "individual_sufficient": "judges own contribution sufficient",
                        "shared_sufficient": "judges the shared result sufficient",
                        "closed": "reports work closed"}
        for item in result.get("persona_activity", []):
            judgment = item["judgment"]
            origin = item.get("origin") or {}
            # Compare exact accepted records, never similar prose or an inferred
            # task state. Reopening and explicit results show them again.
            key = encoded(item)
            if shown_activity is not None and shown_activity.get(item["persona"]) == key:
                continue
            label = item["label"]
            kind = descriptions[judgment["kind"]]
            call = origin.get("invocation")
            self.ui.say(f"Recorded work update — {label}: {kind}"
                        + (f" ({call})." if call else " (call attribution unavailable)."))
            revision = origin.get("mandate_revision")
            if revision is None:
                self.ui.say("The task version for this account is unavailable.")
            elif revision != item["current_mandate_revision"]:
                self.ui.say(f"This account predates the current task amendment (mandate {revision}).")
            self.ui.say(judgment["reason"])
            if shown_activity is not None:
                shown_activity[item["persona"]] = key

    def output(self, work, receipt_json=False, result=None, shown_messages=None, shown_activity=None,
               delivery_details=False):
        if result is None:
            result = self.session("result", work)
        amendment = result.get("recorded_task_amendment")
        if amendment:
            self.ui.say(f"Saved amended request (mandate {amendment['mandate_revision']}): "
                        + amendment["saved_scope"]["request"])
            self.ui.say("Earlier replies and deliveries may predate this amendment.")
        activity = result.get("optional_activity")
        if activity:
            self.ui.say(f"Optional activity {work}: {activity['phase']}.")
            if activity.get("revoked_at_unix") is not None:
                self.ui.say("This opportunity has been revoked.")
            descriptions = {"ready": "ready to participate", "invited": "has not yet chosen",
                            "stopped": "stopped", "paused": "paused", "waiting": "waiting",
                            "scheduled": "chose to reconsider later", "held": "has an unresolved failed attempt",
                            "pending_invocation": "has a pending or unresolved model call",
                            "unavailable": "is unavailable"}
            for participant in activity["participants"]:
                label = participant["label"]
                self.ui.say(f"{label}: {descriptions.get(participant['status'], participant['status'])}.")
                authored = participant.get("last_authored_continuation")
                if authored:
                    decision = authored["decision"]
                    reason = decision.get("reason") or decision.get("purpose")
                    if reason:
                        self.ui.say(f"{label}'s recorded choice: {reason}")
                if participant.get("lifecycle_reason"):
                    self.ui.say(participant["lifecycle_reason"])
        elif result.get("optional_activity_ending"):
            self.ui.say(f"Work {work}: the optional activity ended as recorded. "
                        "No artifact or later benefit was required or established.")
        else:
            status = result.get("status", "unavailable")
            description = {"completed-as-recorded": "completion recorded",
                           "delivered-acceptance-unestablished": "delivered; stakeholder acceptance is unestablished",
                           "ended-unfinished": "ended with unfinished work",
                           "intake-incomplete": "task intake is incomplete"}.get(status, status)
            self.ui.say(f"Work {work}: {description}.")
            phase = result.get("recorded_phase")
            if phase and phase != status:
                if phase == "open":
                    self.ui.say("The work remains open; each persona keeps its own continuation choice.")
                else:
                    self.ui.say(f"Recorded work phase: {phase}.")
        for creation in result.get("persona_creations", []):
            parent = creation["parent_label"]
            child = creation.get("child")
            if child:
                character = "accepted" if child["character_accepted"] else "not yet accepted"
                self.ui.say(f"Persona creation: {parent} created {child['label']} ({child['persona']}). "
                            f"Initial character: {character}.")
                opportunities = child["work_opportunities"]
                if not opportunities:
                    self.ui.say("No work invitation is recorded. Open a task or optional activity "
                                "to give this persona an opportunity.")
                for opportunity in opportunities:
                    acceptance = "accepted" if opportunity["responsibility_accepted"] else "not accepted"
                    self.ui.say(f"Child work {opportunity['work']}: responsibility {acceptance}; "
                                f"{opportunity['continuation']}. Open that work to inspect its result.")
            else:
                self.ui.say(f"Persona creation: {parent}'s adopted request is pending. "
                            "Continue this work to finish the recorded creation.")
        self.show_persona_activity(result, shown_activity)
        messages = result.get("messages", [])
        deliveries = result.get("local_deliveries", [])
        feedback = (result.get("requester_feedback") or {}).get("items", [])
        if feedback:
            pending = sum(item.get("status") == "pending" for item in feedback)
            superseded = sum(item.get("status") == "superseded" for item in feedback)
            self.ui.say(f"Visible requester feedback: {pending} pending, {superseded} replaced, "
                        f"{len(feedback) - pending - superseded} with a recorded or unavailable disposition. "
                        "A persona's account does not establish that a requested artifact change was made.")
            for item in feedback:
                status = {"pending": "has not yet accounted for this feedback",
                          "handled": "marked this feedback handled",
                          "superseded": "has earlier feedback explicitly replaced by your later direction",
                          "declined": "declined this feedback"}.get(item.get("status"), "has no currently available feedback disposition")
                self.ui.say(f"{item['label']} {status}.")
                self.ui.say(feedback_mandate_summary(item))
                if item.get("feedback_body_supplied"):
                    self.ui.say("Your feedback: " + item["feedback_text"])
                if item.get("file_references"):
                    self.ui.say("Attached exact file references (metadata, not supplied content):")
                    for reference in item["file_references"]:
                        self.ui.say(json.dumps(reference, ensure_ascii=False, sort_keys=True))
                if item.get("account"):
                    self.ui.say(item["label"] + "'s account: " + item["account"]["reason"])
        displayed = self.show_messages(messages, shown_messages=shown_messages)
        for delivery in deliveries:
            self.ui.say("Local delivery with full contents and qualifications:" if delivery_details or receipt_json
                        else "Delivered files:")
            parts = None if receipt_json else readable_delivery(delivery, summary=not delivery_details)
            if parts is None:
                if receipt_json or delivery_details:
                    self.ui.say(json.dumps(delivery, ensure_ascii=False, indent=2))
                else:
                    self.ui.say("A delivery record is available, but its file list could not be rendered. "
                                "Choose v for the complete record, or j in the work menu for its exact receipt.")
            else:
                for part in parts:
                    self.ui.say(part)
        if result.get("truncated"):
            self.ui.say("This permission-projected result is truncated at its existing bound.")
        if result.get("human_decision_pointers"):
            self.ui.say("Explicit acceptance is separate; no acceptance was fabricated. See docs/activation.md and RUNTIME-REFERENCE.md.")
        return displayed

    def pending_work_input(self):
        item = self.p["work_input"]
        return item if item is not None and not item["delivered"] else None

    def deliver_work_input(self):
        item = self.pending_work_input()
        require(item is not None, "no_pending_work_input")
        request = item["request"]
        # This one replaceable transport file is reconstructed from the journal's
        # exact request. The journal lock protects it through the helper call.
        # A new identity may replace the pointer only after confirmed delivery.
        self.j.directory.write("work-input.json", encoded(request))
        receipt = self.session(item["kind"], str(self.j.directory.path / "work-input.json"))
        if item["kind"] == "amend-work":
            require(receipt.get("work") == request["work"] and receipt.get("id") == request["id"]
                    and type(receipt.get("scope_saved")) is bool
                    and receipt.get("snapshot_changed") is (not receipt["scope_saved"])
                    and receipt.get("inference_performed") is False
                    and receipt.get("budget_reset") is False, "amendment_receipt_mismatch")
            item["delivered"] = True
            self.j.save()
            if receipt["scope_saved"]:
                self.ui.say(f"Amended request saved as mandate {receipt['mandate_revision']}.")
                for persona, status in receipt["resumptions_at_save"].items():
                    self.ui.say(f"{persona}: {status.replace('_', ' ')}")
                self.ui.say("Choose Continue to start eligible work.")
                if receipt.get("replayed"):
                    self.ui.say("This is the earlier receipt; no new resumption was requested.")
            else:
                self.ui.say("The task changed before the amendment was saved. Choose Amend task again to use its current state.")
            return
        if item["kind"] == "resume-participant":
            require(receipt.get("work") == request["work"]
                    and receipt.get("persona") == request["persona"]
                    and receipt.get("id") == request["id"]
                    and receipt.get("inference_performed") is False
                    and receipt.get("budget_reset") is False
                    and receipt.get("replay_authorized") is False
                    and type(receipt.get("replayed")) is bool
                    and type(receipt.get("resume_recorded")) is bool
                    and receipt.get("snapshot_changed") is (not receipt["resume_recorded"])
                    and receipt.get("persona_resumed") is
                        (receipt["resume_recorded"] and not receipt["replayed"]),
                    "resume_receipt_mismatch")
            recovery = receipt.get("recovery")
            require(recovery is None or (isinstance(recovery, dict)
                    and recovery.get("kind") in PARTICIPANT_RECOVERY_REASONS
                    and isinstance(recovery.get("invocation"), str)
                    and isinstance(recovery.get("terminal"), dict)), "resume_receipt_mismatch")
            require(receipt.get("recovery_performed") is
                    (recovery is not None and not receipt["replayed"]), "resume_receipt_mismatch")
            if receipt["resume_recorded"]:
                recorded = receipt.get("receipt")
                require(isinstance(recorded, dict) and recorded.get("request") == request
                        and recorded.get("recovery") == recovery,
                        "resume_receipt_mismatch")
            else:
                require(recovery is None, "resume_receipt_mismatch")
            item["delivered"] = True
            self.j.save()
            if receipt["resume_recorded"]:
                self.ui.say("The explicit resume is recorded. " +
                            ("This is the earlier receipt; it did not resume the participant again. "
                             if receipt.get("replayed") else "") +
                            "Choose Continue to review current eligibility and start a model session.")
                if recovery is not None:
                    if receipt["replayed"]:
                        self.ui.say(f"The original recovery retained the failure from {recovery['invocation']}. "
                                    "This receipt does not queue that failure again.")
                    else:
                        self.ui.say(f"The failure from {recovery['invocation']} is retained for the next decision. "
                                    "Its proposed actions were not replayed.")
            else:
                self.ui.say("The work changed before this resume could apply. Nobody was resumed. "
                            "Choose Resume participant again to review the current state.")
            return
        require(receipt.get("work") == request["work"]
                and receipt.get("inference_performed") is False
                and receipt.get("persona_resumed") is False
                and receipt.get("budget_reset") is False, "input_receipt_mismatch")
        if item["kind"] == "feedback":
            require(receipt.get("persona") == request["persona"], "input_receipt_mismatch")
        else:
            require(receipt.get("question") == request["reply"]["question"]
                    and receipt.get("reply") == request["reply"]["id"], "input_receipt_mismatch")
        item["delivered"] = True
        self.j.save()
        self.ui.say("Your input is retained. " + ("The same input was already recorded. "
                    if receipt.get("replayed") else "") + "No model call was started.")
        if receipt.get("queued"):
            self.ui.say("It is queued for delivery after the current calls and commands settle. "
                        "The current model input is unchanged; a stopped participant is not resumed.")
        elif receipt.get("delivery", {}).get("kind") == "unavailable":
            self.ui.say("Delivery is unavailable: " + receipt["delivery"]["reason"])
        if receipt.get("existing_waits_satisfied", 0):
            self.ui.say("It satisfied an existing requester-input wait. A live session can continue; "
                        "otherwise choose Continue to run the work.")
        else:
            self.ui.say("Participants keep their existing continuation choices. Stopped or paused participants need an explicit resume.")

    def queue_work_input(self, kind, request):
        require(self.pending_work_input() is None, "pending_work_input_requires_delivery")
        self.p["work_input"] = {"kind": kind, "request": request, "delivered": False}
        self.j.save()
        self.deliver_work_input()

    def pending_work_control(self):
        # Stops have their own durable slot so unconfirmed feedback or an answer
        # cannot prevent the requester from stopping active work.
        raw = self.j.directory.read("work-control.json")
        if raw is None:
            return None
        item = decode(raw)
        require(isinstance(item, dict) and set(item) == {"kind", "request", "delivered"}
                and item["kind"] in ("stop-work", "resume-work")
                and type(item["delivered"]) is bool, "invalid_work_control")
        request = item["request"]
        require(isinstance(request, dict) and identifier(request.get("id"))
                and request.get("operator") == self.p["operator"]
                and identifier(request.get("work")), "invalid_work_control")
        if item["kind"] == "stop-work":
            require(set(request) == {"id", "operator", "work", "action", "reason"}
                    and request["action"] in ("pause", "cancel") and safe_text(request["reason"]),
                    "invalid_work_control")
        else:
            require(set(request) == {"id", "operator", "work", "expected_runtime", "expected_launch"},
                    "invalid_work_control")
            for key, name in (("expected_runtime", "runtime-state-v1"), ("expected_launch", "runtime-launch-v1")):
                ref = request[key]
                require(isinstance(ref, dict) and set(ref) == {"id", "revision", "sha256"}
                        and ref["id"] == name and type(ref["revision"]) is int and ref["revision"] > 0
                        and re.fullmatch(r"[a-f0-9]{64}", ref["sha256"]) is not None, "invalid_work_control")
        return None if item["delivered"] else item

    def deliver_work_control(self):
        item = self.pending_work_control()
        require(item is not None, "no_pending_work_control")
        request = item["request"]
        self.j.directory.write("control-request.json", encoded(request))
        receipt = self.session(item["kind"], str(self.j.directory.path / "control-request.json"))
        require(receipt.get("id") == request["id"] and receipt.get("work") == request["work"]
                and receipt.get("request") == request and type(receipt.get("replayed")) is bool
                and type(receipt.get("control_recorded")) is bool
                and receipt.get("inference_performed") is False and receipt.get("persona_resumed") is False
                and receipt.get("goal_completed") is False, "work_control_receipt_mismatch")
        if item["kind"] == "resume-work":
            require(receipt.get("snapshot_changed") is (not receipt["control_recorded"])
                    and (not receipt["replayed"] or receipt["control_recorded"]), "work_control_receipt_mismatch")
        else:
            require((receipt.get("runtime_request") is not None) is receipt["control_recorded"]
                    and (receipt.get("receipt") is not None) is receipt["control_recorded"],
                    "work_control_receipt_mismatch")
        item["delivered"] = True
        self.j.directory.write("work-control.json", encoded(item))
        if receipt["replayed"]:
            self.ui.say("This is the earlier control receipt. The action was not applied again; inspect the current work status.")
        elif not receipt["control_recorded"] and item["kind"] == "stop-work":
            self.ui.say("The current work phase does not allow that control. This request made no change and will not apply later; inspect the work status.")
        elif not receipt["control_recorded"]:
            self.ui.say("The work changed before resumption. Choose Resume work again to use its current state.")
        elif item["kind"] == "resume-work":
            self.ui.say("The work-wide pause is lifted. Individual continuation choices remain. Choose Continue for eligible work, or Resume participant for an individual stop or pause.")
        elif request["action"] == "pause":
            self.ui.say("Work pause recorded. Any active calls and commands will record their outcomes. Use Resume work later to lift this pause.")
        else:
            self.ui.say("Work cancellation recorded. Any active calls and commands will record their outcomes. Saved results remain available; cancellation does not mark the task complete.")

    def control_work(self, work, action):
        if self.pending_work_control() is not None:
            self.ui.say("A saved work control still needs confirmation. Choose i to finish that same request.")
            return
        request = {"id": "control-" + uuid.uuid4().hex, "operator": self.p["operator"], "work": work}
        if action == "resume":
            current, view = self.work_snapshot(work)
            require(current is not None, "work_unavailable")
            if current["phase"] != "paused":
                self.ui.say("This work has no work-wide pause. Use Continue for eligible work or Resume participant for an individual stop or pause.")
                return
            request.update(expected_runtime=view["runtime"], expected_launch=view["launch"])
            kind = "resume-work"
        else:
            default = "Paused by the requester in the launcher." if action == "pause" else "Canceled by the requester in the launcher."
            reason = self.ui.ask(f"Reason to {action} this work [Enter: {default}]")
            request.update(action=action, reason=reason if reason.strip() else default)
            kind = "stop-work"
        self.j.directory.write("work-control.json", encoded({"kind": kind, "request": request, "delivered": False}))
        self.deliver_work_control()

    def choose_participant(self, work):
        participants = work["participants"]
        for i, participant in enumerate(participants, 1):
            attempt = participant.get("active_invocation")
            status = (self.attempt_status(attempt) if attempt and
                      attempt["state"] in ("Prepared", "Dispatched", "Responded") else
                      self.continuation_label(participant["continuation"]))
            self.ui.say(f"{i}: {participant.get('label', participant['persona'])} "
                        f"({participant['persona']}) — {status}")
            recovery = participant.get("resume_recovery")
            if recovery is not None:
                reason = PARTICIPANT_RECOVERY_REASONS.get(recovery["kind"], "recorded call failure")
                self.ui.say(f"   {recovery['invocation']}: {reason}; the next decision receives the failure.")
        choice = self.ui.ask("Participant number (Enter to go back):").strip()
        if not choice:
            return None
        require(choice.isdecimal() and 1 <= int(choice) <= len(participants), "invalid_participant_choice")
        return participants[int(choice) - 1]["persona"]

    def resume_participant(self, work, after):
        if self.pending_work_input() is not None:
            self.ui.say("An earlier work action still needs confirmation. Return to the work list and choose i to finish it.")
            return
        view = self.inspect(after)
        current = next((w for w in view["works"] if w["id"] == work["id"]), None)
        if current is None:
            self.ui.say("This work is no longer in the current view. Return to the work list.")
            return
        candidates = [p for p in current["participants"] if p.get("resume_candidate")]
        if not candidates:
            self.ui.say("No participant can be resumed here. Ready participants can Continue; "
                        "choose Resume work for a work-wide pause. Unresolved execution needs its recorded recovery controls.")
            return
        persona = self.choose_participant({"participants": candidates})
        if persona is None:
            return
        self.queue_work_input("resume-participant", {"id": "resume-" + uuid.uuid4().hex,
            "operator": self.p["operator"], "work": work["id"], "persona": persona,
            "expected_runtime": view["runtime"], "expected_launch": view["launch"],
            "authorize_resume": True})

    def amend_work(self, work):
        if self.pending_work_input() is not None:
            self.ui.say("An earlier work action still needs confirmation. Return to the work list and choose i to finish it.")
            return
        view = self.session("amend-work-snapshot", work["id"])
        scope = view["current_scope"]
        if (scope["outcomes"] != {"result": {"description": scope["request"],
                "criterion": scope["request"], "required_reviews": []}} or scope["review_roles"]):
            self.ui.say("This task has custom outcome or review terms. Use the full amendment command described in docs/task-amendments.md.")
            return
        self.ui.say("Current request: " + scope["request"])
        text = self.ui.ask("New complete request (saves the amendment and requests eligible resumption; Enter to cancel):")
        if not text.strip():
            return
        require(safe_text(text), "invalid_amendment_text")
        scope["request"] = text
        scope["outcomes"]["result"]["description"] = text
        scope["outcomes"]["result"]["criterion"] = text
        key = "amend-" + uuid.uuid4().hex
        invitations = [{"id": f"{key}-{i}", "to": actor, "terms": {
            "mandate_revision": view["next_mandate_revision"],
            "exact_terms": "Invitation to consider the amended mandate voluntarily; any responsibility or decision arrangement requires fresh persona acceptance.",
            "outcomes": [], "review_roles": []}}
            for i, actor in enumerate(view["participants"])]
        self.queue_work_input("amend-work", {"id": key, "operator": self.p["operator"],
            "work": work["id"], "scope": scope, "invitations": invitations,
            "resume": view["participants"], "expected_runtime": view["runtime"],
            "expected_launch": view["launch"], "authorize_amend": True})

    def feedback(self, work):
        if self.pending_work_input() is not None:
            self.ui.say("An earlier input still needs delivery confirmation. Return to the work list and choose i to finish that same input.")
            return
        persona = self.choose_participant(work)
        if persona is None:
            return
        text = self.ui.ask(f"Feedback to store for {persona} in {work['id']} (Enter to cancel):")
        if not text.strip():
            return
        require(safe_text(text), "invalid_feedback_text")
        result = self.session("result", "--", work["id"])
        current = [item for item in (result.get("requester_feedback") or {}).get("items", [])
                   if item.get("persona") == persona
                   and item.get("status") not in ("superseded", "delivery_unavailable")
                   and item.get("queued_current_direction", True)]
        supersedes = []
        if current:
            self.ui.say("Earlier current or queued feedback without a later replacement:")
            for index, item in enumerate(current, 1):
                self.ui.say(f"{index}. {item['feedback_text']}")
                self.ui.say(feedback_mandate_summary(item))
            choice = self.ui.ask("Enter numbers of feedback this message replaces, separated by spaces, "
                                 "or Enter to keep all earlier directions. Their original history is retained:")
            if choice.strip():
                numbers = choice.split()
                require(all(value.isascii() and value.isdigit() for value in numbers), "invalid_feedback_selection")
                selected = [int(value) for value in numbers]
                require(len(set(selected)) == len(selected)
                        and all(1 <= value <= len(current) for value in selected), "invalid_feedback_selection")
                supersedes = [current[value - 1]["observation"] for value in selected]
        request = {"id": "feedback-" + uuid.uuid4().hex,
                   "work": work["id"], "persona": persona, "text": text}
        if supersedes:
            request["supersedes"] = supersedes
        self.queue_work_input("feedback", request)

    def questions(self, work):
        result = self.session("questions", "--", work)
        for answer in result.get("answer_delivery", []):
            delivery = answer["delivery"]
            if delivery["kind"] == "queued":
                self.ui.say(f"Your answer {answer['reply']} is queued until current calls and commands settle.")
            elif delivery["kind"] == "unavailable":
                self.ui.say(f"Your answer {answer['reply']} could not be delivered: {delivery['reason']}")
        view = result.get("shared_questions")
        if view is None or not view.get("items"):
            self.ui.say("No shared questions are currently available in this work.")
            return
        available = []
        for item in view["items"]:
            question = item.get("question")
            if question is None:
                self.ui.say("A shared question is currently unavailable.")
                continue
            available.append(item)
            self.ui.say(f"{len(available)}: {question['text']}\n"
                        f"Owner: {item['owner']}; addressed to: {question['addressee']}; "
                        f"question work: {item.get('question_work', work)}")
            self.ui.say("Why it matters: " + question["why"])
            self.ui.say("Affected claims: " + "; ".join(question["affected_claims"]))
            self.ui.say("Evidence needed: " + question["evidence_needed"])
            if question.get("human_input_required"):
                self.ui.say("Input needed from the person concerned: " + question["human_input_required"])
            if question.get("provisional_interpretation"):
                self.ui.say("Provisional interpretation: " + question["provisional_interpretation"])
            if item.get("author_accepted_on_question_work") is False:
                self.ui.say("The author has no current acceptance of this work; they can address the question from another work.")
            for reply in item["replies"]:
                if "input" not in reply:
                    self.ui.say("A reply is currently unavailable.")
                    continue
                self.ui.say(f"Reply {reply['input']['id']} from {reply['attribution']['author']}:\n"
                            f"{reply['input']['text']}\nAttributed basis: {reply['input']['basis']}")
                if reply['attribution'].get('causal_work'):
                    self.ui.say("Reply authored in: " + reply['attribution']['causal_work'])
            for conclusion in item["conclusions"]:
                if "input" not in conclusion:
                    self.ui.say("An owner conclusion is currently unavailable.")
                    continue
                value = conclusion["input"]
                self.ui.say(f"Owner conclusion ({value['disposition']}): {value['conclusion']}")
                for reply_id, account in value["replies"].items():
                    self.ui.say(f"  {reply_id}: {account['disposition']} — {account['reason']}")
            pending = item.get("owner_review_pending")
            self.ui.say("Owner review: " + ("pending" if pending is True else
                        "current replies accounted for" if pending is False else "unavailable"))
        self.ui.say("Reply bases and conclusions are attributed statements; they do not independently establish facts or consent.")
        if not available:
            return
        if self.pending_work_input() is not None:
            self.ui.say("An earlier input still needs delivery confirmation. Return to the work list and choose i to finish that same input.")
            return
        choice = self.ui.ask("Question number to answer (Enter to go back):").strip()
        if not choice:
            return
        require(choice.isdecimal() and 1 <= int(choice) <= len(available), "invalid_question_choice")
        selected = available[int(choice) - 1]
        text = self.ui.ask("Your answer to store for this question's readers (Enter to cancel):")
        if not text.strip():
            return
        require(safe_text(text), "invalid_answer_text")
        basis = self.ui.ask("Basis for your answer, such as your preference or an observation (Enter to cancel):")
        if not basis.strip():
            return
        require(safe_text(basis), "invalid_answer_basis")
        self.queue_work_input("answer-question", {"work": work, "reply": {
            "id": "answer-" + uuid.uuid4().hex, "question": selected["question"]["id"],
            "question_work": selected.get("question_work", work),
            "question_revision": selected["question_revision"], "text": text, "basis": basis}})

    def development(self, work):
        persona = self.choose_participant(work)
        if persona is None:
            return
        source = self.ui.ask("Exact source ID (Enter for all retained characteristic revisions):").strip()
        invocation = self.ui.ask("Accepted call ID to inspect (Enter for guidance only):").strip()
        args = ["development"]
        if source:
            args.append("--source=" + source)
        if invocation:
            args.append("--invocation=" + invocation)
        result = self.session(*args, "--", persona)
        self.ui.say(f"{result['label']} ({persona}) — retained characteristic guidance")
        lifecycle = result.get("lifecycle")
        if lifecycle:
            self.ui.say(f"Identity: {lifecycle['status']}; quarantined: {lifecycle['quarantined']}. "
                        "Work participation and continuation are separate.")
        origin = result.get("creation_origin")
        if origin and origin.get("available"):
            self.ui.say(f"Starting proposal: authored by {origin['parent']} in "
                        f"{origin['proposal_author_invocation']}.")
            self.ui.say("Proposal reference: " + json.dumps(origin["proposal"], ensure_ascii=False))
            self.ui.say(f"Creation context: {origin['causal_work']}. "
                        "The exact receipt separately identifies the creation request.")
            self.ui.say("Creation receipt: " + json.dumps(origin["creation_receipt"], ensure_ascii=False))
        character = result.get("character")
        if character:
            self.ui.say(f"Current character revision: {character['revision']}")
            for scope in ("ocean", "vad"):
                self.ui.say("Stable OCEAN tendencies:" if scope == "ocean" else "Baseline VAD:")
                for name, value in character[scope].items():
                    self.ui.say(f"{name}: {value}")
        affect = result.get("transient_affect")
        if affect:
            self.development_affect(affect["context"], [affect["observation"]] if affect.get("observation") else [],
                                    "Current modeled affect")
        for revision in result["characteristic_revisions"]:
            self.ui.say(f"{revision['source']} @ {revision['revision']} "
                        f"({'current' if revision['current'] else 'historical'})")
            self.ui.say("Authorship: " + json.dumps(revision["origin"], ensure_ascii=False))
            # Keep the complete qualified package. A convenient prose excerpt
            # must not detach a fragment from its required accompanying text.
            self.ui.say(json.dumps(revision["context"], ensure_ascii=False, indent=2))
        evidence = result.get("invocation_evidence")
        if evidence:
            self.ui.say(f"Accepted call: {evidence['invocation']}; completed provider input matches: "
                        f"{evidence['provider_completed_with_matching_input']}")
            supplied = evidence["supplied_characteristic_revisions"]
            for revision in supplied:
                ref = revision["reference"]
                self.ui.say(f"Characteristic text supplied: {ref['source']} @ {ref['revision']}")
                self.ui.say("Exact representation and range: " + json.dumps(ref, ensure_ascii=False))
                self.ui.say(f"Supplied text: {revision['supplied_utf8_bytes']} UTF-8 bytes; "
                            f"SHA-256 {revision['supplied_text_sha256']}; "
                            f"exact retained projection matched: {revision['exact_projected_body_matched']}; "
                            f"complete UTF-8 file supplied: {revision['complete_body_in_prepared_input']}")
                self.ui.say("Supply routes and qualification roles: "
                            + json.dumps(revision["supply_routes"], ensure_ascii=False))
            if not supplied:
                self.ui.say("No characteristic entries from the revisions in this view were supplied through the routes inspected here.")
            if evidence.get("supplied_transient_affect") is not None:
                self.development_affect(evidence["supplied_transient_affect"],
                                        evidence.get("supplied_affect_observations", []),
                                        "Modeled affect supplied to this accepted call")
            self.development_decision(evidence)
            self.resource_use(evidence)
        if result.get("truncated"):
            self.ui.say("This bounded view omits additional revisions. Use an exact source ID to narrow it.")
        self.ui.say("Retained guidance and actual input supply do not by themselves establish improved behavior. Review the work and its consequences.")

    def development_affect(self, context, observations, label):
        self.ui.say(f"{label}: {context.get('status', 'unavailable')}.")
        self.ui.say("Authored contextual interpretation, separate from baseline character; no psychological measurement.")
        self.ui.say(json.dumps(context, ensure_ascii=False, indent=2))
        for observation in observations:
            # Preserve attribution and complete required qualifications together.
            self.ui.say(json.dumps(observation, ensure_ascii=False, indent=2))

    def resource_use(self, evidence):
        resources = evidence.get("resource_use")
        if resources is None:
            self.ui.say("This helper does not expose supporting-resource supply evidence.")
            return
        if not resources.get("available"):
            self.ui.say("Supporting-resource evidence unavailable: " + resources.get("reason", "unavailable"))
            return
        self.ui.say("Supporting resources supplied to this accepted call (metadata):")
        for resource in resources["supplied_supporting_resources"]:
            ref = resource["reference"]
            self.ui.say(f"{resource['path']} — {ref['source']} @ {ref['revision']}")
            self.ui.say("Exact representation and range: " + json.dumps(ref, ensure_ascii=False))
            self.ui.say(f"Supplied text: {resource['supplied_utf8_bytes']} UTF-8 bytes; "
                        f"SHA-256 {resource['supplied_text_sha256']}; "
                        f"exact retained projection matched: {resource['exact_projected_body_matched']}; "
                        f"complete UTF-8 revision matched: {resource['complete_utf8_revision_matched']}")
            self.ui.say("Supply route: " + json.dumps(resource["supply_route"], ensure_ascii=False)
                        + "; qualification roles: " + json.dumps(resource["roles"], ensure_ascii=False))
        if not resources["supplied_supporting_resources"]:
            self.ui.say("No supporting-file entries were supplied through the routes inspected here.")
        self.ui.say(f"Execution intents in this accepted decision: {resources['execution_intents_in_accepted_decision']}.")
        self.ui.say(resources["scope"])
        self.ui.say(resources["limits"])
        execution = evidence.get("computation_execution")
        if execution is not None:
            if not execution.get("available"):
                self.ui.say("Computation evidence unavailable: " + execution.get("reason", "unavailable"))
            for item in execution.get("executions", []):
                self.ui.say(f"Computation {item['operation']}: {item['state']}.")
                self.ui.say("Exact program digest: " + item["program_sha256"])
                terminal = item.get("terminal")
                if terminal:
                    self.ui.say("Observed result: " + json.dumps(terminal["body"]["outcome"], ensure_ascii=False))
                    for bundle in item.get("current_required_qualifications", []):
                        for entry in bundle.get("entries", []):
                            self.ui.say("Required qualification: " + entry["text"])
                self.ui.say("Execution alone does not establish correctness or skill application.")

    def development_decision(self, evidence):
        decision = evidence.get("accepted_decision")
        if decision is None:
            self.ui.say("This helper does not expose the accepted maintenance decision.")
        elif not decision.get("available"):
            self.ui.say("Accepted decision unavailable: " + decision.get("reason", "unavailable"))
        else:
            self.ui.say("Recorded adoption: " + decision["adoption_status"])
            if decision.get("adoption_reason"):
                self.ui.say(decision["adoption_reason"])
            for field, label in (("work", "Authored work judgment"),
                                 ("fragment_maintenance", "Characteristic maintenance"),
                                 ("asset_maintenance", "Supporting-asset maintenance"),
                                 ("next_fragments", "Authored next fragments"),
                                 ("next_assets", "Authored next assets"),
                                 ("continuation", "Authored continuation")):
                # Preserve reasons, consequences, evidence and exact selections;
                # a short label must not detach a judgment from its conditions.
                self.ui.say(label + ": " + json.dumps(decision[field], ensure_ascii=False, indent=2))
            if not decision["operations"]:
                self.ui.say("No operations were proposed in this response.")
            for operation in decision["operations"]:
                name = operation.get("command") or operation["kind"]
                self.ui.say(f"{name.replace('_', ' ')} — {operation['disposition'].replace('_', ' ')} "
                            f"({operation['id']})")
                stored = operation.get("retained_write")
                if stored and stored["status"] == "retained_exact_revision":
                    ref = stored["reference"]
                    self.ui.say(f"Retained {stored['scope']}: {ref['source']} @ {ref['revision']} "
                                "(metadata only; current selection is separate)")
                    if stored.get("source_retired"):
                        self.ui.say("This source is retired; this historical write does not make it usable again.")
                elif stored:
                    self.ui.say("An exact retained write is unavailable in this view.")
            self.ui.say(decision["scope"])
        accounting = evidence.get("input_accounting")
        if accounting:
            self.ui.say(f"This call's prompt: {accounting['prompt_utf8_bytes']} UTF-8 bytes; "
                        f"instructions/schema {accounting['instructions_and_application_schema_utf8_bytes']}, "
                        f"work controls {accounting['control_utf8_bytes']}, "
                        f"qualified context {accounting['qualified_context_utf8_bytes']}, "
                        f"section separators {accounting['section_separator_utf8_bytes']}.")
            self.ui.say(f"Assembled bytes including reserves: {accounting['assembled_bytes_with_reserve']}.")
            transport = accounting.get("provider_request_bytes")
            if transport is not None:
                self.ui.say(f"Matching provider request: {transport} transport bytes.")
            usage = accounting.get("provider_reported_usage")
            self.ui.say("Provider-reported usage: " + (json.dumps(usage, ensure_ascii=False) if usage is not None
                                                      else "unavailable for a matching completed provider input"))
            if accounting.get("store_usage_is_unknown"):
                self.ui.say("Provider usage remains uncertain; reported tokens alone do not settle its input/output reservation.")
            navigation = accounting.get("store_reservation", {}).get("known_native_navigation")
            if navigation is not None:
                self.ui.say(f"Known native navigation charge: {navigation['bytes']} bytes. "
                            "Work attempts and navigation allowances were not renewed.")
            self.ui.say(accounting["scope"])

    def work_menu(self, work, after):
        self.ui.say(f"Work {work['id']} — original request: {work['need']}")
        if work.get("task_amendment"):
            self.ui.say("Current amended request: " + work["task_amendment"]["current_scope"]["request"])
        while True:
            self.ui.say("r: results; v: full delivery contents and qualifications; j: delivery receipt JSON; e: export files; q: questions and answers; f: feedback; d: persona development; "
                        "m: amend task; u: resume participant; c: continue; p: pause work; x: cancel work; "
                        "w: resume work; i: confirm pending control; a: Codex auth source; b: back")
            choice = self.ui.ask("Work action:").strip().lower()
            if choice in ("", "b"):
                return
            if choice == "r":
                self.output(work["id"])
            elif choice == "v":
                self.output(work["id"], delivery_details=True)
            elif choice == "j":
                self.output(work["id"], receipt_json=True)
            elif choice == "e":
                destination = self.ui.ask("New directory for delivered and cited files (blank to return):").strip()
                if destination:
                    receipt = self.session("result", work["id"], "--export-directory", destination)
                    if receipt.get("status") == "no_exportable_files":
                        self.ui.say("No delivered package or complete cited file is available to export. "
                                    "Choose Results to read the personas' replies. "
                                    "Files they created directly are in the task workspace shown in the work list.")
                    else:
                        self.ui.say(f"Saved {len(receipt['files'])} files in {receipt['destination']}.")
            elif choice == "q":
                self.questions(work["id"])
            elif choice == "f":
                self.feedback(work)
            elif choice == "m":
                self.amend_work(work)
            elif choice == "d":
                self.development(work)
            elif choice == "u":
                self.resume_participant(work, after)
            elif choice in ("p", "x", "w"):
                self.control_work(work["id"], {"p": "pause", "x": "cancel", "w": "resume"}[choice])
            elif choice == "i":
                if self.pending_work_control() is not None:
                    self.deliver_work_control()
                else:
                    self.ui.say("No work control is waiting for confirmation.")
            elif choice == "a":
                if self.inspect(after)["settings"].get("codex_auth_only_supported"):
                    self.codex_auth_source(change=True)
                else:
                    self.ui.say("This runtime does not use the Codex auth-only route. Its saved provider remains unchanged.")
            elif choice == "c":
                view = self.inspect(after)
                current = next((w for w in view["works"] if w["id"] == work["id"]), None)
                if current is not None and self.eligible(view, current):
                    self.run_work(work["id"], view)
                else:
                    self.ui.say("No participant is currently eligible. Choose Resume participant for a stopped, "
                                "individually paused or definitively rejected participant. "
                                "Choose Resume work to lift a work-wide pause. "
                                "Inspect other holds and unresolved execution using RUNTIME-REFERENCE.md.")
            else:
                self.ui.say("Choose a listed action.")

    @staticmethod
    def eligible(view, work):
        return (not view["frozen"]
                and work["phase"] == "open"
                and any(p["eligible_candidate"] or p.get("continuation_recovery_candidate", False)
                        or p.get("scheduled_for_unix") is not None
                        for p in work["participants"]))

    @staticmethod
    def scheduled_time(at):
        try:
            return time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(at))
        except (OverflowError, OSError, ValueError):
            return f"Unix time {at}"

    @staticmethod
    def continuation_label(value):
        return {"absent": "no continuation chosen", "immediate": "wants to continue",
                "wait": "waiting", "reconsider_at": "scheduled to reconsider",
                "persona_pause": "paused", "persona_stop": "stopped"}.get(value, value)

    def work_availability(self, view, work):
        if view["frozen"]:
            return "runtime paused"
        if work["phase"] == "paused":
            return "work paused; choose Resume work to lift the pause"
        if work["phase"] == "ended":
            return "work ended; review its recorded outcome and results"
        if work["phase"] != "open":
            return "review results"
        if any(p["eligible_candidate"] or p.get("continuation_recovery_candidate", False)
               for p in work["participants"]):
            return "ready to continue"
        scheduled = [p["scheduled_for_unix"] for p in work["participants"]
                     if p.get("scheduled_for_unix") is not None]
        if scheduled:
            return "scheduled to reconsider at " + self.scheduled_time(min(scheduled))
        if any(p.get("active_invocation") and p["active_invocation"]["state"] in
               ("Prepared", "Dispatched", "Responded") for p in work["participants"]):
            return "has pending calls; inspect their outcomes"
        if any(p.get("failed_attempt_hold") or p.get("resume_recovery") for p in work["participants"]):
            return "review failed attempts"
        if work["participants"] and all(p["continuation"] == "persona_stop" for p in work["participants"]):
            return "participants stopped; review their results"
        return "no participant ready to continue"

    def codex_auth_defaults(self):
        previous = self.p["codex_auth"] or {}
        host_home = previous.get("host_home") or os.environ.get("HOME", "")
        return {"executable": previous.get("executable") or shutil.which("codex") or "",
                "host_home": host_home,
                "codex_home": previous.get("codex_home") or os.environ.get("CODEX_HOME")
                              or str(Path(host_home) / ".codex")}

    def codex_auth_source(self, change=False, use_defaults=False):
        if self.p["codex_auth"] is None or change:
            paths = self.codex_auth_defaults()
            if not use_defaults:
                paths["executable"] = self.ui.ask(f"Trusted Codex executable (authentication only) [{paths['executable']}]:").strip() or paths["executable"]
                paths["host_home"] = self.ui.ask(f"Home directory to give that Codex process [{paths['host_home']}]:").strip() or paths["host_home"]
                if self.p["codex_auth"] is None and not os.environ.get("CODEX_HOME"):
                    paths["codex_home"] = str(Path(paths["host_home"]) / ".codex")
                paths["codex_home"] = self.ui.ask(f"Existing Codex state directory [{paths['codex_home']}]:").strip() or paths["codex_home"]
            try:
                require(all(path and "\0" not in path for path in paths.values()), "invalid_codex_auth_paths")
                paths = {name: str(Path(path).resolve(strict=True)) for name, path in paths.items()}
                require(valid_codex_paths(paths), "invalid_codex_auth_paths")
                require(Path(paths["executable"]).is_file() and os.access(paths["executable"], os.X_OK)
                        and Path(paths["host_home"]).is_dir() and Path(paths["codex_home"]).is_dir(),
                        "codex_auth_source_unavailable")
            except (Blocked, OSError, RuntimeError):
                if use_defaults:
                    self.ui.say("The displayed Codex locations are unavailable. Select the existing installation and login to use.")
                    return self.codex_auth_source(change=True)
                raise Blocked("codex_auth_source_unavailable") from None
            self.p["codex_auth"] = paths
            self.j.save()
            self.ui.say("Saved these locations. No authentication request or model call was made.")
        return {"kind": "codex_auth_only", **self.p["codex_auth"], "authorize_codex_auth_only": False}

    def session_binding(self, outcome, session):
        if outcome.get("status") == "not_recorded":
            return
        claim = outcome.get("claim")
        expected = {"id": session["id"], "operator": self.p["operator"],
                    "work": session["work"], "route": session["route"]}
        require(isinstance(claim, dict) and claim.get("request") == expected, "session_binding_changed")
        require(claim.get("launch") == session["expected_launch"], "session_binding_changed")

    def session_output(self, session, shown_messages=None):
        view = self.session("session-messages", "--id", session["id"])
        outcome = view["session"]
        self.session_binding(outcome, session)
        scope = view.get("message_scope")
        if scope is None:
            self.ui.say("A complete call range is unavailable for this session. "
                        "Choose r in the work menu to inspect the permitted work history.")
            return outcome
        require(scope.get("work") == session["work"], "session_binding_changed")
        messages = view["messages"]
        summary = f"Messages from session {session['id']}: {len(messages)} available."
        if shown_messages is not None and messages:
            remaining = sum(message["id"] not in shown_messages for message in messages)
            summary += f" {remaining} not shown yet." if remaining else " All shown above."
        self.ui.say(summary)
        self.show_messages(messages, show_invocation=True, shown_messages=shown_messages)
        if view.get("truncated"):
            self.ui.say("This view contains the most recent whole messages that fit its output limit.")
        self.ui.say("Choose r in the work menu for the full reply history, feedback accounts and qualified deliveries.")
        return outcome

    def show_session_outcome(self, terminal):
        kind = terminal.get("kind")
        require(kind in ("returned", "failed"), "invalid_session_outcome")
        if kind == "returned":
            self.ui.say(f"Session finished: {terminal.get('stop')}; last recorded work call: {terminal.get('last_invocation')}.")
            failure = terminal.get("driver_failure")
            if failure:
                self.ui.say(f"Recorded driver failure during this session: {failure}. "
                            "The work list shows current participant status.")
        else:
            failure = terminal.get("code")
            self.ui.say(f"Session stopped: {failure}. Review the saved state before another run.")
        if failure == "mandatory_context_overflow":
            self.ui.say("Context construction failed with a retained overflow diagnostic. "
                        "Current ordinary assembly has no application byte quota; inspect the exact "
                        "diagnostic and source binding before continuing.")

    def work_snapshot(self, work):
        after = None
        while True:
            snapshot = self.inspect(after)
            current = next((item for item in snapshot["works"] if item["id"] == work), None)
            if current is not None or not snapshot.get("next_after"):
                return current, snapshot
            after = snapshot["next_after"]

    @staticmethod
    def attempt_status(attempt):
        return {"Prepared": "preparing the call", "Dispatched": "waiting for the model",
                "Responded": "applying the response"}.get(attempt["state"], attempt["status"])

    def work_progress(self, work):
        current, snapshot = self.work_snapshot(work)
        participants = [] if current is None else current["participants"]
        active = [f"{participant['label']} ({attempt['id']}, {self.attempt_status(attempt)})"
                  for participant in participants
                  if (attempt := participant.get("active_invocation"))
                  and attempt["state"] in ("Prepared", "Dispatched", "Responded")]
        tool_states = {"Admitted": "queued", "Dispatched": "awaiting outcome",
                       "Succeeded": "publishing result", "Failed": "publishing failure",
                       "Canceled": "publishing cancellation", "Uncertain": "outcome unresolved"}
        tools = [f"{participant['label']} ({command['operation']}, "
                 f"{tool_states.get(command['state'], command['state'])})"
                 for participant in participants
                 for command in participant["pending_host_commands"]]
        held = [participant["label"] for participant in participants
                if participant.get("failed_attempt_hold")]
        recovery = [participant["label"] for participant in participants
                    if participant.get("resume_recovery")
                    and not participant.get("failed_attempt_hold")]
        activity = []
        if active:
            activity.append("Working: " + "; ".join(active))
        if tools:
            activity.append("Pending tool commands: " + "; ".join(tools))
        scheduled = [f"{participant['label']} ({self.scheduled_time(participant['scheduled_for_unix'])})"
                     for participant in participants if participant.get("scheduled_for_unix") is not None]
        if scheduled:
            activity.append("Scheduled returns: " + "; ".join(scheduled))
        status = ". ".join(activity) + "." if activity else "Waiting for the session's next recorded activity or outcome."
        status += f" Recorded call attempts in this runtime: {snapshot['settings']['steps']}."
        if recovery:
            status += (" Call failures retained for recovery: " + ", ".join(recovery)
                       + ". Awaiting the session's recovery outcome.")
        if held:
            status += " Participants with invocation recovery holds: " + ", ".join(held) + "."
        return status

    def live_input(self, work, shown_messages=None):
        # This is the original foreground session. Input commands only inspect
        # or record input/stop controls; none starts or resumes an invocation.
        try:
            choice = self.ui.ask("While running:").strip().lower()
            if choice in ("r", "v"):
                displayed = self.output(work, delivery_details=choice == "v")
                if shown_messages is not None:
                    shown_messages.update(displayed)
            elif choice == "q":
                self.questions(work)
            elif choice == "f":
                current, _ = self.work_snapshot(work)
                require(current is not None, "work_unavailable")
                self.feedback(current)
            elif choice == "i":
                if self.pending_work_control() is not None:
                    self.deliver_work_control()
                elif self.pending_work_input() is not None:
                    self.deliver_work_input()
                else:
                    self.ui.say("No input is waiting for delivery confirmation.")
            elif choice in ("p", "x"):
                self.control_work(work, "pause" if choice == "p" else "cancel")
            elif choice:
                self.ui.say("Use r: results; v: full delivery contents and qualifications; q: questions and answers; f: feedback; p: pause work; x: cancel work; i: confirm pending input/control.")
            else:
                self.ui.say(self.work_progress(work))
        except Halt:
            self.ui.say("Terminal input closed; waiting for the running session's outcome.")
            return False
        except (Blocked, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
            code = error.code if isinstance(error, Blocked) else "live_input_unavailable"
            self.ui.say(f"Input action could not finish: {code}. The session is still running. "
                        "Any unconfirmed input is retained; use i to check that same input.")
        return True

    def pending_feedback_notice(self, result):
        pending = [item for item in (result.get("requester_feedback") or {}).get("items", [])
                   if item.get("status") == "pending"]
        if pending:
            labels = sorted({item.get("label", item["persona"]) for item in pending})
            self.ui.say(f"{len(pending)} feedback item(s) still await a response from "
                        + ", ".join(labels) + ". Choose r in the work menu to inspect them. "
                        "If the participant stopped, use u to resume them before c to continue.")

    def run_work(self, work, view, reuse=False):
        settings = view["settings"]
        if reuse:
            s = self.p["session"]
            require(s is not None and s["work"] == work, "session_binding_changed")
            route = s["route"]
        elif settings.get("mode") == "siwc_oauth":
            self.account(settings)
            route = {"kind": "native_siwc", "vault": self.p["vault"]}
        elif settings.get("codex_auth_only_supported") is True:
            route = self.codex_auth_source()
        else:
            raise Blocked("interactive_provider_route_unsupported")
        if route["kind"] == "native_siwc":
            require(settings.get("mode") == "siwc_oauth" and route["vault"] == self.p["vault"],
                    "session_binding_changed")
            if reuse:
                self.account(settings)
        else:
            require(settings.get("codex_auth_only_supported") is True, "session_binding_changed")
            self.ui.say(f"Auth-only Codex: {route['executable']}; HOME: {route['host_home']}; "
                        f"Codex state: {route['codex_home']}. Saved account: {settings['account_sha256']}.")
            self.ui.say("The native runtime will ask Codex only for authentication and check the returned account. "
                        "Codex may refresh its saved credentials. Persona inference uses direct provider requests; "
                        "the launcher does not import or record tokens.")
        self.ui.say("Saved persona models: " + "; ".join(
            f"{persona}: {selection['model']} / {selection['effort']}"
            for persona, selection in settings["persona_models"].items()))
        self.ui.say("No application call quota or local model timeout. "
                    "The session continues until the personas stop, work is paused, or execution cannot proceed.")
        if not reuse:
            if self.p["session"] is not None:
                self.p.setdefault("session_history", []).append(self.p["session"])
            self.p["session"] = {"id": "session-" + uuid.uuid4().hex, "work": work,
                                 "route": route, "expected_launch": view["launch"]}
            self.j.save()
        s = self.p["session"]
        require(s["work"] == work and s["route"] == route, "session_binding_changed")
        args = ["run", "--id", s["id"], "--work", work]
        self.j.directory.write("session-launch.json", encoded(s["expected_launch"]))
        args += ["--expected-launch", str(self.j.directory.path / "session-launch.json")]
        if route["kind"] == "native_siwc":
            args += ["--vault", route["vault"]]
        else:
            args += ["--codex-executable", route["executable"], "--host-home", route["host_home"],
                     "--codex-home", route["codex_home"]]
            if route["authorize_codex_auth_only"]:
                args += ["--authorize-codex-auth-only"]
        # Work messages can be read while the session is running. Do not label
        # them as a completed session's output or reconstruct a terminal range.
        seen_messages = None
        # A baseline observation is not a display. Keep only replies actually
        # printed in this foreground run out of its automatic completion view.
        shown_messages = set()
        shown_activity = {}
        try:
            seen_messages = {message["id"] for message in self.session("result", work).get("messages", [])}
        except (Blocked, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            pass
        self.ui.say(f"Submitting {s['id']}. Waiting for its recorded native outcome.")
        self.ui.say("Work updates and new replies appear as work continues. Type r for results, v for full delivery contents and qualifications, q for questions and answers, "
                    "f for feedback, p to pause work, x to cancel work, or i to confirm pending input/control, then press Enter.")
        last_update = [None, 0.0]

        def report_progress():
            nonlocal seen_messages
            # Read-only status of this same live helper. A failed observation
            # never cancels, replays or creates another session.
            try:
                status = self.work_progress(work)
            except (Blocked, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
                status = "Live status unavailable; still waiting for the original session's outcome."
            now = time.monotonic()
            if status != last_update[0] or now - last_update[1] >= 60:
                self.ui.say(status)
                last_update[:] = [status, now]
            try:
                result = self.session("result", work)
                self.show_persona_activity(result, shown_activity)
                messages = result.get("messages", [])
                if seen_messages is not None:
                    fresh = [message for message in messages
                             if message["id"] not in seen_messages
                             and message["id"] not in shown_messages]
                    if fresh:
                        self.ui.say(f"New replies for {work}:")
                        self.show_messages(fresh, show_invocation=True, shown_messages=shown_messages)
                if seen_messages is None:
                    seen_messages = set()
                seen_messages.update(message["id"] for message in messages)
            except (Blocked, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
                # Inspection failure cannot terminate or replay the live run.
                pass

        # Host commands have no default duration limit. Wait for the native
        # session's outcome; explicit interruption still stops this helper.
        outcome = self.session(*args, timeout=None, on_wait=report_progress,
                               on_input=lambda: self.live_input(work, shown_messages=shown_messages))
        self.session_binding(outcome, s)
        require(outcome.get("status") == "finished", "session_outcome_unknown_no_replay")
        self.show_session_outcome(outcome["terminal"]["outcome"])
        self.session_output(s, shown_messages=shown_messages)
        result = self.session("result", work)
        self.pending_feedback_notice(result)
        if result.get("optional_activity") or result.get("persona_creations"):
            self.output(work, result=result, shown_messages=shown_messages, shown_activity=shown_activity)
        else:
            # A session can settle before the next live poll observes its final
            # accepted account. Display that exact update at completion too.
            self.show_persona_activity(result, shown_activity)

    def recover_session(self, view):
        s = self.p["session"]
        if s is None:
            return
        try:
            status = self.session_output(s)
            self.ui.say(f"Previous session {s['id']}: {status['status']}")
            if status["status"] == "finished":
                self.show_session_outcome(status["terminal"]["outcome"])
        except Blocked as error:
            # Current runtime inspection already succeeded. A failed historical
            # view supplies no outcome or retry permission for this exact session,
            # but must not hide independently selectable current work.
            self.ui.say(f"Previous session {s['id']} could not be displayed ({error.code}). "
                        "Its saved identity is retained and will not be resubmitted. "
                        "The work list below uses the current runtime state.")
            return
        try:
            self.pending_feedback_notice(self.session("result", s["work"]))
        except (Blocked, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            self.ui.say("Current feedback could not be inspected; use r in the work menu to check it.")
        if status["status"] == "outcome_unrecorded":
            self.ui.say("This session has no retained terminal outcome. Its exact identity remains saved and is not replayed. "
                        "The work menu can select a distinct session for currently eligible participants.")
            return
        if status["status"] == "not_recorded":
            if s["expected_launch"] != view["launch"]:
                self.ui.say("This unstarted session's saved launch has changed, so its exact request cannot run. "
                            "No session was started. Return to the work menu to review a fresh run.")
                return
            self.ui.say("The selected session has no runtime claim. Submitting its same exact saved request.")
            self.run_work(s["work"], view, reuse=True)
        # Finished sessions are never dispatched again. A later explicit menu
        # action may request a distinct session through current eligible routes.

    def submit_pending(self, need):
        if need.get("kind") == "optional_activity":
            result = self.session("optional-activity", "--id", need["id"])
            require(result.get("status") in ("episode_admitted", "already_admitted"),
                    "optional_activity_unavailable")
            need["work"] = result["work"]
            self.j.save()
            self.ui.say(f"Optional activity {need['work']}: the personas choose what to explore or discuss, "
                        "whether to participate and when to stop.")
            return need["work"]
        self.ui.say("Exact pending need:\n" + need["text"])
        if not need["shared"]:
            need["require_acceptance"] = self.ui.ask("Require your explicit acceptance of the eventual exact release? [yes/NO]").strip().lower() == "yes"
            need["shared"] = True
            self.j.save()
        args = ["intake", "--id", need["id"]]
        if need["require_acceptance"]:
            args += ["--require-acceptance"]
        result = self.session(*args, "--", need["text"])
        need["work"] = result["receipt"]["work"]
        self.j.save()
        self.ui.say(f"Retained keyed intake: {need['work']}. No inference was needed for intake.")
        return need["work"]

    def run(self):
        if not self.p["needs"] and self.p["runtime"] is None:
            choice = self.ui.ask("Start with a task [t], optional exploration and conversation [o], or existing work [Enter]:").strip().lower()
            if choice == "o":
                self.capture_optional_activity()
            elif choice == "t":
                self.capture_need()
        if self.p["operator"] is None:
            operator = self.ui.ask("Local operator attribution [local-operator]:").strip() or "local-operator"
            require(identifier(operator), "invalid_operator")
            self.p["operator"] = operator
            self.j.save()
        self.h.install()
        self.choose_runtime()
        root = Path(self.p["runtime"]["path"])
        # Only a new UI-owned root is eligible for atomic fresh bootstrap. An
        # existing unrecognized database is never assumed to be partial setup.
        if self.p["runtime"]["owned"] and self.p["setup_authorized"]:
            # The selected action retains these exact immutable request bytes. Actual
            # setup completion is established only by the runtime-owned receipt.
            path, _ = self.j.document("bootstrap")
            self.session("bootstrap", path)
        elif self.p["runtime"]["owned"]:
            require(not (root / "state.sqlite3").exists(), "unrecognized_existing_runtime")
            self.bootstrap()
        view = self.inspect()
        self.recover_session(view)
        after = None
        while True:
            view = self.inspect(after)
            pending = [n for n in self.p["needs"] if n["work"] is None]
            settings = view["settings"]
            self.ui.say(f"Default model: {settings.get('model') or 'unavailable'} / "
                        f"{settings.get('effort') or 'unavailable'}.")
            credential_status = settings.get("credential_status")
            if credential_status == "saved_expiry_elapsed":
                self.ui.say("The saved sign-in has expired. Renew it and update this runtime's provider settings before continuing work.")
            elif credential_status == "incompatible_saved_provider":
                self.ui.say("The saved provider settings cannot start work. You can still inspect saved results.")
            else:
                self.ui.say("Sign-in is checked when work starts; viewing saved work makes no model call.")
            if view["frozen"]:
                self.ui.say("This runtime is paused. Saved results remain available.")
            if view["active_invocations_present"]:
                self.ui.say("Some model-call outcomes are pending or need recovery. Open the work to inspect its status.")
            if settings.get("working_directory"):
                self.ui.say(f"Task workspace: {settings['working_directory']}")
            for i, work in enumerate(view["works"], 1):
                counts = {}
                for participant in work["participants"]:
                    label = self.continuation_label(participant["continuation"])
                    counts[label] = counts.get(label, 0) + 1
                continuations = ", ".join(f"{count} {label}" for label, count in counts.items())
                self.ui.say(f"{i}: {work['need']}")
                self.ui.say(f"   {work['id']} — {self.work_availability(view, work)}. "
                            f"Last choices: {continuations or 'none recorded'}.")
                attempt = work.get("last_attempt")
                if attempt:
                    self.ui.say(f"   Last recorded call: {attempt['id']} — {attempt['status'].replace('_', ' ')}.")
                self.show_call_failures(work)
                for participant in work["participants"]:
                    lifecycle = participant.get("lifecycle")
                    if lifecycle and (lifecycle["status"] != "active" or lifecycle["quarantined"]):
                        self.ui.say(f"   {participant['persona']}: identity {lifecycle['status']}; "
                                    f"quarantined={lifecycle['quarantined']}. See persona lifecycle controls in RUNTIME-REFERENCE.md.")
                    if participant.get("failed_attempt_hold"):
                        self.ui.say(f"   {participant['persona']}: failed-attempt hold {participant['failed_attempt_hold']}")
            saved_input = self.pending_work_input()
            saved_control = self.pending_work_control()
            if saved_control:
                control = saved_control["request"]
                action = control.get("action", "resume")
                self.ui.say(f"Saved {action} control for {control['work']} awaits confirmation. Choose i to finish that same request.")
            if saved_input:
                request = saved_input["request"]
                if saved_input["kind"] == "resume-participant":
                    self.ui.say(f"Saved explicit resume for {request['persona']} in {request['work']} awaiting submission.")
                    self.ui.say("i: finish this same resume request; an earlier receipt cannot resume anyone again")
                elif saved_input["kind"] == "amend-work":
                    self.ui.say(f"Saved amendment for {request['work']} awaiting submission:\n{request['scope']['request']}")
                    self.ui.say("i: finish this same amendment; its identity and scope will be reused")
                else:
                    content = request if saved_input["kind"] == "feedback" else request["reply"]
                    self.ui.say(f"Saved {saved_input['kind']} for {request['work']} awaiting submission:\n{content['text']}")
                    if "basis" in content:
                        self.ui.say("Basis: " + content["basis"])
                    self.ui.say("i: deliver this same saved input; its identity and text will be reused")
            self.ui.say(f"{len(pending)} pending request(s). p: submit first pending; n: new need; o: optional activity; "
                        "work number: open work actions; more: next saved page; q: leave")
            choice = self.ui.ask("Choose:").strip().lower()
            if choice == "q" or not choice:
                return
            if choice == "n":
                self.capture_need()
            elif choice == "o":
                work_id = self.submit_pending(self.capture_optional_activity())
                after = None
                self.run_work(work_id, self.inspect())
            elif choice == "i" and saved_control:
                self.deliver_work_control()
            elif choice == "i" and saved_input:
                self.deliver_work_input()
            elif choice == "p" and pending:
                work_id = self.submit_pending(pending[0])
                after = None
                self.output(work_id)
                # The explicit run choice follows intake. The runtime still decides
                # admission; the UI cannot wake Stop or override a hold.
                self.run_work(work_id, self.inspect())
            elif choice == "more" and view.get("next_after"):
                after = view["next_after"]
            elif choice.isdecimal() and 1 <= int(choice) <= len(view["works"]):
                work = view["works"][int(choice) - 1]
                self.work_menu(work, after)
            else:
                self.ui.say("Choose a listed entry. No operation was started.")


def progress_candidates():
    """Return deterministic persistent candidates; never choose temporary storage implicitly."""
    explicit = os.environ.get("PERSONAS_LAUNCHER_HOME")
    if explicit is not None:
        require(bool(explicit) and "\x00" not in explicit, "invalid_explicit_state_path")
        explicit = str(Path(explicit).resolve(strict=False))
        require(path_text(explicit), "invalid_explicit_private_path")
        return [explicit]
    candidates = []
    xdg = os.environ.get("XDG_STATE_HOME")
    home = os.environ.get("HOME")
    if xdg and path_text(xdg):
        candidates.append(str(Path(xdg) / "ai-personas-launcher"))
    if home and path_text(home):
        candidates.append(str(Path(home) / ".local" / "state" / "ai-personas-launcher"))
    return list(dict.fromkeys(candidates))


def preflight_progress_creation(path):
    """Side-effect-free advisory check of the nearest existing parent descriptor.

    This checks host write/search access before creation. It does not reserve
    the path or guarantee a later mkdir/write. A filesystem failure never
    silently selects another journal location.
    """
    require(path_text(str(path)), "unsafe_private_path")
    parent = Path(path).resolve(strict=False).parent
    parts = parent.parts[1:]
    try:
        # Resolve platform constants inside the guarded capability boundary:
        # unsupported Python builds fail with a stable launcher error.
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW
        fd = os.open("/", flags)
    except (OSError, AttributeError, TypeError) as error:
        raise Blocked("progress_location_unavailable") from None
    try:
        for part in parts:
            try:
                child = os.open(part, flags, dir_fd=fd)
            except FileNotFoundError:
                break
            except (OSError, AttributeError, TypeError):
                raise Blocked("progress_location_unavailable") from None
            try:
                info = os.fstat(child)
                require(stat.S_ISDIR(info.st_mode), "unsafe_private_path")
            except BaseException:
                os.close(child)
                raise
            os.close(fd)
            fd = child
        try:
            writable = os.access(".", os.W_OK | os.X_OK, dir_fd=fd, effective_ids=True)
        except (OSError, AttributeError, TypeError, NotImplementedError):
            raise Blocked("progress_writability_unverifiable") from None
        require(writable, "progress_location_not_writable")
    finally:
        os.close(fd)


def select_progress_directory(ui, default=None):
    """Automatically select standard persistent state without moving/resetting journals."""
    candidates = progress_candidates()
    require(candidates, "no_supported_persistent_state_location")
    existing, safe_missing = [], []
    for path in candidates:
        try:
            fd = PrivateDirectory.open_path(path, missing_ok=True)
            if fd is None:
                safe_missing.append(path)
            else:
                try:
                    existing.append((path, not os.listdir(fd)))
                finally:
                    os.close(fd)
        except (Blocked, OSError):
            # Inaccessible is not absent. Never fall through to HOME and thereby
            # strand pending identities in an XDG journal we could not inspect.
            raise Blocked("progress_location_unavailable") from None
    require(len(existing) <= 1, "multiple_saved_progress_locations_choose_explicit_override")
    if existing:
        path, empty = existing[0]
        # An existing empty directory has no saved operation to recover. Journal
        # rechecks its contents under the exclusive lock before initializing it.
        # Nonempty directories without progress still cannot be reset here.
        return PrivateDirectory(path), empty
    require(safe_missing, "no_supported_persistent_state_location")
    path = safe_missing[0]
    # Check actual host access before creating the selected journal.
    preflight_progress_creation(path)
    ui.say(f"Selected private persistent progress location: {path}")
    ui.say("The journal stores pending requests and operation identities, not credentials.")
    try:
        return PrivateDirectory(path, create=True), True
    except (Blocked, OSError):
        raise Blocked("progress_location_unavailable") from None


def main():
    ui = Terminal()
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        ui.say("Interactive ./personas needs a terminal. Explicit argument-based commands remain available; see --help.")
        return 2
    if os.name != "posix" or sys.version_info < (3, 9):
        ui.say("This interaction needs Unix and Python 3.9+. No installer was run.")
        return 2
    directory = journal = helpers = None
    try:
        directory, create = select_progress_directory(ui)
        journal = Journal(directory, create=create)
        checkout = Path(__file__).resolve().parents[1]
        helpers = Helpers(journal, checkout, ui)
        Dialogue(journal, helpers, ui, checkout).run()
        return 0
    except (Halt, KeyboardInterrupt):
        ui.say("Stopped. Any durably saved progress and operation identities remain; nothing will retry automatically.")
        return 0
    except Blocked as error:
        ui.say(f"Blocked: {error.code}. Saved progress was not reset. See {DOC}. "
               "Reopening ./personas rechecks retained identities; it is not permission to replay an uncertain request.")
        return 1
    except (OSError, ValueError, KeyError, TypeError, RecursionError, subprocess.SubprocessError):
        ui.say(f"Local operation failed or helper output was invalid. No automatic retry. "
               f"Keep the saved progress; inspect the exact operation status on return. See {DOC}.")
        return 1
    finally:
        if helpers:
            helpers.close()
        if journal:
            journal.close()
        if directory:
            directory.close()


if __name__ == "__main__":
    sys.exit(main())
