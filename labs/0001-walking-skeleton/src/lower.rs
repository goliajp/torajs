//! AST → IR.
//!
//! - Top-level `FnDecl`s are hoisted: each gets its own `IrFunction`.
//! - Top-level non-fn stmts are lowered into `functions[0]` (`main`, arity 0).
//! - Each function ends with an implicit `LoadUndef; Ret` so the call stack
//!   always has a clean way to pop the frame even if the user forgot a `return`.
//! - Block scope: entering `{ ... }` pushes a fresh scope; declarations inside
//!   are visible only within. Slots are monotonically allocated (no reuse).

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};
use crate::ir::{IrFunction, IrModule, Op};
use crate::value::Value;

pub fn lower(ast: &Ast) -> IrModule {
    // 1. Pre-allocate `functions[0] = main`, then one slot per top-level FnDecl.
    let mut functions: Vec<IrFunction> = vec![IrFunction {
        name: "main".into(),
        arity: 0,
        locals_count: 0,
        code: Vec::new(),
    }];
    let mut fn_names: HashMap<String, u32> = HashMap::new();
    for stmt in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = stmt {
            let id = functions.len() as u32;
            fn_names.insert(name.clone(), id);
            functions.push(IrFunction {
                name: name.clone(),
                arity: params.len() as u8,
                locals_count: 0, // patched after lowering body
                code: Vec::new(),
            });
        }
    }

    let mut consts: Vec<Value> = Vec::new();
    let mut host_fns: Vec<String> = Vec::new();

    // 2. Lower each function body.
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = stmt
        {
            let (code, locals) = {
                let mut l =
                    FnLowering::new(ast, &mut consts, &mut host_fns, &mut functions, &fn_names);
                for p in params {
                    l.declare_local(p.name.clone());
                }
                for s in body {
                    l.lower_stmt(s);
                }
                l.code.push(Op::LoadUndef);
                l.code.push(Op::Ret);
                (std::mem::take(&mut l.code), l.next_slot)
            };
            let fn_id = fn_names[name];
            functions[fn_id as usize].code = code;
            functions[fn_id as usize].locals_count = locals;
        }
    }

    // 3. Lower main (top-level non-FnDecl stmts).
    let (main_code, main_locals) = {
        let mut l = FnLowering::new(ast, &mut consts, &mut host_fns, &mut functions, &fn_names);
        for stmt in &ast.stmts {
            if !matches!(stmt, Stmt::FnDecl { .. }) {
                l.lower_stmt(stmt);
            }
        }
        l.code.push(Op::LoadUndef);
        l.code.push(Op::Ret);
        (std::mem::take(&mut l.code), l.next_slot)
    };
    functions[0].code = main_code;
    functions[0].locals_count = main_locals;

    IrModule {
        consts,
        host_fns,
        functions,
    }
}

struct FnLowering<'a, 'b> {
    ast: &'a Ast,
    consts: &'b mut Vec<Value>,
    host_fns: &'b mut Vec<String>,
    functions: &'b mut Vec<IrFunction>,
    fn_names: &'b HashMap<String, u32>,
    code: Vec<Op>,
    scopes: Vec<HashMap<String, u8>>,
    next_slot: u8,
}

impl<'a, 'b> FnLowering<'a, 'b> {
    fn new(
        ast: &'a Ast,
        consts: &'b mut Vec<Value>,
        host_fns: &'b mut Vec<String>,
        functions: &'b mut Vec<IrFunction>,
        fn_names: &'b HashMap<String, u32>,
    ) -> Self {
        Self {
            ast,
            consts,
            host_fns,
            functions,
            fn_names,
            code: Vec::new(),
            scopes: vec![HashMap::new()],
            next_slot: 0,
        }
    }

    /// Lower an inline arrow-fn body into a fresh `IrFunction` slot.
    /// Returns the new fn_id. Outer per-function state (code/scopes/next_slot)
    /// is saved, replaced with fresh state for the inner body, then restored.
    fn lower_arrow_fn(&mut self, params: &[crate::ast::Param], body: &[Stmt]) -> u32 {
        let new_id = self.functions.len() as u32;
        self.functions.push(IrFunction {
            name: format!("__arrow_{new_id}"),
            arity: params.len() as u8,
            locals_count: 0,
            code: Vec::new(),
        });

        let saved_code = std::mem::take(&mut self.code);
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_next_slot = self.next_slot;

        self.scopes = vec![HashMap::new()];
        self.next_slot = 0;
        // self.code is already empty after the take

        for p in params {
            self.declare_local(p.name.clone());
        }
        for s in body {
            self.lower_stmt(s);
        }
        self.code.push(Op::LoadUndef);
        self.code.push(Op::Ret);

        let new_code = std::mem::take(&mut self.code);
        let new_locals = self.next_slot;
        self.functions[new_id as usize].code = new_code;
        self.functions[new_id as usize].locals_count = new_locals;

        self.code = saved_code;
        self.scopes = saved_scopes;
        self.next_slot = saved_next_slot;

        new_id
    }

    fn declare_local(&mut self, name: String) -> u8 {
        let slot = self.next_slot;
        self.next_slot += 1;
        self.scopes
            .last_mut()
            .expect("at least one scope")
            .insert(name, slot);
        slot
    }

    fn lookup_local(&self, name: &str) -> Option<u8> {
        for s in self.scopes.iter().rev() {
            if let Some(&slot) = s.get(name) {
                return Some(slot);
            }
        }
        None
    }

    fn intern_const(&mut self, v: Value) -> u32 {
        let id = self.consts.len() as u32;
        self.consts.push(v);
        id
    }

