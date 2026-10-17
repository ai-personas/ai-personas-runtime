#!/usr/bin/env python3
"""Narrow offline tests of the actual dialogue, not live auth/model experiments."""
import copy
import hashlib
import importlib.util
import json
import os
import sys
import stat
import subprocess
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("personas_dialogue", ROOT / "scripts/personas_dialogue.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class Answers(m.Terminal):
    def __init__(self, answers):
        self.answers = list(answers)
        self.messages = []
        self.questions = []
        self.events = []

    def say(self, text):
        self.messages.append(str(text))
        self.events.append(("say", str(text)))

    def ask(self, text):
        self.questions.append(text)
        self.events.append(("ask", text))
        if not self.answers:
            raise AssertionError("Unexpected prompt: " + text)
        answer = self.answers.pop(0)
        if answer is None:
            raise m.Halt()
        return answer


class MockHelpers:
    """Synthetic backend contract. No real binaries, vaults, Stores or network.

    Effects are retained across UI/journal reopen, including when an injected
    lost response prevents the real dialogue from observing success.
    """
    def __init__(self, journal):
        self.j = journal
        self.calls = []
        self.connected = False
        self.probes = {}
        self.configuration_count = 0
        self.intakes = {}
        self.runs = {}
        self.setup = None
        self.lost = None
        self.phase = "open"
        self.stop = False
        self.frozen = False
        self.attempts = 16
        self.expiry = 9999999999
        self.account = "a" * 64

    def install(self):
        # Installation discovery/build is separately tested without runtime use.
        return

    @staticmethod
    def val(args, key):
        return args[args.index(key) + 1]

    def inspect(self):
        settings = {"mode": "siwc_oauth", "model": "fixture-model", "effort": "high",
                    "account_sha256": self.account, "saved_expires_at_unix": 9999999999,
                    "max_steps": 4}
        works = [{"id": w, "phase": self.phase, "attempts_left": self.attempts,
                  "participants": [{"continuation": "persona_stop" if self.stop else "immediate",
                                    "eligible_candidate": not self.stop and not self.frozen and self.phase == "open"}]}
                 for w in self.intakes.values()]
        return {"settings": settings, "works": works, "frozen": self.frozen,
                "active_invocations_present": False, "next_after": None}

    def call(self, name, args, **kwargs):
        self.calls.append((name, list(args)))
        if name == "personas-auth":
            if "sign-in" in args or "refresh" in args:
                self.connected = True
            return {"grant_connected": self.connected, "expires_at_unix": self.expiry,
                    "account_sha256": self.account}
        if name == "personas-provider":
            op = args[2]
            if op == "catalogue":
                return {"models": [{"display_name": "Fixture Only", "slug": "fixture-model"}],
                        "catalogue_sha256": "c" * 64, "effort_candidates_unverified": ["low", "high"]}
            if op == "renewal-status":
                return {"explicit_provider_migration_required": False}
            key = self.val(args, "--id")
            if op == "plan":
                request = self.j.data["probes"][-1]["request"]
                view = {"plan": {"request": copy.deepcopy(request), "account_sha256": self.account},
                        "plan_sha256": "d" * 64, "maximum_request_bytes": 10000,
                        "maximum_response_bytes": 20000, "maximum_stream_bytes": 40000,
                        "cost_notice": "Fixture: remote tokens/dollars are NOT hard bounded."}
                self.probes.setdefault(key, {"phase": "planned", "view": view})
                return view
            if op == "probe-status":
                return {"phase": self.probes[key]["phase"], "remote_usage_may_remain": False}
            if op == "probe":
                assert self.probes[key]["phase"] == "planned", "Synthetic dispatch was repeated"
                assert "--acknowledge-uncapped-remote-usage" in args
                self.probes[key]["phase"] = "outcome_unrecorded" if self.lost == "probe" else "completed_verified"
                if self.lost == "probe":
                    self.lost = None
                    raise m.Blocked("fixture_lost_probe")
                return {"phase": "completed_verified"}
            if op == "configuration":
                self.configuration_count += 1
                return {"mode": "siwc_oauth", "model": "fixture-model", "effort": "high",
                        "credentials": {"kind": "o_auth", "expires_at_unix": 9999999999},
                        "evidence": {"account_sha256": self.account}}
        assert name == "personas-session", (name, args)
        op = args[4]
        if op == "bootstrap":
            request = json.loads(Path(args[5]).read_text())
            if "--preview" in args:
                return {"runtime_config": {"navigation_per_call": {"calls": 8}, "navigation_per_work": {"calls": 128},
                            "context_bytes": 196608, "response_bytes": 32768, "max_repair_attempts": 0},
                        "storage_limits": {"total": {"attempts": 512}, "protected_closeout": {"attempts": 16},
                                           "max_identity_records": 100000, "max_history_events": 100000},
                        "seed_grants": {"character": True}}
            assert "--authorize-setup" in args
            if self.setup:
                assert self.setup == request, "Bootstrap settings were overwritten"
            self.setup = request
            if self.lost == "bootstrap":
                self.lost = None
                raise m.Blocked("fixture_lost_bootstrap")
            return {"replayed": True, "inference_performed": False}
        if op == "inspect":
            assert self.setup is not None
            return self.inspect()
        if op == "intake":
            key = self.val(args, "--id")
            work = self.intakes.setdefault(key, "work-" + str(len(self.intakes) + 1))
            if self.lost == "intake":
                self.lost = None
                raise m.Blocked("fixture_lost_intake")
            return {"receipt": {"work": work}}
        if op == "result":
            return {"status": "fixture-unfinished", "messages": [{"sender": "fixture", "text": "fixture result"}],
                    "local_deliveries": []}
        if op == "session-status":
            return {"status": self.runs.get(self.val(args, "--id"), "not_recorded")}
        if op == "run":
            if self.lost == "before_run":
                self.lost = None
                raise m.Blocked("fixture_before_dispatch")
            key = self.val(args, "--id")
            assert key not in self.runs, "Synthetic session redispatched"
            assert "--authorize-run" in args and "--authorize-native" in args
            self.runs[key] = "outcome_unrecorded" if self.lost == "run" else "finished"
            self.attempts -= 1
            if self.lost == "run":
                self.lost = None
                raise m.Blocked("fixture_lost_run")
            return {"status": "finished"}
        raise AssertionError((name, args))


# Exact prompts in the real dialogue. Blanks explicitly accept displayed resource
# proposals, never model/effort identity, grants, sign-in or inference consent.
FIRST = ["Exact need: café; $(not-a-command)", "", "", "yes", "s", "yes",
         "yes", "1", "high", "", "", "yes", "", "", "", "", "", "yes"]
SUBMIT = ["p", "", "yes", "yes"]


class DialogueTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="personas-ui-fixture-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.directory = m.PrivateDirectory(str(self.root))
        self.j = m.Journal(self.directory, create=True)
        self.h = MockHelpers(self.j)
        self.addCleanup(self.close)

    def close(self):
        if self.j:
            self.j.close()
            self.j = None
        if self.directory:
            self.directory.close()
            self.directory = None

    def reopen(self):
        self.j.close()
        self.j = m.Journal(self.directory)
        self.h.j = self.j

    def dialogue(self, answers):
        ui = Answers(answers)
        d = m.Dialogue(self.j, self.h, ui, ROOT)
        d.run()
        self.assertEqual(ui.answers, [])
        return ui

    def count(self, helper, op):
        return sum(name == helper and op in args for name, args in self.h.calls)

    def test_first_launch_signin_proof_atomic_setup_keyed_intake_and_single_session(self):
        self.dialogue(FIRST + SUBMIT + ["q"])
        self.assertEqual(len(self.h.intakes), 1)
        self.assertEqual(len(self.h.runs), 1)
        self.assertEqual(self.count("personas-provider", "probe"), 1)
        self.assertTrue(self.j.data["setup_authorized"])
        self.assertTrue(self.j.data["settings"])
        self.assertEqual(self.h.configuration_count, 1)
        self.assertIn("$(not-a-command)", self.j.data["needs"][0]["text"])
        self.assertFalse((ROOT / "not-a-command").exists())

    def test_return_reuses_installation_settings_work_and_requires_fresh_run(self):
        self.dialogue(FIRST + SUBMIT + ["q"])
        first = self.j.data["session"]["id"]
        self.reopen()
        self.dialogue(["1", "yes", "yes", "q"])
        self.assertNotEqual(self.j.data["session"]["id"], first)
        self.assertEqual(len(self.h.runs), 2)
        self.assertEqual(self.count("personas-provider", "probe"), 1)
        self.assertEqual(self.count("personas-auth", "sign-in"), 1)
        self.assertEqual(len(self.h.intakes), 1)

    def test_durable_settings_survive_lost_progress_pointer_without_reprobing(self):
        self.dialogue(FIRST + ["q"])
        expected = self.j.document("settings")[1]
        self.j.data["settings"] = None
        self.j.save()
        self.reopen()
        d = m.Dialogue(self.j, self.h, Answers([]), ROOT)
        self.assertEqual(d.settings(), expected)
        self.assertEqual(self.h.configuration_count, 1)
        self.assertEqual(self.count("personas-provider", "probe"), 1)
        self.assertIsNotNone(self.j.data["settings"])

    def test_native_remote_signin_guidance_precedes_choice_without_auth_side_effect(self):
        before_needs = copy.deepcopy(self.j.data["needs"])
        need = {"id": "need-guidance", "text": "Keep this pending", "require_acceptance": False,
                "shared": False, "work": None}
        self.j.data["needs"].append(need)
        self.j.save()
        ui = Answers(["yes", "q"])
        d = m.Dialogue(self.j, self.h, ui, ROOT)
        with self.assertRaises(m.Halt):
            d.account()
        events = ui.events
        access_index = next(i for i, (kind, text) in enumerate(events)
                            if kind == "ask" and text == "Access action:")
        guidance = (
            "same host",
            "https://developers.openai.com/siwc/token-sharing-open-source/sign-in",
            "https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms",
            "does not implement credential import or transfer",
        )
        guidance_indices = []
        for required in guidance:
            index = next(i for i, (kind, text) in enumerate(events)
                         if kind == "say" and required in text)
            self.assertLess(index, access_index, f"{required!r} must precede Access action")
            guidance_indices.append(index)
        self.assertEqual(guidance_indices, sorted(guidance_indices))
        self.assertFalse(any(kind == "ask" and "Authorize sign-in" in text
                             for kind, text in events))
        self.assertEqual(self.count("personas-auth", "sign-in"), 0)
        self.assertEqual(self.count("personas-auth", "refresh"), 0)
        self.assertEqual(self.count("personas-provider", "probe"), 0)
        self.assertEqual(self.j.data["needs"], before_needs + [need])
        self.reopen()
        self.assertEqual(self.j.data["needs"], before_needs + [need])

    def test_declined_probe_preserves_need_and_does_not_setup_or_dispatch(self):
        with self.assertRaises(m.Halt):
            self.dialogue(FIRST[:11] + ["no"])
        self.assertEqual(len(self.j.data["needs"]), 1)
        self.assertEqual(self.count("personas-provider", "probe"), 0)
        self.assertIsNone(self.h.setup)
        self.reopen()
        # Resume SAME unexecuted plan with new specific consent, not a new probe.
        self.dialogue(["yes"] + FIRST[12:] + ["q"])
        self.assertEqual(len(self.h.probes), 1)

    def test_declined_setup_is_not_authorized_by_a_saved_request_file(self):
        with self.assertRaises(m.Halt):
            self.dialogue(FIRST[:-1] + ["no"])
        self.assertIsNotNone(self.j.data["bootstrap"])
        self.assertFalse(self.j.data["setup_authorized"])
        self.assertIsNone(self.h.setup)
        self.reopen()
        self.dialogue(["", "yes", "q"])
        self.assertTrue(self.j.data["setup_authorized"])
        self.assertEqual(self.count("personas-provider", "probe"), 1)

    def test_unknown_probe_on_restart_is_not_replaced_or_dispatched(self):
        self.h.lost = "probe"
        with self.assertRaises(m.Blocked):
            self.dialogue(FIRST)
        key = self.j.data["probes"][0]["request"]["id"]
        self.reopen()
        with self.assertRaisesRegex(m.Blocked, "probe_outcome_unknown_no_replay"):
            self.dialogue([])
        self.assertEqual(len(self.h.probes), 1)
        self.assertEqual(self.j.data["probes"][0]["request"]["id"], key)
        self.assertEqual(self.count("personas-provider", "probe"), 1)

    def test_lost_setup_response_reconciles_same_exact_request(self):
        self.h.lost = "bootstrap"
        with self.assertRaises(m.Blocked):
            self.dialogue(FIRST)
        initial = copy.deepcopy(self.h.setup)
        self.reopen()
        self.dialogue(["q"])
        self.assertEqual(initial, self.h.setup)
        self.assertEqual(self.count("personas-provider", "probe"), 1)

    def test_lost_intake_reply_keeps_key_content_and_acceptance(self):
        self.h.lost = "intake"
        with self.assertRaises(m.Blocked):
            self.dialogue(FIRST + SUBMIT)
        need = copy.deepcopy(self.j.data["needs"][0])
        self.reopen()
        self.dialogue(["p", "yes", "q"])
        self.assertEqual(len(self.h.intakes), 1)
        self.assertEqual(self.j.data["needs"][0]["id"], need["id"])
        self.assertEqual(self.j.data["needs"][0]["text"], need["text"])
        self.assertEqual(len(self.h.runs), 1)

    def test_lost_session_response_queries_same_id_and_blocks_new_dispatch(self):
        self.h.lost = "run"
        with self.assertRaises(m.Blocked):
            self.dialogue(FIRST + SUBMIT)
        before = copy.deepcopy(self.j.data["session"])
        self.reopen()
        with self.assertRaisesRegex(m.Blocked, "session_outcome_unknown_no_replay"):
            self.dialogue([])
        self.assertEqual(before, self.j.data["session"])
        self.assertEqual(len(self.h.runs), 1)

    def test_stop_pause_and_exhaustion_are_not_resume_or_budget_grants(self):
        self.dialogue(FIRST + SUBMIT + ["q"])
        for stop, phase, attempts in [(True, "open", 15), (False, "paused", 15), (False, "open", 0)]:
            self.h.stop, self.h.phase, self.h.attempts = stop, phase, attempts
            before = len(self.h.runs)
            self.reopen()
            self.dialogue(["1", "q"])
            self.assertEqual(len(self.h.runs), before)
            self.assertEqual(self.h.attempts, attempts)

    def test_account_or_expiry_mismatch_is_not_implicit_migration(self):
        self.dialogue(FIRST + SUBMIT + ["q"])
        self.h.expiry -= 1
        self.reopen()
        with self.assertRaisesRegex(m.Blocked, "explicit_provider_migration_required"):
            self.dialogue(["1", "yes"])
        self.assertEqual(len(self.h.runs), 1)
        self.assertEqual(self.count("personas-auth", "refresh"), 0)

    def test_saved_unclaimed_session_uses_same_identity_after_fresh_consent(self):
        self.h.lost = "before_run"
        with self.assertRaises(m.Blocked):
            self.dialogue(FIRST + SUBMIT)
        retained = copy.deepcopy(self.j.data["session"])
        self.reopen()
        self.dialogue(["yes", "yes", "q"])
        self.assertEqual(retained, self.j.data["session"])
        self.assertEqual(len(self.h.runs), 1)

    def test_shell_skips_python_without_linux_capabilities(self):
        import pty
        bindir = self.root / "fake-python-bin"
        bindir.mkdir(mode=0o700)
        log = self.root / "selected-python"
        for name, capable in (("python3.14", False), ("python3.13", True)):
            script = bindir / name
            script.write_text(
                "#!/bin/sh\n"
                "if [ \"$2\" = \"-c\" ]; then\n"
                "  case \"$3\" in *sys.version_info*) exit 0;; esac\n"
                + ("  exit 0\n" if capable else "  exit 1\n") +
                "fi\n"
                f"printf '%s\\n' '{name}' > \"$PERSONAS_TEST_SELECTED\"\n"
                "exit 0\n"
            )
            script.chmod(0o700)
        master, slave = pty.openpty()
        try:
            proc = subprocess.Popen(
                ["/bin/sh", "./personas"], cwd=ROOT, stdin=slave, stdout=slave, stderr=slave,
                env={"HOME": str(self.root), "PATH": str(bindir) + ":/usr/bin:/bin",
                     "PERSONAS_TEST_SELECTED": str(log)})
            self.assertEqual(proc.wait(timeout=5), 0)
            self.assertEqual(log.read_text().strip(), "python3.13")
        finally:
            os.close(master)
            os.close(slave)

    def test_shell_no_arguments_enters_real_dialogue_and_decline_creates_no_state(self):
        import pty
        import shutil
        import subprocess
        import sys
        checkout = self.root / "checkout"
        (checkout / "scripts").mkdir(parents=True)
        shutil.copyfile(ROOT / "personas", checkout / "personas")
        (checkout / "personas").chmod(0o700)
        shutil.copyfile(ROOT / "scripts/personas_dialogue.py", checkout / "scripts/personas_dialogue.py")
        selected = self.root / "declined-private-state"
        master, slave = pty.openpty()
        try:
            proc = subprocess.Popen(["./personas"], cwd=checkout, stdin=slave, stdout=slave, stderr=slave,
                    env={"HOME":str(self.root), "PERSONAS_LAUNCHER_HOME":str(selected),
                         "PATH":str(Path(sys.executable).parent) + ":/usr/bin:/bin"})
            os.write(master, b"n\n\nno\n")
            try:
                self.assertEqual(proc.wait(timeout=10), 0)
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait()
            self.assertFalse(selected.exists())
        finally:
            os.close(master)
            os.close(slave)

    def test_journal_ownership_lock_symlink_and_malformed_json_are_not_reset(self):
        with self.assertRaises(BlockingIOError):
            m.Journal(self.directory)
        self.j.close()
        self.j = None
        before = b'{"version":1,"version":1}'
        self.directory.write("progress.json", before)
        with self.assertRaises(m.Blocked):
            m.Journal(self.directory)
        self.assertEqual(self.directory.read("progress.json"), before)
        os.symlink(self.root, self.root / "link")
        with self.assertRaises((OSError, m.Blocked)):
            m.PrivateDirectory(str(self.root / "link"))

    def test_private_state_rejects_active_checkout_but_ignores_unrelated_ancestor_marker(self):
        candidate = self.root / "private-state"
        candidate.mkdir(mode=0o700)
        (candidate / ".git").mkdir()
        with patch.object(m, "inside_checkout", return_value=False):
            directory = m.PrivateDirectory(str(candidate))
            directory.close()
        with patch.object(m, "inside_checkout", return_value=True):
            with self.assertRaisesRegex(m.Blocked, "state_inside_repository"):
                m.PrivateDirectory(str(candidate))

    def test_missing_installation_build_decline_never_executes_cargo(self):
        ui = Answers(["b", "no"])
        host = m.Helpers(self.j, ROOT, ui)
        with patch.object(host, "candidates", return_value=[]), patch.object(m.shutil, "which", return_value="/fixture/cargo"), \
                patch.object(m.subprocess, "Popen") as spawn:
            with self.assertRaises(m.Halt):
                host.install()
            spawn.assert_not_called()
        self.assertIsNone(self.j.data["installation"])

    def test_private_build_uses_child_umask_and_requires_build_and_adoption_consent(self):
        import sys
        cargo = self.root / "fake-cargo"
        cargo.write_text(
            "#!" + sys.executable + "\n"
            "import json, os, pathlib, sys\n"
            "target = pathlib.Path(os.environ['CARGO_TARGET_DIR'])\n"
            "(target / 'debug' / 'deps').mkdir(parents=True, mode=0o777, exist_ok=True)\n"
            "mask = os.umask(0); os.umask(mask)\n"
            "record = {'args': sys.argv[1:], 'cwd': os.getcwd(), 'umask': mask}\n"
            "(target / 'trace.json').write_text(json.dumps(record))\n"
            "for name in " + repr(m.NAMES) + ":\n"
            "    fd = os.open(target / 'debug' / name, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o777)\n"
            "    with os.fdopen(fd, 'wb') as out: out.write(b'fixture helper; never executed\\n')\n"
        )
        cargo.chmod(0o700)  # Only our synthetic Cargo, not an existing installation.
        parent_mask = os.umask(0o002)
        try:
            for approval in ("no", "yes"):
                ui = Answers(["b", "yes", approval])
                host = m.Helpers(self.j, ROOT, ui)
                with patch.object(host, "candidates", return_value=[]), \
                        patch.object(m.shutil, "which", return_value=str(cargo)), \
                        patch.dict(os.environ, {"HOME": str(self.root), "PATH": ""}, clear=True):
                    if approval == "no":
                        with self.assertRaises(m.Halt):
                            host.install()
                        self.assertIsNone(self.j.data["installation"])
                    else:
                        host.install()
                self.assertEqual(ui.answers, [])
                self.assertEqual(sum("Build this exact checkout" in q for q in ui.questions), 1)
                self.assertEqual(sum("Use these four local helpers" in q for q in ui.questions), 1)
                self.assertEqual(os.umask(0o002), 0o002, "The parent umask was changed")
                target = self.root / "build"
                trace = json.loads((target / "trace.json").read_text())
                self.assertEqual(trace, {"args": ["build", "--locked", "--bins", "-j", "1"],
                                         "cwd": str(ROOT), "umask": 0o077})
                for folder in (target, target / "debug", target / "debug/deps"):
                    self.assertEqual(folder.stat().st_mode & 0o777, 0o700)
                for name in m.NAMES:
                    binary = target / "debug" / name
                    self.assertEqual(binary.stat().st_mode & 0o777, 0o700)
                    digest = m.binary_digest(binary)
                    if approval == "yes":
                        self.assertEqual(self.j.data["installation"][name],
                                         {"path": str(binary), "sha256": digest})
            self.reopen()
            # Reuse only the approved hashes; no rebuild or new adoption prompt.
            with patch.object(m.subprocess, "Popen") as spawn:
                m.Helpers(self.j, ROOT, Answers([])).install()
                spawn.assert_not_called()
        finally:
            os.umask(parent_mask)

    def test_existing_writable_installation_is_rejected_without_chmod(self):
        folder = self.root / "unapproved-installation"
        folder.mkdir(mode=0o700)
        for name in m.NAMES:
            (folder / name).write_bytes(b"untrusted fixture; do not execute")
            (folder / name).chmod(0o775)
        folder.chmod(0o775)
        ui = Answers(["p", str(folder)])
        with patch.object(m.Helpers, "candidates", return_value=[]), \
                patch.object(m.subprocess, "Popen") as spawn:
            with self.assertRaisesRegex(m.Blocked, "unsafe_installation"):
                m.Helpers(self.j, ROOT, ui).install()
            spawn.assert_not_called()
        self.assertIsNone(self.j.data["installation"])
        self.assertEqual(folder.stat().st_mode & 0o777, 0o775)
        self.assertTrue(all((folder / name).stat().st_mode & 0o777 == 0o775 for name in m.NAMES))

    def interactive_main(self, home, answers, progress_path=None):
        ui = Answers(answers)
        def helpers(journal, checkout, terminal):
            self.h.j = journal
            return self.h
        env = {"HOME": str(home), "PATH": ""}
        if progress_path is not None:
            env["PERSONAS_LAUNCHER_HOME"] = str(progress_path)
        with patch.dict(os.environ, env, clear=True), \
                patch.object(m, "Terminal", return_value=ui), \
                patch.object(m, "Helpers", side_effect=helpers), \
                patch.object(m.sys.stdin, "isatty", return_value=True), \
                patch.object(m.sys.stdout, "isatty", return_value=True):
            code = m.main()
        self.assertEqual(ui.answers, [])
        return code, ui

    def repository_home(self):
        home = self.root / "repository-home"
        home.mkdir(mode=0o700)
        (home / ".git").write_text("fixture repository marker\n")
        return home

    def test_repository_default_alternate_journal_reopens_exact_unknown_probe(self):
        home = self.repository_home()
        selected = self.root / "alternate private progress"
        self.h.lost = "probe"
        code, ui = self.interactive_main(home, ["yes"] + FIRST[:12], progress_path=selected)
        self.assertEqual(code, 1)
        self.assertFalse(any("state_inside_repository" in msg for msg in ui.messages))
        self.assertFalse((home / ".local").exists())
        before = (selected / "progress.json").read_bytes()
        progress = json.loads(before)
        self.assertEqual(progress["needs"][0]["text"], FIRST[0])
        self.assertTrue(progress["probes"][0]["attempted"])
        probe_id = progress["probes"][0]["request"]["id"]
        self.assertEqual(self.h.probes[probe_id]["phase"], "outcome_unrecorded")
        code, ui = self.interactive_main(home, [], progress_path=selected)
        self.assertEqual(code, 1)
        self.assertTrue(any("probe_outcome_unknown_no_replay" in msg for msg in ui.messages))
        self.assertEqual((selected / "progress.json").read_bytes(), before)
        self.assertEqual(self.count("personas-provider", "probe"), 1)
        self.assertEqual(self.h.intakes, {})
        self.assertEqual(self.h.runs, {})
        self.assertFalse((home / ".local").exists())

    def test_repository_default_storage_decline_creates_nothing(self):
        home = self.repository_home()
        selected = self.root / "declined-alternate" / "progress"
        code, ui = self.interactive_main(home, ["no"], progress_path=selected)
        self.assertEqual(code, 0)
        self.assertTrue(any("Stopped" in msg for msg in ui.messages))
        self.assertFalse(selected.parent.exists())
        self.assertEqual(list(home.iterdir()), [home / ".git"])
        self.assertEqual(self.h.calls, [])

    def test_existing_journal_errors_never_offer_new_progress_or_relocation(self):
        for case in ("empty_directory", "missing_progress", "malformed", "locked"):
            with self.subTest(case=case):
                selected = self.root / case
                directory = journal = None
                try:
                    directory = m.PrivateDirectory(str(selected), create=True)
                    if case in ("missing_progress", "malformed", "locked"):
                        journal = m.Journal(directory, create=True)
                        if case != "locked":
                            journal.close()
                            journal = None
                        if case == "missing_progress":
                            (selected / "progress.json").unlink()
                        if case == "malformed":
                            directory.write("progress.json", b'{"version":1,"version":1}')
                    before = {p.name: p.read_bytes() for p in selected.iterdir()} if selected.exists() else None
                    code, ui = self.interactive_main(self.root, [], progress_path=selected)
                    self.assertEqual(code, 1)
                    self.assertEqual(len(ui.questions), 0, "An invalid existing journal must not prompt for relocation or grants")
                    after = {p.name: p.read_bytes() for p in selected.iterdir()} if selected.exists() else None
                    self.assertEqual(after, before)
                    self.assertEqual(self.h.calls, [])
                finally:
                    if journal:
                        journal.close()
                    if directory:
                        directory.close()

    def test_new_progress_choice_cannot_adopt_or_replace_existing_records(self):
        selected = self.root / "retained-progress"
        directory = m.PrivateDirectory(str(selected), create=True)
        try:
            journal = m.Journal(directory, create=True)
            journal.close()
            before = directory.read("progress.json")
            code, ui = self.interactive_main(self.root, ["", "", "q"], progress_path=selected)
            self.assertEqual(code, 0)
            # Existing journal is reused, not silently replaced or reset.
            with self.assertRaisesRegex(m.Blocked, "progress_already_exists"):
                m.Journal(directory, create=True)
            after = json.loads(directory.read("progress.json"))
            self.assertEqual(after["version"], 1)
            self.assertEqual(after["needs"], [])
            self.assertEqual(self.h.calls, [])
        finally:
            directory.close()

    def test_actual_helper_adapter_keeps_arguments_literal_and_redacts_stderr(self):
        # Use the current Python ELF as the helper on Linux; never execute a script
        # through the verified memfd path. Non-Linux retains the script fixture.
        import sys
        host = m.Helpers(self.j, ROOT, Answers([]))
        need = "--anything; $(not-a-command) café"
        if sys.platform.startswith("linux"):
            target = self.root / "helper-elf"
            target.write_bytes(Path(sys.executable).resolve().read_bytes())
            target.chmod(0o700)
            digest = m.binary_digest(str(target))
            self.j.data["installation"] = {n: {"path": str(target), "sha256": digest} for n in m.NAMES}
            self.j.save()
            code = "import json,sys; print(json.dumps({'ok':True,'result':sys.argv[1:]}))"
            self.assertEqual(host.call("personas-session", ["-c", code, "intake", "--", need]),
                             ["intake", "--", need])
            bad = "import sys; print('FIXTURE_SECRET_NOT_FOR_OUTPUT',file=sys.stderr); sys.exit(1)"
            with self.assertRaises(m.Blocked) as caught:
                host.call("personas-session", ["-c", bad])
            self.assertNotIn("FIXTURE_SECRET", str(caught.exception))
        else:
            target = self.root / "helper"
            target.write_text("#!" + sys.executable + "\nimport json,sys\nprint(json.dumps({'ok':True,'result':sys.argv[1:]}))\n")
            target.chmod(0o700)
            self.j.data["installation"] = {n: {"path": str(target), "sha256": m.binary_digest(target)} for n in m.NAMES}
            self.j.save()
            self.assertEqual(host.call("personas-session", ["intake", "--", need]), ["intake", "--", need])
            target.write_text("#!" + sys.executable + "\nimport sys\nprint('FIXTURE_SECRET_NOT_FOR_OUTPUT',file=sys.stderr)\nsys.exit(1)\n")
            self.j.data["installation"]["personas-session"]["sha256"] = m.binary_digest(target)
            with self.assertRaises(m.Blocked) as caught:
                host.call("personas-session", [])
            self.assertNotIn("FIXTURE_SECRET", str(caught.exception))


    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux mapped-UID installation policy")
    def test_linux_adoption_allows_mapped_ancestor_but_pins_owned_leaf(self):
        from types import SimpleNamespace
        folder = self.root / "mapped-root-install"
        folder.mkdir(mode=0o700)
        source = Path(sys.executable).resolve()
        for name in m.NAMES:
            target = folder / name
            target.write_bytes(source.read_bytes())
            target.chmod(0o700)
        original_lstat = Path.lstat
        foreign_uid = os.geteuid() + 12345
        def mapped_lstat(path):
            info = original_lstat(path)
            if path == self.root:
                return SimpleNamespace(st_mode=info.st_mode, st_uid=foreign_uid,
                    st_nlink=info.st_nlink, st_size=info.st_size)
            if path == self.root.parent:
                return SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_uid=foreign_uid,
                    st_nlink=info.st_nlink, st_size=info.st_size)
            return info
        host = m.Helpers(self.j, ROOT, Answers(["yes"]))
        with patch.object(Path, "lstat", mapped_lstat):
            host.adopt(folder)
        for name in m.NAMES:
            self.assertRegex(self.j.data["installation"][name]["sha256"], r"^[a-f0-9]{64}$")
        # The stored digest remains mandatory at execution; changing the leaf refuses.
        target = folder / m.NAMES[0]
        target.write_bytes(b"changed")
        approved = self.j.data["installation"][m.NAMES[0]]["sha256"]
        self.assertNotEqual(m.binary_digest(target), approved)
        with self.assertRaises(m.Blocked):
            host.call(m.NAMES[0], [])

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux sealed-snapshot contract")
    def test_linux_hardlinked_source_is_pinned_by_approved_snapshot_digest(self):
        target = self.root / "hardlinked-elf"
        target.write_bytes(Path(sys.executable).resolve().read_bytes())
        target.chmod(0o700)
        os.link(target, self.root / "hardlink-alias")
        approved = m.binary_digest(target)
        self.assertRegex(approved, r"^[a-f0-9]{64}$")
        if m.VerifiedExecutable.supported():
            verified = m.VerifiedExecutable(str(target), approved)
            try:
                self.assertEqual(verified.digest, approved)
            finally:
                verified.close()

    @unittest.skipUnless(m.VerifiedExecutable.supported(), "Linux memfd sealing unavailable")
    def test_verified_snapshot_executes_original_after_path_mutation_and_replacement(self):
        source = Path(sys.executable).resolve()
        target = self.root / "approved-elf"
        target.write_bytes(source.read_bytes())
        target.chmod(0o700)
        expected = hashlib.sha256(target.read_bytes()).hexdigest()
        verified = m.VerifiedExecutable(str(target), expected)
        try:
            # Mutate the original inode in place, then replace its pathname.
            with target.open("r+b") as out:
                out.seek(0)
                out.write(b"NOT-AN-ELF")
                out.flush()
                os.fsync(out.fileno())
            replacement = self.root / "replacement"
            replacement.write_text("not the approved executable")
            os.replace(replacement, target)
            proc = subprocess.run([verified.path, "-c", "print('sealed-original')"],
                                  pass_fds=(verified.fd,), stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, timeout=5, check=False)
            self.assertEqual(proc.returncode, 0, proc.stderr.decode("utf-8", "replace"))
            self.assertEqual(proc.stdout, b"sealed-original\n")
        finally:
            verified.close()

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux memfd snapshot contract")
    def test_verified_snapshot_missing_capability_fails_closed(self):
        target = self.root / "capability-fixture-elf"
        target.write_bytes(Path(sys.executable).resolve().read_bytes())
        target.chmod(0o700)
        if not m.VerifiedExecutable.supported():
            with self.assertRaisesRegex(m.Blocked, "verified_exec_unavailable"):
                m.VerifiedExecutable(str(target), "0" * 64)
            return
        with patch.object(m.fcntl, "fcntl", None):
            with self.assertRaisesRegex(m.Blocked, "verified_exec_unavailable"):
                m.VerifiedExecutable(str(target), "0" * 64)

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux memfd snapshot contract")
    def test_verified_snapshot_rejects_wrong_digest_and_shebang(self):
        with self.assertRaises(m.Blocked):
            m.VerifiedExecutable(str(Path(sys.executable).resolve()), "0" * 64)
        script = self.root / "helper-script"
        script.write_text("#!/bin/sh\nexit 0\n")
        script.chmod(0o700)
        if m.VerifiedExecutable.supported():
            with self.assertRaisesRegex(m.Blocked, "verified_exec_format_unsupported"):
                m.VerifiedExecutable(str(script), hashlib.sha256(script.read_bytes()).hexdigest())
        else:
            with self.assertRaisesRegex(m.Blocked, "verified_exec_unavailable"):
                m.VerifiedExecutable(str(script), hashlib.sha256(script.read_bytes()).hexdigest())

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux memfd snapshot contract")
    def test_verified_snapshot_rejects_fifo_without_blocking(self):
        fifo = self.root / "helper-fifo"
        os.mkfifo(fifo, 0o600)
        code = "import sys; from pathlib import Path; sys.path.insert(0,sys.argv[1]); import importlib.util; p=Path(sys.argv[2]); s=importlib.util.spec_from_file_location('pd',p); m=importlib.util.module_from_spec(s); s.loader.exec_module(m); m.VerifiedExecutable(sys.argv[3], '0'*64)"
        proc = subprocess.run([sys.executable, "-c", code, str(ROOT / "scripts"),
                               str(ROOT / "scripts/personas_dialogue.py"), str(fifo)],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=1, check=False)
        self.assertNotEqual(proc.returncode, 0)



