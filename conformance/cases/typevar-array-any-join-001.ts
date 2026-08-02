// Structural any-absorbing typevar join (check_typevar.rs): a rest
// pattern collects Array(Any) while an assert-style generic's other
// argument is a typed array literal — `T` inferred as Array(Any)
// earlier must absorb Array(Number) instead of rejecting (t262
// dstr rest family, 149-case cluster r283).
function notSame<T>(a: T, b: T): boolean {
  return true;
}
const src = [1, 2, 3];
const [x, ...rest] = src;
console.log(notSame(rest, [2, 3]));
console.log(notSame([9] as any[], [1]));
console.log(notSame([1], [9] as any[]));
console.log(x, rest);
