#pragma once
#include <stdbool.h>
#include <stdint.h>
#include "input_types.h"

/* BT-AUDIO-20261107. Pure AVRCP Target -> semantic input adapter. The caller
 * supplies the active connection generation, never an unchecked remote id.
 * A callback queues the resulting operation; it must not run UI/network IO. */
typedef struct {
    uint64_t generation;
    uint32_t pressed;
    bool connected;
} pm_bluetooth_input;

static inline void pm_bluetooth_input_reset(pm_bluetooth_input *s,
        uint64_t generation, bool connected) {
    if(!s)return;
    *s=(pm_bluetooth_input){.generation=generation,.connected=connected};
}

/* Mapping is fixed, not inferred from GPIO count. POWER is deliberately absent.
 * PLAY and PAUSE are alternate forms of the same semantic SUB operation. */
static inline bool pm_bluetooth_input_event(pm_bluetooth_input *s,
        uint64_t generation,uint8_t opcode,bool release,pm_operation *out) {
    if(!s || !out || !s->connected || generation!=s->generation)return false;
    unsigned key;pm_operation op;
    switch(opcode){
    case 0x44: key=0;op=(pm_operation){PM_OP_SUB};break; /* PLAY */
    case 0x46: key=1;op=(pm_operation){PM_OP_SUB};break; /* PAUSE */
    case 0x4b: key=2;op=(pm_operation){PM_OP_DOWN};break; /* FORWARD */
    case 0x4c: key=3;op=(pm_operation){PM_OP_UP};break; /* BACKWARD */
    default:return false;
    }
    uint32_t bit=1u<<key;
    if(release){s->pressed&=~bit;return false;}
    if(s->pressed&bit)return false;
    s->pressed|=bit;*out=op;return true;
}
