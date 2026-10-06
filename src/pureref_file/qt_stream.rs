//! Qt `QDataStream` encoding shared by both `.pur` versions: big-endian, no padding.
//! The QVariant helpers cover the variant cells of the 2.x database.

use super::PureRefTransform;

const QVARIANT_QRECTF: u32 = 20;
const QVARIANT_QSIZEF: u32 = 22;
const QVARIANT_QTRANSFORM: u32 = 80;
const QVARIANT_USER_TYPE: u32 = 1024;
const NULL_LENGTH: u32 = u32::MAX;

pub(super) struct QtReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> QtReader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(super) fn position(&self) -> usize {
        self.position
    }

    pub(super) fn seek(&mut self, position: usize) -> Result<(), String> {
        if position > self.bytes.len() {
            return Err(format!("offset {position} is past the end of the data"));
        }
        self.position = position;
        Ok(())
    }

    pub(super) fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| format!("data ends early at offset {}", self.position))?;
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.take(N)?.try_into().expect("take returns N bytes"))
    }

    pub(super) fn u8(&mut self) -> Result<u8, String> {
        Ok(self.array::<1>()?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub(super) fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    pub(super) fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    pub(super) fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_be_bytes(self.array()?))
    }

    pub(super) fn byte_array(&mut self) -> Result<Option<&'a [u8]>, String> {
        match self.u32()? {
            NULL_LENGTH => Ok(None),
            len => self.take(len as usize).map(Some),
        }
    }

    pub(super) fn string(&mut self) -> Result<Option<String>, String> {
        let Some(bytes) = self.byte_array()? else {
            return Ok(None);
        };
        let (units, odd) = bytes.as_chunks::<2>();
        if !odd.is_empty() {
            return Err("string has an odd UTF-16 byte length".to_owned());
        }
        let units = units.iter().map(|unit| u16::from_be_bytes(*unit));
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map(Some)
            .map_err(|error| format!("string is not valid UTF-16: {error}"))
    }

    pub(super) fn transform(&mut self) -> Result<PureRefTransform, String> {
        let mut values = [0.0; 9];
        for value in &mut values {
            *value = self.f64()?;
        }
        // PureRef only writes affine transforms, so m13, m23 and m33 are always 0, 0, 1.
        let [m11, m12, _, m21, m22, _, dx, dy, _] = values;
        Ok(PureRefTransform {
            m11,
            m12,
            m21,
            m22,
            dx,
            dy,
        })
    }

    /// Reads a `QPainterPath` and keeps only its element points, which is all a
    /// bounding box needs. Curve control points over-approximate the curve.
    pub(super) fn path_points(&mut self) -> Result<Vec<(f64, f64)>, String> {
        let count = self.u32()? as usize;
        let mut points = Vec::with_capacity(count.min(self.bytes.len() / 20));
        for _ in 0..count {
            let _element_type = self.u32()?;
            points.push((self.f64()?, self.f64()?));
        }
        if count > 0 {
            let _fill_start_and_rule = self.take(8)?;
        }
        Ok(points)
    }

    /// Reads a QVariant header and returns the type id and, for user types, the type name.
    fn variant_type(&mut self) -> Result<(u32, Option<&'a [u8]>), String> {
        let type_id = self.u32()?;
        if self.u8()? != 0 {
            return Err(format!("QVariant of type {type_id} is null"));
        }
        let name = if type_id == QVARIANT_USER_TYPE {
            self.byte_array()?
        } else {
            None
        };
        Ok((type_id, name))
    }
}

pub(super) fn variant_transform(bytes: &[u8]) -> Result<PureRefTransform, String> {
    let mut reader = QtReader::new(bytes);
    match reader.variant_type()? {
        (QVARIANT_QTRANSFORM, _) => reader.transform(),
        (type_id, _) => Err(format!(
            "expected a QTransform variant, found type {type_id}"
        )),
    }
}

pub(super) fn variant_path_points(bytes: &[u8]) -> Result<Vec<(f64, f64)>, String> {
    let mut reader = QtReader::new(bytes);
    match reader.variant_type()? {
        (QVARIANT_USER_TYPE, Some(b"QPainterPath\0")) => reader.path_points(),
        _ => Err("expected a QPainterPath variant".to_owned()),
    }
}

