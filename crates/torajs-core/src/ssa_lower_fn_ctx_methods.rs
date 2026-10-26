//! `LowerCtx` methods that only [`crate::ssa_lower_fn::lower_fn`]
//! calls — the param-materialization and closure-env preamble steps
//! chunk 450 lifted out of its body, plus the capture-box helper the
//! let-decl path shares with them. Moved out of `ssa_lower_fn.rs`
//! verbatim to keep that file under the size limit.

use crate::ast;
use crate::ssa::{InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{CLOSURE_CAP_BASE_OFF, LocalInfo, LowerCtx, decode_env_ann};

/// Where a capture-binding preamble reads its env pointer from.
#[derive(Clone, Copy)]
pub(crate) enum EnvSource {
    /// The `__env` param's local slot — loaded per capture, as the
    /// closure preamble always did.
    Slot(ValueId),
    /// An env pointer already computed.
    Ptr(ValueId),
}

/// The lifter's synthetic param names — never user ARGUMENTS, so the
/// S2 missing-argument normalization below skips them and they don't
/// advance the user-argument index the argc compare counts against.
const SYNTHETIC_PARAMS: [&str; 6] = [
    "__env",
    "__torajs_argc",
    "__this",
    "__torajs_argv",
    // A constructor's second head slot (rotation 616): not an
    // argument the program passed, so it neither counts toward the
    // §10.2.1.4 position nor binds undefined on a short call — left
    // in, every declared ctor param sat one position too far and
    // the last one read undefined (`constructor(a, b, c)` on the
    // ctor argv lane, rotation 625), and `super()` with no argument
    // cleared the new.target itself.
    "__new_target",
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
    /// A typed optional param (`p?: P`, pointer-shaped) takes its
    /// slot's undefined sentinel the same way; a slot type with no
    /// sentinel keeps the raw param.
    fn normalize_missing_param(
        &mut self,
        pid: ValueId,
        argc: ValueId,
        idx: i64,
        ty: Type,
    ) -> ValueId {
        let sentinel = matches!(ty, Type::Str | Type::Substr | Type::FnSig(_))
            || ty.spells_undef_with_generic_cell();
        if ty != Type::Any && !sentinel {
            return pid;
        }
        let tmp = self.alloca(ty, None);
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
        self.cur_block = fill_blk;
        let undef = if ty == Type::Any {
            Operand::Value(self.f.append_inst(
                fill_blk,
                InstKind::BoxAny(5, Operand::ConstI64(0)),
                Type::Any,
                None,
            ))
        } else {
            self.str_undef_sentinel_for(ty)
                .expect("a slot type with an undefined sentinel")
        };
        self.f
            .append_void(fill_blk, InstKind::Store(undef, Operand::Value(tmp), 0));
        self.f.set_term(fill_blk, Terminator::Br(join_blk));
        self.cur_block = join_blk;
        self.f.append_inst(
            join_blk,
            InstKind::Load(ty, Operand::Value(tmp), 0),
            ty,
            None,
        )
    }

    pub(crate) fn materialize_fn_params(
        &mut self,
        fn_name: &str,
        param_setup: Vec<(String, ValueId, Type)>,
        assigned_in_body: &std::collections::HashSet<String>,
        params: &[crate::ast::Param],
    ) {
        let optional: std::collections::HashSet<&str> = params
            .iter()
            .filter(|p| {
                p.type_ann
                    .as_deref()
                    .is_some_and(|a| a.starts_with("__nullable("))
            })
            .map(|p| p.name.as_str())
            .collect();
        // S2 normalization applies only to fns carrying the hidden
        // argc (`__env`-first; since S1-T1 also the this-first
        // method_argv family), to Any-repr user params and to optional
        // ones — the checker's short-call admit is gated to tails of
        // those two kinds.
        let argc_pid = param_setup
            .iter()
            .find(|(n, _, _)| n == "__torajs_argc")
            .map(|(_, p, _)| *p);
        let mut user_idx: i64 = 0;
        for (pname, pid, ty) in param_setup {
            let synthetic = SYNTHETIC_PARAMS.contains(&pname.as_str());
            let pid = match (synthetic, ty, argc_pid) {
                (false, Type::Any, Some(argc)) => {
                    self.normalize_missing_param(pid, argc, user_idx, ty)
                }
                (false, _, Some(argc)) if optional.contains(pname.as_str()) => {
                    self.normalize_missing_param(pid, argc, user_idx, ty)
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
            let escape_captured = (ty.is_copy()
                && self.escape_captured_lets.contains(&pname)
                && self.mutated_captured_lets.contains(&pname))
                || boxed_noncopy;
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
            if borrows_caller && !escape_captured {
                crate::ssa_lower_tail_call::note_outer_slot(self, slot);
            }
            self.locals.insert(
                pname.clone(),
                LocalInfo {
                    slot,
                    ty,
                    moved: borrows_caller,
                    // A boxed param owns its box stake — the fn-exit
                    // boxed walk releases it (`!borrowed` gate).
                    borrowed: borrows_caller && !boxed_noncopy,
                    boxed: escape_captured,
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
        // 551-03 — an any-widened clone of a lifted body has no
        // construction site of its own; the call enters it with the
        // ORIGINAL cell's env, so the original's layout is its layout.
        let layout_of = self
            .ast
            .widen_clone_origin
            .get(fn_name)
            .map_or(fn_name, String::as_str);
        let cap_meta: Vec<(String, Type, bool)> = if cap_names.is_empty() {
            Vec::new()
        } else {
            self.closure_captures
                .get(layout_of)
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "ssa-lower: lifted closure `{fn_name}` has no capture types — \
                         construction site must run before body lowering"
                    )
                })
        };
        // 556-02 — captures that were variadic bindings at the
        // construction site route their calls boxed here too.
        let variadic_caps: Vec<String> = self
            .closure_capture_facts
            .get(layout_of)
            .map(|f| f.variadic.clone())
            .unwrap_or_default();
        let env_slot = self
            .locals
            .get("__env")
            .copied()
            .expect("__env param materialized as local")
            .slot;
        self.bind_env_captures(EnvSource::Slot(env_slot), &cap_meta, &variadic_caps, None);
        // Self-name binding — the trailing slot after the captures
        // holds the cell (mint-site self-store; borrowed edge, no
        // stake to release at exit). A same-named param shadows the
        // binding (§15.5.5), so an existing local wins.
        if let Some(sn) = self_name
            && !self.locals.contains_key(&sn)
        {
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
                    boxed: false,
                    scope_depth: 0,
                },
            );
            // 403-02 — the call lane's self-recursion proof reads
            // this slot identity (see the field doc).
            self.self_name_slot = Some(local);
            crate::ssa_lower_tail_call::note_outer_slot(self, local);
            self.scope_stack[0].push(sn);
        }
    }

    /// Bind each capture of a layout under its name, as a closure body
    /// does: a byref slot holds a capture-box pointer and IS the
    /// binding's slot (reads and writes hit the shared live binding);
    /// a by-value slot copies into a fresh local. `env` is where the
    /// env pointer comes from — the `__env` param slot for a closure
    /// body, a value already in hand for a generator's iterator method
    /// (RFC 20260912, `emit_gen_env_preamble`). `only` restricts the
    /// binding to some of the layout's names; offsets always follow
    /// the full layout.
    pub(crate) fn bind_env_captures(
        &mut self,
        env: EnvSource,
        cap_meta: &[(String, Type, bool)],
        variadic_caps: &[String],
        only: Option<&[String]>,
    ) {
        for (i, (cap_name, cap_ty, is_byref)) in cap_meta.iter().enumerate() {
            let cap_ty = *cap_ty;
            let is_byref = *is_byref;
            if only.is_some_and(|names| !names.iter().any(|n| n == cap_name)) {
                continue;
            }
            let env_ptr = match env {
                EnvSource::Slot(slot) => self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
                    Type::Ptr,
                    None,
                ),
                EnvSource::Ptr(ptr) => ptr,
            };
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
            crate::ssa_lower_tail_call::note_outer_slot(self, cap_slot);
            self.locals.insert(
                cap_name.clone(),
                LocalInfo {
                    slot: cap_slot,
                    ty: cap_ty,
                    moved: true,
                    borrowed: true,
                    boxed: is_byref,
                    scope_depth: 0,
                },
            );
            if variadic_caps.contains(cap_name) {
                self.variadic_locals.insert(cap_name.clone());
            }
            self.scope_stack[0].push(cap_name.clone());
        }
    }
}
