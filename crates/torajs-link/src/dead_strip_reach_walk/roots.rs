//! The roots of one reach closure: the nodes the user fns' reloc
//! targets make live, and the section-flag and no-dead-strip roots.

use torajs_codegen::CompiledFunction;
use torajs_codegen::id_hash::HashMap;
use torajs_obj::{
    S_ATTR_LIVE_SUPPORT, S_ATTR_NO_DEAD_STRIP, S_MOD_INIT_FUNC_POINTERS, S_TERM_FUNC_POINTERS,
    S_THREAD_LOCAL_VARIABLES, SECTION_TYPE,
};

use super::Walker;
use crate::archives_merge::reloc_target_name;
use crate::dead_strip_reach::{Node, Pred};
use crate::dead_strip_reach_build::trim16;
use crate::dead_strip_reach_resolve::node_for_addr;

impl Walker<'_, '_> {
    /// The node each user-fn reloc target makes live (user-defined,
    /// link-defined and dyld-resolved names make none), in reloc order.
    pub(crate) fn user_roots(
        &mut self,
        user_funcs: &[CompiledFunction],
        sym_loc: &mut Vec<u64>,
    ) -> Vec<Node> {
        let mut roots = Vec::new();
        let mut seeds: HashMap<&str, Option<Node>> = HashMap::default();
        for f in user_funcs {
            for r in &f.relocs {
                let Some(name) = reloc_target_name(&r.kind) else {
                    continue;
                };
                let node = match seeds.get(name) {
                    Some(&node) => node,
                    None => {
                        let node = self.root(name, sym_loc);
                        seeds.insert(name, node);
                        node
                    }
                };
                if let Some(node) = node {
                    if let Some(p) = self.preds.as_mut() {
                        p.entry(node)
                            .or_insert_with(|| Pred::UserFn(format!("{name} (in {})", f.name)));
                    }
                    roots.push(node);
                }
            }
        }
        roots
    }

    fn root(&self, name: &str, sym_loc: &mut Vec<u64>) -> Option<Node> {
        if self.defined_in_user.contains(name) || self.required.dyld_imports.contains_key(name) {
            return None;
        }
        let row = self.merged.image.find_symbol(name)?;
        self.sym_node(row as u32, sym_loc)
    }

    /// ld64 dead-strip GC roots beyond the extern seeds: sections
    /// flagged no-dead-strip / live-support, mod-init & term-func
    /// pointer sections, TLV descriptor tables (the pipeline emits
    /// their thunk slots unconditionally, and their entries point at
    /// `__thread_data` initial values), and `#[used]`-style symbols
    /// carrying N_NO_DEAD_STRIP in `n_desc`.
    pub(crate) fn flag_roots(&mut self, worklist: &mut Vec<Node>) {
        for (&key, r) in self.walk.keys.iter().zip(&self.walk.members) {
            for (i, sa) in r.sects.iter().enumerate() {
                let ty = sa.flags & SECTION_TYPE;
                // `__LD,__compact_unwind` is link-only input: ld64 folds
                // it into `__TEXT,__unwind_info` and it never reaches a
                // final binary; this pipeline builds no unwind_info at
                // all (panic=abort substrate), so its LIVE_SUPPORT flag
                // must not root the per-fn unwind rows — treating it as
                // a hard root kept every unwind-covered fn (whole
                // members: date, arr, str…) alive with no consumer.
                // Equivalent to ld64's `-no_compact_unwind`.
                // `__TEXT,__eh_frame` is the same kind of input: its FDEs
                // describe functions and are LIVE_SUPPORT (live only if
                // what they describe is), and this pipeline registers no
                // unwind tables either. As a root, the iter archives' std
                // FDEs kept almost all of std — sockets, process spawn,
                // threads — and 179 libSystem imports that dyld binds on
                // every launch of an iter-built program.
                if matches!(trim16(&sa.sectname), b"__compact_unwind" | b"__eh_frame") {
                    continue;
                }
                if sa.flags & (S_ATTR_NO_DEAD_STRIP | S_ATTR_LIVE_SUPPORT) != 0
                    || ty == S_MOD_INIT_FUNC_POINTERS
                    || ty == S_TERM_FUNC_POINTERS
                    || ty == S_THREAD_LOCAL_VARIABLES
                {
                    let node = Node::AllSect {
                        key,
                        sect: (i + 1) as u8,
                    };
                    if let Some(p) = self.preds.as_mut() {
                        p.entry(node).or_insert(Pred::FlagRoot);
                    }
                    worklist.push(node);
                }
            }
            for e in r.no_dead_strip_rows() {
                let node = node_for_addr(key, r, e.n_sect, e.n_value);
                if let Some(p) = self.preds.as_mut() {
                    p.entry(node).or_insert(Pred::FlagRoot);
                }
                worklist.push(node);
            }
        }
    }
}