/// PureRef's `BigRational` sort order, approximated as `f64` for sorting.
pub(super) fn variant_rational(bytes: &[u8]) -> Result<f64, String> {
    let mut reader = QtReader::new(bytes);
    match reader.variant_type()? {
        (QVARIANT_USER_TYPE, Some(b"BigRational\0")) => {
            let numerator = big_integer(&mut reader)?;
            let denominator = big_integer(&mut reader)?;
            Ok(if denominator == 0.0 {
                numerator
            } else {
                numerator / denominator
            })
        }
        _ => Err("expected a BigRational variant".to_owned()),
    }
}

fn big_integer(reader: &mut QtReader) -> Result<f64, String> {
    let sign = match reader.u32()? {
        0 => 0.0,
        1 => 1.0,
        u32::MAX => -1.0,
        other => return Err(format!("unknown BigRational sign {other}")),
    };
    let block_count = reader.u64()?;
    let mut magnitude = 0.0;
    let mut scale = 1.0;
    for _ in 0..block_count {
        magnitude += f64::from(reader.u32()?) * scale;
        scale *= 4_294_967_296.0;
    }
    Ok(sign * magnitude)
}

pub(super) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_f64(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_byte_array(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

pub(super) fn put_string(out: &mut Vec<u8>, value: &str) {
    put_u32(out, encoded_string_len(value) as u32 - 4);
    for unit in value.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
}

pub(super) fn encoded_string_len(value: &str) -> usize {
    4 + 2 * value.encode_utf16().count()
}

fn put_variant_type(out: &mut Vec<u8>, type_id: u32) {
    put_u32(out, type_id);
    out.push(0);
}

fn put_user_variant_type(out: &mut Vec<u8>, name: &[u8]) {
    put_variant_type(out, QVARIANT_USER_TYPE);
    put_byte_array(out, name);
}

pub(super) fn transform_variant(transform: PureRefTransform) -> Vec<u8> {
    let mut out = Vec::with_capacity(77);
    put_variant_type(&mut out, QVARIANT_QTRANSFORM);
    let PureRefTransform {
        m11,
        m12,
        m21,
        m22,
        dx,
        dy,
    } = transform;
    for value in [m11, m12, 0.0, m21, m22, 0.0, dx, dy, 1.0] {
        put_f64(&mut out, value);
    }
    out
}

pub(super) fn rect_variant(x: f64, y: f64, width: f64, height: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(37);
    put_variant_type(&mut out, QVARIANT_QRECTF);
    for value in [x, y, width, height] {
        put_f64(&mut out, value);
    }
    out
}

pub(super) fn size_variant(width: f64, height: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(21);
    put_variant_type(&mut out, QVARIANT_QSIZEF);
    put_f64(&mut out, width);
    put_f64(&mut out, height);
    out
}

/// A closed polygon path: one MoveTo followed by LineTos, fill start 0, odd-even fill.
pub(super) fn polygon_path_variant(points: &[(f64, f64)]) -> Vec<u8> {
    const MOVE_TO: u32 = 0;
    const LINE_TO: u32 = 1;
    let mut out = Vec::new();
    put_user_variant_type(&mut out, b"QPainterPath\0");
    put_u32(&mut out, points.len() as u32);
    for (index, (x, y)) in points.iter().enumerate() {
        put_u32(&mut out, if index == 0 { MOVE_TO } else { LINE_TO });
        put_f64(&mut out, *x);
        put_f64(&mut out, *y);
    }
    if !points.is_empty() {
        put_u32(&mut out, 0);
        put_u32(&mut out, 0);
    }
    out
}

/// `value / 1` as a `BigRational`: sign, then one 32-bit block each for numerator and denominator.
pub(super) fn whole_rational_variant(value: u32) -> Vec<u8> {
    let mut out = Vec::new();
    put_user_variant_type(&mut out, b"BigRational\0");
    for part in [value, 1] {
        put_u32(&mut out, u32::from(part != 0));
        put_u64(&mut out, 1);
        put_u32(&mut out, part);
    }
    out
}
