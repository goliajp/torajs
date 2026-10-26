//! The static layout of a baseline `MH_EXECUTE` image
//! ([`crate::exec::compute_layout`]).

/// Layout decisions the link driver makes before any bytes are emitted.
#[derive(Debug, Clone)]
pub struct ExecLayout {
    /// File offset where the `__text` payload begins — packed
    /// right after the header + load commands, 16-aligned (ld64
    /// layout; the whole `__TEXT` segment maps r-x anyway).
    pub text_file_offset: u32,
    /// `__text` payload size in bytes (sum of all
    /// `ResolvedFunction.bytes.len()`).
    pub text_size: u32,
    /// `__TEXT` segment vmaddr (`0x1_0000_0000`).
    pub text_vmaddr: u64,
    /// `__TEXT` segment vmsize — page-aligned, covers header +
    /// load commands + `__text` payload.
    pub text_vmsize: u64,
    /// File offset where `__LINKEDIT` data begins (start of the
    /// nlist table).
    pub linkedit_file_offset: u32,
    /// `__LINKEDIT` vmaddr — `text_vmaddr + text_vmsize`.
    pub linkedit_vmaddr: u64,
    /// `__LINKEDIT` vmsize — page-aligned, covers symtab + strtab.
    pub linkedit_vmsize: u64,
    /// File offset of the nlist table.
    pub symoff: u32,
    /// File offset of the string table.
    pub stroff: u32,
    /// Total file size = `linkedit_file_offset + linkedit_data_size`.
    pub total_size: u32,
    /// Number of defined-extern symbols (one per `funcs[i]`).
    pub nsyms: u32,
    /// String table byte length.
    pub strsize: u32,
    /// Virtual address of the entry function (`_main`) in the
    /// final image — used to fill `LC_MAIN.entry_off`.
    pub entry_file_offset: u32,
    /// Final virtual address of each function — `fn_vaddrs[i]` =
    /// `text_vmaddr + (text section file offset relative to __TEXT)
    /// + fn_offset_in_text`. Same indexing as
    /// `LinkConfig.funcs`.
    pub fn_vaddrs: Vec<u64>,
    /// File offset of the embedded codesign SuperBlob —
    /// `LC_CODE_SIGNATURE.dataoff` points here.
    pub codesign_dataoff: u32,
    /// Codesign blob byte length (= `LC_CODE_SIGNATURE.datasize`).
    pub codesign_datasize: u32,
}
