// RFC 20260713-generator-fn-value-substrate blade 6 — an any-receiver
// member/index READ on null/undefined throws TypeError catchably
// across fn boundaries. Pre-fix ast_throw_info only marked method
// CALLS (chunk 701): a named fn whose only throw source was `p.x`
// (p: any) was M4.3.b-skipped at the caller, the pending TypeError
// silently dropped, the read answered garbage NaN-box bits, and a
// downstream drop walk could SIGSEGV (exit 139).

function readX(p: any): any {
  return p.x;
}
try {
  readX(null);
  console.log("no throw");
} catch (e) {
  console.log("member null:", (e as Error).name);   // member null: TypeError
}
try {
  readX(undefined);
  console.log("no throw");
} catch (e) {
  console.log("member undef:", (e as Error).name);  // member undef: TypeError
}
console.log(readX({ x: 5 }));                        // 5

function readIdx(p: any): any {
  return p[0];
}
try {
  readIdx(null);
  console.log("no throw");
} catch (e) {
  console.log("index null:", (e as Error).name);    // index null: TypeError
}
console.log(readIdx([9]));                           // 9

// Local any binding inside a fn (same M4.3.b face).
function localRead(): any {
  const obj: any = null;
  return obj.x;
}
try {
  localRead();
  console.log("no throw");
} catch (e) {
  console.log("local null:", (e as Error).name);    // local null: TypeError
}

// Generator param destructure of null throws at the factory call and
// the temp result drops cleanly (pre-fix this SIGSEGV'd).
const g = function* ({ x, y }: any) {
  yield x;
  yield y;
};
try {
  g(null);
  console.log("no throw");
} catch (e) {
  console.log("destr null:", (e as Error).name);    // destr null: TypeError
}
const git = g({ x: 1, y: 2 });
console.log(git.next().value, git.next().value);     // 1 2
