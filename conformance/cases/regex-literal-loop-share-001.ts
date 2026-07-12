// Regex-literal LICM singleton ownership — a fn-scope hoisted
// compile (ssa_lower_lit::lower_regex) is SHARED by every taking
// consumer: let/const bindings, object fields, array slots, and
// ident assignment each take +1. Pre-fix, the taker's scope-drop
// stole the fn's only stake and every later occurrence of the same
// literal ran use-after-free (loop iterations 2+ answered false).

function viaLet(): number {
  let n = 0;
  for (let i = 0; i < 4; i++) {
    const a = /[0-9]/;
    if (a.test("7")) n++;
  }
  return n;
}
function viaField(): number {
  let n = 0;
  for (let i = 0; i < 4; i++) {
    const o = { r: /[2-9]/ };
    if (o.r.test("7")) n++;
  }
  return n;
}
function viaArr(): number {
  let n = 0;
  for (let i = 0; i < 4; i++) {
    const a = [/[3-9]/];
    if (a[0].test("7")) n++;
  }
  return n;
}
function viaAssign(): number {
  let v: RegExp = /x/;
  let n = 0;
  for (let i = 0; i < 4; i++) {
    v = /[4-9]/;
    if (v.test("7")) n++;
  }
  return n;
}
function viaNewLiteral(): number {
  // `new RegExp("literal")` desugars to the same hoisted shape.
  let n = 0;
  for (let i = 0; i < 4; i++) {
    const a = new RegExp("[0-9]");
    if (a.test("7")) n++;
  }
  return n;
}
function viaMemberAssign(): number {
  const o = { r: /x/ };
  let n = 0;
  for (let i = 0; i < 4; i++) {
    o.r = /[5-9]/;
    if (o.r.test("7")) n++;
  }
  return n;
}
console.log(viaLet(), viaField(), viaArr(), viaAssign(), viaNewLiteral(), viaMemberAssign());
// expression-position and pass-through shapes stay correct
function viaExpr(): number {
  let n = 0;
  for (let i = 0; i < 4; i++) {
    if (/[6-9]/.test("7")) n++;
  }
  return n;
}
function take(r: RegExp): boolean {
  return r.test("7");
}
function viaArg(): number {
  let n = 0;
  for (let i = 0; i < 4; i++) {
    if (take(/[7-9]/)) n++;
  }
  return n;
}
console.log(viaExpr(), viaArg());