class LinuxVerifiedExecutableTests(unittest.TestCase):
    """Isolated executable tests; intentionally do not instantiate private vault/journal."""
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="verified-exec-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.root.chmod(0o700)

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux memfd snapshot contract")
    def test_descriptor_snapshot_survives_inode_mutation_and_path_replacement(self):
        if not m.VerifiedExecutable.supported():
            self.skipTest("required memfd sealing/procfs capabilities unavailable")
        source = Path(sys.executable).resolve()
        target = self.root / "helper-elf"
        target.write_bytes(source.read_bytes())
        target.chmod(0o700)
        expected = hashlib.sha256(target.read_bytes()).hexdigest()
        verified = m.VerifiedExecutable(str(target), expected)
        try:
            with target.open("r+b") as out:
                out.seek(0); out.write(b"MUTATED-ELF"); out.flush(); os.fsync(out.fileno())
            replacement = self.root / "replacement"
            replacement.write_text("not executable")
            os.replace(replacement, target)
            proc = subprocess.run([verified.path, "-c", "print('snapshot-ok')"], pass_fds=(verified.fd,),
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
            self.assertEqual(proc.returncode, 0, proc.stderr.decode("utf-8", "replace"))
            self.assertEqual(proc.stdout, b"snapshot-ok\n")
        finally:
            verified.close()

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux source policy")
    def test_fifo_is_rejected_without_blocking(self):
        fifo = self.root / "helper-fifo"; os.mkfifo(fifo, 0o600)
        code = ("import sys; from pathlib import Path; sys.path.insert(0,sys.argv[1]); "
                "import importlib.util; p=Path(sys.argv[2]); s=importlib.util.spec_from_file_location('pd',p); "
                "m=importlib.util.module_from_spec(s); s.loader.exec_module(m); "
                "m.binary_digest(sys.argv[3])")
        proc = subprocess.run([sys.executable, "-c", code, str(ROOT / "scripts"),
                               str(ROOT / "scripts/personas_dialogue.py"), str(fifo)],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
        self.assertNotEqual(proc.returncode, 0)

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux capability checks")
    def test_missing_fcntl_callable_is_controlled(self):
        target = self.root / "helper-elf"
        target.write_bytes(Path(sys.executable).resolve().read_bytes()); target.chmod(0o700)
        with patch.object(m.fcntl, "fcntl", None):
            with self.assertRaisesRegex(m.Blocked, "verified_exec_unavailable"):
                m.VerifiedExecutable(str(target), hashlib.sha256(target.read_bytes()).hexdigest())

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux descriptor cleanup")
    def test_child_directory_fstat_failure_closes_new_descriptor(self):
        folder = self.root / "helpers"
        folder.mkdir(mode=0o700)
        target = folder / "helper"
        target.write_bytes(Path(sys.executable).resolve().read_bytes())
        target.chmod(0o700)
        original_fstat = os.fstat
        def fail_child(fd):
            try:
                name = os.readlink(f"/proc/self/fd/{fd}")
            except OSError:
                name = ""
            if name == str(folder):
                raise OSError("synthetic fstat failure")
            return original_fstat(fd)
        before = len(os.listdir("/proc/self/fd"))
        with patch.object(m.os, "fstat", fail_child):
            with self.assertRaisesRegex(m.Blocked, "unsafe_installation_path"):
                m._linux_open_regular(str(target))
        self.assertEqual(len(os.listdir("/proc/self/fd")), before)

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux mapped ancestor policy")
    def test_descriptor_anchored_adoption_and_leaf_link_refusal(self):
        # The fixture models a mapped-UID, non-writable intermediate directory
        # through fstat; this tests policy acceptance, not resistance to its owner
        # changing mode after approval. No private Vault or runtime Store is opened.
        folder = self.root / "helpers"; folder.mkdir(mode=0o700)
        binary = Path(sys.executable).resolve().read_bytes()
        for name in m.NAMES:
            target = folder / name; target.write_bytes(binary); target.chmod(0o700)
        class JournalFixture:
            data = {"installation": None}
            def save(self): pass
        class Consent:
            def consent(self, _): pass
        host = m.Helpers(JournalFixture(), ROOT, Consent())
        # Model a mapped foreign UID on a non-writable intermediate directory.
        # The real openat/fstat traversal still runs; only that directory's
        # metadata is represented as the mapped host view.
        original_fstat = os.fstat
        mapped_path = str(self.root)
        def mapped_fstat(fd):
            info = original_fstat(fd)
            try:
                actual = os.readlink(f"/proc/self/fd/{fd}")
            except OSError:
                actual = ""
            if actual == mapped_path and stat.S_ISDIR(info.st_mode):
                from types import SimpleNamespace
                return SimpleNamespace(st_mode=stat.S_IFDIR | 0o555, st_uid=os.geteuid() + 12345,
                    st_nlink=info.st_nlink, st_size=info.st_size, st_dev=info.st_dev,
                    st_ino=info.st_ino, st_mtime_ns=info.st_mtime_ns, st_ctime_ns=info.st_ctime_ns)
            return info
        # Cargo/build layouts may hard-link a root-owned helper. Explicit approval
        # pins its stable bytes; execution later snapshots those exact bytes into
        # a sealed, single-object memfd. The source link itself is never executed.
        target = folder / m.NAMES[0]
        os.link(target, self.root / "alias")
        with patch.object(m.os, "fstat", mapped_fstat):
            host.adopt(folder)
        self.assertEqual(set(host.j.data["installation"]), set(m.NAMES))
        approved = host.j.data["installation"][m.NAMES[0]]["sha256"]
        if m.VerifiedExecutable.supported():
            verified = m.VerifiedExecutable(str(target), approved)
            try:
                self.assertEqual(verified.digest, approved)
            finally:
                verified.close()


class AutomaticProgressSelectionTests(unittest.TestCase):
    def test_selects_standard_persistent_candidate_and_creates_journal_after_consent(self):
        with tempfile.TemporaryDirectory(prefix="personas-auto-state-") as raw:
            root = Path(raw)
            root.chmod(0o700)
            ui = Answers(["yes"])
            with patch.dict(os.environ, {"XDG_STATE_HOME": str(root), "HOME": str(root)}, clear=False), \
                    patch.object(m, "inside_checkout", return_value=False):
                directory, created = m.select_progress_directory(ui)
                try:
                    self.assertTrue(created)
                    self.assertEqual(directory.path, root / "ai-personas-launcher")
                    journal = m.Journal(directory, create=True)
                    journal.close()
                    self.assertTrue((directory.path / "progress.json").is_file())
                finally:
                    directory.close()
            self.assertEqual(ui.answers, [])

    def test_preflight_accepts_mapped_owner_nonwritable_ancestor(self):
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory(prefix="personas-mapped-preflight-") as raw:
            root = Path(raw); root.chmod(0o700)
            target = root / "state" / "journal"
            real_fstat = os.fstat
            calls = 0
            def mapped_fstat(fd):
                nonlocal calls
                calls += 1
                info = real_fstat(fd)
                # Model a mapped/foreign UID on an intermediate directory with
                # no group/other write permission; the leaf is still validated
                # by PrivateDirectory when it is created/opened.
                if calls > 1:
                    return SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_uid=42424)
                return info
            with patch.object(m.os, "fstat", side_effect=mapped_fstat):
                m.preflight_progress_creation(target)

    def test_preflight_missing_open_flag_is_controlled(self):
        with tempfile.TemporaryDirectory(prefix="personas-preflight-capability-") as raw:
            target = Path(raw) / "state" / "journal"
            with patch.object(m.os, "O_CLOEXEC", None):
                with self.assertRaisesRegex(m.Blocked, "progress_location_unavailable"):
                    m.preflight_progress_creation(target)

    def test_readonly_parent_is_rejected_before_creation_consent(self):
        with tempfile.TemporaryDirectory(prefix="personas-auto-readonly-") as raw:
            root = Path(raw); root.chmod(0o700)
            ui = Answers([])
            with patch.dict(os.environ, {"XDG_STATE_HOME": str(root), "HOME": str(root)}, clear=False), \
                    patch.object(m, "inside_checkout", return_value=False), \
                    patch.object(m.os, "access", return_value=False):
                with self.assertRaisesRegex(m.Blocked, "progress_location_not_writable"):
                    m.select_progress_directory(ui)
            self.assertEqual(ui.questions, [], "writability preflight must precede consent")
            self.assertFalse((root / "ai-personas-launcher").exists())

    def test_preflight_is_advisory_and_creation_race_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="personas-auto-race-") as raw:
            root = Path(raw); root.chmod(0o700)
            ui = Answers(["yes"])
            with patch.dict(os.environ, {"XDG_STATE_HOME": str(root), "HOME": str(root)}, clear=False), \
                    patch.object(m, "inside_checkout", return_value=False), \
                    patch.object(m.PrivateDirectory, "__init__", side_effect=OSError("read-only race")):
                with self.assertRaisesRegex(m.Blocked, "progress_location_unavailable_after_consent"):
                    m.select_progress_directory(ui)
            self.assertEqual(ui.answers, [])
            self.assertEqual(len(ui.questions), 1)

    def test_existing_saved_location_wins_without_reset_or_prompt(self):
        with tempfile.TemporaryDirectory(prefix="personas-auto-existing-") as raw:
            root = Path(raw); root.chmod(0o700)
            saved = root / "state" / "ai-personas-launcher"
            saved.mkdir(parents=True, mode=0o700)
            (saved / "progress.json").write_text('{"retained":true}')
            (saved / "progress.json").chmod(0o600)
            ui = Answers([])
            with patch.dict(os.environ, {"XDG_STATE_HOME": str(root / "state"), "HOME": str(root)}, clear=False), \
                    patch.object(m, "inside_checkout", return_value=False):
                directory, created = m.select_progress_directory(ui)
                try:
                    self.assertFalse(created)
                    self.assertEqual(directory.path, saved)
                    self.assertEqual(directory.read("progress.json"), b'{"retained":true}')
                finally:
                    directory.close()
            self.assertEqual(ui.answers, [])

    def test_inaccessible_preferred_candidate_blocks_home_fallback(self):
        with tempfile.TemporaryDirectory(prefix="personas-auto-inaccessible-") as raw:
            root = Path(raw); root.chmod(0o700)
            xdg = root / "xdg" / "ai-personas-launcher"
            home = root / ".local" / "state" / "ai-personas-launcher"
            ui = Answers([])
            with patch.dict(os.environ, {"XDG_STATE_HOME": str(root / "xdg"), "HOME": str(root)}, clear=False), \
                    patch.object(m.PrivateDirectory, "open_path", side_effect=[PermissionError("synthetic inaccessible")]):
                with self.assertRaisesRegex(m.Blocked, "progress_location_unavailable"):
                    m.select_progress_directory(ui)
            self.assertFalse(home.exists(), "must not create a fallback journal when preferred state is inaccessible")
            self.assertEqual(ui.answers, [])

    def test_inside_checkout_resolution_runtime_error_is_controlled_refusal(self):
        with patch.object(m.Path, "resolve", side_effect=RuntimeError("synthetic symlink loop")):
            with self.assertRaisesRegex(m.Blocked, "unsafe_private_path"):
                m.inside_checkout("/synthetic/path")

    def test_multiple_existing_journals_block_instead_of_relocating(self):
        with tempfile.TemporaryDirectory(prefix="personas-auto-ambiguous-") as raw:
            root = Path(raw); root.chmod(0o700)
            xdg = root / "xdg" / "ai-personas-launcher"
            home = root / ".local" / "state" / "ai-personas-launcher"
            xdg.mkdir(parents=True, mode=0o700); home.mkdir(parents=True, mode=0o700)
            ui = Answers([])
            with patch.dict(os.environ, {"XDG_STATE_HOME": str(root / "xdg"), "HOME": str(root)}, clear=False), \
                    patch.object(m, "inside_checkout", return_value=False):
                with self.assertRaisesRegex(m.Blocked, "multiple_saved_progress_locations"):
                    m.select_progress_directory(ui)
            self.assertEqual(ui.answers, [])

if __name__ == "__main__":
    unittest.main()
