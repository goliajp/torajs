//! The probe itself; see `main.rs`.

#[path = "sig.rs"]
mod sig;

use std::ffi::c_void;
use std::os::fd::AsRawFd;
use std::process::Command;
use std::time::Instant;

const PAGE: usize = 0x4000;
/// 149 pages: the prelinked runtime text is 2427552 bytes.
const PAGES: usize = 149;
const LEN: usize = PAGE * PAGES;
const BASE: usize = 0x100_0000_0000;

const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const PROT_EXEC: i32 = 4;
const MAP_PRIVATE: i32 = 2;
const MAP_FIXED: i32 = 0x10;
const MAP_JIT: i32 = 0x800;
const MAP_ANON: i32 = 0x1000;

unsafe extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64)
    -> *mut c_void;
    fn mprotect(addr: *mut c_void, len: usize, prot: i32) -> i32;
    fn pthread_jit_write_protect_np(enabled: i32);
    fn sys_icache_invalidate(start: *mut c_void, len: usize);
    fn mach_vm_allocate(task: u32, addr: *mut u64, size: u64, flags: i32) -> i32;
    fn mach_vm_deallocate(task: u32, addr: u64, size: u64) -> i32;
    #[link_name = "_mh_execute_header"]
    static MH_EXECUTE_HEADER: u8;
    static mach_task_self_: u32;
}

/// `VM_FLAGS_FIXED` without `VM_FLAGS_OVERWRITE`.
const VM_FLAGS_FIXED: i32 = 0;

#[repr(C, align(16384))]
struct Blob([u8; LEN]);

const fn put(b: &mut [u8; LEN], at: usize, word: u32) {
    let w = word.to_le_bytes();
    (b[at], b[at + 1], b[at + 2], b[at + 3]) = (w[0], w[1], w[2], w[3]);
}

const fn blob() -> [u8; LEN] {
    let mut b = [0u8; LEN];
    let mut i = 0;
    while i < PAGES {
        put(&mut b, i * PAGE, 0xD280_0000 | ((i as u32) << 5));
        put(&mut b, i * PAGE + 4, 0xD65F_03C0);
        i += 1;
    }
    b
}

static BLOB: Blob = Blob(blob());

fn fail(what: &str) -> ! {
    eprintln!("{what}: {}", std::io::Error::last_os_error());
    std::process::exit(3);
}

fn mapped(p: *mut c_void, what: &str) -> *mut u8 {
    if p as isize == -1 {
        fail(what);
    }
    p.cast()
}

/// The blob's offset in the executable file: the main image's
/// `__TEXT` starts at file offset 0 and at the mach header.
fn blob_file_off() -> i64 {
    let header = &raw const MH_EXECUTE_HEADER as usize;
    (BLOB.0.as_ptr() as usize - header) as i64
}

/// Which candidate bases a fixed, non-overwriting allocation of the
/// probe's size gets (kern_return 0), then where an anonymous mmap
/// hinted at the base lands.
fn scan_bases() {
    let mut line = String::from("fixed allocation:");
    for shift in [32u32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 44, 46] {
        for mult in [1u64, 3] {
            let want = mult << shift;
            let mut addr = want;
            let task = unsafe { mach_task_self_ };
            let kr = unsafe { mach_vm_allocate(task, &mut addr, LEN as u64, VM_FLAGS_FIXED) };
            if kr == 0 {
                unsafe { mach_vm_deallocate(task, addr, LEN as u64) };
            }
            line.push_str(&format!(" {want:#x}={kr}"));
        }
    }
    println!("{line}");
    let hint = unsafe { mmap(BASE as *mut c_void, LEN, 0, MAP_PRIVATE | MAP_ANON, -1, 0) };
    println!(
        "mmap hinted at the base lands at {:p}",
        mapped(hint, "hinted mmap")
    );
}

/// Claim the base with a fixed allocation that fails instead of
/// replacing anything already mapped there; the mappings below then
/// replace the claim with MAP_FIXED.
fn reserve() -> *mut u8 {
    let mut addr = BASE as u64;
    let kr = unsafe { mach_vm_allocate(mach_task_self_, &mut addr, LEN as u64, VM_FLAGS_FIXED) };
    if kr != 0 {
        eprintln!("mach_vm_allocate at {BASE:#x}: kern_return {kr}");
        std::process::exit(4);
    }
    addr as *mut u8
}

fn map_file(path: &std::path::Path, at: *mut u8) {
    let f = std::fs::File::open(path).unwrap_or_else(|_| fail("open"));
    let flags = MAP_PRIVATE | MAP_FIXED;
    let prot = PROT_READ | PROT_EXEC;
    let p = unsafe { mmap(at.cast(), LEN, prot, flags, f.as_raw_fd(), blob_file_off()) };
    mapped(p, "mmap file");
}

