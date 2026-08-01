//! Generator try/catch exception regions — RFC 20260802.
//!
//! The `Stmt::Try` arm of [`GenSm::lower`] plus the dispatch-wrap the
//! assembly applies when regions exist. Regenerator-tryEntries shape:
//! because `GenSm::alloc_state` is monotonic, "every state allocated
//! while lowering the try body" is the contiguous range
//! `[try_entry, region_end]`, which numerically equals lexical
//! containment — an inner try's range nests inside the outer's, and
//! the inner CATCH's states (allocated after the inner region closes,
//! but still during the outer body's lowering) land inside the outer
//! range only. Innermost-first dispatch (descending `start`) therefore
//! routes a rethrow from an inner catch to the outer catch, matching
//! ES try semantics with zero extra bookkeeping.

use super::desugar_generators_sm::{GenSm, RESUME_LOCAL};
use super::desugar_generators_sm_finally::stmts_block_finally_region;
use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// One try/catch exception region. A throw escaping user code while
/// `__gen_st ∈ [start, end]` stores the exception into `this.<slot>`
/// and re-enters the dispatch at `catch_entry`.
pub(super) struct TryRegion {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) catch_entry: usize,
    pub(super) slot: String,
    /// D3b — `Some((ret_entry, ret_slot))` for a finally region whose
    /// return copy was minted: a `return()` injected while suspended
    /// in `[start, end]` stores the value into `this.<ret_slot>` and
    /// re-enters the dispatch at `ret_entry` (the D3a copy, which
    /// runs F and completes with the slot — chaining outer frames).
    /// `None` for catch regions (a catch never intercepts a return
    /// completion, §27.5.1.7) and for finally regions with no yield
    /// in B — no suspendable state sits in their range, so injection
    /// can't target them and the copy isn't minted.
    pub(super) ret: Option<(usize, String)>,
}

