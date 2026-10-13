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


class AttributionTests(unittest.TestCase):
    def test_codex_solo(self):
        self.assertEqual(module.validate("0x0sky@aiaiaiai.org", "fix: example\n\nAssisted-by: OpenAI Codex\n"), [])

    def test_joint_in_both_directions(self):
        for author, other in [("0x0sky", "x0skai"), ("x0skai", "0x0sky")]:
            message = f"fix: example\n\nCo-authored-by: {other} <{other}@aiaiaiai.org>\nCo-authored-by: Claude <noreply@anthropic.com>\nAssisted-by: OpenAI Codex\n"
            self.assertEqual(module.validate(f"{author}@aiaiaiai.org", message), [])

    def test_missing_tool_credit(self):
        for author in module.ACCOUNTS.values():
            self.assertTrue(module.validate(author, "fix: example\n"))

    def test_wrong_email(self):
        self.assertTrue(module.validate("x0skai@aiaiaiai.org", "fix: example\n\nCo-authored-by: 0x0sky <0x0skai@aiaiaiai.org>\nCo-authored-by: Claude <noreply@anthropic.com>\n"))

    def test_self_and_duplicate(self):
        for tail in ["Co-authored-by: 0x0sky <0x0sky@aiaiaiai.org>\n", "Assisted-by: OpenAI Codex\n"]:
            self.assertTrue(module.validate("0x0sky@aiaiaiai.org", "fix: example\n\nAssisted-by: OpenAI Codex\n" + tail))

    def test_body_is_not_trailer(self):
        self.assertTrue(module.validate("0x0sky@aiaiaiai.org", "fix: example\n\nAssisted-by: OpenAI Codex\n\nMore prose.\n"))

    def test_external_contributor_is_not_forced_to_credit_tools(self):
        self.assertEqual(module.validate("human@example.org", "fix: example\n"), [])

    def test_verified_api_identity_still_requires_codex_credit(self):
        email = "75973992+0x0sky@users.noreply.github.com"
        self.assertEqual(module.validate(email, "fix: example\n\nAssisted-by: OpenAI Codex\n"), [])
        self.assertTrue(module.validate(email, "fix: example\n"))


if __name__ == "__main__":
    unittest.main()
