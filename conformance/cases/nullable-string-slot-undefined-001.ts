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
// Three neighbours are NOT fixed here and are registered instead:
//   - `null as any` into the same slot still answers the text "null";
//     the Str slot has no null sentinel, only an undefined one.
//   - `typeof` on a non-nullable `string` slot folds to "string" at
//     compile time, so `let bare: string = (undefined as any)` reports
//     "string" where bun reports "undefined" (the value compares equal
//     to undefined either way, which is what this file pins).
//   - boxing the sentinel back into an `any` re-spells it as the text,
//     so `(bare as any) === "undefined"` is true here and false in bun.

let s: string | undefined = (undefined as any);
console.log("undef", s === undefined, typeof s, JSON.stringify(s));

let ok: string | undefined = ("x" as any);
console.log("value", ok === "x", ok!.length);

// Same boundary without the union: the value still compares equal to
// undefined rather than to the text.
let bare: string = (undefined as any);
console.log("bare", bare === undefined);

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
