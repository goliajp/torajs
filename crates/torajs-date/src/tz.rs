//! In-house IANA TZif (RFC 8536) v2 reader.
//!
//! Replaces the libc `localtime_r` / `mktime` timezone lookup — the
//! last libc holdout on the Date local-time path — with a direct parse
//! of `/etc/localtime` via `torajs-syscall` open/read/close. The zone
//! data yields exactly one primitive: [`local_utoff`] — the UTC offset
//! (seconds, DST included) effective at a given instant. `tm.rs`
//! combines it with the pure-Rust `civil` calendar math, so no libc is
//! touched anywhere on the path.
//!
//! Only the subset RFC 8536 calls the "version-2+ data block" is
//! parsed: 8-byte transition times, their type indices, and the
//! `ttinfo` UT-offset table. Leap seconds, designation strings, and the
//! trailing POSIX TZ rule are irrelevant to offset lookup and skipped.

/// One `ttinfo` record: a UT offset plus its DST flag.
struct TtInfo {
    utoff: i32,
    isdst: bool,
}

/// Parsed timezone: sorted transition instants, the type index each
/// transition switches to, and the offset table they index.
pub struct Tz {
    transitions: Vec<i64>,
    types: Vec<u8>,
    ttinfos: Vec<TtInfo>,
}

const HEADER_LEN: usize = 44;

fn be_u32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn be_i32(b: &[u8], o: usize) -> i32 {
    i32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn be_i64(b: &[u8], o: usize) -> i64 {
    i64::from_be_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ])
}

/// The six `u32` counts in a TZif header (offsets 20..44).
struct Counts {
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

fn parse_counts(b: &[u8], o: usize) -> Option<Counts> {
    if b.len() < o + HEADER_LEN {
        return None;
    }
    Some(Counts {
        isutcnt: be_u32(b, o + 20) as usize,
        isstdcnt: be_u32(b, o + 24) as usize,
        leapcnt: be_u32(b, o + 28) as usize,
        timecnt: be_u32(b, o + 32) as usize,
        typecnt: be_u32(b, o + 36) as usize,
        charcnt: be_u32(b, o + 40) as usize,
    })
}

/// Parse a TZif v2/v3 file into a [`Tz`]. Returns `None` on any
/// malformation (bad magic, version 1 only, truncation) — every field
/// access is bounds-checked first so a corrupt file degrades to "no
/// zone info", never a panic.
pub fn parse_tzif(buf: &[u8]) -> Option<Tz> {
    if buf.len() < HEADER_LEN || &buf[0..4] != b"TZif" {
        return None;
    }
    // version 1 has no 8-byte block; we require v2/v3.
    if buf[4] != b'2' && buf[4] != b'3' {
        return None;
    }

    // header 1 → size of the v1 (4-byte-time) block we skip over.
    let h1 = parse_counts(buf, 0)?;
    let v1_block = h1
        .timecnt
        .checked_mul(4)?
        .checked_add(h1.timecnt)?
        .checked_add(h1.typecnt.checked_mul(6)?)?
        .checked_add(h1.charcnt)?
        .checked_add(h1.leapcnt.checked_mul(8)?)?
        .checked_add(h1.isstdcnt)?
        .checked_add(h1.isutcnt)?;

    // header 2 (the v2 block) starts after header1 + v1 block.
    let h2_off = HEADER_LEN.checked_add(v1_block)?;
    if buf.len() < h2_off + HEADER_LEN || &buf[h2_off..h2_off + 4] != b"TZif" {
        return None;
    }
    let h2 = parse_counts(buf, h2_off)?;
    if h2.typecnt == 0 {
        return None;
    }

    // v2 body: 8-byte transition times, then 1-byte type indices, then
    // the 6-byte ttinfo records. Bounds-check the whole span up front.
    let body = h2_off + HEADER_LEN;
    let trans_bytes = h2.timecnt.checked_mul(8)?;
    let tt_bytes = h2.typecnt.checked_mul(6)?;
    let tt_off = body.checked_add(trans_bytes)?.checked_add(h2.timecnt)?;
    let need = tt_off.checked_add(tt_bytes)?;
    if buf.len() < need {
        return None;
    }

    let mut transitions = Vec::with_capacity(h2.timecnt);
    for i in 0..h2.timecnt {
        transitions.push(be_i64(buf, body + i * 8));
    }
    let idx_off = body + trans_bytes;
    let mut types = Vec::with_capacity(h2.timecnt);
    for i in 0..h2.timecnt {
        types.push(buf[idx_off + i]);
    }
    let mut ttinfos = Vec::with_capacity(h2.typecnt);
    for i in 0..h2.typecnt {
        let o = tt_off + i * 6;
        ttinfos.push(TtInfo {
            utoff: be_i32(buf, o),
            isdst: buf[o + 4] != 0,
        });
    }

    Some(Tz {
        transitions,
        types,
        ttinfos,
    })
}

impl Tz {
    /// UT offset (seconds) effective at `t` (seconds since the epoch).
    pub fn utoff_at(&self, t: i64) -> i32 {
        self.info_at(t).0
    }

