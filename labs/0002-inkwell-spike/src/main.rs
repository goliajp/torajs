// Inkwell spike: hand-built LLVM IR for bench cases, emit object file, link
// with system cc → native binary. Time the result vs current torajs-aot.
//
// What this validates (gate for the P3.5 pivot to a real LLVM backend):
//   1. Inkwell + llvm-sys-221 + brew LLVM 22 link cleanly on darwin/arm64
//   2. We can construct SSA modules programmatically — the API shape we'll
//      use in P3.5 to lower our SSA IR
//   3. LLVM 22 + module.run_passes("default<O1>") matches Apple clang 21 -O1
//      on the perf-leading bench cases
//   4. LLVM's loop-idiom recognizer fires on the Brian Kernighan popcount
//      pattern — this is the moat (we beat rust on popcount only because
//      LLVM detects it and emits ARM `cnt.16b` NEON)
//
// Usage:
//   LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm \
//   cargo run --release -p inkwell-spike -- <fib40|popcount> [opt-level]
//   # opt-level ∈ {O0, O1, O2, O3}; default O1.

use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::values::FunctionValue;
use std::path::PathBuf;
use std::process::Command;

/// Adds a `void @print_i64(i64)` function that writes the decimal digits of
/// the argument followed by a newline via libc `putchar`. Same impl shape
/// as the wasm-via-C `print_i64` baked into bench/aot-host/main.c, just
/// re-emitted as LLVM IR so we don't depend on wasm2c output.
fn add_print_i64<'ctx>(
    ctx: &'ctx Context,
    module: &Module<'ctx>,
    putchar: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let void_t = ctx.void_type();

    let fn_t = void_t.fn_type(&[i64_t.into()], false);
    let f = module.add_function("print_i64", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let loop1 = ctx.append_basic_block(f, "loop1");
    let dump = ctx.append_basic_block(f, "dump");
    let loop2 = ctx.append_basic_block(f, "loop2");
    let pop = ctx.append_basic_block(f, "pop");
    let done = ctx.append_basic_block(f, "done");

    builder.position_at_end(entry);
    let buf = builder.build_alloca(i64_t.array_type(20), "buf").unwrap();
    let cnt_a = builder.build_alloca(i64_t, "count").unwrap();
    builder
        .build_store(cnt_a, i64_t.const_int(0, false))
        .unwrap();
    let n_a = builder.build_alloca(i64_t, "n").unwrap();
    let arg = f.get_nth_param(0).unwrap().into_int_value();
    builder.build_store(n_a, arg).unwrap();
    builder.build_unconditional_branch(loop1).unwrap();

    builder.position_at_end(loop1);
    let n_cur = builder
        .build_load(i64_t, n_a, "n_cur")
        .unwrap()
        .into_int_value();
    let zero = i64_t.const_int(0, false);
    let pos = builder
        .build_int_compare(inkwell::IntPredicate::SGT, n_cur, zero, "pos")
        .unwrap();
    builder.build_conditional_branch(pos, dump, loop2).unwrap();

    builder.position_at_end(dump);
    let ten = i64_t.const_int(10, false);
    let digit = builder.build_int_signed_rem(n_cur, ten, "digit").unwrap();
    let ascii = builder
        .build_int_add(digit, i64_t.const_int(b'0' as u64, false), "ascii")
        .unwrap();
    let cnt = builder
        .build_load(i64_t, cnt_a, "cnt")
        .unwrap()
        .into_int_value();
    let slot = unsafe {
        builder
            .build_in_bounds_gep(
                i64_t.array_type(20),
                buf,
                &[i64_t.const_int(0, false), cnt],
                "slot",
            )
            .unwrap()
    };
    builder.build_store(slot, ascii).unwrap();
    let cnt_next = builder
        .build_int_add(cnt, i64_t.const_int(1, false), "cnt_next")
        .unwrap();
    builder.build_store(cnt_a, cnt_next).unwrap();
    let n_next = builder.build_int_signed_div(n_cur, ten, "n_next").unwrap();
    builder.build_store(n_a, n_next).unwrap();
    builder.build_unconditional_branch(loop1).unwrap();

    builder.position_at_end(loop2);
    let cnt2 = builder
        .build_load(i64_t, cnt_a, "cnt2")
        .unwrap()
        .into_int_value();
    let still = builder
        .build_int_compare(inkwell::IntPredicate::SGT, cnt2, zero, "still")
        .unwrap();
    builder.build_conditional_branch(still, pop, done).unwrap();

    builder.position_at_end(pop);
    let cnt_dec = builder
        .build_int_sub(cnt2, i64_t.const_int(1, false), "cnt_dec")
        .unwrap();
    builder.build_store(cnt_a, cnt_dec).unwrap();
    let pop_slot = unsafe {
        builder
            .build_in_bounds_gep(
                i64_t.array_type(20),
                buf,
                &[i64_t.const_int(0, false), cnt_dec],
                "pop_slot",
            )
            .unwrap()
    };
    let ch = builder
        .build_load(i64_t, pop_slot, "ch")
        .unwrap()
        .into_int_value();
    let ch32 = builder.build_int_truncate(ch, i32_t, "ch32").unwrap();
    builder.build_call(putchar, &[ch32.into()], "_pc").unwrap();
    builder.build_unconditional_branch(loop2).unwrap();

    builder.position_at_end(done);
    let nl = i32_t.const_int(b'\n' as u64, false);
    builder.build_call(putchar, &[nl.into()], "_nl").unwrap();
    builder.build_return(None).unwrap();

    f
}

