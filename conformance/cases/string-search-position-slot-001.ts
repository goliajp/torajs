// The second slot of a String search is a coerced position, and the
// six lanes that take one refused every shape but Number and
// Undefined. Its spec default is not one rule but three, and only the
// first of them is what ToNumber alone would give.

const one: any = 1;
const oneStr: any = "1";
const undef: any = undefined;
const nan: any = NaN;
const notNum: any = "zz";

// Plain ToIntegerOrInfinity — NaN and undefined both read as 0.
console.log("abcabc".indexOf("a", one));
console.log("abcabc".indexOf("a", undef));
console.log("abcabc".indexOf("a", nan));
console.log("abc".includes("c", one), "abc".includes("c", undef), "abc".includes("c", nan));
console.log("abc".startsWith("b", one), "abc".startsWith("a", undef), "abc".startsWith("a", nan));

// lastIndexOf reads a NaN position as +Infinity (§22.1.3.10 step 5-6),
// which is also how its undefined arrives. A literal NaN is a Number
// and takes the same reading.
console.log("abcabc".lastIndexOf("a", one));
console.log("abcabc".lastIndexOf("a", undef));
console.log("abcabc".lastIndexOf("a", nan));
console.log("abcabc".lastIndexOf("a", notNum));
console.log("abcabc".lastIndexOf("a", NaN), "abcabc".lastIndexOf("a", "zz"));
console.log("abcabc".lastIndexOf("a", 1), "abcabc".lastIndexOf("a"));

// endsWith and split test the slot for `undefined` ITSELF, so NaN and
// undefined part ways.
console.log("abc".endsWith("c", undef), "abc".endsWith("c", nan), "abc".endsWith("a", one));
console.log("abc".endsWith("c", "3"), "abc".endsWith("c", NaN), "abc".endsWith("c", undefined));
console.log(JSON.stringify("a,b,c".split(",", one)));
console.log(JSON.stringify("a,b,c".split(",", undef)));
console.log(JSON.stringify("a,b,c".split(",", nan)));
console.log(JSON.stringify("a,b,c".split(",", oneStr)));
console.log(JSON.stringify("a,b,c".split(",", "2")), JSON.stringify("a,b,c".split(",", NaN)));

// Statically-shaped non-Number operands take the same coercion.
console.log("abcabc".indexOf("a", "1"));
console.log("abc".includes("c", "1"), "abc".startsWith("b", "1"));
console.log("abc".includes("c", true), "abc".indexOf("c", null));

// The trailing-arg spellings carried their own copy of the gate.
console.log("abcabc".indexOf("a", one, 0));
console.log("abc".endsWith("c", undef, 0));
console.log("abcabc".lastIndexOf("a", undef, 0));

// Typed spellings and the omitted-argument defaults are unchanged.
console.log("abcabc".indexOf("a", 1), "abc".startsWith("b", 1), "abc".endsWith("c", 3));
console.log(JSON.stringify("a,b,c".split(",", 2)), JSON.stringify("a,b,c".split(",")));
console.log("abc".indexOf("a", undefined), "abc".lastIndexOf("a", undefined), "abc".endsWith("c", undefined));
console.log("abc".slice(0, 2), "abc".substring(0, 2), "abc".substr(0, 2), "ab".repeat(2), "abc".at(1));

// §22.1.3.23 step 2 dispatches a user @@split BEFORE step 3 coerces
// the limit, and passes it raw — so the limit's coercion has to stand
// down when the separator may carry one, or a `valueOf` the spec
// never runs fires and the splitter sees a number where it was given
// an object.
const probed: string[] = [];
const rawLimit: any = {
  valueOf: function (): number {
    probed.push("valueOf");
    return 2;
  },
};
const splitter: any = {};
splitter[Symbol.split] = function (_s: any, l: any) {
  return l === rawLimit;
};
console.log("abc".split(splitter, rawLimit), probed.length);