    /// `(UT offset seconds, is-DST)` effective at `t` — the DST bit
    /// picks between the standard / daylight display names in
    /// [`zone_long_name`].
    pub fn info_at(&self, t: i64) -> (i32, bool) {
        // before the first transition (or no transitions at all): the
        // first non-DST type, else type 0 — RFC 8536 §3.2.
        if self.transitions.is_empty() || t < self.transitions[0] {
            return self
                .ttinfos
                .iter()
                .find(|i| !i.isdst)
                .or_else(|| self.ttinfos.first())
                .map(|i| (i.utoff, i.isdst))
                .unwrap_or((0, false));
        }
        // largest index whose transition instant is ≤ t.
        let idx = self.transitions.partition_point(|&x| x <= t) - 1;
        let ti = *self.types.get(idx).unwrap_or(&0) as usize;
        self.ttinfos
            .get(ti)
            .map(|i| (i.utoff, i.isdst))
            .unwrap_or((0, false))
    }
}

unsafe extern "C" {
    // torajs-process — borrowed lookup in the kernel envp block
    // (cargo test links the lib.rs stub instead).
    fn __torajs_env_lookup_raw(name: *const u8, name_len: i64, out_len: *mut i64) -> *const u8;
}

// Single-threaded runtime (see feedback_mutex_contended_test_segv): a
// plain static cache avoids re-reading + re-parsing /etc/localtime on
// every Date getter. NOT `#[thread_local]` — 16-c-2 established that
// plain statics are metal-safe under build-std, while thread-locals
// drag `__tlv_bootstrap` back into the binary.
static mut TZ_CACHE: Option<Tz> = None;
static mut TZ_TRIED: bool = false;

// TZ env override, probed once — bun/V8 honor TZ over the system
// zone, and because every getter and formatter flows through
// `cached()` / `zone_id()`, the override is uniform across the whole
// Date surface (no getHours-vs-toString split).
static mut TZ_ENV: Option<&'static str> = None;
static mut TZ_ENV_TRIED: bool = false;

/// The `TZ` environment variable as a leaked 'static ("Asia/Tokyo"),
/// or `None` when unset / empty / not valid UTF-8.
fn tz_env_zone() -> Option<&'static str> {
    unsafe {
        if !*(&raw const TZ_ENV_TRIED) {
            *(&raw mut TZ_ENV_TRIED) = true;
            *(&raw mut TZ_ENV) = read_tz_env();
        }
        *(&raw const TZ_ENV)
    }
}

fn read_tz_env() -> Option<&'static str> {
    let name = b"TZ";
    let mut len: i64 = 0;
    let ptr = unsafe { __torajs_env_lookup_raw(name.as_ptr(), name.len() as i64, &mut len) };
    if ptr.is_null() || len <= 0 || len > 255 {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
    let id = core::str::from_utf8(bytes).ok()?;
    Some(Box::leak(String::from(id).into_boxed_str()))
}

