// Any-lane Substr-view concat (test262 charAt S15.5.4.4_A1_T2 root):
// charAt through the any tier answers a Substr VIEW; the concat
// kernels read the plain Str layout only, so the view's parent
// pointer bytes leaked as payload (" @") and the three-way concat
// walked off the block (SIGSEGV). ToString now flattens the view.

const s: any = "false";
const a: any = s.charAt(0);
const b: any = s.charAt(1);
console.log(a + b); // fa
console.log(a + b + s.charAt(2)); // fal
console.log(a + "!"); // f!
console.log("!" + a); // !f

// the original test262 shape — Boolean wrapper receiver + reified
// builtin + boolean args
const inst: any = new Boolean(false);
inst.charAt = (String.prototype as any).charAt;
console.log(inst.charAt(false) + inst.charAt(true) + inst.charAt(2)); // fal

// longer than the ShortStr cap (heap-concat path)
const t: any = "abcdefgh";
console.log(t.charAt(0) + t.charAt(1) + t.charAt(2) + t.charAt(3) + t.charAt(4) + t.charAt(5)); // abcdef

// substr view from slice-family methods concats the same way
const v: any = s.substring(0, 3);
console.log(v + "-" + s.charAt(4)); // fal-e
console.log("done");
