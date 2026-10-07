//! Timed runs measured by the harness itself.
//!
//! Each run is spawned directly, timed with a monotonic clock around
//! the spawn, and its resource account is read from the kernel before
//! the zombie is reaped: `proc_pid_rusage` on macOS (instructions,
//! cycles, CPU time, footprint, disk I/O, page-ins), `wait4` everywhere
//! (peak RSS). hyperfine only surfaced median / stddev / peak RSS; the
//! counters it dropped are what separate a code change from the
//! machine state on the day.

use std::ffi::c_void;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

const SIGKILL: i32 = 9;

#[repr(C)]
struct Timeval {
    tv_sec: i64,
    tv_usec: i32,
}

/// `struct rusage`: two timevals then 14 longs, `ru_maxrss` first.
/// Bytes on macOS, KiB on Linux.
#[repr(C)]
struct Rusage {
    ru_utime: Timeval,
    ru_stime: Timeval,
    ru_maxrss: i64,
    rest: [i64; 13],
}

unsafe extern "C" {
    fn wait4(pid: i32, status: *mut i32, options: i32, rusage: *mut Rusage) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(target_os = "macos")]
use crate::rusage_darwin as darwin;

/// One measured run.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunSample {
    pub wall_ms: f64,
    pub max_rss_kb: u64,
    pub user_ms: Option<f64>,
    pub system_ms: Option<f64>,
    pub instructions: Option<u64>,
    pub cycles: Option<u64>,
    pub pinstructions: Option<u64>,
    pub pcycles: Option<u64>,
    pub phys_footprint_kb: Option<u64>,
    pub pageins: Option<u64>,
    pub diskio_read_bytes: Option<u64>,
    pub diskio_written_bytes: Option<u64>,
    pub minor_faults: Option<u64>,
    pub major_faults: Option<u64>,
    pub ctx_switches: Option<u64>,
}

/// The per-cell reduction of `runs` samples.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunStats {
    pub median_ms: f64,
    pub stddev_ms: f64,
    pub min_ms: f64,
    pub samples_ms: Vec<f64>,
    pub max_rss_kb: u64,
    pub user_ms: Option<f64>,
    pub system_ms: Option<f64>,
    pub instructions: Option<u64>,
    pub cycles: Option<u64>,
    pub pinstructions: Option<u64>,
    pub pcycles: Option<u64>,
    pub phys_footprint_kb: Option<u64>,
    pub pageins: Option<u64>,
    pub diskio_read_bytes: Option<u64>,
    pub diskio_written_bytes: Option<u64>,
    pub minor_faults: Option<u64>,
    pub major_faults: Option<u64>,
    pub ctx_switches: Option<u64>,
}

/// Spawn `argv` with `env` overlaid, discard its output, and wait for
/// it with the zombie left in place so the kernel's account can be
/// read; a watchdog SIGKILLs the whole process group at `timeout`.
fn run_once(argv: &[String], env: &[(String, String)], timeout: Duration) -> Result<RunSample> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    // A fixed, minimal environment: the size of the environment block
    // moves the stack and with it the measurement (Mytkowicz 2009),
    // so the child sees the same few variables on every machine.
    command.env_clear();
    for k in ["PATH", "HOME", "TMPDIR"] {
        if let Ok(v) = std::env::var(k) {
            command.env(k, v);
        }
    }
    for (k, v) in env {
        command.env(k, v);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let start = Instant::now();
    let child = command
        .spawn()
        .with_context(|| format!("spawn `{}`", argv[0]))?;
    let pid = child.id() as i32;
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if done_rx.recv_timeout(timeout).is_err() {
            unsafe { kill(-pid, SIGKILL) };
            true
        } else {
            false
        }
    });

    let mut sample = RunSample::default();
    #[cfg(target_os = "macos")]
    {
        use darwin::*;
        let mut si = SigInfo {
            si_signo: 0,
            si_errno: 0,
            si_code: 0,
            si_pid: 0,
            si_uid: 0,
            si_status: 0,
            si_addr: std::ptr::null_mut(),
            si_value: 0,
            si_band: 0,
            pad: [0; 7],
        };
        let rc = unsafe { waitid(P_PID, pid as u32, &mut si, WEXITED | WNOWAIT) };
        sample.wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        if rc == 0 {
            let mut ri = RusageInfoV6::default();
            let ok = unsafe {
                proc_pid_rusage(
                    pid,
                    RUSAGE_INFO_V6,
                    (&mut ri as *mut RusageInfoV6).cast::<c_void>(),
                )
            } == 0;
            if ok {
                sample.user_ms = Some(abs_to_ns(ri.ri_user_time) / 1e6);
                sample.system_ms = Some(abs_to_ns(ri.ri_system_time) / 1e6);
                sample.instructions = Some(ri.ri_instructions);
                sample.cycles = Some(ri.ri_cycles);
                sample.pinstructions = Some(ri.ri_pinstructions);
                sample.pcycles = Some(ri.ri_pcycles);
                sample.phys_footprint_kb = Some(ri.ri_lifetime_max_phys_footprint / 1024);
                sample.pageins = Some(ri.ri_pageins);
                sample.diskio_read_bytes = Some(ri.ri_diskio_bytesread);
                sample.diskio_written_bytes = Some(ri.ri_diskio_byteswritten);
            }
        }
    }
    let mut status: i32 = 0;
    let mut ru = Rusage {
        ru_utime: Timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        ru_stime: Timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        ru_maxrss: 0,
        rest: [0; 13],
    };
    let reaped = unsafe { wait4(pid, &mut status, 0, &mut ru) };
    #[cfg(not(target_os = "macos"))]
    {
        sample.wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    }
    let _ = done_tx.send(());
    let timed_out = watchdog.join().unwrap_or(false);
    anyhow::ensure!(
        !timed_out,
        "`{}` timeout after {}s (SIGKILL group)",
        argv[0],
        timeout.as_secs()
    );
    anyhow::ensure!(reaped == pid, "wait4 `{}` failed", argv[0]);
    if status & 0x7f != 0 {
        return Err(crate::run_step::KilledBySignal::from_wait(&argv[0], status).into());
    }
    let code = (status >> 8) & 0xff;
    anyhow::ensure!(code == 0, "`{}` exited with code {code}", argv[0]);
    // ru_minflt, ru_majflt, ru_nvcsw + ru_nivcsw
    sample.minor_faults = Some(ru.rest[3] as u64);
    sample.major_faults = Some(ru.rest[4] as u64);
    sample.ctx_switches = Some((ru.rest[11] + ru.rest[12]) as u64);
    sample.max_rss_kb = if cfg!(target_os = "macos") {
        (ru.ru_maxrss as u64) / 1024
    } else {
        ru.ru_maxrss as u64
    };
    Ok(sample)
}

