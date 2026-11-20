// SPDX-License-Identifier: GPL-2.0-only
/* One current-thread V8 technical projection. Public-message interpretation is
 * deliberately owned by the adapter. No target reads survive this callback. */
#include <linux/ktime.h>
#include <linux/bitmap.h>
#include <linux/list.h>
#include <linux/rbtree.h>
#include <linux/pid.h>
#include <linux/sched.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <linux/unaligned.h>
#ifdef CONFIG_ARM64
#include <asm/sysreg.h>
#endif
#include "as_runtime_capture.h"

enum capture_field {
#define AS_RUNTIME_FIELD(name) CF_##name,
#include "as_runtime_fields.h"
#undef AS_RUNTIME_FIELD
    CF_FIELDS
};
struct captured_read {
    struct list_head link;
    struct rb_node index;
    u64 address;
    size_t bytes;
    int error;
    u8 data[];
};
struct captured_node {
    struct list_head link;
    u64 address;
    unsigned int scope;
};
struct capture_walk {
    u8 *data;
    size_t size, capacity;
    const u64 *available, *values;
    struct list_head reads, nodes;
    struct rb_root read_index;
    size_t read_bytes;
    bool full;
    int read_error;
    u64 failures;
};
#define V(c, name) ((c)->values[CF_##name])
#define HAS(c, name) ((c)->available[CF_##name / 64] & BIT_ULL(CF_##name % 64))
static void status(struct capture_walk *c, u32 value)
{
    u32 old = get_unaligned_le32(c->data + 4);
    if (old != 3 && (value == 3 || old == 0))
        put_unaligned_le32(value, c->data + 4);
}
static void *reserve(struct capture_walk *c, u32 kind, size_t bytes)
{
    struct capture_walk *r = c;
    void *body;
    if (c->full || bytes > U32_MAX || r->size > r->capacity ||
        bytes + 40 > r->capacity - r->size) {
        if (!c->full && r->size <= r->capacity && r->capacity - r->size >= 32) {
            u8 *tail = r->data + r->size;
            put_unaligned_le32(AS_CAPTURE_GAP, tail); put_unaligned_le32(24, tail + 4);
            memset(tail + 8, 0, 20); put_unaligned_le32(AS_CAPTURE_CAPACITY, tail + 28); r->size += 32;
        }
        c->full = true; status(c, 1); return NULL;
    }
    put_unaligned_le32(kind, r->data + r->size);
    put_unaligned_le32(bytes, r->data + r->size + 4);
    body = r->data + r->size + 8; r->size += 8 + bytes;
    return body;
}
static void gap(struct capture_walk *c, u64 object, u64 address, int error, u32 reason)
{
    void *body;
    c->failures++;
    status(c, reason == AS_CAPTURE_CHANGED ? 3 : 1);
    if (error && !get_unaligned_le32(c->data + 56))
        put_unaligned_le32(error, c->data + 56);
    body = reserve(c, AS_CAPTURE_GAP, 24);
    if (!body) return;
    put_unaligned_le64(object, body); put_unaligned_le64(address, body + 8);
    put_unaligned_le32(error, body + 16); put_unaligned_le32(reason, body + 20);
}
static bool need(struct capture_walk *c, const enum capture_field *fields, size_t count)
{
    size_t i;
    for (i = 0; i < count; i++)
        if (!(c->available[fields[i] / 64] & BIT_ULL(fields[i] % 64))) {
            gap(c, 0, 0, 0, AS_CAPTURE_UNSUPPORTED); return false;
        }
    return true;
}
#define NEED(c, ...) need(c, (const enum capture_field[]){__VA_ARGS__}, \
    sizeof((const enum capture_field[]){__VA_ARGS__}) / sizeof(enum capture_field))
