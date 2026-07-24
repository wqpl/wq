import unittest
from unittest.mock import patch

from release_notes import (
    ReleaseNotesError,
    commit_messages,
    extract_release_notes,
    previous_tag,
    render_release_notes,
)


class ReleaseNotesTests(unittest.TestCase):
    def test_extracts_multiline_notes_in_commit_order(self) -> None:
        messages = [
            """add first feature

Release Notes:

- Added the first feature with a description that
  continues on another line.
""",
            """internal cleanup

Release Notes:

- N/A
""",
            """fix second feature

Release Notes:

- Fixed the second feature.
""",
            "commit without release notes",
        ]

        self.assertEqual(
            extract_release_notes(messages),
            [
                (
                    "- Added the first feature with a description that\n"
                    "  continues on another line."
                ),
                "- Fixed the second feature.",
            ],
        )

    def test_stops_at_content_outside_the_release_notes_list(self) -> None:
        message = """subject

Release Notes:

- Added one feature.

Signed-off-by: Example <example@example.com>
"""

        self.assertEqual(extract_release_notes([message]), ["- Added one feature."])

    def test_renders_a_fallback_when_no_user_facing_notes_exist(self) -> None:
        self.assertEqual(
            render_release_notes([]),
            "No user-facing changes were recorded.\n",
        )

    @patch("release_notes.git_output")
    def test_finds_the_previous_reachable_tag(self, git_output_mock) -> None:
        git_output_mock.return_value = "v1.2.2\n"

        self.assertEqual(previous_tag("v1.2.3"), "v1.2.2")
        git_output_mock.assert_called_once_with(
            "describe", "--tags", "--abbrev=0", "v1.2.3^"
        )

    @patch("release_notes.git_output")
    def test_uses_full_history_when_there_is_no_previous_tag(
        self, git_output_mock
    ) -> None:
        git_output_mock.side_effect = ReleaseNotesError

        self.assertIsNone(previous_tag("v1.0.0"))

    @patch("release_notes.git_output_bytes")
    def test_reads_commits_after_the_previous_tag_in_oldest_first_order(
        self, git_output_mock
    ) -> None:
        git_output_mock.return_value = b"first\0\nsecond\0\n"

        self.assertEqual(
            commit_messages("v1.2.3", "v1.2.2"),
            ["first", "second"],
        )
        git_output_mock.assert_called_once_with(
            "log",
            "--reverse",
            "--format=%B%x00",
            "v1.2.2..v1.2.3",
        )


if __name__ == "__main__":
    unittest.main()