impl GenSm<'_> {
    /// `try { B } catch (e) { C }` with a yield anywhere inside:
    /// region-lower B into its own state range, C into states past the
    /// range, and record the region for the assembly's dispatch wrap.
    ///
    /// `try { B } finally { F }` (no catch) takes the D1 duplication
    /// path in [`Self::lower_try_finally`] when the gate walker
    /// clears it. Fallback (no yield at all / catch+finally combined
    /// / B carries a return or escaping jump): verbatim inline push —
    /// exactly the pre-RFC catch-all shape.
    pub(super) fn lower_try(
        &mut self,
        body: Vec<Stmt>,
        had_catch: bool,
        catch_param: Option<String>,
        catch_type: Option<String>,
        catch_body: Vec<Stmt>,
        finally_body: Option<Vec<Stmt>>,
    ) {
        let has_yield = body.iter().any(stmt_contains_yield)
            || catch_body.iter().any(stmt_contains_yield)
            || finally_body
                .as_ref()
                .is_some_and(|f| f.iter().any(stmt_contains_yield));
        // D1/D2 finally regions handle `try {B} [catch {C}] finally
        // {F}` when neither B nor C carries a return or a jump that
        // escapes the try — either would have to run F on the way
        // out, which the duplication lowering below doesn't route
        // (registered follow-up). F itself MAY return (the Return
        // arm's done-step correctly encodes "finally overrides the
        // completion") and MAY yield (each copy is state-machined
        // independently).
        let finally_ok = finally_body.is_some()
            && !stmts_block_finally_region(&body, false)
            && !stmts_block_finally_region(&catch_body, false)
            && !finally_body
                .as_ref()
                .is_some_and(|f| stmts_block_finally_region(f, true));
        if !has_yield
            || (finally_body.is_some() && !finally_ok)
            || (finally_body.is_none() && !had_catch)
        {
            let mut s = Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            };
            self.rewrite_nested_returns(&mut s);
            self.cur_buf.push(s);
            return;
        }
        if let Some(f) = finally_body {
            if had_catch {
                // D2 — catch+finally decomposes into the standard
                // nesting `try { try {B} catch {C} } finally {F}`
                // (§14.13.3: catch handles first, finally always
                // runs after). The inner try/catch region nests
                // inside the outer finally region numerically, and
                // the inner CATCH's states land inside the outer
                // range only — so a throw from C routes to F's
                // exception copy, exactly the spec order.
                let inner = Stmt::Try {
                    body,
                    had_catch,
                    catch_param,
                    catch_type,
                    catch_body,
                    finally_body: None,
                };
                self.lower_try_finally(vec![inner], f);
            } else {
                self.lower_try_finally(body, f);
            }
            return;
        }

        // Region path. The pre-try arm holds only the goto, so state
        // 0 (suspendedStart) and any code before the try stay outside
        // the region — a `throw()` injected there escapes, per
        // §27.5.1.4.
        let try_entry = self.alloc_state();
        let mut g = self.emit_goto(try_entry);
        self.cur_buf.append(&mut g);
        self.flush_cur();

        self.cur_state = try_entry;
        self.lower_seq(body);
        // Every state allocated so far while lowering the body — the
        // region. catch_entry / post are allocated AFTER this line so
        // the catch's own states stay outside it.
        let region_end = self.arms.len() - 1;
        let catch_entry = self.alloc_state();
        let post = self.alloc_state();
        let mut exit = self.emit_goto(post);
        self.cur_buf.append(&mut exit);
        self.flush_cur();

        // Catch arm: the param becomes a hoisted `this.<slot>` field
        // (it must survive yield boundaries inside C); references in
        // C rewrite to the slot before lowering.
        self.cur_state = catch_entry;
        let slot = format!("__caught{catch_entry}");
        self.hoisted.push((slot.clone(), "any".into()));
        if let Some(pname) = &catch_param {
            super::desugar_generators_rewrite::rewrite_ident_to_this_slot(
                self.ast,
                &catch_body,
                pname,
                &slot,
            );
        }
        self.lower_seq(catch_body);
        let mut cexit = self.emit_goto(post);
        self.cur_buf.append(&mut cexit);
        self.flush_cur();

        self.cur_state = post;
        self.regions.push(TryRegion {
            start: try_entry,
            end: region_end,
            catch_entry,
            slot,
            ret: None,
        });
    }
}

/// Catch binding of the dispatch wrap. Unannotated ⇒ Any per P7.2b-2.
const DISPATCH_EXC: &str = "__gen_exc";

/// Local carrying the `throw()` injection flag through one `next()`
/// call — seeded from `this.__inject` (which is cleared in the same
/// breath) so later `while (true)` iterations after a region catch
/// don't re-fire the injected throw.
const INJECT_LOCAL: &str = "__gen_inj";

/// D3b — local carrying the `return()` injection flag through one
/// `next()` call, seeded-and-cleared exactly like [`INJECT_LOCAL`].
const RET_LOCAL: &str = "__gen_ret";

/// Seed a local from a boolean `this.<field>` and clear the field in
/// the same breath: `let <local>: boolean = this.<field>;
/// this.<field> = false;` — the C2 prologue shape, shared by the
/// throw() and return() injection flags.
fn emit_flag_seed_pair(ast: &mut Ast, local: &str, field: &str) -> [Stmt; 2] {
    let this_read = ast.add_expr(Expr::This);
    let flag_read = ast.add_expr(Expr::Member {
        obj: this_read,
        name: field.into(),
    });
    let seed = Stmt::LetDecl {
        mutable: true,
        name: local.into(),
        type_ann: Some("boolean".into()),
        init: flag_read,
        is_var: false,
    };
    let this_clear = ast.add_expr(Expr::This);
    let flag_member = ast.add_expr(Expr::Member {
        obj: this_clear,
        name: field.into(),
    });
    let false_lit = ast.add_expr(Expr::Bool(false));
    let clear = ast.add_expr(Expr::Assign {
        target: flag_member,
        value: false_lit,
    });
    [seed, Stmt::Expr(clear)]
}

