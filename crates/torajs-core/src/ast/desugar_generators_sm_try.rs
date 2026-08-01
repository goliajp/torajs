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
use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::{Ast, BinOp, Expr, Stmt};

/// One try/catch exception region. A throw escaping user code while
/// `__gen_st ∈ [start, end]` stores the exception into `this.<slot>`
/// and re-enters the dispatch at `catch_entry`.
pub(super) struct TryRegion {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) catch_entry: usize,
    pub(super) slot: String,
}

impl GenSm<'_> {
    /// `try { B } catch (e) { C }` with a yield anywhere inside:
    /// region-lower B into its own state range, C into states past the
    /// range, and record the region for the assembly's dispatch wrap.
    ///
    /// Fallback (no yield at all / `finally` present / no catch):
    /// verbatim inline push — exactly the pre-RFC catch-all shape.
    /// The finally-with-yield face is a registered follow-up.
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
        if !has_yield || finally_body.is_some() || !had_catch {
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

/// C2 — prologue pair for region-bearing generators:
/// `let __gen_inj: boolean = this.__inject; this.__inject = false;`
pub(super) fn emit_inject_prologue(ast: &mut Ast) -> Vec<Stmt> {
    let this_read = ast.add_expr(Expr::This);
    let inj_read = ast.add_expr(Expr::Member {
        obj: this_read,
        name: "__inject".into(),
    });
    let seed = Stmt::LetDecl {
        mutable: true,
        name: INJECT_LOCAL.into(),
        type_ann: Some("boolean".into()),
        init: inj_read,
        is_var: false,
    };
    let this_clear = ast.add_expr(Expr::This);
    let inj_member = ast.add_expr(Expr::Member {
        obj: this_clear,
        name: "__inject".into(),
    });
    let false_lit = ast.add_expr(Expr::Bool(false));
    let clear = ast.add_expr(Expr::Assign {
        target: inj_member,
        value: false_lit,
    });
    vec![seed, Stmt::Expr(clear)]
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
) -> Vec<Stmt> {
    loop_body.insert(0, emit_inject_check(ast));
    let mut regs: Vec<&TryRegion> = regions.iter().collect();
    regs.sort_by(|a, b| b.start.cmp(&a.start));

    // Build the routing chain tail-first: else { throw __gen_exc; }.
    let exc = ast.add_expr(Expr::Ident(DISPATCH_EXC.into()));
    let mut chain = Stmt::Throw(exc);
    for r in regs.iter().rev() {
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
        let cond = ast.add_expr(Expr::BinOp {
            op: BinOp::LAnd,
            left: ge,
            right: le,
        });
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
