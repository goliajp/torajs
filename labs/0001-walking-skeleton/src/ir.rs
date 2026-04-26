//! Stack-machine IR. Shared between interpreter and (future) AOT — keep them in lockstep.

use crate::value::Value;

#[derive(Debug, Clone, Copy)]
pub enum Op {
    LoadConst(u32),
    LoadHost(u32),
    Call(u8),
    Pop,
    Ret,
}

#[derive(Debug, Default)]
pub struct IrModule {
    pub consts: Vec<Value>,
    pub host_fns: Vec<String>,
    pub code: Vec<Op>,
}

impl IrModule {
    pub fn print(&self) {
        println!(".data");
        for (i, c) in self.consts.iter().enumerate() {
            match c {
                Value::String(s) => println!("  const{i}: {:?}", s.as_str()),
                Value::Number(n) => println!("  const{i}: {n}"),
                Value::Undefined => println!("  const{i}: undefined"),
                Value::HostFn(h) => println!("  const{i}: <host {h}>"),
            }
        }
        println!(".host");
        for (i, n) in self.host_fns.iter().enumerate() {
            println!("  host{i}: {n}");
        }
        println!(".code");
        for op in &self.code {
            match op {
                Op::LoadConst(c) => println!("  load_const const{c}"),
                Op::LoadHost(h) => println!("  load_host  host{h}"),
                Op::Call(arity) => println!("  call       {arity}"),
                Op::Pop => println!("  pop"),
                Op::Ret => println!("  ret"),
            }
        }
    }
}
