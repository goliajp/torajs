// ES2026 json-parse-with-source — JSON.rawJSON (§25.5.1) mints a
// frozen null-prototype [[IsRawJSON]] carrier whose text splices
// verbatim into JSON.stringify output; JSON.isRawJSON (§25.5.3)
// probes for the slot. Error messages are engine-specific, so the
// throw probes print only the constructor name.

// ---- stringify splices the validated text verbatim ----
console.log(JSON.stringify(JSON.rawJSON(1)));
console.log(JSON.stringify(JSON.rawJSON(1.1)));
console.log(JSON.stringify(JSON.rawJSON(-1)));
console.log(JSON.stringify(JSON.rawJSON(1.1e1)));
console.log(JSON.stringify(JSON.rawJSON(1.1e-1)));
console.log(JSON.stringify(JSON.rawJSON(null)));
console.log(JSON.stringify(JSON.rawJSON(true)));
console.log(JSON.stringify(JSON.rawJSON(false)));
console.log(JSON.stringify(JSON.rawJSON('"foo"')));
console.log(JSON.stringify(JSON.rawJSON("9007199254740993")));

// ---- nested in objects / arrays ----
console.log(JSON.stringify({ x: JSON.rawJSON(1), y: JSON.rawJSON(2) }));
console.log(JSON.stringify({ x: { y: JSON.rawJSON(37) } }));
console.log(JSON.stringify([JSON.rawJSON(1), JSON.rawJSON(1.1)]));
console.log(JSON.stringify([JSON.rawJSON('"1"'), JSON.rawJSON(true), JSON.rawJSON(null)]));

// ---- carrier shape: frozen, null proto, single own key ----
const r = JSON.rawJSON(1);
console.log(Object.getPrototypeOf(r) === null);
console.log(Object.isFrozen(r));
console.log(r.rawJSON);
console.log(Object.getOwnPropertyNames(r).join(","));

// ---- isRawJSON ----
console.log(JSON.isRawJSON(r));
console.log(JSON.isRawJSON(JSON.rawJSON("123")));
console.log(JSON.isRawJSON({ rawJSON: "1" }));
console.log(JSON.isRawJSON(42));
console.log(JSON.isRawJSON("1"));
console.log(JSON.isRawJSON(undefined));
console.log(JSON.isRawJSON(null));
console.log(JSON.isRawJSON([]));

// ---- §25.5.1 rejection surface ----
function probe(fn: () => void): void {
  try {
    fn();
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).name);
  }
}
probe(() => JSON.rawJSON(""));          // SyntaxError (empty)
probe(() => JSON.rawJSON(" 1"));        // SyntaxError (leading space)
probe(() => JSON.rawJSON("1 "));        // SyntaxError (trailing space)
probe(() => JSON.rawJSON("\t1"));       // SyntaxError (leading tab)
probe(() => JSON.rawJSON("1\n"));       // SyntaxError (trailing LF)
probe(() => JSON.rawJSON("{}"));        // SyntaxError (object outermost)
probe(() => JSON.rawJSON("[]"));        // SyntaxError (array outermost)
probe(() => JSON.rawJSON(undefined));   // SyntaxError ("undefined" invalid)
probe(() => JSON.rawJSON({}));          // SyntaxError ("[object Object]")
probe(() => JSON.rawJSON("01"));        // SyntaxError (leading zero)
probe(() => JSON.rawJSON("1."));        // SyntaxError (bare point)
probe(() => JSON.rawJSON('"open'));     // SyntaxError (unterminated)
probe(() => JSON.rawJSON(Symbol("x"))); // TypeError (ToString(Symbol))
