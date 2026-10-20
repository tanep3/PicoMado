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
    if (pressed != b->stable && (uint32_t)(now_ms - b->changed_ms) >= 35) {
        b->stable = pressed;
        return pressed;
    }
    return false;
}

bool pm_button_wakes(unsigned count,unsigned index,bool long_press){
    return (count==4 && index==2) || (count==2 && index==1 && long_press);
}
bool pm_button_wake_step(pm_button *b,unsigned count,unsigned index,
                         bool pressed,uint32_t now,uint32_t long_ms){
    bool edge=pm_button_step(b,pressed,now);
    if(edge){b->held_ms=now;b->wake_reported=false;b->wake_armed=true;}
    if(!b->stable){b->wake_reported=false;b->wake_armed=false;return false;}
    if(count==4)return edge && pm_button_wakes(count,index,false);
    if(count!=2 || !long_ms || !b->wake_armed || b->wake_reported || !pressed ||
       (uint32_t)(now-b->held_ms)<long_ms)return false;
    if(!pm_button_wakes(count,index,true))return false;
    b->wake_reported=true;return true;
}

int pm_button_input_step(pm_button *b,unsigned count,unsigned index,bool pressed,
                         uint32_t now,uint32_t long_ms,bool standby){
    bool was_stable=b->stable;
    bool edge=pm_button_step(b,pressed,now);
    if(count==4)return edge && (!standby || index==2)?(int)index:-1;
    if(count!=2 || index>=2)return -1;
    if(edge){b->held_ms=now;b->press_long_ms=long_ms;b->long_reported=false;b->wake_armed=true;}
    if(b->stable && pressed && b->wake_armed && !b->long_reported &&
       (uint32_t)(now-b->held_ms)>=b->press_long_ms){
        b->long_reported=true;
        return !standby || index==1?(index==0?0:2):-1;
    }
    if(was_stable && !b->stable){
        bool short_press=b->wake_armed && !b->long_reported;
        b->wake_armed=false;
        return short_press && !standby?(index==0?1:3):-1;
    }
    return -1;
}
