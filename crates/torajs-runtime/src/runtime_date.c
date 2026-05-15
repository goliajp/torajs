/*
 * runtime_date.c — torajs v0.2 #2 Date class.
 *
 * Heap layout: { universal_heap_header (8B); int64_t ms_since_epoch (8B) }
 *   total 16 bytes; tag = __TORAJS_TAG_DATE (5).
 *
 * Phase 2.0a scope:
 *   - Constructors: `new Date()` (current time) / `new Date(ms)` (from
 *     milliseconds since UNIX epoch).
 *   - Static: `Date.now()` returns ms-since-epoch as i64.
 *   - Instance: `.getTime()` / `.valueOf()` return i64 ms;
 *     `.toISOString()` formats UTC `YYYY-MM-DDTHH:MM:SS.sssZ`.
 *
 * Phase 2.0b will add:
 *   - `new Date(year, month, day, hour?, min?, sec?, ms?)` — components
 *   - `new Date(iso_string)` — ISO 8601 parser
 *   - `Date.parse(s)` / `Date.UTC(...)` static helpers
 *   - getFullYear / getMonth / getDate / getHours / getMinutes /
 *     getSeconds / getMilliseconds / getDay / getUTCFullYear / etc.
 *   - setX counterparts (returns new ms value)
 *   - toString / toLocaleString (basic locale = en-US)
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <limits.h>

/* Mirror of runtime_str.c heap header — binary compatible. The
 * date .o links against `__torajs_rc_dec` and `__torajs_str_alloc_pooled`
 * from runtime_str.c. */

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_DATE      5

#define __TORAJS_STR_HDR_SIZE  16

extern int __torajs_rc_dec(void *p);
extern uint8_t *__torajs_str_alloc_pooled(uint64_t len);

/* Date heap layout. */
typedef struct {
    __torajs_heap_header_t header;
    int64_t ms;       /* ms since UNIX epoch (signed; pre-1970 dates are negative). */
} Date;

/* ============================================================
 * Time source — wall clock in ms since 1970-01-01 UTC.
 * ============================================================ */

static int64_t now_ms(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_REALTIME, &ts) != 0) return 0;
    return (int64_t)ts.tv_sec * 1000 + (int64_t)(ts.tv_nsec / 1000000);
}

/* ============================================================
 * Constructors.
 * ============================================================ */

static Date *date_alloc(int64_t ms) {
    Date *d = (Date *)malloc(sizeof(Date));
    d->header.refcount = 1;
    d->header.type_tag = __TORAJS_TAG_DATE;
    d->header.flags = 0;
    d->ms = ms;
    return d;
}

void *__torajs_date_now(void) {
    return date_alloc(now_ms());
}

void *__torajs_date_from_ms(int64_t ms) {
    return date_alloc(ms);
}

/* Phase 2.0b.2 — component ctor `new Date(year, month, day, hour, min,
 * sec, ms)`. JS spec: LOCAL time interpretation (libc `mktime`).
 * Missing args default to month=0, day=1, the rest 0. The desugar
 * pass pads missing args with -1 sentinel, but we restate defaults
 * here so callers can pass full 7-arg with intentional zeros. */
int64_t __torajs_date_components_to_local_ms(
    int64_t year, int64_t month, int64_t day,
    int64_t hour, int64_t minute, int64_t second, int64_t milli
) {
    /* JS quirk: 0-99 year is interpreted as 1900-1999. */
    if (year >= 0 && year < 100) year += 1900;
    struct tm tm;
    memset(&tm, 0, sizeof(tm));
    tm.tm_year = (int)(year - 1900);
    tm.tm_mon  = (int)month;       /* JS 0-indexed → tm_mon 0-indexed */
    tm.tm_mday = (int)day;
    tm.tm_hour = (int)hour;
    tm.tm_min  = (int)minute;
    tm.tm_sec  = (int)second;
    tm.tm_isdst = -1;              /* let libc decide DST for the zone */
    time_t t = mktime(&tm);
    if (t == (time_t)-1) return 0; /* invalid date — return epoch (best effort) */
    return (int64_t)t * 1000 + milli;
}

void *__torajs_date_from_components(
    int64_t year, int64_t month, int64_t day,
    int64_t hour, int64_t minute, int64_t second, int64_t milli
) {
    return date_alloc(__torajs_date_components_to_local_ms(
        year, month, day, hour, minute, second, milli
    ));
}

