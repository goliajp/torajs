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
