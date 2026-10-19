#!/usr/bin/env python3
"""Offline launcher contract tests. Never invoke a real Personas installation.

Only temporary fake executables run, under a fresh HOME, XDG_STATE_HOME and PATH.
No Cargo, provider, credentials, real Store, or additional packages are needed.
"""
import json
import os
from pathlib import Path
import pty
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SHELL = os.environ.get("LAUNCHER_TEST_SHELL")


class LauncherSmoke(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="personas-launcher-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.checkout = self.root / "source checkout"
        self.checkout.mkdir()
        self.launcher = self.checkout / "personas"
        shutil.copyfile(ROOT / "personas", self.launcher)
        self.launcher.chmod(0o755)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.cwd = self.root / "caller workspace"
        self.cwd.mkdir()
        self.home = self.root / "home"
        self.home.mkdir()
        self.trace = self.root / "trace.jsonl"
        # Deliberately do not inherit PATH, credentials, shell startup files,
        # binary overrides, or any installed runtime configuration.
        self.env = {
            "PATH": str(self.bin),
            "HOME": str(self.home),
            "XDG_STATE_HOME": str(self.root / "state"),
            "LC_ALL": "C",
            "TRACE": str(self.trace),
        }

    def command(self, *args):
        return ([SHELL] if SHELL else []) + [str(self.launcher), *args]

    def fake(self, name="ai-personas", directory=None):
        target = (directory or self.bin) / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(
            "#!" + sys.executable + "\n"
            "import json, os, signal, sys\n"
            "record = {'args': sys.argv[1:], 'binary': sys.argv[0],\n"
            "          'cwd': os.getcwd(), 'pid': os.getpid(),\n"
            "          'home': os.environ['HOME']}\n"
            "with open(os.environ['TRACE'], 'a') as out:\n"
            "    out.write(json.dumps(record) + '\\n')\n"
            "sys.stdout.write(os.environ.get('FAKE_STDOUT', 'fixture output\\n'))\n"
            "sys.stderr.write(os.environ.get('FAKE_STDERR', ''))\n"
            "sys.stdout.flush()\n"
            "sys.stderr.flush()\n"
            "if os.environ.get('FAKE_SIGNAL'):\n"
            "    os.kill(os.getpid(), int(os.environ['FAKE_SIGNAL']))\n"
            "sys.exit(int(os.environ.get('FAKE_EXIT', '0')))\n"
        )
        target.chmod(0o755)
        return target

    def run_launcher(self, *args, input=""):
        return subprocess.run(
            self.command(*args), input=input, text=True, capture_output=True,
            cwd=self.cwd, env=self.env, timeout=5, check=False,
        )

    def records(self):
        if not self.trace.exists():
            return []
        return [json.loads(line) for line in self.trace.read_text().splitlines()]

    def assert_not_run(self, result, code=2):
        self.assertEqual(result.returncode, code, result.stderr)
        self.assertEqual(self.records(), [])
        self.assertFalse((self.root / "state").exists())
        self.assertEqual(list(self.home.iterdir()), [])

    def test_help_needs_no_installation_or_state(self):
        result = self.run_launcher("--help")
        self.assert_not_run(result, 0)
        self.assertIn("--provider-config", result.stdout)
        self.assertIn("docs/activation.md", result.stdout)

    def test_missing_installation_has_explicit_setup_path(self):
        result = self.run_launcher("a need")
        self.assert_not_run(result, 127)
        self.assertIn("cargo build --locked --release", result.stderr)

    def test_exact_need_is_one_argument_not_shell_code(self):
        self.fake()
        need = "Quotes ' \"; $(touch forbidden); `touch forbidden`; \\ *\nUnicode: café"
        result = self.run_launcher(need)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records()[0]["args"], ["run", "--", need])
        self.assertEqual(list(self.cwd.iterdir()), [])

    def test_dash_prefixed_need_is_data(self):
        self.fake()
        result = self.run_launcher("--", "--model is part of my request")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records()[0]["args"], ["run", "--", "--model is part of my request"])

    def test_globals_and_new_need_acceptance(self):
        self.fake()
        state_root = str(self.root / "state with spaces")
        result = self.run_launcher(
            "--state-root", state_root, "--operator", "owner",
            "--json", "--require-acceptance", "the need",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records()[0]["args"], [
            "--state-root=" + state_root, "--operator=owner",
            "run", "--json", "--require-acceptance", "--", "the need",
        ])

    def test_resume_is_only_the_existing_run_resume(self):
        self.fake()
        result = self.run_launcher("--resume", "work-00000001", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records()[0]["args"], ["run", "--json", "--resume=work-00000001"])

    def test_result_never_selects_the_auth_binary(self):
        self.fake()
        self.fake("personas-auth")
        self.env["PERSONAS_AUTH_BIN"] = "/does/not/exist"
        result = self.run_launcher("--result", "work-00000001", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records()[0]["args"], ["result", "--json", "--", "work-00000001"])
        self.assertTrue(self.records()[0]["binary"].endswith("/ai-personas"))

    def test_path_precedes_checkout_release(self):
        installed = self.fake()
        self.fake(directory=self.checkout / "target/release")
        self.assertEqual(self.run_launcher("a need").returncode, 0)
        self.assertEqual(self.records()[0]["binary"], str(installed))

    def test_release_fallback_preserves_caller_directory(self):
        built = self.fake(directory=self.checkout / "target/release")
        self.assertEqual(self.run_launcher("a need").returncode, 0)
        self.assertEqual(self.records()[0]["binary"], str(built))
        self.assertEqual(self.records()[0]["cwd"], str(self.cwd))
        self.assertEqual(self.records()[0]["home"], str(self.home))

    def test_explicit_binary_precedes_path_and_release(self):
        self.fake()
        self.fake(directory=self.checkout / "target/release")
        selected = self.fake(directory=self.root / "explicit installation")
        self.env["PERSONAS_BIN"] = str(selected)
        self.assertEqual(self.run_launcher("a need").returncode, 0)
        self.assertEqual(self.records()[0]["binary"], str(selected))

    def test_bad_explicit_binary_never_falls_back(self):
        self.fake()
        non_executable = self.root / "not executable"
        non_executable.write_text("not a program")
        for value in ["", "relative/path", "/missing", str(self.bin), str(non_executable)]:
            with self.subTest(value=value):
                self.env["PERSONAS_BIN"] = value
                self.assert_not_run(self.run_launcher("a need"), 126)

    def test_no_implicit_debug_build_fallback(self):
        self.fake(directory=self.checkout / "target/debug")
        self.assert_not_run(self.run_launcher("a need"), 127)

    def native_args(self):
        return [
            "--vault", str(self.root / "unopened vault.json"), "--authorize",
            "--state-root", str(self.root / "unopened state"), "--operator", "owner",
        ]

    def test_native_need_uses_only_explicit_existing_handoff(self):
        self.fake()
        auth = self.fake("personas-auth")
        result = self.run_launcher(*self.native_args(), "--json", "--", "--literal need")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records()[0]["binary"], str(auth))
        self.assertEqual(self.records()[0]["args"], [
            "--vault=" + str(self.root / "unopened vault.json"), "native-run", "--authorize",
            "--state-root=" + str(self.root / "unopened state"), "--operator=owner",
            "--need=--literal need",
        ])
        self.assertFalse((self.root / "unopened vault.json").exists())
        self.assertFalse((self.root / "unopened state").exists())

    def test_native_existing_work_is_not_a_targeted_resume(self):
        self.fake("personas-auth")
        result = self.run_launcher(*self.native_args(), "--resume", "work-00000001")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.records()[0]["args"][-1], "--work=work-00000001")

    def test_native_override_is_separate_from_runtime_override(self):
        self.fake("personas-auth")
        selected = self.fake("personas-auth", self.root / "native installation")
        self.env["PERSONAS_BIN"] = "/missing/runtime"
        self.env["PERSONAS_AUTH_BIN"] = str(selected)
        self.assertEqual(self.run_launcher(*self.native_args(), "a need").returncode, 0)
        self.assertEqual(self.records()[0]["binary"], str(selected))

    def test_missing_native_binary_never_switches_access_route(self):
        self.fake()
        self.assert_not_run(self.run_launcher(*self.native_args(), "a need"), 127)

    def test_invalid_arguments_are_rejected_without_runtime_access(self):
        self.fake()
        self.fake("personas-auth")
        cases = [
            ["--state-root"], ["--operator", ""], ["--state-root", "--json", "a need"],
            ["--state-root", "/a", "--state-root", "/b", "a need"],
            ["--resume", "one", "--result", "two"], ["--resume", "one", "another need"],
            ["--result", "one", "--require-acceptance"],
            ["--resume", "one", "--require-acceptance"],
            ["two", "arguments"], ["--", "one", "two"], ["--model", "invented"],
            ["--provider-config", "/file"], ["--authorize", "a need"],
            ["--vault", "/vault", "a need"],
            ["--vault", "relative", "--authorize", "a need"],
            ["--vault", "/vault", "--authorize", "a need"],
            ["--vault", "/vault", "--authorize", "--state-root", "/state", "a need"],
            self.native_args() + ["--require-acceptance", "a need"],
            self.native_args() + ["--result", "one"],
        ]
        for args in cases:
            with self.subTest(args=args):
                self.assert_not_run(self.run_launcher(*args))

    def test_empty_or_blank_need_never_runs(self):
        self.fake()
        for need in ["", "   ", "\t\n"]:
            with self.subTest(need=need):
                self.assert_not_run(self.run_launcher(need))

    def test_noninteractive_stdin_is_not_an_implicit_request(self):
        self.fake()
        self.assert_not_run(self.run_launcher(input="unsubmitted need\n"))

    def terminal_run(self, data):
        master, slave = pty.openpty()
        try:
            proc = subprocess.Popen(
                # No arguments now enter the integrated dialogue. This fixture
                # retains coverage of the explicit-option legacy prompt path.
                self.command("--operator", "fixture-human"), stdin=slave, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                cwd=self.cwd, env=self.env, text=True,
            )
            try:
                os.write(master, data)
                stdout, stderr = proc.communicate(timeout=5)
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait()
            return subprocess.CompletedProcess(proc.args, proc.returncode, stdout, stderr)
        finally:
            os.close(master)
            os.close(slave)

    def test_terminal_prompt_reads_exactly_one_literal_need(self):
        self.fake()
        result = self.terminal_run(b"a need with \\ and spaces\nsecond line\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("What would you like", result.stderr)
        self.assertEqual(len(self.records()), 1)
        self.assertEqual(self.records()[0]["args"], ["--operator=fixture-human", "run", "--", "a need with \\ and spaces"])

    def test_terminal_eof_does_not_start_work(self):
        self.fake()
        self.assert_not_run(self.terminal_run(b"\x04"))

    def test_terminal_blank_does_not_start_work(self):
        self.fake()
        self.assert_not_run(self.terminal_run(b"   \n"))

    def test_failure_output_and_exit_status_are_preserved_without_retry(self):
        self.fake()
        self.env.update(FAKE_EXIT="23", FAKE_STDOUT='{"status":"held"}\n', FAKE_STDERR="fixture refusal\n")
        result = self.run_launcher("a need")
        self.assertEqual(result.returncode, 23)
        self.assertEqual(result.stdout, '{"status":"held"}\n')
        self.assertEqual(result.stderr, "fixture refusal\n")
        self.assertEqual(len(self.records()), 1)

    def test_held_or_uncertain_output_does_not_trigger_recovery(self):
        self.fake()
        for status in ["held", "paused", "uncertain", "unfinished"]:
            with self.subTest(status=status):
                before = len(self.records())
                output = json.dumps({"status": status}) + "\n"
                self.env["FAKE_STDOUT"] = output
                result = self.run_launcher("--resume", "work-00000001")
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, output)
                self.assertEqual(len(self.records()), before + 1)
                self.assertEqual(self.records()[-1]["args"], ["run", "--resume=work-00000001"])

    def test_native_failure_never_retries_or_uses_ordinary_runtime(self):
        self.fake()
        auth = self.fake("personas-auth")
        self.env["FAKE_EXIT"] = "17"
        result = self.run_launcher(*self.native_args(), "a need")
        self.assertEqual(result.returncode, 17)
        self.assertEqual(len(self.records()), 1)
        self.assertEqual(self.records()[0]["binary"], str(auth))

    def test_exec_preserves_process_identity_and_termination(self):
        self.fake()
        self.env["FAKE_SIGNAL"] = str(int(signal.SIGTERM))
        proc = subprocess.Popen(self.command("a need"), cwd=self.cwd, env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        proc.communicate(timeout=5)
        self.assertEqual(proc.returncode, -signal.SIGTERM)
        self.assertEqual(self.records()[0]["pid"], proc.pid)

    def test_readme_stays_short_and_moved_anchor_is_linked(self):
        readme = (ROOT / "README.md").read_text()
        quickstart = (ROOT / "docs/QUICKSTART.md").read_text()
        self.assertLessEqual(len(readme.split()), 600)
        self.assertIn("docs/interactive-launch.md", readme)
        self.assertIn("../RUNTIME-REFERENCE.md#foreground-cohort-workflow", quickstart)
        self.assertNotIn("../README.md#foreground-cohort-workflow", quickstart)
        self.assertIn("## Foreground cohort workflow", (ROOT / "RUNTIME-REFERENCE.md").read_text())


if __name__ == "__main__":
    unittest.main()