fn build_fib40<'ctx>(ctx: &'ctx Context) -> Module<'ctx> {
    let module = ctx.create_module("fib40");
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();

    let putchar_t = i32_t.fn_type(&[i32_t.into()], false);
    let putchar = module.add_function("putchar", putchar_t, None);
    let print_i64 = add_print_i64(ctx, &module, putchar);

    let fib_t = i64_t.fn_type(&[i64_t.into()], false);
    let fib_fn = module.add_function("fib", fib_t, None);
    {
        let entry = ctx.append_basic_block(fib_fn, "entry");
        let recurse = ctx.append_basic_block(fib_fn, "recurse");
        let base = ctx.append_basic_block(fib_fn, "base");
        builder.position_at_end(entry);
        let n = fib_fn.get_nth_param(0).unwrap().into_int_value();
        let two = i64_t.const_int(2, false);
        let lt2 = builder
            .build_int_compare(inkwell::IntPredicate::SLT, n, two, "lt2")
            .unwrap();
        builder
            .build_conditional_branch(lt2, base, recurse)
            .unwrap();

        builder.position_at_end(base);
        builder.build_return(Some(&n)).unwrap();

        builder.position_at_end(recurse);
        let one = i64_t.const_int(1, false);
        let n1 = builder.build_int_sub(n, one, "n1").unwrap();
        let n2 = builder.build_int_sub(n, two, "n2").unwrap();
        let f1 = builder
            .build_call(fib_fn, &[n1.into()], "f1")
            .unwrap()
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let f2 = builder
            .build_call(fib_fn, &[n2.into()], "f2")
            .unwrap()
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let sum = builder.build_int_add(f1, f2, "sum").unwrap();
        builder.build_return(Some(&sum)).unwrap();
    }

    let main_t = i32_t.fn_type(&[], false);
    let main_fn = module.add_function("main", main_t, None);
    let entry = ctx.append_basic_block(main_fn, "entry");
    builder.position_at_end(entry);
    let r = builder
        .build_call(fib_fn, &[i64_t.const_int(40, false).into()], "r")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    builder.build_call(print_i64, &[r.into()], "_pr").unwrap();
    builder
        .build_return(Some(&i32_t.const_int(0, false)))
        .unwrap();

    module
}

