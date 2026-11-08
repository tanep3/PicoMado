#include <assert.h>
#include <stdio.h>
#include "bluetooth_rebuffer.h"
int main(void){
 pm_bt_rebuffer s;pm_bt_rebuffer_reset(&s,7,9);
 assert(pm_bt_rebuffer_current(&s,7,9)&&!pm_bt_rebuffer_current(&s,8,9));
 assert(pm_bt_rebuffer_step(&s,100,false,false,false)==PM_BT_RB_WAIT);
 assert(pm_bt_rebuffer_step(&s,110,true,false,false)==PM_BT_RB_READY&&!s.shortage&&!s.episodes);
 for(unsigned i=0;i<3;i++){
  uint32_t now=1000+i*100;
  assert(pm_bt_rebuffer_step(&s,now,false,false,true)==PM_BT_RB_WAIT);
  assert(pm_bt_rebuffer_step(&s,now+20,true,false,true)==PM_BT_RB_WAIT);
  assert(pm_bt_rebuffer_step(&s,now+80,true,true,true)==PM_BT_RB_RESUME);
  pm_bt_rebuffer_sent(&s,now+81);assert(!s.shortage&&!s.buffering&&s.episodes==i+1);
 }
 assert(pm_bt_rebuffer_step(&s,1400,false,false,true)==PM_BT_RB_EXHAUSTED);
 pm_bt_rebuffer_reset(&s,7,10);
 assert(pm_bt_rebuffer_step(&s,UINT32_MAX-100,false,false,true)==PM_BT_RB_WAIT);
 assert(pm_bt_rebuffer_step(&s,1398,true,true,true)==PM_BT_RB_RESUME);
 assert(pm_bt_rebuffer_step(&s,1399,true,true,true)==PM_BT_RB_TIMEOUT);
 pm_bt_rebuffer_reset(&s,7,11);s.episodes=3;pm_bt_rebuffer_sent(&s,100);
 pm_bt_rebuffer_sent(&s,5099);assert(s.episodes==3);pm_bt_rebuffer_sent(&s,5100);assert(!s.episodes);
 puts("Bluetooth rebuffer policy: timely refill, full refill, deadline, repeat limit, healthy reset, scope and wrap PASS");
}
