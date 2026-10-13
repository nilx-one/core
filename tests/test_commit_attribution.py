# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: MPL-2.0
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "attribution", Path(__file__).parents[1] / "scripts/check_commit_attribution.py"
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
ZERO, OTHER = module.ACCOUNTS["0x0sky"], module.ACCOUNTS["x0skai"]


class AttributionTests(unittest.TestCase):
    def test_solo_account_needs_no_tool_credit(self):
        self.assertEqual(module.validate(ZERO, "fix: example\n"), [])

    def test_codex_credit(self):
        self.assertEqual(module.validate(ZERO, "fix: example\n\nAssisted-by: OpenAI Codex\n"), [])

    def test_claude_credit_from_api_identity_is_not_forced_to_codex(self):
        message = "fix: example\n\nCo-authored-by: Claude <noreply@anthropic.com>\n"
        self.assertEqual(module.validate(ZERO, message), [])

    def test_joint_in_both_directions(self):
        for author, other in [("0x0sky", "x0skai"), ("x0skai", "0x0sky")]:
            message = (
                f"fix: example\n\nCo-authored-by: {other} <{module.ACCOUNTS[other]}>\n"
                "Co-authored-by: Claude <noreply@anthropic.com>\nAssisted-by: OpenAI Codex\n"
            )
            self.assertEqual(module.validate(module.ACCOUNTS[author], message), [])

    def test_wrong_account_address(self):
        self.assertTrue(module.validate(OTHER, f"fix: example\n\nCo-authored-by: x0skai <{ZERO}>\n"))

    def test_self_and_duplicate(self):
        self.assertTrue(module.validate(ZERO, f"fix: example\n\nCo-authored-by: 0x0sky <{ZERO}>\n"))
        self.assertTrue(module.validate(ZERO, "fix: example\n\nAssisted-by: OpenAI Codex\nAssisted-by: OpenAI Codex\n"))

    def test_unknown_tool_name_rejected(self):
        self.assertTrue(module.validate(ZERO, "fix: example\n\nAssisted-by: OpenAI ChatGPT\n"))

    def test_external_contributor(self):
        self.assertEqual(module.validate("human@example.org", "fix: example\n"), [])


if __name__ == "__main__":
    unittest.main()
