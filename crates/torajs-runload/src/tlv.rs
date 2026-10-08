//! Thread-local variables of the prelinked runtime image, bound the way
//! dyld binds an image's: every TLV descriptor (`thunk`, `key`,
//! `offset`) gets [`tlv_get_addr`] and one pthread key. Code reaches a
//! variable with `ldr x8, [x0]; blr x8`, x0 holding the descriptor;
//! the thunk returns the calling thread's block plus `offset`, and
//! allocates the block from the template on the thread's first access.
//!
//! Callers rely on the thunk preserving every register but x0, x16,
//! x17 and lr, so the slow path saves x1-x15 and q0-q31 around the
//! Rust allocation.

use std::alloc::Layout;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};

/// Layout of a descriptor in the image's data region.
#[repr(C)]
pub struct TlvDescriptor {
    pub thunk: usize,
    pub key: usize,
    pub offset: usize,
}

/// The per-thread block: `block_len` bytes, zero but for `init` (the
/// image's loaded `__thread_data`) copied in at `init_at`. [`bind`]
/// refuses a template whose `init` does not fit.
#[derive(Clone, Copy)]
pub struct TlvTemplate {
    pub init: &'static [u8],
    pub init_at: usize,
    pub block_len: usize,
}

/// What every descriptor of the image is filled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TlvBinding {
    pub thunk: usize,
    pub key: usize,
}

/// Why [`bind`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindError {
    /// `init` placed at `init_at` runs past `block_len`.
    InitOutsideBlock,
    /// `block_len` is too large for one allocation.
    BlockTooLarge,
    /// A template is already bound; blocks allocated from it are freed
    /// with its layout, so it cannot change.
    AlreadyBound,
    /// `pthread_key_create` failed with this code.
    KeyCreate(i32),
}

const BLOCK_ALIGN: usize = 16;

static BOUND: AtomicBool = AtomicBool::new(false);
static INIT: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static INIT_LEN: AtomicUsize = AtomicUsize::new(0);
static INIT_AT: AtomicUsize = AtomicUsize::new(0);
static BLOCK_LEN: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    fn pthread_key_create(key: *mut usize, dtor: Option<unsafe extern "C" fn(*mut u8)>) -> i32;
    fn pthread_setspecific(key: usize, value: *const u8) -> i32;
}

fn block_layout() -> Layout {
    let len = BLOCK_LEN.load(Relaxed).max(1);
    Layout::from_size_align(len, BLOCK_ALIGN).expect("tlv block layout")
}

unsafe extern "C" fn free_block(block: *mut u8) {
    unsafe { std::alloc::dealloc(block, block_layout()) };
}

/// The slow path: give the calling thread its block and return the
/// variable's address in it.
unsafe extern "C" fn allocate(desc: *const TlvDescriptor) -> *mut u8 {
    let layout = block_layout();
    let block = unsafe { std::alloc::alloc_zeroed(layout) };
    if block.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    let init = INIT.load(Relaxed);
    unsafe {
        let at = block.add(INIT_AT.load(Relaxed));
        std::ptr::copy_nonoverlapping(init, at, INIT_LEN.load(Relaxed));
        let desc = &*desc;
        if pthread_setspecific(desc.key, block) != 0 {
            std::process::abort();
        }
        block.add(desc.offset)
    }
}