/* `Date.UTC(year, month, day, hour, min, sec, ms)` — UTC interpretation.
 * Returns ms. Computed via days_from_civil (inverse of civil_from_days). */
static int64_t days_from_civil(int32_t y, uint32_t m, uint32_t d) {
    /* Howard Hinnant — "days_from_civil": inverse of civil_from_days. */
    if (m <= 2) y -= 1;
    int64_t era = (y >= 0 ? y : y - 399) / 400;
    uint32_t yoe = (uint32_t)(y - era * 400);          /* [0, 399] */
    uint32_t doy = (uint32_t)((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5 + d - 1); /* [0, 365] */
    uint32_t doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + (int64_t)doe - 719468;
}

int64_t __torajs_date_utc_components(
    int64_t year, int64_t month, int64_t day,
    int64_t hour, int64_t minute, int64_t second, int64_t milli
) {
    if (year >= 0 && year < 100) year += 1900;
    /* Normalize month overflow into year (JS `new Date(2020, 13, 1)`
     * means 2021-02-01). */
    int64_t y = year + month / 12;
    int64_t m = month % 12;
    if (m < 0) { m += 12; y -= 1; }
    int64_t days = days_from_civil((int32_t)y, (uint32_t)(m + 1), (uint32_t)day);
    int64_t ms = days * 86400000
               + hour * 3600000
               + minute * 60000
               + second * 1000
               + milli;
    return ms;
}

/* ============================================================
 * ISO 8601 parser. Phase 2.0b.2 — handles the canonical extended
 * format `YYYY-MM-DDTHH:MM:SS.sssZ` and the date-only `YYYY-MM-DD`,
 * plus a few common offset forms (`+HH:MM`, `-HH:MM`, `Z`).
 *
 * Returns ms-since-epoch on success, INT64_MIN sentinel on failure
 * (caller maps to JS's NaN — but tr's i64 has no NaN, so .parse()
 * returning INT64_MIN is the substrate's choice; a Phase 2.0c
 * refinement could route through f64 + NaN encoding once
 * non-Copy NaN values flow cleanly through the type system).
 * ============================================================ */

#define DATE_PARSE_FAIL INT64_MIN

static int read_int(const uint8_t *s, int64_t len, int64_t *i, int n_digits, int64_t *out) {
    if (*i + n_digits > len) return 0;
    int64_t v = 0;
    for (int k = 0; k < n_digits; k++) {
        uint8_t c = s[*i + k];
        if (c < '0' || c > '9') return 0;
        v = v * 10 + (c - '0');
    }
    *i += n_digits;
    *out = v;
    return 1;
}

int64_t __torajs_date_parse_iso(const void *str_ptr) {
    if (!str_ptr) return DATE_PARSE_FAIL;
    const uint8_t *s = (const uint8_t *)str_ptr + __TORAJS_STR_HDR_SIZE;
    int64_t len = *(uint64_t *)((const uint8_t *)str_ptr + 8);
    int64_t i = 0;
    int64_t year, mon, day;
    /* Optional leading sign for extended-year form. */
    int64_t year_sign = 1;
    if (i < len && (s[i] == '+' || s[i] == '-')) {
        if (s[i] == '-') year_sign = -1;
        i++;
        /* Extended year is 6 digits per spec. */
        if (!read_int(s, len, &i, 6, &year)) return DATE_PARSE_FAIL;
    } else {
        if (!read_int(s, len, &i, 4, &year)) return DATE_PARSE_FAIL;
    }
    year *= year_sign;
    mon = 1; day = 1;
    int64_t hour = 0, minute = 0, second = 0, milli = 0;
    int has_time = 0;
    int has_z = 0;
    int tz_sign = 0; int64_t tz_h = 0, tz_m = 0;
    if (i < len && s[i] == '-') {
        i++;
        if (!read_int(s, len, &i, 2, &mon)) return DATE_PARSE_FAIL;
        if (i < len && s[i] == '-') {
            i++;
            if (!read_int(s, len, &i, 2, &day)) return DATE_PARSE_FAIL;
        }
    }
    if (i < len && (s[i] == 'T' || s[i] == ' ')) {
        i++;
        has_time = 1;
        if (!read_int(s, len, &i, 2, &hour)) return DATE_PARSE_FAIL;
        if (i < len && s[i] == ':') {
            i++;
            if (!read_int(s, len, &i, 2, &minute)) return DATE_PARSE_FAIL;
            if (i < len && s[i] == ':') {
                i++;
                if (!read_int(s, len, &i, 2, &second)) return DATE_PARSE_FAIL;
                if (i < len && s[i] == '.') {
                    i++;
                    /* Up to 3 ms digits; pad if shorter. */
                    int digits = 0;
                    while (i < len && digits < 3 && s[i] >= '0' && s[i] <= '9') {
                        milli = milli * 10 + (s[i] - '0');
                        i++; digits++;
                    }
                    while (digits < 3) { milli *= 10; digits++; }
                    /* Skip remaining sub-ms digits (sub-spec extension). */
                    while (i < len && s[i] >= '0' && s[i] <= '9') i++;
                }
            }
        }
        if (i < len) {
            if (s[i] == 'Z') { has_z = 1; i++; }
            else if (s[i] == '+' || s[i] == '-') {
                tz_sign = (s[i] == '+') ? 1 : -1;
                i++;
                if (!read_int(s, len, &i, 2, &tz_h)) return DATE_PARSE_FAIL;
                if (i < len && s[i] == ':') i++;
                read_int(s, len, &i, 2, &tz_m);
            }
        }
    }
    if (i != len) return DATE_PARSE_FAIL;
    /* JS spec: date-only ISO (YYYY-MM-DD) is treated as UTC. With time
     * and no Z/offset → local time. With Z / offset → UTC adjusted by
     * the offset. */
    int64_t ms;
    if (!has_time) {
        ms = __torajs_date_utc_components(year, mon - 1, day, 0, 0, 0, 0);
    } else if (has_z) {
        ms = __torajs_date_utc_components(year, mon - 1, day, hour, minute, second, milli);
    } else if (tz_sign != 0) {
        ms = __torajs_date_utc_components(year, mon - 1, day, hour, minute, second, milli);
        int64_t off_ms = (tz_h * 60 + tz_m) * 60 * 1000 * tz_sign;
        ms -= off_ms;  /* offset means "local = UTC + offset" → subtract. */
    } else {
        ms = __torajs_date_components_to_local_ms(year, mon - 1, day, hour, minute, second, milli);
    }
    return ms;
}

void *__torajs_date_from_iso(const void *str_ptr) {
    int64_t ms = __torajs_date_parse_iso(str_ptr);
    if (ms == DATE_PARSE_FAIL) ms = 0;  /* Best-effort; spec says NaN. */
    return date_alloc(ms);
}

/* ============================================================
 * Drop.
 * ============================================================ */

void __torajs_date_drop(void *d_ptr) {
    if (!d_ptr) return;
    if (!__torajs_rc_dec(d_ptr)) return;
    free(d_ptr);
}

/* ============================================================
 * Static methods.
 * ============================================================ */

int64_t __torajs_date_now_static(void) {
    return now_ms();
}

/* ============================================================
 * Instance methods.
 * ============================================================ */

int64_t __torajs_date_get_time(const void *d_ptr) {
    if (!d_ptr) return 0;
    return ((const Date *)d_ptr)->ms;
}

/* Forward decl for T-30 helpers below — civil_from_days lives further
 * down (after toISOString). */
static void civil_from_days(int64_t z, int32_t *out_y, uint32_t *out_m, uint32_t *out_d);

/* T-30 — Date setters + annexB methods. ECMAScript §B.2.4 (annexB)
 * defines `getYear` / `setYear` for legacy compat: getYear returns
 * year - 1900 (so 2026 → 126); setYear takes a year and writes
 * year < 100 ? year + 1900 : year. `setTime` is per §21.4.4.27 —
 * overwrite the ms slot. `toGMTString` is §B.2.4.3 — alias for
 * toUTCString.
 *
 * All setters mutate in place and return the new ms value (per spec
 * — Date.prototype.setX returns the new time). NULL receiver is a
 * defensive no-op rather than a TypeError; check.rs's static type
 * guards prevent it for typed Type::Date but the runtime helper
 * stays defensive for Any-tagged dispatch. */

int64_t __torajs_date_set_time(void *d_ptr, int64_t ms) {
    if (!d_ptr) return 0;
    ((Date *)d_ptr)->ms = ms;
    return ms;
}

/* Forward decl — localtime_decompose is below the getX accessors. */
static void localtime_decompose(int64_t ms, struct tm *out);

int64_t __torajs_date_get_year(const void *d_ptr) {
    if (!d_ptr) return 0;
    /* annexB §B.2.4.1: year - 1900 in LOCAL time, matching getFullYear's
     * timezone (1995 → 95, 2026 → 126, 1899 → -1). Use localtime_r
     * so behavior matches getFullYear / setYear's local-time interp. */
    struct tm tm;
    localtime_decompose(((const Date *)d_ptr)->ms, &tm);
    return (int64_t)tm.tm_year;  /* tm_year is already year - 1900 */
}

int64_t __torajs_date_set_year(void *d_ptr, int64_t year) {
    if (!d_ptr) return 0;
    /* annexB: 0 ≤ year < 100 → year += 1900. Otherwise use as-is.
     * Read existing month/day/time-of-day in LOCAL time, recompose
     * with the new year. */
    if (year >= 0 && year < 100) {
        year += 1900;
    }
    struct tm tm;
    localtime_decompose(((Date *)d_ptr)->ms, &tm);
    int64_t cur_ms = ((Date *)d_ptr)->ms;
    int64_t day_ms = 86400000;
    int64_t days = cur_ms / day_ms;
    int64_t tod = cur_ms - days * day_ms;
    if (tod < 0) { tod += day_ms; }
    int64_t new_ms = __torajs_date_components_to_local_ms(
        (int32_t)year, tm.tm_mon, tm.tm_mday,
        tm.tm_hour, tm.tm_min, tm.tm_sec,
        (int32_t)(tod % 1000));
    ((Date *)d_ptr)->ms = new_ms;
    return new_ms;
}

/* toGMTString = toUTCString (annexB alias). Format: "Wed, 14 Jun 2017
 * 07:00:00 GMT". */
void *__torajs_date_to_gmt_string(const void *d_ptr) {
    if (!d_ptr) {
        uint8_t *s = __torajs_str_alloc_pooled(0);
        return s;
    }
    int64_t ms = ((const Date *)d_ptr)->ms;
    int64_t day_ms = 86400000;
    int64_t days = ms / day_ms;
    int64_t tod = ms - days * day_ms;
    if (tod < 0) { tod += day_ms; days -= 1; }
    int32_t year;
    uint32_t month, mday;
    civil_from_days(days, &year, &month, &mday);
    int64_t hour = tod / 3600000;
    int64_t rem = tod - hour * 3600000;
    int64_t minute = rem / 60000;
    rem -= minute * 60000;
    int64_t second = rem / 1000;
    /* Day of week: 1970-01-01 was a Thursday (=4). days mod 7 with
     * (n + 4) % 7 gives Sun=0..Sat=6. */
    int64_t dow = ((days % 7) + 4 + 7) % 7;
    static const char *day_names[] = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"};
    static const char *month_names[] = {
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
    };
    char buf[40];
    int n = snprintf(buf, sizeof(buf),
                     "%s, %02u %s %04d %02lld:%02lld:%02lld GMT",
                     day_names[dow], mday, month_names[month - 1], year,
                     (long long)hour, (long long)minute, (long long)second);
    if (n < 0) n = 0;
    if (n > (int)sizeof(buf) - 1) n = sizeof(buf) - 1;
    uint8_t *s = __torajs_str_alloc_pooled((uint64_t)n);
    memcpy(s + __TORAJS_STR_HDR_SIZE, buf, (size_t)n);
    return s;
}

/* Decompose ms-since-epoch into {y, m, d, hour, min, sec, ms}.
 * Used by every getX accessor and by toISOString. UTC by design;
 * locale-time variants come with timezone work in Phase 2.0c. */
static void decompose(int64_t ms,
                      int32_t *y, uint32_t *m, uint32_t *d,
                      int32_t *hour, int32_t *minute,
                      int32_t *second, int32_t *milli) {
    int64_t day_ms = 86400000;
    int64_t days = ms / day_ms;
    int64_t tod  = ms - days * day_ms;
    if (tod < 0) { tod += day_ms; days -= 1; }
    civil_from_days(days, y, m, d);
    *hour = (int32_t)(tod / 3600000);
    int64_t rem = tod - (int64_t)(*hour) * 3600000;
    *minute = (int32_t)(rem / 60000);
    rem -= (int64_t)(*minute) * 60000;
    *second = (int32_t)(rem / 1000);
    *milli  = (int32_t)(rem - (int64_t)(*second) * 1000);
}

/* Phase 2.0b — UTC getters. JS spec separates local-time accessors
 * (`getFullYear`, etc.) from UTC accessors (`getUTCFullYear` etc.).
 * tr maps both to per-direction helpers below: UTC variants run the
 * branch-free civil_from_days arithmetic; local variants delegate to
 * libc's `localtime_r` so they honor the TZ environment. */
int64_t __torajs_date_get_utc_full_year(const void *d_ptr) {
    if (!d_ptr) return 0;
    int32_t y; uint32_t m, d; int32_t h, mi, s, ms;
    decompose(((const Date *)d_ptr)->ms, &y, &m, &d, &h, &mi, &s, &ms);
    return y;
}
int64_t __torajs_date_get_utc_month(const void *d_ptr) {
    if (!d_ptr) return 0;
    int32_t y; uint32_t m, d; int32_t h, mi, s, ms;
    decompose(((const Date *)d_ptr)->ms, &y, &m, &d, &h, &mi, &s, &ms);
    return (int64_t)m - 1; /* JS 0-indexed */
}
int64_t __torajs_date_get_utc_date(const void *d_ptr) {
    if (!d_ptr) return 0;
    int32_t y; uint32_t m, d; int32_t h, mi, s, ms;
    decompose(((const Date *)d_ptr)->ms, &y, &m, &d, &h, &mi, &s, &ms);
    return d;
}
int64_t __torajs_date_get_utc_hours(const void *d_ptr) {
    if (!d_ptr) return 0;
    int32_t y; uint32_t m, d; int32_t h, mi, s, ms;
    decompose(((const Date *)d_ptr)->ms, &y, &m, &d, &h, &mi, &s, &ms);
    return h;
}
int64_t __torajs_date_get_utc_minutes(const void *d_ptr) {
    if (!d_ptr) return 0;
    int32_t y; uint32_t m, d; int32_t h, mi, s, ms;
    decompose(((const Date *)d_ptr)->ms, &y, &m, &d, &h, &mi, &s, &ms);
    return mi;
}
int64_t __torajs_date_get_utc_seconds(const void *d_ptr) {
    if (!d_ptr) return 0;
    int32_t y; uint32_t m, d; int32_t h, mi, s, ms;
    decompose(((const Date *)d_ptr)->ms, &y, &m, &d, &h, &mi, &s, &ms);
    return s;
}
int64_t __torajs_date_get_utc_milliseconds(const void *d_ptr) {
    if (!d_ptr) return 0;
    int32_t y; uint32_t m, d; int32_t h, mi, s, ms;
    decompose(((const Date *)d_ptr)->ms, &y, &m, &d, &h, &mi, &s, &ms);
    return ms;
}
int64_t __torajs_date_get_utc_day(const void *d_ptr) {
    if (!d_ptr) return 0;
    int64_t day_ms = 86400000;
    int64_t days = ((const Date *)d_ptr)->ms / day_ms;
    int64_t tod  = ((const Date *)d_ptr)->ms - days * day_ms;
    if (tod < 0) days -= 1;
    int64_t dow = (days + 4) % 7;
    if (dow < 0) dow += 7;
    return dow;
}

/* Local-time getters — use localtime_r so the result honors the TZ
 * env var. Match bun (and every other JS engine) which reports
 * the user's local-zone interpretation of the underlying ms. */
static void localtime_decompose(int64_t ms, struct tm *out) {
    /* JS Date allows ms to be larger than libc time_t on some
     * platforms; clamp by truncating to the i64 second value
     * libc accepts on the host. We split so sub-second stays
     * accessible to getMilliseconds. */
    time_t secs = (time_t)(ms / 1000);
    /* localtime_r is POSIX-thread-safe and what JS engines lean on. */
    localtime_r(&secs, out);
}
int64_t __torajs_date_get_full_year(const void *d_ptr) {
    if (!d_ptr) return 0;
    struct tm tm; localtime_decompose(((const Date *)d_ptr)->ms, &tm);
    return tm.tm_year + 1900;
}
int64_t __torajs_date_get_month(const void *d_ptr) {
    if (!d_ptr) return 0;
    struct tm tm; localtime_decompose(((const Date *)d_ptr)->ms, &tm);
    return tm.tm_mon; /* libc tm_mon is already 0-indexed */
}
int64_t __torajs_date_get_date(const void *d_ptr) {
    if (!d_ptr) return 0;
    struct tm tm; localtime_decompose(((const Date *)d_ptr)->ms, &tm);
    return tm.tm_mday;
}
int64_t __torajs_date_get_hours(const void *d_ptr) {
    if (!d_ptr) return 0;
    struct tm tm; localtime_decompose(((const Date *)d_ptr)->ms, &tm);
    return tm.tm_hour;
}
int64_t __torajs_date_get_minutes(const void *d_ptr) {
    if (!d_ptr) return 0;
    struct tm tm; localtime_decompose(((const Date *)d_ptr)->ms, &tm);
    return tm.tm_min;
}
int64_t __torajs_date_get_seconds(const void *d_ptr) {
    if (!d_ptr) return 0;
    struct tm tm; localtime_decompose(((const Date *)d_ptr)->ms, &tm);
    return tm.tm_sec;
}
int64_t __torajs_date_get_milliseconds(const void *d_ptr) {
    if (!d_ptr) return 0;
    /* Sub-second is timezone-invariant. */
    int64_t ms = ((const Date *)d_ptr)->ms;
    int64_t r = ms % 1000;
    if (r < 0) r += 1000;
    return r;
}
int64_t __torajs_date_get_day(const void *d_ptr) {
    if (!d_ptr) return 0;
    struct tm tm; localtime_decompose(((const Date *)d_ptr)->ms, &tm);
    return tm.tm_wday;
}

/* ============================================================
 * toISOString — `YYYY-MM-DDTHH:MM:SS.sssZ` (UTC, ISO 8601 spec).
 *
 * We compute year/month/day from days-since-epoch via the
 * civil-from-days algorithm (Howard Hinnant's date library / C++20
 * <chrono> formula — branch-free, handles full proleptic Gregorian
 * range 0001-9999 correctly).
 * ============================================================ */

/* Howard Hinnant — http://howardhinnant.github.io/date_algorithms.html
 * "civil_from_days" — given z = days from 1970-01-01, returns y/m/d.
 * Handles z negative (pre-1970). */
static void civil_from_days(int64_t z, int32_t *out_y, uint32_t *out_m, uint32_t *out_d) {
    z += 719468;
    int64_t era = (z >= 0 ? z : z - 146096) / 146097;
    uint32_t doe = (uint32_t)(z - era * 146097);                  /* [0, 146097) */
    uint32_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;  /* [0, 400) */
    int32_t y = (int32_t)(yoe) + (int32_t)(era * 400);
    uint32_t doy = doe - (365 * yoe + yoe / 4 - yoe / 100);       /* [0, 365] */
    uint32_t mp = (5 * doy + 2) / 153;                            /* [0, 11] */
    uint32_t d = doy - (153 * mp + 2) / 5 + 1;                    /* [1, 31] */
    uint32_t m = mp < 10 ? mp + 3 : mp - 9;                       /* [1, 12] */
    if (m <= 2) y += 1;
    *out_y = y;
    *out_m = m;
    *out_d = d;
}

void *__torajs_date_to_iso_string(const void *d_ptr) {
    int64_t ms = d_ptr ? ((const Date *)d_ptr)->ms : 0;
    /* Floor-divide ms by 86,400,000 (ms/day) to get days; remainder
     * by 86,400,000 gives the time-of-day (handle negative remainder). */
    int64_t day_ms = 86400000;
    int64_t days = ms / day_ms;
    int64_t tod  = ms - days * day_ms;
    if (tod < 0) { tod += day_ms; days -= 1; }
    int32_t year;
    uint32_t month, mday;
    civil_from_days(days, &year, &month, &mday);
    int64_t hour = tod / 3600000;
    int64_t rem  = tod - hour * 3600000;
    int64_t minute = rem / 60000;
    rem -= minute * 60000;
    int64_t second = rem / 1000;
    int64_t milli  = rem - second * 1000;

    /* JS Date.toISOString format: signed extended-year for year > 9999
     * or year < 0 (e.g. "+010000-01-01T00:00:00.000Z"); otherwise
     * 4-digit year. We only emit the 4-digit form for now (Phase 2.0a
     * scope — pre-0001 / post-9999 dates are deferred). */
    char buf[32];
    int n = snprintf(buf, sizeof(buf),
                     "%04d-%02u-%02uT%02lld:%02lld:%02lld.%03lldZ",
                     year, month, mday,
                     (long long)hour, (long long)minute,
                     (long long)second, (long long)milli);
    if (n < 0) n = 0;
    if (n > (int)sizeof(buf) - 1) n = sizeof(buf) - 1;

    uint8_t *s = __torajs_str_alloc_pooled((uint64_t)n);
    memcpy(s + __TORAJS_STR_HDR_SIZE, buf, (size_t)n);
    return s;
}
