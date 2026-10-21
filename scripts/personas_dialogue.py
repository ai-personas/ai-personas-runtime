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
MAX_JOURNAL = 2 * 1024 * 1024
MAX_OUTPUT = 2 * 1024 * 1024
DOC = "docs/interactive-launch.md"


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
    return isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,64}", value) is not None


def path_text(value):
    return (isinstance(value, str) and value.startswith("/") and len(value.encode()) <= 4096
            and not any(ord(c) < 32 or ord(c) == 127 for c in value)
            and all(p not in ("", ".", "..") for p in value.split("/")[1:]))


def safe_text(value, maximum=32768):
    return (isinstance(value, str) and 0 < len(value.encode()) <= maximum
            and bool(value.strip()) and "\x00" not in value)


def terminal_text(value):
    # Provider-authored text cannot emit terminal escape/control sequences.
    return "".join(c if c in "\n\t" or (c.isprintable() and c != "\x1b") else "�" for c in str(value))


def readable_messages(messages, show_invocation=False):
    """Render already-permitted messages, preserving citations and copy counts.

    Group exact copies only within an attributed decision. Keep every distinct
    subject; references identify files without supplying their bodies.
    """
    groups, positions = [], {}
    for message in messages:
        # Retain every supplied scope field, including any future metadata.
        # Empty citations were omitted by historical/native serialization.
        scope = {key: value for key, value in message.items() if key not in ("id", "subject")}
        scope.setdefault("file_references", [])
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
    return parts


def readable_delivery(delivery):
    """Display complete qualified text, omitting only verified duplicate bytes.

    Unknown or inconsistent packages retain the original JSON presentation.
    All metadata, qualifications and attribution remain in the readable view.
    """
    try:
        artifacts = delivery["artifacts"]
        if not isinstance(artifacts, list) or not artifacts:
            return None
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
            if (len(roots) != 1 or not isinstance(roots[0]["text"], str)
                    or roots[0]["text"].encode("utf-8") != bytes(raw)):
                return None
            parts.append("Artifact record (duplicate byte array shown as its qualified text below):")
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
        return parts
    except (Blocked, KeyError, TypeError, ValueError, AttributeError, UnicodeError):
        return None


def inside_checkout(path):
    """Reject state inside this checkout; normalize resolution failures to a safe refusal."""
    try:
        candidate = Path(path).resolve(strict=False)
        checkout = Path(__file__).resolve().parents[1]
        candidate.relative_to(checkout)
        return True
    except ValueError:
        return False
    except (OSError, RuntimeError):
        # Symlink loops and platform-specific resolution failures must never
        # escape the launcher's controlled error path or be treated as absence.
        raise Blocked("unsafe_private_path") from None


class PrivateDirectory:
    """Descriptor-relative private journal, excluding the active checkout.

    The private leaf is owned by the effective UID and inaccessible to group/other.
    Same-UID processes and root remain trusted and can tamper with state; this is
    not an OS sandbox or rollback protection. Missing/invalid progress is never
    silently reset. Shared sticky ancestors may contain unrelated repository
    markers; the active checkout path itself is rejected.
    """
    def __init__(self, path, create=False):
        self.path = Path(path)
        self.fd = self.open_path(path, create=create)

    @staticmethod
    def open_path(path, create=False, missing_ok=False):
        """Check all existing components; missing_ok is a read-only preflight."""
        require(path_text(str(path)), "unsafe_private_path")
        require(not inside_checkout(path), "state_inside_repository")
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
                info = os.fstat(fd)
                # Descriptor-relative traversal anchors each opened directory. Foreign
                # owners are allowed only for non-group/world-writable ancestors; they
                # can still disrupt availability by renaming ancestors, so root/host
                # and same-UID processes remain trusted. The private leaf is checked
                # separately for effective-UID ownership and mode 0700.
                root_sticky = info.st_uid == 0 and bool(info.st_mode & stat.S_ISVTX)
                require(not (info.st_mode & 0o022) or root_sticky, "writable_path_ancestor")
            info = os.fstat(fd)
            require(info.st_uid == os.geteuid() and not info.st_mode & 0o077, "private_directory_permissions")
            return fd
        except BaseException:
            os.close(fd)
            raise

    @staticmethod
    def no_repository(fd):
        for marker in (".git", ".hg", ".svn"):
            try:
                os.stat(marker, dir_fd=fd, follow_symlinks=False)
            except FileNotFoundError:
                continue
            raise Blocked("state_inside_repository")

    @staticmethod
    def file_ok(info, maximum):
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and info.st_nlink == 1 and not info.st_mode & 0o077
                and info.st_size <= maximum, "unsafe_private_file")

    def read(self, name, maximum=MAX_JOURNAL):
        require(re.fullmatch(r"[a-zA-Z0-9_.-]{1,128}", name) is not None)
        try:
            fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=self.fd)
        except FileNotFoundError:
            return None
        with os.fdopen(fd, "rb") as source:
            self.file_ok(os.fstat(source.fileno()), maximum)
            raw = source.read(maximum + 1)
        require(len(raw) <= maximum)
        return raw

    def write(self, name, raw):
        require(len(raw) <= MAX_JOURNAL)
        self.read(name)  # Reject unsafe pre-existing files; never chmod/repair them.
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
            and route["kind"] == "codex_auth_only" and route["authorize_codex_auth_only"] is True
            and valid_codex_paths({k: route[k] for k in ("executable", "host_home", "codex_home")}))


def new_progress():
    return {"version": 1, "operator": None, "installation": None, "runtime": None,
            "vault": None, "needs": [], "session": None, "probes": [],
            "settings": None, "bootstrap": None, "setup_authorized": False,
            "work_input": None, "codex_auth": None}


