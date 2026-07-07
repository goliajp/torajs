// RFC 20260707-undefined-sentinel-repr chunk 2 — a missed exec/match
// capture slot holds the immortal undefined sentinel cell instead of
// NULL, so print / strict eq / loose eq / truthiness / JSON all tell
// JS undefined apart from JS null. Nullable<Str> real-null behavior
// is unchanged (NULL now unambiguously means null in a Str slot).

const m = /a(b)?/.exec("a");
if (m !== null) {
  console.log(m[1]);
  console.log(m[1] === undefined);
  console.log(m[1] === null);
  console.log(m[1] !== undefined);
  console.log(m[1] == null);
  console.log(m[1] == undefined);
  console.log(m[1] === "undefined");
  const c = m[1];
  console.log(c === undefined);
  console.log(c === c);
  console.log(`t=${m[1]}`);
  console.log("x" + m[1]);
  console.log(JSON.stringify(m));
  if (m[1]) {
    console.log("truthy");
  } else {
    console.log("falsy");
  }
  try {
    console.log(m[1].length);
  } catch (e) {
    console.log("len-throws", e instanceof TypeError);
  }
}

// array-literal undefined Str slot agrees with the sentinel
const expected = ["a", undefined];
if (m !== null) {
  console.log(m[1] === expected[1]);
  console.log(expected[1] === undefined);
  console.log(expected[1] === null);
  console.log(JSON.stringify(expected));
}

// hit path unchanged through every flipped shape
const h = /a(b)/.exec("ab");
if (h !== null) {
  console.log(h[1]);
  console.log(h[1] === undefined, h[1] === null, h[1] == null);
  console.log(h[1] === "b");
  if (h[1]) {
    console.log("hit-truthy");
  }
  console.log(JSON.stringify(h));
}

// real-null Nullable<Str> face unchanged
let s: string | null = null;
console.log(s);
console.log(s === null);
console.log(s == null);
let t: string | null = "ok";
console.log(t, t === null);
console.log("done");
