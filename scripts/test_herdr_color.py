import contextlib
import io
import os
from pathlib import Path
import re
import tempfile
import tomllib
import unittest
from unittest.mock import patch

from scripts import herdr_color as command


class PaneColorCommandTests(unittest.TestCase):
    def test_preset_contract_matches_client_and_backgrounds_are_nearly_black(self):
        source = (Path(__file__).resolve().parent.parent / "src/client/shell/pane_colors.rs").read_text()
        table = source.split("const AUTOMATIC:", 1)[1].split("];", 1)[0]
        colors = re.findall(r"\(0x02([0-9a-f]{6}), 0x02([0-9a-f]{6})\)", table)
        self.assertEqual(colors, [(bg[1:].lower(), border[1:].lower()) for bg, border in command.PRESETS.values()])
        self.assertIn("0x02" + command.TEXT[1:].lower(), source)
        for background, _ in command.PRESETS.values():
            self.assertLessEqual(max(bytes.fromhex(background[1:])), 11)

    def test_preserves_comments_and_all_unmanaged_values(self):
        base = '# NERV / MAGI\n[theme.custom]\ntext = "#FFC98A"\n# keep this\n[[ui.pane_colors.rules]]\npane = "manual"\nbackground = "#112233"\n'
        after = command.updated_content(base, "reviewer", "purple")
        self.assertTrue(after.startswith(base))
        actual = tomllib.loads(after)
        self.assertEqual(actual["theme"], tomllib.loads(base)["theme"])
        self.assertEqual(len(actual["ui"]["pane_colors"]["rules"]), 2)
        self.assertEqual(actual["ui"]["pane_colors"]["rules"][0], tomllib.loads(base)["ui"]["pane_colors"]["rules"][0])

    def test_replacement_and_reset_only_change_requested_managed_rule(self):
        config = command.updated_content("[ui.pane_colors]\nautomatic = true\n", "a", "blue")
        config = command.updated_content(config, "b", "green")
        config = command.updated_content(config, "a", "red")
        rules = tomllib.loads(config)["ui"]["pane_colors"]["rules"]
        self.assertEqual(len(rules), 2)
        self.assertEqual(rules[1]["background"], command.PRESETS["red"][0])
        config = command.updated_content(config, "a", "reset")
        self.assertEqual(tomllib.loads(config)["ui"]["pane_colors"]["rules"][0]["pane"], "b")
        self.assertEqual(command.updated_content(config, "b", "reset"), "[ui.pane_colors]\nautomatic = true\n")

    def test_quotes_umlauts_custom_colors_and_optional_channels(self):
        after = command.updated_content("", 'Prüfung "A"', "#07080b", "#fedcba", "#123abc")
        rule = tomllib.loads(after)["ui"]["pane_colors"]["rules"][0]
        self.assertEqual(rule, {"pane": 'Prüfung "A"', "foreground": "#FEDCBA", "background": "#07080B", "border": "#123ABC"})

    def test_rejects_invalid_colors_and_damaged_config(self):
        for value in ["blue-ish", "#fff", "#gg1234", "#123456\n", "#üüüüüü"]:
            with self.assertRaises(ValueError):
                command.updated_content("", "test", value)
        with self.assertRaises(tomllib.TOMLDecodeError):
            command.updated_content("[broken", "test", "blue")

    def test_refuses_to_drop_unrelated_data_inside_or_after_managed_block(self):
        config = command.updated_content("", "a", "blue")
        for bad in [config + "[terminal]\ndefault_shell='zsh'\n", config.replace(command.END, "[terminal]\ndefault_shell='zsh'\n" + command.END), config.replace(command.END, "")]:
            with self.assertRaises(ValueError):
                command.updated_content(bad, "a", "reset")

    def test_atomic_save_backup_and_mode_and_no_change_on_invalid_input(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "config.toml"
            original = b'# custom\n[theme]\nname="vesper"\n'
            path.write_bytes(original)
            path.chmod(0o640)
            backup = command.save(path, "a", "blue")
            self.assertEqual(backup.read_bytes(), original)
            self.assertEqual(backup.stat().st_mode & 0o777, 0o600)
            self.assertEqual(path.stat().st_mode & 0o777, 0o640)
            saved = path.read_bytes()
            with self.assertRaises(ValueError):
                command.save(path, "a", "#BAD")
            self.assertEqual(path.read_bytes(), saved)
            self.assertIsNone(command.save(path, "a", "blue"))

    def test_managed_comment_is_preserved_by_refusing_before_write(self):
        config = command.updated_content("", "a", "blue")
        config = config.replace('background =', '# manual annotation\nbackground =')
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "config.toml"
            path.write_text(config)
            with self.assertRaises(ValueError):
                command.save(path, "b", "green")
            self.assertEqual(path.read_text(), config)

    def test_utf8_bom_is_preserved(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "config.toml"
            path.write_bytes(b"\xef\xbb\xbf# NERV\n")
            command.save(path, "a", "blue")
            self.assertTrue(path.read_bytes().startswith(b"\xef\xbb\xbf# NERV\n"))
            tomllib.loads(path.read_bytes().decode("utf-8-sig"))

    def test_current_is_resolved_by_server_and_failure_does_not_write(self):
        with tempfile.TemporaryDirectory() as folder, contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            path = Path(folder) / "config.toml"
            with patch.dict(os.environ, {"HERDR_ENV": "1", "HERDR_PANE_ID": "old-id"}):
                with patch.object(command, "request", side_effect=ValueError("pane not found")):
                    self.assertEqual(command.main(["--config", str(path), "--current", "blue"]), 2)
                self.assertFalse(path.exists())
                with patch.object(command, "request", return_value={"pane": {"pane_id": "w2:pA"}}) as request:
                    self.assertEqual(command.main(["--config", str(path), "--current", "blue", "--no-reload"]), 0)
                    request.assert_called_once_with(["pane", "current", "--pane", "old-id"])
                self.assertEqual(tomllib.loads(path.read_text())["ui"]["pane_colors"]["rules"][0]["pane"], "w2:pA")

    def test_whitespace_caller_never_falls_back_to_focused_pane(self):
        with tempfile.TemporaryDirectory() as folder, contextlib.redirect_stderr(io.StringIO()):
            path = Path(folder) / "config.toml"
            with patch.dict(os.environ, {"HERDR_ENV": "1", "HERDR_PANE_ID": "  "}), patch.object(command, "request") as request:
                self.assertEqual(command.main(["--config", str(path), "--current", "blue"]), 2)
                request.assert_not_called()
                self.assertFalse(path.exists())

    def test_reload_failure_preserves_successful_saved_rule(self):
        with tempfile.TemporaryDirectory() as folder, contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            path = Path(folder) / "config.toml"
            with patch.object(command, "request", side_effect=ValueError("server unavailable")):
                self.assertEqual(command.main(["--config", str(path), "--name", "reviewer", "teal"]), 0)
            self.assertEqual(tomllib.loads(path.read_text())["ui"]["pane_colors"]["rules"][0]["pane"], "reviewer")


if __name__ == "__main__":
    unittest.main()
