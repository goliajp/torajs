// RFC 20260812-console-sink knife 2 — console.error / console.warn
// route EVERY runtime type to stderr (sink-pair bracket around the
// stdout-table printer; no per-type _err symbols). The conformance
// oracle only diffs stdout, so the stderr byte-equality vs bun is
// asserted manually in pre-flight (2>/dev/null split); the stdout
// lines here assert the sink switch restores stdout afterwards.
console.error("plain str");
console.warn(42);
console.error(1.5);
console.error(true);
console.error(null);
console.error(undefined);
console.error([1, 2, 3]);
console.error({ a: 1, b: "x" });
console.error(new Map([["k", 1]]));
console.error(new Set([1, 2]));
console.error("a-b".split("-")[0]);
console.error(10n);
console.error("multi", "arg", 3);
console.warn("warn-str");
console.log("stdout-alive-1");
console.error([{ n: 1 }, [2, 3]]);
console.log("stdout-alive-2");
console.error(Symbol("desc"));
console.log("stdout-alive-3");
