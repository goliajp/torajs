// rotation 325 — Object.keys over an owned receiver temp (the
// `D.prototype` member-read chain). Same defect family as the gOPD
// receiver: every kernel in the keys lane reads the receiver without
// consuming it, and no release site existed. Alone, the stranded +1
// on D.prototype only leaked its class group (the census could not
// see it); with a derived class in the picture the collector judged
// the subclass half WHITE and the base half BLACK, and the white
// half's frees dropped the base class-object to zero mid-drain —
// this exact shape underflowed in class-first-class-value-001 once
// the gOPD leak that had been masking it was fixed.
class D {}
console.log(Object.keys(D.prototype).length);
class Sub extends D {}
console.log(Sub.prototype.constructor === Sub);
console.log(new Sub() instanceof D);
