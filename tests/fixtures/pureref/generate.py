"""Independent synthetic PureRef 2.1 fixture, using only Python's stdlib.

Integers/doubles are big endian. QString has a byte count and UTF-16BE data.
The envelope displaces the SQLite prefix to the end. No SnappyRef writer used.
"""
from pathlib import Path
import sqlite3
import struct


def string(text):
    data = text.encode('utf-16-be')
    return struct.pack('>I', len(data)) + data


def transform(x, y, scale=1):
    return struct.pack('>9d', scale, 0, 0, 0, scale, 0, x, y, 1)


def v2():
    db = sqlite3.connect(':memory:')
    db.execute('PRAGMA page_size=512')
    db.executescript('''
        CREATE TABLE images(id, origin, source, data, width, height);
        CREATE TABLE items(id, parent, name, transform, sort_order, z, opacity);
        CREATE TABLE items_images(id, image, image_transform, image_bounds);
        CREATE TABLE items_notes(id, text);
        INSERT INTO images VALUES(7, '/refs/card.png', NULL, NULL, 8, 4);
        INSERT INTO items VALUES(10, -1, 'group', NULL, NULL, 0, 0.5);
        INSERT INTO items VALUES(11, 10, 'card', NULL, NULL, 1, 0.5);
        INSERT INTO items VALUES(12, 10, NULL, NULL, NULL, 2, 1);
        INSERT INTO items_images VALUES(11, 7, NULL, NULL);
        INSERT INTO items_notes VALUES(12, '<p>A &amp; B</p><p>Second<br/>line</p>');
    ''')
    variant = struct.pack('>IB', 80, 0) + transform(20, 30, 2)
    db.execute('UPDATE items SET transform=? WHERE id=10', (variant,))
    data = db.serialize()
    header = string('2.1') + struct.pack('>IQ', 0, len(data))
    header += string('2.0.3') + string('') + struct.pack('>I', 0)
    return header + data[len(header):] + data[:len(header)]


def v1():
    header = string('1.10') + struct.pack('>HHQ', 2, 1, 0)
    header += string('1.11.1') + string('') + bytes(4 + 32 + 72 + 8)
    blob_start = len(header)
    data = bytearray(header + struct.pack('>I', 0xffffffff))
    record = string('GraphicsImageItem') + string('/refs/card.png') + string('card')
    record += struct.pack('>d', 1) + transform(20, 30) + struct.pack('>Id', 7, 1)
    record += transform(-4, -2) + struct.pack('>II', 0, 0)
    data += struct.pack('>Q', len(data) + 8 + len(record)) + record
    record = string('GraphicsTextItem') + string('Legacy note')
    record += transform(-10, 5) + struct.pack('>IdI', 9, 2, 0)
    data += struct.pack('>Q', len(data) + 8 + len(record)) + record
    data += string('')  # save folder
    refs = len(data)
    data += struct.pack('>IQQ', 7, blob_start, blob_start + 4)
    data[16:24] = struct.pack('>Q', refs)
    return data


if __name__ == '__main__':
    (Path(__file__).parent / 'minimal-v1.pur').write_bytes(v1())
    (Path(__file__).parent / 'minimal-v2.pur').write_bytes(v2())
