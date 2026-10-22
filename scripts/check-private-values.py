#!/usr/bin/env python3
"""Refuse a push whose new commits contain this machine's private values.

Reads pre-push lines (`<local ref> <local sha> <remote ref> <remote sha>`) on
stdin and scans each commit not yet on any remote: its message and the lines
its diff adds. The values come from the private `.env.windows` and `$HOME`, so
only this machine can run the check. Output names the place, never the value.
"""
import os
import re
import subprocess
import sys
from pathlib import Path

ZERO = re.compile(r'^0+$')
HUNK = re.compile(r'^@@ -\d+(?:,\d+)? \+(\d+)')
USERS_FOLDER = re.compile(r'^([A-Za-z]:\\Users\\[^\\]+)', re.IGNORECASE)
MIN_WORD = 6


def git(*args):
    return subprocess.run(['git', *args], check=True, text=True, capture_output=True,
                          errors='replace').stdout


def env_file():
    if os.environ.get('SNAPPYREF_WINDOWS_ENV'):
        return Path(os.environ['SNAPPYREF_WINDOWS_ENV'])
    checkout = Path(git('rev-parse', '--show-toplevel').strip())
    if (checkout / '.env.windows').is_file():
        return checkout / '.env.windows'
    # A linked worktree has no copy; the main checkout holds the private file.
    common = Path(git('rev-parse', '--path-format=absolute', '--git-common-dir').strip())
    return common.parent / '.env.windows'


def env_values(path):
    if not path.is_file():
        return []
    values = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith('#') or '=' not in line:
            continue
        key, value = line.removeprefix('export ').split('=', 1)
        value = value.strip().strip('\'"')
        values.append((key.strip(), value))
    return values


def normalize(text):
    """Compare paths whatever their separators or escaping, and ignore case."""
    return re.sub(r'[\\/]+', '\\\\', text).lower()


def private_values():
    found = [('HOME', os.environ['HOME'])] if os.environ.get('HOME') else []
    for key, value in env_values(env_file()):
        found.append((key, value))
        found += [(key, word) for word in value.split() if word != value]
        if folder := USERS_FOLDER.match(value):
            found.append((key, folder.group(1)))
    return [(label, normalize(value)) for label, value in found if len(value) >= MIN_WORD]


def new_commits(lines):
    tips = [sha for line in lines if len(parts := line.split()) == 4
            and not ZERO.match(sha := parts[1])]
    if not tips:
        return []
    return git('rev-list', *tips, '--not', '--remotes').split()


def leaks(commit, values):
    def hits(text):
        text = normalize(text)
        return sorted({label for label, value in values if value in text})

    message = git('show', '-s', '--format=%B', commit)
    for label in hits(message):
        yield f'{commit[:7]} commit message: {label}'
    path, line_no = None, 0
    for line in git('show', '--format=', '--no-color', '--no-ext-diff', commit).splitlines():
        if line.startswith('+++ '):
            path = line[6:] if line.startswith('+++ b/') else None
        elif match := HUNK.match(line):
            line_no = int(match.group(1))
        elif line.startswith('+') and path:
            for label in hits(line[1:]):
                yield f'{commit[:7]} {path}:{line_no}: {label}'
            line_no += 1
        elif line.startswith(' '):
            line_no += 1


def main():
    values = private_values()
    found = [leak for commit in new_commits(sys.stdin.read().splitlines())
             for leak in leaks(commit, values)]
    if found:
        print('Private values in pushed commits. Remove them and amend:', file=sys.stderr)
        for leak in found:
            print(f'  {leak}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
