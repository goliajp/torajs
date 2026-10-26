//! The runtime entries that cannot store under a key, relink a
//! prototype or hand out a function that does: the pure computations
//! (strings, numbers, math, dates, regexps, JSON text), the
//! collections, the readers and the type tests, the drops and
//! retains, the throw state, the frame and promise machinery, the
//! integer-indexed array writes (an index is never a hook key), the
//! fixed-key writes (a function's `name`, a lazily minted `prototype`,
//! the Error family's `name` / `message` / `cause`), private names
//! (never a string key), and calls of a function value (a program
//! function is scanned on its own; a builtin one is only held through
//! a namespace or constructor value, which keeps the statics anyway).
//!
//! Entries named `__torajs_<x>` are listed by `<x>` in one string,
//! sorted and separated by single spaces: a lookup is a binary search
//! over its bytes, and the table costs the binary one relocation
//! rather than one per name.

/// `__torajs_<x>` entries, by `<x>`.
pub(crate) const INERT_RUNTIME: &str = "\
accessor_face_from_any accessor_invoke_getter accessor_pair_new any_call any_call_spread \
any_has_property any_index_get any_index_set any_index_set_soft any_is_arr any_iter_close \
any_iter_close_abrupt any_iter_close_defer any_iter_next any_iter_next_array_like \
any_iter_next_defer any_iter_result_step any_length_get \
any_member_get_priv_tag any_member_get_priv_value any_member_priv_has any_name_get \
any_own_prop_has any_prop_delete any_prop_delete_soft any_prop_enumerable any_prop_has \
any_regexp_prop any_size_get any_sort_undef_pre any_str_symbol_try any_to_bigint any_to_object \
anyv_add_pair anyv_arg_to_struct anyv_arith_pair anyv_arr_cell anyv_arr_fits \
anyv_arr_length_descriptor anyv_bitnot_pair anyv_bitwise_pair anyv_box_from_pair \
anyv_box_str_slot anyv_box_substr_slot anyv_cast_bool anyv_cast_cell anyv_cast_number \
anyv_cast_obj anyv_cast_obj_as_is anyv_cast_obj_members anyv_cast_obj_members_own \
anyv_cast_obj_own anyv_cast_str anyv_cast_str_or_undef anyv_cell_ptr anyv_check_fn \
anyv_class_get anyv_class_register anyv_compare_pair anyv_construct anyv_construct_spread \
anyv_ctor_register anyv_ctorany_register anyv_define_props_source_gate anyv_forin_keys \
anyv_freeze anyv_from_entries anyv_get_proto_of_any anyv_heritage_check anyv_incr_slot \
anyv_incr_value anyv_invoke_recv_first anyv_is_extensible anyv_is_frozen anyv_is_sealed \
anyv_json_gap_str anyv_json_stringify anyv_json_stringify_exotic_obj anyv_json_stringify_full \
anyv_json_stringify_gap anyv_json_stringify_keyed anyv_json_stringify_value anyv_loose_eq \
anyv_number_ctor anyv_obj_rest anyv_or_default anyv_own_entries anyv_own_keys anyv_own_keys_all \
anyv_own_symbols anyv_own_values anyv_payload_rc_inc_pair anyv_prevent_extensions \
anyv_promise_resolve anyv_promise_resolve_fresh anyv_property_key_drop anyv_proto_get \
anyv_proto_member_get anyv_rc_dec anyv_rc_inc anyv_reflect_prevent_extensions \
anyv_reject_regexp_needle anyv_require_object_coercible anyv_rest_fits anyv_retain \
anyv_same_value anyv_seal anyv_str_fits anyv_str_index_descriptor anyv_str_length_descriptor \
anyv_str_slot_tag anyv_str_slot_value anyv_strict_eq anyv_strict_eq_imm_pair anyv_struct_fits \
anyv_struct_keys anyv_substr_slot_tag anyv_substr_slot_value anyv_super_call \
anyv_svz_imm_pair \
anyv_throw_typeerror_if_not_desc_object anyv_throw_typeerror_if_not_object \
anyv_throw_typeerror_if_props_null_only anyv_throw_typeerror_if_props_nullish anyv_to_bool \
anyv_to_callable_cell anyv_to_display_str anyv_to_f64_slot anyv_to_number anyv_to_object_strict \
anyv_to_property_key anyv_to_str anyv_to_str_pair anyv_to_str_pair_prim anyv_to_str_param \
anyv_typeof anyv_unary_neg_pair anyv_unbox_settle anyv_unbox_tag anyv_unbox_value \
anyv_unbox_value_owned arguments_callee argv_init arr_alloc arr_alloc_any arr_alloc_any_filled \
arr_alloc_any_filled_f64 arr_alloc_any_holes arr_any_concat arr_any_index_of \
arr_any_last_index_of arr_any_pop arr_any_push arr_any_shift arr_any_slice \
arr_any_slice_keep_holes arr_any_to_locale_string arr_any_to_reversed arr_any_to_typed \
arr_cell_from_any arr_concat arr_concat_any_arg arr_copy_get arr_copy_within \
arr_delete_tail_guard arr_drop arr_drop_any arr_drop_leaf_instances \
arr_drop_leaf_instances_by_cell arr_drop_leaf_objs arr_drop_leaf_objs_by_cell arr_drop_scalar \
arr_drop_str_elems arr_entries_by_tag arr_extend_any arr_extend_guard arr_extend_keep_holes \
arr_extend_typed_into_any arr_extend_unchecked arr_f64_index_of_undef arr_fill arr_fill_any \
arr_flat arr_flat_any arr_flat_present arr_flat_runtime_depth arr_forin_key_live \
arr_from_any_typed arr_from_string arr_get_any_boxed arr_get_any_owned arr_get_any_tag \
arr_get_any_value arr_has_index arr_index_check_store arr_index_get arr_index_revive_idx \
arr_index_strs arr_iter_create_entries arr_iter_create_keys arr_iter_create_values arr_iter_drop \
arr_iter_step arr_join arr_join_any arr_join_bool arr_join_f64 arr_join_f64_locale arr_join_i64 \
arr_join_i64_locale arr_join_substr arr_keys_only arr_keys_only_of arr_len_write_guard \
arr_map_hole_slow arr_mark_arguments arr_mark_kind arr_mark_last_hole arr_new_from_any \
arr_null_check arr_oob_throw arr_print_bool arr_print_bool_inline arr_print_f64 \
arr_print_f64_inline arr_print_i64 arr_print_i64_inline arr_print_str arr_print_str_inline \
arr_print_substr arr_print_substr_inline arr_push arr_push_any arr_push_cold arr_push_non_deque \
arr_push_unchecked arr_reduce_seed_index arr_reserve arr_reverse arr_set_any arr_set_any_grow \
arr_set_length_any arr_set_length_truncate_scalar arr_set_length_validate arr_shift \
arr_shift_guard arr_slice arr_slice_keep_holes arr_sort_cb arr_sort_exotic arr_species_guard \
arr_splice arr_splice_items arr_subclass_alloc arr_subclass_super_elems arr_subclass_super_len \
arr_substr_adopt_copied arr_substr_materialize_owned arr_throw_reduce_empty \
arr_throw_reduce_right_empty arr_to_reversed arr_typed_nonint_any arr_typed_nonint_f64 \
arr_typed_nonint_str arr_typed_set_grow arr_typed_to_locale_string arr_unshift arr_unshift_any \
arr_with array_from_dyn arraybuffer_create arraybuffer_is_view arrprops_get_tag \
arrprops_get_value async_rejected async_resolved bigint_add bigint_and bigint_as_int_n \
bigint_as_uint_n bigint_clone bigint_cmp bigint_ctor_any bigint_div bigint_drop_rc \
bigint_from_decimal bigint_from_hex bigint_from_number bigint_from_str bigint_is_nonzero \
bigint_mod bigint_mul bigint_neg bigint_not bigint_or bigint_pow bigint_shl bigint_shr \
bigint_sub bigint_to_locale_string bigint_to_number bigint_to_string bigint_to_string_radix \
bigint_xor bool_to_str boolean_wrapper_new boolean_wrapper_subclass_alloc \
boolean_wrapper_subclass_super builtin_ctor_value builtin_proto_method_value capture_box_alloc \
capture_box_drop capture_box_drop_any capture_box_drop_heap capture_box_inc class_cell_raw \
class_cell_release class_computed_field_register class_mark_call_constructs class_mark_intrinsic \
class_source_register class_unit_home class_unit_same_at closure_call_variadic \
closure_call_with_this closure_ctor_value closure_drop_props_slow closure_length_or \
closure_name_str closure_prototype_any closure_source_str collection_add_static \
collection_adder_resolve collection_init_from_iterable ctor_ret_value \
cycle_at_exit_drain cycle_buffer cycle_collect cycle_unbuffer dataview_create date_drop \
date_from_components date_from_iso date_from_ms date_from_value date_get_date date_get_day \
date_get_full_year date_get_hours date_get_milliseconds date_get_minutes date_get_month \
date_get_seconds date_get_time date_get_timezone_offset date_get_utc_date date_get_utc_day \
date_get_utc_full_year date_get_utc_hours date_get_utc_milliseconds date_get_utc_minutes \
date_get_utc_month date_get_utc_seconds date_get_year date_now date_now_static date_parse_iso \
date_set_date date_set_full_year date_set_hours date_set_milliseconds date_set_minutes \
date_set_month date_set_seconds date_set_time date_set_utc_date date_set_utc_full_year \
date_set_utc_hours date_set_utc_milliseconds date_set_utc_minutes date_set_utc_month \
date_set_utc_seconds date_set_year date_subclass_alloc date_subclass_super \
date_subclass_super_components date_to_date_string date_to_gmt_string date_to_iso_string \
date_to_json date_to_locale_date_string date_to_locale_string date_to_locale_time_string \
date_to_string date_to_time_string date_utc_components drop_release dstr_close_pending \
dstr_drain_rest dstr_park_pending dstr_step_one dynobj_alloc dynobj_delete dynobj_entry_is_hole \
dynobj_get_tag dynobj_get_value dynobj_has error_install_cause error_is_error error_message_get \
error_message_present error_name_get error_name_present error_proto_install error_to_string \
error_tostring_dispatch f64_to_str fetch_sync finreg_new finreg_subclass_alloc \
finreg_subclass_super fn_computed_name_define fn_name_str fn_print_outer fn_source_str \
fnprops_bind_cell fnprops_get_tag fnprops_get_value fnsig_to_primitive_str fnsig_to_str \
frame_await_direct frame_await_promise frame_cancelled frame_release_links frame_result_alloc \
frame_return frame_throw frame_trace_links frame_unwind fs_append_file_sync fs_exists_sync \
fs_mkdir_sync fs_read_file_sync fs_readdir_sync fs_size_sync fs_unlink_sync fs_write_file_sync \
function_subclass_alloc generic_tag_match genfn_proto get_builtin_prototype global_ident_get \
global_ident_typeof globalthis_object heap_nullish_check host_jobs_run_until_idle i64_to_str \
in_op_any_num instanceof_builtin_proto instanceof_class_any_tag instanceof_dynamic \
instanceof_fn_value instanceof_generic_any instanceof_object_any io_putc_out io_sink_to_stderr \
io_sink_to_stdout is_constructor is_undef_cell iter_close_abrupt iter_close_value \
iterator_concat iterator_ctor_throw iterator_from iterator_zip iterator_zip_keyed \
jsb_begin_field jsb_finalize jsb_new jsb_push_bool jsb_push_byte jsb_push_field_str jsb_push_i64 \
jsb_push_str_quoted jsb_push_str_raw jsb_stringify_shape json_arr_first json_arr_step \
json_close_indent json_colon json_eat_char json_indent json_is_raw_json json_obj_sep \
json_parse_any json_parse_bool json_parse_float json_parse_int json_parse_reviver \
json_parse_string json_quote_str json_quote_str_top json_raw_json json_value_is \
locale_args_check locale_args_check_date main_exit_code map_clear map_clone map_create \
map_delete map_drop map_entry_get map_entry_get_f64 map_find_str map_get map_get_f64 \
map_get_or_insert map_get_or_insert_computed map_group_by map_has map_iter_create_entries \
map_iter_create_keys map_iter_create_set_entries map_iter_create_values map_iter_drop \
map_iter_next map_iter_step map_iter_step_f64 map_mark_num map_print_outer map_set map_set_str \
map_size map_subclass_alloc map_subclass_super math_abs math_acos math_acosh math_asin \
math_asinh math_atan math_atan2 math_atanh math_cbrt math_ceil math_clz32 math_cos math_cosh \
math_exp math_expm1 math_f16round math_floor math_fround math_imul math_log math_log10 \
math_log1p math_log2 math_max math_min math_pow math_random math_round math_sign math_sin \
math_sinh math_sqrt math_sum_precise math_sum_precise_i64 math_tan math_tanh math_trunc \
microtask_await_byte microtask_yield module_await_promise \
module_await_slow module_await_slow_frame module_body_end module_cancelled module_exit_code \
module_ns_finalize module_yield_rethrow new_target_register new_target_register_entries \
ns_object_console ns_object_json ns_object_math ns_object_reflect ns_static_bind ns_static_cell \
null_to_str num_is_finite_any num_is_finite_f num_is_finite_i num_is_integer_any \
num_is_integer_f num_is_integer_i num_is_nan_any num_is_nan_f num_is_nan_i \
num_is_safe_integer_any num_is_safe_integer_f num_is_safe_integer_i num_parse_float \
num_parse_int num_parse_int_span num_to_exp_f num_to_exp_i num_to_fixed_f num_to_fixed_i \
num_to_locale_f num_to_locale_i num_to_precision_f num_to_precision_i num_to_string_radix_f \
num_to_string_radix_i number_wrapper_new number_wrapper_subclass_alloc \
number_wrapper_subclass_super obj_alloc obj_buffer_slow obj_check_not_frozen \
obj_declared_field_delete obj_deleted_member_get obj_drop_rc obj_drop_sized \
obj_field_exotic_number_or_undef obj_field_exotic_refuse obj_field_exotic_same_cell \
obj_field_get_exotic obj_free_cell obj_freeze obj_freeze_any obj_is_frozen \
obj_key_is_nonenumerable obj_member_deleted obj_own_keys obj_props_drop_slow \
object_create_check_proto object_group_by object_is_f64 perf_now perf_time_origin print_anyv \
print_anyv_inline_top priv_brand_fail priv_cursor priv_field_define priv_install priv_unit_word \
process_argv process_cwd process_exit process_getenv \
process_on_unhandled_rejection_register_closure process_on_unhandled_rejection_register_simple \
process_platform process_stderr_write process_stdout_write promise_all_dyn promise_all_keyed_dyn \
promise_all_sync promise_all_sync_proved promise_alloc_fulfilled promise_alloc_fulfilled_heap \
promise_alloc_fulfilled_heap_repr promise_alloc_fulfilled_repr promise_alloc_rejected \
promise_alloc_rejected_heap promise_allsettled_dyn promise_allsettled_keyed_dyn \
promise_allsettled_sync promise_any_dyn promise_any_sync promise_catch_closure \
promise_catch_simple promise_ctor_patched promise_drop promise_finally promise_finally_closure \
promise_get_value promise_get_value_as promise_obj_return_mark promise_patched_result \
promise_race_dyn promise_race_sync promise_resolve_thenable promise_species_ctor \
promise_stamp_repr promise_subclass_alloc promise_subclass_super promise_then2 \
promise_then_closure promise_then_passthrough promise_then_simple promise_with_resolvers \
proto_cell_raw proxy_create proxy_revocable queue_microtask_closure queue_microtask_simple \
rc_inc reflect_apply reflect_construct regex_compile regex_compile_any regex_compile_baked \
regex_compile_inplace regex_compile_or_throw regex_drop regex_exec regex_exec_span \
regex_get_flags regex_get_last_index regex_get_source regex_has_flag regex_replace_iter_close \
regex_replace_iter_init regex_replace_iter_next regex_set_last_index regex_subclass_alloc \
regex_subclass_super regex_subclass_super_flags regex_test regex_to_string \
regexp_drop_if_coerced regexp_escape_any regexp_from_any register_native_error script_preflight \
set_create set_difference set_intersection set_is_disjoint_from set_is_subset_of \
set_is_superset_of set_print_outer set_relation_setlike set_setop_setlike set_subclass_alloc \
set_subclass_super set_symmetric_difference set_union split_iter_init split_iter_next str_alloc \
str_alloc_capacity str_anchor str_append str_append_lit str_append_substr str_at str_big \
str_blink str_bold str_char_at str_char_code_at str_code_point_at str_concat str_concat_f64 \
str_concat_i64 str_concat_lit str_drop str_ends_with str_ends_with_from str_entries str_eq \
str_eq_cstr str_escape str_fixed str_fontcolor str_fontsize str_from_char_code \
str_from_char_code_f64 str_from_code_point str_from_code_point_f64 str_group_span str_includes \
str_includes_from str_index_keyed str_index_of str_index_of_from str_index_strs str_index_view \
str_is_undef str_is_well_formed str_italics str_keys_only str_last_index_of \
str_last_index_of_from str_link str_locale_case_any str_locale_case_arr str_locale_compare \
str_locale_invalid str_match_all_regex str_match_len str_match_regex str_normalize \
str_null_check str_pad_end str_pad_start str_prepend_lit str_print str_print_inline \
str_prop_enumerable str_prop_has str_regex_replace_any_repl str_repeat str_replace \
str_replace_all str_replace_all_fn str_replace_all_regex str_replace_all_regex_fn \
str_replace_any_pattern str_replace_any_pattern_fn str_replace_any_repl str_replace_fn \
str_replace_regex str_replace_regex_fn str_search_regex str_slice str_small str_sort_cmp \
str_sort_undef_pre str_split str_split_any_sep str_split_any_sep_lim str_split_count \
str_split_count_byte str_split_no_sep str_split_regex str_starts_with str_starts_with_from \
str_strike str_sub str_substr str_substring str_sup str_to_char_arr str_to_locale_lower \
str_to_locale_upper str_to_lower str_to_number str_to_upper str_to_well_formed str_trim \
str_trim_end str_trim_start str_unescape str_uri_decode str_uri_encode string_raw \
string_wrapper_new string_wrapper_subclass_alloc string_wrapper_subclass_super struct_lacks_in \
substr_char_code_at substr_code_point_at substr_concat_lit substr_concat_str_substr \
substr_concat_substr_str substr_concat_substr_substr substr_create substr_drop substr_ends_with \
substr_eq_str substr_includes substr_index_of substr_index_view substr_prepend_lit substr_print \
substr_print_inline substr_share substr_slice substr_starts_with substr_substring \
substr_to_owned substr_trim substr_trim_end substr_trim_into substr_trim_start symbol_alloc \
symbol_async_iterator symbol_description symbol_drop symbol_for symbol_for_any symbol_iterator \
symbol_key_for symbol_key_for_any symbol_print symbol_to_primitive symbol_to_str \
symbol_well_known template_object_begin template_object_end template_object_str then_twin_run \
this_binding_throw throw_check throw_readonly_assign throw_reference_error_name \
throw_reference_error_super_delete throw_reference_error_tdz throw_set throw_take throw_take_tag \
throw_type_error_str to_index typedarray_create typedarray_is_kind typedarray_subclass_alloc \
typedarray_subclass_super uint8array_from_base64 uint8array_from_hex uncaught_exit \
undefined_to_str value_drop_heap weak_key_from_any weak_key_from_any_or_throw weakmap_create \
weakmap_delete weakmap_drop weakmap_get weakmap_get_or_insert weakmap_get_or_insert_computed \
weakmap_has weakmap_set weakmap_subclass_alloc weakmap_subclass_super weakref_create \
weakref_deref_any weakref_drop weakref_new_checked weakref_subclass_alloc weakref_subclass_super \
weakref_target_dying weakset_add weakset_create weakset_delete weakset_drop weakset_has \
weakset_subclass_alloc weakset_subclass_super";

/// Host printers declared without the runtime prefix.
pub(crate) const INERT_HOST: &[&str] = &[
    "print_bool",
    "print_bool_inline",
    "print_f64",
    "print_f64_inline",
    "print_i64",
    "print_i64_inline",
];

/// True when the space-separated, sorted `table` holds `name`.
pub(crate) fn listed(table: &str, name: &str) -> bool {
    let (b, x) = (table.as_bytes(), name.as_bytes());
    // `lo` and `hi` are entry starts (or the end)
    let (mut lo, mut hi) = (0, b.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let start = (b[lo..mid].iter().rposition(|&c| c == b' ')).map_or(lo, |i| lo + i + 1);
        let end = (b[start..].iter().position(|&c| c == b' ')).map_or(b.len(), |i| start + i);
        match b[start..end].cmp(x) {
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Less => lo = end + 1,
            std::cmp::Ordering::Greater => hi = start,
        }
    }
    false
}
