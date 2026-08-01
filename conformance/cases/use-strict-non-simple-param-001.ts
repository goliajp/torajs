// §15.1.1 early error (rest-param-strict-body knife): "use strict"
// in a body may not coexist with a NON-simple parameter list. This
// fixture guards the LEGAL side — every shape here must keep parsing
// (the reject must not overreach). The illegal side (SyntaxError)
// cannot live in a conformance fixture (runner requires exit 0); it
// is covered by test262 rest-param-strict-body / use-strict-with-
// non-simple-param families.

// simple params + directive — legal everywhere
function s1(a: number) {
  "use strict";
  return a + 1;
}
console.log(s1(1));

// non-simple params, NO directive — legal
function n1(a: number = 5, ...rest: number[]) {
  return a + rest.length;
}
console.log(n1(1, 2, 3));

// non-simple params, "use strict" AFTER the directive prologue ends
// (a non-string statement closes the prologue; the string below is a
// plain expression statement, not a directive) — legal
function n2(a: number = 2) {
  let x = a * 2;
  "use strict";
  return x;
}
console.log(n2(3));

// outer non-simple params + NESTED simple-param fn with the
// directive — ContainsUseStrict does not look into nested bodies
function n3(...xs: number[]) {
  function inner() {
    "use strict";
    return 7;
  }
  return inner() + xs.length;
}
console.log(n3(1, 2));

// some other directive-looking string in a non-simple prologue — only
// "use strict" itself is refused
function n4(a: number = 1) {
  "use asm";
  return a;
}
console.log(n4(9));

// method shorthand with default param, directive after prologue — legal
const o = {
  m(k: number = 4) {
    let v = k + 1;
    "use strict";
    return v;
  },
};
console.log(o.m());

// class method with rest param, no directive — legal
class C {
  sum(...vals: number[]) {
    let t = 0;
    for (const v of vals) t += v;
    return t;
  }
}
console.log(new C().sum(1, 2, 3));
