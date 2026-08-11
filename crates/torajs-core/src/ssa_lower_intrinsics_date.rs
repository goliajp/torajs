//! Pass 0 `declare_intrinsic` group: Date class runtime (v0.2 #2,
//! Phase 2.0a + 2.0b + 2.0b.2 + T-30 setters/annexB).
//!
//! chunk 117 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-116). 42 declarations covering ctor / drop /
//! valueOf-isoString / local + UTC getters/setters / component ctor +
//! parse.
//!
//! - Ctor / lifecycle / valueOf: `now`, `from_ms`, `drop`,
//!   `now_static` (Date.now), `get_time` (.getTime/.valueOf),
//!   `to_iso_string` (.toISOString).
//! - Setters (T-30): `set_time`, `get_year`/`set_year` (annexB),
//!   `set_full_year`/`set_month`/`set_date`,
//!   `set_hours`/`set_minutes`/`set_seconds`/`set_milliseconds`.
//! - String rendering: `to_gmt_string`, `to_date_string`,
//!   `to_locale_string`, `to_locale_date_string`,
//!   `to_locale_time_string`.
//! - Local getters: `get_full_year`/`get_month`/`get_date` +
//!   `get_hours`/`get_minutes`/`get_seconds`/`get_milliseconds` +
//!   `get_day` + `get_timezone_offset`.
//! - UTC getters: `get_utc_*` mirror of the local getters.
//! - Component ctor / parse (Phase 2.0b.2):
//!   `from_components(y,mo,d,h,mi,s,ms) -> Date`,
//!   `utc_components(...) -> i64` (Date.UTC),
//!   `from_iso(s) -> Date` (`new Date(string)`),
//!   `parse_iso(s) -> F64` (Date.parse).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct DateIds {
    pub date_now: FuncId,
    pub date_from_ms: FuncId,
    pub date_from_value: FuncId,
    pub date_drop: FuncId,
    pub date_now_static: FuncId,
    pub date_get_time: FuncId,
    pub date_to_iso_string: FuncId,
    pub date_to_json: FuncId,
    pub date_set_time: FuncId,
    pub date_get_year: FuncId,
    pub date_set_year: FuncId,
    pub date_to_gmt_string: FuncId,
    pub date_to_date_string: FuncId,
    pub date_to_time_string: FuncId,
    pub date_to_string: FuncId,
    pub date_to_locale_string: FuncId,
    pub date_to_locale_date_string: FuncId,
    pub date_to_locale_time_string: FuncId,
    pub date_set_full_year: FuncId,
    pub date_set_month: FuncId,
    pub date_set_date: FuncId,
    pub date_set_hours: FuncId,
    pub date_set_minutes: FuncId,
    pub date_set_seconds: FuncId,
    pub date_set_milliseconds: FuncId,
    pub date_set_utc_full_year: FuncId,
    pub date_set_utc_month: FuncId,
    pub date_set_utc_date: FuncId,
    pub date_set_utc_hours: FuncId,
    pub date_set_utc_minutes: FuncId,
    pub date_set_utc_seconds: FuncId,
    pub date_set_utc_milliseconds: FuncId,
    pub date_get_full_year: FuncId,
    pub date_get_month: FuncId,
    pub date_get_date: FuncId,
    pub date_get_hours: FuncId,
    pub date_get_minutes: FuncId,
    pub date_get_seconds: FuncId,
    pub date_get_milliseconds: FuncId,
    pub date_get_day: FuncId,
    pub date_get_timezone_offset: FuncId,
    pub date_get_utc_full_year: FuncId,
    pub date_get_utc_month: FuncId,
    pub date_get_utc_date: FuncId,
    pub date_get_utc_hours: FuncId,
    pub date_get_utc_minutes: FuncId,
    pub date_get_utc_seconds: FuncId,
    pub date_get_utc_milliseconds: FuncId,
    pub date_get_utc_day: FuncId,
    pub date_from_components: FuncId,
    pub date_utc_components: FuncId,
    pub date_from_iso: FuncId,
    pub date_parse_iso: FuncId,
}

/// `(Date) -> f64` getter shape — 21 of the 42 declarations
/// (RFC 20260713-date-invalid-time: spec number semantics, NaN when
/// the receiver is an invalid date)
fn d_f64(module: &mut Module, fn_table: &mut HashMap<String, FuncId>, name: &str) -> FuncId {
    declare_intrinsic(module, fn_table, name, &[Type::Date], Type::F64)
}

/// `(Date) -> Str` rendering shape — the 6 to*String declarations
fn d_str(module: &mut Module, fn_table: &mut HashMap<String, FuncId>, name: &str) -> FuncId {
    declare_intrinsic(module, fn_table, name, &[Type::Date], Type::Str)
}

/// `(Date, f64 × n, i64 present-mask) -> f64` per-field setter shape
/// (n = value + optional cascading components, per the T-30 setter
/// family; the trailing i64 mask carries bit k = arg k supplied)
fn d_set(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    name: &str,
    n: usize,
) -> FuncId {
    let mut params = vec![Type::Date];
    params.extend(std::iter::repeat_n(Type::F64, n));
    params.push(Type::I64);
    declare_intrinsic(module, fn_table, name, &params, Type::F64)
}

