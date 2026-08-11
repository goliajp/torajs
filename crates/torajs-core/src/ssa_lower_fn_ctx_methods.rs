//! `LowerCtx` methods that only [`crate::ssa_lower_fn::lower_fn`]
//! calls — the param-materialization and closure-env preamble steps
//! chunk 450 lifted out of its body, plus the capture-box helper the
//! let-decl path shares with them. Moved out of `ssa_lower_fn.rs`
//! verbatim to keep that file under the size limit.

use crate::ast;
use crate::ssa::{InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{CLOSURE_CAP_BASE_OFF, LocalInfo, LowerCtx, decode_env_ann};

/// The lifter's synthetic param names — never user ARGUMENTS, so the
/// S2 missing-argument normalization below skips them and they don't
/// advance the user-argument index the argc compare counts against.
const SYNTHETIC_PARAMS: [&str; 6] = [
    "__env",
    "__torajs_argc",
    "__this",
    "__torajs_real_argc",
    "__torajs_argv",
    "__yield_arg",
];

impl<'a> LowerCtx<'a> {
    /// T-15.g.5 escape-captured Copy binding: box the value on the heap
    /// (16B = rc + value) via `capture_box_alloc`, bit-casting F64 / zero-
    /// extending Bool into the i64 payload slot. Shared by fn-param
    /// materialization here and the let-decl general path.
    pub(crate) fn emit_capture_boxed(&mut self, ty: Type, v: Operand) -> ValueId {
        let init_i64 = if matches!(ty, Type::F64) {
            let b = self.f.append_inst(
                self.cur_block,
                InstKind::BitCastF64ToI64(v),
                Type::I64,
                None,
            );
            Operand::Value(b)
        } else if matches!(ty, Type::Bool) {
            let b = self
                .f
                .append_inst(self.cur_block, InstKind::ZExtBoolToI64(v), Type::I64, None);
            Operand::Value(b)
        } else {
            v
        };
        self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.capture_box_alloc, vec![init_i64]),
            Type::Ptr,
            None,
        )
    }

    /// step 4 of `lower_fn`: materialize each param as an alloca-backed
    /// local (refcounted capture box for escape-captured Copy params;
    /// `__env` / `__cm_*` `__this` marked moved+borrowed — caller owns).
    ///
    /// `assigned_in_body` — names the body (or a closure it builds)
    /// reassigns. A non-Copy param in that set copies IN an owned
    /// stake and books as an owned local: the reassignment site
    /// clears `moved` at COMPILE time (fn-exit drop then fires) while
    /// the assignment itself may sit on a runtime branch — the
    /// default-param guard `if (x === undefined) x = d` — so on the
    /// explicit-argument path the borrow convention would release the
    /// caller's stake (t262 gen dstr-default regression: the freed
    /// default generator's address was recycled by the next gen cell,
    /// whose ctor-side IteratorClose then closed ITSELF).
    /// RFC 20260810-indirect-argc-abi S2 — §10.2.1.4 argument
    /// binding: a user parameter position at or past the call's real
    /// argc was not passed, so it binds `undefined` regardless of
    /// what a short argv left in its register (the indirect-call
    /// crash lane: a declared-more closure called through a
    /// narrower Function face). Alloca-shaped select (`Select` is
    /// elaborator-only territory): store the raw param, overwrite
    /// with the undefined box on the missing path, load back. The
    /// existing `=== undefined` body guards then fire the param's
    /// default exactly like an explicit-undefined argument.
    fn normalize_missing_any_param(&mut self, pid: ValueId, argc: ValueId, idx: i64) -> ValueId {
        let tmp = self.alloca(Type::Any, None);
        let cb = self.cur_block;
        self.f.append_void(
            cb,
            InstKind::Store(Operand::Value(pid), Operand::Value(tmp), 0),
        );
        let missing = self.f.append_inst(
            cb,
            InstKind::ICmp(
                crate::ssa::IPred::Sle,
                Operand::Value(argc),
                Operand::ConstI64(idx),
            ),
            Type::Bool,
            None,
        );
        let fill_blk = self.f.add_block();
        let join_blk = self.f.add_block();
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(missing),
                then_blk: fill_blk,
                else_blk: join_blk,
            },
        );
        let undef = self.f.append_inst(
            fill_blk,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        self.f.append_void(
            fill_blk,
            InstKind::Store(Operand::Value(undef), Operand::Value(tmp), 0),
        );
        self.f.set_term(fill_blk, Terminator::Br(join_blk));
        self.cur_block = join_blk;
        self.f.append_inst(
            join_blk,
            InstKind::Load(Type::Any, Operand::Value(tmp), 0),
            Type::Any,
            None,
        )
    }

    pub(crate) fn materialize_fn_params(
        &mut self,
        fn_name: &str,
        param_setup: Vec<(String, ValueId, Type)>,
        assigned_in_body: &std::collections::HashSet<String>,
    ) {
        // S2 normalization applies only to fns carrying the hidden
        // argc (`__env`-first; since S1-T1 also the this-first
        // method_argv family) and only to Any-repr user params — a
        // typed slot cannot hold the undefined box, and the
        // checker's short-call admit is gated to all-Any tails.
        let argc_pid = param_setup
            .iter()
            .find(|(n, _, _)| n == "__torajs_argc")
            .map(|(_, p, _)| *p);
        let mut user_idx: i64 = 0;
        for (pname, pid, ty) in param_setup {
            let synthetic = SYNTHETIC_PARAMS.contains(&pname.as_str());
            let pid = match (synthetic, ty, argc_pid) {
                (false, Type::Any, Some(argc)) => {
                    self.normalize_missing_any_param(pid, argc, user_idx)
                }
                _ => pid,
            };
            if !synthetic {
                user_idx += 1;
            }
            self.materialize_one_param(fn_name, pname, pid, ty, assigned_in_body);
        }
    }

    /// One param's alloca/box/LocalInfo bookkeeping — the loop body
    /// `materialize_fn_params` carried before the S2 normalization
    /// grew the fn past the size limit.
    fn materialize_one_param(
        &mut self,
        fn_name: &str,
        pname: String,
        pid: ValueId,
        ty: Type,
        assigned_in_body: &std::collections::HashSet<String>,
    ) {
        {
            // RFC 20260710 C4 — a mutated non-Copy captured PARAM
            // promotes to a capture box like a let (the caller keeps
            // its stake, so the box incs its own share); the byref /
            // assign / drop lanes then ride the C1 machinery.
            let boxed_noncopy = !ty.is_copy()
                && ty != Type::Substr
                && pname != "__env"
                && !(fn_name.starts_with("__cm_") && pname == "__this")
                && self.escape_captured_lets.contains(&pname)
                && self.mutated_captured_lets.contains(&pname);
            let escape_captured =
                (ty.is_copy() && self.escape_captured_lets.contains(&pname)) || boxed_noncopy;
            let slot = if escape_captured {
                if boxed_noncopy {
                    self.emit_owned_result_inc(Operand::Value(pid), ty);
                }
                self.emit_capture_boxed(ty, Operand::Value(pid))
            } else {
                let s = self.alloca(ty, Some(&pname));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(pid), Operand::Value(s), 0),
                );
                s
            };
            if boxed_noncopy {
                self.boxed_noncopy_lets.insert(pname.clone());
            }
            let is_env_param = pname == "__env";
            let is_class_self = fn_name.starts_with("__cm_") && pname == "__this";
            // Reassigned non-Copy param → owned local (see fn doc).
            // Substr stays borrowed (view semantics, no stake to
            // take); the boxed path carries its own stake above.
            let reassigned_owned = !boxed_noncopy
                && !escape_captured
                && !ty.is_copy()
                && ty != Type::Substr
                && !is_env_param
                && !is_class_self
                && assigned_in_body.contains(&pname);
            if reassigned_owned {
                self.emit_owned_result_inc(Operand::Value(pid), ty);
            }
            let borrows_caller = !reassigned_owned
                && (is_env_param || is_class_self || !ty.is_copy() || escape_captured);
            self.locals.insert(
                pname.clone(),
                LocalInfo {
                    slot,
                    ty,
                    moved: borrows_caller,
                    // A boxed param owns its box stake — the fn-exit
                    // boxed walk releases it (`!borrowed` gate).
                    borrowed: borrows_caller && !boxed_noncopy,
                    scope_depth: 0,
                },
            );
            self.scope_stack[0].push(pname);
        }
    }

    /// step 5 of `lower_fn` — M2 closure-body env preamble: for a
    /// first-param `__env`, decode the `__env(c1|c2|...)` annotation and
    /// env-load each capture at offset 8/16/... per the construction-site
    /// `closure_captures` side channel, binding under the capture's name
    /// as moved+borrowed (the env owns the canonical pointer)
    pub(crate) fn emit_closure_env_preamble(&mut self, fn_name: &str, params: &[ast::Param]) {
        let Some(first) = params.first() else { return };
        if first.name != "__env" {
            return;
        }
        let Some(ann) = &first.type_ann else { return };
        let Some(cap_names) = decode_env_ann(ann) else {
            return;
        };
        // §15.5.5 (RFC 20260810) — a named fn-expression carries one
        // extra trailing env slot with the cell itself; bind it even
        // when the capture list is empty.
        let self_name = self.ast.closure_self_names.get(fn_name).cloned();
        if cap_names.is_empty() && self_name.is_none() {
            return;
        }
        // The `__env(c1|c2|...)` ann is the PRE-filter capture list —
        // the construction site drops names that resolved to promoted
        // data globals (those read via GlobalRef like named-fn bodies,
        // no env slot), so the side-channel triples are the env-layout
        // ground truth. A body ident not bound here falls through to
        // the globals path in ident resolution.
        // A zero-capture body needs no side-channel lookup: the ann
        // is the PRE-filter superset of the effective captures, so an
        // empty ann means an empty layout — and the self-slot offset
        // below is CAP_BASE exactly. Looking it up anyway made a
        // self-named zero-capture fn-expr panic when its body lowered
        // before its construction site (for-head destructuring
        // defaults order it that way).
        let cap_meta: Vec<(String, Type, bool)> = if cap_names.is_empty() {
            Vec::new()
        } else {
            self.closure_captures
                .get(fn_name)
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "ssa-lower: lifted closure `{fn_name}` has no capture types — \
                         construction site must run before body lowering"
                    )
                })
        };
        let env_slot = self
            .locals
            .get("__env")
            .copied()
            .expect("__env param materialized as local")
            .slot;
        for (i, (cap_name, cap_ty, is_byref)) in cap_meta.iter().enumerate() {
            let cap_ty = *cap_ty;
            let is_byref = *is_byref;
            let env_ptr = self.f.append_inst(
                self.cur_block,
                InstKind::Load(Type::Ptr, Operand::Value(env_slot), 0),
                Type::Ptr,
                None,
            );
            let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
            // A byref slot holds a capture-box pointer — bind the box
            // value slot directly so body reads AND writes hit the
            // shared live binding (Copy escape-captures + RFC 20260710
            // promoted mutable non-Copy captures).
            let cap_slot = if is_byref {
                if !cap_ty.is_copy() {
                    // Nested closures capturing this name must ride
                    // the same box (byref write_captures arm).
                    self.boxed_noncopy_lets.insert(cap_name.clone());
                }
                self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(env_ptr), offset),
                    Type::Ptr,
                    None,
                )
            } else {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(cap_ty, Operand::Value(env_ptr), offset),
                    cap_ty,
                    None,
                );
                let local = self.alloca(cap_ty, Some(cap_name));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(v), Operand::Value(local), 0),
                );
                local
            };
            self.locals.insert(
                cap_name.clone(),
                LocalInfo {
                    slot: cap_slot,
                    ty: cap_ty,
                    moved: true,
                    borrowed: true,
                    scope_depth: 0,
                },
            );
            self.scope_stack[0].push(cap_name.clone());
        }
        // Self-name binding — the trailing slot after the captures
        // holds the cell (mint-site self-store; borrowed edge, no
        // stake to release at exit). A same-named param shadows the
        // binding (§15.5.5), so an existing local wins.
        if let Some(sn) = self_name {
            if !self.locals.contains_key(&sn) {
                let closure_ty = crate::ssa_lower_closure::closure_value_ty(self, fn_name);
                let env_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(env_slot), 0),
                    Type::Ptr,
                    None,
                );
                let offset = CLOSURE_CAP_BASE_OFF + (cap_meta.len() as u64) * 8;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(closure_ty, Operand::Value(env_ptr), offset),
                    closure_ty,
                    None,
                );
                let local = self.alloca(closure_ty, Some(&sn));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(v), Operand::Value(local), 0),
                );
                self.locals.insert(
                    sn.clone(),
                    LocalInfo {
                        slot: local,
                        ty: closure_ty,
                        moved: true,
                        borrowed: true,
                        scope_depth: 0,
                    },
                );
                self.scope_stack[0].push(sn);
            }
        }
    }
}
