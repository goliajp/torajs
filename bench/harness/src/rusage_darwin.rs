//! macOS kernel interfaces the timed runs read: `waitid` without
//! reaping, `proc_pid_rusage`, and the Mach timebase.

use std::ffi::c_void;

pub(crate) const P_PID: i32 = 1;
pub(crate) const WEXITED: i32 = 0x4;
pub(crate) const WNOWAIT: i32 = 0x20;
pub(crate) const RUSAGE_INFO_V6: i32 = 6;

#[repr(C)]
pub(crate) struct SigInfo {
    pub si_signo: i32,
    pub si_errno: i32,
    pub si_code: i32,
    pub si_pid: i32,
    pub si_uid: u32,
    pub si_status: i32,
    pub si_addr: *mut c_void,
    pub si_value: usize,
    pub si_band: isize,
    pub pad: [usize; 7],
}

/// `struct rusage_info_v6` from `<sys/resource.h>`, laid out field
/// for field so the offsets the kernel writes are the ones read.
#[repr(C)]
#[derive(Default)]
pub(crate) struct RusageInfoV6 {
    pub ri_uuid: [u8; 16],
    pub ri_user_time: u64,
    pub ri_system_time: u64,
    pub ri_pkg_idle_wkups: u64,
    pub ri_interrupt_wkups: u64,
    pub ri_pageins: u64,
    pub ri_wired_size: u64,
    pub ri_resident_size: u64,
    pub ri_phys_footprint: u64,
    pub ri_proc_start_abstime: u64,
    pub ri_proc_exit_abstime: u64,
    pub ri_child_user_time: u64,
    pub ri_child_system_time: u64,
    pub ri_child_pkg_idle_wkups: u64,
    pub ri_child_interrupt_wkups: u64,
    pub ri_child_pageins: u64,
    pub ri_child_elapsed_abstime: u64,
    pub ri_diskio_bytesread: u64,
    pub ri_diskio_byteswritten: u64,
    pub ri_cpu_time_qos_default: u64,
    pub ri_cpu_time_qos_maintenance: u64,
    pub ri_cpu_time_qos_background: u64,
    pub ri_cpu_time_qos_utility: u64,
    pub ri_cpu_time_qos_legacy: u64,
    pub ri_cpu_time_qos_user_initiated: u64,
    pub ri_cpu_time_qos_user_interactive: u64,
    pub ri_billed_system_time: u64,
    pub ri_serviced_system_time: u64,
    pub ri_logical_writes: u64,
    pub ri_lifetime_max_phys_footprint: u64,
    pub ri_instructions: u64,
    pub ri_cycles: u64,
    pub ri_billed_energy: u64,
    pub ri_serviced_energy: u64,
    pub ri_interval_max_phys_footprint: u64,
    pub ri_runnable_time: u64,
    pub ri_flags: u64,
    pub ri_user_ptime: u64,
    pub ri_system_ptime: u64,
    pub ri_pinstructions: u64,
    pub ri_pcycles: u64,
    pub ri_energy_nj: u64,
    pub ri_penergy_nj: u64,
    pub ri_secure_time_in_system: u64,
    pub ri_secure_ptime_in_system: u64,
    pub ri_neural_footprint: u64,
    pub ri_lifetime_max_neural_footprint: u64,
    pub ri_interval_max_neural_footprint: u64,
    pub ri_conclave_footprint: u64,
    pub ri_page_wait_time_mach: u64,
    pub ri_page_cache_hits: u64,
    pub ri_reserved: [u64; 6],
}

#[repr(C)]
pub(crate) struct MachTimebaseInfo {
    pub numer: u32,
    pub denom: u32,
}

unsafe extern "C" {
    pub(crate) fn waitid(idtype: i32, id: u32, info: *mut SigInfo, options: i32) -> i32;
    pub(crate) fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut c_void) -> i32;
    pub(crate) fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

/// Mach absolute time units → nanoseconds (125/3 on Apple Silicon).
pub(crate) fn abs_to_ns(t: u64) -> f64 {
    let mut tb = MachTimebaseInfo { numer: 0, denom: 0 };
    unsafe { mach_timebase_info(&mut tb) };
    if tb.denom == 0 {
        return t as f64;
    }
    t as f64 * tb.numer as f64 / tb.denom as f64
}
