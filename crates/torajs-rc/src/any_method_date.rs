//! The `Date.prototype` half of the any-method id table — moved out
//! of [`crate::any_method`] verbatim when the 383-04 upsert mid
//! pushed that file past the 500-line hard limit (rotation 405).
//! Same numbering domain, same one-name-per-mid contract; the ids
//! stay wherever they were minted.

/// `Date.prototype.getTime`.
pub const ANY_METHOD_GET_TIME: i64 = 25;
/// `Date.prototype.toISOString`.
pub const ANY_METHOD_TO_ISO_STRING: i64 = 27;
/// `Date.prototype.getFullYear`.
pub const ANY_METHOD_GET_FULL_YEAR: i64 = 29;
/// `Date.prototype.getUTCFullYear`.
pub const ANY_METHOD_GET_UTC_FULL_YEAR: i64 = 30;
/// `Date.prototype.getMonth`.
pub const ANY_METHOD_GET_MONTH: i64 = 31;
/// `Date.prototype.getUTCMonth`.
pub const ANY_METHOD_GET_UTC_MONTH: i64 = 32;
/// `Date.prototype.getDate`.
pub const ANY_METHOD_GET_DATE: i64 = 33;
/// `Date.prototype.getUTCDate`.
pub const ANY_METHOD_GET_UTC_DATE: i64 = 34;
/// `Date.prototype.getHours`.
pub const ANY_METHOD_GET_HOURS: i64 = 35;
/// `Date.prototype.getUTCHours`.
pub const ANY_METHOD_GET_UTC_HOURS: i64 = 36;
/// `Date.prototype.getMinutes`.
pub const ANY_METHOD_GET_MINUTES: i64 = 37;
/// `Date.prototype.getUTCMinutes`.
pub const ANY_METHOD_GET_UTC_MINUTES: i64 = 38;
/// `Date.prototype.getSeconds`.
pub const ANY_METHOD_GET_SECONDS: i64 = 39;
/// `Date.prototype.getUTCSeconds`.
pub const ANY_METHOD_GET_UTC_SECONDS: i64 = 40;
/// `Date.prototype.getMilliseconds`.
pub const ANY_METHOD_GET_MILLISECONDS: i64 = 41;
/// `Date.prototype.getUTCMilliseconds`.
pub const ANY_METHOD_GET_UTC_MILLISECONDS: i64 = 42;
/// `Date.prototype.getDay`.
pub const ANY_METHOD_GET_DAY: i64 = 43;
/// `Date.prototype.getUTCDay`.
pub const ANY_METHOD_GET_UTC_DAY: i64 = 44;
/// `Date.prototype.getTimezoneOffset`.
pub const ANY_METHOD_GET_TIMEZONE_OFFSET: i64 = 45;
/// `Date.prototype.setTime`.
pub const ANY_METHOD_SET_TIME: i64 = 46;
/// `Date.prototype.setYear` (annexB §B.2.4.2).
pub const ANY_METHOD_SET_YEAR: i64 = 47;
/// `Date.prototype.getYear` (annexB §B.2.4.1).
pub const ANY_METHOD_GET_YEAR: i64 = 48;
/// `Date.prototype.toGMTString` (annexB alias of toUTCString).
pub const ANY_METHOD_TO_GMT_STRING: i64 = 49;
/// `Date.prototype.toUTCString`.
pub const ANY_METHOD_TO_UTC_STRING: i64 = 50;
/// `Date.prototype.toDateString`.
pub const ANY_METHOD_TO_DATE_STRING: i64 = 51;
/// `Date.prototype.toLocaleDateString`.
pub const ANY_METHOD_TO_LOCALE_DATE_STRING: i64 = 53;
/// `Date.prototype.toLocaleTimeString`.
pub const ANY_METHOD_TO_LOCALE_TIME_STRING: i64 = 54;
/// `Date.prototype.setFullYear`.
pub const ANY_METHOD_SET_FULL_YEAR: i64 = 55;
/// `Date.prototype.setMonth`.
pub const ANY_METHOD_SET_MONTH: i64 = 56;
/// `Date.prototype.setDate`.
pub const ANY_METHOD_SET_DATE: i64 = 57;
/// `Date.prototype.setHours`.
pub const ANY_METHOD_SET_HOURS: i64 = 58;
/// `Date.prototype.setMinutes`.
pub const ANY_METHOD_SET_MINUTES: i64 = 59;
/// `Date.prototype.setSeconds`.
pub const ANY_METHOD_SET_SECONDS: i64 = 60;
/// `Date.prototype.setMilliseconds`.
pub const ANY_METHOD_SET_MILLISECONDS: i64 = 61;
/// `Date.prototype.setUTCFullYear`.
pub const ANY_METHOD_SET_UTC_FULL_YEAR: i64 = 108;
/// `Date.prototype.setUTCMonth`.
pub const ANY_METHOD_SET_UTC_MONTH: i64 = 109;
/// `Date.prototype.setUTCDate`.
pub const ANY_METHOD_SET_UTC_DATE: i64 = 110;
/// `Date.prototype.setUTCHours`.
pub const ANY_METHOD_SET_UTC_HOURS: i64 = 111;
/// `Date.prototype.setUTCMinutes`.
pub const ANY_METHOD_SET_UTC_MINUTES: i64 = 112;
/// `Date.prototype.setUTCSeconds`.
pub const ANY_METHOD_SET_UTC_SECONDS: i64 = 113;
/// `Date.prototype.setUTCMilliseconds`.
pub const ANY_METHOD_SET_UTC_MILLISECONDS: i64 = 114;
/// `Date.prototype.toTimeString` (§21.4.4.42 — RFC 20260721 刀 5).
pub const ANY_METHOD_TO_TIME_STRING: i64 = 160;
