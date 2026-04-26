//! Tree/IR interpreter. Dev-only — production goes through AOT.

use crate::ir::{IrModule, Op};
use crate::value::Value;

struct SavedFrame {
    fn_id: u32,
    pc: usize,
    locals: Vec<Value>,
}

pub fn execute(module: &IrModule) -> Result<(), String> {
    let main = module
        .functions
        .first()
        .ok_or("module has no functions; lower bug")?;
    let mut stack: Vec<Value> = Vec::new();
    let mut current_fn: u32 = 0;
    let mut pc: usize = 0;
    let mut locals: Vec<Value> = vec![Value::Undefined; main.locals_count as usize];
    let mut call_stack: Vec<SavedFrame> = Vec::new();

    loop {
        let func = &module.functions[current_fn as usize];
        if pc >= func.code.len() {
            // missing terminator — lower always emits Ret, so this is a bug
            return Err(format!(
                "ran off end of function `{}` without a terminator",
                func.name
            ));
        }
        let op = func.code[pc];
        pc += 1;
        match op {
            Op::LoadConst(c) => stack.push(module.consts[c as usize].clone()),
            Op::LoadHost(h) => stack.push(Value::HostFn(h)),
            Op::LoadLocal(i) => stack.push(locals[i as usize].clone()),
            Op::StoreLocal(i) => {
                locals[i as usize] = stack.pop().ok_or("stack underflow on store_local")?;
            }
            Op::LoadBool(b) => stack.push(Value::Bool(b)),
            Op::LoadUndef => stack.push(Value::Undefined),
            Op::Call(arity) => {
                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(stack.pop().ok_or("stack underflow popping arg")?);
                }
                args.reverse();
                let callee = stack.pop().ok_or("stack underflow popping callee")?;
                match callee {
                    Value::HostFn(hid) => {
                        let name = &module.host_fns[hid as usize];
                        let result = call_host(name, &args)?;
                        stack.push(result);
                    }
                    Value::Function(fid) => {
                        let target = &module.functions[fid as usize];
                        if target.arity as usize != args.len() {
                            return Err(format!(
                                "arity mismatch calling `{}`: expected {}, got {}",
                                target.name,
                                target.arity,
                                args.len()
                            ));
                        }
                        call_stack.push(SavedFrame {
                            fn_id: current_fn,
                            pc,
                            locals: std::mem::take(&mut locals),
                        });
                        let mut new_locals = vec![Value::Undefined; target.locals_count as usize];
                        for (i, a) in args.into_iter().enumerate() {
                            new_locals[i] = a;
                        }
                        locals = new_locals;
                        current_fn = fid;
                        pc = 0;
                    }
                    other => return Err(format!("not callable: {other:?}")),
                }
            }
            Op::Pop => {
                stack.pop();
            }
            Op::Ret => {
                if let Some(saved) = call_stack.pop() {
                    current_fn = saved.fn_id;
                    pc = saved.pc;
                    locals = saved.locals;
                    // return value remains on stack for caller to consume
                } else {
                    break; // returning from main → done
                }
            }
            Op::Add => binop(&mut stack, |a, b| a + b)?,
            Op::Sub => binop(&mut stack, |a, b| a - b)?,
            Op::Mul => binop(&mut stack, |a, b| a * b)?,
            Op::Div => binop(&mut stack, |a, b| a / b)?,
            Op::Lt => cmp_num(&mut stack, |a, b| a < b)?,
            Op::Gt => cmp_num(&mut stack, |a, b| a > b)?,
            Op::Le => cmp_num(&mut stack, |a, b| a <= b)?,
            Op::Ge => cmp_num(&mut stack, |a, b| a >= b)?,
            Op::Eq3 => strict_eq(&mut stack, false)?,
            Op::Neq3 => strict_eq(&mut stack, true)?,
            Op::Jump(target) => pc = target as usize,
            Op::BrFalse(target) => {
                let v = stack.pop().ok_or("stack underflow on br_false")?;
                let Value::Bool(b) = v else {
                    return Err(format!("br_false expects boolean, got {v:?}"));
                };
                if !b {
                    pc = target as usize;
                }
            }
        }
    }
    Ok(())
}

fn cmp_num(stack: &mut Vec<Value>, f: impl FnOnce(f64, f64) -> bool) -> Result<(), String> {
    let r = stack.pop().ok_or("stack underflow popping rhs")?;
    let l = stack.pop().ok_or("stack underflow popping lhs")?;
    let (Value::Number(l), Value::Number(r)) = (l, r) else {
        return Err("comparison on non-number value".into());
    };
    stack.push(Value::Bool(f(l, r)));
    Ok(())
}

fn strict_eq(stack: &mut Vec<Value>, negate: bool) -> Result<(), String> {
    let r = stack.pop().ok_or("stack underflow popping rhs")?;
    let l = stack.pop().ok_or("stack underflow popping lhs")?;
    let eq = match (&l, &r) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        _ => {
            return Err(format!(
                "strict equality on incompatible types: {l:?} vs {r:?}"
            ));
        }
    };
    stack.push(Value::Bool(if negate { !eq } else { eq }));
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
                    Value::Bool(b) => print!("{b}"),
                    Value::Undefined => print!("undefined"),
                    Value::HostFn(_) | Value::Function(_) => {
                        return Err("cannot console.log a function".into());
                    }
                }
            }
            println!();
            Ok(Value::Undefined)
        }
        other => Err(format!("unknown host function `{other}`")),
    }
}
