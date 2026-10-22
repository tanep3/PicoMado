#pragma once
/* Fixed interference IO only; called by main USB service, never callbacks. */
#include "radio_query.h"
#include "radio_compare.h"
#include "roaming.h"
#include <string.h>
#include "pico/stdlib.h"
#include "pico/cyw43_arch.h"
static inline bool pm_radio_thread_context(void){
    return get_core_num()==0&&__get_current_exception()==0;
}
static inline pm_radio_read pm_radio_interference_get(bool available){
    if(!pm_radio_thread_context())return pm_radio_rejected(PM_RADIO_CONTEXT);
    if(!available)return pm_radio_rejected(PM_RADIO_UNAVAILABLE);
    /* PR151 889e4ccc: encoded GET_INTERFERENCE_MODE, 4-byte caller buffer,
     * STA interface. This SDK does not propagate CDC status/response length. */
    uint32_t raw=UINT32_MAX;
    cyw43_arch_lwip_begin();
    int result=cyw43_ioctl(&cyw43_state,0x1a6,sizeof raw,(uint8_t *)&raw,CYW43_ITF_STA);
    cyw43_arch_lwip_end();
    return pm_radio_decode(result,raw);
}

static inline pm_radio_change pm_radio_interference_change(pm_radio_comparison *state,pm_radio_change_op op){
 pm_radio_read before=pm_radio_rejected(PM_RADIO_UNAVAILABLE);
 if(!pm_radio_thread_context())return pm_radio_change_rejected(op,pm_radio_rejected(PM_RADIO_CONTEXT));
 if(op==PM_RADIO_TRY3&&!state->stale)before=pm_radio_interference_get(true);
 pm_radio_change plan=pm_radio_change_plan(state,op,before);
 if(plan.phase!=PM_CHANGE_WRITE)return plan;
 uint32_t target;
 if(!pm_interference_candidate_value(plan.target,&target))return pm_radio_change_rejected(op,before);
 cyw43_arch_lwip_begin();
 int rc=cyw43_ioctl(&cyw43_state,0x1a9,sizeof target,(uint8_t *)&target,CYW43_ITF_STA);
 cyw43_arch_lwip_end();
 // Even an error return may follow a device-side write. Keep baseline and
 // report a separate GET, never turn a write error into success via readback.
 return pm_radio_change_finish(plan,rc,pm_radio_interference_get(true));
}

/* PR151 fixed roam_off iovar only. No arbitrary iovar or raw IOCTL API. */
static inline pm_radio_read pm_radio_roaming_get(bool available){
 if(!pm_radio_thread_context())return pm_radio_rejected(PM_RADIO_CONTEXT);
 if(!available)return pm_radio_rejected(PM_RADIO_UNAVAILABLE);
 uint8_t buffer[13]="roam_off"; /* name+NUL+LE32, request prefix is invalid as value */
 cyw43_arch_lwip_begin();
 int rc=cyw43_ioctl(&cyw43_state,0x20c,sizeof buffer,buffer,CYW43_ITF_STA);
 cyw43_arch_lwip_end();
 uint32_t raw=(uint32_t)buffer[0]|((uint32_t)buffer[1]<<8)|((uint32_t)buffer[2]<<16)|((uint32_t)buffer[3]<<24);
 return pm_roaming_decode(rc,raw);
}
static inline pm_roaming_change pm_radio_roaming_change(pm_roaming_comparison *state,pm_roaming_op op){
 pm_radio_read before=pm_radio_rejected(PM_RADIO_UNAVAILABLE);
 if(!pm_radio_thread_context())return pm_roaming_rejected(op,pm_radio_rejected(PM_RADIO_CONTEXT));
 if(op==PM_ROAM_OFF&&!state->stale)before=pm_radio_roaming_get(true);
 pm_roaming_change plan=pm_roaming_plan(state,op,before);
 if(plan.phase!=PM_CHANGE_WRITE)return plan;
 uint32_t value;
 if(!pm_roaming_value(plan.target,&value))return pm_roaming_rejected(op,before);
 uint8_t buffer[13]="roam_off";buffer[9]=(uint8_t)value;
 cyw43_arch_lwip_begin();
 int rc=cyw43_ioctl(&cyw43_state,0x20f,sizeof buffer,buffer,CYW43_ITF_STA);
 cyw43_arch_lwip_end();
 plan.wrote=true;plan.write_result=rc;plan.read=pm_radio_roaming_get(true);plan.phase=PM_CHANGE_RETURNED;
 return plan;
}
