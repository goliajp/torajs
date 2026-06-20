// S272 — String.{at,charAt,charCodeAt,codePointAt,repeat,normalize}
// (useful, ...trailing) widen S240 from `args.len() == 2` to `>= 2`
// per ES §22.1.3.{1,2,3,4,17,13} trailing-arg ignore. ssa_lower_str
// mirror evals-and-drops trailing exprs (was `break`, silently
// dropping side effects).

let calls = 0;
function step<T>(v: T): T {
  calls = calls + 1;
  return v;
}

const s = "hello world";

// 2-arg form (baseline S240, single trailing).
console.log(s.at(0, step("a")));
console.log(s.charAt(1, step("b")));
console.log(s.charCodeAt(2, step("c")));
console.log(s.codePointAt(3, step("d")));
console.log(s.repeat(2, step("e")));
console.log("café".normalize("NFC", step("f")));

// 3+ arg form — pre-S272 rejected.
console.log(s.at(-1, step("g"), step("h")));
console.log(s.charAt(0, step("i"), step("j"), step("k")));
console.log(s.codePointAt(5, step("l"), step("m")));
console.log(s.repeat(2, step("n"), step("o"), step("p")));
console.log("café".normalize(undefined, step("q"), step("r")));
console.log("café".normalize("NFC", step("s"), step("t"), step("u")));

// side-effect counter — all step() must eval despite silent-drop.
console.log("calls=" + calls);
