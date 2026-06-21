// ES §22.1.3.20 — String.prototype.search(needle, ...trailing) trailing-
// arg ignore. spec reads only needle; tora prior declared `(String) ->
// Number` strict 1-arg sig at check.rs ~4194 (search NOT in the S239
// indexOf-family carve-out → fixture-001), rejecting 2+ arg shape with
// "expected 1 argument(s), got N". S324 widens check.rs via per-method
// carve-out (typecheck-and-drop args[1..]) + ssa_lower_str dispatch loop
// adds `"search"` to the S240 1-useful trailing-drop list so step()-style
// trailing args fire per ES eval-then-discard.

function step(label: string): number {
  console.log(label);
  return 0;
}

const a = "abc xyz".search("xyz", step("t1") as any);
console.log("a=", a);

const b = "abc xyz".search("foo", step("t2") as any, step("t3") as any);
console.log("b=", b);

const c = "hello".search("ll", step("t4") as any);
console.log("c=", c);
