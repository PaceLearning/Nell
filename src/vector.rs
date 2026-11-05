//! Vectors as the store keeps them: 8 bit integers with one scale per vector, and the cosine between two.

/// A vector as 8 bit integers and the scale that turns them back into numbers.
pub struct Quantized {
    pub scale: f32,
    pub values: Vec<i8>,
}

/// Turns `values` into 8 bit integers. The scale is the largest magnitude over 127, so the largest value becomes
/// 127 or -127 and every other value is rounded to the nearest step of the scale.
pub fn quantize(values: &[f32]) -> Quantized {
    let largest = values.iter().fold(0.0f32, |largest, value| largest.max(value.abs()));
    let mut scale = largest / 127.0;
    if scale == 0.0 {
        scale = 1.0;
    }
    let values = values.iter().map(|value| (value / scale).round().clamp(-127.0, 127.0) as i8).collect();
    Quantized { scale, values }
}

/// Turns 8 bit integers back into numbers with their scale.
pub fn dequantize(scale: f32, values: &[i8]) -> Vec<f32> {
    values.iter().map(|&value| value as f32 * scale).collect()
}

/// The cosine of the angle between two vectors: 1 when they point the same way, 0 when they are unrelated.
/// Zero when either has no length or they differ in length.
pub fn cosine(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() {
        return 0.0;
    }
    let dot: f32 = left.iter().zip(right).map(|(left, right)| left * right).sum();
    let left_length = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_length = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    if left_length == 0.0 || right_length == 0.0 {
        return 0.0;
    }
    dot / (left_length * right_length)
}

#[cfg(test)]
mod tests {
    use super::{cosine, dequantize, quantize};

    #[test]
    fn quantize_and_dequantize_round_trip_within_half_a_step() {
        let values = [0.5, -0.25, 0.125, 0.0, -1.0, 0.63, 0.003];
        let quantized = quantize(&values);
        assert_eq!(quantized.values.len(), values.len());
        assert_eq!(quantized.values[4], -127);
        let back = dequantize(quantized.scale, &quantized.values);
        for (original, restored) in values.iter().zip(&back) {
            assert!((original - restored).abs() <= quantized.scale / 2.0 + 1e-6, "{original} came back as {restored}");
        }
        assert_eq!(back[4], -1.0);
    }

    #[test]
    fn a_vector_of_zeros_round_trips_to_zeros() {
        let quantized = quantize(&[0.0, 0.0, 0.0]);
        assert_eq!(dequantize(quantized.scale, &quantized.values), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn cosine_orders_the_closest_vector_first() {
        let asked = [1.0, 0.0, 0.0];
        let close = [0.9, 0.1, 0.0];
        let sideways = [0.0, 1.0, 0.0];
        let opposite = [-1.0, 0.0, 0.0];
        assert!(cosine(&asked, &close) > cosine(&asked, &sideways));
        assert!(cosine(&asked, &sideways) > cosine(&asked, &opposite));
        assert!((cosine(&asked, &asked) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&asked, &[0.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine(&asked, &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn cosine_keeps_its_order_after_quantizing() {
        let asked = [0.3, -0.2, 0.5, 0.1];
        let close = [0.31, -0.19, 0.48, 0.12];
        let far = [-0.3, 0.4, 0.1, -0.5];
        let close_back = dequantize(quantize(&close).scale, &quantize(&close).values);
        let far_back = dequantize(quantize(&far).scale, &quantize(&far).values);
        assert!(cosine(&asked, &close_back) > cosine(&asked, &far_back));
        assert!((cosine(&asked, &close_back) - cosine(&asked, &close)).abs() < 0.01);
    }
}
