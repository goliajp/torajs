// The builtin prototypes the spec aliases @@iterator to a named
// method of theirs carry it as a REAL own property whose value is
// that very function object -- not an equivalent one.
const I: any = Symbol.iterator;

function d(label: string, o: any): void {
  const x: any = Object.getOwnPropertyDescriptor(o, I);
  console.log(label + " " + (x === undefined ? "MISSING" : typeof x.value + " w=" + x.writable + " e=" + x.enumerable + " c=" + x.configurable));
}

d("Map.p", Map.prototype);
d("Set.p", Set.prototype);
d("String.p", String.prototype);

// §24.1.3.14 / §24.2.3.13 / §24.2.4.8 -- the identity, not just the
// shape: the symbol slot IS entries / values, and Set's keys is the
// same object again.
const MP: any = Map.prototype;
const SP: any = Set.prototype;
const StrP: any = String.prototype;
console.log(MP[I] === MP.entries);
console.log(SP[I] === SP.values);
console.log(SP[I] === SP.keys);
console.log(MP[I] === SP[I]);

// an instance reads the same function object off its own face
console.log(StrP[I] === "a"[I]);

// own symbol keys list in creation order -- @@iterator's clause runs
// before @@toStringTag's
console.log(Object.getOwnPropertySymbols(MP).map((s: any) => String(s)).join(","));
console.log(Object.getOwnPropertySymbols(SP).map((s: any) => String(s)).join(","));
console.log(Object.getOwnPropertySymbols(StrP).map((s: any) => String(s)).join(","));

// the function still iterates when called through the symbol slot
const m = new Map([[1, 2]]);
const s = new Set(["a"]);
const it: any = MP[I].call(m);
console.log(JSON.stringify(it.next().value));
const it2: any = SP[I].call(s);
console.log(it2.next().value);

// and the ordinary iteration faces are untouched
let acc = "";
for (const [k, v] of m) acc += k + "=" + v + ";";
for (const v of s) acc += v + ";";
for (const c of "hi") acc += c;
console.log(acc);
console.log([...m].length, [...s].length, [..."hi"].length);
console.log(Array.from(m).length, Array.from(s).length);
