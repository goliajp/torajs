// chunk 733 — fn-typed array elements are Closure-repr: capturing
// closures pushed into an annotated empty array dispatch through the
// env-first ABI (pre-fix: FnSig-elem direct call jumped into the env
// block, SIGBUS). Includes the str for-init per-iteration capture
// shape (each closure observes its own iteration's binding).
const fns: Array<() => string> = [];
for (let s = "a"; s.length <= 3; s += "x") {
  fns.push(() => s);
}
const single: Array<() => string> = [];
const hello = "hello";
single.push(() => hello);
for (const f of fns) {
  console.log(f());
}
console.log(single[0]());
