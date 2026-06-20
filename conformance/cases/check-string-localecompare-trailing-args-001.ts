// S285 — String.localeCompare(other, locales?, options?, ...trailing)
// trailing-arg ignore per ES §22.1.3.10. Spec reads only `other`;
// tora bytewise helper has no Intl-locale awareness, locales/options/
// trailing all dropped. Same S272 family — trailing args must eval-
// then-discard, not silent-drop. Widens S238 from (1..=3) to any
// positive arity.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

console.log("a".localeCompare("b"));
console.log("a".localeCompare("b", step("en")));
console.log("calls after 2-arg:", calls);

console.log("a".localeCompare("b", step("en"), step(99)));
console.log("calls after 3-arg:", calls);

console.log("a".localeCompare("b", step("en"), step(99), step(100)));
console.log("calls after 4-arg:", calls);

console.log("a".localeCompare("b", step("en"), step(99), step(100), step(101)));
console.log("calls after 5-arg:", calls);
