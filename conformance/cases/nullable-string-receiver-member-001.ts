// A `string | undefined` receiver reaches the member lane, and an
// absent one raises the TypeError instead of answering "UNDEFINED".
//
// A str slot spells its absent value as NULL or as an immortal str
// block whose text is "undefined" — not the generic heap cell — so the
// guard that covers every other pointer shape cannot see it. The read
// side had its own kernel (`__torajs_str_null_check`) for `.length`;
// the method-call side had none, which is why the checker held String
// back when the rest of the pointer shapes were let through. Opening
// the checker without the call guard turns `s.toUpperCase()` into
// "UNDEFINED": a refusal replaced by a wrong answer.
//
// §13.3.4 builds the Reference Record for any base; §7.1.20 ToObject
// raises when it is absent, before the arguments (§13.3.6.1).
//
// Written out directly, never through a helper (r687).

function maybe(): string | undefined {
  return Math.random() < 2 ? "ab" : undefined;
}
function none(): string | undefined {
  return undefined;
}
function side(): string {
  console.log("SIDE");
  return "z";
}

function main(): void {
  const s: string | undefined = maybe();
  console.log(s.length, s.toUpperCase(), s.trim().length);
  console.log(s.charCodeAt(0), s.indexOf("b"), s.replace("a", "z"));
  console.log(s.split("").length, s.padStart(4, "-"), s.at(0));

  const gone: string | undefined = none();
  try {
    console.log(gone.length);
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  try {
    console.log(gone.toUpperCase());
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  try {
    console.log(gone.concat(side()));
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
}

main();