def validate_progress(p):
    fields = set(new_progress())
    require(isinstance(p, dict) and set(p) == fields and p["version"] == 1)
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
    require(isinstance(p["needs"], list) and len(p["needs"]) <= 32)
    seen = set()
    for n in p["needs"]:
        require(isinstance(n, dict) and set(n) == {"id", "text", "require_acceptance", "shared", "work"})
        require(identifier(n["id"]) and n["id"] not in seen and safe_text(n["text"])
                and type(n["require_acceptance"]) is bool and type(n["shared"]) is bool
                and (n["work"] is None or identifier(n["work"])))
        seen.add(n["id"])
    if p["session"] is not None:
        s = p["session"]
        require(isinstance(s, dict)
                and set(s) == {"id", "work", "route", "expected_launch"}
                and identifier(s["id"]) and identifier(s["work"]))
        require(valid_session_route(s["route"]))
        require(valid_launch_reference(s["expected_launch"]))
    require(isinstance(p["probes"], list) and len(p["probes"]) <= 16)
    seen = set()
    for probe in p["probes"]:
        require(isinstance(probe, dict) and set(probe) == {"request", "view", "attempted"})
        q = probe["request"]
        require(isinstance(q, dict) and set(q) == {"id", "catalogue_sha256", "model", "effort", "timeout_ms",
                                                  "max_observed_total_tokens", "require_remote_token_hard_bound"})
        require(identifier(q["id"]) and q["id"] not in seen and safe_text(q["model"], 128)
                and q["effort"] in ("none", "minimal", "low", "medium", "high", "xhigh", "max")
                and type(q["timeout_ms"]) is int and 1 <= q["timeout_ms"] <= 60000
                and type(q["max_observed_total_tokens"]) is int and 1 <= q["max_observed_total_tokens"] <= 65536
                and q["require_remote_token_hard_bound"] is False and type(probe["attempted"]) is bool
                and re.fullmatch(r"[a-f0-9]{64}", q["catalogue_sha256"]) is not None)
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
            require(set(request) == {"id", "work", "persona", "text"}
                    and identifier(request["id"]) and identifier(request["persona"])
                    and safe_text(request["text"], 16384))
        elif item["kind"] == "answer-question":
            require(set(request) == {"work", "reply"} and isinstance(request["reply"], dict))
            reply = request["reply"]
            require(set(reply) == {"id", "question", "question_revision", "text", "basis"}
                    and identifier(reply["id"]) and safe_text(reply["question"], 256)
                    and re.fullmatch(r"[a-f0-9]{64}", reply["question_revision"]) is not None
                    and safe_text(reply["text"], 8192) and safe_text(reply["basis"], 4096))
        elif item["kind"] == "amend-work":
            require(set(request) == {"id", "operator", "work", "scope", "invitations", "resume",
                                     "expected_runtime", "expected_launch", "authorize_amend"}
                    and identifier(request["id"]) and request["operator"] == p["operator"]
                    and request["authorize_amend"] is True
                    and isinstance(request["scope"], dict)
                    and safe_text(request["scope"].get("request"), 32768)
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
                    and request["operator"] == p["operator"] and request["authorize_resume"] is True)
            for key, name in (("expected_runtime", "runtime-state-v1"),
                              ("expected_launch", "runtime-launch-v1")):
                ref = request[key]
                require(isinstance(ref, dict) and set(ref) == {"id", "revision", "sha256"}
                        and ref["id"] == name and type(ref["revision"]) is int and ref["revision"] > 0
                        and re.fullmatch(r"[a-f0-9]{64}", ref["sha256"]) is not None)
        require(len(encoded(request)) <= (262144 if item["kind"] == "amend-work" else 65536))
    require(len(encoded(p)) <= MAX_JOURNAL)


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
            entries = os.listdir(directory.fd)
            require(len(entries) <= 128 and sum(n.startswith(".write-") for n in entries) <= 8,
                    "private_progress_file_bound")
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
        require(len(raw) <= 65536, "configuration_too_large")
        previous = self.directory.read(name, 65536)
        require(previous is None or previous == raw, "immutable_configuration_changed")
        self.directory.write(name, raw)
        ref = {"file": name, "sha256": hashlib.sha256(raw).hexdigest()}
        self.data[kind] = ref
        self.save()
        return str(self.directory.path / name)

    def document(self, kind):
        ref = self.data[kind]
        require(ref is not None)
        raw = self.directory.read(ref["file"], 65536)
        require(raw is not None and hashlib.sha256(raw).hexdigest() == ref["sha256"], "saved_configuration_changed")
        return str(self.directory.path / ref["file"]), decode(raw)

    def close(self):
        os.close(self.lock)


class Terminal:
    def say(self, text):
        print(terminal_text(text), flush=True)

    def ask(self, text):
        print(terminal_text(text), end=" ", flush=True)
        line = sys.stdin.buffer.readline(32770)
        if not line:
            raise Halt()
        require(len(line) <= 32769 and line.endswith(b"\n"), "input_too_large")
        return line[:-1].decode("utf-8")

    def consent(self, text):
        if self.ask(text + " [yes/NO]").strip().lower() != "yes":
            raise Halt()

    def number(self, text, default, low, high):
        for _ in range(3):
            raw = self.ask(f"{text} [{default}]").strip() or str(default)
            if raw.isdecimal() and low <= int(raw) <= high:
                return int(raw)
            self.say(f"Enter {low}..{high}; no authorization has been granted.")
        raise Halt()


def _linux_open_regular(path):
    """Descriptor-anchor path traversal and return a validated regular leaf FD.

    At approval time, reject intermediate directories currently writable by
    unrelated principals, except root-owned sticky shared directories. This is
    not an immutable-origin guarantee: a foreign owner may later change its
    directory mode or rename descendants. The saved digest and sealed execution
    snapshot enforce the approved main-ELF bytes at launch. Host/root and hostile
    same-UID processes remain trusted; dynamic loader and shared libraries do too.
    """
    require(all(type(getattr(os, name, None)) is int for name in
                ("O_DIRECTORY", "O_CLOEXEC", "O_NOFOLLOW", "O_NONBLOCK")),
            "verified_exec_unavailable")
    p = Path(path)
    require(p.is_absolute() and all(part not in (".", "..") for part in p.parts),
            "unsafe_installation_path")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW
    directory = None
    leaf = None
    try:
        directory = os.open("/", flags)
        for part in p.parts[1:-1]:
            child = None
            try:
                child = os.open(part, flags, dir_fd=directory)
                info = os.fstat(child)
                root_sticky = info.st_uid == 0 and bool(info.st_mode & stat.S_ISVTX)
                foreign_owner_can_rename = info.st_uid not in (0, os.geteuid()) and bool(info.st_mode & 0o200)
                writable_by_group_or_other = bool(info.st_mode & 0o022) and not root_sticky
                if not stat.S_ISDIR(info.st_mode) or foreign_owner_can_rename or writable_by_group_or_other:
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
        require(stat.S_ISREG(info.st_mode) and info.st_uid in (0, os.geteuid())
                and not info.st_mode & 0o022
                and info.st_mode & 0o111 and info.st_size <= 128 * 1024 * 1024,
                "unsafe_installation")
        result, leaf = (leaf, None)
        return result, info
    except OSError:
        raise Blocked("unsafe_installation_path") from None
    finally:
        if leaf is not None:
            os.close(leaf)
        if directory is not None:
            os.close(directory)

def _digest_fd(fd, maximum=128 * 1024 * 1024):
    h = hashlib.sha256()
    total = 0
    os.lseek(fd, 0, os.SEEK_SET)
    while True:
        part = os.read(fd, min(1024 * 1024, maximum - total + 1))
        if not part:
            break
        total += len(part)
        require(total <= maximum, "unsafe_installation")
        h.update(part)
    os.lseek(fd, 0, os.SEEK_SET)
    return h.hexdigest()


def _check_installation_ancestry(path):
    # Compatibility name: Linux checks ancestry through open directory FDs in
    # _linux_open_regular; pathname lstat loops are deliberately not trusted.
    require(Path(path).is_absolute(), "invalid_installation")


def _stable_stat(before, after):
    return (before.st_dev, before.st_ino, before.st_mode, before.st_size,
            before.st_mtime_ns, before.st_ctime_ns) == (
            after.st_dev, after.st_ino, after.st_mode, after.st_size,
            after.st_mtime_ns, after.st_ctime_ns)