/// `(Date, f64) -> f64` single-mandatory-arg setter shape
/// (`setTime` / annexB `setYear` — no present mask)
fn d_set1(module: &mut Module, fn_table: &mut HashMap<String, FuncId>, name: &str) -> FuncId {
    declare_intrinsic(module, fn_table, name, &[Type::Date, Type::F64], Type::F64)
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> DateIds {
    let seven_f64 = &[Type::F64; 7];
    DateIds {
        date_now: declare_intrinsic(module, fn_table, "__torajs_date_now", &[], Type::Date),
        date_from_ms: declare_intrinsic(
            module,
            fn_table,
            "__torajs_date_from_ms",
            &[Type::F64],
            Type::Date,
        ),
        date_from_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_date_from_value",
            &[Type::Any],
            Type::Date,
        ),
        date_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_date_drop",
            &[Type::Date],
            Type::Void,
        ),
        date_now_static: declare_intrinsic(
            module,
            fn_table,
            "__torajs_date_now_static",
            &[],
            Type::I64,
        ),
        date_get_time: d_f64(module, fn_table, "__torajs_date_get_time"),
        date_to_iso_string: d_str(module, fn_table, "__torajs_date_to_iso_string"),
        date_to_json: d_str(module, fn_table, "__torajs_date_to_json"),
        date_set_time: d_set1(module, fn_table, "__torajs_date_set_time"),
        date_get_year: d_f64(module, fn_table, "__torajs_date_get_year"),
        date_set_year: d_set1(module, fn_table, "__torajs_date_set_year"),
        date_to_gmt_string: d_str(module, fn_table, "__torajs_date_to_gmt_string"),
        date_to_date_string: d_str(module, fn_table, "__torajs_date_to_date_string"),
        date_to_time_string: d_str(module, fn_table, "__torajs_date_to_time_string"),
        date_to_string: d_str(module, fn_table, "__torajs_date_to_string"),
        date_to_locale_string: d_str(module, fn_table, "__torajs_date_to_locale_string"),
        date_to_locale_date_string: d_str(module, fn_table, "__torajs_date_to_locale_date_string"),
        date_to_locale_time_string: d_str(module, fn_table, "__torajs_date_to_locale_time_string"),
        date_set_full_year: d_set(module, fn_table, "__torajs_date_set_full_year", 3),
        date_set_month: d_set(module, fn_table, "__torajs_date_set_month", 2),
        date_set_date: d_set(module, fn_table, "__torajs_date_set_date", 1),
        date_set_hours: d_set(module, fn_table, "__torajs_date_set_hours", 4),
        date_set_minutes: d_set(module, fn_table, "__torajs_date_set_minutes", 3),
        date_set_seconds: d_set(module, fn_table, "__torajs_date_set_seconds", 2),
        date_set_milliseconds: d_set(module, fn_table, "__torajs_date_set_milliseconds", 1),
        date_set_utc_full_year: d_set(module, fn_table, "__torajs_date_set_utc_full_year", 3),
        date_set_utc_month: d_set(module, fn_table, "__torajs_date_set_utc_month", 2),
        date_set_utc_date: d_set(module, fn_table, "__torajs_date_set_utc_date", 1),
        date_set_utc_hours: d_set(module, fn_table, "__torajs_date_set_utc_hours", 4),
        date_set_utc_minutes: d_set(module, fn_table, "__torajs_date_set_utc_minutes", 3),
        date_set_utc_seconds: d_set(module, fn_table, "__torajs_date_set_utc_seconds", 2),
        date_set_utc_milliseconds: d_set(module, fn_table, "__torajs_date_set_utc_milliseconds", 1),
        date_get_full_year: d_f64(module, fn_table, "__torajs_date_get_full_year"),
        date_get_month: d_f64(module, fn_table, "__torajs_date_get_month"),
        date_get_date: d_f64(module, fn_table, "__torajs_date_get_date"),
        date_get_hours: d_f64(module, fn_table, "__torajs_date_get_hours"),
        date_get_minutes: d_f64(module, fn_table, "__torajs_date_get_minutes"),
        date_get_seconds: d_f64(module, fn_table, "__torajs_date_get_seconds"),
        date_get_milliseconds: d_f64(module, fn_table, "__torajs_date_get_milliseconds"),
        date_get_day: d_f64(module, fn_table, "__torajs_date_get_day"),
        date_get_timezone_offset: d_f64(module, fn_table, "__torajs_date_get_timezone_offset"),
        date_get_utc_full_year: d_f64(module, fn_table, "__torajs_date_get_utc_full_year"),
        date_get_utc_month: d_f64(module, fn_table, "__torajs_date_get_utc_month"),
        date_get_utc_date: d_f64(module, fn_table, "__torajs_date_get_utc_date"),
        date_get_utc_hours: d_f64(module, fn_table, "__torajs_date_get_utc_hours"),
        date_get_utc_minutes: d_f64(module, fn_table, "__torajs_date_get_utc_minutes"),
        date_get_utc_seconds: d_f64(module, fn_table, "__torajs_date_get_utc_seconds"),
        date_get_utc_milliseconds: d_f64(module, fn_table, "__torajs_date_get_utc_milliseconds"),
        date_get_utc_day: d_f64(module, fn_table, "__torajs_date_get_utc_day"),
        date_from_components: declare_intrinsic(
            module,
            fn_table,
            "__torajs_date_from_components",
            seven_f64,
            Type::Date,
        ),
        date_utc_components: declare_intrinsic(
            module,
            fn_table,
            "__torajs_date_utc_components",
            seven_f64,
            Type::F64,
        ),
        date_from_iso: declare_intrinsic(
            module,
            fn_table,
            "__torajs_date_from_iso",
            &[Type::Str],
            Type::Date,
        ),
        date_parse_iso: declare_intrinsic(
            module,
            fn_table,
            "__torajs_date_parse_iso",
            &[Type::Str],
            Type::F64,
        ),
    }
}
