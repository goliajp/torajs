//! The printed names of the one-operand instructions and the pin words.

use crate::ssa::{InstKind, PinWord};

/// The printed opcode of the one-operand refcount / box instructions.
pub(super) fn one_operand_mnemonic(kind: &InstKind) -> String {
    match kind {
        InstKind::RcInc(_) => "rc_inc".into(),
        InstKind::RcIncAny(_) => "rc_inc_any".into(),
        InstKind::RcDecAny(_) => "rc_dec_any".into(),
        InstKind::BoxAny(tag, _) => format!("box_any {tag}"),
        InstKind::AnyIsNumber(_) => "any_is_number".into(),
        InstKind::AnyIsInt32(_) => "any_is_int32".into(),
        InstKind::AnyToInt32(_) => "any_to_int32".into(),
        InstKind::BoxInt32(_) => "box_int32".into(),
        InstKind::BoxF64(_) => "box_f64".into(),
        InstKind::SiToFp(_) => "sitofp".into(),
        InstKind::FpToSi(_) => "fptosi".into(),
        InstKind::FpToInt32(_) => "fptoint32".into(),
        InstKind::ZExtBoolToI64(_) => "zext_bool".into(),
        InstKind::ZExtI32ToI64(_) => "zext_i32".into(),
        InstKind::BitCastF64ToI64(_) => "bitcast_f64_to_i64".into(),
        InstKind::BitCastI64ToF64(_) => "bitcast_i64_to_f64".into(),
        InstKind::IntToPtr(_) => "inttoptr".into(),
        InstKind::PtrToInt(_) => "ptrtoint".into(),
        InstKind::TruncI64ToBool(_) => "trunc_i64_to_bool".into(),
        InstKind::Identity(_) => "id".into(),
        InstKind::Neg(_) => "neg".into(),
        InstKind::Ctpop(_) => "ctpop".into(),
        InstKind::Cttz(_) => "cttz".into(),
        other => unreachable!("not a one-operand instruction: {other:?}"),
    }
}

pub(super) fn pin_word(word: PinWord) -> String {
    match word {
        PinWord::Len => "len".to_string(),
        PinWord::Data => "data".to_string(),
        PinWord::Scalar(k) => format!("s{k}"),
    }
}
