import tempfile
import unittest
from pathlib import Path
from unittest import mock

import wqbench


class WqbenchHarnessTests(unittest.TestCase):
    def test_run_hyperfine_skips_nonzero_wq_exit(self) -> None:
        with tempfile.TemporaryDirectory(dir=wqbench.PROJECT_ROOT) as tmp_dir:
            tmp_path = Path(tmp_dir)
            script = tmp_path / "boom.wq"
            script.write_text('raise "boom"\n')
            benches_dir = tmp_path / "benches"
            benches_dir.mkdir()

            preflight = mock.Mock(returncode=1, stdout="", stderr="boom\n")
            with mock.patch.object(
                wqbench.subprocess,
                "run",
                return_value=preflight,
            ) as run:
                rows, skipped = wqbench.run_hyperfine_individual(
                    [script],
                    benches_dir=benches_dir,
                    binary_path=Path("target/debug/wq"),
                    warmup=0,
                    min_runs=1,
                )

        self.assertEqual(rows, [])
        self.assertEqual(len(skipped), 1)
        self.assertEqual(skipped[0]["benchmark"], "boom")
        self.assertEqual(skipped[0]["reason"], "wq exited with code 1")
        self.assertIn("stderr: boom", skipped[0]["detail"])
        run.assert_called_once()


if __name__ == "__main__":
    unittest.main()
