//! Tree/IR interpreter. Dev-only — production goes through AOT.

use crate::ir::{IrModule, Op};
use crate::value::Value;

pub fn execute(module: &IrModule) -> Result<(), String> {
    let mut stack: Vec<Value> = Vec::new();
    let mut locals: Vec<Value> = vec![Value::Undefined; module.locals_count as usize];
    let mut pc = 0;

    while pc < module.code.len() {
        let op = module.code[pc];
        pc += 1;
        match op {
            Op::LoadConst(c) => stack.push(module.consts[c as usize].clone()),
            Op::LoadHost(h) => stack.push(Value::HostFn(h)),
            Op::LoadLocal(i) => stack.push(locals[i as usize].clone()),
            Op::StoreLocal(i) => {
                locals[i as usize] = stack.pop().ok_or("stack underflow on store_local")?;
            }
            Op::Call(arity) => {
                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(stack.pop().ok_or("stack underflow popping arg")?);
                }
                args.reverse();
                let callee = stack.pop().ok_or("stack underflow popping callee")?;
                let result = match callee {
                    Value::HostFn(hid) => {
                        let name = &module.host_fns[hid as usize];
                        call_host(name, &args)?
                    }
                    other => return Err(format!("not callable: {other:?}")),
                };
                stack.push(result);
            }
            Op::Pop => {
                stack.pop();
            }
            Op::Ret => break,
            Op::Add => binop(&mut stack, |a, b| a + b)?,
            Op::Sub => binop(&mut stack, |a, b| a - b)?,
            Op::Mul => binop(&mut stack, |a, b| a * b)?,
            Op::Div => binop(&mut stack, |a, b| a / b)?,
        }
    }
    Ok(())
}

fn binop(stack: &mut Vec<Value>, f: impl FnOnce(f64, f64) -> f64) -> Result<(), String> {
    let r = stack.pop().ok_or("stack underflow popping rhs")?;
    let l = stack.pop().ok_or("stack underflow popping lhs")?;
    let (Value::Number(l), Value::Number(r)) = (l, r) else {
        return Err("arithmetic on non-number value".into());
    };
    stack.push(Value::Number(f(l, r)));
    Ok(())
}

fn call_host(name: &str, args: &[Value]) -> Result<Value, String> {
    match name {
        "console.log" => {
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    print!(" ");
                }
                match a {
                    Value::String(s) => print!("{s}"),
                    Value::Number(n) => print!("{n}"),
                    Value::Undefined => print!("undefined"),
                    Value::HostFn(_) => return Err("cannot console.log a host function".into()),
                }
            }
            println!();
            Ok(Value::Undefined)
        }
        other => Err(format!("unknown host function `{other}`")),
    }
}