fn cached() -> Option<&'static Tz> {
    unsafe {
        if !*(&raw const TZ_TRIED) {
            *(&raw mut TZ_TRIED) = true;
            let parsed = read_zone_data().and_then(|buf| parse_tzif(&buf));
            *(&raw mut TZ_CACHE) = parsed;
        }
        (*(&raw const TZ_CACHE)).as_ref()
    }
}

/// The TZif bytes for the effective zone: a `TZ` env zone id resolves
/// through the system zoneinfo directories (macOS then Linux layout),
/// anything else — unset TZ, traversal-shaped ids, missing files —
/// falls back to `/etc/localtime`.
fn read_zone_data() -> Option<Vec<u8>> {
    if let Some(id) = tz_env_zone() {
        if !id.contains("..") && !id.starts_with('/') {
            for dir in ["/var/db/timezone/zoneinfo/", "/usr/share/zoneinfo/"] {
                let mut path = String::with_capacity(dir.len() + id.len() + 1);
                path.push_str(dir);
                path.push_str(id);
                path.push('\0');
                if let Some(buf) = read_file(path.as_bytes()) {
                    return Some(buf);
                }
            }
        }
    }
    read_file(b"/etc/localtime\0")
}

/// Read a file fully via syscalls (no libc `fopen`). `path` is a
/// NUL-terminated byte string.
fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let fd =
        unsafe { torajs_syscall::open(path.as_ptr(), torajs_syscall::sysno::O_RDONLY) }.ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match unsafe { torajs_syscall::read(fd, &mut chunk) } {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => {
                let _ = torajs_syscall::close(fd);
                return None;
            }
        }
    }
    let _ = torajs_syscall::close(fd);
    Some(buf)
}

/// UTC offset in seconds (DST included) effective at `utc_secs`.
/// Degrades to 0 (UTC) if the zone file is missing or unparseable —
/// safe, never a panic.
pub fn local_utoff(utc_secs: i64) -> i32 {
    cached().map(|tz| tz.utoff_at(utc_secs)).unwrap_or(0)
}

/// `(UTC offset seconds, is-DST)` effective at `utc_secs` — degrades
/// to `(0, false)` like [`local_utoff`].
pub fn local_info(utc_secs: i64) -> (i32, bool) {
    cached()
        .map(|tz| tz.info_at(utc_secs))
        .unwrap_or((0, false))
}

// IANA zone id ("Asia/Tokyo") probed once from the /etc/localtime
// symlink target — same plain-static caching rationale as TZ_CACHE.
static mut ZONE_ID: Option<&'static str> = None;
static mut ZONE_ID_TRIED: bool = false;

/// The host's IANA zone id, read from the `/etc/localtime` symlink
/// (macOS points it at `/var/db/timezone/zoneinfo/<id>`; Linux at
/// `/usr/share/zoneinfo/<id>`). `None` when the link is missing, not
/// a symlink, or has no `zoneinfo/` segment.
pub fn zone_id() -> Option<&'static str> {
    unsafe {
        if !*(&raw const ZONE_ID_TRIED) {
            *(&raw mut ZONE_ID_TRIED) = true;
            *(&raw mut ZONE_ID) = read_zone_id();
        }
        *(&raw const ZONE_ID)
    }
}

fn read_zone_id() -> Option<&'static str> {
    // TZ env wins when it names a resolvable zone file — the same
    // gate `read_zone_data` applies, so the display name can never
    // disagree with the offsets in use.
    if let Some(id) = tz_env_zone() {
        if !id.contains("..") && !id.starts_with('/') {
            for dir in ["/var/db/timezone/zoneinfo/", "/usr/share/zoneinfo/"] {
                let mut path = String::with_capacity(dir.len() + id.len() + 1);
                path.push_str(dir);
                path.push_str(id);
                path.push('\0');
                if let Ok(fd) = unsafe {
                    torajs_syscall::open(path.as_ptr(), torajs_syscall::sysno::O_RDONLY)
                } {
                    let _ = torajs_syscall::close(fd);
                    return Some(id);
                }
            }
        }
    }
    let path = b"/etc/localtime\0";
    let mut buf = [0u8; 256];
    let n = unsafe { torajs_syscall::readlink(path.as_ptr(), &mut buf) }.ok()?;
    if n == 0 || n >= buf.len() {
        // empty or truncated target — no reliable id.
        return None;
    }
    let target = &buf[..n];
    let marker = b"zoneinfo/";
    let start = target
        .windows(marker.len())
        .position(|w| w == marker)?
        .checked_add(marker.len())?;
    let id = core::str::from_utf8(&target[start..]).ok()?;
    // 'static via a one-time leak — the id lives for the process.
    Some(Box::leak(String::from(id).into_boxed_str()))
}

