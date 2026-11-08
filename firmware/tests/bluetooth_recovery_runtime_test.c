#include <assert.h>
#include <stdio.h>
#include "bluetooth_recovery.h"
#include "bluetooth_runtime.h"
#define A2DP_W4_DISCOVER_SEPS 3
#define AVDTP_SIGNALING_CONNECTION_INITIATOR_W4_ANSWER 9
#define A2DP_STREAMING_OPENED 17
#define A2DP_W4_SET_CONFIGURATION 8
#define AVDTP_SIGNALING_CONNECTION_OPENED 7
#define ERROR_CODE_ACL_CONNECTION_ALREADY_EXISTS 11
typedef struct {int state,initiator_config_state,acceptor_config_state;uint8_t seid;} avdtp_stream_endpoint_t;
static uint8_t avdtp_stream_endpoint_seid(avdtp_stream_endpoint_t *e){return e->seid;}
struct fake_connection {struct {int state;avdtp_stream_endpoint_t *local_stream_endpoint;} a2dp_source_config_process;int initiator_connection_state,state,configuration_state;uint8_t initiator_transaction_label,acceptor_transaction_label;struct {uint8_t signal_identifier,transaction_label;} initiator_signaling_packet,acceptor_signaling_packet;void *active_stream_endpoint;};
static avdtp_stream_endpoint_t endpoint;
typedef struct fake_connection avdtp_connection_t;
typedef struct {unsigned unused;} btstack_timer_source_t;
static pm_bt_recovery recovery;static uint64_t generation;static uint8_t peer[6];
static bool wanted,stopping,owner_exists,retry_pending;
static uint16_t cid,remote_cid;static uint32_t now,rate;
static void *encoder;static btstack_timer_source_t recovery_timer;
static avdtp_connection_t owner;
static unsigned disconnects,remote_disconnects,establishes,failures,reports,timers,cancels;
static uint8_t disconnect_result,establish_result;
static bool recovery_releasing(void){return recovery.phase==PM_BR_RELEASE||recovery.phase==PM_BR_COOLDOWN;}
static void recovery_cancel(void){pm_bt_recovery_cancel(&recovery);}
static void retry_cancel(void){retry_pending=false;++cancels;}
static void recovery_fail(uint16_t status);
static pm_bt_event terminal;
static void retry_schedule(void){assert(!recovery_releasing());if(pm_bt_recovery_reserve(&recovery))retry_pending=true;else recovery_fail(11);}
static uint32_t btstack_run_loop_get_time_ms(void){return now;}
static avdtp_connection_t *avdtp_get_connection_for_bd_addr(uint8_t *p){assert(p==peer);return owner_exists?&owner:NULL;}
static avdtp_connection_t *avdtp_get_connection_for_avdtp_cid(uint16_t c){assert(c==cid);return owner_exists?&owner:NULL;}
static uint8_t a2dp_source_disconnect(uint16_t c){assert(c==cid);++disconnects;return disconnect_result;}
static uint8_t avrcp_disconnect(uint16_t c){assert(c==remote_cid);++remote_disconnects;return 0;}
static uint8_t a2dp_source_establish_stream(uint8_t *p,uint16_t *out){assert(p==peer&&!owner_exists&&!cid&&!remote_cid);++establishes;*out=9;return establish_result;}
static void emit(pm_bt_event e){if(e.kind==PM_BT_ERROR){terminal=e;++failures;return;}if(e.kind==PM_BT_DISCONNECTED||e.kind==PM_BT_STOPPED)++reports;else assert(e.kind==PM_BT_LINK_DIAGNOSTIC);}
static void media_stop(void){}
static void stopped_if_idle(void){}
static void btstack_run_loop_set_timer(btstack_timer_source_t *t,uint32_t delay){assert(t==&recovery_timer&&delay==250);}
static void btstack_run_loop_add_timer(btstack_timer_source_t *t){assert(t==&recovery_timer);++timers;}
#include "../src/bluetooth_recovery_runtime.inc"
#include "../src/bluetooth_signaling_released.inc"
static void reset(void){generation=1;wanted=owner_exists=true;stopping=retry_pending=false;cid=4;remote_cid=8;now=0;memset(&owner,0,sizeof owner);memset(&endpoint,0,sizeof endpoint);owner.state=7;owner.a2dp_source_config_process.state=3;owner.initiator_connection_state=9;disconnects=remote_disconnects=establishes=failures=reports=timers=cancels=0;disconnect_result=establish_result=0;pm_bt_recovery_start(&recovery,1,peer,0);recovery_poll(NULL);}
static void poll(uint32_t t){now=t;recovery_poll(NULL);}
static void configuration(void){
 owner.a2dp_source_config_process.state=8;owner.initiator_connection_state=0;
 owner.a2dp_source_config_process.local_stream_endpoint=&endpoint;owner.active_stream_endpoint=&endpoint;
 endpoint.state=1;endpoint.initiator_config_state=10;endpoint.seid=1;owner.configuration_state=1;
 owner.initiator_signaling_packet.signal_identifier=3;
}
static void test_configuration(void){
 reset();configuration();poll(0);poll(4999);assert(!disconnects);poll(5000);assert(disconnects==1);
 /* Discovery time cannot be inherited; transaction/substate changes reset it. */
 reset();poll(4999);configuration();poll(4999);poll(5000);assert(!disconnects);
 endpoint.initiator_config_state++;poll(9998);poll(9999);assert(!disconnects);
 owner.initiator_transaction_label++;poll(14997);poll(14998);assert(!disconnects);poll(19997);assert(disconnects==1);
 reset();configuration();owner.a2dp_source_config_process.local_stream_endpoint=NULL;poll(0);poll(6000);assert(!disconnects);poll(25000);assert(failures==1);
 reset();configuration();poll(0);owner_exists=false;poll(4999);owner_exists=true;poll(5000);assert(!disconnects);poll(10000);assert(disconnects==1);
 reset();configuration();poll(0);owner.state=5;poll(4999);owner.state=7;poll(5000);assert(!disconnects);poll(10000);assert(disconnects==1);
 reset();configuration();poll(0);owner.a2dp_source_config_process.state=9;poll(4999);poll(7000);assert(!disconnects);
 reset();configuration();poll(0);owner.a2dp_source_config_process.state=17;poll(4999);assert(recovery.phase==PM_BR_OFF);configuration();poll(20000);assert(!disconnects);
 /* ACL collision and either stage share one retry budget. */
 for(unsigned first=0;first<2;first++){
  reset();assert(pm_bt_recovery_reserve(&recovery));if(first)configuration();poll(0);poll(5000);assert(recovery.retries==2&&disconnects==1);
  handle_signaling_released(4);remote_cid=0;owner_exists=false;poll(5001);poll(5501);assert(establishes==1);
  owner_exists=true;if(first){owner.a2dp_source_config_process.state=3;owner.initiator_connection_state=9;}else configuration();
  poll(5502);poll(10502);assert(failures==1&&disconnects==1&&establishes==1);poll(20000);assert(failures==1);
 }
 for(unsigned phase=0;phase<3;phase++)for(unsigned cancel=0;cancel<4;cancel++){
  reset();configuration();poll(0);if(phase){poll(5000);if(phase==2){handle_signaling_released(4);remote_cid=0;owner_exists=false;poll(5100);}}
  if(cancel==0)wanted=false;else if(cancel==1)generation++;else if(cancel==2)peer[0]++;else stopping=true;
  poll(phase==2?5600:5100);assert(recovery.phase==PM_BR_OFF&&!establishes);if(cancel==2)peer[0]--;
 }
 for(unsigned phase=0;phase<3;phase++){
  reset();configuration();poll(0);if(phase){poll(5000);if(phase==2){handle_signaling_released(4);remote_cid=0;owner_exists=false;poll(5100);}}
  recovery.since=recovery.release_started=24999;poll(24999);poll(25000);assert(failures==1&&!establishes);poll(25001);assert(failures==1);
 }
}
static void test_terminal_details(void){
 reset();poll(5000);poll(8000);assert(terminal.status==0xff10&&terminal.detail1==PM_BR_REASON_TEARDOWN&&terminal.detail2==8000&&terminal.detail3==1);
 reset();recovery.retries=2;poll(5000);assert(terminal.detail1==PM_BR_REASON_RETRY_CAP&&terminal.detail2==5000&&terminal.detail3==2);
 reset();poll(5000);poll(25000);assert(terminal.detail1==PM_BR_REASON_GLOBAL&&terminal.detail2==25000&&terminal.detail3==1);
 assert(recovery.reason==PM_BR_REASON_NONE);poll(25001);assert(failures==1);
 reset();recovery.reason=PM_BR_REASON_GLOBAL;recovery_fail(12);assert(terminal.status==12&&!terminal.detail1&&!terminal.detail2&&!terminal.detail3);
 reset();now=26000;pm_bt_recovery_start(&recovery,1,peer,1000);recovery_poll(NULL);assert(terminal.detail1==PM_BR_REASON_GLOBAL&&terminal.detail2==25000);
 reset();recovery.reason=PM_BR_REASON_TEARDOWN;wanted=false;recovery_fail(0xff10);assert(!failures&&recovery.reason==PM_BR_REASON_NONE);
}
int main(void){test_terminal_details();
 test_configuration();
 for(unsigned order=0;order<2;order++){
  reset();poll(5000);assert(disconnects==1&&remote_disconnects==1&&!establishes);
  handle_signaling_released(3);assert(cid==4&&!reports);
  if(order==0){handle_signaling_released(4);poll(5100);assert(!establishes);remote_cid=0;}
  else {remote_cid=0;poll(5100);assert(!establishes);handle_signaling_released(4);}
  handle_signaling_released(4);poll(5200);assert(!reports&&!establishes);
  owner_exists=false;poll(5300);poll(5799);assert(!establishes);poll(5800);assert(establishes==1&&cid==9&&!reports&&!failures);
 }
 reset();poll(5000);owner.a2dp_source_config_process.state=17;poll(5100);assert(recovery.phase==PM_BR_RELEASE);handle_signaling_released(4);remote_cid=0;poll(8000);assert(failures==1&&!establishes);poll(9000);assert(failures==1);
 reset();disconnect_result=12;poll(5000);assert(failures==1&&!establishes);poll(6000);assert(failures==1);
 reset();poll(5000);handle_signaling_released(4);remote_cid=0;owner_exists=false;poll(5100);wanted=false;poll(5600);assert(!establishes&&recovery.phase==PM_BR_OFF);
 reset();poll(5000);handle_signaling_released(4);remote_cid=0;owner_exists=false;poll(5100);establish_result=12;poll(5600);assert(failures==1&&establishes==1);poll(7000);assert(failures==1);
 reset();poll(5000);handle_signaling_released(4);remote_cid=0;owner_exists=false;poll(5100);generation=2;poll(5600);assert(!establishes);
 reset();now=24000;pm_bt_recovery_start(&recovery,1,peer,0);recovery_poll(NULL);poll(25000);assert(failures==1&&!disconnects&&!establishes);
 reset();recovery_fail(12);handle_signaling_released(4);assert(!reports&&failures==1);
 reset();wanted=false;pm_bt_recovery_cancel(&recovery);handle_signaling_released(4);assert(!reports);
 reset();pm_bt_recovery_cancel(&recovery);handle_signaling_released(4);assert(reports==1);
 puts("Bluetooth recovery adapter: release ordering, owner cleanup, effects, internal notification suppression PASS");
}
