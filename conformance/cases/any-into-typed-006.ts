// A typed rest parameter reached through `any` binds each argument as
// passed: `null`, a number behind `string[]`, an object or a string
// behind `number[]` reach the body as themselves, not as their
// coercion to the element type.
class Rest {
  n: number = 0;
  kinds(...xs: string[]): string {
    let out = "";
    for (const x of xs) out += typeof x + ",";
    return out;
  }
  joined(...xs: string[]): string {
    return xs.join("-");
  }
  sum(...xs: number[]): number {
    let t = 0;
    for (const x of xs) t += x;
    return t;
  }
  firstLen(...xs: string[]): number {
    return xs[0].length;
  }
}

const r: any = new Rest();
console.log(r.kinds("a", "b"), r.joined("a", "b"), r.sum(1, 2, 3), r.sum());
console.log(r.kinds("a", null), r.joined("a", null, undefined), r.kinds(1, {}));
console.log(r.sum(1, "2"), r.sum(1, null), r.sum(1.5, 2.5), r.sum(true, 1));
try {
  console.log(r.firstLen(null));
} catch (e) {
  console.log("firstLen", e instanceof TypeError);
}
console.log(r.firstLen("abc"));
