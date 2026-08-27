//! SIMD-accelerated 8-color ARGB downsampler and blender.
//!
//! Provides AVX2 (x86_64) and NEON (aarch64) SIMD intrinsics with bitwise
//! decomposition, channel lane accumulation, and seamless scalar fallback.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

/// Blends 8 ARGB colors into a single composite ARGB color with alpha-weighting.
#[inline(always)]
pub fn blend_8_colors(colors: &[u32; 8]) -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                return blend_8_colors_avx2(colors);
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        unsafe {
            return blend_8_colors_neon(colors);
        }
    }

    #[allow(unreachable_code)]
    blend_8_colors_scalar(colors)
}

/// Fallback scalar implementation of 8-color ARGB blending.
#[inline(always)]
pub fn blend_8_colors_scalar(colors: &[u32; 8]) -> u32 {
    let mut sum_r: u32 = 0;
    let mut sum_g: u32 = 0;
    let mut sum_b: u32 = 0;
    let mut sum_a: u32 = 0;
    let mut count: u32 = 0;

    for &c in colors {
        let a = (c >> 24) & 0xFF;
        if a > 0 {
            let r = (c >> 16) & 0xFF;
            let g = (c >> 8) & 0xFF;
            let b = c & 0xFF;

            sum_a += a;
            sum_r += r;
            sum_g += g;
            sum_b += b;
            count += 1;
        }
    }

    if count == 0 {
        return 0;
    }

    let avg_a = sum_a / count;
    let avg_r = sum_r / count;
    let avg_g = sum_g / count;
    let avg_b = sum_b / count;

    (avg_a << 24) | (avg_r << 16) | (avg_g << 8) | avg_b
}

/// x86_64 AVX2 vectorized 8-color ARGB blend.
///
/// # Safety
///
/// Caller must ensure CPU target feature `avx2` is available before invocation.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub unsafe fn blend_8_colors_avx2(colors: &[u32; 8]) -> u32 {
    let raw = _mm256_loadu_si256(colors.as_ptr() as *const __m256i);

    let mask_a = _mm256_set1_epi32(0xFF000000u32 as i32);
    let mask_r = _mm256_set1_epi32(0x00FF0000);
    let mask_g = _mm256_set1_epi32(0x0000FF00);
    let mask_b = _mm256_set1_epi32(0x000000FF);

    let a = _mm256_srli_epi32(_mm256_and_si256(raw, mask_a), 24);
    let r = _mm256_srli_epi32(_mm256_and_si256(raw, mask_r), 16);
    let g = _mm256_srli_epi32(_mm256_and_si256(raw, mask_g), 8);
    let b = _mm256_and_si256(raw, mask_b);

    let is_solid = _mm256_cmpgt_epi32(a, _mm256_setzero_si256());
    let valid_count_vec = _mm256_and_si256(is_solid, _mm256_set1_epi32(1));

    let mut a_arr = [0i32; 8];
    let mut r_arr = [0i32; 8];
    let mut g_arr = [0i32; 8];
    let mut b_arr = [0i32; 8];
    let mut cnt_arr = [0i32; 8];

    _mm256_storeu_si256(a_arr.as_mut_ptr() as *mut __m256i, a);
    _mm256_storeu_si256(r_arr.as_mut_ptr() as *mut __m256i, r);
    _mm256_storeu_si256(g_arr.as_mut_ptr() as *mut __m256i, g);
    _mm256_storeu_si256(b_arr.as_mut_ptr() as *mut __m256i, b);
    _mm256_storeu_si256(cnt_arr.as_mut_ptr() as *mut __m256i, valid_count_vec);

    let mut sum_a = 0;
    let mut sum_r = 0;
    let mut sum_g = 0;
    let mut sum_b = 0;
    let mut count = 0;

    for i in 0..8 {
        if cnt_arr[i] > 0 {
            sum_a += a_arr[i] as u32;
            sum_r += r_arr[i] as u32;
            sum_g += g_arr[i] as u32;
            sum_b += b_arr[i] as u32;
            count += 1;
        }
    }

    if count == 0 {
        return 0;
    }

    let avg_a = sum_a / count;
    let avg_r = sum_r / count;
    let avg_g = sum_g / count;
    let avg_b = sum_b / count;

    (avg_a << 24) | (avg_r << 16) | (avg_g << 8) | avg_b
}

/// ARM aarch64 NEON vectorized 8-color ARGB blend.
///
/// # Safety
///
/// Caller must ensure pointer alignment and aarch64 execution environment.
#[cfg(target_arch = "aarch64")]
pub unsafe fn blend_8_colors_neon(colors: &[u32; 8]) -> u32 {
    let low = vld1q_u32(colors.as_ptr());
    let high = vld1q_u32(colors.as_ptr().add(4));

    let mask_ff = vdupq_n_u32(0xFF);

    let a_low = vandq_u32(vshrq_n_u32(low, 24), mask_ff);
    let r_low = vandq_u32(vshrq_n_u32(low, 16), mask_ff);
    let g_low = vandq_u32(vshrq_n_u32(low, 8), mask_ff);
    let b_low = vandq_u32(low, mask_ff);

    let a_high = vandq_u32(vshrq_n_u32(high, 24), mask_ff);
    let r_high = vandq_u32(vshrq_n_u32(high, 16), mask_ff);
    let g_high = vandq_u32(vshrq_n_u32(high, 8), mask_ff);
    let b_high = vandq_u32(high, mask_ff);

    let zero = vdupq_n_u32(0);
    let mask_low = vcgtq_u32(a_low, zero);
    let mask_high = vcgtq_u32(a_high, zero);

    let one = vdupq_n_u32(1);
    let valid_low = vandq_u32(mask_low, one);
    let valid_high = vandq_u32(mask_high, one);

    let total_valid = vaddvq_u32(vaddq_u32(valid_low, valid_high));
    if total_valid == 0 {
        return 0;
    }

    let sum_a = vaddvq_u32(vaddq_u32(
        vandq_u32(a_low, mask_low),
        vandq_u32(a_high, mask_high),
    ));
    let sum_r = vaddvq_u32(vaddq_u32(
        vandq_u32(r_low, mask_low),
        vandq_u32(r_high, mask_high),
    ));
    let sum_g = vaddvq_u32(vaddq_u32(
        vandq_u32(g_low, mask_low),
        vandq_u32(g_high, mask_high),
    ));
    let sum_b = vaddvq_u32(vaddq_u32(
        vandq_u32(b_low, mask_low),
        vandq_u32(b_high, mask_high),
    ));

    let avg_a = sum_a / total_valid;
    let avg_r = sum_r / total_valid;
    let avg_g = sum_g / total_valid;
    let avg_b = sum_b / total_valid;

    (avg_a << 24) | (avg_r << 16) | (avg_g << 8) | avg_b
}
