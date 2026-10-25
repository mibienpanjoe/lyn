"""Invalid upgrade inputs must refuse before creating or launching a fixture."""
import contextlib
import io
import sys
import unittest
from unittest.mock import patch

import package_gui_test as fixture
import package_test as native_fixture
from packaged_gui_runtime import preserves_previous


class RoundtripInputs(unittest.TestCase):
    def test_context_changes_during_save_cannot_become_the_new_baseline(self):
        context = dict(id='context', name='Package GUI context', kind='standalone',
                       project_key=None, project_path=None, created_at='first', updated_at='first')
        old = {'old': dict(id='old', text_body='Old note', context_id='context')}
        saved = dict(old, new=dict(id='new', text_body='New note', context_id='context'))
        self.assertTrue(preserves_previous(saved, [context], old, [context]))
        self.assertTrue(preserves_previous(saved, [context], {}, []))
        for key, value in (('id', 'changed'), ('name', 'changed'), ('created_at', 'changed'), ('updated_at', 'changed'),
                           ('kind', 'project'), ('project_key', 'changed'), ('project_path', '/changed')):
            with self.subTest(key=key):
                changed = dict(context, **{key: value})
                self.assertFalse(preserves_previous(saved, [changed], old, [context]))
        changed = dict(saved, old=dict(old['old'], text_body='Changed old note'))
        self.assertFalse(preserves_previous(changed, [context], old, [context]))

    def arguments(self, *previous):
        return ['package_gui_test.py', '--receipt', '/missing/current.json',
                '--portable', '/missing/current.tar.gz', '--deb', '/missing/current.deb', *previous]

    def test_incomplete_previous_inputs_refuse_before_package_reads(self):
        for driver in (fixture, native_fixture):
            for supplied in (['--previous-receipt', '/missing/old.json'],
                             ['--previous-portable', '/missing/old.tar.gz', '--previous-deb', '/missing/old.deb']):
                with self.subTest(supplied=supplied), patch.object(sys, 'argv', self.arguments(*supplied)), \
                        patch.object(driver, 'read_packages') as read, contextlib.redirect_stderr(io.StringIO()) as output:
                    with self.assertRaises(SystemExit) as error:
                        driver.main()
                    self.assertEqual(error.exception.code, 2)
                    self.assertIn('all three previous-package inputs', output.getvalue())
                    read.assert_not_called()

    def test_same_version_refuses_before_any_fixture(self):
        previous = ['--previous-receipt', '/missing/old.json', '--previous-portable',
                    '/missing/old.tar.gz', '--previous-deb', '/missing/old.deb']
        for driver in (fixture, native_fixture):
            with patch.object(sys, 'argv', self.arguments(*previous)), \
                    patch.object(driver, 'read_packages', return_value=({'version': '0.7.0'}, {})), \
                    patch.object(driver.subprocess, 'run') as run, \
                    patch.object(driver.tempfile, 'TemporaryDirectory') as directory, \
                    contextlib.redirect_stderr(io.StringIO()) as output:
                with self.assertRaises(SystemExit) as error:
                    driver.main()
                self.assertEqual(error.exception.code, 2)
                self.assertIn('distinct application versions', output.getvalue())
                run.assert_not_called()
                directory.assert_not_called()

    def test_native_equal_payload_refuses_without_compiling_or_launching(self):
        previous = ['--previous-receipt', '/missing/old.json', '--previous-portable',
                    '/missing/old.tar.gz', '--previous-deb', '/missing/old.deb']
        packages = [({'version': version, 'moduleSha256': 'same'}, {}) for version in ('0.7.0', '0.6.8')]
        with patch.object(sys, 'argv', self.arguments(*previous)), \
                patch.object(native_fixture, 'read_packages', side_effect=packages), \
                patch.object(native_fixture.subprocess, 'run') as run, \
                contextlib.redirect_stderr(io.StringIO()) as output:
            with self.assertRaises(SystemExit) as error:
                native_fixture.main()
            self.assertEqual(error.exception.code, 2)
            self.assertIn('distinct native payloads', output.getvalue())
            run.assert_not_called()



if __name__ == '__main__':
    unittest.main()
