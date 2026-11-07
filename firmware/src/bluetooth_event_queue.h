#pragma once
#include "bluetooth_runtime.h"
/* Metadata is expendable; terminal/control state must not be displaced by
 * discovery chatter. At all-terminal saturation retain the newest states. */
typedef struct {pm_bt_event entries[8];unsigned count;uint32_t dropped;} pm_bt_event_queue;
static inline bool pm_bt_event_terminal(pm_bt_event_kind k){return k==PM_BT_ERROR||k==PM_BT_CONNECTED||k==PM_BT_DISCONNECTED||k==PM_BT_STOPPED;}
static inline void pm_bt_event_drop(pm_bt_event_queue *q){if(q->dropped!=UINT32_MAX)++q->dropped;}
static inline void pm_bt_event_enqueue(pm_bt_event_queue *q,pm_bt_event e){
 if(!pm_bt_event_terminal(e.kind)&&q->count>=4){pm_bt_event_drop(q);return;}
 if(q->count==8){
  unsigned victim=0;
  while(victim<8&&pm_bt_event_terminal(q->entries[victim].kind))++victim;
  if(victim==8)victim=0; /* finite capacity: retain newest terminal state */
  memmove(&q->entries[victim],&q->entries[victim+1],(7-victim)*sizeof e);--q->count;pm_bt_event_drop(q);
 }
 q->entries[q->count++]=e;
}
static inline bool pm_bt_event_dequeue(pm_bt_event_queue *q,pm_bt_event *e){
 if(!e||!q->count)return false;
 *e=q->entries[0];--q->count;memmove(q->entries,q->entries+1,q->count*sizeof *e);return true;
}