/// The descriptor thunk. Fast path as in dyld: the thread's slot for
/// the key is read straight from the TSD array TPIDRRO_EL0 points at
/// (its low three bits hold the CPU number), which is where
/// `pthread_setspecific` stores it.
///
/// # Safety
///
/// Called only through a descriptor [`bind`]'s binding filled in, with
/// x0 pointing at that descriptor.
#[unsafe(naked)]
pub unsafe extern "C" fn tlv_get_addr() {
    std::arch::naked_asm!(
        "ldr x16, [x0, #8]",
        "mrs x17, TPIDRRO_EL0",
        "and x17, x17, #-8",
        "ldr x17, [x17, x16, lsl #3]",
        "cbz x17, 2f",
        "ldr x16, [x0, #16]",
        "add x0, x17, x16",
        "ret",
        "2:",
        "stp x29, x30, [sp, #-16]!",
        "mov x29, sp",
        "sub sp, sp, #640",
        "stp x1, x2, [sp, #0]",
        "stp x3, x4, [sp, #16]",
        "stp x5, x6, [sp, #32]",
        "stp x7, x8, [sp, #48]",
        "stp x9, x10, [sp, #64]",
        "stp x11, x12, [sp, #80]",
        "stp x13, x14, [sp, #96]",
        "str x15, [sp, #112]",
        "stp q0, q1, [sp, #128]",
        "stp q2, q3, [sp, #160]",
        "stp q4, q5, [sp, #192]",
        "stp q6, q7, [sp, #224]",
        "stp q8, q9, [sp, #256]",
        "stp q10, q11, [sp, #288]",
        "stp q12, q13, [sp, #320]",
        "stp q14, q15, [sp, #352]",
        "stp q16, q17, [sp, #384]",
        "stp q18, q19, [sp, #416]",
        "stp q20, q21, [sp, #448]",
        "stp q22, q23, [sp, #480]",
        "stp q24, q25, [sp, #512]",
        "stp q26, q27, [sp, #544]",
        "stp q28, q29, [sp, #576]",
        "stp q30, q31, [sp, #608]",
        "bl {allocate}",
        "ldp x1, x2, [sp, #0]",
        "ldp x3, x4, [sp, #16]",
        "ldp x5, x6, [sp, #32]",
        "ldp x7, x8, [sp, #48]",
        "ldp x9, x10, [sp, #64]",
        "ldp x11, x12, [sp, #80]",
        "ldp x13, x14, [sp, #96]",
        "ldr x15, [sp, #112]",
        "ldp q0, q1, [sp, #128]",
        "ldp q2, q3, [sp, #160]",
        "ldp q4, q5, [sp, #192]",
        "ldp q6, q7, [sp, #224]",
        "ldp q8, q9, [sp, #256]",
        "ldp q10, q11, [sp, #288]",
        "ldp q12, q13, [sp, #320]",
        "ldp q14, q15, [sp, #352]",
        "ldp q16, q17, [sp, #384]",
        "ldp q18, q19, [sp, #416]",
        "ldp q20, q21, [sp, #448]",
        "ldp q22, q23, [sp, #480]",
        "ldp q24, q25, [sp, #512]",
        "ldp q26, q27, [sp, #544]",
        "ldp q28, q29, [sp, #576]",
        "ldp q30, q31, [sp, #608]",
        "mov sp, x29",
        "ldp x29, x30, [sp], #16",
        "ret",
        allocate = sym allocate,
    );
}

/// Record the block template and create the key. A process binds one
/// image, once, before any of its code runs; blocks are freed by the
/// key's destructor when their thread exits.
pub fn bind(template: TlvTemplate) -> Result<TlvBinding, BindError> {
    let fits = template
        .init_at
        .checked_add(template.init.len())
        .is_some_and(|end| end <= template.block_len);
    if !fits {
        return Err(BindError::InitOutsideBlock);
    }
    if Layout::from_size_align(template.block_len.max(1), BLOCK_ALIGN).is_err() {
        return Err(BindError::BlockTooLarge);
    }
    if BOUND
        .compare_exchange(false, true, Relaxed, Relaxed)
        .is_err()
    {
        return Err(BindError::AlreadyBound);
    }
    INIT.store(template.init.as_ptr().cast_mut(), Relaxed);
    INIT_LEN.store(template.init.len(), Relaxed);
    INIT_AT.store(template.init_at, Relaxed);
    BLOCK_LEN.store(template.block_len, Relaxed);
    let mut key = 0usize;
    let rc = unsafe { pthread_key_create(&mut key, Some(free_block)) };
    if rc != 0 {
        // no key, so no block was allocated from this template
        BOUND.store(false, Relaxed);
        return Err(BindError::KeyCreate(rc));
    }
    Ok(TlvBinding {
        thunk: tlv_get_addr as *const () as usize,
        key,
    })
}

#[cfg(test)]
mod tests;
