#!/usr/bin/env python3
"""Refuse a push whose new commits publish this machine's private values or
any image outside the repo's asset folders.

Reads pre-push lines (`<local ref> <local sha> <remote ref> <remote sha>`) on
stdin and scans each commit not yet on any remote: its message and the files
it adds or changes. The values are `$HOME` and the SNAPPYREF_WINDOWS_*
settings that windows-env.sh loads from the private `.env.windows`, so only
this machine can run the check. Output names the place, never the value.

Screenshots stay with the agent and the user. When the user wants one on
GitHub, they upload it to the PR themselves.
"""
import os
import re
import subprocess
import sys
from pathlib import Path

ZERO = re.compile(r'^0+$')
WINDOWS_ENV = Path(__file__).resolve().parent / 'windows-env.sh'
MIN_LENGTH = 6
IMAGE = re.compile(r'\.(png|jpe?g|gif|webp|bmp|tiff?|heic|avif)$', re.IGNORECASE)
ASSET_FOLDERS = ('assets/', 'tests/fixtures/', 'vendor/')


def git(*args):
    return subprocess.run(['git', *args], check=True, capture_output=True).stdout


def text(data):
    return data.decode('utf-8', errors='replace')


def normalize(value):
    """Compare paths whatever their separators or escaping, and ignore case."""
    return re.sub(r'[\\/]+', r'\\', value).lower()


def windows_settings():
    # Without a .env.windows the loader fails and only $HOME is checked.
    loaded = subprocess.run(['bash', '-c', 'source "$1" >/dev/null 2>&1 && env -0', '_',
                             str(WINDOWS_ENV)], capture_output=True).stdout
    for entry in text(loaded).split('\0'):
        key, _, value = entry.partition('=')
        if key.startswith('SNAPPYREF_WINDOWS_'):
            yield key, value


def private_values():
    found = [('HOME', os.environ.get('HOME', ''))]
    for key, value in windows_settings():
        found.append((key, value))
        found += [(key, word) for word in value.split() if word != value]
    return [(label, normalize(value)) for label, value in found
            if len(normalize(value)) >= MIN_LENGTH]


def new_commits(lines):
    tips = [sha for line in lines if len(parts := line.split()) == 4
            and not ZERO.match(sha := parts[1])]
    if not tips:
        return []
    return text(git('rev-list', *tips, '--not', '--remotes')).split()


def changed_files(commit):
    """(path, blob) for each file the commit adds or changes, gitlinks left out."""
    # -z keeps paths raw, so no quoting or diff prefix setting can hide one.
    raw = git('diff-tree', '-r', '-m', '-z', '--root', '--no-commit-id', '--diff-filter=d',
              commit).split(b'\0')
    files = set()
    for header, path in zip(raw[0::2], raw[1::2]):
        _, mode, _, blob, _ = header.split()
        if mode != b'160000':
            files.add((path, blob.decode()))
    return sorted(files)


def leaks(commit, values):
    def hits(content):
        content = normalize(content)
        return sorted({label for label, value in values if value in content})

    for label in hits(text(git('show', '-s', '--format=%B', commit))):
        yield f'{commit[:7]} commit message: {label}'
    for path, blob in changed_files(commit):
        if IMAGE.search(text(path)) and not text(path).startswith(ASSET_FOLDERS):
            yield f'{commit[:7]} {text(path)}: image outside {", ".join(ASSET_FOLDERS)}'
            continue
        data = git('cat-file', 'blob', blob)
        if b'\0' in data:
            continue
        for line_no, line in enumerate(text(data).splitlines(), 1):
            for label in hits(line):
                yield f'{commit[:7]} {text(path)}:{line_no}: {label}'


def main():
    values = private_values()
    found = [leak for commit in new_commits(sys.stdin.read().splitlines())
             for leak in leaks(commit, values)]
    if found:
        print('Private values or images in pushed commits. Remove them and amend.', file=sys.stderr)
        print('Show screenshots in the thread; the user uploads any they want on GitHub.',
              file=sys.stderr)
        for leak in found:
            print(f'  {leak}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
