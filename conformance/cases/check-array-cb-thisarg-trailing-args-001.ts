// S270 — Array.prototype.{map,filter,every,some,forEach,find,findIndex,
// findLast,findLastIndex,flatMap}(cb, thisArg?, ...trailing) per ES
// §23.1.3.{18,16,3,28,12,9,10,8,9.5}: spec uses only cb + thisArg;
// trailing args silent-drop. tora's typed-cb dispatch in check.rs
// previously only accepted exactly `params+1` (cb + thisArg) and
// rejected 1+ extra trailing args with "expected N argument(s), got M".

let calls = 0;
function step<T>(v: T): T {
  calls = calls + 1;
  return v;
}

const xs = [1, 2, 3, 4];

// 2-arg form: cb + thisArg — already accepted (P1 wedge baseline).
console.log(xs.map((x) => x * 2, undefined));
console.log(xs.filter((x) => x > 1, null));
console.log(xs.every((x) => x > 0, undefined));
console.log(xs.some((x) => x > 3, null));
console.log(xs.find((x) => x > 2, undefined));
console.log(xs.findIndex((x) => x > 2, null));

// 3-arg form: cb + thisArg + trailing — fail pre-S270.
console.log(xs.map((x) => x + 1, {}, step("a")));
console.log(xs.filter((x) => x > 2, null, step("b")));
console.log(xs.every((x) => x > 0, null, step("c")));
console.log(xs.some((x) => x > 3, null, step("d")));
console.log(xs.find((x) => x > 1, undefined, step("e")));
console.log(xs.findIndex((x) => x === 3, null, step("f")));

xs.forEach((x) => {
  // noop
}, null, step("g"));
console.log("forEach OK");

// 4+ arg form: cb + thisArg + multi trailing — fail pre-S270.
console.log(xs.map((x) => x, {}, step("h"), step("i"), step("j")));
console.log(xs.filter((x) => x > 0, null, step("k"), step("l")));

// side-effect counter — every trailing expr must eval-and-drop.
console.log("calls=" + calls);