/// C2 — prologue for region-bearing generators:
/// `let __gen_inj: boolean = this.__inject; this.__inject = false;`
/// plus (D3b, only when a finally region can take a return
/// injection) the same pair for `__gen_ret` / `this.__ret_pending`.
pub(super) fn emit_inject_prologue(ast: &mut Ast, with_ret: bool) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = emit_flag_seed_pair(ast, INJECT_LOCAL, "__inject").into();
    if with_ret {
        out.extend(emit_flag_seed_pair(ast, RET_LOCAL, "__ret_pending"));
    }
    out
}

/// C2 — `if (__gen_inj) { __gen_inj = false; throw this.__thrown; }`,
/// the first statement inside the dispatch try. Firing BEFORE the
/// if-chain means the suspended arm's code never runs (§27.5.1.4:
/// resume with a throw completion at the yield), and the region wrap
/// catches it exactly when the suspended state sits inside a try.
fn emit_inject_check(ast: &mut Ast) -> Stmt {
    let cond = ast.add_expr(Expr::Ident(INJECT_LOCAL.into()));
    let inj_write = ast.add_expr(Expr::Ident(INJECT_LOCAL.into()));
    let false_lit = ast.add_expr(Expr::Bool(false));
    let disarm = ast.add_expr(Expr::Assign {
        target: inj_write,
        value: false_lit,
    });
    let this_id = ast.add_expr(Expr::This);
    let thrown = ast.add_expr(Expr::Member {
        obj: this_id,
        name: "__thrown".into(),
    });
    Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Block(vec![Stmt::Expr(disarm), Stmt::Throw(thrown)])),
        else_branch: None,
    }
}

/// Build the `__gen_st ∈ [start, end]` range test for one region.
fn emit_range_cond(ast: &mut Ast, r: &TryRegion) -> ExprId {
    let st_lo = ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
    let lo = ast.add_expr(Expr::Number(r.start as f64));
    let ge = ast.add_expr(Expr::BinOp {
        op: BinOp::Ge,
        left: st_lo,
        right: lo,
    });
    let st_hi = ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
    let hi = ast.add_expr(Expr::Number(r.end as f64));
    let le = ast.add_expr(Expr::BinOp {
        op: BinOp::Le,
        left: st_hi,
        right: hi,
    });
    ast.add_expr(Expr::BinOp {
        op: BinOp::LAnd,
        left: ge,
        right: le,
    })
}

/// D3b — `if (__gen_ret) { __gen_ret = false; <route> }`, the
/// return-injection check next to the throw check (§27.5.1.7
/// GeneratorResumeAbrupt with a return completion). Routing,
/// innermost-first over the finally regions only: suspended inside
/// one ⇒ stash the value into its D3a return slot and re-enter the
/// dispatch at its return copy (F runs, may yield, completes with
/// the slot — chaining outer frames for free); in none ⇒ the
/// generator just completes with `{ value, done: true }` — the
/// prologue kill already stamped it dead, and suspendedStart /
/// completed / suspended-outside-any-finally all take this branch.
/// Returns `None` when no region can take a return injection.
fn emit_return_check(ast: &mut Ast, regions: &[TryRegion], yield_ty: &str) -> Option<Stmt> {
    let mut regs: Vec<&TryRegion> = regions.iter().filter(|r| r.ret.is_some()).collect();
    if regs.is_empty() {
        return None;
    }
    regs.sort_by(|a, b| b.start.cmp(&a.start));

    // Tail: complete directly. Value routing mirrors the D3a return
    // copy — exactly one As wrap (`as any` boxes the any lane's step
    // write; `as T` unboxes the any slot back to the typed lane).
    let this_id = ast.add_expr(Expr::This);
    let inj_read = ast.add_expr(Expr::Member {
        obj: this_id,
        name: "__ret_inj".into(),
    });
    let val = ast.add_expr(Expr::As {
        expr: inj_read,
        ty_ann: yield_ty.into(),
    });
    let done = ast.add_expr(Expr::Bool(true));
    let step = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), val), ("done".into(), done)],
    });
    let mut chain = Stmt::Return(Some(step));
    for r in regs.iter().rev() {
        let (ret_entry, ret_slot) = r.ret.as_ref().expect("filtered above");
        let cond = emit_range_cond(ast, r);
        let this_slot = ast.add_expr(Expr::This);
        let slot_member = ast.add_expr(Expr::Member {
            obj: this_slot,
            name: ret_slot.clone(),
        });
        let this_inj = ast.add_expr(Expr::This);
        let inj = ast.add_expr(Expr::Member {
            obj: this_inj,
            name: "__ret_inj".into(),
        });
        let store = ast.add_expr(Expr::Assign {
            target: slot_member,
            value: inj,
        });
        let st_write = ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let entry_lit = ast.add_expr(Expr::Number(*ret_entry as f64));
        let goto = ast.add_expr(Expr::Assign {
            target: st_write,
            value: entry_lit,
        });
        chain = Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Block(vec![Stmt::Expr(store), Stmt::Expr(goto)])),
            else_branch: Some(Box::new(chain)),
        };
    }

    let cond = ast.add_expr(Expr::Ident(RET_LOCAL.into()));
    let ret_write = ast.add_expr(Expr::Ident(RET_LOCAL.into()));
    let false_lit = ast.add_expr(Expr::Bool(false));
    let disarm = ast.add_expr(Expr::Assign {
        target: ret_write,
        value: false_lit,
    });
    Some(Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Block(vec![Stmt::Expr(disarm), chain])),
        else_branch: None,
    })
}