fn median_f64(xs: &mut [f64]) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

fn median_u64(xs: &mut [u64]) -> u64 {
    xs.sort_unstable();
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2
    }
}

fn median_of<T: Copy, F: Fn(&RunSample) -> Option<T>>(
    samples: &[RunSample],
    get: F,
    reduce: fn(&mut [T]) -> T,
) -> Option<T> {
    let mut xs: Vec<T> = samples.iter().filter_map(&get).collect();
    if xs.is_empty() {
        None
    } else {
        Some(reduce(&mut xs))
    }
}

/// `warmup` unmeasured runs, then `runs` measured ones.
pub(crate) fn measure_runs(
    cmd: &str,
    warmup: u32,
    runs: u32,
    env: &[(String, String)],
    timeout: Duration,
) -> Result<RunStats> {
    let argv = crate::runner::split_cmd(cmd);
    anyhow::ensure!(!argv.is_empty(), "empty command");
    anyhow::ensure!(runs >= 1, "runs must be at least 1");
    for _ in 0..warmup {
        run_once(&argv, env, timeout)?;
    }
    let mut samples = Vec::with_capacity(runs as usize);
    for _ in 0..runs {
        samples.push(run_once(&argv, env, timeout)?);
    }
    let mut walls: Vec<f64> = samples.iter().map(|s| s.wall_ms).collect();
    let samples_ms = walls.clone();
    let median_ms = median_f64(&mut walls);
    let n = walls.len() as f64;
    let mean = walls.iter().sum::<f64>() / n;
    let stddev_ms = if walls.len() > 1 {
        (walls.iter().map(|w| (w - mean) * (w - mean)).sum::<f64>() / (n - 1.0)).sqrt()
    } else {
        0.0
    };
    Ok(RunStats {
        median_ms,
        stddev_ms,
        min_ms: walls[0],
        samples_ms,
        max_rss_kb: samples.iter().map(|s| s.max_rss_kb).max().unwrap_or(0),
        user_ms: median_of(&samples, |s| s.user_ms, median_f64),
        system_ms: median_of(&samples, |s| s.system_ms, median_f64),
        instructions: median_of(&samples, |s| s.instructions, median_u64),
        cycles: median_of(&samples, |s| s.cycles, median_u64),
        pinstructions: median_of(&samples, |s| s.pinstructions, median_u64),
        pcycles: median_of(&samples, |s| s.pcycles, median_u64),
        phys_footprint_kb: samples.iter().filter_map(|s| s.phys_footprint_kb).max(),
        pageins: median_of(&samples, |s| s.pageins, median_u64),
        diskio_read_bytes: median_of(&samples, |s| s.diskio_read_bytes, median_u64),
        diskio_written_bytes: median_of(&samples, |s| s.diskio_written_bytes, median_u64),
        minor_faults: median_of(&samples, |s| s.minor_faults, median_u64),
        major_faults: median_of(&samples, |s| s.major_faults, median_u64),
        ctx_switches: median_of(&samples, |s| s.ctx_switches, median_u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn true_has_samples_and_counters() {
        let st = measure_runs("/usr/bin/true", 1, 3, &[], Duration::from_secs(5)).expect("true");
        assert_eq!(st.samples_ms.len(), 3);
        assert!(st.median_ms > 0.0 && st.min_ms <= st.median_ms);
        assert!(st.max_rss_kb > 0);
        assert!(st.minor_faults.unwrap_or(0) > 0, "{st:?}");
        if cfg!(target_os = "macos") {
            assert!(st.instructions.unwrap_or(0) > 0, "{st:?}");
            assert!(st.phys_footprint_kb.unwrap_or(0) > 0, "{st:?}");
        }
    }

    #[test]
    fn nonzero_exit_is_an_error() {
        let err = measure_runs("false", 0, 1, &[], Duration::from_secs(5)).expect_err("false");
        assert!(err.to_string().contains("exited"), "{err}");
    }

    #[test]
    fn timeout_kills_the_group() {
        let start = Instant::now();
        let err = measure_runs(
            "sh -c sleep 30 & wait",
            0,
            1,
            &[],
            Duration::from_millis(200),
        );
        assert!(err.is_err());
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "{:?}",
            start.elapsed()
        );
    }
}
