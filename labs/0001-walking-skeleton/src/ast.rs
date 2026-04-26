//! AST — arena-allocated. Children referenced by `ExprId(u32)`, not Box.
//!
//! P0 only models the subset needed for `console.log("hello")`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String),
    String(String),
    Number(f64),
    Member { obj: ExprId, name: String },
    Call { callee: ExprId, args: Vec<ExprId> },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(ExprId),
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
            }
        }
    }

    fn print_expr(&self, id: ExprId, indent: usize) {
        let pad = "  ".repeat(indent);
        match self.get_expr(id) {
            Expr::Ident(n) => println!("{pad}Ident({n:?})"),
            Expr::String(s) => println!("{pad}String({s:?})"),
            Expr::Number(n) => println!("{pad}Number({n})"),
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
        }
    }
}
