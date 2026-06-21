// WHATWG HTML §queueMicrotask + Web IDL §3.2.1 over-arity rule —
// operations silently ignore trailing args. Prior tora check.rs
// hard-coded `args.len() != 1` reject; spec-aligned tora widens
// the gate + typecheck-and-drop args[1..] + ssa_lower mirror
// lower-and-drop so step()-style trailing side-effect exprs fire
// per Web IDL eval-then-discard semantics. S323.

function step(label: string): number {
  console.log(label);
  return 0;
}

queueMicrotask((): void => {
  console.log("micro1");
}, step("t1") as any);

queueMicrotask((): void => {
  console.log("micro2");
}, step("t2") as any, step("t3") as any);

console.log("after queue");
