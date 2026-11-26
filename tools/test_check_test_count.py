# =============================================================================
# EH3D-PHOENIX-CAD - tools/test_check_test_count.py
# Copyright (C) 2026 JuanenRac (Electro Hobby 3D) <electrohobby3d@gmail.com>
# GPL-3.0-or-later - see LICENSE
# =============================================================================
"""The count of tests: logs made with a fixed seed add up, and a wrong or partial log is refused."""
from __future__ import annotations

import random
import unittest

import check_test_count as count


class CountTests(unittest.TestCase):
    def test_logs_add_up_and_a_wrong_log_or_readme_is_refused(self) -> None:
        seeded = random.Random(2026)
        for _ in range(50):
            parts = [seeded.randrange(0, 2000) for _ in range(seeded.randrange(1, 12))]
            log = "   Doc-tests x\n" + "".join(f"test result: ok. {p} passed; 0 failed; 0 ignored\n" for p in parts)
            self.assertEqual(count.measured(log), sum(parts))
        for wrong in ("", "test result: ok. 3 passed; 0 failed", "Doc-tests x\ntest result: FAILED. 3 passed; 1 failed"):
            with self.assertRaises(ValueError):
                count.measured(wrong)
        good = "There are 70 tests (`cargo test`).\n| 70 |\nThe 70 tests.\n"
        self.assertEqual(count.problems(70, {"a": good}), [])
        self.assertEqual(len(count.problems(71, {"a": good})), 2)
        self.assertEqual(len(count.problems(7, {"a": good})), 2)


if __name__ == "__main__":
    unittest.main()
