#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
/* Media operations are intentionally distinct from GPIO pm_operation. */
typedef enum { PM_MEDIA_TOGGLE, PM_MEDIA_NEXT, PM_MEDIA_PREVIOUS, PM_MEDIA_CANCEL,
    PM_MEDIA_VOLUME_UP, PM_MEDIA_VOLUME_DOWN } pm_media_operation;
static inline const char *pm_media_operation_name(pm_media_operation op) {
    switch(op) {
    case PM_MEDIA_TOGGLE:return "toggle_playback";
    case PM_MEDIA_NEXT:return "channel_next";
    case PM_MEDIA_PREVIOUS:return "channel_previous";
    case PM_MEDIA_CANCEL:return "cancel";
    case PM_MEDIA_VOLUME_UP:return "volume_up";
    case PM_MEDIA_VOLUME_DOWN:return "volume_down";
    default:return NULL;
    }
}
typedef struct { uint64_t incarnation,session; bool enabled; } pm_media_input_session;
static inline void pm_media_input_welcome(pm_media_input_session *s,
        uint64_t incarnation,uint64_t session,uint64_t version) {
    *s=(pm_media_input_session){incarnation,session,version==1};
}
static inline bool pm_media_input_allowed(const pm_media_input_session *s,
        uint64_t incarnation,uint64_t session,bool active) {
    return active && s->enabled && s->incarnation==incarnation && s->session==session;
}

/* BT-AUDIO-20261107. Pure AVRCP Target -> semantic input adapter. The caller
 * supplies the active connection generation, never an unchecked remote id.
 * A callback queues the resulting operation; it must not run UI/network IO. */
typedef struct {
    uint64_t generation;
    uint32_t pressed,pressed_ms[6];
    bool connected;
} pm_bluetooth_input;

static inline void pm_bluetooth_input_reset(pm_bluetooth_input *s,
        uint64_t generation, bool connected) {
    if(!s)return;
    *s=(pm_bluetooth_input){.generation=generation,.connected=connected};
}

#define PM_BT_INPUT_LONG_MS 1800u
#define PM_BT_INPUT_STALE_MS 10000u
static inline void pm_bluetooth_input_expire(pm_bluetooth_input *s,uint32_t now) {
    for(unsigned i=0;i<6;i++)if((s->pressed&(1u<<i)) &&
            (uint32_t)(now-s->pressed_ms[i])>=PM_BT_INPUT_STALE_MS)s->pressed&=~(1u<<i);
}
/* Callback timestamps, not queue processing time, determine hold duration. */
static inline bool pm_bluetooth_input_event(pm_bluetooth_input *s,
        uint64_t generation,uint8_t opcode,bool release,uint32_t received_ms,
        uint32_t now,pm_media_operation *out) {
    if(!s || !out || !s->connected || generation!=s->generation)return false;
    pm_bluetooth_input_expire(s,now);
    if((uint32_t)(now-received_ms)>=PM_BT_INPUT_STALE_MS)return false;
    unsigned key;pm_media_operation op;
    switch(opcode){
    case 0x44:key=0;op=PM_MEDIA_TOGGLE;break;
    case 0x46:key=1;op=PM_MEDIA_TOGGLE;break;
    case 0x4b:key=2;op=PM_MEDIA_NEXT;break;
    case 0x4c:key=3;op=PM_MEDIA_PREVIOUS;break;
    case 0x41:key=4;op=PM_MEDIA_VOLUME_UP;break;
    case 0x42:key=5;op=PM_MEDIA_VOLUME_DOWN;break;
    default:return false;
    }
    uint32_t bit=1u<<key;
    if(release){
        bool held=(s->pressed&bit)!=0;s->pressed&=~bit;
        if(!held||key>=2)return false;
        uint32_t elapsed=received_ms-s->pressed_ms[key];
        if(elapsed>=PM_BT_INPUT_STALE_MS)return false;
        *out=elapsed>=PM_BT_INPUT_LONG_MS?PM_MEDIA_CANCEL:PM_MEDIA_TOGGLE;
        return true;
    }
    if(s->pressed&bit)return false;
    s->pressed|=bit;s->pressed_ms[key]=received_ms;
    if(key<2)return false;
    *out=op;return true;
}
