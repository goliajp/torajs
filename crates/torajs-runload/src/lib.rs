//! In-process loader for the prelinked runtime image. `tr run` places
//! the image in its own address space, binds the image's imports and
//! thread-local variables, and calls the program's entry directly
//! instead of writing and starting a new executable.

#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
pub mod tlv;
