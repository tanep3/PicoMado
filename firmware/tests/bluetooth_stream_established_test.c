#include <assert.h>
#include <stdio.h>
#include "bluetooth_recovery.h"
#include "bluetooth_runtime.h"
static pm_bt_recovery recovery;
static uint8_t peer[6];static uint16_t cid,remote_cid;
static bool wanted;static uint32_t now;static unsigned disconnected,connected,failed,remote_calls,cancelled;
static bool recovery_releasing(void){return recovery.phase==PM_BR_RELEASE||recovery.phase==PM_BR_COOLDOWN;}
static void retry_cancel(void){++cancelled;}
static void recovery_cancel(void){pm_bt_recovery_cancel(&recovery);}
static void recovery_fail(uint16_t status){assert(status);recovery_cancel();if(!wanted)return;wanted=false;++failed;}
static uint32_t btstack_run_loop_get_time_ms(void){return now;}
static uint8_t a2dp_source_disconnect(uint16_t c){assert(c==cid);++disconnected;return 0;}
static uint8_t avrcp_connect(uint8_t *p,uint16_t *c){assert(p==peer);++remote_calls;*c=8;return 0;}
static void emit(pm_bt_event e){assert(e.kind==PM_BT_CONNECTED);++connected;}
#include "../src/bluetooth_stream_established.inc"
static void reset(void){wanted=true;cid=4;remote_cid=0;now=0;disconnected=connected=failed=remote_calls=cancelled=0;pm_bt_recovery_start(&recovery,1,peer,0);}
int main(void){
 for(unsigned phase=PM_BR_RELEASE;phase<=PM_BR_COOLDOWN;phase++){reset();recovery.phase=(pm_bt_recovery_phase)phase;handle_stream_established(4,0);assert(!connected&&!failed&&disconnected==1&&recovery.phase==(pm_bt_recovery_phase)phase);handle_stream_established(4,11);assert(!failed);}
 reset();handle_stream_established(3,0);handle_stream_established(3,11);assert(!connected&&!failed&&!disconnected);
 reset();wanted=false;handle_stream_established(4,0);assert(!connected&&disconnected==1);
 reset();now=25000;handle_stream_established(4,0);assert(failed==1&&!connected&&disconnected==1);handle_stream_established(4,0);assert(failed==1&&!connected);
 reset();now=24999;handle_stream_established(4,0);assert(connected==1&&remote_calls==1&&recovery.phase==PM_BR_OFF);
 reset();handle_stream_established(4,11);handle_stream_established(4,11);handle_stream_established(4,0);assert(failed==1&&!connected&&disconnected==1&&recovery.phase==PM_BR_OFF);
 puts("Bluetooth stream handler: teardown late success, stale CID, deadline, terminal once PASS");
}
