"""Safe, fixed startup recipes for native foreground-shape acceptance."""

from pathlib import Path
import unittest

from foreground_fixture import recipe


class ForegroundRecipeTest(unittest.TestCase):
    def test_all_shapes_disable_profiles_and_inherited_shell_configuration(self):
        for shape in ("shell", "exec-tui", "child-tui"):
            args, environment = recipe(shape, Path("/private/beta"), Path("/private/config"))
            self.assertEqual(args[:4], ["/bin/bash", "--noprofile", "--norc", "-c"])
            self.assertEqual(set(environment), {"PATH", "TERM", "LANG", "HTOPRC", "XDG_CONFIG_HOME"})
            self.assertEqual(environment["HTOPRC"], "/private/config/fixture-htoprc")

    def test_paths_stay_positional_and_tui_output_is_pid_filtered(self):
        target = Path("/private/beta with $() and ' quotes")
        for shape in ("shell", "exec-tui", "child-tui"):
            args, _ = recipe(shape, target, Path("/private/config"))
            self.assertEqual(args[-1], str(target))
            self.assertNotIn(str(target), args[4])
            if shape != "shell":
                self.assertIn('--pid "$$"', args[4])
        with self.assertRaises(AssertionError):
            recipe("arbitrary-command", target, Path("/private/config"))

    def test_exit_shell_waits_on_private_fifo_not_terminal_input(self):
        args, environment = recipe("exit-shell", Path("/private/beta"), Path("/private/config"))
        self.assertEqual(args[:4], ["/bin/bash", "--noprofile", "--norc", "-c"])
        self.assertEqual(args[4], 'IFS= read -r < "$2"')
        self.assertEqual(args[-2:], ["/private/beta", "/private/runtime/shell-exit.fifo"])
        self.assertNotIn("BASH_ENV", environment)
        self.assertNotIn("ENV", environment)


if __name__ == "__main__":
    unittest.main()
