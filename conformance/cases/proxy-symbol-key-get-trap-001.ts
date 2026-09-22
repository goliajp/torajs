// §10.5.1 [[Get]] on a proxy runs the `get` trap whatever the key
// kind is — including the symbol reads the RUNTIME makes on its own:
// ToPrimitive asking for `@@toPrimitive` (§7.1.1) and
// Object.prototype.toString asking for `@@toStringTag` (§20.1.3.6).
// Those two used to walk past the handler straight into the target's
// own dict, so `String(proxy)` showed the handler one key where the
// spec shows three, and a program had no way to intercept either.
// A read the program writes (`p[Symbol.x]`) always trapped.

let seen: string[] = [];
const plain: any = new Proxy({} as any, {
  get(t: any, k: any) {
    seen.push(String(k));
    return (t as any)[k];
  },
});

console.log("string", String(plain));
console.log("string-keys", seen.join("|"));

seen = [];
console.log("tostring", Object.prototype.toString.call(plain));
console.log("tostring-keys", seen.join("|"));

seen = [];
console.log("written", plain[Symbol.toPrimitive]);
console.log("written-keys", seen.join("|"));

// The trap's ANSWER is used, not merely observed.
const tagged: any = new Proxy({} as any, {
  get(_t: any, k: any) {
    if (k === Symbol.toStringTag) return "Custom";
    if (k === Symbol.toPrimitive) return () => "PRIM";
    return undefined;
  },
});

console.log("tag", Object.prototype.toString.call(tagged));
console.log("prim", String(tagged));
console.log("template", `${tagged}`);
console.log("concat", tagged + "");

// A handler that answers undefined for @@toPrimitive falls through to
// the ordinary toString / valueOf walk, which the trap also sees.
seen = [];
const fallthrough: any = new Proxy({} as any, {
  get(_t: any, k: any) {
    seen.push(String(k));
    if (k === "toString") return () => "FALL";
    return undefined;
  },
});
console.log("fall", String(fallthrough));
console.log("fall-keys", seen.join("|"));
