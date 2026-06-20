// S295 — Date instance 0-arg getter / format method family
// (getTime / valueOf / toISOString / toJSON / getFullYear /
// getUTC*/getMonth/getDate/getHours/getMinutes/getSeconds/
// getMilliseconds/getDay/getTimezoneOffset/getYear/toGMTString/
// toUTCString/toDateString/toLocaleString/toLocaleDateString/
// toLocaleTimeString) trailing-arg ignore per ES §21.4.4.*.
// Spec methods either take 0 args or have optional `key`/`locales`/
// `options` args that tora's subset silently drops; check.rs S261
// already typecheck-and-drops trailing operands. ssa_lower's
// `else { vec![recv_op] }` branch now lower-and-drops them so
// step()-style side-effect exprs fire per ES eval-then-discard
// (S272 idiom).

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const d: Date = new Date(0);

console.log("getTime trail:", d.getTime(step("g1")));
console.log("calls:", calls);
console.log("getTime multi:", d.getTime(step("g2"), step(42), step(true)));
console.log("calls:", calls);
console.log("valueOf trail:", d.valueOf(step("v1")));
console.log("calls:", calls);
console.log("getFullYear trail:", d.getFullYear(step("y1")));
console.log("getUTCFullYear trail:", d.getUTCFullYear(step("y2")));
console.log("getMonth trail:", d.getMonth(step("m1")));
console.log("getDate trail:", d.getDate(step("dt1")));
console.log("getHours trail:", d.getUTCHours(step("h1")));
console.log("getMinutes trail:", d.getUTCMinutes(step("mn1")));
console.log("getSeconds trail:", d.getUTCSeconds(step("s1")));
console.log("getMilliseconds trail:", d.getUTCMilliseconds(step("ms1")));
console.log("getDay trail:", d.getUTCDay(step("dy1")));
console.log("calls:", calls);
console.log("toISOString trail:", d.toISOString(step("iso1")));
console.log("toISOString multi:", d.toISOString(step("iso2"), step(99)));
console.log("toJSON trail:", d.toJSON(step("j1")));
console.log("calls:", calls);
console.log("toUTCString trail:", d.toUTCString(step("u1"), step(true)));
console.log("toDateString trail:", d.toDateString(step("ds1")));
console.log("calls final:", calls);