static bool read_at(struct capture_walk *c, u64 address, void *output, size_t bytes)
{
    struct captured_read *r;
    struct rb_node **link = &c->read_index.rb_node, *parent = NULL;
    int error;
    if (c->full) return false;
    while (*link) {
        parent = *link;
        r = rb_entry(parent, struct captured_read, index);
        if (r->address == address && r->bytes == bytes) {
            if (r->error) { c->read_error = r->error; c->failures++; return false; }
            memcpy(output, r->data, bytes); return true;
        }
        if (address < r->address || (address == r->address && bytes < r->bytes))
            link = &parent->rb_left;
        else
            link = &parent->rb_right;
    }
    if (address > U64_MAX - bytes || bytes > c->capacity ||
        c->read_bytes > c->capacity - bytes) {
        gap(c, 0, address, 0, AS_CAPTURE_CAPACITY); return false;
    }
    r = kmalloc(sizeof(*r) + bytes, GFP_ATOMIC);
    if (!r) { gap(c, 0, address, 0, AS_CAPTURE_ALLOCATION); return false; }
    error = copy_from_user_nofault(r->data, (void __user *)(unsigned long)address, bytes);
    r->address = address; r->bytes = bytes; r->error = error;
    rb_link_node(&r->index, parent, link); rb_insert_color(&r->index, &c->read_index);
    list_add_tail(&r->link, &c->reads);
    if (error) { c->read_error = error; gap(c, 0, address, error, AS_CAPTURE_READ_FAULT); return false; }
    c->read_bytes += bytes;
    memcpy(output, r->data, bytes); return true;
}
static bool word(struct capture_walk *c, u64 address, u64 *value)
{
    __le64 raw;
    if (!read_at(c, address, &raw, sizeof(raw))) return false;
    *value = le64_to_cpu(raw); return true;
}
static bool at(struct capture_walk *c, u64 object, u64 offset, u64 *address)
{
    u64 tag;
    if (!NEED(c, CF_V8_HeapObjectTag, CF_V8_HeapObjectTagMask, CF_V8_TaggedSize)) return false;
    tag = V(c, V8_HeapObjectTag);
    if ((object & V(c, V8_HeapObjectTagMask)) != tag || object < tag ||
        !V(c, V8_TaggedSize) || (object - tag) % V(c, V8_TaggedSize) ||
        object - tag > U64_MAX - offset) {
        gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT); return false;
    }
    *address = object - tag + offset; return true;
}
static bool object_word(struct capture_walk *c, u64 object, u64 offset, u64 *value)
{ u64 address; return at(c, object, offset, &address) && word(c, address, value); }
static bool smi(struct capture_walk *c, u64 raw, s64 *value)
{
    u64 shift, mask;
    if (!NEED(c, CF_V8_SmiTag, CF_V8_SmiTagMask, CF_V8_SmiShiftSize, CF_V8_SmiValueShift)) return false;
    shift = V(c, V8_SmiShiftSize) + V(c, V8_SmiValueShift);
    if (!shift || shift >= 64) return false;
    mask = BIT_ULL(shift) - 1;
    if ((raw & V(c, V8_SmiTagMask)) != V(c, V8_SmiTag) || (raw & mask) != V(c, V8_SmiTag)) return false;
    *value = (s64)raw >> shift; return true;
}
static bool header(struct capture_walk *c, u64 object, u64 *map, u32 *type)
{
    u64 address;
    __le16 raw;
    if (!NEED(c, CF_V8_class_HeapObject__map__Map, CF_V8_class_Map__instance_type__uint16_t)) return false;
    if (!object_word(c, object, V(c, V8_class_HeapObject__map__Map), map) ||
        !at(c, *map, V(c, V8_class_Map__instance_type__uint16_t), &address) ||
        !read_at(c, address, &raw, 2)) return false;
    *type = le16_to_cpu(raw); return true;
}
static bool seen(struct capture_walk *c, u64 address, unsigned int scope)
{
    struct captured_node *n;
    list_for_each_entry(n, &c->nodes, link)
        if (n->address == address && n->scope == scope) return true;
    n = kmalloc(sizeof(*n), GFP_ATOMIC);
    if (!n) { gap(c, address, 0, 0, AS_CAPTURE_ALLOCATION); return true; }
    n->address = address; n->scope = scope; list_add_tail(&n->link, &c->nodes);
    return false;
}
static bool string_units(struct capture_walk *c, u64 object, u32 start, u32 count,
                         u8 *output, unsigned int depth, size_t *copied)
{
    u64 map, address, first, second, raw;
    u32 type, length, representation;
    __le32 raw_length;
    s64 offset;
    if (depth > 8) { gap(c, object, 0, 0, AS_CAPTURE_CAPACITY); return false; }
    if (!NEED(c, CF_V8_FirstNonstringType, CF_V8_class_String__length__int32_t,
              CF_V8_StringRepresentationMask, CF_V8_StringEncodingMask,
              CF_V8_SeqStringTag, CF_V8_OneByteStringTag)) return false;
    if (!header(c, object, &map, &type) || type >= V(c, V8_FirstNonstringType) ||
        !at(c, object, V(c, V8_class_String__length__int32_t), &address) ||
        !read_at(c, address, &raw_length, 4)) return false;
    length = le32_to_cpu(raw_length);
    if (start > length || count > length - start) { gap(c, object, address, 0, AS_CAPTURE_INVALID_LAYOUT); return false; }
    if (!count) return true;
    representation = type & V(c, V8_StringRepresentationMask);
    if (representation == V(c, V8_SeqStringTag)) {
        if ((type & V(c, V8_StringEncodingMask)) == V(c, V8_OneByteStringTag)) {
            u32 i;
            if (!NEED(c, CF_V8_class_SeqOneByteString__chars__char) ||
                !at(c, object, V(c, V8_class_SeqOneByteString__chars__char) + start, &address)) return false;
            for (i = 0; i < count; i++) {
                u8 byte;
                if (!read_at(c, address + i, &byte, 1)) return false;
                put_unaligned_le16(byte, output + i * 2); *copied += 2;
            }
            return true;
        }
        if (!NEED(c, CF_V8_class_SeqTwoByteString__chars__char) ||
            !at(c, object, V(c, V8_class_SeqTwoByteString__chars__char) + (u64)start * 2, &address)) return false;
        {
            size_t i;
            for (i = 0; i < (size_t)count * 2; i++) {
                if (!read_at(c, address + i, output + i, 1)) return false;
                (*copied)++;
            }
            return true;
        }
    }
    if (HAS(c, V8_ThinStringTag) && representation == V(c, V8_ThinStringTag)) {
        if (!NEED(c, CF_V8_class_ThinString__actual__String) ||
            !object_word(c, object, V(c, V8_class_ThinString__actual__String), &first)) return false;
        return string_units(c, first, start, count, output, depth + 1, copied);
    }
    if (HAS(c, V8_SlicedStringTag) && representation == V(c, V8_SlicedStringTag)) {
        if (!NEED(c, CF_V8_class_SlicedString__parent__String, CF_V8_class_SlicedString__offset__SMI) ||
            !object_word(c, object, V(c, V8_class_SlicedString__parent__String), &first) ||
            !object_word(c, object, V(c, V8_class_SlicedString__offset__SMI), &raw) ||
            !smi(c, raw, &offset) || offset < 0 || offset > U32_MAX - start) return false;
        return string_units(c, first, start + offset, count, output, depth + 1, copied);
    }
    if (HAS(c, V8_ConsStringTag) && representation == V(c, V8_ConsStringTag)) {
        u32 left;
        if (!NEED(c, CF_V8_class_ConsString__first__String, CF_V8_class_ConsString__second__String) ||
            !object_word(c, object, V(c, V8_class_ConsString__first__String), &first) ||
            !object_word(c, object, V(c, V8_class_ConsString__second__String), &second) ||
            !at(c, first, V(c, V8_class_String__length__int32_t), &address) ||
            !read_at(c, address, &raw_length, 4)) return false;
        length = le32_to_cpu(raw_length);
        left = start < length ? min(count, length - start) : 0;
        return (!left || string_units(c, first, start, left, output, depth + 1, copied)) &&
            (left == count || string_units(c, second, start > length ? start - length : 0,
                                          count - left, output + left * 2, depth + 1, copied));
    }
    gap(c, object, 0, 0, AS_CAPTURE_UNSUPPORTED); return false;
}
static u8 *string(struct capture_walk *c, u64 object, u32 *units)
{
    __le32 raw;
    u64 address;
    u8 *result;
    size_t copied = 0;
    bool complete;
    void *body;
    if (!NEED(c, CF_V8_class_String__length__int32_t) ||
        !at(c, object, V(c, V8_class_String__length__int32_t), &address) || !read_at(c, address, &raw, 4)) return NULL;
    *units = le32_to_cpu(raw);
    if (*units > (c->capacity - 64) / 2) { gap(c, object, address, 0, AS_CAPTURE_CAPACITY); return NULL; }
    result = kmalloc(max_t(size_t, 1, (size_t)*units * 2), GFP_ATOMIC);
    if (!result) { gap(c, object, address, 0, AS_CAPTURE_ALLOCATION); return NULL; }
    c->read_error = 0;
    complete = string_units(c, object, 0, *units, result, 0, &copied);
    if (!seen(c, object, 5)) {
        body = reserve(c, AS_CAPTURE_STRING, 24 + copied);
        if (body) {
            put_unaligned_le64(object, body); put_unaligned_le32(*units, body + 8);
            put_unaligned_le32(complete ? 0 : 1, body + 12); put_unaligned_le32(copied, body + 16);
            put_unaligned_le32(complete ? 0 : c->read_error, body + 20);
            memcpy(body + 24, result, copied);
        }
    }
    if (!complete) { kfree(result); return NULL; }
    return result;
}
static bool name_is(const u8 *name, u32 units, const char *expected)
{
    u32 i;
    if (strlen(expected) != units) return false;
    for (i = 0; i < units; i++) if (get_unaligned_le16(name + i * 2) != expected[i]) return false;
    return true;
}
static bool public_name(const u8 *name, u32 units)
{
    static const char * const names[] = { "type", "role", "id", "name", "arguments", "content", "responseId", "provider", "api", "model" };
    size_t i;
    for (i = 0; i < ARRAY_SIZE(names); i++) if (name_is(name, units, names[i])) return true;
    return false;
}
static void project(struct capture_walk *c, u64 object, unsigned int scope, unsigned int depth);
static void property(struct capture_walk *c, u64 object, u64 key, u64 raw, u64 slot,
                     u64 details, unsigned int scope, unsigned int depth, bool tagged_values, u32 property_kind)
{
    u64 map;
    u32 type, units = 0, representation = 0;
    s64 decoded_details;
    u8 *name = NULL;
    void *body;
    if (scope != 1 && header(c, key, &map, &type) && HAS(c, V8_FirstNonstringType) && type < V(c, V8_FirstNonstringType))
        name = string(c, key, &units);
    body = reserve(c, AS_CAPTURE_PROPERTY, 48);
    if (!body) { kfree(name); return; }
    put_unaligned_le64(object, body); put_unaligned_le64(key, body + 8); put_unaligned_le64(raw, body + 16);
    put_unaligned_le64(slot, body + 24); put_unaligned_le64(details, body + 32);
    if (tagged_values) representation = 0;
    else if (!HAS(c, V8_prop_representation_mask) || !HAS(c, V8_prop_representation_shift) ||
        !HAS(c, V8_prop_representation_double) || V(c, V8_prop_representation_shift) >= 32 ||
        !smi(c, details, &decoded_details)) representation = 2;
    else if (((decoded_details & V(c, V8_prop_representation_mask)) >> V(c, V8_prop_representation_shift)) ==
        V(c, V8_prop_representation_double)) representation = 1;
    put_unaligned_le32(representation, body + 40); put_unaligned_le32(property_kind, body + 44);
    if (property_kind || (scope == 2 && (!name || !public_name(name, units)))) { kfree(name); return; }
    if (representation) { gap(c, object, slot, 0, AS_CAPTURE_UNSUPPORTED); kfree(name); return; }
    if (scope == 1) project(c, raw, 2, depth + 1);
    else if (scope == 3 || (name && name_is(name, units, "arguments"))) project(c, raw, 3, depth + 1);
    else if (name && name_is(name, units, "content")) project(c, raw, 4, depth + 1);
    else project(c, raw, 0, depth + 1);
    kfree(name);
}
static void dictionary_fields(struct capture_walk *c, u64 object, unsigned int scope, unsigned int depth)
{
    u64 dictionary, map, raw, slot, key, value, details, entry, stride;
    u32 type;
    s64 capacity, length, decoded, i;
    if (!NEED(c, CF_NAME_DICTIONARY_TYPE, CF_NAME_DICTIONARY_CAPACITY, CF_NAME_DICTIONARY_ENTRY_BASE,
        CF_NAME_DICTIONARY_ENTRY_SIZE, CF_NAME_DICTIONARY_VALUE, CF_NAME_DICTIONARY_DETAILS,
        CF_V8_class_JSReceiver__raw_properties_or_hash__Tagged_Object_, CF_V8_class_FixedArrayBase__length__SMI,
        CF_V8_class_FixedArray__data__uintptr_t, CF_V8_type_Oddball__ODDBALL_TYPE,
        CF_V8_prop_kind_mask, CF_V8_prop_kind_Data)) return;
    if (!object_word(c, object, V(c, V8_class_JSReceiver__raw_properties_or_hash__Tagged_Object_), &dictionary) ||
        !header(c, dictionary, &map, &type) || type != V(c, NAME_DICTIONARY_TYPE) ||
        !object_word(c, dictionary, V(c, NAME_DICTIONARY_CAPACITY), &raw) || !smi(c, raw, &capacity) ||
        !object_word(c, dictionary, V(c, V8_class_FixedArrayBase__length__SMI), &raw) || !smi(c, raw, &length)) {
        gap(c, object, 0, 0, AS_CAPTURE_UNSUPPORTED); return;
    }
    stride = V(c, NAME_DICTIONARY_ENTRY_SIZE);
    if (capacity < 0 || length < 0 || (u64)capacity > c->capacity / 48 ||
        !stride || stride > 128 || V(c, NAME_DICTIONARY_VALUE) >= stride || V(c, NAME_DICTIONARY_DETAILS) >= stride ||
        V(c, NAME_DICTIONARY_ENTRY_BASE) + (u64)capacity * stride >
            V(c, V8_class_FixedArray__data__uintptr_t) + (u64)length * 8) {
        gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT); return;
    }
    for (i = 0; i < capacity && !c->full; i++) {
        if (!at(c, dictionary, V(c, NAME_DICTIONARY_ENTRY_BASE) + i * stride, &entry) ||
            !word(c, entry, &key) || !header(c, key, &map, &type)) continue;
        /* Empty/deleted dictionary keys are V8 Oddballs, not data properties. */
        if (type == V(c, V8_type_Oddball__ODDBALL_TYPE)) continue;
        slot = entry + V(c, NAME_DICTIONARY_VALUE);
        if (!word(c, slot, &value) || !word(c, entry + V(c, NAME_DICTIONARY_DETAILS), &details)) continue;
        if (!smi(c, details, &decoded)) {
            property(c, object, key, value, slot, details, scope, depth, true, 2);
            gap(c, object, entry, 0, AS_CAPTURE_INVALID_LAYOUT); continue;
        }
        if ((decoded & V(c, V8_prop_kind_mask)) != V(c, V8_prop_kind_Data)) {
            property(c, object, key, value, slot, details, scope, depth, true, 1);
            if (scope == 3) gap(c, object, entry, 0, AS_CAPTURE_UNSUPPORTED);
            continue;
        }
        property(c, object, key, value, slot, details, scope, depth, true, 0);
    }
}
static void object_fields(struct capture_walk *c, u64 object, u64 map, unsigned int scope, unsigned int depth)
{
    u64 address, descriptors, dmap, raw, key, value, props, slot, field_index, capacity;
    u32 bits, count, dtype;
    u8 size_words, start_words;
    __le32 raw_bits;
    s64 details;
    u32 i;
    if (!NEED(c, CF_V8_class_Map__bit_field3__int, CF_V8_bit_field3_is_dictionary_map_shift)) return;
    if (!at(c, map, V(c, V8_class_Map__bit_field3__int), &address) || !read_at(c, address, &raw_bits, 4)) return;
    bits = le32_to_cpu(raw_bits);
    if (V(c, V8_bit_field3_is_dictionary_map_shift) >= 32) { gap(c, object, address, 0, AS_CAPTURE_INVALID_LAYOUT); return; }
    if (bits & BIT(V(c, V8_bit_field3_is_dictionary_map_shift))) { dictionary_fields(c, object, scope, depth); return; }
    if (!NEED(c, CF_V8_bit_field3_number_of_own_descriptors_mask, CF_V8_bit_field3_number_of_own_descriptors_shift,
        CF_V8_class_Map__instance_descriptors__Tagged_DescriptorArray_, CF_V8_type_DescriptorArray__DESCRIPTOR_ARRAY_TYPE,
        CF_V8_class_DescriptorArray__header_size__uintptr_t, CF_V8_prop_desc_size, CF_V8_prop_desc_key,
        CF_V8_prop_desc_details, CF_V8_prop_desc_value, CF_V8_prop_kind_mask, CF_V8_prop_kind_Data,
        CF_V8_prop_location_mask, CF_V8_prop_location_shift, CF_V8_prop_location_Descriptor,
        CF_V8_prop_index_mask, CF_V8_prop_index_shift, CF_V8_class_Map__instance_size_in_words__char,
        CF_V8_class_Map__inobject_properties_start_or_constructor_function_index__char)) return;
    if (V(c, V8_bit_field3_number_of_own_descriptors_shift) >= 32) {
        gap(c, object, address, 0, AS_CAPTURE_INVALID_LAYOUT); return;
    }
    if (V(c, V8_prop_location_shift) >= 32 || V(c, V8_prop_index_shift) >= 32 ||
        !V(c, V8_prop_desc_size) || V(c, V8_prop_desc_size) > 16 ||
        V(c, V8_prop_desc_key) >= V(c, V8_prop_desc_size) ||
        V(c, V8_prop_desc_details) >= V(c, V8_prop_desc_size) ||
        V(c, V8_prop_desc_value) >= V(c, V8_prop_desc_size)) {
        gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT); return;
    }
    count = (bits & V(c, V8_bit_field3_number_of_own_descriptors_mask)) >> V(c, V8_bit_field3_number_of_own_descriptors_shift);
    if (!count) return;
    if (count > c->capacity / 48 || !object_word(c, map, V(c, V8_class_Map__instance_descriptors__Tagged_DescriptorArray_), &descriptors) ||
        !header(c, descriptors, &dmap, &dtype) || dtype != V(c, V8_type_DescriptorArray__DESCRIPTOR_ARRAY_TYPE)) {
        gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT); return;
    }
    if (!at(c, map, V(c, V8_class_Map__instance_size_in_words__char), &address) || !read_at(c, address, &size_words, 1) ||
        !at(c, map, V(c, V8_class_Map__inobject_properties_start_or_constructor_function_index__char), &address) ||
        !read_at(c, address, &start_words, 1)) return;
    if (size_words < start_words) { gap(c, object, address, 0, AS_CAPTURE_INVALID_LAYOUT); return; }
    capacity = size_words - start_words;
    for (i = 0; i < count && !c->full; i++) {
        u64 entry;
        if (!at(c, descriptors, V(c, V8_class_DescriptorArray__header_size__uintptr_t) +
            (u64)i * V(c, V8_prop_desc_size) * 8, &entry) ||
            !word(c, entry + V(c, V8_prop_desc_key) * 8, &key) ||
            !word(c, entry + V(c, V8_prop_desc_details) * 8, &raw)) continue;
        if (!smi(c, raw, &details)) { gap(c, object, entry, 0, AS_CAPTURE_INVALID_LAYOUT); continue; }
        if ((details & V(c, V8_prop_kind_mask)) != V(c, V8_prop_kind_Data)) {
            if (scope == 3) gap(c, object, entry, 0, AS_CAPTURE_UNSUPPORTED);
            continue;
        }
        if (((details & V(c, V8_prop_location_mask)) >> V(c, V8_prop_location_shift)) == V(c, V8_prop_location_Descriptor)) {
            slot = entry + V(c, V8_prop_desc_value) * 8;
        } else {
            field_index = (details & V(c, V8_prop_index_mask)) >> V(c, V8_prop_index_shift);
            if (field_index < capacity) {
                if (!at(c, object, (start_words + field_index) * 8, &slot)) continue;
            } else {
                s64 length;
                u64 praw;
                if (!NEED(c, CF_V8_class_JSReceiver__raw_properties_or_hash__Tagged_Object_, CF_PROPERTY_LENGTH,
                    CF_PROPERTY_LENGTH_MASK, CF_PROPERTY_DATA, CF_V8_type_PropertyArray__PROPERTY_ARRAY_TYPE)) continue;
                if (!object_word(c, object, V(c, V8_class_JSReceiver__raw_properties_or_hash__Tagged_Object_), &props) ||
                    !header(c, props, &dmap, &dtype)) continue;
                if (dtype != V(c, V8_type_PropertyArray__PROPERTY_ARRAY_TYPE)) {
                    gap(c, object, 0, 0, AS_CAPTURE_UNSUPPORTED); continue;
                }
                if (!object_word(c, props, V(c, PROPERTY_LENGTH), &praw)) continue;
                if (!smi(c, praw, &length) || length < 0 ||
                    field_index - capacity >= (length & V(c, PROPERTY_LENGTH_MASK))) {
                    gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT); continue;
                }
                if (!at(c, props, V(c, PROPERTY_DATA) + (field_index - capacity) * 8, &slot)) continue;
            }
        }
        if (!word(c, slot, &value)) continue;
        property(c, object, key, value, slot, raw, scope, depth, false, 0);
    }
}
static void array(struct capture_walk *c, u64 object, unsigned int scope, unsigned int depth)
{
    u64 elements, map, raw, address;
    s64 length, capacity, i;
    u32 type;
    void *body;
    if (!NEED(c, CF_JS_ARRAY_LENGTH, CF_V8_class_JSObject__elements__Object, CF_V8_type_FixedArray__FIXED_ARRAY_TYPE,
        CF_V8_class_FixedArrayBase__length__SMI, CF_V8_class_FixedArray__data__uintptr_t)) return;
    if (!object_word(c, object, V(c, V8_class_JSObject__elements__Object), &elements) ||
        !header(c, elements, &map, &type) || type != V(c, V8_type_FixedArray__FIXED_ARRAY_TYPE) ||
        !object_word(c, object, V(c, JS_ARRAY_LENGTH), &raw) || !smi(c, raw, &length) ||
        !object_word(c, elements, V(c, V8_class_FixedArrayBase__length__SMI), &raw) || !smi(c, raw, &capacity)) {
        gap(c, object, 0, 0, AS_CAPTURE_UNSUPPORTED); return;
    }
    if (length < 0 || capacity < length || (u64)length > c->capacity / 32) {
        gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT); return;
    }
    body = reserve(c, AS_CAPTURE_ARRAY, 32); if (!body) return;
    put_unaligned_le64(object, body); put_unaligned_le64(elements, body + 8);
    put_unaligned_le64(length, body + 16); put_unaligned_le64(capacity, body + 24);
    for (i = 0; i < length && !c->full; i++) {
        if (!at(c, elements, V(c, V8_class_FixedArray__data__uintptr_t) + i * 8, &address) || !word(c, address, &raw)) break;
        body = reserve(c, AS_CAPTURE_MEMBER, 32); if (!body) break;
        put_unaligned_le64(object, body); put_unaligned_le64(i, body + 8);
        put_unaligned_le64(raw, body + 16); put_unaligned_le64(address, body + 24);
        if (scope == 3) project(c, raw, 3, depth + 1);
    }
}
static void project(struct capture_walk *c, u64 object, unsigned int scope, unsigned int depth)
{
    u64 map, address, raw;
    u32 type, units;
    u64 failures = c->failures;
    s64 number;
    void *body;
    if (c->full || seen(c, object, scope)) return;
    if (depth > 8) { gap(c, object, 0, 0, AS_CAPTURE_CAPACITY); return; }
    if (smi(c, object, &number)) {
        body = reserve(c, AS_CAPTURE_SMI, 16); if (!body) return;
        put_unaligned_le64(object, body); put_unaligned_le64(number, body + 8); return;
    }
    if (!header(c, object, &map, &type)) return;
    body = reserve(c, AS_CAPTURE_OBJECT, 24); if (!body) return;
    put_unaligned_le64(object, body); put_unaligned_le64(map, body + 8);
    put_unaligned_le32(type, body + 16); put_unaligned_le32(scope, body + 20);
    if ((scope == 1 || scope == 2) &&
        !(HAS(c, V8_type_JSObject__JS_OBJECT_TYPE) && type == V(c, V8_type_JSObject__JS_OBJECT_TYPE)) &&
        !(HAS(c, V8_APIObjectType) && type == V(c, V8_APIObjectType))) return;
    if (HAS(c, V8_FirstNonstringType) && type < V(c, V8_FirstNonstringType)) {
        u8 *text = string(c, object, &units);
        if (!text) return;
        kfree(text); return;
    }
    if (HAS(c, V8_type_HeapNumber__HEAP_NUMBER_TYPE) && type == V(c, V8_type_HeapNumber__HEAP_NUMBER_TYPE)) {
        if (!NEED(c, CF_V8_class_HeapNumber__value__double) || !object_word(c, object, V(c, V8_class_HeapNumber__value__double), &raw)) return;
        body = reserve(c, AS_CAPTURE_NUMBER, 16);
        if (body) { put_unaligned_le64(object, body); put_unaligned_le64(raw, body + 8); }
    } else if (HAS(c, V8_type_Oddball__ODDBALL_TYPE) && type == V(c, V8_type_Oddball__ODDBALL_TYPE)) {
        u32 semantic = 0;
        if (!NEED(c, CF_V8_class_Oddball__kind__int) || !object_word(c, object, V(c, V8_class_Oddball__kind__int), &raw) || !smi(c, raw, &number)) return;
        if (HAS(c, V8_OddballFalse) && number == (s64)V(c, V8_OddballFalse)) semantic = 1;
        else if (HAS(c, V8_OddballTrue) && number == (s64)V(c, V8_OddballTrue)) semantic = 2;
        else if (HAS(c, V8_OddballNull) && number == (s64)V(c, V8_OddballNull)) semantic = 3;
        else if (HAS(c, V8_OddballUndefined) && number == (s64)V(c, V8_OddballUndefined)) semantic = 4;
        body = reserve(c, AS_CAPTURE_ODDBALL, 24);
        if (body) { put_unaligned_le64(object, body); put_unaligned_le64(number, body + 8); put_unaligned_le32(semantic, body + 16); put_unaligned_le32(0, body + 20); }
    } else if ((scope == 3 || scope == 4) && HAS(c, V8_type_JSArray__JS_ARRAY_TYPE) && type == V(c, V8_type_JSArray__JS_ARRAY_TYPE)) {
        array(c, object, scope, depth);
    } else if (scope && ((HAS(c, V8_type_JSObject__JS_OBJECT_TYPE) && type == V(c, V8_type_JSObject__JS_OBJECT_TYPE)) ||
        (HAS(c, V8_APIObjectType) && type == V(c, V8_APIObjectType)))) {
        object_fields(c, object, map, scope, depth);
    } else if (scope == 3) {
        if (at(c, object, 0, &address)) gap(c, object, address, 0, AS_CAPTURE_UNSUPPORTED);
    }
    if (scope == 3) {
        body = reserve(c, AS_CAPTURE_END, 16);
        if (body) { put_unaligned_le64(object, body); put_unaligned_le32(scope, body + 8);
            put_unaligned_le32(c->failures == failures ? 1 : 0, body + 12); }
    }
}
static void frame(struct capture_walk *c, u64 object)
{
    u64 map, table, raw, address, first, stride;
    u32 type;
    s64 length, elements, deleted, buckets, bucket, index, next, live = 0;
    unsigned long *visited = NULL;
    void *body;
    if (!NEED(c, CF_V8_type_JSMap__JS_MAP_TYPE, CF_JS_COLLECTION_TABLE, CF_ORDERED_MAP_TYPE,
        CF_ORDERED_ELEMENTS, CF_ORDERED_DELETED, CF_ORDERED_BUCKETS, CF_ORDERED_ENTRY_VALUE, CF_ORDERED_ENTRY_SIZE,
        CF_V8_class_FixedArrayBase__length__SMI, CF_V8_class_FixedArray__data__uintptr_t)) return;
    if (!header(c, object, &map, &type) || type != V(c, V8_type_JSMap__JS_MAP_TYPE) ||
        !object_word(c, object, V(c, JS_COLLECTION_TABLE), &table) || !header(c, table, &map, &type) ||
        type != V(c, ORDERED_MAP_TYPE) ||
        !object_word(c, table, V(c, V8_class_FixedArrayBase__length__SMI), &raw) || !smi(c, raw, &length) ||
        !object_word(c, table, V(c, ORDERED_ELEMENTS), &raw) || !smi(c, raw, &elements) ||
        !object_word(c, table, V(c, ORDERED_DELETED), &raw) || !smi(c, raw, &deleted) ||
        !object_word(c, table, V(c, ORDERED_BUCKETS), &raw) || !smi(c, raw, &buckets)) {
        gap(c, object, 0, 0, AS_CAPTURE_UNSUPPORTED); return;
    }
    /* The actual CurrentKey getter supplies entry stride, cross-checked with
     * actual bucket/data/Smi getters. No compiled V8-version layout fallback. */
    stride = V(c, ORDERED_ENTRY_SIZE);
    first = V(c, ORDERED_BUCKETS) + 8;
    if (elements < 0 || deleted < 0 || buckets < 0 || length < 0 ||
        (u64)(elements + deleted) > c->capacity / 40 || (u64)buckets > c->capacity / 8 ||
        stride != 24 || first < V(c, V8_class_FixedArray__data__uintptr_t) ||
        first + (u64)buckets * 8 + (u64)(elements + deleted) * stride >
            V(c, V8_class_FixedArray__data__uintptr_t) + (u64)length * 8 ||
        V(c, ORDERED_ENTRY_VALUE) != first + 8) {
        gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT); return;
    }
    body = reserve(c, AS_CAPTURE_MAP, 48); if (!body) return;
    put_unaligned_le64(object, body); put_unaligned_le64(table, body + 8); put_unaligned_le64(length, body + 16);
    put_unaligned_le64(elements, body + 24); put_unaligned_le64(deleted, body + 32); put_unaligned_le64(buckets, body + 40);
    visited = bitmap_zalloc(elements + deleted, GFP_ATOMIC);
    if (elements + deleted && !visited) { gap(c, object, 0, 0, AS_CAPTURE_ALLOCATION); return; }
    for (bucket = 0; bucket < buckets && !c->full; bucket++) {
        if (!object_word(c, table, first + bucket * 8, &raw) || !smi(c, raw, &index)) break;
        while (index != -1 && !c->full) {
            u64 key, value;
            if (index < 0 || index >= elements + deleted || test_and_set_bit(index, visited)) {
                gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT); goto done;
            }
            if (!at(c, table, first + buckets * 8 + index * stride, &address) || !word(c, address, &key) ||
                !word(c, address + 8, &value) || !word(c, address + 16, &raw) || !smi(c, raw, &next)) goto done;
            body = reserve(c, AS_CAPTURE_ENTRY, 40); if (!body) goto done;
            put_unaligned_le64(object, body); put_unaligned_le64(index, body + 8); put_unaligned_le64(key, body + 16);
            put_unaligned_le64(value, body + 24); put_unaligned_le64(address, body + 32);
            live++; project(c, value, 1, 0); index = next;
        }
    }
