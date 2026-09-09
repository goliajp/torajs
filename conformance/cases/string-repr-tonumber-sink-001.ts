// The ToNumber sink in lowering is chosen by the operand's REPR, not
// by its static type. `s as any` leaves a Str repr under an Any
// annotation, so every Math.* argument coercion reached the sink's
// panic arm — while the very same value bound as `const s: any = "5"`
// boxes at the binding and answers off the any lane.
//
// §7.1.4 ToNumber(String) is StringToNumber, and the runtime has
// carried it all along (`__torajs_str_to_number`); the sink was the
// one place in the family that had no arm for it.

const s: string = "5";
const t: any = "5";

console.log(Math.max(s as any, 1), Math.min(s as any, 9));
console.log(Math.abs(s as any), Math.round(s as any), Math.floor(s as any));
console.log(Math.trunc(s as any), Math.sqrt(s as any), Math.ceil(s as any));

// the boxed spelling was always right — it is the control, not the fix
console.log(Math.max(t, 1), Math.abs(t), Math.round(t));

// unparseable strings answer NaN, not a refusal
console.log(Math.abs("abc" as any), Math.max("abc" as any, 1));
