#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn round_non_negative_to_u32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn f32_from_f64(value: f64) -> f32 {
    value as f32
}

pub(super) fn u32_from_usize(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(super) fn i32_from_usize(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub(super) fn f64_from_usize(value: usize) -> f64 {
    f64::from(u32_from_usize(value))
}

pub(super) fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

pub(super) fn scaled_dimensions(src_w: u32, src_h: u32, target_w: Option<u32>) -> (u32, u32, f32) {
    if let Some(tw) = target_w
        && tw > 0
        && tw != src_w
    {
        let scale_f64 = f64::from(tw) / f64::from(src_w);
        let dst_w = round_non_negative_to_u32(f64::from(src_w) * scale_f64).max(1);
        let dst_h = round_non_negative_to_u32(f64::from(src_h) * scale_f64).max(1);
        let scale = f32_from_f64(scale_f64);
        return (dst_w, dst_h, scale);
    }
    (src_w, src_h, 1.0)
}
