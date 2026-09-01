// rotation 550 — an owned temp held across a sibling's throw edge is
// released on the throw path in the binop / template / any-method /
// any-call / variadic / optcall / dynamic-new lanes (churn probes:
// 21-40MB → 2MB per 600k caught throws). The normal path of every
// shape still answers its value (no double release).
const mk = (n: number): any => ({ n });
const s = (n: number): string => "v" + n;
const boom = (): any => {
  throw new Error("x");
};
let caught = 0;
const N = 200;

// 1. binop concat — a fresh Str / fresh any left operand across a
//    throwing right operand
for (let i = 0; i < N; i++) {
  try {
    const r = s(i) + boom();
    console.log(r);
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    const r = mk(i) + boom();
    console.log(r);
  } catch (e) {
    caught++;
  }
}
console.log(s(1) + s(2), mk(3) + "!", String(mk(4)) + String(s(5)));

// 2. template literal — the String(sub) concat chain
for (let i = 0; i < N; i++) {
  try {
    const r = `a${s(i)}b${boom()}c`;
    console.log(r);
  } catch (e) {
    caught++;
  }
}
console.log(`a${s(6)}b${mk(7)}c`);

// 3. any-method receiver across a throwing argument (named + index)
for (let i = 0; i < N; i++) {
  try {
    mk(i).toString(boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    mk(i)["toString"](boom());
  } catch (e) {
    caught++;
  }
}
console.log(mk(8).toString(), mk(9)["toString"]());

// 4. variadic binding — an earlier boxed arg across a later throw
const va = (...xs: any[]): number => xs.length;
for (let i = 0; i < N; i++) {
  try {
    va(mk(i), boom());
  } catch (e) {
    caught++;
  }
}
console.log(va(mk(10), 1, "z"));

// 5. optcall method / bare any-call callee / new on a dynamic ctor
for (let i = 0; i < N; i++) {
  try {
    mk(i).toString?.(boom());
  } catch (e) {
    caught++;
  }
}
const curried = (a: number): any => (b: any) => a + b;
for (let i = 0; i < N; i++) {
  try {
    curried(i)(boom());
  } catch (e) {
    caught++;
  }
}
class C {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
}
const K: any = C;
for (let i = 0; i < N; i++) {
  try {
    new K(mk(i), boom());
  } catch (e) {
    caught++;
  }
}
console.log(mk(11).toString?.(), curried(1)(2), new K(3).v);

console.log(caught);