def binary_digest(path):
    p = Path(path)
    require(p.is_absolute(), "invalid_installation")
    if sys.platform.startswith("linux"):
        # Descriptor-relative traversal, not a pathname lstat preflight.
        fd, before = _linux_open_regular(str(p))
        try:
            require(before.st_uid in (0, os.geteuid()), "unsafe_installation")
            digest = _digest_fd(fd)
            require(_stable_stat(before, os.fstat(fd)), "installation_changed_during_hash")
            return digest
        except OSError:
            raise Blocked("unsafe_installation") from None
        finally:
            os.close(fd)
    for ancestor in (p, *p.parents):
        info = ancestor.lstat()
        require(not stat.S_ISLNK(info.st_mode) and info.st_uid in (0, os.geteuid()), "unsafe_installation_path")
        require(not info.st_mode & 0o022 or
                (stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and info.st_mode & stat.S_ISVTX),
                "writable_installation_path")
    info = p.stat()
    require(stat.S_ISREG(info.st_mode) and info.st_uid in (0, os.geteuid())
            and not info.st_mode & 0o022 and os.access(p, os.X_OK)
            and info.st_size <= 128 * 1024 * 1024, "unsafe_installation")
    with p.open("rb") as f:
        h = hashlib.sha256()
        while True:
            part = f.read(1024 * 1024)
            if not part:
                break
            h.update(part)
    return h.hexdigest()


class VerifiedExecutable:
    """Linux-only immutable snapshot of a pinned ELF helper.

    Pins the main ELF bytes only. The kernel loader and shared libraries remain
    trusted host dependencies. Same-UID hostile processes and root are outside
    this snapshot's threat boundary.
    """
    MAX_BYTES = 128 * 1024 * 1024

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
        source_fd, before = _linux_open_regular(path)
        self.fd = None
        try:
            require(before.st_uid in (0, os.geteuid()), "unsafe_installation")
            require(before.st_mode & 0o111 and not before.st_mode & 0o022, "unsafe_installation")
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
                require(total <= self.MAX_BYTES, "unsafe_installation")
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


MAX_TOOLCHAIN_CANDIDATES = 8
MAX_TOOLCHAIN_DIRECTORY_ENTRIES = 256
MAX_TOOLCHAIN_PROBE_SECONDS = 12.0
MAX_RUSTC_VERSION_BYTES = 4096


def discover_cargo():
    """Discover bounded candidates without executing them or scanning unbounded roots.

    PATH Cargo keeps precedence. Off-PATH discovery is limited to configured
    RUSTUP_HOME, or HOME/.rustup when RUSTUP_HOME is unset. A toolchain root with
    more than MAX_TOOLCHAIN_DIRECTORY_ENTRIES entries fails closed rather than
    selecting from a partial, enumeration-order-dependent view.
    """
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
                    if len(names) > MAX_TOOLCHAIN_DIRECTORY_ENTRIES:
                        raise Blocked("rustup_toolchain_directory_limit")
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
        if len(candidates) >= MAX_TOOLCHAIN_CANDIDATES:
            break
    return candidates[:MAX_TOOLCHAIN_CANDIDATES]


def _parse_rustc_version(output):
    if not isinstance(output, str) or len(output.encode("utf-8", "replace")) > MAX_RUSTC_VERSION_BYTES:
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
        proc.wait(timeout=0.2)
    except (OSError, subprocess.SubprocessError):
        pass


def _supported_rustc(rustc, env, deadline, cwd):
    """Probe one compiler with a shared deadline, nonblocking pipe and byte cap."""
    if deadline - time.monotonic() <= 0:
        return False
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
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                events = poll.select(min(0.1, remaining))
                if not events:
                    continue
                try:
                    part = os.read(fd, min(8192, MAX_RUSTC_VERSION_BYTES + 1 - len(output)))
                except BlockingIOError:
                    continue
                if not part:
                    poll.unregister(proc.stdout)
                    continue
                output.extend(part)
                if len(output) > MAX_RUSTC_VERSION_BYTES:
                    return False
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return False
        try:
            code = proc.wait(timeout=remaining)
        except subprocess.TimeoutExpired:
            return False
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
    """Select a candidate within shared candidate/time bounds using the build env."""
    deadline = time.monotonic() + MAX_TOOLCHAIN_PROBE_SECONDS
    for cargo, toolchain_bin in candidates[:MAX_TOOLCHAIN_CANDIDATES]:
        if time.monotonic() >= deadline:
            break
        env = dict(base_env)
        if toolchain_bin:
            env["PATH"] = toolchain_bin + os.pathsep + env.get("PATH", "")
            rustc = str(Path(toolchain_bin) / "rustc")
        else:
            rustc = shutil.which("rustc", path=env.get("PATH"))
            if not rustc:
                continue
        if _supported_rustc(rustc, env, deadline, cwd):
            return cargo, toolchain_bin, env
    return None


