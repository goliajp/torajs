const toJSON = (Date.prototype as any).toJSON;
console.log(toJSON.call({ valueOf: () => NaN }));
const d: any = new Date(0);
console.log(d.toJSON());
const bad: any = new Date(NaN);
console.log(bad.toJSON());
console.log(toJSON.call(d));
