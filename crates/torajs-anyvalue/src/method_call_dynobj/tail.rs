//! The nominal-struct method arm — split out of the parent under the
//! 500-line file discipline (rotation 146). A child module reaches its
//! parent's private items, so the extern block, the method-id
//! constants and the shared helpers stay private with no visibility
//! churn.
//!
//! The IsCallable own-entry probes that used to live beside it went
//! with OrdinaryToPrimitive's own dispatch: §7.1.1.1 asks IsCallable
//! of what `Get` answered, so the walk has no reason to probe a
//! receiver's own entries for callability any more.

use core::ffi::c_void;

use super::*;

/// MethodMeta flags bit 1 — ABI mirror of torajs-structmeta's
/// `METHOD_FLAG_TWIN_PRIMARY` (the record's adapter is the
/// recv-first `__cmany_` twin; see 404-01).
const METHOD_FLAG_TWIN_PRIMARY: u32 = 2;

/// `Tag::Obj` arm — see module doc. `obj` is the struct cell, and
/// the field slot read is a borrow (the receiver owns it and
/// outlives the call).
pub(crate) unsafe fn struct_method(
    obj: *mut c_void,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // 刀 2 (RFC 20260714-t262-top-clusters) — a NULL-name entry
        // is the reified-cell `.call` re-dispatch; an array-family
        // mid runs the ES generic array-like semantics over the
        // struct receiver (index/length reads route the chunk-744
        // struct arms). Mutators stay excluded: a static-layout
        // struct has no growable props bag to relocate.
        if name_str.is_null()
            && crate::method_call_arraylike_concat::obj_supported(mid)
            && !crate::method_call_arraylike_mut::arraylike_mut_supported(mid)
        {
            return crate::method_call_arraylike_concat::obj_method(
                obj,
                mid,
                core::ptr::null_mut(),
                argv,
                argc,
            );
        }
        if !name_str.is_null() {
            let class_tag = (obj.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read();
            let layout = __torajs_struct_layout_lookup(class_tag);
            if !layout.is_null() {
                let k = torajs_rc::str_wtf8::StrWtf8::of(name_str.cast());
                let (name_bytes, name_len) = (k.as_ptr(), k.len());
                let idx = __torajs_struct_field_find(layout, name_bytes, name_len);
                if idx != u32::MAX {
                    let info = __torajs_struct_field_info(layout, idx);
                    let raw = (obj.cast::<u8>().add(info.field_byte_offset as usize) as *const u64)
                        .read();
                    let pair = match info.type_tag {
                        // The slot is itself a NaN-box AnyValue.
                        FIELD_TAG_ANY => closure_boxed_entry(raw),
                        // The slot is the raw closure env cell.
                        FIELD_TAG_CLOSURE => closure_cell_entry(raw as *mut c_void),
                        _ => None,
                    };
                    if let Some((env, entry)) = pair {
                        // Recv-first field value binds the struct
                        // instance as `this` (knife 2f).
                        let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64);
                        return crate::method_call::invoke_with_this(env, entry, recv, argv, argc);
                    }
                    // §7.2.3 over an Any-typed slot — the pair
                    // probe above is a closure-cell test, and a
                    // callable Proxy or a class object has no pair
                    // to jump to. Neither is a miss.
                    if info.type_tag == FIELD_TAG_ANY {
                        let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64);
                        if let Some(v) =
                            crate::call_non_closure::call_with_this(raw, recv, argv, argc)
                        {
                            return v;
                        }
                    }
                    // A resolved field that isn't callable keeps the
                    // TypeError — never shadowed by the fallback.
                    return not_callable();
                }
                // 刀 4 (RFC 20260714-t262-top-clusters) — no such
                // FIELD: probe the class-methods dispatch table. A
                // hit invokes the `__cm_<C>__<m>` body through its
                // boxed adapter with the instance in the env slot
                // (the adapter feeds it into the `__this` param).
                //
                // RFC 20260714-objlit-accessor blade 5 — class
                // ACCESSORS ride the same table under `__getter_<p>`,
                // and that spelling is not a callable method name:
                // `o.__getter_b()` keeps the honest no-such TypeError
                // (the property read reaches the getter, the call does
                // not). A user method really named `b` still wins here.
                // A re-parented instance (RFC 20260920) answers its
                // class's methods and accessors only if its user
                // chain still reaches them — the chain read below.
                let reparented = crate::member_get_layout::obj_user_proto_raised(obj);
                let mut mflags: u32 = 0;
                let adapter =
                    if !reparented && __torajs_accessor_name_kind(name_bytes, name_len) == 255 {
                        __torajs_struct_method_find_flags(layout, name_bytes, name_len, &mut mflags)
                    } else {
                        core::ptr::null()
                    };
                if !adapter.is_null() {
                    // 404-01 — a twin-primary record's adapter is the
                    // receiver-polymorphic `__cmany_` twin: recv-first
                    // calling convention (receiver box in argv[0], env
                    // dropped). Minted for GENERIC classes, whose mono
                    // bodies would misread a specialization's layout.
                    if mflags & METHOD_FLAG_TWIN_PRIMARY != 0 {
                        let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64);
                        let mut argv2: Vec<u64> = Vec::with_capacity(argc as usize + 1);
                        argv2.push(recv);
                        if argc > 0 {
                            argv2.extend_from_slice(core::slice::from_raw_parts(
                                argv,
                                argc as usize,
                            ));
                        }
                        return crate::method_call::invoke_boxed(
                            obj,
                            adapter as u64,
                            argv2.as_ptr(),
                            argc + 1,
                        );
                    }
                    return crate::method_call::invoke_boxed(obj, adapter as u64, argv, argc);
                }
                // S2.34 — getter-as-callee (mirror of the dynobj /
                // arr accessor arms): a class ACCESSOR under this
                // name runs its getter first (receiver in the env
                // slot, the `__cm_<C>__<p>_get` adapter shape), and
                // the (owned) answer dispatches as the callee with
                // the instance bound as `this` (§13.3.6 EvaluateCall
                // — the Reference base). A throwing getter aborts
                // before the callee probe; a resolved non-callable
                // keeps the TypeError.
                let getter_key: Vec<u8> = b"__getter_"
                    .iter()
                    .chain(core::slice::from_raw_parts(name_bytes, name_len as usize))
                    .copied()
                    .collect();
                let getter_adapter = if reparented {
                    core::ptr::null()
                } else {
                    __torajs_struct_method_find(
                        layout,
                        getter_key.as_ptr(),
                        getter_key.len() as u32,
                    )
                };
                if !getter_adapter.is_null() {
                    let got = crate::method_call::invoke_boxed(
                        obj,
                        getter_adapter as u64,
                        core::ptr::null(),
                        0,
                    );
                    if __torajs_throw_check() != 0 {
                        return got;
                    }
                    if let Some((env, entry)) = closure_boxed_entry(got) {
                        let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64);
                        let r = crate::method_call::invoke_with_this(env, entry, recv, argv, argc);
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                        return r;
                    }
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                    return not_callable();
                }
                // RFC 20260802-class-computed-member 刀 2 — a full
                // field / class-method / accessor miss reads the class
                // prototype chain's OWN entries: a runtime-computed
                // method defined at the class-decl position lands on
                // `__proto_<C>` under its runtime key, which no static
                // table above can answer. A callable hit invokes with
                // the instance bound as `this` (the pair is a borrow —
                // the prototype entry keeps the stake). A reified
                // BUILTIN cell (`__proto_Error`'s `toString`) falls
                // through instead: its boxed dual entry is the
                // bare-receiver throw, and the ordinary fallback below
                // re-routes it through the mid dispatcher with the
                // receiver bound. A chain miss keeps the fallback too.
                let (ptag, pval) =
                    crate::struct_error_msg::struct_proto_chain_pair(obj, name_str.cast());
                if ptag == 4
                    && let Some((env, entry)) = closure_cell_entry(pval as *mut c_void)
                    && crate::method_value::builtin_method_mid(env).is_none()
                {
                    let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64);
                    return crate::method_call::invoke_with_this(env, entry, recv, argv, argc);
                }
            }
        }
        // %Iterator.prototype% helper inheritance (RFC 20260730
        // 刀 2): a full miss (no field, no class method, no
        // accessor) on an instance whose prototype chain carries
        // %Iterator.prototype% — a generator object, an `extends
        // Iterator` heir — mints the lazy helper. Own/class methods
        // above stay authoritative (shadowing wins per §10.1.8);
        // ordinary class instances fail the chain walk and keep the
        // ordinary miss path.
        if __torajs_instanceof_builtin_proto(obj as u64, ITERATOR_PROTO_TAG)
            && let Some(v) = crate::iter_helper::try_helper_chain(obj, mid, argv, argc)
        {
            return v;
        }
        // No layout / absent field → the inherited Object.prototype
        // surface, mirroring the dynobj arm's absent branch.
        object_proto_fallback(obj, mid, name_str, true, core::ptr::null_mut(), argv, argc)
    }
}

/// `%Iterator.prototype%`'s builtin-proto tag (torajs-rc
/// `builtin_proto.rs` tag space, RFC 20260730-iterator-global).
const ITERATOR_PROTO_TAG: i64 = 15;

unsafe extern "C" {
    /// torajs-meta — §7.3.22 prototype-chain membership against a
    /// builtin proto singleton (RFC 20260730-iterator-global).
    fn __torajs_instanceof_builtin_proto(v: u64, proto_tag: i64) -> bool;
}