class Helpers:
    def __init__(self, journal, checkout, ui):
        self.j = journal
        self.checkout = checkout
        self.ui = ui

    def candidates(self):
        paths = []
        explicit = os.environ.get("PERSONAS_BIN")
        if explicit is not None:
            require(path_text(explicit), "invalid_explicit_installation")
            paths.append(Path(explicit).parent)
        installed = shutil.which("ai-personas")
        if installed:
            paths.append(Path(installed).parent)
        paths += [self.checkout / "target/release", self.checkout / "target/debug"]
        found = []
        for folder in paths[:4]:
            if folder not in found and all((folder / n).is_file() for n in NAMES):
                found.append(folder)
        return found

    def adopt(self, folder):
        mapping = {}
        selected_folder = Path(folder).resolve(strict=True)
        for name in NAMES:
            # Resolve the chosen directory, not each executable leaf: O_NOFOLLOW
            # must see and reject a symlink at the selected helper name.
            path = selected_folder / name
            mapping[name] = {"path": str(path), "sha256": binary_digest(path)}
        digest_summary = "; ".join(f"{name}={item['sha256']}" for name, item in mapping.items())
        self.ui.consent(f"Approve these exact helper bytes from {selected_folder}? SHA-256: {digest_summary}")
        self.j.data["installation"] = mapping
        self.j.save()

    def install(self):
        explicit = os.environ.get("PERSONAS_BIN")
        if explicit is not None:
            require(path_text(explicit), "invalid_explicit_installation")
            selected = Path(explicit).parent.resolve(strict=True)
            installed = self.j.data["installation"]
            if installed is not None and Path(installed["ai-personas"]["path"]).parent != selected:
                # An explicit installation choice also applies to a saved
                # journal. Keep its runtime and pending operation identities.
                self.ui.say(f"PERSONAS_BIN selects native helpers in {selected}.")
                self.adopt(selected)
                return
        if self.j.data["installation"] is not None:
            try:
                for item in self.j.data["installation"].values():
                    require(binary_digest(item["path"]) == item["sha256"], "approved_binary_changed")
                return
            except (Blocked, OSError):
                self.ui.say("An approved helper is missing or changed. It will not execute under the old approval.")
                self.ui.consent("Choose/build and explicitly approve another installation here? "
                                "Your runtime, configuration, pending keys and budgets will remain unchanged")
                self.j.data["installation"] = None
                self.j.save()
        candidates = self.candidates()
        for i, folder in enumerate(candidates, 1):
            self.ui.say(f"{i}: existing helpers in {folder}")
        self.ui.say("b: build this checkout with Cargo; p: select an existing helper directory; q: leave")
        choice = self.ui.ask("Installation choice:").strip().lower()
        if choice.isdecimal() and 1 <= int(choice) <= len(candidates):
            self.adopt(candidates[int(choice) - 1])
        elif choice == "p":
            folder = self.ui.ask("Absolute directory containing all four current helpers:").strip()
            require(path_text(folder), "invalid_installation")
            self.adopt(Path(folder))
        elif choice == "b":
            try:
                candidates = discover_cargo()
            except Blocked as error:
                if error.code != "rustup_toolchain_directory_limit":
                    raise
                self.ui.say("The configured Rustup toolchain root has more than 256 entries; discovery refused a partial scan. "
                            "Set RUSTUP_HOME to the intended root or remove stale toolchain entries under host policy, "
                            "then retry. No files were changed.")
                raise Halt()
            if not candidates:
                self.ui.say("No Cargo candidate was found on PATH or in the configured Rustup toolchain root. "
                            "Install or repair Rust 1.88+ and the native C toolchain/linker under host policy, "
                            "or choose an approved existing helper directory. No installer was run.")
                return self.install()
            self.ui.consent("Build optimized helpers from this exact checkout with locked Cargo dependencies, one job and no debug symbols? "
                            "Cargo may download locked crates and execute their build scripts. No sudo/system installation. "
                            "Build/cache files consume disk; failure preserves your need")
            target = self.j.directory.path / "build"
            env = self.environment()
            for k in ("CARGO_HOME", "RUSTUP_HOME"):
                if k in os.environ:
                    env[k] = os.environ[k]
            env.update(CARGO_PROFILE_RELEASE_DEBUG="0", CARGO_INCREMENTAL="0",
                       CARGO_TARGET_DIR=str(target))
            selected = select_supported_cargo(candidates, env, self.checkout)
            if selected is None:
                self.ui.say("No discovered candidate reported Rust 1.88+ within the bounded compiler-probe budget. "
                            "Repair the configured toolchain or choose an approved existing helper directory. "
                            "No installer or provider operation was run.")
                return self.install()
            cargo, toolchain_bin, env = selected
            self.ui.say("Building local helpers; Cargo progress follows. Interrupting never starts provider work.")
            # Child-only (Python 3.9+): keep new build paths private even when
            # the caller uses umask 0002. Never chmod an existing installation.
            proc = subprocess.Popen([cargo, "build", "--locked", "--release", "--bins", "-j", "1"],
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

    def call(self, name, args, timeout=30, auth_display=False):
        item = self.j.data["installation"][name]
        verified = None
        executable = item["path"]
        pass_fds = ()
        if sys.platform.startswith("linux"):
            verified = VerifiedExecutable(item["path"], item["sha256"])
            executable = verified.path
            pass_fds = (verified.fd,)
        else:
            require(binary_digest(item["path"]) == item["sha256"], "approved_binary_changed")
        try:
            proc = subprocess.Popen([executable, *args], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=None if auth_display else subprocess.PIPE,
                                    cwd=self.checkout, env=self.environment(), start_new_session=True,
                                    pass_fds=pass_fds)
        except BaseException:
            if verified is not None:
                verified.close()
            raise
        if verified is not None:
            verified.close()
        out, err = bytearray(), bytearray()
        try:
            with selectors.DefaultSelector() as poll:
                poll.register(proc.stdout, selectors.EVENT_READ, out)
                if proc.stderr:
                    poll.register(proc.stderr, selectors.EVENT_READ, err)
                deadline = None if timeout is None else time.monotonic() + timeout
                while poll.get_map():
                    if deadline is not None:
                        require(time.monotonic() < deadline, "helper_outcome_unknown")
                    interval = 0.2 if deadline is None else min(0.2, max(0, deadline - time.monotonic()))
                    for key, _ in poll.select(interval):
                        part = os.read(key.fd, 8192)
                        if not part:
                            poll.unregister(key.fileobj)
                        else:
                            key.data.extend(part)
                            require(len(key.data) <= (MAX_OUTPUT if key.data is out else 262144),
                                    "helper_output_bound_unknown")
                code = proc.wait(timeout=None if deadline is None else max(0.1, deadline - time.monotonic()))
            if code != 0:
                try:
                    # The session CLI may precede its static JSON error with
                    # the Codex auth-only refresh notice. Never echo that stream.
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

    def session(self, *args, timeout=30):
        return self.h.call("personas-session", ["--state-root", self.p["runtime"]["path"],
                            "--operator", self.p["operator"], *args], timeout=timeout)

    def provider(self, *args, timeout=75):
        return self.h.call("personas-provider", ["--vault", self.p["vault"], *args], timeout=timeout)

    def capture_need(self):
        require(len(self.p["needs"]) < 32, "pending_request_limit")
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
            require(path_text(path), "unsafe_private_path")
            self.ui.consent("Inspect this selected saved runtime as the named operator? "
                            "Inspection includes original needs, permissions and metered status; it grants no resume")
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
            self.ui.consent("Use this private location for explicit native SIWC sign-in and credential refresh? "
                            "The Rust auth helper validates ownership, permissions, account binding and expiry. "
                            "No Codex credentials are imported and no inference is authorized by sign-in.")
            self.p["vault"] = path
            self.j.save()
        for _ in range(3):
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
            self.ui.consent(f"Authorize {command} for this exact vault and private credential storage? "
                            "Sign-in uses your browser on this host. Renewal never changes saved runtime pins")
            # The auth helper alone displays its generated URL on stderr, never in
            # a journal or returned UI log. Never retry exchange/refresh on error.
            self.h.call("personas-auth", ["--vault", self.p["vault"], command, "--authorize"],
                        timeout=240, auth_display=True)
        raise Blocked("access_unavailable")

    def new_probe(self):
        require(len(self.p["probes"]) < 16, "probe_identity_limit")
        self.ui.consent("Read this selected account's live model catalogue and retain bounded private catalogue evidence? "
                        "This GET performs no inference")
        catalog = self.provider("catalogue", "--authorize-read", "--retain-private-evidence")
        models = catalog.get("models")
        require(isinstance(models, list) and 1 <= len(models) <= 128, "no_selectable_models")
        for i, model in enumerate(models, 1):
            self.ui.say(f"{i}: {model['display_name']} ({model['slug']})")
        raw = self.ui.ask("Choose the exact model number (no default):").strip()
        require(raw.isdecimal() and 1 <= int(raw) <= len(models), "model_selection_required")
        model = models[int(raw) - 1]["slug"]
        efforts = catalog.get("effort_candidates_unverified", [])
        self.ui.say("Unverified effort candidates, NOT advertised support: " + ", ".join(efforts))
        effort = self.ui.ask("Choose an exact effort to verify (no default):").strip()
        require(effort in efforts, "effort_selection_required")
        timeout = self.ui.number("Probe local deadline in milliseconds", 60000, 1, 60000)
        tokens = self.ui.number("Probe observed-total-token PASS threshold (NOT a remote cap)", 4096, 1, 65536)
        request = {"id": "probe-" + uuid.uuid4().hex, "catalogue_sha256": catalog["catalogue_sha256"],
                   "model": model, "effort": effort, "timeout_ms": timeout,
                   "max_observed_total_tokens": tokens, "require_remote_token_hard_bound": False}
        probe = {"request": request, "view": None, "attempted": False}
        self.p["probes"].append(probe)
        self.j.save()
        return probe

    def settings(self):
        if self.p["settings"] is not None:
            path, settings = self.j.document("settings")
            # Reuse durable generated settings beyond the catalogue TTL, but never
            # bypass current credential-generation/account/expiry diagnostics.
            renewal = self.provider("renewal-status", "--saved-config", path)
            require(not renewal["explicit_provider_migration_required"], "explicit_reverification_required")
            return settings
        if self.p["probes"]:
            # A settings file can be durable even if the following progress write
            # was interrupted. Reattach only those exact bytes after the backend
            # confirms their current account/generation/proof; never invent them
            # or repeat the paid probe just because its UI pointer was lost.
            key = self.p["probes"][-1]["request"]["id"]
            name = "settings-" + key + ".json"
            raw = self.j.directory.read(name, 65536)
            if raw is not None:
                saved = decode(raw)
                path = str(self.j.directory.path / name)
                renewal = self.provider("renewal-status", "--saved-config", path)
                require(not renewal["explicit_provider_migration_required"], "explicit_reverification_required")
                self.j.keep("settings", saved, key)
                return saved
        probe = self.p["probes"][-1] if self.p["probes"] else self.new_probe()
        while True:
            q = probe["request"]
            if probe["view"] is None:
                args = ["plan", "--id", q["id"], "--catalogue-sha256", q["catalogue_sha256"],
                        "--model", q["model"], "--effort", q["effort"], "--timeout-ms", str(q["timeout_ms"]),
                        "--max-observed-total-tokens", str(q["max_observed_total_tokens"]), "--retain-private-evidence"]
                try:
                    probe["view"] = self.provider(*args)
                    self.j.save()
                except Blocked as error:
                    if error.code != "catalogue_stale":
                        raise
                    self.ui.say("The unexecuted catalogue/plan expired; no request was dispatched by this UI.")
                    self.ui.consent("Start a NEW explicit catalogue selection and verification plan? Old identity remains retained")
                    probe = self.new_probe()
                    continue
            v = probe["view"]
            args = ["--id", q["id"], "--plan-sha256", v["plan_sha256"]]
            status = self.provider("probe-status", *args)
            phase = status.get("phase")
            if phase == "planned":
                self.ui.say(f"One capability check: model={q['model']}, effort={q['effort']}, "
                            f"account={v['plan']['account_sha256']}; local deadline={q['timeout_ms']}ms; "
                            f"request<={v['maximum_request_bytes']} bytes, response<={v['maximum_response_bytes']} bytes, "
                            f"stream<={v['maximum_stream_bytes']} bytes. Observed token threshold={q['max_observed_total_tokens']}.")
                self.ui.say(v["cost_notice"])
                self.ui.consent("Authorize exactly this ONE potentially charged probe, private completed evidence retention, "
                                "and UNCAPPED remote token/dollar exposure? A hard remote cap is unsupported; declining saves your need")
                probe["attempted"] = True
                self.j.save()
                status = self.provider("probe", *args, "--authorize-one-request", "--retain-private-evidence",
                                       "--acknowledge-uncapped-remote-usage", timeout=q["timeout_ms"] / 1000 + 15)
                phase = status.get("phase")
            if phase == "completed_verified":
                try:
                    # Do this IMMEDIATELY, before any further question: the proof
                    # to configuration binding has a five-minute catalogue TTL.
                    settings = self.provider("configuration", *args)
                    self.j.keep("settings", settings, q["id"])
                    self.ui.say("Strict-carrier/effort proof was verified and generated settings were saved privately. "
                                "No persona work or runtime sharing has been authorized yet.")
                    return settings
                except Blocked as error:
                    if error.code not in ("catalogue_stale", "credential_changed"):
                        raise
                    self.ui.say("Completed proof cannot be bound to configuration now. No replacement probe was started.")
            elif phase in ("outcome_unrecorded", "failed_or_uncertain") or status.get("remote_usage_may_remain"):
                self.ui.say("Probe outcome/usage is unresolved. This identity is retained and never replayed. "
                            "Fresh probe IDs are not a recovery mechanism. See docs/provider-onboarding.md.")
                raise Blocked("probe_outcome_unknown_no_replay")
            else:
                self.ui.say(f"Capability remains unestablished: {phase}, {status.get('code')}. "
                            "No model/effort substitution or fabricated evidence is permitted.")
            self.ui.consent("Authorize planning a distinct NEW verification after this recorded terminal outcome? "
                            "This does not yet authorize another probe; any backend uncertainty block still controls")
            probe = self.new_probe()

    def bootstrap(self):
        self.account()
        if self.p["bootstrap"] is None:
            settings = self.settings()
            cohort = self.ui.number("Number of continuing personas", 2, 1, 8)
            noun = "persona" if cohort == 1 else "personas"
            self.ui.say(f"Proposed local cohort: {cohort} continuing {noun} and you, without assigned owner/reviewer roles. "
                        "Grants permit their own fragments, assets, character proposals, work commands and cohort messages. "
                        "Host command access is enabled by default: personas can install and use tools with this host account and environment.")
            attempts = self.ui.number("Lifetime attempt allowance for each new work", 16, 1, 128)
            total = self.ui.number("Saved launch lifetime foreground-step ceiling (never auto-refilled)", 64, 1, 256)
            steps = self.ui.number("Maximum foreground attempts per explicitly authorized session", 4, 1, min(128, total))
            works = self.ui.number("Maximum saved work identities", 8, 1, 128)
            request = {"id": "bootstrap-" + uuid.uuid4().hex, "operator": self.p["operator"],
                       "allow_direct_provider": True, "setup": {"cohort": cohort, "owner": 1,
                       "destination": "openai-siwc", "provider": settings, "working_directory": str(self.checkout),
                       "share_cohort": True, "max_steps": steps, "max_considerations": 128,
                       "max_work_attempts": attempts, "max_total_steps": total, "max_works": works}}
            self.j.keep("bootstrap", request, request["id"])
        path, request = self.j.document("bootstrap")
        preview = self.session("bootstrap", path, "--preview")
        setup = request["setup"]
        noun = "persona" if setup["cohort"] == 1 else "personas"
        self.ui.say(f"Runtime: {self.p['runtime']['path']}\n"
                    f"Operator: {self.p['operator']}; exact model/effort: {setup['provider']['model']} / {setup['provider']['effort']}\n"
                    f"Share needs, authorized persona context and outputs with OpenAI SIWC, {setup['cohort']} {noun} and this requester. "
                    f"Cohort self-authorship is permitted, not forced. Work/session/lifetime caps: "
                    f"{setup['max_work_attempts']} / {setup['max_steps']} / {setup['max_total_steps']}; works: {setup['max_works']}.\n"
                    "All local storage/navigation/repair/closeout limits below are controlling finite allocations, "
                    "not a hard filesystem, remote-token or dollar ceiling:")
        config, storage = preview["runtime_config"], preview["storage_limits"]
        for label, values in (("Controlling Store total", storage["total"]),
                              ("Protected closeout within total", storage["protected_closeout"]),
                              ("Navigation per call", config["navigation_per_call"]),
                              ("Navigation per work", config["navigation_per_work"])):
            self.ui.say(label + ": " + json.dumps(values, sort_keys=True))
        self.ui.say(f"Context/response bytes: {config['context_bytes']}/{config['response_bytes']}; "
                    f"repair allowance within work attempts: {config['max_repair_attempts']}; "
                    f"Store identities/history: {storage['max_identity_records']}/{storage['max_history_events']}. "
                    "These logical limits may exceed free disk; they reserve no physical disk or remote tokens.")
        if self.ui.ask("Show the full exact resource, corpus and policy contract? [yes/NO]").strip().lower() == "yes":
            self.ui.say(json.dumps(preview, ensure_ascii=False, indent=2))
        self.ui.consent("Persist these exact local permissions, identities and resource limits? "
                        "No inference now. Existing initialized Stores will NOT be overwritten")
        self.p["setup_authorized"] = True
        self.j.save()
        self.session("bootstrap", path, "--authorize-setup")
        # No UI 'setup complete' flag: every return/restart consults Runtime.

    def inspect(self, after=None):
        args = ["inspect", "--limit", "8"]
        if after:
            args += ["--after", after]
        return self.session(*args)

    def output(self, work, receipt_json=False):
        result = self.session("result", work)
        amendment = result.get("recorded_task_amendment")
        if amendment:
            self.ui.say(f"Saved amended request (mandate {amendment['mandate_revision']}): "
                        + amendment["saved_scope"]["request"])
            self.ui.say("Earlier replies and deliveries may predate this amendment.")
        if result.get("optional_activity_ending"):
            self.ui.say(f"Work {work}: the optional activity ended as recorded. "
                        "No artifact or later benefit was required or established.")
        else:
            self.ui.say(f"Work {work}: {result.get('status', 'unavailable')}. "
                        "Local delivery/attributed completion is not independent quality or human acceptance.")
        messages = result.get("messages", [])
        deliveries = result.get("local_deliveries", [])
        feedback = (result.get("requester_feedback") or {}).get("items", [])
        if feedback:
            pending = sum(item.get("status") == "pending" for item in feedback)
            self.ui.say(f"Visible requester feedback: {pending} pending, {len(feedback) - pending} with a recorded or unavailable disposition. "
                        "A persona's account does not establish that a requested artifact change was made.")
            for item in feedback:
                status = {"pending": "has not yet accounted for this feedback",
                          "handled": "marked this feedback handled",
                          "declined": "declined this feedback"}.get(item.get("status"), "has no currently available feedback disposition")
                self.ui.say(f"{item['label']} {status}.")
                if item.get("feedback_body_supplied"):
                    self.ui.say("Your feedback: " + item["feedback_text"])
                if item.get("account"):
                    self.ui.say(item["label"] + "'s account: " + item["account"]["reason"])
        for part in readable_messages(messages):
            self.ui.say(part)
        for delivery in deliveries:
            self.ui.say("Permission-projected local delivery (including required qualifications):")
            parts = None if receipt_json else readable_delivery(delivery)
            if parts is None:
                self.ui.say(json.dumps(delivery, ensure_ascii=False, indent=2))
            else:
                for part in parts:
                    self.ui.say(part)
        if result.get("truncated"):
            self.ui.say("This permission-projected result is truncated at its existing bound.")
        if result.get("human_decision_pointers"):
            self.ui.say("Explicit acceptance is separate; no acceptance was fabricated. See docs/activation.md and RUNTIME-REFERENCE.md.")

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
                self.ui.say("Choose Continue to start eligible work using the remaining resources.")
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
                    and type(receipt.get("resume_recorded")) is bool
                    and receipt.get("snapshot_changed") is (not receipt["resume_recorded"]),
                    "resume_receipt_mismatch")
            if receipt["resume_recorded"]:
                recorded = receipt.get("receipt")
                require(isinstance(recorded, dict) and recorded.get("request") == request,
                        "resume_receipt_mismatch")
            else:
                require(receipt.get("persona_resumed") is False, "resume_receipt_mismatch")
            item["delivered"] = True
            self.j.save()
            if receipt["resume_recorded"]:
                self.ui.say("The explicit resume is recorded. " +
                            ("This is the earlier receipt; it did not resume the participant again. "
                             if receipt.get("replayed") else "") +
                            "Choose Continue to review current eligibility and start a model session.")
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
        if receipt.get("existing_waits_satisfied", 0):
            self.ui.say("It satisfied an existing requester-input wait. Choose Continue when you want work to proceed.")
        else:
            self.ui.say("Participants keep their existing continuation choices. Stopped or paused participants need an explicit resume.")

    def queue_work_input(self, kind, request):
        require(self.pending_work_input() is None, "pending_work_input_requires_delivery")
        self.p["work_input"] = {"kind": kind, "request": request, "delivered": False}
        self.j.save()
        self.deliver_work_input()

    def choose_participant(self, work):
        participants = work["participants"]
        for i, participant in enumerate(participants, 1):
            self.ui.say(f"{i}: {participant.get('label', participant['persona'])} "
                        f"({participant['persona']}) — {participant['continuation']}")
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
                        "work-wide pauses, holds and exhausted resources need their separate controls.")
            return
        persona = self.choose_participant({"participants": candidates})
        if persona is None:
            return
        self.ui.consent(f"Resume {persona} for {work['id']}? Starting a model session remains a separate choice")
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
        require(safe_text(text, 32768), "amendment_too_large")
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
        require(safe_text(text, 16384), "feedback_too_large")
        self.queue_work_input("feedback", {"id": "feedback-" + uuid.uuid4().hex,
            "work": work["id"], "persona": persona, "text": text})

    def questions(self, work):
        result = self.session("questions", "--", work)
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
                        f"Owner: {item['owner']}; addressed to: {question['addressee']}")
            self.ui.say("Why it matters: " + question["why"])
            self.ui.say("Affected claims: " + "; ".join(question["affected_claims"]))
            self.ui.say("Evidence needed: " + question["evidence_needed"])
            if question.get("human_input_required"):
                self.ui.say("Input needed from the person concerned: " + question["human_input_required"])
            if question.get("provisional_interpretation"):
                self.ui.say("Provisional interpretation: " + question["provisional_interpretation"])
            if item.get("follow_through") == "ownership_gap_original_owner_has_no_current_acceptance":
                self.ui.say("The original owner no longer has current accepted participation; follow-through has an ownership gap.")
            for reply in item["replies"]:
                if "input" not in reply:
                    self.ui.say("A reply is currently unavailable.")
                    continue
                self.ui.say(f"Reply {reply['input']['id']} from {reply['attribution']['author']}:\n"
                            f"{reply['input']['text']}\nAttributed basis: {reply['input']['basis']}")
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
        text = self.ui.ask("Your answer to store for the permitted work participants (Enter to cancel):")
        if not text.strip():
            return
        require(safe_text(text, 8192), "answer_too_large")
        basis = self.ui.ask("Basis for your answer, such as your preference or an observation (Enter to cancel):")
        if not basis.strip():
            return
        require(safe_text(basis, 4096), "answer_basis_too_large")
        self.queue_work_input("answer-question", {"work": work, "reply": {
            "id": "answer-" + uuid.uuid4().hex, "question": selected["question"]["id"],
            "question_revision": selected["question_revision"], "text": text, "basis": basis}})

    def development(self, work):
        persona = self.choose_participant(work)
        if persona is None:
            return
        source = self.ui.ask("Exact source ID (Enter to show up to eight retained revisions):").strip()
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
        character = result.get("character")
        if character:
            self.ui.say(f"Current character revision: {character['revision']}")
            for scope in ("ocean", "vad"):
                for name, value in character[scope].items():
                    self.ui.say(f"{name}: {value}")
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
                self.ui.say(f"Supplied in full: {ref['source']} @ {ref['revision']}; "
                            "route: " + json.dumps(revision["supply_routes"], ensure_ascii=False))
            if not supplied:
                self.ui.say("None of the characteristic revisions in this view was supplied in full to this call.")
            self.development_decision(evidence)
            self.resource_use(evidence)
        if result.get("truncated"):
            self.ui.say("This bounded view omits additional revisions. Use an exact source ID to narrow it.")
        self.ui.say("Retained guidance and actual input supply do not by themselves establish improved behavior. Review the work and its consequences.")

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
                self.ui.say("Store usage accounting remains uncertain; reported provider tokens do not refund its reservation.")
            self.ui.say(accounting["scope"])

    def work_menu(self, work, after):
        self.ui.say(f"Work {work['id']} — original request: {work['need']}")
        if work.get("task_amendment"):
            self.ui.say("Current amended request: " + work["task_amendment"]["current_scope"]["request"])
        for _ in range(16):
            self.ui.say("r: results; j: delivery receipt JSON; q: questions and answers; f: feedback; d: persona development; "
                        "m: amend task; u: resume participant; c: continue; a: Codex auth source; b: back")
            choice = self.ui.ask("Work action:").strip().lower()
            if choice in ("", "b"):
                return
            if choice == "r":
                self.output(work["id"])
            elif choice == "j":
                self.output(work["id"], receipt_json=True)
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
                    self.ui.say("No participant is currently eligible. Choose Resume participant for a stopped "
                                "or individually paused participant. Other holds and limits use the controls in RUNTIME-REFERENCE.md.")
            else:
                self.ui.say("Choose a listed action.")
        self.ui.say("Returning to the work list; progress remains saved.")

    @staticmethod
    def eligible(view, work):
        return (not view["frozen"]
                and work["phase"] == "open" and work["attempts_left"] > 0
                and any(p["eligible_candidate"] for p in work["participants"]))

    def codex_auth_source(self, change=False):
        if self.p["codex_auth"] is None or change:
            previous = self.p["codex_auth"] or {}
            default = previous.get("executable") or shutil.which("codex") or ""
            executable = self.ui.ask(f"Trusted Codex executable (authentication only) [{default}]:").strip() or default
            default_home = previous.get("host_home") or os.environ.get("HOME", "")
            host_home = self.ui.ask(f"Home directory to give that Codex process [{default_home}]:").strip() or default_home
            default_state = previous.get("codex_home") or os.environ.get("CODEX_HOME") or str(Path(host_home) / ".codex")
            codex_home = self.ui.ask(f"Existing Codex state directory [{default_state}]:").strip() or default_state
            paths = {"executable": executable, "host_home": host_home, "codex_home": codex_home}
            require(valid_codex_paths(paths), "invalid_codex_auth_paths")
            require(Path(executable).is_file() and os.access(executable, os.X_OK)
                    and Path(host_home).is_dir() and Path(codex_home).is_dir(), "codex_auth_source_unavailable")
            self.p["codex_auth"] = paths
            self.j.save()
            self.ui.say("Saved these locations. No authentication request or model call was made.")
        return {"kind": "codex_auth_only", **self.p["codex_auth"], "authorize_codex_auth_only": True}

    def session_binding(self, outcome, session):
        if outcome.get("status") == "not_recorded":
            return
        claim = outcome.get("claim")
        expected = {"id": session["id"], "operator": self.p["operator"],
                    "work": session["work"], "route": session["route"]}
        require(isinstance(claim, dict) and claim.get("request") == expected, "session_binding_changed")
        require(claim.get("launch") == session["expected_launch"], "session_binding_changed")

    def session_output(self, session):
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
        self.ui.say(f"Messages from session {session['id']}: {len(messages)} available.")
        for part in readable_messages(messages, show_invocation=True):
            self.ui.say(part)
        if view.get("truncated"):
            self.ui.say("This view contains the most recent whole messages that fit its output limit.")
        self.ui.say("Choose r in the work menu for the full reply history, feedback accounts and qualified deliveries.")
        return outcome

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
        self.ui.say(f"Exact saved model/effort: {settings['model']} / {settings['effort']}; "
                    f"at most {settings['max_steps']} attempts, within remaining work/runtime limits. "
                    "OAuth has NO hard remote token/dollar cap; local time/bytes do not guarantee remote cancellation.")
        self.ui.consent("Authorize this ONE bounded native foreground session and sending its currently permitted "
                        "context to this exact saved account through the displayed authentication route? "
                        "Stops, pauses, holds and existing limits remain effective")
        if not reuse:
            self.p["session"] = {"id": "session-" + uuid.uuid4().hex, "work": work,
                                 "route": route, "expected_launch": view["launch"]}
            self.j.save()
        s = self.p["session"]
        require(s["work"] == work and s["route"] == route, "session_binding_changed")
        args = ["run", "--id", s["id"], "--work", work, "--authorize-run"]
        self.j.directory.write("session-launch.json", encoded(s["expected_launch"]))
        args += ["--expected-launch", str(self.j.directory.path / "session-launch.json")]
        if route["kind"] == "native_siwc":
            args += ["--vault", route["vault"], "--authorize-native"]
        else:
            args += ["--codex-executable", route["executable"], "--host-home", route["host_home"],
                     "--codex-home", route["codex_home"], "--authorize-codex-auth-only"]
        self.ui.say(f"Submitting {s['id']}. Waiting for its recorded native outcome.")
        # Host commands have no default duration limit. Wait for the native
        # session's outcome; explicit interruption still stops this helper.
        outcome = self.session(*args, timeout=None)
        self.session_binding(outcome, s)
        require(outcome.get("status") == "finished", "session_outcome_unknown_no_replay")
        terminal = outcome["terminal"]["outcome"]
        failure = terminal.get("driver_failure") or terminal.get("code")
        if failure:
            self.ui.say(f"Session stopped: {failure}. Review the saved state before another run.")
            if failure == "mandatory_context_overflow":
                self.ui.say("Mandatory context exceeds its allocation. Removing optional selections cannot fix this. "
                            "Use the explicit context-capacity-snapshot / extend-context-capacity controls "
                            "described in RUNTIME-REFERENCE.md to inspect recovery within existing provider allowances.")
        else:
            self.ui.say(f"Session finished: {terminal.get('stop')}; last recorded work call: {terminal.get('last_invocation')}.")
        self.session_output(s)

    def recover_session(self, view):
        s = self.p["session"]
        if s is None:
            return
        status = self.session_output(s)
        self.ui.say(f"Previous session {s['id']}: {status['status']}")
        if status["status"] == "outcome_unrecorded":
            self.ui.say("No terminal outcome is retained. No new session or replay will be started. "
                        "Keep this progress and use the exact advanced evidence/recovery controls in RUNTIME-REFERENCE.md.")
            raise Blocked("session_outcome_unknown_no_replay")
        if status["status"] == "not_recorded":
            if s["expected_launch"] != view["launch"]:
                self.ui.say("This unstarted session's saved launch has changed, so its exact request cannot run. "
                            "No session was started. Return to the work menu to review a fresh run.")
                return
            self.ui.say("The retained ID has no runtime claim. It is the ONLY ID eligible for a retry after your fresh consent.")
            if self.ui.ask("Authorize reviewing this same unclaimed session for dispatch? [yes/NO]").strip().lower() != "yes":
                raise Halt()
            self.run_work(s["work"], view, reuse=True)
        # Finished sessions are never dispatched again. A later explicit menu
        # action may request a distinct session through current eligible routes.

    def submit_pending(self, need):
        self.ui.say("Exact pending need:\n" + need["text"])
        if not need["shared"]:
            need["require_acceptance"] = self.ui.ask("Require your explicit acceptance of the eventual exact release? [yes/NO]").strip().lower() == "yes"
            self.ui.consent("Share and durably submit this exact need to the saved cohort and provider within its existing "
                            "permissions and per-work budget? Includes voluntary arrangement formation; no assigned roles or extra effects")
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
            # Recorded consent is for these exact immutable request bytes. Actual
            # setup completion is established only by the runtime-owned receipt.
            path, _ = self.j.document("bootstrap")
            self.session("bootstrap", path, "--authorize-setup")
        elif self.p["runtime"]["owned"]:
            require(not (root / "state.sqlite3").exists(), "unrecognized_existing_runtime")
            self.bootstrap()
        view = self.inspect()
        self.recover_session(view)
        after = None
        for _ in range(64):
            view = self.inspect(after)
            pending = [n for n in self.p["needs"] if n["work"] is None]
            settings = view["settings"]
            self.ui.say(f"Saved provider: {settings.get('model')} / {settings.get('effort')}; "
                        f"credential status: {settings.get('credential_status', 'unchecked')}; "
                        f"frozen={view['frozen']}, active invocation={view['active_invocations_present']}.")
            for i, work in enumerate(view["works"], 1):
                continuations = ", ".join(p["continuation"] for p in work["participants"])
                self.ui.say(f"{i}: {work['id']} — {work['phase']}, {work['attempts_left']} attempts left; "
                            f"{continuations or 'no participant continuation'}; "
                            f"{'eligible candidate' if self.eligible(view, work) else 'blocked/dormant'}")
                attempt = work.get("last_attempt")
                if attempt:
                    self.ui.say(f"   Last retained attempt: {attempt['id']} — {attempt['status']}; "
                                f"reservation: {attempt.get('reservation_state', 'unavailable')}")
                for participant in work["participants"]:
                    lifecycle = participant.get("lifecycle")
                    if lifecycle and (lifecycle["status"] != "active" or lifecycle["quarantined"]):
                        self.ui.say(f"   {participant['persona']}: identity {lifecycle['status']}; "
                                    f"quarantined={lifecycle['quarantined']}. See persona lifecycle controls in RUNTIME-REFERENCE.md.")
                    if participant.get("failed_attempt_hold"):
                        self.ui.say(f"   {participant['persona']}: failed-attempt hold {participant['failed_attempt_hold']}")
            saved_input = self.pending_work_input()
            if saved_input:
                request = saved_input["request"]
                if saved_input["kind"] == "resume-participant":
                    self.ui.say(f"Saved explicit resume for {request['persona']} in {request['work']} awaiting confirmation.")
                    self.ui.say("i: finish this same resume request; an earlier receipt cannot resume anyone again")
                elif saved_input["kind"] == "amend-work":
                    self.ui.say(f"Saved amendment for {request['work']} awaiting confirmation:\n{request['scope']['request']}")
                    self.ui.say("i: finish this same amendment; its identity and scope will be reused")
                else:
                    content = request if saved_input["kind"] == "feedback" else request["reply"]
                    self.ui.say(f"Saved {saved_input['kind']} for {request['work']} awaiting confirmation:\n{content['text']}")
                    if "basis" in content:
                        self.ui.say("Basis: " + content["basis"])
                    self.ui.say("i: deliver this same saved input; its identity and text will be reused")
            self.ui.say(f"{len(pending)} private pending need(s). p: submit first pending; n: new need; "
                        "work number: open work actions; more: next saved page; q: leave")
            choice = self.ui.ask("Choose:").strip().lower()
            if choice == "q" or not choice:
                return
            if choice == "n":
                self.capture_need()
            elif choice == "i" and saved_input:
                self.deliver_work_input()
            elif choice == "p" and pending:
                work_id = self.submit_pending(pending[0])
                after = None
                self.output(work_id)
                # Explicit run consent follows intake. The runtime still decides
                # admission; the UI cannot wake Stop or override a hold.
                self.run_work(work_id, self.inspect())
            elif choice == "more" and view.get("next_after"):
                after = view["next_after"]
            elif choice.isdecimal() and 1 <= int(choice) <= len(view["works"]):
                work = view["works"][int(choice) - 1]
                self.work_menu(work, after)
            else:
                self.ui.say("Choose a listed entry. No operation was started.")
        self.ui.say("This foreground interaction reached its menu bound; progress remains saved.")


