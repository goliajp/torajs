// A module that awaits at its top level and imports another module
// dynamically (§13.3.10): the import's promise is awaited in place like
// any other, inside a `try` and in a loop, and a function value that
// imports reaches the same namespace. When the import settles is the
// host's to decide (§16.2.1.10 HostLoadImportedModule), so no `.then`
// chain races it here.
const log: string[] = [];

const first = await Promise.resolve(1);
log.push("first " + first);
const mod = await import("./tla-dynimport-001-lib.ts");
log.push("lib " + mod.twice(21));
try {
  const again = await import("./tla-dynimport-001-lib.ts");
  log.push("same " + (again === mod) + " " + again.LABEL);
} catch (e) {
  log.push("caught " + e);
}
const load = async () => (await import("./tla-dynimport-001-lib.ts")).twice(5);
for (let i = 0; i < 2; i++) log.push("loop " + (await load()));
await null;
for (const line of log) console.log(line);