done:
    if (live != elements) gap(c, object, 0, 0, AS_CAPTURE_INVALID_LAYOUT);
    bitmap_free(visited);
}
size_t as_runtime_capture_current(const u64 *available, const u64 *values, void *output, size_t capacity)
{
    struct capture_walk c;
    struct captured_read *read, *rnext;
    struct captured_node *node, *nnext;
    u64 tp = 0, isolate, value;
    u32 presence = 0;
    if (capacity < 64) return 0;
    memset(&c, 0, sizeof(c)); c.data = output; c.size = 64; c.capacity = capacity;
    c.available = available; c.values = values;
    memset(output, 0, 64);
    INIT_LIST_HEAD(&c.reads); INIT_LIST_HEAD(&c.nodes);
    c.read_index = RB_ROOT;
    put_unaligned_le32(task_tgid_nr(current), c.data); put_unaligned_le32(2, c.data + 4);
    put_unaligned_le64(current->group_leader->start_boottime, c.data + 8);
    put_unaligned_le64(ktime_get_boottime_ns(), c.data + 16);
#ifdef CONFIG_ARM64
    tp = read_sysreg(tpidr_el0); presence |= 1;
    put_unaligned_le64(tp, c.data + 32);
#endif
    if (!presence || !NEED(&c, CF_CURRENT_ISOLATE_TLS, CF_CONTINUATION_FRAME,
        CF_V8_SystemPointerSize, CF_V8_TaggedSize) || V(&c, V8_SystemPointerSize) != 8 || V(&c, V8_TaggedSize) != 8) {
        gap(&c, 0, 0, 0, AS_CAPTURE_UNSUPPORTED); goto done;
    }
    if (tp > U64_MAX - V(&c, CURRENT_ISOLATE_TLS) || !word(&c, tp + V(&c, CURRENT_ISOLATE_TLS), &isolate)) goto done;
    presence |= 2; put_unaligned_le64(isolate, c.data + 40);
    if (!isolate) { gap(&c, 0, tp + V(&c, CURRENT_ISOLATE_TLS), 0, AS_CAPTURE_NO_CURRENT_ISOLATE); goto done; }
    if (isolate > U64_MAX - V(&c, CONTINUATION_FRAME) || !word(&c, isolate + V(&c, CONTINUATION_FRAME), &value)) goto done;
    presence |= 4; put_unaligned_le64(value, c.data + 48);
    put_unaligned_le32(0, c.data + 4);
    frame(&c, value);
done:
    /* Recheck the complete acquired root/backing/field/byte read set while the
     * same owner callback and original current Frame remain active. */
    list_for_each_entry(read, &c.reads, link) {
        size_t offset;
        if (read->error) continue;
        for (offset = 0; offset < read->bytes; offset += 64) {
            u8 bytes[64];
            size_t count = min_t(size_t, 64, read->bytes - offset);
            int error = copy_from_user_nofault(bytes, (void __user *)(unsigned long)(read->address + offset), count);
            if (error) { status(&c, 3); gap(&c, 0, read->address + offset, error, AS_CAPTURE_READ_FAULT); break; }
            if (memcmp(bytes, read->data + offset, count)) { gap(&c, 0, read->address + offset, 0, AS_CAPTURE_CHANGED); break; }
        }
    }
#ifdef CONFIG_ARM64
    if (tp != read_sysreg(tpidr_el0)) gap(&c, 0, 0, 0, AS_CAPTURE_CHANGED);
#endif
    put_unaligned_le64(ktime_get_boottime_ns(), c.data + 24); put_unaligned_le32(presence, c.data + 60);
    list_for_each_entry_safe(read, rnext, &c.reads, link) { list_del(&read->link); kfree(read); }
    list_for_each_entry_safe(node, nnext, &c.nodes, link) { list_del(&node->link); kfree(node); }
    return c.size;
}
