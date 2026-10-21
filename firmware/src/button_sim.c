#include "button_sim.h"
#include <string.h>
static bool active(const pm_button_sim *s) { return s->phase==PM_SIM_STAGING || s->phase==PM_SIM_WAIT_CLOSE || s->phase==PM_SIM_RUNNING; }
static bool due(uint32_t now,uint32_t at) { return (int32_t)(now-at)>=0; }
void pm_sim_init(pm_button_sim *s,unsigned buttons) { memset(s,0,sizeof *s);s->buttons=buttons; }
void pm_sim_cancel(pm_button_sim *s,pm_sim_reason why,uint32_t now) {
    if(!active(s))return;
    /* Cancellation is NOT a normal release: reset without invoking the FSM. */
    memset(&s->virtual_button,0,sizeof s->virtual_button);s->raw_down=false;s->release_requested=false;
    for(unsigned i=0;i<s->count;i++)if(!s->steps[i].released){s->steps[i].cancelled=true;s->steps[i].released=true;s->steps[i].released_ms=now;}
    s->phase=PM_SIM_CANCELLED;s->reason=why;
}
pm_sim_result pm_sim_begin(pm_button_sim *s,uint64_t job,unsigned n,uint64_t session,unsigned state,uint32_t now) {
    if(active(s))return PM_SIM_BUSY;
    if(!pm_button_binding_at(s->buttons,0))return PM_SIM_UNSUPPORTED;
    if(!job || !n || n>PM_SIM_STEPS)return PM_SIM_INVALID;
    if(job<=s->last_job)return PM_SIM_OLD_JOB;
    unsigned buttons=s->buttons;bool connected=s->connected,ever=s->ever_connected;
    memset(s,0,sizeof *s);s->buttons=buttons;s->connected=connected;s->ever_connected=ever;
    s->id=s->last_job=job;s->count=n;s->session=session;s->expected_state=state;s->phase=PM_SIM_STAGING;s->lease_end_ms=now+10000;
    return PM_SIM_OK;
}
pm_sim_result pm_sim_step_set(pm_button_sim *s,uint64_t job,unsigned slot,uint32_t delay,unsigned key,uint32_t hold) {
    if(s->id!=job || s->phase!=PM_SIM_STAGING || slot>=s->count || key>=s->buttons || hold<80 || hold>PM_SIM_HOLD_MAX_MS || delay>PM_SIM_JOB_MAX_MS || hold+70>PM_SIM_JOB_MAX_MS-delay)return PM_SIM_INVALID;
    pm_sim_step *v=&s->steps[slot];
    if(v->present)return v->delay_ms==delay && v->key==key && v->hold_ms==hold?PM_SIM_OK:PM_SIM_CONFLICT;
    *v=(pm_sim_step){.delay_ms=delay,.hold_ms=hold,.key=key,.present=true,.logical=-1};return PM_SIM_OK;
}
pm_sim_result pm_sim_commit(pm_button_sim *s,uint64_t job,uint32_t now) {
    if(s->id!=job)return PM_SIM_INVALID;
    if(s->phase==PM_SIM_WAIT_CLOSE)return PM_SIM_OK;
    if(s->phase!=PM_SIM_STAGING || !s->connected)return PM_SIM_INVALID;
    for(unsigned i=0;i<s->count;i++){
        pm_sim_step *v=&s->steps[i];
        if(!v->present || (i && v->delay_ms<s->steps[i-1].delay_ms+s->steps[i-1].hold_ms+70))return PM_SIM_INVALID;
    }
    for(unsigned i=0;i<s->count;i++)s->steps[i].accepted=true;
    s->detached=true;s->phase=PM_SIM_WAIT_CLOSE;s->lease_end_ms=now+10000;return PM_SIM_OK;
}
pm_sim_result pm_sim_press(pm_button_sim *s,uint64_t job,unsigned key,uint32_t lease,uint64_t session,unsigned state,uint32_t now) {
    if(!pm_button_binding_at(s->buttons,0))return PM_SIM_UNSUPPORTED;
    if(!s->connected || key>=s->buttons || lease<80 || lease>PM_SIM_HOLD_MAX_MS)return PM_SIM_INVALID;
    pm_sim_result r=pm_sim_begin(s,job,1,session,state,now);if(r!=PM_SIM_OK)return r;
    s->steps[0]=(pm_sim_step){.key=key,.present=true,.accepted=true,.logical=-1};
    s->phase=PM_SIM_RUNNING;s->start_ms=now;s->raw_down=true;s->lease_end_ms=now+lease;return PM_SIM_OK;
}
pm_sim_result pm_sim_release(pm_button_sim *s,uint64_t job,uint32_t now) {
    if(s->id!=job)return PM_SIM_INVALID;
    if(s->phase==PM_SIM_DONE || s->phase==PM_SIM_CANCELLED || s->release_requested)return PM_SIM_OK;
    if(s->phase!=PM_SIM_RUNNING || s->detached)return PM_SIM_INVALID;
    s->raw_down=false;s->release_requested=true;s->release_ms=now;return PM_SIM_OK;
}
pm_sim_dispatch pm_sim_tick(pm_button_sim *s,bool connected,unsigned real_mask,bool allowed,uint64_t session,unsigned state,bool standby,uint32_t long_ms,uint32_t now) {
    pm_sim_dispatch out={0};bool falling=s->connected && !connected,rising=s->ever_connected && !s->connected && connected;
    s->connected=connected;s->ever_connected|=connected;
    if(real_mask){pm_sim_cancel(s,PM_SIM_CANCEL_PHYSICAL,now);return out;}
    if((rising && s->phase==PM_SIM_RUNNING && s->detached) || (falling && active(s) && !(s->phase==PM_SIM_WAIT_CLOSE && s->detached))){pm_sim_cancel(s,PM_SIM_CANCEL_USB,now);return out;}
    if(active(s) && (session!=s->session || state!=s->expected_state)){pm_sim_cancel(s,PM_SIM_CANCEL_STATE,now);return out;}
    if((s->phase==PM_SIM_STAGING || s->phase==PM_SIM_WAIT_CLOSE) && due(now,s->lease_end_ms)){pm_sim_cancel(s,PM_SIM_CANCEL_LEASE,now);return out;}
    if(s->phase==PM_SIM_WAIT_CLOSE && falling){s->phase=PM_SIM_RUNNING;s->start_ms=now+PM_SIM_CLOSE_GUARD_MS;}
    if(s->phase!=PM_SIM_RUNNING)return out;
    if(!s->detached && due(now,s->lease_end_ms)){pm_sim_cancel(s,PM_SIM_CANCEL_LEASE,now);return out;}
    if(s->detached && due(now,s->start_ms+PM_SIM_JOB_MAX_MS)){pm_sim_cancel(s,PM_SIM_CANCEL_LEASE,now);return out;}
    if(s->current>=s->count){s->phase=PM_SIM_DONE;return out;}
    pm_sim_step *v=&s->steps[s->current];
    if(s->detached){
        uint32_t press=s->start_ms+v->delay_ms,release=press+v->hold_ms;
        if(!due(now,press))return out;
        /* Never catch up an overdue press or normal release after BUSY. */
        if((!s->raw_down && !s->release_requested && (uint32_t)(now-press)>50) || (!s->release_requested && due(now,release) && (uint32_t)(now-release)>50)){
            pm_sim_cancel(s,PM_SIM_CANCEL_LEASE,now);return out;
        }
        if(!s->release_requested){s->raw_down=!due(now,release);if(!s->raw_down){s->release_requested=true;s->release_ms=now;}}
    }
    if(!allowed){if(s->raw_down || s->virtual_button.stable)pm_sim_cancel(s,PM_SIM_CANCEL_BUSY,now);return out;}
    int event=pm_button_input_step(&s->virtual_button,s->buttons,v->key,s->raw_down,now,long_ms,standby);
    if(event>=0)out=(pm_sim_dispatch){.valid=true,.logical=(unsigned)event,.step=s->current,.long_press=pm_button_event_is_long(s->buttons,v->key,(unsigned)event),.job=s->id};
    if(s->release_requested && !s->virtual_button.stable && !s->virtual_button.raw && due(now,s->release_ms+35)){
        v->released=true;v->released_ms=now;s->current++;s->release_requested=false;s->raw_down=false;memset(&s->virtual_button,0,sizeof s->virtual_button);
        if(s->current==s->count)s->phase=PM_SIM_DONE;
    }
    return out;
}
void pm_sim_dispatched(pm_button_sim *s,unsigned slot,unsigned logical,uint32_t now,uint64_t session,unsigned state) {
    if(slot>=s->count)return;
    s->steps[slot].dispatched=true;s->steps[slot].logical=(int)logical;s->steps[slot].dispatched_ms=now;
    /* A state transition caused by this very button is legitimate. */
    s->session=session;s->expected_state=state;
}
