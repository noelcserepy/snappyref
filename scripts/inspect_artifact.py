"""Validate the report and PNG evidence emitted by inspect scenarios."""
import json
from pathlib import Path
import struct
import zlib


def validate_png(path):
    try:
        _validate_png(path)
    except (struct.error, zlib.error) as error:
        raise ValueError(f'invalid PNG: {path}: {error}') from error


def _validate_png(path):
    data = Path(path).read_bytes()
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError(f'invalid PNG: {path}')
    offset, chunks = 8, []
    while offset < len(data):
        size = int.from_bytes(data[offset:offset + 4], 'big')
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        end = offset + 12 + size
        if end > len(data) or zlib.crc32(kind + payload) != int.from_bytes(data[end - 4:end], 'big'):
            raise ValueError(f'corrupt PNG chunk: {path}')
        chunks.append((kind, payload))
        offset = end
    if not chunks or chunks[0][0] != b'IHDR' or chunks[-1] != (b'IEND', b''):
        raise ValueError(f'incomplete PNG: {path}')
    width, height, depth, color, compression, filtering, interlace = struct.unpack('>IIBBBBB', chunks[0][1])
    # Inspect captures are noninterlaced, 8-bit RGB/RGBA screenshots.
    if (not width or not height or depth != 8 or color not in (2, 6)
            or compression or filtering or interlace
            or sum(kind == b'IHDR' for kind, _ in chunks) != 1
            or any(kind == b'IEND' for kind, _ in chunks[:-1])
            or any(kind[:1].isupper() and kind not in (b'IHDR', b'PLTE', b'IDAT', b'IEND')
                   for kind, _ in chunks)):
        raise ValueError(f'invalid screenshot PNG header: {path}')
    stride = 1 + width * (3 if color == 2 else 4)
    decoder = zlib.decompressobj()
    pixels = decoder.decompress(b''.join(payload for kind, payload in chunks if kind == b'IDAT'),
                                height * stride + 1)
    if (not decoder.eof or decoder.unused_data or len(pixels) != height * stride
            or any(value > 4 for value in pixels[::stride])):
        raise ValueError(f'invalid PNG pixels: {path}')


def load_report(artifact, scenario):
    artifact = Path(artifact).resolve()
    report = json.loads((artifact / 'report.json').read_text())
    if (not isinstance(report, dict) or report.get('scenario') != scenario
            or report.get('status') not in ('pass', 'fail')
            or not isinstance(report.get('artifact_dir'), str)
            or Path(report['artifact_dir']).resolve() != artifact
            or not isinstance(report.get('assertions'), list) or not report['assertions']
            or not all(isinstance(item, dict) and isinstance(item.get('name'), str)
                       and type(item.get('passed')) is bool for item in report['assertions'])
            or not isinstance(report.get('frames'), list)
            or not all(isinstance(frame, str) for frame in report['frames'])):
        raise ValueError(f'invalid inspect report: {scenario}')
    # Rust scenarios capture screenshots; MCP tools can produce text-only evidence.
    if report['status'] == 'pass' and ((not report['frames'] and scenario != 'mcp-drive')
                                     or not all(item['passed'] for item in report['assertions'])):
        raise ValueError(f'incomplete inspect report: {scenario}')
    for frame in report['frames']:
        path = Path(frame).resolve()
        if not path.is_relative_to(artifact):
            raise ValueError(f'frame outside inspect artifact: {frame}')
        validate_png(path)
    return report
