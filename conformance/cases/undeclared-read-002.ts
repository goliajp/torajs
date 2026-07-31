// RFC 20260730-undeclared-ident 刀 3 — undeclared reads inside
// closures and as call callees (§6.2.5.5). Covers:
//   - IIFE body read (the assert.throws(ReferenceError, function(){x;})
//     canonical shape's engine half)
//   - bare call of an undeclared name (GetValue precedes the call)
//   - mixed captures: declared name still captured, undeclared one
//     pruned from the env and thrown at the read
//   - arrow-fn body read
try {
  (function () {
    // @ts-ignore
    qq;
  })();
} catch (e) {
  console.log("closure:", (e as Error).message);
}
try {
  // @ts-ignore
  ff();
} catch (e) {
  console.log("call:", (e as Error).message);
}
let real = 7;
try {
  (function () {
    console.log("real:", real);
    // @ts-ignore
    gone;
  })();
} catch (e) {
  console.log("mixed:", (e as Error).message);
}
try {
  const a = () => {
    // @ts-ignore
    return zz2 + 1;
  };
  a();
} catch (e) {
  console.log("arrow:", (e as Error).message);
}
console.log("end2");
