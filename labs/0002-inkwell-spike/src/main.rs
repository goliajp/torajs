// Inkwell spike: hand-built LLVM IR for fib40, emit object file, link with
// system ld → native binary. Time it vs current torajs-aot (150 ms target).
//
// What this validates:
//   1. Inkwell + llvm-sys-221 + brew LLVM 22 link cleanly on darwin/arm64
//   2. We can construct an SSA module programmatically (the API shape we'll
//      use in P3.5 to lower our IR)
//   3. LLVM 22 -O1 on a recursive i64 fib matches or beats Apple clang 21 -O1
//      via the wasm-via-C path (current torajs-aot fib40 = 150 ms)
//
// Non-goals:
//   - frontend integration (P3.5)
//   - shared SSA IR (P3.5)
//   - production linker invocation (use system `cc` for now)
//
// Usage:
//   LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm cargo run --release -p inkwell-spike

use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use std::path::PathBuf;
use std::process::Command;

fn build_module<'ctx>(ctx: &'ctx Context) -> Module<'ctx> {
    let module = ctx.create_module("fib40");
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let void_t = ctx.void_type();

    // declare i32 @putchar(i32)
    let putchar_t = i32_t.fn_type(&[i32_t.into()], false);
    let putchar = module.add_function("putchar", putchar_t, None);

    // define i64 @fib(i64 %n) { ... } — straight recursive form, no memoization,
    // matches what tree-walk + wasm-via-C versions do.
    let fib_t = i64_t.fn_type(&[i64_t.into()], false);
    let fib_fn = module.add_function("fib", fib_t, None);
    {
        let entry = ctx.append_basic_block(fib_fn, "entry");
        let recurse = ctx.append_basic_block(fib_fn, "recurse");
        let base = ctx.append_basic_block(fib_fn, "base");
        builder.position_at_end(entry);

        let n = fib_fn.get_nth_param(0).unwrap().into_int_value();
        let two = i64_t.const_int(2, false);
        let cmp = builder
            .build_int_compare(inkwell::IntPredicate::SLT, n, two, "lt2")
            .unwrap();
        builder.build_conditional_branch(cmp, base, recurse).unwrap();

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

    // define i32 @main() {
    //   %r = call i64 @fib(i64 40)
    //   ; print decimal digits of %r + '\n' via repeated putchar
    //   ret i32 0
    // }
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

    // print_i64(%r) — inline impl: divide-by-10 in a loop, push digits,
    // pop them out via putchar. fib(40) = 102334155 fits in 9 digits.
    let print_i64_t = void_t.fn_type(&[i64_t.into()], false);
    let print_i64 = module.add_function("print_i64", print_i64_t, None);
    let pi_entry = ctx.append_basic_block(print_i64, "entry");
    let pi_loop1 = ctx.append_basic_block(print_i64, "loop1");
    let pi_loop2 = ctx.append_basic_block(print_i64, "loop2");
    let pi_done = ctx.append_basic_block(print_i64, "done");
    builder.position_at_end(pi_entry);
    let buf = builder.build_alloca(i64_t.array_type(20), "buf").unwrap();
    let count_alloca = builder.build_alloca(i64_t, "count").unwrap();
    builder
        .build_store(count_alloca, i64_t.const_int(0, false))
        .unwrap();
    let n_alloca = builder.build_alloca(i64_t, "n").unwrap();
    let n_param = print_i64.get_nth_param(0).unwrap().into_int_value();
    builder.build_store(n_alloca, n_param).unwrap();
    builder.build_unconditional_branch(pi_loop1).unwrap();

    // loop1: while (n > 0) { buf[count++] = n%10 + '0'; n /= 10 }
    builder.position_at_end(pi_loop1);
    let n_cur = builder
        .build_load(i64_t, n_alloca, "n_cur")
        .unwrap()
        .into_int_value();
    let zero = i64_t.const_int(0, false);
    let cmp = builder
        .build_int_compare(inkwell::IntPredicate::SGT, n_cur, zero, "n_pos")
        .unwrap();
    let dump_block = ctx.append_basic_block(print_i64, "dump_block");
    builder
        .build_conditional_branch(cmp, dump_block, pi_loop2)
        .unwrap();

    builder.position_at_end(dump_block);
    let ten = i64_t.const_int(10, false);
    let digit = builder.build_int_signed_rem(n_cur, ten, "digit").unwrap();
    let ascii = builder
        .build_int_add(digit, i64_t.const_int(b'0' as u64, false), "ascii")
        .unwrap();
    let cnt = builder
        .build_load(i64_t, count_alloca, "cnt")
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
    builder.build_store(count_alloca, cnt_next).unwrap();
    let n_next = builder.build_int_signed_div(n_cur, ten, "n_next").unwrap();
    builder.build_store(n_alloca, n_next).unwrap();
    builder.build_unconditional_branch(pi_loop1).unwrap();

    // loop2: pop digits and putchar each
    builder.position_at_end(pi_loop2);
    let cnt2 = builder
        .build_load(i64_t, count_alloca, "cnt2")
        .unwrap()
        .into_int_value();
    let cnt_zero = builder
        .build_int_compare(inkwell::IntPredicate::SGT, cnt2, zero, "still")
        .unwrap();
    let pop_block = ctx.append_basic_block(print_i64, "pop_block");
    builder
        .build_conditional_branch(cnt_zero, pop_block, pi_done)
        .unwrap();

    builder.position_at_end(pop_block);
    let cnt2_dec = builder
        .build_int_sub(cnt2, i64_t.const_int(1, false), "cnt_dec")
        .unwrap();
    builder.build_store(count_alloca, cnt2_dec).unwrap();
    let pop_slot = unsafe {
        builder
            .build_in_bounds_gep(
                i64_t.array_type(20),
                buf,
                &[i64_t.const_int(0, false), cnt2_dec],
                "pop_slot",
            )
            .unwrap()
    };
    let ch = builder
        .build_load(i64_t, pop_slot, "ch")
        .unwrap()
        .into_int_value();
    let ch32 = builder.build_int_truncate(ch, i32_t, "ch32").unwrap();
    builder
        .build_call(putchar, &[ch32.into()], "_pc")
        .unwrap();
    builder.build_unconditional_branch(pi_loop2).unwrap();

    builder.position_at_end(pi_done);
    let nl = i32_t.const_int(b'\n' as u64, false);
    builder.build_call(putchar, &[nl.into()], "_nl").unwrap();
    builder.build_return(None).unwrap();

    // back to main: call print_i64(r) and ret 0
    builder.position_at_end(entry);
    builder.build_call(print_i64, &[r.into()], "_pr").unwrap();
    builder
        .build_return(Some(&i32_t.const_int(0, false)))
        .unwrap();

    module
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::create();
    let module = build_module(&ctx);

    if let Err(e) = module.verify() {
        eprintln!("module verify failed:\n{}", e.to_string());
        return Err("verify".into());
    }

    let ir_path: PathBuf = std::env::temp_dir().join("inkwell-spike-fib40.ll");
    module.print_to_file(&ir_path)?;
    eprintln!("ir: {}", ir_path.display());

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
            OptimizationLevel::Less, // -O1: matches fib40's bench-tuned aot_clang_flags
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or("create_target_machine returned None")?;

    // Run the IR-level optimization pipeline. write_to_file alone runs only
    // the codegen passes (instruction selection, register allocation); it does
    // NOT run instcombine, mem2reg, gvn, simplifycfg, or any of the IR-level
    // cleanup that clang -O1 includes by default.  Without this, the recursive
    // fib() runs about 1.5× slower than the wasm-via-C torajs-aot baseline.
    let level = std::env::var("SPIKE_OPT").unwrap_or_else(|_| "O1".into());
    let pipeline = format!("default<{level}>");
    module
        .run_passes(&pipeline, &machine, PassBuilderOptions::create())
        .map_err(|e| format!("run_passes({pipeline}): {}", e.to_string()))?;
    eprintln!("opt: {pipeline}");

    let ir_after_path: PathBuf = std::env::temp_dir().join("inkwell-spike-fib40.opt.ll");
    module.print_to_file(&ir_after_path)?;

    let obj_path: PathBuf = std::env::temp_dir().join("inkwell-spike-fib40.o");
    machine.write_to_file(&module, FileType::Object, &obj_path)?;
    eprintln!("obj: {}", obj_path.display());

    let bin_path: PathBuf = std::env::temp_dir().join("inkwell-spike-fib40");
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
