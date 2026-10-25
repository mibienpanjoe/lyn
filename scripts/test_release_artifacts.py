"""Release transport refuses stale or changed inputs before GitHub mutation."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from release_artifacts import expected_names, stage, verify, publish, project_version

REPO = 'mibienpanjoe/lyn'
SHA = 'a' * 40
VERSION = '0.6.8'


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='lyn-release-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'source'
        self.source.mkdir()
        self.combined = self.root / 'combined'
        self.combined.mkdir()
        for platform in ('linux', 'windows'):
            paths = []
            for name in expected_names(platform, VERSION):
                path = self.source / name
                path.write_bytes(('fixture ' + name).encode())
                paths.append(path)
            destination = self.root / platform
            stage(platform, VERSION, SHA, paths, destination)
            for path in destination.iterdir():
                (self.combined / path.name).write_bytes(path.read_bytes())

    def test_complete_same_commit_artifacts_are_admitted(self):
        version, paths = verify(self.combined, SHA, 'v' + VERSION)
        self.assertEqual(version, VERSION)
        self.assertEqual(len(paths), 6)

    def test_wrong_tag_commit_missing_changed_and_extra_files_refuse(self):
        for sha, tag in [('b' * 40, 'v' + VERSION), (SHA, 'v9.9.9')]:
            with self.assertRaises(ValueError): verify(self.combined, sha, tag)
        file = self.combined / expected_names('windows', VERSION)[0]
        file.write_bytes(b'changed')
        with self.assertRaises(ValueError): verify(self.combined, SHA, 'v' + VERSION)
        file.unlink()
        with self.assertRaises((ValueError, FileNotFoundError)): verify(self.combined, SHA, 'v' + VERSION)

    def test_extra_and_symlink_inputs_refuse(self):
        extra = self.combined / 'unknown.exe'
        extra.write_bytes(b'unknown')
        with self.assertRaises(ValueError): verify(self.combined, SHA, 'v' + VERSION)
        extra.unlink()
        path = self.combined / expected_names('windows', VERSION)[0]
        path.unlink()
        path.symlink_to(self.source / path.name)
        with self.assertRaises(ValueError): verify(self.combined, SHA, 'v' + VERSION)

    def test_metadata_platform_or_version_cannot_be_substituted(self):
        path = self.combined / 'windows-release.json'
        metadata = json.loads(path.read_text())
        for field, value in [('platform', 'linux'), ('version', '9.9.9'), ('sourceSha', 'b' * 40)]:
            path.write_text(json.dumps({**metadata, field: value}))
            with self.assertRaises(ValueError): verify(self.combined, SHA, 'v' + VERSION)

    def test_project_version_requires_all_three_manifests_to_agree(self):
        project = self.root / 'project'
        (project / 'src-tauri').mkdir(parents=True)
        (project / 'package.json').write_text(json.dumps({'version': VERSION}))
        (project / 'src-tauri/tauri.conf.json').write_text(json.dumps({'version': VERSION}))
        (project / 'src-tauri/Cargo.toml').write_text('[package]\nname = "lyn"\nversion = "' + VERSION + '"\n')
        self.assertEqual(project_version(project), VERSION)
        (project / 'package.json').write_text(json.dumps({'version': '9.9.9'}))
        with self.assertRaises(ValueError): project_version(project)

    def test_existing_stage_is_never_adopted(self):
        with self.assertRaises(FileExistsError):
            stage('linux', VERSION, SHA, [], self.combined)

    def test_changed_input_refuses_before_any_network_or_release_action(self):
        (self.combined / expected_names('windows', VERSION)[0]).write_bytes(b'changed')
        with patch('release_artifacts.subprocess.run') as run:
            with self.assertRaises(ValueError): publish(self.combined, SHA, 'v' + VERSION, REPO)
            run.assert_not_called()

    def test_draft_upload_must_finish_before_publication(self):
        def execute(args, **kwargs):
            if args[0] == 'git': return subprocess.CompletedProcess(args, 0, f'{SHA}\trefs/tags/v{VERSION}\n')
            if args[:3] == ['gh', 'release', 'create']:
                notes = Path(args[args.index('--notes-file') + 1]).read_text()
                self.assertIn('\n\n', notes)
                self.assertNotIn('\\n', notes)
            return subprocess.CompletedProcess(args, 0)
        with patch('release_artifacts.subprocess.run', side_effect=execute) as run:
            publish(self.combined, SHA, 'v' + VERSION, REPO)
        calls = [item.args[0] for item in run.call_args_list]
        self.assertEqual(calls[0][:2], ['git', 'ls-remote'])
        self.assertEqual(calls[1][:3], ['gh', 'release', 'create'])
        self.assertIn('--draft', calls[1])
        self.assertIn('--verify-tag', calls[1])
        self.assertEqual(calls[2][:2], ['git', 'ls-remote'])
        self.assertEqual(calls[3], ['gh', 'release', 'edit', 'v' + VERSION, '--draft=false', '--repo', 'github.com/' + REPO])

    def test_tag_changed_during_upload_keeps_the_draft_private(self):
        observations = 0
        def execute(args, **kwargs):
            nonlocal observations
            if args[0] == 'git':
                observations += 1
                value = SHA if observations == 1 else 'b' * 40
                return subprocess.CompletedProcess(args, 0, f'{value}\trefs/tags/v{VERSION}\n')
            return subprocess.CompletedProcess(args, 0)
        with patch('release_artifacts.subprocess.run', side_effect=execute) as run:
            with self.assertRaises(ValueError): publish(self.combined, SHA, 'v' + VERSION, REPO)
            self.assertEqual(run.call_count, 3)
            self.assertNotIn('edit', run.call_args.args[0])

    def test_moved_tag_and_failed_upload_never_promote(self):
        with patch('release_artifacts.subprocess.run', return_value=subprocess.CompletedProcess([], 0, f'{"b" * 40}\trefs/tags/v{VERSION}\n')) as run:
            with self.assertRaises(ValueError): publish(self.combined, SHA, 'v' + VERSION, REPO)
            self.assertEqual(run.call_count, 1)
        def execute(args, **kwargs):
            if args[0] == 'git': return subprocess.CompletedProcess(args, 0, f'{SHA}\trefs/tags/v{VERSION}\n')
            raise subprocess.CalledProcessError(1, args)
        with patch('release_artifacts.subprocess.run', side_effect=execute) as run:
            with self.assertRaises(subprocess.CalledProcessError): publish(self.combined, SHA, 'v' + VERSION, REPO)
            self.assertEqual(run.call_count, 2)


if __name__ == '__main__':
    unittest.main()
