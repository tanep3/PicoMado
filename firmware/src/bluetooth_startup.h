#pragma once
#include <stdbool.h>
#include <stdint.h>

#ifndef PM_BT_STARTUP_DIAGNOSTIC
#define PM_BT_STARTUP_DIAGNOSTIC 0
#endif
#if PM_BT_STARTUP_DIAGNOSTIC < 0 || PM_BT_STARTUP_DIAGNOSTIC > 2
#error PM_BT_STARTUP_DIAGNOSTIC must be 0, 1 or 2
#endif

/* Diagnostic rule only. No clocks, IO, PCB pointers or successful BT results. */
typedef struct {
    uint64_t connection;
    uint32_t ack,progress_ms;
    bool valid,progressed;
} pm_bt_startup_gate;
typedef struct {
    uint64_t connection;
    uint32_t ack,last_byte;
    bool available,unsent,unacked;
} pm_bt_startup_sample;
typedef enum { PM_BT_START_WAIT,PM_BT_START_BASELINE,PM_BT_START_RESET,
    PM_BT_START_PROGRESS,PM_BT_START_OPEN } pm_bt_startup_step;
static inline pm_bt_startup_step pm_bt_startup_observe(pm_bt_startup_gate *g,
        pm_bt_startup_sample s,uint32_t now){
    if(!s.available||!s.connection){
        bool had=g->valid;*g=(pm_bt_startup_gate){0};
        return had?PM_BT_START_RESET:PM_BT_START_WAIT;
    }
    if(!g->valid||g->connection!=s.connection){
        *g=(pm_bt_startup_gate){.connection=s.connection,.ack=s.ack,.valid=true};
        return PM_BT_START_BASELINE;
    }
    uint32_t delta=s.ack-g->ack;
    if(delta>=UINT32_C(0x80000000)){
        *g=(pm_bt_startup_gate){.connection=s.connection,.ack=s.ack,.valid=true};
        return PM_BT_START_RESET;
    }
    g->ack=s.ack;
    if(delta&&!g->progressed){
        g->progressed=true;g->progress_ms=now;return PM_BT_START_PROGRESS;
    }
    return g->progressed&&(uint32_t)(now-g->progress_ms)>=5000&&
        !s.unsent&&!s.unacked&&s.ack==s.last_byte?PM_BT_START_OPEN:PM_BT_START_WAIT;
}

/* First HCI lifecycle capture; callback writes fixed values under runtime lock. */
typedef struct {
    uint32_t begin_ms,end_ms,working_ms;
    int result;
    uint8_t seen;
} pm_bt_startup_times;
static inline void pm_bt_startup_capture(pm_bt_startup_times *t,unsigned event,
        uint32_t now,int result){
    if(t->seen&event)return;
    if(event==1)t->begin_ms=now;
    else if(event==2){t->end_ms=now;t->result=result;}
    else if(event==4)t->working_ms=now;
    else return;
    t->seen|=(uint8_t)event;
}
