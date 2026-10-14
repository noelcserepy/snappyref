# Literal format fixtures

Each static image is 2 by 1 pixels, opaque red followed by opaque green. EXR is
tone-mapped on import to RGB8 values `(186, 69, 69)` and `(69, 186, 69)`. These files
were created locally for the import decoder tests. No downloaded image is used.

- `red-green.ff` has a farbfeld header, big-endian dimensions, and two RGBA16 pixels.
- `red-green.hdr` has a Radiance RGBE header and two uncompressed RGBE pixels.
- `red-green.ico` contains a PNG image in an ICO directory, written with Pillow 10.4.
- `red-green.psd` has a PSD v1 header, empty color/resources/layer sections, and
  uncompressed planar RGB8 data.
- `red-green.jp2` was encoded losslessly from a two-pixel PPM with
  `opj_compress -i red-green.ppm -o red-green.jp2 -n 1`.
- `red-green.jxl` was encoded losslessly from the same PPM with
  `cjxl red-green.ppm red-green.jxl -d 0 --quiet`.
- `red-green.exr` was written with the existing `exr` crate's `write_rgb_file`
  using `(1.0, 0.0, 0.0)` and `(0.0, 1.0, 0.0)` as RGB32F values.

The source PPM bytes are `P6\n2 1\n255\n` followed by `ff 00 00 00 ff 00`.
JP2 and PSD use the macOS native decoder and are tested only on macOS.
The encoders are needed only to regenerate fixtures, never to run tests.

`red-blue.webp` is a lossless 2 by 2 animation, opaque red for 120 ms, then
opaque blue for 250 ms. It was written with Pillow 10.4 using `save_all=True`,
`append_images=[blue]`, `duration=[120, 250]`, `loop=0`, and `lossless=True`.
Both ANMF headers have the no-blend bit set, `flags |= 2`, so each opaque
frame replaces the canvas.
