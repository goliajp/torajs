// S282 — String.{replace,replaceAll,split}(useful, useful, ...trailing)
// trailing-arg ignore per ES §22.1.3.{18,19,21}. Spec-defined 2-arg
// methods; tora helpers are 2-arg only. Same S272 family — trailing
// args must eval-then-discard, not silent-drop.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const s = "foo bar foo";

// replace
console.log(s.replace("foo", "X"));
console.log(s.replace("foo", "X", step(1)));
console.log(s.replace("foo", "X", step(2), step(3)));
console.log("calls after replace:", calls);

// replaceAll
console.log(s.replaceAll("foo", "X"));
console.log(s.replaceAll("foo", "X", step(4)));
console.log(s.replaceAll("foo", "X", step(5), step(6)));
console.log("calls after replaceAll:", calls);

// split
console.log(s.split(" ").length);
console.log(s.split(" ", 2).length);
console.log(s.split(" ", 2, step(7)).length);
console.log(s.split(" ", 2, step(8), step(9)).length);
console.log("calls after split:", calls);
