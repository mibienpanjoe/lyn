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
import uuid

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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipt', required=True, type=Path)
    parser.add_argument('--portable', required=True, type=Path)
    parser.add_argument('--deb', required=True, type=Path)
    parser.add_argument('--guard-only', action='store_true')
    args = parser.parse_args()
    receipt = json.loads(bounded_file(args.receipt, 16384))
    with tarfile.open(args.portable, 'r:gz') as archive:
        portable = read_archive_binary(archive, f"lyn-{receipt['version']}-linux-x86_64/lyn")
    data = subprocess.check_output(['dpkg-deb', '--fsys-tarfile', str(args.deb)], timeout=10)
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        deb = read_archive_binary(archive, 'usr/bin/lyn')
    if deb != debian_binary(portable):
        raise ValueError('Package binaries differ beyond the expected Tauri marker')
    user_home = Path(os.environ['HOME'])
    if (os.getuid() <= 0 or os.getgid() <= 0 or user_home.parent != Path('/home')
            or user_home.resolve(strict=True) != user_home or not REPO.is_relative_to(user_home)):
        raise ValueError('This fixture requires an ordinary user, canonical /home/<user> and a checkout beneath it')
    sdk_value = os.environ.get('LYN_GNOME_SDK_ROOT')
    sdk = Path(sdk_value).resolve(strict=True) if sdk_value else None
    for kind, binary, digest in [('portable', portable, 'binarySha256'), ('debian', deb, 'debianBinarySha256')]:
        with tempfile.TemporaryDirectory(prefix='lyn-package-gui-') as directory:
            output = Path(directory)
            recover_bundle(binary, receipt, digest, output / 'resource')
            executable = output / 'lyn'
            executable.write_bytes(binary)
            executable.chmod(0o700)
            for name in ('isolated_run.py', 'desktop_isolation.py', 'desktop_accessibility.py',
                         'desktop_lifecycle.py', 'packaged_gui_pixels.py', 'packaged_gui_guard.py', 'packaged_gui_runtime.py',
                         'desktop-a11y.conf', 'desktop-openbox.xml'):
                shutil.copyfile(ROOT / 'prototype' / name, output / name)
            control = dict(binary=str(executable), version=receipt['version'], uid=os.getuid(), gid=os.getgid(),
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
                for path in [*output.glob('*.png'), *output.glob('*.log')]:
                    destination = REPO / 'tasks' / f'packaged-gui-{kind}-{uuid.uuid4().hex}-{path.name}'
                    shutil.copyfile(path, destination)
                    print(json.dumps({'artifact': str(destination), 'package': kind}), flush=True)
            if result:
                raise SystemExit(result)
            print(f'PASS: {kind} packaged GUI filesystem guard' + ('' if args.guard_only else ' and real bundled UI/save'), flush=True)


if __name__ == '__main__':
    main()
