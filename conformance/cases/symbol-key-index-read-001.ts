// `o[sym]` reads the symbol-keyed slot — the read half of the §6.1.7
// property-key domain.
//
// The previous chunk gave dynobj a real symbol-key namespace, but the
// only way to reach it was `Object.defineProperty` /
// `getOwnPropertyDescriptor`: the index expression itself was a loud
// typecheck reject (`index must be number, got Symbol`), because
// `check_type_of_index` admitted only Number for every receiver and
// String for the `any` / struct pair.
//
// The runtime side does NOT thread symbol keys through the string
// cascade. Each receiver arm there is a three-step shape — entry-table
// probe, then name-keyed fallbacks (`key_is(key, "prototype")`, the
// virtual `name` / `length` pair, canonical-index decode,
// builtin-prototype method reify), then the prototype chain. Only the
// first and last steps mean anything for a symbol key, and the middle
// ones would read Str payload offsets off a 16-byte Symbol cell. So a
// symbol key gets its own short walk: own dict, then what the receiver
// inherits. §10.1.8.1 OrdinaryGet does not care which key kind it is
// looking up, which is why the chain walk is shared.

const s = Symbol("k");
const o: any = {};
Object.defineProperty(o, s, {
  value: 42,
  enumerable: true,
  writable: true,
  configurable: true,
});
console.log("read", o[s]);
// a different symbol with the SAME description is a different key
console.log("miss", o[Symbol("k")]);
console.log("wk-absent", o[Symbol.replace]);
Object.defineProperty(o, Symbol.replace, { value: "R" });
console.log("wk", o[Symbol.replace]);

// §10.1.8.1 — inherited through an explicit [[Prototype]], and an own
// entry shadows it
const proto: any = {};
Object.defineProperty(proto, s, { value: "inherited" });
const child: any = Object.create(proto);
console.log("inherit", child[s]);
Object.defineProperty(child, s, { value: "own" });
console.log("shadow", child[s]);
// grandparent — the walk recurses
const grand: any = Object.create(child);
console.log("grandparent", grand[s]);
// a null-prototype dict inherits nothing
const bare: any = Object.create(null);
console.log("null-proto", bare[s]);

// an entry STORING undefined is present, not absent
const u: any = {};
Object.defineProperty(u, s, { value: undefined, enumerable: true });
console.log("stored-undef", u[s], Object.getOwnPropertySymbols(u).length);

// receivers that keep their dict in an in-layout slot — reading a
// symbol key must not disturb the virtual name/length pair or the
// element domain
const fn: any = function g() {};
Object.defineProperty(fn, s, { value: 7 });
console.log("fn", fn[s], fn.name, fn.length);
const arr: any = [1, 2];
Object.defineProperty(arr, s, { value: 9 });
console.log("arr", arr[s], arr[0], arr.length, JSON.stringify(arr));

// string keys keep answering exactly as before, on the same object
const mix: any = { x: 1, length: 5 };
Object.defineProperty(mix, s, { value: 2 });
console.log("mix", mix["x"], mix.length, mix[s], mix["nope"]);
console.log("mix-proto", typeof mix["toString"], mix.toString());

// the key can arrive through any expression whose static type is
// `symbol` (an `any`-typed key stays a loud reject — that needs a
// runtime ToPropertyKey split across number / string / symbol, which
// is its own gap, recorded in the plan)
const alias = s;
console.log("via-alias", o[alias]);
const list = [s];
console.log("via-elem", o[list[0]]);
function pick(k: symbol): any {
  return o[k];
}
console.log("via-param", pick(s));
