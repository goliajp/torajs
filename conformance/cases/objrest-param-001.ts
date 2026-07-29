// ES §14.3.3.1 — an object binding pattern in a PARAMETER position may
// end in a rest element, exactly like one in a `let` / `const` head.
// The two pattern readers are separate code paths (the `PatShape` one
// for declarations, the synth-name walker for params), and only the
// declaration side had ever learned `...rest`; the param walker
// rejected the token outright. Both now emit through one shared
// `__spread_omit__` sentinel, so the omit set is computed the same way
// on both sides.

function tail({ a, ...rest }) {
  return a + " " + JSON.stringify(rest);
}
console.log(tail({ a: 1, b: 2, c: 3 }));

// no named fields at all — the omit set is empty, `all` is a copy
function all({ ...everything }) {
  return JSON.stringify(everything);
}
console.log(all({ x: 1, y: 2 }));

// a renamed field still contributes its SOURCE key to the omit set,
// not the binding name
function renamed({ p: q, ...others }) {
  return q + " " + JSON.stringify(others);
}
console.log(renamed({ p: 5, s: 6, t: 7 }));

// every key named — the rest object is empty, not undefined
function nothingLeft({ a, b, ...rest }) {
  return JSON.stringify(rest);
}
console.log(nothingLeft({ a: 1, b: 2 }));

// the pattern still binds its named fields normally alongside the rest
function both({ a, b, ...rest }) {
  return a + b + " " + JSON.stringify(rest);
}
console.log(both({ a: 10, b: 20, c: 30, d: 40 }));
