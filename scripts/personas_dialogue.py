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


def new_progress():
    return {"version": 1, "operator": None, "installation": None, "runtime": None,
            "vault": None, "needs": [], "session": None, "probes": [],
            "settings": None, "bootstrap": None, "setup_authorized": False}


def validate_progress(p):
    require(isinstance(p, dict) and set(p) == set(new_progress()) and p["version"] == 1)
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
        require(isinstance(s, dict) and set(s) == {"id", "work", "vault"}
                and identifier(s["id"]) and identifier(s["work"]) and path_text(s["vault"]))
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
            cargo = shutil.which("cargo")
            if not cargo:
                self.ui.say("Cargo/Rust 1.88+ and a native C toolchain/linker are required to build. "
                            "No installer or system change was run. Install the toolchain from rust-lang.org/tools/install "
                            "under your host policy, or choose an approved existing helper directory here.")
                return self.install()
            self.ui.consent("Build this exact checkout with locked Cargo dependencies, one job and no debug symbols? "
                            "Cargo may download locked crates and execute their build scripts. No sudo/system installation. "
                            "Build/cache files consume disk; failure preserves your need")
            target = self.j.directory.path / "build"
            env = self.environment()
            env.update(CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0", CARGO_INCREMENTAL="0",
                       CARGO_TARGET_DIR=str(target))
            for k in ("CARGO_HOME", "RUSTUP_HOME"):
                if k in os.environ:
                    env[k] = os.environ[k]
            self.ui.say("Building local helpers; Cargo progress follows. Interrupting never starts provider work.")
            # Child-only (Python 3.9+): keep new build paths private even when
            # the caller uses umask 0002. Never chmod an existing installation.
            proc = subprocess.Popen([cargo, "build", "--locked", "--bins", "-j", "1"],
                                    cwd=self.checkout, env=env, start_new_session=True, umask=0o077)
            try:
                code = proc.wait()
            except BaseException:
                self.stop(proc)
                raise
            require(code == 0, "build_failed")
            self.adopt(target / "debug")
        else:
            raise Halt()

    @staticmethod
    def environment():
        # Native helpers use their explicit vault, not inherited provider secrets.
        return {k: os.environ[k] for k in ("HOME", "PATH", "LANG", "LC_ALL", "TMPDIR")
                if k in os.environ}

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
                deadline = time.monotonic() + timeout
                while poll.get_map():
                    require(time.monotonic() < deadline, "helper_outcome_unknown")
                    for key, _ in poll.select(min(0.2, max(0, deadline - time.monotonic()))):
                        part = os.read(key.fd, 8192)
                        if not part:
                            poll.unregister(key.fileobj)
                        else:
                            key.data.extend(part)
                            require(len(key.data) <= (MAX_OUTPUT if key.data is out else 262144),
                                    "helper_output_bound_unknown")
                code = proc.wait(timeout=max(0.1, deadline - time.monotonic()))
            if code != 0:
                try:
                    error = decode(err)
                    label = error.get("code", "helper_rejected")
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
            self.ui.say("Proposed local cohort: 2 continuing personas and you, without assigned owner/reviewer roles. "
                        "Grants permit their own fragments, assets, character proposals, work commands and cohort messages. "
                        "This grants NO host shell/tools, external effects, automatic retries or background work.")
            attempts = self.ui.number("Lifetime attempt allowance for each new work", 16, 1, 128)
            total = self.ui.number("Saved launch lifetime foreground-step ceiling (never auto-refilled)", 64, 1, 256)
            steps = self.ui.number("Maximum foreground attempts per explicitly authorized session", 4, 1, min(128, total))
            works = self.ui.number("Maximum saved work identities", 8, 1, 128)
            request = {"id": "bootstrap-" + uuid.uuid4().hex, "operator": self.p["operator"],
                       "allow_direct_provider": True, "setup": {"cohort": 2, "owner": 1,
                       "destination": "openai-siwc", "provider": settings, "working_directory": str(self.checkout),
                       "share_cohort": True, "max_steps": steps, "max_considerations": 128,
                       "max_work_attempts": attempts, "max_total_steps": total, "max_works": works}}
            self.j.keep("bootstrap", request, request["id"])
        path, request = self.j.document("bootstrap")
        preview = self.session("bootstrap", path, "--preview")
        setup = request["setup"]
        self.ui.say(f"Runtime: {self.p['runtime']['path']}\n"
                    f"Operator: {self.p['operator']}; exact model/effort: {setup['provider']['model']} / {setup['provider']['effort']}\n"
                    f"Share needs, authorized persona context and outputs with OpenAI SIWC, 2 personas and this requester. "
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

    def output(self, work):
        result = self.session("result", work)
        self.ui.say(f"Work {work}: {result.get('status', 'unavailable')}. "
                    "Local delivery/attributed completion is not independent quality or human acceptance.")
        for msg in result.get("messages", []):
            self.ui.say(f"{msg.get('sender', 'unknown author')}: {msg.get('text', '')}")
        for delivery in result.get("local_deliveries", []):
            self.ui.say("Permission-projected local delivery (including required qualifications):")
            # Preserve the complete qualified package, policy/author attribution,
            # and exact body. Never peel text out of its qualifications.
            self.ui.say(json.dumps(delivery, ensure_ascii=False, indent=2))
        if result.get("truncated"):
            self.ui.say("This permission-projected result is truncated at its existing bound.")
        if result.get("human_decision_pointers"):
            self.ui.say("Explicit acceptance is separate; no acceptance was fabricated. See docs/activation.md and RUNTIME-REFERENCE.md.")

    @staticmethod
    def eligible(view, work):
        return (not view["frozen"] and not view["active_invocations_present"]
                and work["phase"] == "open" and work["attempts_left"] > 0
                and any(p["eligible_candidate"] for p in work["participants"]))

    def run_work(self, work, view, reuse=False):
        settings = view["settings"]
        require(settings.get("mode") == "siwc_oauth", "interactive_native_siwc_only")
        self.account(settings)
        self.ui.say(f"Exact saved model/effort: {settings['model']} / {settings['effort']}; "
                    f"at most {settings['max_steps']} attempts, within remaining work/runtime limits. "
                    "SIWC has NO hard remote token/dollar cap; local time/bytes do not guarantee remote cancellation.")
        self.ui.consent("Authorize this ONE bounded native foreground session and sending its currently permitted "
                        "context to this exact saved account? Stops, pauses, holds and existing limits remain effective")
        if not reuse:
            self.p["session"] = {"id": "session-" + uuid.uuid4().hex, "work": work, "vault": self.p["vault"]}
            self.j.save()
        s = self.p["session"]
        require(s["work"] == work and s["vault"] == self.p["vault"], "session_binding_changed")
        outcome = self.session("run", "--id", s["id"], "--work", work, "--authorize-run",
                               "--vault", s["vault"], "--authorize-native",
                               timeout=settings["max_steps"] * 300 + 60)
        self.output(work)
        require(outcome.get("status") == "finished", "session_outcome_unknown_no_replay")

    def recover_session(self, view):
        s = self.p["session"]
        if s is None:
            return
        status = self.session("session-status", "--id", s["id"])
        self.ui.say(f"Previous session {s['id']}: {status['status']}")
        self.output(s["work"])
        if status["status"] == "outcome_unrecorded":
            self.ui.say("No terminal outcome is retained. No new session or replay will be started. "
                        "Keep this progress and use the exact advanced evidence/recovery controls in RUNTIME-REFERENCE.md.")
            raise Blocked("session_outcome_unknown_no_replay")
        if status["status"] == "not_recorded":
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
                    if participant.get("failed_attempt_hold"):
                        self.ui.say(f"   {participant['persona']}: failed-attempt hold {participant['failed_attempt_hold']}")
            self.ui.say(f"{len(pending)} private pending need(s). p: submit first pending; n: new need; "
                        "work number: result/continue; more: next saved page; q: leave")
            choice = self.ui.ask("Choose:").strip().lower()
            if choice == "q" or not choice:
                return
            if choice == "n":
                self.capture_need()
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
                self.output(work["id"])
                if self.eligible(view, work):
                    if self.ui.ask("Continue only currently eligible participants? [yes/NO]").strip().lower() == "yes":
                        self.run_work(work["id"], view)
                else:
                    self.ui.say("No eligible continuation was authorized. Stop, pause, complete rejection, expired credentials, "
                                "uncertain usage, holds or exhausted limits require their separate supported decisions. "
                                "Inspect the retained status above; see RUNTIME-REFERENCE.md and docs/activation.md. "
                                "No automatic refresh, migration, resume or replenishment was performed.")
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
