// String wrapper `length` through DYNAMIC keys (§10.4.3
// StringGetOwnProperty). The literal-key form pre-lowers to the
// static length read and was fine; the dynamic form (`obj[k]`,
// test262 propertyHelper's shape) fell to the expando probe —
// reads answered undefined and a store landed in the expando,
// shadowing length as a writable property.

function readKey(o: any, k: string): any {
  return o[k];
}
function writeKey(o: any, k: string, v: any): string {
  try {
    o[k] = v;
    return "stored";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

const s: any = new String("ab");
console.log(readKey(s, "length")); // 2
const empty: any = new String("");
console.log(readKey(empty, "length")); // 0

// non-writable: strict store throws, no expando shadow
console.log(writeKey(s, "length", "unlikelyValue")); // TypeError
console.log(readKey(s, "length")); // 2
console.log(s.length); // 2 (static read agrees)

// in-range code-unit index store refuses too
console.log(writeKey(s, "0", "z")); // TypeError
console.log(s[0]); // a

// out-of-range index stays an ordinary expando key
console.log(writeKey(s, "5", "x")); // stored
console.log(readKey(s, "5")); // x

// plain expando keys keep working
console.log(writeKey(s, "foo", 7)); // stored
console.log(readKey(s, "foo")); // 7

// Number/Boolean wrappers have no length face
const n: any = new Number(3);
console.log(readKey(n, "length")); // undefined
console.log(writeKey(n, "length", 9)); // stored (ordinary expando)
console.log(readKey(n, "length")); // 9
console.log("done");
