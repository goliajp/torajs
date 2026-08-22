// JSON output is built in one byte buffer that several producers
// write into. A UTF-16 Str decodes to multi-byte UTF-8 there, which
// makes the whole buffer UTF-8 — so a Latin-1 payload copied in raw
// leaves a lead byte that eats whatever follows it. Every case below
// puts a Latin-1 code point next to a UTF-16 one.
function show(v: any) {
  console.log(JSON.stringify(v));
}

const acute = "\u00e9";
const upper = "\u00c9";
const wide = "\u4e2d";

show([upper, wide]);
show([acute, wide]);
show([wide, acute]);
show([acute, wide, "z"]);
show({ [acute]: wide });
show({ k: acute, w: wide });
show([acute.toUpperCase(), wide.toUpperCase()]);
show([(acute + wide).slice(0, 1), (acute + wide).slice(1)]);
show([[acute], [wide]]);
show({ a: { b: acute }, c: [wide] });

// A Latin-1 payload with no wide neighbour still round-trips, and
// escapes next to one keep working.
show([acute]);
show([acute + '"' + wide]);
show(["\n" + acute, wide]);
console.log(JSON.stringify([upper, wide]).length);
