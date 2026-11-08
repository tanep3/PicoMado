/* Execute the production refresh helper against a deliberately bounded SDK fake. */
#include <assert.h>
#include <stdio.h>
#include "bluetooth_recovery.h"
#include "bluetooth_runtime.h"
#include "bluetooth_rebuffer.h"
static pm_bt_rebuffer rebuffer;
#define AVDTP_SIGNALING_CONNECTION_OPENED 7
#define A2DP_STREAMING_OPENED 17
#define AVDTP_STREAM_ENDPOINT_OPENED 6
#define AVDTP_STREAM_ENDPOINT_STREAMING 7
#define OPEN 4
static bool ready,stopping,scanning,wanted,retry_pending;
static uint16_t cid;static uint8_t peer[6];static uint64_t generation;
static pm_bluetooth_audio audio;
static bool streaming,pending;static unsigned prepared_count,prepared_bytes;
static uint64_t accepted_frames,consumed_frames;
static unsigned rate;static void *encoder;static pm_bt_recovery recovery;
typedef struct {int state;uint16_t l2cap_media_cid;} avdtp_stream_endpoint_t;
typedef struct {int state;uint16_t con_handle;struct {int state;bool stream_endpoint_configured;avdtp_stream_endpoint_t *local_stream_endpoint;} a2dp_source_config_process;avdtp_stream_endpoint_t *active_stream_endpoint;} avdtp_connection_t;
typedef struct {int state;} hci_connection_t;
static avdtp_stream_endpoint_t endpoint,other_endpoint;static avdtp_connection_t owner,other_owner;
static hci_connection_t link;static bool owner_exists,peer_owner_matches,link_exists;
static uint32_t now;static pm_bt_event result;static unsigned emitted;
static uint32_t btstack_run_loop_get_time_ms(void){return now;}
static avdtp_connection_t *avdtp_get_connection_for_avdtp_cid(uint16_t c){assert(c==cid);return owner_exists?&owner:NULL;}
static avdtp_connection_t *avdtp_get_connection_for_bd_addr(uint8_t *p){assert(p==peer);return peer_owner_matches?&owner:&other_owner;}
static hci_connection_t *hci_connection_for_handle(uint16_t h){assert(h==owner.con_handle);return link_exists?&link:NULL;}
static void emit(pm_bt_event e){if(!e.generation)e.generation=generation;result=e;emitted++;}
#include "../src/bluetooth_refresh.inc"
static bool recovery_releasing(void){return recovery.phase==PM_BR_RELEASE||recovery.phase==PM_BR_COOLDOWN;}
static void recovery_cancel(void){pm_bt_recovery_cancel(&recovery);}
static void media_stop(void){audio.active=false;streaming=pending=false;prepared_count=prepared_bytes=0;}
static void stopped_if_idle(void){}
#include "../src/bluetooth_signaling_released.inc"
static void reset(void){memset(&audio,0,sizeof audio);pm_bt_rebuffer_reset(&rebuffer,0,0);ready=wanted=owner_exists=peer_owner_matches=link_exists=true;stopping=scanning=retry_pending=false;cid=4;generation=1;rate=48000;encoder=&owner;recovery=(pm_bt_recovery){0};now=100;emitted=0;endpoint=(avdtp_stream_endpoint_t){6,73};owner=(avdtp_connection_t){.state=7,.con_handle=12,.a2dp_source_config_process={17,true,&endpoint},.active_stream_endpoint=&endpoint};link.state=OPEN;}
#define REJECT(change) do {reset();change;assert(!pm_bt_runtime_refresh_connected(2,peer,0));assert(!emitted&&generation==1);}while(0)
int main(void){
 reset();assert(pm_bt_runtime_refresh_connected(2,peer,0));assert(emitted==1&&result.generation==2&&result.kind==PM_BT_CONNECTED&&generation==2);assert(encoder==&owner&&rate==48000&&cid==4&&wanted&&endpoint.l2cap_media_cid==73);
 reset();endpoint.state=7;
 memset(&audio,0x5a,sizeof audio);audio.generation=77;audio.active=true;
 pm_bt_rebuffer_reset(&rebuffer,1,77);rebuffer.shortage=rebuffer.buffering=rebuffer.resuming=true;
 rebuffer.since=13;rebuffer.episodes=2;rebuffer.healthy_since=11;
 pm_bt_rebuffer saved_rebuffer=rebuffer;
 pm_bluetooth_audio saved=audio;streaming=pending=true;prepared_count=9;prepared_bytes=694;accepted_frames=4096;consumed_frames=1152;
 assert(pm_bt_runtime_refresh_connected(2,peer,0));
 saved_rebuffer.generation=2;assert(!memcmp(&rebuffer,&saved_rebuffer,sizeof rebuffer));
 assert(!memcmp(&audio,&saved,sizeof audio)&&streaming&&pending&&prepared_count==9&&prepared_bytes==694&&accepted_frames==4096&&consumed_frames==1152);
 reset();audio.active=true;audio.generation=77;pm_bt_rebuffer_reset(&rebuffer,2,77);
 assert(!pm_bt_runtime_refresh_connected(3,peer,0)&&generation==1&&rebuffer.generation==2);
 assert(!pm_bt_rebuffer_current(&rebuffer,generation,audio.generation));
 reset();audio.active=true;audio.generation=77;pm_bt_rebuffer_reset(&rebuffer,1,76);
 assert(!pm_bt_runtime_refresh_connected(3,peer,0)&&generation==1&&rebuffer.stream==76);
 REJECT(ready=false);REJECT(stopping=true);REJECT(scanning=true);REJECT(wanted=false);REJECT(cid=0);REJECT(retry_pending=true);REJECT(recovery.phase=PM_BR_OBSERVE);REJECT(recovery.phase=PM_BR_RELEASE);REJECT(recovery.phase=PM_BR_COOLDOWN);
 REJECT(encoder=NULL);REJECT(rate=0);REJECT(owner_exists=false);REJECT(peer_owner_matches=false);REJECT(link_exists=false);REJECT(link.state=3);REJECT(owner.state=8);REJECT(owner.a2dp_source_config_process.state=8);REJECT(owner.a2dp_source_config_process.stream_endpoint_configured=false);REJECT(owner.a2dp_source_config_process.local_stream_endpoint=NULL);REJECT(owner.active_stream_endpoint=&other_endpoint);REJECT(endpoint.l2cap_media_cid=0);REJECT(endpoint.state=1);REJECT(now=25000);
 reset();uint8_t other[6]={1};assert(!pm_bt_runtime_refresh_connected(2,other,0));assert(!pm_bt_runtime_refresh_connected(0,peer,0));assert(!pm_bt_runtime_refresh_connected(2,NULL,0));
 reset();now=24999;assert(pm_bt_runtime_refresh_connected(2,peer,0));
 reset();assert(pm_bt_runtime_refresh_connected(22,peer,0));handle_signaling_released(3);assert(emitted==1);handle_signaling_released(4);assert(emitted==2&&result.kind==PM_BT_DISCONNECTED&&result.generation==22);
 puts("Bluetooth refresh: current peer/SDK/ACL/media ownership, boundaries, generation, codec preservation PASS");
}
