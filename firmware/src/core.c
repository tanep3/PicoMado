#include "core.h"
#include <string.h>

uint32_t pm_crc32(const uint8_t *bytes, size_t len) {
    uint32_t crc = 0xffffffffu;
    for (size_t i = 0; i < len; ++i) {
        crc ^= bytes[i];
        for (unsigned b = 0; b < 8; ++b)
            crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1u)));
    }
    return ~crc;
}

void pm_reset(pm_assembly *a) { a->active = false; a->used = 0; }

bool pm_token_equal(pm_token a, pm_token b) {
    return a.incarnation == b.incarnation && a.session_epoch == b.session_epoch &&
           a.display_epoch == b.display_epoch && a.screen_owner_epoch == b.screen_owner_epoch &&
           a.frame_id == b.frame_id;
}

bool pm_token_current(pm_token t, uint64_t i, uint64_t s, uint64_t d, uint64_t o) {
    return t.incarnation == i && t.session_epoch == s &&
           t.display_epoch == d && t.screen_owner_epoch == o;
}

bool pm_frame_header_valid(uint64_t width,uint64_t height,const char *format,
                           uint64_t length,uint64_t crc){
    return width==PM_W && height==PM_H && format && !strcmp(format,PM_PIXEL_FORMAT_NAME) &&
        length==PM_IMAGE_BYTES && crc<=UINT32_MAX;
}
bool pm_start(pm_assembly *a, pm_token token, unsigned width, unsigned height,
              const char *format, uint32_t length, uint32_t crc) {
    pm_reset(a);
    if (!pm_frame_header_valid(width,height,format,length,crc)) return false;
    a->token = token; a->expected = length; a->crc = crc; a->active = true;
    return true;
}

bool pm_chunk(pm_assembly *a, pm_token token, uint32_t offset,
              const uint8_t *bytes, size_t length, bool *complete) {
    *complete = false;
    if (!a->active || !pm_token_equal(a->token, token) || offset != a->used ||
        !length || length > PM_FRAGMENT_BYTES || length > a->expected - a->used) {
        pm_reset(a); return false;
    }
    memcpy(a->pixels + a->used, bytes, length);
    a->used += length;
    if (a->used == a->expected) {
        if (pm_crc32(a->pixels, a->used) != a->crc) { pm_reset(a); return false; }
        *complete = true;
        a->active = false;
    }
    return true;
}

bool pm_button_step(pm_button *b, bool pressed, uint32_t now_ms) {
    if (pressed != b->raw) { b->raw = pressed; b->changed_ms = now_ms; }
    if (pressed != b->stable && (uint32_t)(now_ms - b->changed_ms) >= PM_BUTTON_DEBOUNCE_MS) {
        b->stable = pressed;
        return pressed;
    }
    return false;
}

// Composition data for the existing boards; gesture processing below has no
// knowledge of a button count. Custom layouts can call the same processor.
static const pm_button_binding four_bindings[]={
    {{PM_OP_SUB},{0},true,false,false},{{PM_OP_UP},{0},true,false,false},
    {{PM_OP_POWER},{0},true,true,true},{{PM_OP_DOWN},{0},true,false,false}
};
static const pm_button_binding two_bindings[]={
    {{PM_OP_UP},{PM_OP_SUB},false,false,false},
    {{PM_OP_DOWN},{PM_OP_POWER},false,false,true}
};
const pm_button_binding *pm_button_binding_at(unsigned count,unsigned index){
    const pm_button_binding *bindings=count==4?four_bindings:count==2?two_bindings:NULL;
    return bindings&&index<count?&bindings[index]:NULL;
}
bool pm_button_event_is_long(unsigned count,unsigned index,unsigned logical){
    const pm_button_binding *b=pm_button_binding_at(count,index);
    return b&&!b->on_press&&pm_operation_index(b->long_operation)==logical;
}
bool pm_button_wakes(unsigned count,unsigned index,bool long_press){
    const pm_button_binding *b=pm_button_binding_at(count,index);
    return b&&(long_press?b->long_wakes:b->short_wakes);
}
bool pm_button_wake_step(pm_button *b,unsigned count,unsigned index,
                         bool pressed,uint32_t now,uint32_t long_ms){
    const pm_button_binding *binding=pm_button_binding_at(count,index);if(!binding)return false;
    bool edge=pm_button_step(b,pressed,now);
    if(edge){b->held_ms=now;b->wake_reported=false;b->wake_armed=true;}
    if(!b->stable){b->wake_reported=false;b->wake_armed=false;return false;}
    if(binding->on_press)return edge&&binding->short_wakes;
    if(!long_ms||!b->wake_armed||b->wake_reported||!pressed||
       (uint32_t)(now-b->held_ms)<long_ms||!binding->long_wakes)return false;
    b->wake_reported=true;return true;
}
bool pm_button_binding_step(pm_button *b,const pm_button_binding *binding,bool pressed,
                         uint32_t now,uint32_t long_ms,bool standby,
                         pm_operation *operation,bool *long_press){
    if(!binding||!operation||!long_press||!pm_operation_valid(binding->short_operation))return false;
    bool was_stable=b->stable,edge=pm_button_step(b,pressed,now);
    if(binding->on_press){
        if(!edge||(standby&&!binding->short_wakes))return false;
        *operation=binding->short_operation;*long_press=false;return true;
    }
    if(edge){b->held_ms=now;b->press_long_ms=long_ms;b->long_reported=false;b->wake_armed=true;}
    if(b->stable&&pressed&&b->wake_armed&&!b->long_reported&&
       pm_operation_valid(binding->long_operation)&&(uint32_t)(now-b->held_ms)>=b->press_long_ms){
        b->long_reported=true;
        if(standby&&!binding->long_wakes)return false;
        *operation=binding->long_operation;*long_press=true;return true;
    }
    if(was_stable&&!b->stable){
        bool short_press=b->wake_armed&&!b->long_reported;b->wake_armed=false;
        if(short_press&&(!standby||binding->short_wakes)){
            *operation=binding->short_operation;*long_press=false;return true;
        }
    }
    return false;
}
bool pm_button_sampled_binding_step(pm_button *b,const pm_button_binding *binding,
                         bool pressed,uint32_t now,uint32_t long_ms,bool standby,
                         pm_operation *operation,bool *long_press){
    if(!b||!binding||!operation||!long_press||!pm_operation_valid(binding->short_operation))return false;
    bool gap=b->sample_seen&&(uint32_t)(now-b->sampled_ms)>PM_BUTTON_MAX_SAMPLE_GAP_MS;
    b->sampled_ms=now;b->sample_seen=true;
    if(gap){
        /* No action, including a short release, survives an unobserved interval.
         * Keep only the new raw fact; do not invent a stable pressed level. */
        *b=(pm_button){.raw=pressed,.changed_ms=now,.sampled_ms=now,
            .sample_seen=true,.release_required=true};
        return false;
    }
    if(b->release_required){
        if(pressed!=b->raw){b->raw=pressed;b->changed_ms=now;}
        if(!pressed&&(uint32_t)(now-b->changed_ms)>=PM_BUTTON_DEBOUNCE_MS)
            b->release_required=false;
        return false;
    }
    return pm_button_binding_step(b,binding,pressed,now,long_ms,standby,operation,long_press);
}
int pm_button_input_step(pm_button *b,unsigned count,unsigned index,bool pressed,
                         uint32_t now,uint32_t long_ms,bool standby){
    pm_operation operation;bool held;
    return pm_button_binding_step(b,pm_button_binding_at(count,index),pressed,now,long_ms,
        standby,&operation,&held)?(int)pm_operation_index(operation):-1;
}
