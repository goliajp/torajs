//! Register a copy of this executable's code signature with the kernel,
//! as dyld does before it maps a dylib's code.

use std::ffi::c_void;
use std::os::fd::AsRawFd;

const LC_CODE_SIGNATURE: u32 = 0x1d;
const F_ADDFILESIGS_RETURN: i32 = 97;

unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// The code signature's (file offset, size) from this executable's
/// `LC_CODE_SIGNATURE`; a byte-for-byte copy has it at the same place.
fn code_signature() -> (u32, u32) {
    let header = &raw const super::MH_EXECUTE_HEADER;
    let word = |off: usize| unsafe { header.add(off).cast::<u32>().read_unaligned() };
    let mut at = 32;
    for _ in 0..word(16) {
        if word(at) == LC_CODE_SIGNATURE {
            return (word(at + 8), word(at + 12));
        }
        at += word(at + 4) as usize;
    }
    panic!("no LC_CODE_SIGNATURE");
}

/// `fsignatures_t`: for `F_ADDFILESIGS*`, `blob_start` is the
/// signature's file offset.
#[repr(C)]
struct FSignatures {
    file_start: i64,
    blob_start: *mut c_void,
    blob_size: usize,
    fsignatures_size: usize,
    cdhash: [u8; 20],
    hash_type: i32,
}

pub fn add_file_sigs(path: &str) {
    let f = std::fs::File::open(path).unwrap_or_else(|_| super::fail("open"));
    let (off, size) = code_signature();
    let mut sigs = FSignatures {
        file_start: 0,
        blob_start: off as usize as *mut c_void,
        blob_size: size as usize,
        fsignatures_size: std::mem::size_of::<FSignatures>(),
        cdhash: [0; 20],
        hash_type: 0,
    };
    if unsafe { fcntl(f.as_raw_fd(), F_ADDFILESIGS_RETURN, &mut sigs) } == -1 {
        super::fail("F_ADDFILESIGS_RETURN");
    }
}
