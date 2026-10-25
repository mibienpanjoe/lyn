"""Stage exact same-commit build outputs and publish only a complete release.

Linux native admission/runtime checks belong to the preceding build job. These
manifests bind their artifact transport; they are not native install authority.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = re.compile(r'[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?')
SHA = re.compile(r'[0-9a-f]{40}')
MAX_ASSET = 512 * 1024 * 1024


def project_version(root=ROOT):
    version = json.loads((root / 'src-tauri/tauri.conf.json').read_text())['version']
    package = json.loads((root / 'package.json').read_text())['version']
    cargo = (root / 'src-tauri/Cargo.toml').read_text().split('[package]', 1)[1].split('\n[', 1)[0]
    match = re.search(r'^version\s*=\s*"([^"]+)"\s*(?:#.*)?$', cargo, re.MULTILINE)
    if not isinstance(version, str) or not VERSION.fullmatch(version) or package != version or not match or match[1] != version:
        raise ValueError('Project version manifests disagree or are unsupported')
    return version


def expected_names(platform, version):
    if not VERSION.fullmatch(version): raise ValueError('Invalid release version')
    if platform == 'linux':
        return [f'Lyn_{version}_amd64.deb', f'lyn-{version}-linux-x86_64.tar.gz',
                f'lyn-{version}-linux-x86_64-native.json']
    if platform == 'windows': return [f'Lyn_{version}_x64-setup.exe']
    raise ValueError('Unsupported release platform')


def file_record(path):
    if path.is_symlink() or not path.is_file(): raise ValueError('Regular release files are required')
    size = path.stat().st_size
    if not 0 < size <= MAX_ASSET: raise ValueError('Invalid release file size')
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for part in iter(lambda: source.read(1024 * 1024), b''): digest.update(part)
    return dict(name=path.name, length=size, sha256=digest.hexdigest())


def stage(platform, version, sha, files, destination):
    if destination.exists() or destination.is_symlink(): raise FileExistsError(destination)
    if not SHA.fullmatch(sha): raise ValueError('Invalid source commit')
    if sorted(path.name for path in files) != sorted(expected_names(platform, version)):
        raise ValueError('Expected the exact current-version release files')
    for path in files: file_record(path)
    destination.mkdir(parents=True)
    records = []
    for source in files:
        copied = destination / source.name
        shutil.copyfile(source, copied)
        records.append(file_record(copied))
    metadata = dict(schema=1, platform=platform, version=version, sourceSha=sha, files=records)
    (destination / f'{platform}-release.json').write_text(json.dumps(metadata, sort_keys=True) + '\n')


def verify(directory, sha, tag):
    if not SHA.fullmatch(sha) or directory.is_symlink(): raise ValueError('Invalid release input')
    assets, version, names = [], None, set()
    for platform in ('linux', 'windows'):
        metadata_path = directory / f'{platform}-release.json'
        if metadata_path.is_symlink(): raise ValueError('Release metadata must not be a symlink')
        with metadata_path.open('rb') as source: data = source.read(16385)
        if len(data) > 16384: raise ValueError('Oversized release metadata')
        metadata = json.loads(data)
        if (set(metadata) != {'schema', 'platform', 'version', 'sourceSha', 'files'} or metadata['schema'] != 1
                or metadata['platform'] != platform or metadata['sourceSha'] != sha):
            raise ValueError('Release metadata does not bind this platform/commit')
        current = metadata['version']
        if (version is not None and version != current) or tag != 'v' + current:
            raise ValueError('Release version/tag mismatch')
        version = current
        expected = expected_names(platform, version)
        if not isinstance(metadata['files'], list) or len(metadata['files']) != len(expected):
            raise ValueError('Incomplete release files')
        if sorted(item['name'] for item in metadata['files']) != sorted(expected):
            raise ValueError('Unknown, duplicate or missing release files')
        for item in metadata['files']:
            if set(item) != {'name', 'length', 'sha256'} or type(item['length']) is not int:
                raise ValueError('Invalid release file record')
            path = directory / item['name']
            if file_record(path) != item: raise ValueError('Release artifact changed after verification')
            assets.append(path)
            names.add(path.name)
        assets.append(metadata_path)
        names.add(metadata_path.name)
    if {path.name for path in directory.iterdir()} != names: raise ValueError('Unknown release-stage entries')
    return version, sorted(assets)


def require_remote_tag(sha, tag, repository):
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*', repository):
        raise ValueError('Invalid GitHub repository')
    ref = 'refs/tags/' + tag
    result = subprocess.run(['git', 'ls-remote', '--exit-code', f'https://github.com/{repository}.git', ref, ref + '^{}'],
                            check=True, stdout=subprocess.PIPE, text=True, timeout=30)
    refs = {}
    for line in result.stdout.splitlines():
        fields = line.split()
        if len(fields) != 2 or not SHA.fullmatch(fields[0]) or fields[1] not in (ref, ref + '^{}') or fields[1] in refs:
            raise ValueError('Unexpected tag observation')
        refs[fields[1]] = fields[0]
    if ref not in refs or refs.get(ref + '^{}', refs.get(ref)) != sha:
        raise ValueError('Release tag no longer identifies the verified source commit')


def publish(directory, sha, tag, repository):
    version, assets = verify(directory, sha, tag)
    require_remote_tag(sha, tag, repository)
    with tempfile.TemporaryDirectory(prefix='lyn-release-notes-') as folder:
        notes = Path(folder) / 'notes.md'
        notes.write_text(f'Unsigned prerelease artifacts from commit `{sha}`.\n\n'
                         'Linux: .deb and portable archive with the experimental GTK 3.24.33 / VTE 0.68.0 native GNOME integration. '
                         'Activation is explicit in Settings; open terminals are never restarted automatically.\n\n'
                         'Windows: experimental NSIS installer. Local speech remains an optional Settings download.\n')
        # No overwrite/clobber: existing releases, including interrupted drafts,
        # refuse for explicit review instead of replacing published assets.
        subprocess.run(['gh', 'release', 'create', tag, *map(str, assets), '--repo', 'github.com/' + repository,
                        '--verify-tag', '--draft', '--prerelease', '--title', f'Lyn v{version}', '--notes-file', str(notes)],
                       check=True, timeout=120)
        require_remote_tag(sha, tag, repository)
        subprocess.run(['gh', 'release', 'edit', tag, '--draft=false', '--repo', 'github.com/' + repository], check=True, timeout=30)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    version_parser = commands.add_parser('version')
    version_parser.add_argument('--ref', required=True)
    stage_parser = commands.add_parser('stage')
    stage_parser.add_argument('--platform', choices=('linux', 'windows'), required=True)
    stage_parser.add_argument('--sha', required=True)
    stage_parser.add_argument('--destination', required=True, type=Path)
    publish_parser = commands.add_parser('publish')
    publish_parser.add_argument('--directory', required=True, type=Path)
    publish_parser.add_argument('--sha', required=True)
    publish_parser.add_argument('--tag', required=True)
    publish_parser.add_argument('--repo', required=True)
    args = parser.parse_args()
    if args.command == 'version':
        version = project_version()
        if args.ref.startswith('refs/tags/') and args.ref != 'refs/tags/v' + version:
            raise ValueError('Version tag and project version disagree')
        print(version)
    elif args.command == 'stage':
        observed = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, check=True,
                                  stdout=subprocess.PIPE, text=True).stdout.strip()
        if observed != args.sha: raise ValueError('Checkout and build commit disagree')
        version = project_version()
        names = expected_names(args.platform, version)
        if args.platform == 'linux':
            files = [ROOT / 'src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb' / names[0],
                     *[ROOT / 'dist-packages' / name for name in names[1:]]]
        else:
            files = [ROOT / 'src-tauri/target/release/bundle/nsis' / names[0]]
        stage(args.platform, version, args.sha, files, args.destination)
    else:
        if args.tag != 'v' + project_version(): raise ValueError('Tag and checkout version disagree')
        publish(args.directory, args.sha, args.tag, args.repo)


if __name__ == '__main__':
    main()
