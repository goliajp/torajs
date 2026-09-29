// v0.3 #3.c: process.argv / Bun.argv via main(argc, argv) plumbing.
// LLVM main signature widened to (i32, ptr); FnLower emits an
// __torajs_argv_init call at main entry to capture into runtime
// globals. process.argv[0] and [1] are the binary path on tr vs the
// runtime binary and the script path on bun; the strings differ, the
// count does not, and the first user argument is at index 2 on both.

const argv = process.argv;
console.log(argv.length);
console.log(argv[1].length > 0);
console.log(argv[2] === undefined);
console.log(argv[0].length > 0);
console.log(typeof argv[0]);

const bargv = Bun.argv;
console.log(bargv.length > 0);
console.log(bargv.length === argv.length);
