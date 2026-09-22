// A declared `string` slot taking an `any` is not a ToString site.
// Running §7.1.17 there spelled `undefined` as the six-letter TEXT
// "undefined", so the slot held a 9-character string: `s === undefined`
// was false and `typeof s` was "string".
//
// The Str slot has an undefined sentinel for exactly this, and every
// other boundary into one already used it — the boxed parameter
// adapter (the same kernel this now calls), a missing argument, an
// `undefined` literal. Only the let annotation ran the real ToString.
//
// One neighbour is NOT fixed and is registered instead: `null as any`
// into the same slot still answers the text "null", because the Str
// slot has an undefined sentinel and no null one. Giving it one means
// a second immortal cell plus every station that recognises the first
// (drop, strict-eq, print, the any boundary) learning about it — its
// own piece of work.

let s: string | undefined = (undefined as any);
console.log("undef", s === undefined, typeof s, JSON.stringify(s));

let ok: string | undefined = ("x" as any);
console.log("value", ok === "x", ok!.length);

// Same boundary without the union. The annotation never said the slot
// could hold undefined, but the conversion above puts the sentinel
// there anyway, so `typeof` has to ask instead of folding to "string"
// the way a plain `string` binding still does.
let bare: string = (undefined as any);
console.log("bare", bare === undefined, typeof bare);

const plain: string = "lit";
console.log("plain", typeof plain);

// Crossing back into `any` re-encodes the sentinel as undefined rather
// than re-spelling it as the text — both spellings of the crossing.
const boxed: any = s;
console.log("boxed", boxed === undefined, typeof boxed);
console.log("asany", (bare as any) === undefined, (bare as any) === "undefined");

// The real ToString is untouched — these all still spell it out.
console.log("tostring", String(undefined), String(null), `${undefined}`);
console.log("join", [undefined, null, "a"].join(","));
const any_undef: any = undefined;
console.log("concat", "x" + any_undef);

// Reading a member off the sentinel throws, as it does off undefined.
try {
  console.log("len", (s as any).length);
} catch (e) {
  console.log("len THREW", (e as Error).name);
}
