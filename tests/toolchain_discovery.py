#!/usr/bin/env python3
"""Bounded Rustup discovery tests; fake compiler scripts only, no real toolchain."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("personas_dialogue", ROOT / "scripts" / "personas_dialogue.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ToolchainDiscoveryTests(unittest.TestCase):
    def make_toolchain(self, root, name, rustc_body=None):
        bindir = root / "toolchains" / name / "bin"
        bindir.mkdir(parents=True)
        cargo = bindir / "cargo"
        cargo.write_text("#!/bin/sh\nexit 0\n")
        cargo.chmod(0o755)
        rustc = bindir / "rustc"
        rustc.write_text("#!" + sys.executable + "\n" + (rustc_body or "print('rustc 1.90.0 (synthetic)')\n"))
        rustc.chmod(0o755)
        return bindir

    def test_stable_channel_discovered_without_preconsent_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp) / "rustup"
            bindir = self.make_toolchain(home, "stable-x86_64-unknown-linux-gnu")
            with patch.dict(os.environ, {"HOME": tmp, "RUSTUP_HOME": str(home), "PATH": ""}, clear=True), \
                 patch.object(m.shutil, "which", return_value=None), patch.object(m.subprocess, "Popen") as run:
                self.assertEqual(m.discover_cargo(), [(str(bindir / "cargo"), str(bindir))])
                run.assert_not_called()

    def test_unusable_high_candidate_falls_back_to_supported_lower(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "rustup"
            high = self.make_toolchain(root, "1.91.0-x86_64-unknown-linux-gnu", "raise SystemExit(9)\n")
            low = self.make_toolchain(root, "1.90.1-x86_64-unknown-linux-gnu")
            candidates = [(str(high / "cargo"), str(high)), (str(low / "cargo"), str(low))]
            selected = m.select_supported_cargo(candidates, {"PATH": os.environ.get("PATH", "")}, tmp)
            self.assertEqual(selected[:2], candidates[1])
            self.assertTrue(selected[2]["PATH"].startswith(str(low) + os.pathsep))

    def test_probe_uses_the_exact_build_environment_and_checkout_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bindir = self.make_toolchain(root / "rustup", "1.90.0-x86_64-unknown-linux-gnu")
            report = root / "probe.json"
            rustc = bindir / "rustc"
            rustc.write_text(
                "#!" + sys.executable + "\n"
                "import json, os\n"
                f"open({str(report)!r}, 'w').write(json.dumps(dict(cwd=os.getcwd(), cargo_home=os.environ.get('CARGO_HOME'), rustup_home=os.environ.get('RUSTUP_HOME'), target=os.environ.get('CARGO_TARGET_DIR'))))\n"
                "print('rustc 1.90.0 (synthetic)')\n"
            )
            rustc.chmod(0o755)
            env = {"PATH": os.environ.get("PATH", ""), "CARGO_HOME": "/synthetic/cargo-home",
                   "RUSTUP_HOME": "/synthetic/rustup-home", "CARGO_TARGET_DIR": "/synthetic/build"}
            selected = m.select_supported_cargo([(str(bindir / "cargo"), str(bindir))], env, str(root))
            self.assertIsNotNone(selected)
            evidence = __import__("json").loads(report.read_text())
            self.assertEqual(evidence, {"cwd": str(root), "cargo_home": env["CARGO_HOME"],
                                        "rustup_home": env["RUSTUP_HOME"], "target": env["CARGO_TARGET_DIR"]})
            self.assertEqual(selected[2]["CARGO_HOME"], env["CARGO_HOME"])
            self.assertEqual(selected[2]["RUSTUP_HOME"], env["RUSTUP_HOME"])
            self.assertEqual(selected[2]["CARGO_TARGET_DIR"], env["CARGO_TARGET_DIR"])

    def test_minimum_version_and_prerelease_are_rejected(self):
        self.assertFalse(m._parse_rustc_version("rustc 1.87.9 (x)"))
        self.assertFalse(m._parse_rustc_version("rustc 1.88.0-nightly (x)"))
        self.assertTrue(m._parse_rustc_version("rustc 1.88.0 (x)"))
        self.assertFalse(m._parse_rustc_version("rustc 1.90.0 " + "x" * 5000))

    def test_configured_root_is_authoritative(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            configured = root / "configured"
            configured.mkdir()
            self.make_toolchain(root / "home" / ".rustup", "stable-x86_64-unknown-linux-gnu")
            with patch.dict(os.environ, {"HOME": str(root / "home"), "RUSTUP_HOME": str(configured), "PATH": ""}, clear=True), \
                 patch.object(m.shutil, "which", return_value=None):
                self.assertEqual(m.discover_cargo(), [])

    def test_path_precedence_and_no_preconsent_execution(self):
        with patch.object(m.shutil, "which", return_value="/trusted/bin/cargo"), patch.object(m.subprocess, "Popen") as run:
            self.assertEqual(m.discover_cargo()[0], ("/trusted/bin/cargo", None))
            run.assert_not_called()

    def test_directory_enumeration_limit_fails_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "rustup"
            toolchains = root / "toolchains"
            toolchains.mkdir(parents=True)
            for i in range(m.MAX_TOOLCHAIN_DIRECTORY_ENTRIES + 1):
                (toolchains / f"irrelevant-{i:03d}").mkdir()
            with patch.dict(os.environ, {"HOME": tmp, "RUSTUP_HOME": str(root), "PATH": ""}, clear=True), \
                 patch.object(m.shutil, "which", return_value=None):
                with self.assertRaisesRegex(m.Blocked, "rustup_toolchain_directory_limit"):
                    m.discover_cargo()

    def _pid_running(self, pid):
        stat_path = Path(f"/proc/{pid}/stat")
        try:
            fields = stat_path.read_text().split()
            return fields[2] != "Z"
        except FileNotFoundError:
            return False

    def test_parent_exit_with_inherited_pipe_times_out_and_kills_descendant(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            pidfile = root / "child.pid"
            script = root / "fake-rustc"
            script.write_text(
                "#!" + sys.executable + "\n"
                "import subprocess, sys, time\n"
                f"child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\n"
                f"open({str(pidfile)!r}, 'w').write(str(child.pid))\n"
                "print('rustc 1.90.0 (synthetic)', flush=True)\n"
                "sys.exit(0)\n"
            )
            script.chmod(0o755)
            started = time.monotonic()
            accepted = m._supported_rustc(str(script), os.environ.copy(), started + 0.6, tmp)
            elapsed = time.monotonic() - started
            self.assertFalse(accepted)
            self.assertLess(elapsed, 2.0, "probe exceeded its deadline")
            pid = int(pidfile.read_text())
            limit = time.monotonic() + 1.5
            while self._pid_running(pid) and time.monotonic() < limit:
                time.sleep(0.02)
            self.assertFalse(self._pid_running(pid), "descendant survived process-group cleanup")

    def test_oversized_output_is_rejected_and_process_group_cleaned(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            pidfile = root / "child.pid"
            script = root / "fake-rustc"
            script.write_text(
                "#!" + sys.executable + "\n"
                "import subprocess, sys, time\n"
                f"child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\n"
                f"open({str(pidfile)!r}, 'w').write(str(child.pid))\n"
                "sys.stdout.write('X' * 20000); sys.stdout.flush()\n"
                "time.sleep(30)\n"
            )
            script.chmod(0o755)
            started = time.monotonic()
            accepted = m._supported_rustc(str(script), os.environ.copy(), started + 2.0, tmp)
            self.assertFalse(accepted)
            self.assertLess(time.monotonic() - started, 2.5)
            pid = int(pidfile.read_text())
            limit = time.monotonic() + 1.5
            while self._pid_running(pid) and time.monotonic() < limit:
                time.sleep(0.02)
            self.assertFalse(self._pid_running(pid), "descendant survived output-limit cleanup")


if __name__ == "__main__":
    unittest.main(verbosity=2)