/// MAP_JIT refuses MAP_FIXED, so the claim is dropped and the base is
/// passed as a hint.
fn map_jit(at: *mut u8) {
    unsafe { mach_vm_deallocate(mach_task_self_, at as u64, LEN as u64) };
    let flags = MAP_PRIVATE | MAP_ANON | MAP_JIT;
    let prot = PROT_READ | PROT_WRITE | PROT_EXEC;
    let p = mapped(
        unsafe { mmap(at.cast(), LEN, prot, flags, -1, 0) },
        "mmap jit",
    );
    if p != at {
        eprintln!("MAP_JIT landed at {p:p}, not {at:p}");
        std::process::exit(4);
    }
    unsafe {
        pthread_jit_write_protect_np(0);
        std::ptr::copy_nonoverlapping(BLOB.0.as_ptr(), at, LEN);
        pthread_jit_write_protect_np(1);
        sys_icache_invalidate(at.cast(), LEN);
    }
}

fn map_anon_rx(at: *mut u8) {
    let flags = MAP_PRIVATE | MAP_ANON | MAP_FIXED;
    let p = unsafe { mmap(at.cast(), LEN, PROT_READ | PROT_WRITE, flags, -1, 0) };
    mapped(p, "mmap anon");
    unsafe {
        std::ptr::copy_nonoverlapping(BLOB.0.as_ptr(), at, LEN);
        if mprotect(at.cast(), LEN, PROT_READ | PROT_EXEC) != 0 {
            fail("mprotect");
        }
        sys_icache_invalidate(at.cast(), LEN);
    }
}

fn call_every_page(at: *mut u8) {
    for i in 0..PAGES {
        let f: extern "C" fn() -> u64 = unsafe { std::mem::transmute(at.add(i * PAGE)) };
        let got = f();
        if got != i as u64 {
            eprintln!("page {i} returned {got}");
            std::process::exit(5);
        }
    }
}

fn child(mode: &str, file: Option<&str>) {
    let t0 = Instant::now();
    let at = reserve();
    let t1 = Instant::now();
    let exe = std::env::current_exe().unwrap_or_else(|_| fail("current_exe"));
    match mode {
        "file" => map_file(&exe, at),
        "copy" | "tampered" => {
            sig::add_file_sigs(file.expect("file"));
            map_file(std::path::Path::new(file.expect("file")), at);
        }
        "jit" => map_jit(at),
        "anon-rx" => map_anon_rx(at),
        _ => panic!("unknown mode {mode}"),
    }
    let t2 = Instant::now();
    call_every_page(at);
    let t3 = Instant::now();
    let us = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1e6;
    println!("{:.1} {:.1} {:.1}", us(t0, t1), us(t1, t2), us(t2, t3));
}

/// A copy of the executable, still carrying the original signature;
/// with `tamper`, the first instruction of blob page 5 is changed.
fn exe_copy(tamper: bool) -> String {
    let exe = std::env::current_exe().expect("current_exe");
    let mut bytes = std::fs::read(&exe).expect("read exe");
    if tamper {
        bytes[blob_file_off() as usize + 5 * PAGE] ^= 0x20;
    }
    let name = format!("probe-selfmap-{}-{tamper}", std::process::id());
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes).expect("write copy");
    path.to_string_lossy().into_owned()
}

fn report(mode: &str, runs: usize, file: Option<&str>) {
    let exe = std::env::current_exe().expect("current_exe");
    let mut rows: Vec<[f64; 3]> = Vec::new();
    for _ in 0..runs {
        let mut cmd = Command::new(&exe);
        cmd.arg("--child").arg(mode);
        if let Some(f) = file {
            cmd.arg(f);
        }
        let out = cmd.output().expect("spawn child");
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            println!("{mode:<9} died: {:?} {}", out.status, err.trim());
            return;
        }
        let line = String::from_utf8_lossy(&out.stdout);
        let v: Vec<f64> = line
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        rows.push([v[0], v[1], v[2]]);
    }
    let stat = |k: usize| {
        let mut col: Vec<f64> = rows.iter().map(|r| r[k]).collect();
        col.sort_by(f64::total_cmp);
        (col[0], col[col.len() / 2])
    };
    let (r, m, c) = (stat(0), stat(1), stat(2));
    println!(
        "{mode:<9} ok x{runs}  reserve {:.1}/{:.1}  place {:.1}/{:.1}  first calls {:.1}/{:.1}  (us, min/median)",
        r.0, r.1, m.0, m.1, c.0, c.1
    );
}

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--child") {
        child(&args[1], args.get(2).map(String::as_str));
        return;
    }
    let runs = args.first().map_or(15, |s| s.parse().expect("runs"));
    println!(
        "blob {LEN} bytes ({PAGES} pages) at file offset {:#x}, base {BASE:#x}",
        blob_file_off()
    );
    scan_bases();
    report("file", runs, None);
    report("jit", runs, None);
    report("anon-rx", runs, None);
    for (mode, tamper) in [("copy", false), ("tampered", true)] {
        let path = exe_copy(tamper);
        report(mode, 1, Some(&path));
        std::fs::remove_file(&path).expect("remove copy");
    }
}
