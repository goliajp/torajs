// §25.1.6 / §25.3.4 — every method %ArrayBuffer.prototype% and
// %DataView.prototype% own begins by requiring its own internal slot, the
// same way §23.2.4.4 does for the typed arrays. tr had that rule and
// applied it to the typed-array families only, so a borrowed
// ArrayBuffer.prototype.slice fell past the gate into the ordinary
// re-dispatch and ran Array.prototype.slice instead.

function probe(label: string, f: () => unknown): void {
  try {
    console.log(label + ": " + String(f()));
  } catch (e) {
    console.log(label + ": " + (e as any).constructor.name);
  }
}

const abp: any = ArrayBuffer.prototype;
probe("ab-slice-on-array", () => abp.slice.call([1, 2, 3]));
probe("ab-slice-on-string", () => abp.slice.call("abc"));
probe("ab-slice-on-object", () => abp.slice.call({}));
probe("ab-slice-on-number", () => abp.slice.call(1));
probe("ab-transfer-on-object", () => abp.transfer.call({}));

// DataView's own row is in the table for the same reason the other three
// are, but it cannot be exercised through a borrowed method yet: tr does
// not reify DataView.prototype's methods, so `DataView.prototype.getUint8`
// is undefined here and a probe through it would be green for the wrong
// reason — and printing that fact would make the fixture disagree with
// bun, where it IS reified. The direct spelling below is the control this
// fixture can honestly make; the table row costs nothing and leaving it
// out is what would repeat the half-application this commit is about.

// Controls — the right brand still answers.
const buf = new ArrayBuffer(4);
probe("ab-slice-ok", () => (abp.slice.call(buf, 0, 2) as ArrayBuffer).byteLength);
const dv = new DataView(new ArrayBuffer(4));
probe("dv-getUint8-direct", () => dv.getUint8(0));

// Control — an INHERITED method borrowed off ArrayBuffer.prototype is not
// one the prototype owns, so the brand gate must not claim it.
probe("ab-hasOwnProperty", () => abp.hasOwnProperty.call({ a: 1 }, "a"));
probe("ab-inherited-toString", () => typeof abp.propertyIsEnumerable);

// Control — the typed-array families are unchanged.
const tap: any = Object.getPrototypeOf(Uint8Array.prototype);
probe("ta-slice-on-array", () => tap.slice.call([1, 2, 3]));
probe("ta-slice-ok", () => Array.from(new Uint8Array([1, 2, 3]).slice(0, 2)).join(","));
