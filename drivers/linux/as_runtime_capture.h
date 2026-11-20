/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_RUNTIME_CAPTURE_H
#define AS_RUNTIME_CAPTURE_H
#include <linux/types.h>
/* Fill the original caller-owned message slice before this callback returns.
 * No output owner/allocation/refcount is introduced. The return is bytes filled;
 * capacity <64 returns 0 for physical space shortage without touching output or
 * inventing an errno. All temporary read-set resources end inside this call. */
size_t as_runtime_capture_current(const u64 *available, const u64 *values,
                                 void *output, size_t capacity);

/* Immutable little-endian technical projection, not a ToolExecution assertion.
 * Header: owner_tgid:u32,status:u32,owner_group_birth:u64,begin_ns:u64,end_ns:u64,
 * thread_pointer:u64,isolate:u64,frame:u64,native_error:i32,presence:u32.
 * presence bit0=thread_pointer,bit1=isolate,bit2=frame; an acquired zero remains
 * present. Unavailable metadata is represented by the original message's
 * header and GAP. Allocation failures have their own GAP, never a read errno.
 * status: 0 Complete,1 Partial,2 Unavailable,3 Inconsistent. A complete technical
 * projection may contain zero, one or many public-message candidates.
 * Every following TLV has kind:u32,payload_bytes:u32 then its payload below.
 * Addresses and tagged references are scoped only to this one owned capture.
 * All strings use actual UTF-16 code units, including unpaired surrogates.
 */
enum as_runtime_projection {
    /* tagged:u64,map:u64,instance_type:u32,scope:u32 (1 store,2 public,3 argument) */
    AS_CAPTURE_OBJECT = 1,
    /* holder,key,value,original_slot,details:5*u64,representation:u32,property_kind:u32.
     * representation0=tagged,1=unhandled double,2=unavailable representation.
     * property_kind0=actual own data,1=actual non-data,2=details not decoded.
     * Non-data tuples retain acquired values but never call/access the getter.
     * The key's STRING node carries its original complete/partial name. */
    AS_CAPTURE_PROPERTY,
    /* tagged:u64,requested_units:u32,status:u32,acquired_bytes:u32,
     * native_error:i32; actual UTF16LE byte prefix (possibly an odd byte).
     * status0 complete,1 partial. Complete iff acquired_bytes=2*requested_units.
     * native_error is an actual string read errno only, zero if none. */
    AS_CAPTURE_STRING,
    /* tagged:u64,signed_value:i64 */
    AS_CAPTURE_SMI,
    /* tagged:u64,actual IEEE754 binary64 bits:u64 */
    AS_CAPTURE_NUMBER,
    /* tagged:u64,actual V8 Oddball kind:i64,semantic:u32,reserved:u32.
     * semantic from actual v8dbg: 0 unknown,1 false,2 true,3 null,4 undefined. */
    AS_CAPTURE_ODDBALL,
    /* tagged:u64,elements:u64,length:i64,capacity:i64 */
    AS_CAPTURE_ARRAY,
    /* array:u64,index:u64,value:u64,original_slot:u64 */
    AS_CAPTURE_MEMBER,
    /* map,table:u64;length,elements,deleted,buckets:4*i64 */
    AS_CAPTURE_MAP,
    /* map,index,key,value,original_entry:5*u64 */
    AS_CAPTURE_ENTRY,
    /* object:u64,address:u64,native_error:i32,reason:u32 */
    AS_CAPTURE_GAP,
    /* tagged:u64,scope:u32,complete:u32. Arguments require a complete own-data
     * enumeration; a partial object is never silently turned into a subset. */
    AS_CAPTURE_END,
};
enum as_runtime_capture_gap {
    AS_CAPTURE_READ_FAULT = 1,
    AS_CAPTURE_CAPACITY,
    AS_CAPTURE_UNSUPPORTED,
    AS_CAPTURE_CHANGED,
    AS_CAPTURE_INVALID_LAYOUT,
    AS_CAPTURE_NO_CURRENT_ISOLATE,
    AS_CAPTURE_ALLOCATION,
};
#endif
