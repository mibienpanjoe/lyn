"""A fresh checkout must retain diagnostics without pre-existing local tasks."""
from pathlib import Path
import tempfile
import unittest
from package_gui_test import preserve_diagnostics


class DiagnosticTests(unittest.TestCase):
    def test_missing_artifact_directory_is_created_and_existing_files_preserved(self):
        with tempfile.TemporaryDirectory(prefix='lyn-gui-artifact-test-') as folder:
            root = Path(folder)
            output = root / 'output'
            output.mkdir()
            (output / 'app.log').write_bytes(b'owned log')
            (output / 'saved.png').write_bytes(b'owned image')
            destination = root / 'tasks'
            first = preserve_diagnostics(output, 'portable', destination)
            self.assertEqual(sorted(path.read_bytes() for path in first), [b'owned image', b'owned log'])
            second = preserve_diagnostics(output, 'portable', destination)
            self.assertTrue(set(first).isdisjoint(second))
            self.assertEqual(len(list(destination.iterdir())), 4)

    def test_symlink_destination_or_source_is_refused(self):
        with tempfile.TemporaryDirectory(prefix='lyn-gui-artifact-test-') as folder:
            root = Path(folder)
            output = root / 'output'
            output.mkdir()
            target = root / 'actual'
            target.mkdir()
            destination = root / 'tasks'
            destination.symlink_to(target, target_is_directory=True)
            with self.assertRaises(ValueError): preserve_diagnostics(output, 'portable', destination)
            destination.unlink()
            (output / 'app.log').symlink_to(root / 'external')
            with self.assertRaises(ValueError): preserve_diagnostics(output, 'portable', destination)
            self.assertFalse(list(target.iterdir()))


if __name__ == '__main__':
    unittest.main()
