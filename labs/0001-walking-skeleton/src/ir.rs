//! Stack-machine IR. Shared between interpreter and (future) AOT — keep them in lockstep.
//!
//! Each function carries its own code vec; jumps are absolute indices into that vec.
//! Index 0 of `IrModule.functions` is `main` (the top-level program).

use crate::value::Value;

#[derive(Debug, Clone, Copy)]
pub enum Op {
    LoadConst(u32),
    LoadHost(u32),
    LoadLocal(u8),
    StoreLocal(u8),
    LoadBool(bool),
    LoadUndef,
    Call(u8),
    Pop,
    Ret,
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Gt,
    Le,
    Ge,
    Eq3,  // === strict equality
    Neq3, // !==
    Jump(u32),
    BrFalse(u32),
}

#[derive(Debug)]
pub struct IrFunction {
    pub name: String,
    pub arity: u8,
    pub locals_count: u8,
    pub code: Vec<Op>,
}

#[derive(Debug, Default)]
pub struct IrModule {
    pub consts: Vec<Value>,
    pub host_fns: Vec<String>,
    /// `functions[0]` is `main` (top-level code, arity 0).
    pub functions: Vec<IrFunction>,
}

impl IrModule {
    pub fn print(&self) {
        println!(".data");
        for (i, c) in self.consts.iter().enumerate() {
            match c {
                Value::String(s) => println!("  const{i}: {:?}", s.as_str()),
                Value::Number(n) => println!("  const{i}: {n}"),
                Value::Bool(b) => println!("  const{i}: {b}"),
                Value::Undefined => println!("  const{i}: undefined"),
                Value::HostFn(h) => println!("  const{i}: <host {h}>"),
                Value::Function(f) => println!("  const{i}: <fn {f}>"),
            }
        }
        println!(".host");
        for (i, n) in self.host_fns.iter().enumerate() {
            println!("  host{i}: {n}");
        }
        for f in &self.functions {
            println!();
            println!(
                ".function {} arity={} locals={}",
                f.name, f.arity, f.locals_count
            );
            for (i, op) in f.code.iter().enumerate() {
                print_op(i, op);
            }
        }
    }
}

fn print_op(i: usize, op: &Op) {
    match op {
        Op::LoadConst(c) => println!("  {i:>3}: load_const  const{c}"),
        Op::LoadHost(h) => println!("  {i:>3}: load_host   host{h}"),
        Op::LoadLocal(s) => println!("  {i:>3}: load_local  local{s}"),
        Op::StoreLocal(s) => println!("  {i:>3}: store_local local{s}"),
        Op::LoadBool(b) => println!("  {i:>3}: load_bool   {b}"),
        Op::LoadUndef => println!("  {i:>3}: load_undef"),
        Op::Call(arity) => println!("  {i:>3}: call        {arity}"),
        Op::Pop => println!("  {i:>3}: pop"),
        Op::Ret => println!("  {i:>3}: ret"),
        Op::Add => println!("  {i:>3}: add"),
        Op::Sub => println!("  {i:>3}: sub"),
        Op::Mul => println!("  {i:>3}: mul"),
        Op::Div => println!("  {i:>3}: div"),
        Op::Lt => println!("  {i:>3}: lt"),
        Op::Gt => println!("  {i:>3}: gt"),
        Op::Le => println!("  {i:>3}: le"),
        Op::Ge => println!("  {i:>3}: ge"),
        Op::Eq3 => println!("  {i:>3}: eq3"),
        Op::Neq3 => println!("  {i:>3}: neq3"),
        Op::Jump(t) => println!("  {i:>3}: jump        @{t}"),
        Op::BrFalse(t) => println!("  {i:>3}: br_false    @{t}"),
    }
}
