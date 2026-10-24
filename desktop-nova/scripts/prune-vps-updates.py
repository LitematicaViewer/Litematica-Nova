#!/usr/bin/env python3
"""Prune obsolete VPS release packages only after a complete feed switch."""

import argparse
import base64
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
from urllib.parse import unquote, urlsplit


DEFAULT_ROOT = Path('/srv/nova-updates')
VERSION = re.compile(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?\Z')


def report(event, **fields):
    print(json.dumps({'event': event, **fields}, ensure_ascii=True), flush=True)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def version_key(value):
    match = VERSION.fullmatch(value)
    require(match is not None, 'Unrecognized release version')
    a, b, c, suffix = match.groups()
    prerelease = tuple((0, int(x)) if x.isdigit() else (1, x) for x in (suffix or '').split('.'))
    return int(a), int(b), int(c), suffix is None, prerelease


def exact_directory(path):
    require(path.is_absolute() and not path.is_symlink() and path.is_dir(), f'Unsafe directory: {path}')
    require(path.resolve(strict=True) == path, f'Noncanonical directory: {path}')


def regular_nonempty(path):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_size > 0, f'Missing or invalid payload: {path}')


def validate_old_tree(path, releases):
    exact_directory(path)
    require(path.parent == releases, f'Outside releases directory: {path}')
    for child in path.rglob('*'):
        require(not child.is_symlink(), f'Symlink in deletion target: {child}')
        require(child.resolve(strict=True).is_relative_to(path), f'Escaping deletion target: {child}')
        mode = child.lstat().st_mode
        require(stat.S_ISREG(mode) or stat.S_ISDIR(mode), f'Special file in deletion target: {child}')


def available_bytes(root):
    info = os.statvfs(root)
    return info.f_bavail * info.f_frsize


def prune(root):
    exact_directory(root)
    releases = root / 'releases'
    exact_directory(releases)
    feed_path = root / 'latest.json'
    regular_nonempty(feed_path)
    original = feed_path.read_bytes()
    feed = json.loads(original)
    current_version = version_key(feed['version'])
    platforms = feed['platforms']
    require(isinstance(platforms, dict) and platforms, 'Empty platforms')
    keep_names, payloads = set(), set()
    for platform, item in platforms.items():
        require(isinstance(item, dict), 'Invalid platform entry')
        signature = item.get('signature')
        require(isinstance(signature, str) and signature.strip(), 'Missing payload signature')
        require(bool(base64.b64decode(signature, validate=True)), 'Invalid payload signature encoding')
        url = urlsplit(item['url'])
        require(url.scheme == 'https' and url.netloc and not url.username and not url.password,
                'Payload URL must use HTTPS')
        path = unquote(url.path)
        require('\\' not in path and '\x00' not in path, 'Invalid payload path')
        match = re.fullmatch(r'/nova-updates/releases/([^/]+)/([^/]+)', path)
        require(match is not None and not url.query and not url.fragment, 'Unexpected payload URL path')
        release, name = match.groups()
        require(release not in ('.', '..') and name not in ('.', '..'), 'Invalid payload basename')
        require(release.startswith('beta-') and version_key(release[5:]) == current_version,
                'Feed version and release directory differ')
        keep_names.add(release)
        payloads.add(name)
        if platform.startswith('darwin-') and name.endswith('.app.tar.gz'):
            payloads.add(name[:-11] + '.dmg')
    require(len(keep_names) == 1, 'Platforms refer to different release directories')
    keep = releases / keep_names.pop()
    exact_directory(keep)
    for name in payloads:
        regular_nonempty(keep / name)
    # Preserve the whole current directory, including notes and auxiliary metadata.
    targets = []
    for child in sorted(releases.iterdir()):
        if child == keep:
            continue
        if not child.name.startswith('beta-') or not VERSION.fullmatch(child.name[5:]):
            report('preserved_unknown', path=str(child))
            continue
        if version_key(child.name[5:]) >= current_version:
            report('preserved_candidate', path=str(child))
            continue
        validate_old_tree(child, releases)
        targets.append(child)
    # Validate every target before the first deletion, then fail closed if the publisher changes the feed.
    require(feed_path.read_bytes() == original, 'Feed changed during validation; nothing deleted')
    before = available_bytes(root)
    for target in targets:
        require(feed_path.read_bytes() == original, 'Feed changed during cleanup; stopped')
        for name in payloads:
            regular_nonempty(keep / name)
        validate_old_tree(target, releases)
        prior = available_bytes(root)
        shutil.rmtree(target)
        report('deleted', path=str(target), released_bytes=available_bytes(root) - prior)
    report('complete', retained=str(keep), deleted_count=len(targets),
           released_bytes=available_bytes(root) - before, available_bytes=available_bytes(root))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=DEFAULT_ROOT)
    args = parser.parse_args()
    exact_directory(args.root)
    lock = Path('/run/lock/nova-prune-update-releases.lock') if args.root == DEFAULT_ROOT else args.root / '.prune.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        require(stat.S_ISREG(os.fstat(descriptor).st_mode), 'Invalid cleanup lock')
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            report('busy_noop')
            return 0
        prune(args.root)
        return 0
    finally:
        os.close(descriptor)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, KeyError, OSError, TypeError, json.JSONDecodeError) as error:
        report('refused', reason=str(error))
        sys.exit(1)