/// CLDR en long display name for the host zone at `utc_secs` — e.g.
/// `"Japan Standard Time"`, or the daylight variant when DST is in
/// effect. `None` when the zone id is unknown or not in the table
/// (callers fall back to the numeric `GMT+HHMM` form).
pub fn zone_long_name(utc_secs: i64) -> Option<&'static str> {
    let id = zone_id()?;
    let (_, dst) = local_info(utc_secs);
    let names = &crate::tz_names::TZ_LONG_NAMES;
    let idx = names.binary_search_by(|(z, _, _)| (*z).cmp(id)).ok()?;
    let (_, std_name, dst_name) = names[idx];
    Some(if dst { dst_name } else { std_name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_offset_plausible() {
        // host-zone offset for 2024-06-15T12:00:00Z: a whole-minute
        // multiple within ±14h (the IANA extreme). Loose enough to pass
        // in any CI timezone, strict enough to catch a parse that
        // returns garbage.
        let off = local_utoff(1_718_452_800);
        assert!((-50400..=50400).contains(&off), "off={off} out of ±14h");
        assert_eq!(off % 60, 0, "offset not a whole-minute multiple: {off}");
    }

    /// Build a minimal valid v2 TZif with a single fixed-offset ttinfo
    /// and zero transitions, so `utoff_at` must return `offset` always.
    fn make_min_tzif(offset: i32) -> Vec<u8> {
        fn header(out: &mut Vec<u8>) {
            out.extend_from_slice(b"TZif2"); // magic + version
            out.extend_from_slice(&[0u8; 15]); // reserved
            out.extend_from_slice(&0u32.to_be_bytes()); // isutcnt
            out.extend_from_slice(&0u32.to_be_bytes()); // isstdcnt
            out.extend_from_slice(&0u32.to_be_bytes()); // leapcnt
            out.extend_from_slice(&0u32.to_be_bytes()); // timecnt
            out.extend_from_slice(&1u32.to_be_bytes()); // typecnt = 1
            out.extend_from_slice(&1u32.to_be_bytes()); // charcnt = 1
        }
        let mut out = Vec::new();
        // v1 block: header + 1 ttinfo (6 B) + 1 designation byte.
        header(&mut out);
        out.extend_from_slice(&offset.to_be_bytes()); // utoff
        out.push(0); // isdst
        out.push(0); // desigidx
        out.push(0); // designation "\0"
        // v2 block: header + 1 ttinfo + 1 designation byte.
        header(&mut out);
        out.extend_from_slice(&offset.to_be_bytes());
        out.push(0);
        out.push(0);
        out.push(0);
        out
    }

    #[test]
    fn synthetic_fixed_offset_parses() {
        let buf = make_min_tzif(32400); // +09:00, JST-like
        let tz = parse_tzif(&buf).expect("parse min tzif");
        assert_eq!(tz.utoff_at(0), 32400);
        assert_eq!(tz.utoff_at(2_000_000_000), 32400);
        assert_eq!(tz.utoff_at(-2_000_000_000), 32400);
    }

    #[test]
    fn rejects_non_tzif() {
        assert!(parse_tzif(b"not a tzif file at all").is_none());
        assert!(parse_tzif(b"TZif1\0\0\0").is_none()); // v1 unsupported
        assert!(parse_tzif(b"").is_none());
    }
}
