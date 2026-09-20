// §22.1.3.{7,8,23} steps 3-5 then step 7 — the needle slot of
// startsWith / endsWith / includes over a needle whose type is NOT known.
// IsRegExp runs first, then ToString. Before this the slot had no road at
// all: it fell through to the raw argv tail and handed an un-coerced
// operand to a helper whose ABI declares Str, so every one of these
// SEGV'd (exit 139) or answered a value read out of a NaN-box.

function show(label: string, f: () => boolean): void {
  try {
    console.log(label + ": " + String(f()));
  } catch (e) {
    console.log(label + ": " + String(e));
  }
}

// The ordinary case the crash made impossible: an `any` that holds a string.
const s: any = "a";
show("any-str-startsWith", () => "ab".startsWith(s));
show("any-str-endsWith", () => "ab".endsWith(s));
show("any-str-includes", () => "ab".includes(s));

// ToString reaches every value (step 7), so a number needle stringifies.
const n: any = 1;
show("any-num", () => "a1b".includes(n));
const b: any = true;
show("any-bool", () => "xtruey".includes(b));

// A user toString is what step 7 calls.
const o: any = { toString(): string { return "b"; } };
show("any-tostring", () => "ab".endsWith(o));

// Step 3-5: a regex raises before any coercion — note the toString below
// must NOT run, which is why the throw check sits ahead of the coercion.
let coerced = 0;
const re: any = /a/;
(re as any).toString = function (): string { coerced = coerced + 1; return "a"; };
show("any-regexp", () => "ab".startsWith(re));
console.log("regexp toString ran: " + String(coerced));

// IsRegExp step 2 is a plain Get of @@match, not GetMethod: a truthy
// non-callable value still makes it a regex.
const faux: any = { toString(): string { return "a"; } };
faux[Symbol.match] = true;
show("any-symbol-match", () => "ab".startsWith(faux));

// ... and a falsy @@match means NOT a regex, so the coercion runs.
const unfaux: any = { toString(): string { return "a"; } };
unfaux[Symbol.match] = false;
show("any-symbol-match-false", () => "ab".startsWith(unfaux));

// A real regex whose @@match was shadowed away is still a regex by step 4.
const shadow: any = /a/;
shadow[Symbol.match] = undefined;
show("any-regexp-shadowed", () => "ab".startsWith(shadow));

// §7.1.17 — ToString of a symbol throws, at step 7 not step 5.
const sym: any = Symbol("s");
show("any-symbol", () => "ab".startsWith(sym));

// The 2-arg spelling used to be refused outright ("arg 0 must be string,
// got Any") while the 1-arg spelling of the same call compiled.
show("any-2arg", () => "ab".startsWith(s, 0));
show("any-2arg-end", () => "ab".endsWith(s, 1));
show("any-2arg-inc", () => "ab".includes(s, 0));

// Nullish keeps answering the literal text §7.1.17 gives it.
console.log("undefined".includes(undefined as any));
console.log("null".includes(null as any));

// Controls — the typed road is untouched and still free.
console.log("ab".startsWith("a"), "ab".endsWith("b"), "ab".includes("b"));
