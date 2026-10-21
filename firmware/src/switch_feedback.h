#pragma once
#include "core.h"
#include <limits.h>
#include <string.h>

/* Pure local presentation state. No IO, clock reads, plugin IDs or pixels.
 * Current server replies once per input, in connection order. Each accepted
 * display change advances the epoch; a no-op consumes a reply without doing so.
 * This lower bound keeps an earlier click's image from completing a later one. */
#define PM_SWITCH_DELAY_US UINT64_C(200000)
typedef struct {
    uint64_t sent, replied, started_us, base_epoch, minimum_epoch, minimum_frame;
    uint64_t incarnation, session, owner;
    pm_token received;
    bool waiting, received_valid, hint_requested;
} pm_switch_feedback;
static inline void pm_switch_reset(pm_switch_feedback *s){memset(s,0,sizeof *s);}
static inline bool pm_switch_scope(const pm_switch_feedback *s,pm_token t){
    return t.incarnation==s->incarnation && t.session_epoch==s->session && t.screen_owner_epoch==s->owner;
}
static inline bool pm_switch_eligible(const pm_switch_feedback *s,pm_token t){
    return !s->waiting || (pm_switch_scope(s,t) && t.display_epoch>=s->minimum_epoch && t.frame_id>s->minimum_frame);
}
static inline bool pm_switch_received(const pm_switch_feedback *s){
    return s->waiting && s->received_valid && pm_switch_eligible(s,s->received);
}
static inline void pm_switch_cancel(pm_switch_feedback *s){
    s->waiting=s->received_valid=s->hint_requested=false;
}
static inline void pm_switch_sent(pm_switch_feedback *s,pm_token current,uint64_t now,bool visual){
    if(!pm_switch_scope(s,current))pm_switch_reset(s);
    if(!s->waiting && s->sent==s->replied)s->sent=s->replied=0;
    if(s->sent==UINT64_MAX){pm_switch_cancel(s);return;}
    s->incarnation=current.incarnation;s->session=current.session_epoch;s->owner=current.screen_owner_epoch;
    s->sent++;
    if(!visual){pm_switch_cancel(s);return;}
    uint64_t floor=current.display_epoch;
    if(s->waiting && s->minimum_epoch>floor)floor=s->minimum_epoch;
    s->started_us=now;s->base_epoch=current.display_epoch;
    s->minimum_epoch=floor==UINT64_MAX?floor:floor+1;
    s->minimum_frame=current.frame_id;s->received_valid=false;s->waiting=true;
    /* A second click reuses an already visible/requested identical hint. */
}
/* true means the latest request was explicitly a same-epoch no-op. */
static inline bool pm_switch_reply(pm_switch_feedback *s,uint64_t epoch,bool has_epoch){
    if(s->replied>=s->sent)return false;
    s->replied++;
    if(!s->waiting || !has_epoch)return false;
    uint64_t remaining=s->sent-s->replied;
    s->minimum_epoch=epoch>UINT64_MAX-remaining?UINT64_MAX:epoch+remaining;
    if(!remaining && epoch<=s->base_epoch && !pm_switch_received(s)){
        pm_switch_cancel(s);return true;
    }
    return false;
}
static inline void pm_switch_receive(pm_switch_feedback *s,pm_token t){
    if(!s->waiting || !pm_switch_scope(s,t) || t.frame_id<=s->minimum_frame)return;
    if(!s->received_valid || t.display_epoch>s->received.display_epoch ||
       (t.display_epoch==s->received.display_epoch && t.frame_id>s->received.frame_id)){
        s->received=t;s->received_valid=true;
    }
}
/* The caller discards assembly ownership; independently copied video/driver
 * pixels are not passed here. A remembered token never outlives its pixels. */
static inline void pm_switch_forget(pm_switch_feedback *s,pm_token t){
    if(s->received_valid && pm_token_equal(s->received,t))s->received_valid=false;
}
static inline bool pm_switch_due(const pm_switch_feedback *s,uint64_t now){
    return s->waiting && !s->hint_requested && !pm_switch_received(s) &&
        now>=s->started_us && now-s->started_us>=PM_SWITCH_DELAY_US;
}
static inline bool pm_switch_complete(pm_switch_feedback *s,pm_token t){
    if(!s->waiting || !pm_switch_eligible(s,t))return false;
    pm_switch_cancel(s);return true;
}
_Static_assert(sizeof(pm_switch_feedback)<=128,"feedback owns bounded metadata only");
