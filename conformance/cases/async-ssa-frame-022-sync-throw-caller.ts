// An async function whose body never awaits rejects its result where
// the body raises, and the call returns normally: the caller sees no
// throw of its own and goes on, even when the body runs inline in it
// (§27.7.5.2 AsyncBlockStart step 3.g, §27.7.5.1 step 4.e).
function h(k: number): number {
  if (k > 2) throw new Error("h" + k);
  return k > 0 ? h(k + 1) : k;
}
async function g(k: number): Promise<number> {
  return h(k) + 1;
}
function run(): number {
  g(1).catch((e: any) => console.log("caught " + e.message));
  const v = h(0);
  console.log("after h " + v);
  return v;
}
run();

// the same, with the async body small enough to be inlined into its caller
async function small(k: number): Promise<number> {
  if (k > 1) throw new Error("small " + k);
  return k;
}
function loop(): number {
  let s = 0;
  for (let i = 0; i < 4; i++) {
    small(i).then(
      (v) => console.log("small ok " + v),
      (e: any) => console.log("small caught " + e.message),
    );
    s += h(0) + i;
  }
  console.log("loop " + s);
  return s;
}
loop();

// a caller with its own try: the rejection is no throw to it
function guarded(): string {
  try {
    small(5).catch((e: any) => console.log("guarded caught " + e.message));
    const v = h(0);
    return "guarded ok " + v;
  } catch (e: any) {
    return "guarded wrongly threw " + e.message;
  }
}
console.log(guarded());

// a caller reached through a method and a closure
class C {
  go(k: number): number {
    g(k).catch((e: any) => console.log("method caught " + e.message));
    return h(0) + k;
  }
}
console.log("method " + new C().go(3));
const arrow = (k: number): number => {
  small(k).catch((e: any) => console.log("arrow caught " + e.message));
  return h(0) - k;
};
console.log("arrow " + arrow(2));
console.log("main end");