    fn intern_host(&mut self, name: &str) -> u32 {
        if let Some(i) = self.host_fns.iter().position(|n| n == name) {
            return i as u32;
        }
        let id = self.host_fns.len() as u32;
        self.host_fns.push(name.into());
        id
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(eid) => {
                self.lower_expr(*eid);
                self.code.push(Op::Pop);
            }
            Stmt::LetDecl { name, init, .. } => {
                self.lower_expr(*init);
                let slot = self.declare_local(name.clone());
                self.code.push(Op::StoreLocal(slot));
            }
            Stmt::Block(stmts) => {
                self.scopes.push(HashMap::new());
                for s in stmts {
                    self.lower_stmt(s);
                }
                self.scopes.pop();
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.lower_expr(*cond);
                let br_pos = self.code.len();
                self.code.push(Op::BrFalse(0));
                self.lower_stmt(then_branch);
                if let Some(eb) = else_branch {
                    let jump_pos = self.code.len();
                    self.code.push(Op::Jump(0));
                    let else_target = self.code.len() as u32;
                    self.code[br_pos] = Op::BrFalse(else_target);
                    self.lower_stmt(eb);
                    let end_target = self.code.len() as u32;
                    self.code[jump_pos] = Op::Jump(end_target);
                } else {
                    let end_target = self.code.len() as u32;
                    self.code[br_pos] = Op::BrFalse(end_target);
                }
            }
            Stmt::While { cond, body } => {
                let loop_start = self.code.len() as u32;
                self.lower_expr(*cond);
                let br_pos = self.code.len();
                self.code.push(Op::BrFalse(0));
                self.lower_stmt(body);
                self.code.push(Op::Jump(loop_start));
                let loop_end = self.code.len() as u32;
                self.code[br_pos] = Op::BrFalse(loop_end);
            }
            Stmt::Return(maybe_expr) => {
                match maybe_expr {
                    Some(eid) => self.lower_expr(*eid),
                    None => self.code.push(Op::LoadUndef),
                }
                self.code.push(Op::Ret);
            }
            Stmt::FnDecl { .. } => {
                // Type checker only allows top-level FnDecls; the program
                // walks them at the top level. Reaching here means the AST
                // contains a nested FnDecl, which we don't yet support.
                unreachable!("nested FnDecl reached lower_stmt");
            }
        }
    }

    fn lower_expr(&mut self, eid: ExprId) {
        match self.ast.get_expr(eid) {
            Expr::String(s) => {
                let cid = self.intern_const(Value::String(Rc::new(s.clone())));
                self.code.push(Op::LoadConst(cid));
            }
            Expr::Number(n) => {
                let cid = self.intern_const(Value::Number(*n));
                self.code.push(Op::LoadConst(cid));
            }
            Expr::Bool(b) => {
                self.code.push(Op::LoadBool(*b));
            }
            Expr::Ident(name) => {
                if let Some(slot) = self.lookup_local(name) {
                    self.code.push(Op::LoadLocal(slot));
                    return;
                }
                if let Some(&fn_id) = self.fn_names.get(name) {
                    let cid = self.intern_const(Value::Function(fn_id));
                    self.code.push(Op::LoadConst(cid));
                    return;
                }
                unreachable!("lower: unknown identifier `{name}` slipped past type-check");
            }
            Expr::Member { obj, name } => {
                if let Expr::Ident(obj_name) = self.ast.get_expr(*obj)
                    && obj_name == "console"
                    && name == "log"
                {
                    let hid = self.intern_host("console.log");
                    self.code.push(Op::LoadHost(hid));
                    return;
                }
                if name == "length" {
                    let obj_id = *obj;
                    self.lower_expr(obj_id);
                    self.code.push(Op::Length);
                    return;
                }
                unreachable!("lower: unsupported member access slipped past type-check");
            }
            Expr::Call { callee, args } => {
                self.lower_expr(*callee);
                for a in args {
                    self.lower_expr(*a);
                }
                self.code.push(Op::Call(args.len() as u8));
            }
            Expr::BinOp { op, left, right } => {
                self.lower_expr(*left);
                self.lower_expr(*right);
                self.code.push(match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Lt => Op::Lt,
                    BinOp::Gt => Op::Gt,
                    BinOp::Le => Op::Le,
                    BinOp::Ge => Op::Ge,
                    BinOp::Eq => Op::Eq3,
                    BinOp::Neq => Op::Neq3,
                });
            }
            Expr::Assign { target, value } => {
                let Expr::Ident(name) = self.ast.get_expr(*target) else {
                    unreachable!("lower: non-ident assignment target slipped past type-check");
                };
                let slot = self
                    .lookup_local(name)
                    .unwrap_or_else(|| panic!("lower: assign to undeclared `{name}`"));
                self.lower_expr(*value);
                self.code.push(Op::StoreLocal(slot));
                self.code.push(Op::LoadLocal(slot));
            }
            Expr::ArrowFn { params, body, .. } => {
                // Clone so we can call &mut self methods without holding a
                // borrow on ast.exprs[eid].
                let params = params.clone();
                let body = body.clone();
                let new_id = self.lower_arrow_fn(&params, &body);
                let cid = self.intern_const(Value::Function(new_id));
                self.code.push(Op::LoadConst(cid));
            }
            Expr::Index { obj, index } => {
                let o = *obj;
                let i = *index;
                self.lower_expr(o);
                self.lower_expr(i);
                self.code.push(Op::IndexAccess);
            }
            Expr::Array(elements) => {
                let ids: Vec<ExprId> = elements.clone();
                let n = ids.len() as u32;
                for eid in ids {
                    self.lower_expr(eid);
                }
                self.code.push(Op::ArrayNew(n));
            }
        }
    }
}
