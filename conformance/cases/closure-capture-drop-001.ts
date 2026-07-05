// RFC 20260705 closure-capture-ownership Phase 1 — non-Copy captures
// are owned by the env and released by the synthesized env-drop:
// single capture, sibling captures (each env takes its own stake),
// escaping closures, outer reassignment after capture.
function single(): number {
  const s = "owned-by-env-" + 1;
  const f = (): number => s.length;
  return f();
}
console.log(single());

// sibling captures: two envs share one str; each releases its own
// stake — the value survives until the last env drops
function siblings(): number {
  const s = "shared-capture";
  const f = (): number => s.length;
  const g = (): number => s.length;
  return f() + g();
}
console.log(siblings());

// escaping closure: env outlives the constructing frame; the capture
// stays alive until the returned closure is dropped
function make(): () => number {
  const s = "escaped-capture-payload";
  return (): number => s.length;
}
const h = make();
console.log(h());
console.log(h());

// outer reassignment after capture: the env keeps the old cell, the
// outer binding moves on to the new one
function reassign(): number {
  let s = "before-reassign";
  const f = (): number => s.length;
  const a = f();
  s = "after";
  return a + s.length;
}
console.log(reassign());

// Map capture: universal-header cell released through the
// tag-dispatched drop
function mapCap(): number {
  const m = new Map<number, number>();
  m.set(1, 10);
  m.set(2, 20);
  const f = (): number => m.size;
  return f();
}
console.log(mapCap());

// capture used across multiple invocations before drop
function counterish(): number {
  const s = "invoked-thrice";
  const f = (): number => s.length;
  return f() + f() + f();
}
console.log(counterish());
console.log("done");
