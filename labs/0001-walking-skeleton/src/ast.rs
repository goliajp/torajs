//! AST — arena-allocated. Children referenced by `ExprId(u32)`, not Box.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String),
    String(String),
    Number(f64),
    BinOp {
        op: BinOp,
        left: ExprId,
        right: ExprId,
    },
    Member {
        obj: ExprId,
        name: String,
    },
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    Assign {
        target: ExprId,
        value: ExprId,
    },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(ExprId),
    LetDecl {
        mutable: bool,
        name: String,
        type_ann: Option<String>,
        init: ExprId,
    },
}

#[derive(Debug, Default)]
pub struct Ast {
    pub stmts: Vec<Stmt>,
    pub exprs: Vec<Expr>,
}

impl Ast {
    pub fn add_expr(&mut self, e: Expr) -> ExprId {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(e);
        id
    }

    pub fn get_expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.0 as usize]
    }

    pub fn print(&self) {
        for s in &self.stmts {
            match s {
                Stmt::Expr(eid) => {
                    println!("ExprStmt");
                    self.print_expr(*eid, 1);
                }
                Stmt::LetDecl {
                    mutable,
                    name,
                    type_ann,
                    init,
                } => {
                    let kw = if *mutable { "let" } else { "const" };
                    match type_ann {
                        Some(ann) => println!("{kw} {name}: {ann}"),
                        None => println!("{kw} {name}"),
                    }
                    self.print_expr(*init, 1);
                }
            }
        }
    }

    fn print_expr(&self, id: ExprId, indent: usize) {
        let pad = "  ".repeat(indent);
        match self.get_expr(id) {
            Expr::Ident(n) => println!("{pad}Ident({n:?})"),
            Expr::String(s) => println!("{pad}String({s:?})"),
            Expr::Number(n) => println!("{pad}Number({n})"),
            Expr::BinOp { op, left, right } => {
                println!("{pad}BinOp({op:?})");
                self.print_expr(*left, indent + 1);
                self.print_expr(*right, indent + 1);
            }
            Expr::Member { obj, name } => {
                println!("{pad}Member");
                self.print_expr(*obj, indent + 1);
                println!("{pad}  .{name}");
            }
            Expr::Call { callee, args } => {
                println!("{pad}Call");
                self.print_expr(*callee, indent + 1);
                println!("{pad}  args:");
                for a in args {
                    self.print_expr(*a, indent + 2);
                }
            }
            Expr::Assign { target, value } => {
                println!("{pad}Assign");
                self.print_expr(*target, indent + 1);
                println!("{pad}  =");
                self.print_expr(*value, indent + 1);
            }
        }
    }
}