def progress_candidates():
    """Return deterministic persistent candidates; never choose temporary storage implicitly."""
    explicit = os.environ.get("PERSONAS_LAUNCHER_HOME")
    if explicit is not None:
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

    This checks effective write/search access before asking consent. It does not
    reserve the path or guarantee a later mkdir/write: creation reopens and
    revalidates every component, so races and quota/read-only failures still fail
    closed after consent without selecting another journal location.
    """
    require(path_text(str(path)), "unsafe_private_path")
    parent = Path(path).parent
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
                # Match PrivateDirectory's intermediate-component policy: owner
                # identity alone is not a security boundary. Reject directories
                # writable by group/other, except root-owned sticky ancestors.
                # The final private leaf is separately required to be owned by
                # this UID and private; same-UID/root remain trusted principals.
                require(not info.st_mode & 0o022 or
                        (info.st_uid == 0 and info.st_mode & stat.S_ISVTX),
                        "writable_path_ancestor")
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
                os.close(fd)
                existing.append(path)
        except (Blocked, OSError):
            # Inaccessible is not absent. Never fall through to HOME and thereby
            # strand pending identities in an XDG journal we could not inspect.
            raise Blocked("progress_location_unavailable") from None
    require(len(existing) <= 1, "multiple_saved_progress_locations_choose_explicit_override")
    if existing:
        return PrivateDirectory(existing[0]), False
    require(safe_missing, "no_supported_persistent_state_location")
    path = safe_missing[0]
    # Fail before asking consent when the nearest existing parent is read-only.
    # This is advisory only; create=True repeats descriptor-safe validation.
    preflight_progress_creation(path)
    ui.say(f"Selected private persistent progress location: {path}")
    ui.say("The journal stores pending requests and operation identities, not credentials.")
    ui.consent("Create a new private progress journal here? This authorizes no sign-in or model call.")
    try:
        return PrivateDirectory(path, create=True), True
    except (Blocked, OSError):
        raise Blocked("progress_location_unavailable_after_consent") from None


def main():
    ui = Terminal()
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        ui.say("Interactive ./personas needs a terminal. Explicit argument-based commands remain available; see --help.")
        return 2
    if os.name != "posix" or sys.version_info < (3, 9):
        ui.say("This interaction needs Unix and Python 3.9+. No installer was run.")
        return 2
    directory = journal = None
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
        if journal:
            journal.close()
        if directory:
            directory.close()


if __name__ == "__main__":
    sys.exit(main())
