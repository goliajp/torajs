//! Apple `.a` archive + Mach-O member symbol-table parsing. The
//! parsers live in `torajs_rtimage::archive` (the runtime image is
//! built from them when `tr` is compiled); re-exported here so the
//! link pipeline keeps one import path for `ArMember` and the member
//! symtab errors.

pub use torajs_rtimage::archive::{
    AR_HEADER_SIZE, AR_MAGIC, ArMember, ArParseError, ArchiveIndex, ArchiveParseError,
    ArchiveSymbol, MemberSymtabError, build_archive_index, parse_archive,
    parse_member_defined_externs,
};
pub use torajs_rtimage::member_name::MemberName;

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid `.a` containing a single member with
    /// a short name (no BSD long-name encoding).
    fn build_short_name_archive(name: &str, data: &[u8]) -> Vec<u8> {
        assert!(name.len() <= 16);
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(AR_MAGIC);
        let mut header = [b' '; AR_HEADER_SIZE];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[16] = b'0';
        let size_str = format!("{}", data.len());
        header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
        header[58] = b'`';
        header[59] = b'\n';
        archive.extend_from_slice(&header);
        archive.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            archive.push(0);
        }
        archive
    }

    /// Synthetic single-member archive carrying a hand-built
    /// Mach-O with one `_foo` symbol round-trips through
    /// `build_archive_index`.
    #[test]
    fn synthetic_single_sym_archive_round_trips() {
        // Re-use torajs-obj's writer to produce a real .o, wrap
        // it in a single-member archive, and confirm
        // `build_archive_index` extracts the sym.
        use torajs_codegen::compile_function;
        use torajs_core::ssa::{
            BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
            ValueInfo,
        };
        use torajs_obj::write_object;

        let v0 = ValueId(0);
        let f = Function {
            name: "_foo".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(7), Operand::ConstI64(0)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        };
        let cf = compile_function(&f);
        let obj_bytes = write_object(std::slice::from_ref(&cf));

        let archive = build_short_name_archive("foo.o", &obj_bytes);
        let members = parse_archive(&archive).unwrap();
        assert_eq!(members.len(), 1);
        let index = build_archive_index(&members).unwrap();
        assert!(
            index.contains_key("_foo"),
            "synthetic archive must surface _foo; got {:?}",
            index.keys().collect::<Vec<_>>()
        );
        let sym = &index["_foo"];
        assert_eq!(sym.member_idx, 0);
        // _foo lives at section-relative offset 0 inside the .o.
        assert_eq!(sym.n_value, 0);
    }
}
