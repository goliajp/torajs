// The inherited try / withResolvers statics answer C INSTANCES —
// the capability constructs through the subclass ctor chain
// (rotation 451 knife 5).
class CP extends Promise<any> {}
const wr = (CP as any).withResolvers();
console.log(wr.promise instanceof CP);
wr.promise.then((v: any) => console.log("wr", v));
wr.resolve(3);
const tp = (CP as any).try(() => 9);
console.log(tp instanceof CP);
tp.then((v: any) => console.log("try", v));
