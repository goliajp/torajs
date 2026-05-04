// v0.3 #3.c: process.argv / Bun.argv via main(argc, argv) plumbing.
// LLVM main signature widened to (i32, ptr); FnLower emits an
// __torajs_argv_init call at main entry to capture into runtime
// globals. process.argv[0] is the binary path on tr (native-compiled
// convention) vs the bun runtime binary on bun — semantic difference
// in argv[0] is documented; this case asserts only on length and
// non-emptiness shapes that agree across both.

const argv = process.argv;
console.log(argv.length > 0);
console.log(argv[0].length > 0);
console.log(typeof argv[0]);

const bargv = Bun.argv;
console.log(bargv.length > 0);
console.log(bargv.length === argv.length);
