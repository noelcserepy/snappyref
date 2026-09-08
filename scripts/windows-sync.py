#!/usr/bin/env python3
"""Synchronize source files to the Windows execution checkout."""
import base64
import io
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PROTECTED = {'.git', 'target', 'library', 'models', 'originals', 'thumbs', 'library.db', '.env.windows'}


def source_paths(root):
    raw = subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=root)
    paths = sorted(set(raw.decode().strip('\0').split('\0')) - {''})
    result = []
    for name in paths:
        parts = PurePosixPath(name).parts
        if (not parts or name.startswith('/') or any(part in ('', '.', '..') for part in parts)
                or any(char in name for char in '\\:\r\n\t"<>|?*')
                or any(part.endswith((' ', '.')) for part in parts)
                or parts[0].lower() in PROTECTED - {'.env.windows'}):
            raise ValueError(f'unsafe managed source path: {name!r}')
        if parts[0].startswith('.env') and not parts[0].endswith('.example'):
            continue
        path = root / name
        if any(parent.is_symlink() for parent in path.parents if parent.is_relative_to(root)):
            raise ValueError(f'symlink directory in managed source: {name}')
        if path.is_symlink() and (not path.resolve().is_relative_to(root.resolve()) or not path.is_file()):
            raise ValueError(f'source symlink must target a file inside checkout: {name}')
        if path.is_file():
            result.append(name)
    if not {'Cargo.toml', 'src/main.rs'}.issubset(result):
        raise ValueError('source inventory lacks repository markers')
    if len({name.casefold() for name in result}) != len(result):
        raise ValueError('source inventory contains Windows case collisions')
    return result


def main():
    paths = source_paths(ROOT)
    origin = subprocess.check_output(['git', 'remote', 'get-url', 'origin'], cwd=ROOT, text=True).strip()
    if not origin:
        raise ValueError('source repository requires an origin')
    destination = os.environ['SNAPPYREF_WINDOWS_REPO'].replace("'", "''")
    bootstrap = f"""$ErrorActionPreference = 'Stop'
$work = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory $work | Out-Null
try {{
  tar -xf - -C $work
  if ($LASTEXITCODE -ne 0) {{ throw 'sync transfer failed' }}
  & (Join-Path $work 'apply.ps1') -Source $work -Destination '{destination}'
}} finally {{ Remove-Item -LiteralPath $work -Recurse -Force }}
"""
    command = base64.b64encode(bootstrap.encode('utf-16le')).decode()
    with tempfile.TemporaryFile() as stream:
        with tarfile.open(fileobj=stream, mode='w', dereference=True) as archive:
            for name in paths:
                archive.add(ROOT / name, arcname='source/' + name, recursive=False)
            archive.add(ROOT / 'scripts/windows-sync-apply.ps1', arcname='apply.ps1')
            data = json.dumps(dict(origin=origin, files=paths)).encode()
            info = tarfile.TarInfo('manifest.json')
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
        stream.seek(0)
        subprocess.run(['ssh', '-o', 'BatchMode=yes', os.environ['SNAPPYREF_WINDOWS_HOST'],
                        'powershell', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', command], stdin=stream, check=True)


if __name__ == '__main__':
    main()
