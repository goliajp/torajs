// S328 — WHATWG console §1.1.2 (debug) / §1.1.4 (info). bun/node alias
// both to log (stdout). Pre-S328 check.rs rejected `console.info(...)`
// and `console.debug(...)` with "no member `.info` on type
// Object('console')" because the sig table only listed log/error/warn.
// ssa_lower's console_method_member mirrored the same omission.

let s = 0;
const step = (v: number): number => {
  s++;
  return v;
};

// Both methods accept variadic args (same shape as log)
console.info("info1");
console.info("a=", 1, "b=", 2);
console.debug("debug1");
console.debug("x=", true);

// step()-counter probe: trailing args evaluate
console.info("v=", step(7));
console.debug("v=", step(11));

console.log("steps=", s);
