/* BT-AUDIO-20261107: compile the real main-loop adapter against bounded fake IO.
 * No SDK, socket, USB or Bluetooth hardware is opened. */
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "bluetooth_runtime.h"
#include "bluetooth_input.h"
#include "device_api.h"
#define PM_BLUETOOTH_AUDIO 1
#define ACTIVE 1
static int state=ACTIVE;
static uint64_t incarnation=1,session_epoch=2;
static pm_media_input_session media_input_session;
static bool send_result=true;
static char outbound_json[2048],last_report[2048];
static uint32_t now_ms;
static bool init_result=true,ready_result=true,queued;
static pm_bt_event event;
static unsigned init_calls,stop_calls,scan_calls,connect_calls,reports,inputs;
static uint64_t last_stop,last_scan,last_connect;
static unsigned locks;
static uint32_t get_absolute_time(void){return now_ms;}
static uint32_t to_ms_since_boot(uint32_t t){return t;}
static void cyw43_arch_lwip_begin(void){assert(!locks);locks++;}
static void cyw43_arch_lwip_end(void){assert(locks==1);locks--;}
bool pm_bt_runtime_init(void){assert(locks);init_calls++;return init_result;}
bool pm_bt_runtime_ready(void){assert(locks);return ready_result;}
void pm_bt_runtime_stop(uint64_t g){assert(locks);stop_calls++;last_stop=g;ready_result=false;}
uint8_t pm_bt_runtime_scan(uint64_t g){assert(locks&&ready_result);scan_calls++;last_scan=g;return 0;}
uint8_t pm_bt_runtime_connect(uint64_t g,const uint8_t address[6]){assert(locks&&ready_result);assert(address[5]==0x55);connect_calls++;last_connect=g;return 0;}
uint8_t pm_bt_runtime_connect_at(uint64_t g,const uint8_t address[6],uint32_t received_ms){assert((uint32_t)(now_ms-received_ms)<25000);return pm_bt_runtime_connect(g,address);}
void pm_bt_runtime_enable_input(bool enabled){assert(locks);(void)enabled;}
bool pm_bt_runtime_next(pm_bt_event *out){assert(locks);if(!queued)return false;*out=event;queued=false;return true;}
static bool pm_net_send(unsigned kind,const char *j){assert(!locks&&(kind==21||kind==27));if(kind==27)inputs++;else reports++;snprintf(last_report,sizeof last_report,"%s",j);return send_result;}
/* Minimal fixture parser: adapter validation is tested, JSON parser is not replaced in production. */
static bool number(const char *j,const char *key,uint64_t *v){char needle[80];snprintf(needle,sizeof needle,"\"%s\":",key);const char *p=strstr(j,needle);if(!p)return false;char *end;*v=strtoull(p+strlen(needle),&end,10);return end!=p+strlen(needle);}
static bool string_equals(const char *j,const char *key,const char *v){char needle[100];snprintf(needle,sizeof needle,"\"%s\":\"%s\"",key,v);return strstr(j,needle)!=NULL;}
void pm_bt_runtime_link_snapshot(pm_bt_link_snapshot *out){assert(locks);memset(out,0,sizeof *out);}
static void pm_logf(const char *fmt,...){(void)fmt;assert(!locks);}
#include "../src/bluetooth_control.inc"
static void emit(pm_bt_event_kind kind,uint64_t generation){assert(!queued);event=(pm_bt_event){.kind=kind,.generation=generation};queued=true;}
static pm_api_result command(uint64_t gen,uint64_t id,const char *action){char j[512];snprintf(j,sizeof j,"{\"incarnation\":1,\"session_epoch\":2,\"token\":{\"session\":2,\"generation\":%llu,\"command_id\":%llu,\"expires_ms\":1},\"action\":\"%s\",\"target\":\"00:11:22:33:44:55\"}",(unsigned long long)gen,(unsigned long long)id,action);return api_bluetooth(20,j);}
static void reset(void){
    bt_token=(pm_bt_command_token){0};bt_incarnation=0;bt_action=0;
    bt_initialized=bt_pending=bt_preparing=bt_waiting=bt_timed_out=false;
    bt_input=(pm_bluetooth_input){0};bt_input_epoch=0;bt_received_ms=0;
    media_input_session=(pm_media_input_session){0};send_result=true;
    now_ms=0;init_result=ready_result=true;queued=false;
    init_calls=stop_calls=scan_calls=connect_calls=reports=inputs=locks=0;
    state=ACTIVE;incarnation=1;session_epoch=2;last_report[0]=0;
}
static void finish_stop(uint64_t gen){ready_result=true;emit(PM_BT_STOPPED,gen);poll_bluetooth();}
static void test_scan_and_supersede(void){
    reset();assert(command(1,1,"discover")==PM_API_PENDING);poll_bluetooth();
    assert(stop_calls==1&&last_stop==1&&!scan_calls&&!reports);
    poll_bluetooth();assert(!scan_calls);finish_stop(1);
    assert(scan_calls==1&&last_scan==1&&!reports);
    emit(PM_BT_SCAN_DONE,1);poll_bluetooth();assert(reports==1&&strstr(last_report,"true"));
    assert(command(2,2,"connect")==PM_API_PENDING);poll_bluetooth();assert(stop_calls==2);
    /* A new command supersedes the connect while waiting for stop. */
    assert(command(3,3,"discover")==PM_API_PENDING);
    finish_stop(2);assert(stop_calls==3&&last_stop==3&&!connect_calls);
    finish_stop(3);assert(scan_calls==2&&last_scan==3&&!connect_calls);
    assert(command(2,4,"connect")==PM_API_IGNORED);
    assert(command(4,3,"connect")==PM_API_IGNORED);
    emit(PM_BT_CONNECTED,2);poll_bluetooth();assert(reports==1&&!bt_input.connected);
}
static void test_deadline_and_init_failure(void){
    reset();command(1,1,"discover");poll_bluetooth();now_ms=24999;poll_bluetooth();assert(!reports);
    now_ms=25000;poll_bluetooth();assert(reports==1&&!bt_pending&&bt_timed_out&&last_stop==1);
    finish_stop(1);emit(PM_BT_CONNECTED,1);poll_bluetooth();assert(reports==1&&!scan_calls&&!bt_input.connected);
    reset();init_result=false;command(1,1,"discover");poll_bluetooth();
    assert(reports==1&&init_calls==1&&!bt_pending&&strstr(last_report,"false"));
    poll_bluetooth();assert(init_calls==1&&reports==1);
    /* u32 local-clock rollover still observes the finite timeout. */
    reset();now_ms=UINT32_MAX-100;command(1,1,"discover");poll_bluetooth();now_ms+=25000;poll_bluetooth();assert(bt_timed_out);
}
static void test_connect_stop_and_stale_session(void){
    reset();command(1,1,"connect");poll_bluetooth();finish_stop(1);assert(connect_calls==1&&last_connect==1);
    emit(PM_BT_CONNECTED,1);poll_bluetooth();assert(bt_input.connected&&reports==1);
    now_ms=30000;poll_bluetooth();assert(!bt_timed_out&&reports==1); // established link has no command deadline
    command(2,2,"stop");poll_bluetooth();assert(last_stop==2&&!bt_input.connected);
    finish_stop(2);assert(reports==2&&strstr(last_report,"true"));
    session_epoch=3;poll_bluetooth();assert(last_stop==0&&!bt_pending&&!bt_input.connected);
}
static void remote_input(uint8_t opcode,bool pressed,uint32_t received){
    emit(PM_BT_INPUT,7);event.operation=opcode;event.pressed=pressed;event.input_ms=received;poll_bluetooth();
}
static void test_media_input(void){
    reset();bt_initialized=true;bt_token.generation=7;bt_token.session=2;bt_incarnation=1;
    pm_bluetooth_input_reset(&bt_input,7,true);
    remote_input(0x44,true,0);now_ms=10;remote_input(0x44,false,10);assert(inputs==0);
    pm_media_input_welcome(&media_input_session,1,2,1);
    remote_input(0x44,true,10);assert(inputs==0);
    now_ms=20;remote_input(0x44,false,20);assert(inputs==1&&strstr(last_report,"toggle_playback"));
    remote_input(0x46,true,20);now_ms=1820;remote_input(0x46,false,1820);
    assert(inputs==2&&strstr(last_report,"cancel"));
    const uint8_t codes[]={0x4b,0x4c,0x41,0x42};
    const char *names[]={"channel_next","channel_previous","volume_up","volume_down"};
    for(unsigned i=0;i<4;i++){
        remote_input(codes[i],true,now_ms);
        assert(inputs==i+3&&strstr(last_report,names[i])&&strstr(last_report,"media_input"));
        assert(strstr(last_report,"\"incarnation\":1")&&strstr(last_report,"\"session_epoch\":2"));
        remote_input(codes[i],true,now_ms);assert(inputs==i+3);
        remote_input(codes[i],false,now_ms);assert(inputs==i+3);
    }
    emit(PM_BT_INPUT,6);event.operation=0x41;event.pressed=true;poll_bluetooth();assert(inputs==6);
    remote_input(0x40,true,now_ms);assert(inputs==6);
    send_result=false;remote_input(0x41,true,now_ms);assert(inputs==7);
    poll_bluetooth();assert(inputs==7); /* Failed send is not replayed. */
    remote_input(0x44,true,now_ms);
    emit(PM_BT_INPUT,7);event.operation=0x44;event.pressed=false;
    event.input_ms=now_ms;event.input_epoch=1;poll_bluetooth();assert(inputs==7);
    /* A new AVRCP owner cannot release the previous owner's held button. */
    remote_input(0x44,true,now_ms);now_ms+=10000;poll_bluetooth();
    remote_input(0x44,false,now_ms);assert(inputs==7); /* Missing release expires. */
    remote_input(0x44,true,now_ms);now_ms++;
    pm_media_input_welcome(&media_input_session,1,2,2);
    remote_input(0x44,false,now_ms);assert(inputs==7);
    pm_media_input_welcome(&media_input_session,1,2,1);
    remote_input(0x44,true,now_ms);session_epoch=3;
    assert(!send_media_input(PM_MEDIA_NEXT));poll_bluetooth();assert(!bt_input.connected&&inputs==7);
    state=0;assert(!send_media_input(PM_MEDIA_NEXT));
}
int main(void){test_media_input();test_scan_and_supersede();test_deadline_and_init_failure();test_connect_stop_and_stale_session();puts("Bluetooth control: supersession, asynchronous stop, stale identity, deadline, init failure PASS");}
