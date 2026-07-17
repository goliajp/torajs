// gOPD 15.2.3.3-4 constructor family — every builtin prototype owns
// a `constructor` property {writable: true, enumerable: false,
// configurable: true}, and its value has ONE identity per builtin:
// desc.value === <Ctor>.prototype.constructor.

function check(tag: string, proto: any): void {
  const desc: any = Object.getOwnPropertyDescriptor(proto, "constructor");
  const same = desc.value === proto.constructor;
  console.log(tag, same, desc.writable, desc.enumerable, desc.configurable);
}

check("Object:", Object.prototype);
check("Array:", Array.prototype);
check("String:", String.prototype);
check("Boolean:", Boolean.prototype);
check("Number:", Number.prototype);
check("Date:", Date.prototype);
check("Function:", Function.prototype);
check("Promise:", Promise.prototype);
// Error-family prototypes ride the class-synth singleton (separate
// mechanism) — their constructor face is a recorded follow-up.

// identity is stable across reads
const c1: any = (Date.prototype as any).constructor;
const c2: any = (Date.prototype as any).constructor;
console.log(c1 === c2); // true
// and distinct across builtins
console.log(c1 === (Array.prototype as any).constructor); // false
console.log("done");