/// Wrap the assembled dispatch if-chain in
/// `try { <chain> } catch (__gen_exc) { <region routing> }`.
/// Routing is an if-else chain checked innermost-first (descending
/// `start`); a state in no region rethrows, which propagates out of
/// `next()` — the prologue already stamped the generator dead.
/// After a matched arm stores the exception and moves `__gen_st`,
/// control falls out of the catch and the `while (true)` re-enters
/// the dispatch at the catch-entry state.
pub(super) fn wrap_dispatch_with_region_catch(
    ast: &mut Ast,
    mut loop_body: Vec<Stmt>,
    regions: &[TryRegion],
    yield_ty: &str,
) -> Vec<Stmt> {
    // D3b return check sits after the throw check; after its routing
    // arm moves `__gen_st`, control falls into the if-chain below in
    // the same iteration and the return-copy arm fires.
    if let Some(ret_check) = emit_return_check(ast, regions, yield_ty) {
        loop_body.insert(0, ret_check);
    }
    loop_body.insert(0, emit_inject_check(ast));
    let mut regs: Vec<&TryRegion> = regions.iter().collect();
    regs.sort_by(|a, b| b.start.cmp(&a.start));

    // Build the routing chain tail-first: else { throw __gen_exc; }.
    let exc = ast.add_expr(Expr::Ident(DISPATCH_EXC.into()));
    let mut chain = Stmt::Throw(exc);
    for r in regs.iter().rev() {
        let cond = emit_range_cond(ast, r);
        let this_id = ast.add_expr(Expr::This);
        let slot_member = ast.add_expr(Expr::Member {
            obj: this_id,
            name: r.slot.clone(),
        });
        let exc_read = ast.add_expr(Expr::Ident(DISPATCH_EXC.into()));
        let store = ast.add_expr(Expr::Assign {
            target: slot_member,
            value: exc_read,
        });
        let st_write = ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let entry_lit = ast.add_expr(Expr::Number(r.catch_entry as f64));
        let goto = ast.add_expr(Expr::Assign {
            target: st_write,
            value: entry_lit,
        });
        chain = Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Block(vec![Stmt::Expr(store), Stmt::Expr(goto)])),
            else_branch: Some(Box::new(chain)),
        };
    }

    vec![Stmt::Try {
        body: loop_body,
        had_catch: true,
        catch_param: Some(DISPATCH_EXC.into()),
        catch_type: None,
        catch_body: vec![chain],
        finally_body: None,
    }]
}