/// popcount(x): Brian Kernighan loop. main: sum popcount(0..10_000_000).
/// Critical case: LLVM must recognize the BK pattern and emit @llvm.ctpop.i64
/// → ARM cnt.16b NEON. Without that, this case regresses 20× (from 2.86 ms
/// to ~57 ms — bun-class).
fn build_popcount<'ctx>(ctx: &'ctx Context) -> Module<'ctx> {
    let module = ctx.create_module("popcount");
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();

    let putchar_t = i32_t.fn_type(&[i32_t.into()], false);
    let putchar = module.add_function("putchar", putchar_t, None);
    let print_i64 = add_print_i64(ctx, &module, putchar);

    // i64 popcount(i64 x)
    let popcnt_t = i64_t.fn_type(&[i64_t.into()], false);
    let popcnt_fn = module.add_function("popcount", popcnt_t, None);
    {
        let entry = ctx.append_basic_block(popcnt_fn, "entry");
        let loop_h = ctx.append_basic_block(popcnt_fn, "loop_h");
        let loop_b = ctx.append_basic_block(popcnt_fn, "loop_b");
        let exit = ctx.append_basic_block(popcnt_fn, "exit");
        builder.position_at_end(entry);
        let x = popcnt_fn.get_nth_param(0).unwrap().into_int_value();
        let n_a = builder.build_alloca(i64_t, "n").unwrap();
        let cnt_a = builder.build_alloca(i64_t, "count").unwrap();
        builder.build_store(n_a, x).unwrap();
        builder
            .build_store(cnt_a, i64_t.const_int(0, false))
            .unwrap();
        builder.build_unconditional_branch(loop_h).unwrap();

        builder.position_at_end(loop_h);
        let n = builder
            .build_load(i64_t, n_a, "n")
            .unwrap()
            .into_int_value();
        let zero = i64_t.const_int(0, false);
        let nz = builder
            .build_int_compare(inkwell::IntPredicate::NE, n, zero, "nz")
            .unwrap();
        builder.build_conditional_branch(nz, loop_b, exit).unwrap();

        builder.position_at_end(loop_b);
        // n = n & (n - 1)
        let n_minus_1 = builder
            .build_int_sub(n, i64_t.const_int(1, false), "nm1")
            .unwrap();
        let n_new = builder.build_and(n, n_minus_1, "n_and").unwrap();
        builder.build_store(n_a, n_new).unwrap();
        // count = count + 1
        let cnt = builder
            .build_load(i64_t, cnt_a, "cnt")
            .unwrap()
            .into_int_value();
        let cnt_new = builder
            .build_int_add(cnt, i64_t.const_int(1, false), "cnt_new")
            .unwrap();
        builder.build_store(cnt_a, cnt_new).unwrap();
        builder.build_unconditional_branch(loop_h).unwrap();

        builder.position_at_end(exit);
        let cnt = builder
            .build_load(i64_t, cnt_a, "cnt_ret")
            .unwrap()
            .into_int_value();
        builder.build_return(Some(&cnt)).unwrap();
    }

    // i32 main():
    //   total = 0; i = 0
    //   while (i < 10_000_000) { total += popcount(i); i += 1 }
    //   print_i64(total); ret 0
    let main_t = i32_t.fn_type(&[], false);
    let main_fn = module.add_function("main", main_t, None);
    let entry = ctx.append_basic_block(main_fn, "entry");
    let loop_h = ctx.append_basic_block(main_fn, "loop_h");
    let loop_b = ctx.append_basic_block(main_fn, "loop_b");
    let exit = ctx.append_basic_block(main_fn, "exit");

    builder.position_at_end(entry);
    let total_a = builder.build_alloca(i64_t, "total").unwrap();
    let i_a = builder.build_alloca(i64_t, "i").unwrap();
    builder
        .build_store(total_a, i64_t.const_int(0, false))
        .unwrap();
    builder.build_store(i_a, i64_t.const_int(0, false)).unwrap();
    builder.build_unconditional_branch(loop_h).unwrap();

    builder.position_at_end(loop_h);
    let i = builder
        .build_load(i64_t, i_a, "i")
        .unwrap()
        .into_int_value();
    let limit = i64_t.const_int(10_000_000, false);
    let lt = builder
        .build_int_compare(inkwell::IntPredicate::SLT, i, limit, "lt")
        .unwrap();
    builder.build_conditional_branch(lt, loop_b, exit).unwrap();

    builder.position_at_end(loop_b);
    let pc = builder
        .build_call(popcnt_fn, &[i.into()], "pc")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    let total = builder
        .build_load(i64_t, total_a, "total")
        .unwrap()
        .into_int_value();
    let total_new = builder.build_int_add(total, pc, "total_new").unwrap();
    builder.build_store(total_a, total_new).unwrap();
    let i_new = builder
        .build_int_add(i, i64_t.const_int(1, false), "i_new")
        .unwrap();
    builder.build_store(i_a, i_new).unwrap();
    builder.build_unconditional_branch(loop_h).unwrap();

    builder.position_at_end(exit);
    let total = builder
        .build_load(i64_t, total_a, "total_ret")
        .unwrap()
        .into_int_value();
    builder
        .build_call(print_i64, &[total.into()], "_pr")
        .unwrap();
    builder
        .build_return(Some(&i32_t.const_int(0, false)))
        .unwrap();

    module
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let case = args.get(1).map(String::as_str).unwrap_or("fib40");
    let level = args.get(2).cloned().unwrap_or_else(|| "O1".into());

    let ctx = Context::create();
    let module = match case {
        "fib40" => build_fib40(&ctx),
        "popcount" => build_popcount(&ctx),
        other => return Err(format!("unknown case: {other} (try fib40 | popcount)").into()),
    };

    if let Err(e) = module.verify() {
        eprintln!("module verify failed:\n{}", e.to_string());
        return Err("verify".into());
    }

    let ir_path: PathBuf =
        std::env::temp_dir().join(format!("inkwell-spike-{case}-{level}.pre.ll"));
    module.print_to_file(&ir_path)?;
    eprintln!("ir(pre): {}", ir_path.display());

    Target::initialize_aarch64(&InitializationConfig::default());
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple)?;
    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();
    eprintln!("triple: {triple} cpu: {cpu}");

    let machine = target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::Less,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or("create_target_machine returned None")?;

    let pipeline = format!("default<{level}>");
    module
        .run_passes(&pipeline, &machine, PassBuilderOptions::create())
        .map_err(|e| format!("run_passes({pipeline}): {}", e.to_string()))?;
    eprintln!("opt: {pipeline}");

    let ir_after_path: PathBuf =
        std::env::temp_dir().join(format!("inkwell-spike-{case}-{level}.opt.ll"));
    module.print_to_file(&ir_after_path)?;
    eprintln!("ir(post): {}", ir_after_path.display());

    let obj_path: PathBuf = std::env::temp_dir().join(format!("inkwell-spike-{case}-{level}.o"));
    machine.write_to_file(&module, FileType::Object, &obj_path)?;

    let bin_path: PathBuf = std::env::temp_dir().join(format!("inkwell-spike-{case}-{level}"));
    let status = Command::new("cc")
        .arg(&obj_path)
        .arg("-o")
        .arg(&bin_path)
        .status()?;
    if !status.success() {
        return Err(format!("cc link failed: {status}").into());
    }
    eprintln!("bin: {}", bin_path.display());
    println!("{}", bin_path.display());
    Ok(())
}
