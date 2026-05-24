//! Black-box tests for the TLS throw-slot API. The slot is process-
//! global (a `static AtomicI64` pair), so each test must clear it on
//! entry — Rust test harness runs tests sequentially per-binary
//! unless `--test-threads` is overridden, but be defensive against
//! future shifts.

use torajs_throw::{
    __torajs_throw_check, __torajs_throw_set, __torajs_throw_take, __torajs_throw_take_tag,
};

/// Reset the throw slot before / after each test. The public API
/// doesn't have a "clear" fn — `take` clears active but leaves
/// tag/value undisturbed. We re-set to zero then take, which leaves
/// the slot semantically empty.
fn reset() {
    unsafe {
        __torajs_throw_set(0, 0);
        let _ = __torajs_throw_take();
    }
}

#[test]
fn check_returns_zero_when_idle() {
    reset();
    let v = unsafe { __torajs_throw_check() };
    assert_eq!(v, 0, "no throw pending → check returns 0");
}

#[test]
fn set_then_check_returns_one() {
    reset();
    unsafe { __torajs_throw_set(0x1234, 0xfedc) };
    let v = unsafe { __torajs_throw_check() };
    assert_eq!(v, 1, "after set, check returns 1");
    reset();
}

#[test]
fn take_returns_value_and_clears_active() {
    reset();
    unsafe { __torajs_throw_set(42, 0xdead_beef) };
    let value = unsafe { __torajs_throw_take() };
    assert_eq!(value, 0xdead_beef);
    let active = unsafe { __torajs_throw_check() };
    assert_eq!(active, 0, "take must clear active");
    reset();
}

#[test]
fn take_tag_does_not_clear_active() {
    reset();
    unsafe { __torajs_throw_set(42, 99) };
    let tag = unsafe { __torajs_throw_take_tag() };
    assert_eq!(tag, 42);
    let still_active = unsafe { __torajs_throw_check() };
    assert_eq!(
        still_active, 1,
        "take_tag is peek-only; active must remain set until take"
    );
    let _ = unsafe { __torajs_throw_take() };
    reset();
}

#[test]
fn typical_throw_catch_sequence() {
    // Mirror the catch-block IR shape: take_tag (peek) → take (clear).
    reset();
    unsafe { __torajs_throw_set(7, 0x1111_2222) };

    let pending = unsafe { __torajs_throw_check() };
    assert_eq!(pending, 1);

    let tag = unsafe { __torajs_throw_take_tag() };
    assert_eq!(tag, 7);

    let value = unsafe { __torajs_throw_take() };
    assert_eq!(value, 0x1111_2222);

    let after = unsafe { __torajs_throw_check() };
    assert_eq!(after, 0, "after take, slot is empty");
    reset();
}

#[test]
fn set_overwrites_previous_pending_throw() {
    reset();
    unsafe {
        __torajs_throw_set(1, 0x1111);
        __torajs_throw_set(2, 0x2222);
    }
    let tag = unsafe { __torajs_throw_take_tag() };
    let value = unsafe { __torajs_throw_take() };
    assert_eq!(tag, 2);
    assert_eq!(value, 0x2222);
    reset();
}

#[test]
fn take_after_take_returns_stale_value() {
    // The semantic is "take clears active flag; value/tag stay
    // around as readable bytes". A second take returns the same
    // value — but check() will read 0 so callers shouldn't read it
    // unless check() was 1 since the last clear.
    reset();
    unsafe { __torajs_throw_set(11, 0xcafe) };
    let v1 = unsafe { __torajs_throw_take() };
    assert_eq!(v1, 0xcafe);
    let v2 = unsafe { __torajs_throw_take() };
    assert_eq!(v2, 0xcafe, "documented behavior: bytes stay until next set");
    reset();
}
