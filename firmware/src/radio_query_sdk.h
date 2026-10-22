#pragma once
/* SDK IO adapter; only the main USB service calls it. No SET/variable ioctl. */
#include "radio_query.h"
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
