// `await` of a typed promise whose value is a Copy type (number,
// boolean) that is still pending once every queued job has run:
// nothing left can settle it, and this await cannot suspend yet, so it
// raises instead of answering `0` / `false` — the code after the await
// never runs on a value that does not exist. Heap-typed lanes (bigint,
// string) raise the same way. A settled promise still answers its value.
//
// The raise is a transition: the specification suspends here forever
// (the module never finishes evaluating), which needs the event loop
// this runtime does not have yet. bun spins on the first pending
// await, so the expected output is pinned beside this file.
function neverNum(): Promise<number> {
  return new Promise(() => {});
}
function neverBool(): Promise<boolean> {
  return new Promise(() => {});
}
function neverBig(): Promise<bigint> {
  return new Promise(() => {});
}

const r: number = await Promise.resolve(3);
console.log("resolved", r);
let res: any;
const later: Promise<boolean> = new Promise((r) => {
  res = r;
});
queueMicrotask(() => res(true));
const t: boolean = await later;
console.log("settled by a job", t);

try {
  const n: number = await neverNum();
  console.log("number after never", n);
} catch (e) {
  console.log("number raised", e instanceof TypeError);
}
try {
  const b: boolean = await neverBool();
  console.log("boolean after never", b);
} catch (e) {
  console.log("boolean raised", e instanceof TypeError);
}
try {
  const g: bigint = await neverBig();
  console.log("bigint after never", g);
} catch (e) {
  console.log("bigint raised", e instanceof TypeError);
}
const again: number = await Promise.resolve(4);
console.log("resolved again", again);
