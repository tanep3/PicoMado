#pragma once
#include "core.h"

// Pixels remain in the existing assembly. This model owns only the matching
// token/policy and cannot perform display, network, clock or logging IO.
typedef struct {
    pm_token token;
    uint64_t refresh_minute;
    unsigned refresh_minutes;
} pm_eink_frame;
typedef enum { PM_EINK_EMPTY, PM_EINK_RECEIVING, PM_EINK_READY } pm_eink_pending_phase;
typedef struct { pm_eink_frame frame; pm_eink_pending_phase phase; } pm_eink_pending;

static inline void pm_eink_pending_reset(pm_eink_pending *p){p->phase=PM_EINK_EMPTY;}
// Call only after side-effect-free header validation and assembly acceptance.
static inline void pm_eink_pending_start(pm_eink_pending *p,pm_token token,unsigned minutes,uint64_t minute){
    p->frame=(pm_eink_frame){.token=token,.refresh_minutes=minutes,.refresh_minute=minute};
    p->phase=PM_EINK_RECEIVING;
}
static inline bool pm_eink_pending_complete(pm_eink_pending *p,pm_token token){
    if(p->phase!=PM_EINK_RECEIVING || !pm_token_equal(p->frame.token,token))return false;
    p->phase=PM_EINK_READY;return true;
}
static inline bool pm_eink_pending_take(pm_eink_pending *p,bool idle,bool allowed,
    uint64_t incarnation,uint64_t session,uint64_t display,uint64_t owner,pm_eink_frame *frame){
    if(p->phase!=PM_EINK_READY)return false;
    if(!allowed || !pm_token_current(p->frame.token,incarnation,session,display,owner)){
        pm_eink_pending_reset(p);return false;
    }
    if(!idle)return false;
    *frame=p->frame;pm_eink_pending_reset(p);return true;
}
_Static_assert(sizeof(pm_eink_pending)<=64,"E-ink pending must not add a pixel buffer");
