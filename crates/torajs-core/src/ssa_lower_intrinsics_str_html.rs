//! Pass 0 `declare_intrinsic` group: Annex B B.2.2 String.prototype
//! HTML methods (`torajs-str/html.rs` kernels).
//!
//! Two shapes only: the four attributed forms (anchor / fontcolor /
//! fontsize / link) take `(Str, Str) -> Str`; the nine plain wraps
//! (big / blink / bold / fixed / italics / small / strike / sub /
//! sup) take `(Str) -> Str`.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct StrHtmlIds {
    pub str_anchor: FuncId,
    pub str_fontcolor: FuncId,
    pub str_fontsize: FuncId,
    pub str_link: FuncId,
    pub str_big: FuncId,
    pub str_blink: FuncId,
    pub str_bold: FuncId,
    pub str_fixed: FuncId,
    pub str_italics: FuncId,
    pub str_small: FuncId,
    pub str_strike: FuncId,
    pub str_sub: FuncId,
    pub str_sup: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> StrHtmlIds {
    let mut attr =
        |name: &str| declare_intrinsic(module, fn_table, name, &[Type::Str, Type::Str], Type::Str);
    let str_anchor = attr("__torajs_str_anchor");
    let str_fontcolor = attr("__torajs_str_fontcolor");
    let str_fontsize = attr("__torajs_str_fontsize");
    let str_link = attr("__torajs_str_link");
    let mut plain = |name: &str| declare_intrinsic(module, fn_table, name, &[Type::Str], Type::Str);
    StrHtmlIds {
        str_anchor,
        str_fontcolor,
        str_fontsize,
        str_link,
        str_big: plain("__torajs_str_big"),
        str_blink: plain("__torajs_str_blink"),
        str_bold: plain("__torajs_str_bold"),
        str_fixed: plain("__torajs_str_fixed"),
        str_italics: plain("__torajs_str_italics"),
        str_small: plain("__torajs_str_small"),
        str_strike: plain("__torajs_str_strike"),
        str_sub: plain("__torajs_str_sub"),
        str_sup: plain("__torajs_str_sup"),
    }
}
