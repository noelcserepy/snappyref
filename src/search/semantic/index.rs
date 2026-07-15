pub fn decode_f32_blob(blob: &[u8]) -> Result<Vec<f32>, String> {
    if !blob.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err("embedding blob has invalid length".to_owned());
    }

    blob.chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| {
            let bytes: [u8; 4] = chunk
                .try_into()
                .map_err(|_| "embedding blob chunk decode failed".to_owned())?;
            Ok(f32::from_ne_bytes(bytes))
        })
        .collect()
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Result<f32, String> {
    if left.len() != right.len() {
        return Err("embedding dimensions do not match".to_owned());
    }

    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (lhs, rhs) in left.iter().zip(right.iter()) {
        dot += lhs * rhs;
        left_norm += lhs * lhs;
        right_norm += rhs * rhs;
    }

    if left_norm <= f32::EPSILON || right_norm <= f32::EPSILON {
        return Ok(0.0);
    }

    Ok(dot / (left_norm.sqrt() * right_norm.sqrt()))
}
