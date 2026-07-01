import argparse
import fcntl
import io
import json
import multiprocessing
import tempfile
import time
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

import hotchoco


class HotchocoHarnessTests(unittest.TestCase):
    def test_expected_exit_code_for_prefers_specific_override(self) -> None:
        testcase = {
            "expected_exit_code": 4,
            "expected_exit_codes": {
                "tiny": 1,
                "tiny/exec": 2,
            },
        }
        mode = {"name": "exec", "expected_exit_code": 3}

        self.assertEqual(hotchoco.expected_exit_code_for(testcase, mode, "tiny"), 2)
        self.assertEqual(hotchoco.expected_exit_code_for(testcase, mode, "other"), 3)
        self.assertEqual(
            hotchoco.expected_exit_code_for(
                {"expected_exit_code": 4}, {"name": "x"}, "y"
            ),
            4,
        )
        self.assertEqual(hotchoco.expected_exit_code_for({}, {"name": "x"}, "y"), 0)

    def test_run_one_test_timeout(self) -> None:
        calls = []

        def fake_run(cmd, **kwargs):
            calls.append((cmd, kwargs))
            return subprocess_result(stdout="ok\n")

        test = {
            "group": "wq",
            "test": "tiny",
            "mode": "exec",
            "source": "tiny.wq",
            "flags": [],
            "capture": "stdout",
            "output_extension": "",
        }
        with tempfile.TemporaryDirectory() as tmp_dir:
            with mock.patch.object(hotchoco.subprocess, "run", side_effect=fake_run):
                hotchoco.run_one_test(test, {}, Path(tmp_dir))

        self.assertEqual(calls[0][1]["timeout"], 30)

    def test_run_one_test_reports_exit_code_mismatch(self) -> None:
        def fake_run(cmd, **kwargs):
            return subprocess_result(stdout="same output\n", returncode=1)

        test = {
            "group": "wq",
            "test": "tiny",
            "mode": "exec",
            "source": "tiny.wq",
            "flags": [],
            "capture": "stdout",
            "output_extension": "",
            "expected_exit_code": 0,
        }
        with tempfile.TemporaryDirectory() as tmp_dir:
            with mock.patch.object(hotchoco.subprocess, "run", side_effect=fake_run):
                result = hotchoco.run_one_test(test, {}, Path(tmp_dir))

            actual = Path(result["output_path"]).read_text()

        self.assertEqual(result["return_code"], 1)
        self.assertEqual(result["expected_exit_code"], 0)
        self.assertIn("same output", actual)
        self.assertNotIn("EXIT CODE MISMATCH", actual)
        self.assertEqual(hotchoco.exit_code_note(result), "exit code 1, expected 0")

    def test_build_wq_cli_uses_debug_build_command(self) -> None:
        with mock.patch.object(hotchoco.subprocess, "run") as run:
            with mock.patch.object(hotchoco.sys, "stderr"):
                run.return_value = subprocess_result(returncode=0)
                hotchoco.build_wq_cli()

        run.assert_called_once_with(
            ["cargo", "build", "-p", "wq-cli"],
            cwd=hotchoco.PROJECT_ROOT,
        )

    def test_compute_diff_labels_changes_without_bare_plus_minus(self) -> None:
        diff = hotchoco.compute_diff("-golden\n=\n", "+actual\n=\n", "wq/tiny/exec")

        self.assertIn("golden: wq/tiny/exec", diff)
        self.assertIn("actual: wq/tiny/exec", diff)
        self.assertIn("-", diff)
        self.assertIn("+", diff)
        self.assertNotIn("\n--golden", diff)
        self.assertNotIn("\n++actual", diff)

    def test_create_output_dir_handles_timestamp_collision(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            suite_dir = Path(tmp_dir) / "suite"
            with (
                mock.patch.object(hotchoco, "SUITE_DIR", suite_dir),
                mock.patch.object(hotchoco, "datetime", FixedDatetime),
            ):
                first = hotchoco.create_output_dir()
                second = hotchoco.create_output_dir()

        self.assertNotEqual(first, second)
        self.assertTrue(first.name.startswith("20260102_030405_000006_"))
        self.assertTrue(second.name.endswith("_1"))

    def test_accept_preserves_pending_state_between_runs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            output_dir = root / "output"
            output_dir.mkdir()
            summary = write_accept_fixture(
                root,
                output_dir,
                [
                    ("wq/one/exec", "old one\n", "new one\n", 0, 0),
                    ("wq/two/exec", "old two\n", "new two\n", 0, 0),
                ],
            )

            with mock.patch.object(
                hotchoco, "latest_output_dir", return_value=output_dir
            ):
                first = io.StringIO()
                with redirect_stdout(first):
                    hotchoco.cmd_accept(mock.Mock(all=False, group=None, test="one"))

                after_first = json.loads((output_dir / "summary.json").read_text())
                self.assertEqual(after_first["wq/one/exec"]["status"], "pass")
                self.assertEqual(after_first["wq/two/exec"]["status"], "fail")
                self.assertIn("1 pending", first.getvalue())

                second = io.StringIO()
                with redirect_stdout(second):
                    hotchoco.cmd_accept(mock.Mock(all=False, group=None, test="two"))

            after_second = json.loads((output_dir / "summary.json").read_text())
            self.assertEqual(after_second["wq/one/exec"]["status"], "pass")
            self.assertEqual(after_second["wq/two/exec"]["status"], "pass")
            self.assertIn("0 pending", second.getvalue())
            self.assertEqual(summary["wq/one/exec"]["status"], "fail")

    def test_accept_keeps_exit_code_mismatch_pending(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            output_dir = root / "output"
            output_dir.mkdir()
            write_accept_fixture(
                root,
                output_dir,
                [("wq/bad/exec", "old\n", "new\n", 1, 0)],
            )

            with mock.patch.object(
                hotchoco, "latest_output_dir", return_value=output_dir
            ):
                first = io.StringIO()
                with redirect_stdout(first):
                    hotchoco.cmd_accept(mock.Mock(all=True, group=None, test=None))

                after_first = json.loads((output_dir / "summary.json").read_text())
                self.assertEqual(after_first["wq/bad/exec"]["status"], "fail")
                self.assertIn("still pending", first.getvalue())
                self.assertIn("1 pending", first.getvalue())

                second = io.StringIO()
                with redirect_stdout(second):
                    hotchoco.cmd_accept(mock.Mock(all=True, group=None, test=None))

            after_second = json.loads((output_dir / "summary.json").read_text())
            self.assertEqual(after_second["wq/bad/exec"]["status"], "fail")
            self.assertIn("No snapshot changes to accept", second.getvalue())
            self.assertIn("1 pending", second.getvalue())

    def test_accept_reloads_summary_after_waiting_for_lock(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            suite_dir = root / "suite"
            output_dir = suite_dir / "output" / "run"
            output_dir.mkdir(parents=True)
            write_accept_fixture(
                root,
                output_dir,
                [
                    ("wq/one/exec", "old one\n", "new one\n", 0, 0),
                    ("wq/two/exec", "old two\n", "new two\n", 0, 0),
                ],
            )

            ready_path = root / "accept-ready"
            process = multiprocessing.get_context("spawn").Process(
                target=accept_in_child,
                args=(str(suite_dir), "two", str(ready_path)),
            )

            with open(hotchoco.summary_lock_path(output_dir), "a") as lock_file:
                fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX)
                try:
                    process.start()
                    wait_for_path(ready_path)
                    time.sleep(0.1)

                    summary = hotchoco.read_summary(output_dir)
                    hotchoco.accept_snapshot_change(summary["wq/one/exec"])
                    hotchoco.write_summary(output_dir, summary)
                finally:
                    fcntl.flock(lock_file.fileno(), fcntl.LOCK_UN)

            process.join(5)
            if process.is_alive():
                process.terminate()
                process.join()
                self.fail("child accept process did not finish")

            self.assertEqual(process.exitcode, 0)
            after = hotchoco.read_summary(output_dir)
            self.assertEqual(after["wq/one/exec"]["status"], "pass")
            self.assertEqual(after["wq/two/exec"]["status"], "pass")


def subprocess_result(stdout="", stderr="", returncode=0):
    return mock.Mock(stdout=stdout, stderr=stderr, returncode=returncode)


class FixedDatetime:
    @classmethod
    def now(cls):
        return cls()

    def strftime(self, _fmt):
        return "20260102_030405_000006"


def write_accept_fixture(root, output_dir, entries):
    summary = {}
    for key, expected_text, actual_text, return_code, expected_exit_code in entries:
        actual_path = root / "actual" / key
        expected_path = root / "golden" / key
        actual_path.parent.mkdir(parents=True, exist_ok=True)
        expected_path.parent.mkdir(parents=True, exist_ok=True)
        actual_path.write_text(actual_text)
        expected_path.write_text(expected_text)
        summary[key] = {
            "status": "fail",
            "output_path": str(actual_path),
            "expected_path": str(expected_path),
            "return_code": return_code,
            "expected_exit_code": expected_exit_code,
        }
    (output_dir / "summary.json").write_text(json.dumps(summary, indent=2))
    return summary


def accept_in_child(suite_dir, test_sel, ready_path):
    hotchoco.SUITE_DIR = Path(suite_dir)
    Path(ready_path).write_text("ready")
    args = argparse.Namespace(all=False, group=None, test=test_sel)
    with redirect_stdout(io.StringIO()):
        hotchoco.cmd_accept(args)


def wait_for_path(path):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.01)
    raise AssertionError(f"timed out waiting for {path}")


if __name__ == "__main__":
    unittest.main()
