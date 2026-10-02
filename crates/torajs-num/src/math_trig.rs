//! Trigonometric and hyperbolic `Math` intrinsics — `Math.sin(x)`
//! through `Math.atanh(x)`, each a thin wrapper over torajs-math.
//! Re-exported from [`crate::math`], whose module doc carries the
//! libm-equivalent symbol map.

/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sin(x: f64) -> f64 {
    torajs_math::sin(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_cos(x: f64) -> f64 {
    torajs_math::cos(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_tan(x: f64) -> f64 {
    torajs_math::tan(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_asin(x: f64) -> f64 {
    torajs_math::asin(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_acos(x: f64) -> f64 {
    torajs_math::acos(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_atan(x: f64) -> f64 {
    torajs_math::atan(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sinh(x: f64) -> f64 {
    torajs_math::sinh(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_cosh(x: f64) -> f64 {
    torajs_math::cosh(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_tanh(x: f64) -> f64 {
    torajs_math::tanh(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_asinh(x: f64) -> f64 {
    torajs_math::asinh(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_acosh(x: f64) -> f64 {
    torajs_math::acosh(x)
}
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_atanh(x: f64) -> f64 {
    torajs_math::atanh(x)
}
