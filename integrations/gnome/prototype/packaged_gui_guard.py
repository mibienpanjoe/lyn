"""Metadata-only prerequisites for a copied package GUI in Bubblewrap."""
import os
import json
from pathlib import Path


def validate(evidence):
    if (evidence.get('uid', 0) <= 0 or evidence.get('gid', 0) <= 0
            or not all(evidence.get(key) is True for key in (
                'private_mount', 'private_pid', 'private_network', 'root_readonly',
                'home_replaced', 'home_unchanged', 'checkout_hidden', 'home_empty',
                'tmp_replaced', 'run_replaced', 'varlib_replaced', 'writes_private'))):
        raise RuntimeError('Packaged GUI filesystem isolation is incomplete')


def require_private_filesystem(control):
    user_home = Path(control['home'])
    metadata = user_home.stat()
    roots = [line.split() for line in Path('/proc/self/mountinfo').read_text().splitlines()
             if line.split()[4] == '/']
    mounts = [line.split() for line in Path('/proc/self/mountinfo').read_text().splitlines()]
    def replaced(path, key):
        current = Path(path).stat()
        return [current.st_dev, current.st_ino] != control[key]
    def private_write_mount(fields):
        if 'rw' not in fields[5].split(','): return True
        mount = Path(fields[4])
        return any(mount == Path(base) or Path(base) in mount.parents
                   for base in ('/home', '/tmp', '/run', '/var/lib', '/proc', '/dev'))
    evidence = dict(uid=os.getuid(), gid=os.getgid(),
        private_mount=os.readlink('/proc/self/ns/mnt') != control['mountNs'],
        private_pid=os.readlink('/proc/self/ns/pid') != control['pidNs'],
        private_network=os.readlink('/proc/self/ns/net') != control['networkNs'],
        root_readonly=len(roots) == 1 and 'ro' in roots[0][5].split(','),
        home_replaced=[metadata.st_dev, metadata.st_ino] != control['homeIdentity'],
        home_unchanged=os.environ.get('HOME') == str(user_home),
        checkout_hidden=not Path(control['checkout']).exists(),
        home_empty=user_home.is_dir() and not any(user_home.iterdir()),
        tmp_replaced=replaced('/tmp', 'tmpIdentity'),
        run_replaced=replaced('/run', 'runIdentity'),
        varlib_replaced=replaced('/var/lib', 'varlibIdentity'),
        writes_private=all(private_write_mount(fields) for fields in mounts))
    print(json.dumps({"check": "packaged_gui_guard_evidence", **evidence,
                      "unprotectedMounts": [fields[4] for fields in mounts if not private_write_mount(fields)]}), flush=True)
    validate(evidence)
    if os.getuid() != control['uid'] or os.getgid() != control['gid']:
        raise RuntimeError('Packaged GUI user identity changed')
    return evidence
