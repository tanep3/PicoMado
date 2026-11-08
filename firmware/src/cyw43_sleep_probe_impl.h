#include "cyw43_sleep_probe.h"
static pm_sleep_status pm_sleep_state;
static void pm_sleep_inc(uint32_t *n){if(*n!=UINT32_MAX)++*n;}
void pm_sleep_get(pm_sleep_status *out){*out=pm_sleep_state;}
static void pm_sleep_kso(bool wake,bool ok,unsigned loops,int read_value,uint32_t now){
 pm_sleep_inc(ok?&pm_sleep_state.success[wake]:&pm_sleep_state.failure[wake]);
 if(loops>pm_sleep_state.max_loops)pm_sleep_state.max_loops=loops;
 pm_sleep_state.last_read=read_value;
 if(!ok)pm_sleep_state.last_failure_us=now;
}
static void pm_sleep_transition(bool before,bool after,uint32_t start,uint32_t end){
 pm_sleep_inc(after?&pm_sleep_state.wake:&pm_sleep_state.sleep);
 pm_sleep_state.before=before;pm_sleep_state.after=after;pm_sleep_state.last_us=end;pm_sleep_state.duration_us=end-start;
}
