"""Launch verified package GUIs with a masked home and no checkout access.

The user namespace deliberately preserves production refusal of unmapped root
identities. This is GUI/manual-save evidence, not native compatibility proof.
"""
import argparse
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

from build import ROOT
from package_resources import recover_bundle

REPO = ROOT.parents[1]
sys.path.insert(0, str(REPO / 'scripts'))
from verify_linux_native import bounded_file, read_archive_binary, debian_binary
sys.path.insert(0, str(ROOT / 'prototype'))
from isolated_run import run_fixture


def identity(path):
    metadata = path.stat()
    return [metadata.st_dev, metadata.st_ino]


def preserve_diagnostics(output, kind, destination=REPO / 'tasks'):
    if destination.is_symlink(): raise ValueError('Diagnostic directory must not be a symlink')
    destination.mkdir(mode=0o700, parents=True, exist_ok=True)
    retained = []
    for path in [*output.glob('*.png'), *output.glob('*.log')]:
        if path.is_symlink() or not path.is_file(): raise ValueError('Expected regular fixture diagnostics')
        descriptor, name = tempfile.mkstemp(prefix=f'packaged-gui-{kind}-', suffix='-' + path.name, dir=destination)
        with os.fdopen(descriptor, 'wb') as target, path.open('rb') as source:
            shutil.copyfileobj(source, target)
        retained.append(Path(name))
        print(json.dumps({'artifact': name, 'package': kind}), flush=True)
    return retained


def read_packages(receipt_path, portable_path, deb_path):
    receipt = json.loads(bounded_file(receipt_path, 16384))
    with tarfile.open(portable_path, 'r:gz') as archive:
        portable = read_archive_binary(archive, f"lyn-{receipt['version']}-linux-x86_64/lyn")
    data = subprocess.check_output(['dpkg-deb', '--fsys-tarfile', str(deb_path)], timeout=10)
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        deb = read_archive_binary(archive, 'usr/bin/lyn')
    if deb != debian_binary(portable):
        raise ValueError('Package binaries differ beyond the expected Tauri marker')
    with tempfile.TemporaryDirectory(prefix='lyn-package-check-') as directory:
        root = Path(directory)
        recover_bundle(portable, receipt, 'binarySha256', root / 'portable')
        recover_bundle(deb, receipt, 'debianBinarySha256', root / 'debian')
    return receipt, {'portable': portable, 'debian': deb}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipt', required=True, type=Path)
    parser.add_argument('--portable', required=True, type=Path)
    parser.add_argument('--deb', required=True, type=Path)
    parser.add_argument('--previous-receipt', type=Path)
    parser.add_argument('--previous-portable', type=Path)
    parser.add_argument('--previous-deb', type=Path)
    parser.add_argument('--guard-only', action='store_true')
    args = parser.parse_args()
    previous = [args.previous_receipt, args.previous_portable, args.previous_deb]
    if any(previous) and not all(previous):
        parser.error('Supply all three previous-package inputs together')
    packages = [read_packages(args.receipt, args.portable, args.deb)]
    if all(previous):
        packages.insert(0, read_packages(*previous))
        if packages[0][0]['version'] == packages[1][0]['version']:
            parser.error('Roundtrip requires distinct application versions')
    order = [0, 1, 0] if len(packages) == 2 else [0]
    user_home = Path(os.environ['HOME'])
    if (os.getuid() <= 0 or os.getgid() <= 0 or user_home.parent != Path('/home')
            or user_home.resolve(strict=True) != user_home or not REPO.is_relative_to(user_home)):
        raise ValueError('This fixture requires an ordinary user, canonical /home/<user> and a checkout beneath it')
    sdk_value = os.environ.get('LYN_GNOME_SDK_ROOT')
    sdk = Path(sdk_value).resolve(strict=True) if sdk_value else None
    for kind in ('portable', 'debian'):
        with tempfile.TemporaryDirectory(prefix='lyn-package-gui-') as directory:
            output = Path(directory)
            installed = []
            for index, (receipt, binaries) in enumerate(packages):
                executable = output / f'lyn-{index}'
                executable.write_bytes(binaries[kind])
                executable.chmod(0o700)
                installed.append(dict(binary=str(executable), version=receipt['version']))
            runs = [dict(installed[index], body=f'Packaged note {7319 + step}')
                    for step, index in enumerate(order)]
            for name in ('isolated_run.py', 'desktop_isolation.py', 'desktop_accessibility.py',
                         'desktop_lifecycle.py', 'packaged_gui_pixels.py', 'packaged_gui_guard.py', 'packaged_gui_runtime.py',
                         'desktop-a11y.conf', 'desktop-openbox.xml'):
                shutil.copyfile(ROOT / 'prototype' / name, output / name)
            control = dict(runs=runs, uid=os.getuid(), gid=os.getgid(),
                           home=str(user_home), homeIdentity=identity(user_home), checkout=str(REPO),
                           tmpIdentity=identity(Path('/tmp')), runIdentity=identity(Path('/run')),
                           varlibIdentity=identity(Path('/var/lib')),
                           mountNs=os.readlink('/proc/self/ns/mnt'), pidNs=os.readlink('/proc/self/ns/pid'),
                           networkNs=os.readlink('/proc/self/ns/net'))
            control_path = output / 'control.json'
            control_path.write_text(json.dumps(control))
            sdk_alias = '/tmp/lyn-package-sdk' if sdk else '-'
            command = ['/usr/bin/bwrap', '--die-with-parent', '--new-session', '--unshare-user',
                       '--unshare-pid', '--unshare-net', '--unshare-ipc', '--unshare-uts',
                       '--uid', str(os.getuid()), '--gid', str(os.getgid()), '--ro-bind', '/', '/',
                       '--ro-bind', '/sys', '/sys', '--tmpfs', '/home', '--dir', str(user_home),
                       '--tmpfs', '/run', '--tmpfs', '/var/lib', '--tmpfs', '/tmp', '--bind', str(output), str(output),
                       '--ro-bind', str(control_path), str(control_path), '--proc', '/proc', '--dev', '/dev']
            if sdk: command += ['--ro-bind', str(sdk), sdk_alias]
            command += ['--chdir', str(output), '--', '/usr/bin/python3', '-B', str(output / 'isolated_run.py'),
                        sdk_alias, str(output / 'packaged_gui_runtime.py'), str(control_path), sdk_alias]
            if args.guard_only: command.append('--guard-only')
            # HOME keeps its original value; Bubblewrap masks that path instead.
            environment = {'HOME': str(user_home), 'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', 'XDG_SESSION_TYPE': 'x11'}
            try:
                result = run_fixture(command, environment)
            finally:
                # Preserve fixture diagnostics only, even when the child refuses.
                preserve_diagnostics(output, kind)
            if result:
                raise SystemExit(result)
            print(f'PASS: {kind} packaged GUI filesystem guard' + ('' if args.guard_only else
                  ' and real bundled UI/save' + (' with previous/current/previous data preservation' if len(runs) == 3 else '')), flush=True)


if __name__ == '__main__':
    main()
