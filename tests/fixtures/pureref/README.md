# Independent PureRef fixtures

`generate.py` builds the small synthetic file with Python's struct and sqlite3
modules. It never imports SnappyRef or consumes `write_board` output. The field
order is documented in `src/pureref_file/read_v2.rs` and `qt_stream.rs`.
Regenerate with `python3 tests/fixtures/pureref/generate.py`.

`minimal-v2.pur` has one linked 8 by 4 image and a rich-text note, both inside
a group scaled by 2 and translated to 20, 30. The image pixel origin is 12,
26 and its composed opacity is 0.25. The source is `/refs/card.png`, the name
is `card`, and the note is `A & B\nSecond\nline`.

Rust tests modify this independent SQLite fixture to exercise corrupt graphs.
This fixture is hand-authored from the byte layout, not PureRef-authored.

`minimal-v1.pur` has one linked 8 by 4 image at pixel origin 16, 28, named
`card`, with source `/refs/card.png`, and a note `Legacy note` at -10, 5.
It uses a hand-packed 1.10 header, image link marker, two item records and
ref table, following the layout documented in `read_v1.rs`.
