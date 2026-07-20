// Descriptor-family statics reified (RFC 20260721-object-descriptor-
// cluster 刀 4) — Object.{getOwnPropertyDescriptor,
// getOwnPropertyDescriptors, create, defineProperty, defineProperties}
// enter the ns-static table for the reflection surface (typeof / name /
// length / gOPD identity / hasOwnProperty); gOPD also settles as a
// detached call through the meta descriptor kernel. The define trio's
// detached-call face stays a recorded loud reject (dynobj-slot
// writeback), not covered here — direct calls keep their typed lowering.
const g: any = Object.getOwnPropertyDescriptor;
console.log(typeof g, g.name, g.length);
const gd: any = g({ a: 1 }, "a");
console.log(gd.value, gd.writable, gd.enumerable, gd.configurable);
console.log(g({ a: 1 }, "nosuch"));

const gs: any = Object.getOwnPropertyDescriptors;
console.log(typeof gs, gs.name, gs.length);

const c: any = Object.create;
console.log(typeof c, c.name, c.length);
const dp: any = Object.defineProperty;
console.log(typeof dp, dp.name, dp.length);
const dps: any = Object.defineProperties;
console.log(typeof dps, dps.name, dps.length);

const d1: any = Object.getOwnPropertyDescriptor(Object, "create");
console.log(d1.writable, d1.enumerable, d1.configurable);
console.log(d1.value === Object.create);
const d2: any = Object.getOwnPropertyDescriptor(Object, "getOwnPropertyDescriptor");
console.log(d2.value === Object.getOwnPropertyDescriptor);
const o: any = Object;
console.log(o.hasOwnProperty("create"), o.hasOwnProperty("defineProperty"));
console.log(o.hasOwnProperty("getOwnPropertyDescriptors"));
