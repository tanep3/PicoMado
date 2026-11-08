#pragma once
#include <stdbool.h>
#include <stdint.h>

/* BT-AUDIO-20261107: one live stream's bounded PCM shortage owner.
 * No IO or sample counters: those remain in the existing audio adapter. */
#define PM_BT_REBUFFER_TIMEOUT_MS 1500u
#define PM_BT_REBUFFER_HEALTHY_MS 5000u
#define PM_BT_REBUFFER_EPISODES 3u
typedef struct {
 uint64_t generation,stream;
 uint32_t since,healthy_since;
 unsigned episodes;
 bool shortage,buffering,resuming,healthy;
} pm_bt_rebuffer;
typedef enum { PM_BT_RB_READY,PM_BT_RB_WAIT,PM_BT_RB_RESUME,
 PM_BT_RB_TIMEOUT,PM_BT_RB_EXHAUSTED } pm_bt_rebuffer_result;
static inline void pm_bt_rebuffer_reset(pm_bt_rebuffer *s,uint64_t generation,uint64_t stream){
 *s=(pm_bt_rebuffer){.generation=generation,.stream=stream};
}
static inline bool pm_bt_rebuffer_current(const pm_bt_rebuffer *s,uint64_t generation,uint64_t stream){
 return stream&&s->stream==stream&&s->generation==generation;
}
/* Caller has verified the existing physical peer/CID/endpoint. Transfer only
 * the same live stream, preserving deadline and every budget field. */
static inline bool pm_bt_rebuffer_refresh(pm_bt_rebuffer *s,uint64_t old_generation,uint64_t new_generation,uint64_t stream){
 if(!new_generation||!pm_bt_rebuffer_current(s,old_generation,stream))return false;
 s->generation=new_generation;return true;
}
static inline pm_bt_rebuffer_result pm_bt_rebuffer_step(pm_bt_rebuffer *s,uint32_t now,
        bool ready,bool refilled,bool deadline_missed){
 if(!s->shortage){
  if(ready)return PM_BT_RB_READY;
  s->shortage=true;s->since=now;
 }
 if((uint32_t)(now-s->since)>=PM_BT_REBUFFER_TIMEOUT_MS)return PM_BT_RB_TIMEOUT;
 if(!s->buffering){
  if(ready&&!deadline_missed){s->shortage=false;return PM_BT_RB_READY;}
  if(!deadline_missed)return PM_BT_RB_WAIT;
  if(s->episodes>=PM_BT_REBUFFER_EPISODES)return PM_BT_RB_EXHAUSTED;
  s->episodes++;s->buffering=true;s->healthy=false;
 }
 if(refilled)s->resuming=true;
 return s->resuming?PM_BT_RB_RESUME:PM_BT_RB_WAIT;
}
static inline void pm_bt_rebuffer_sent(pm_bt_rebuffer *s,uint32_t now){
 if(s->resuming){s->shortage=s->buffering=s->resuming=false;s->healthy=false;}
 if(!s->healthy){s->healthy=true;s->healthy_since=now;}
 else if((uint32_t)(now-s->healthy_since)>=PM_BT_REBUFFER_HEALTHY_MS)s->episodes=0;
}
