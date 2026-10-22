#define PM_RADIO_TEST_ENTRY interference_query_regressions
#include "radio_query_test.c"
static void roaming_fresh(uint32_t raw){
 begin();roaming_comparison=(pm_roaming_comparison){0};radio_comparison=(pm_radio_comparison){0};
 radio_sdk_value=raw;radio_write_calls=0;radio_write_result=0;radio_write_applies=true;radio_after_override=false;
}
static void roam(const char *request){text(request);service();}
int main(void){
 pm_roaming_candidate candidate={0};uint32_t raw=9;
 assert(!pm_roaming_value(candidate,&raw)&&raw==9);
 assert(!pm_roaming_from_raw(2,&candidate)&&!pm_roaming_from_raw(UINT32_MAX,&candidate));
 assert(!pm_roaming_from_raw(0,NULL));
 for(unsigned value=0;value<2;value++){
  assert(pm_roaming_from_raw(value,&candidate)&&pm_roaming_value(candidate,&raw)&&raw==value);
  roaming_fresh(value);text("RADIO ROAMING GET 22");assert(!radio_ioctl_calls);service();
  char expected[100];snprintf(expected,sizeof expected,"raw=%08x roam_off_candidate=%u cdc_status=unverified",value,value);expect(expected);
 }
 roaming_fresh(0);roam("RADIO ROAMING OFF 22 1");
 assert(roaming_comparison.held&&radio_ioctl_calls==2&&radio_write_calls==1&&radio_last_written==1);
 assert(pm_roaming_value(roaming_comparison.baseline,&raw)&&raw==0);expect("op=O s=returned w=0 r=0 x=00000001 t=1 base=0 m=1 cdc_status=unverified");
 roam("RADIO ROAMING RESTORE 22 2");assert(radio_write_calls==2&&radio_last_written==0&&radio_sdk_value==0);
 roam("RADIO ROAMING OFF 22 3");assert(radio_write_calls==3&&radio_sdk_value==1);
 unsigned reads=radio_ioctl_calls,writes=radio_write_calls;
 roam("RADIO ROAMING OFF 22 3");assert(reads==radio_ioctl_calls&&writes==radio_write_calls);
 roam("RADIO INTERFERENCE TRY3 22 3");expect("radio error=conflict");assert(reads==radio_ioctl_calls&&writes==radio_write_calls);
 roam("RADIO ROAMING RESTORE 22 3");expect("radio error=conflict");
 roam("RADIO ROAMING OFF 22 2");expect("radio error=stale_id");
 pm_usb_control_poll(false);poll_local_inputs(clock_ms,true);roam("RADIO ROAMING OFF 22 3");assert(reads==radio_ioctl_calls&&writes==radio_write_calls);
 roam("RADIO ROAMING CAPS");expect("last_id=3 last_op=1\n");
 roam("RADIO ROAMING RESTORE 22 4");assert(radio_last_written==0);
 roaming_fresh(1);roam("RADIO ROAMING OFF 22 1");expect("s=already_off");assert(!radio_write_calls&&!roaming_comparison.held);
 roam("RADIO ROAMING RESTORE 22 2");expect("s=no_base");assert(!radio_write_calls);
 const uint32_t invalid[]={2,4,0x80,0x100,0x01000000,UINT32_MAX};
 for(unsigned i=0;i<sizeof invalid/sizeof invalid[0];i++){
  roaming_fresh(invalid[i]);roam("RADIO ROAMING OFF 22 1");expect("s=unknown");assert(!radio_write_calls&&!roaming_comparison.held);
 }
 for(unsigned n=0;n<4;n++){
  roaming_fresh(0);radio_copy_bytes=n;roam("RADIO ROAMING OFF 22 1");expect("s=unknown");assert(!radio_write_calls&&!roaming_comparison.held);
 }
 roaming_fresh(0);radio_sdk_result=-110;roam("RADIO ROAMING OFF 22 1");expect("s=sdk_error");assert(!radio_write_calls&&!roaming_comparison.held);
 roaming_fresh(0);radio_write_result=-110;roam("RADIO ROAMING OFF 22 1");expect("s=werr");assert(roaming_comparison.held&&radio_sdk_value==1);
 radio_write_result=0;roam("RADIO ROAMING RESTORE 22 2");assert(radio_sdk_value==0&&radio_write_calls==2);
 roaming_fresh(0);radio_write_applies=false;roam("RADIO ROAMING OFF 22 1");expect("s=mismatch");assert(roaming_comparison.held);
 roaming_fresh(0);radio_after_override=true;radio_after_result=-110;radio_after_value=0;
 roam("RADIO ROAMING OFF 22 1");expect("s=rerr");assert(roaming_comparison.held);
 roaming_fresh(0);radio_after_override=true;radio_after_result=0;radio_after_value=2;
 roam("RADIO ROAMING OFF 22 1");expect("s=unknown");assert(roaming_comparison.held);
 // Recovery needs the initialized STA, not a TCP connection or current link.
 roaming_fresh(0);roam("RADIO ROAMING OFF 22 1");state=RETRY;ip_link=CYW43_LINK_DOWN;
 roam("RADIO ROAMING RESTORE 22 2");assert(radio_sdk_value==0&&radio_write_calls==2);
 roaming_fresh(0);roam("RADIO ROAMING OFF 22 1");wifi_off();assert(roaming_comparison.stale);
 roam("RADIO ROAMING RESTORE 22 2");expect("s=unavailable");assert(radio_write_calls==1);
 cyw43_arch_enable_sta_mode();wifi_enabled=true;state=WIFI;
 roam("RADIO ROAMING RESTORE 22 3");assert(radio_sdk_value==0&&radio_write_calls==2&&roaming_comparison.stale);
 state=ACTIVE;ip_link=CYW43_LINK_UP;reads=radio_ioctl_calls;
 roam("RADIO ROAMING OFF 22 4");expect("s=stale");assert(radio_ioctl_calls==reads&&radio_write_calls==2);
 // No hidden automatic calls on reconnect or ordinary servicing.
 reads=radio_ioctl_calls;writes=radio_write_calls;for(unsigned i=0;i<10;i++)service();assert(reads==radio_ioctl_calls&&writes==radio_write_calls);
 const app_state unavailable[]={MENU,WIFI,RETRY,STANDBY,ENDPOINT,SERVER,SESSION};
 for(unsigned i=0;i<sizeof unavailable/sizeof unavailable[0];i++){
  roaming_fresh(0);state=unavailable[i];roam("RADIO ROAMING OFF 22 1");expect("s=unavailable");assert(!radio_ioctl_calls&&!radio_write_calls);
 }
 roaming_fresh(0);switching.waiting=true;roam("RADIO ROAMING OFF 22 1");expect("s=busy");assert(!radio_ioctl_calls);
 switching.waiting=false;roam("RADIO ROAMING OFF 22 1");assert(!radio_ioctl_calls);
 roaming_fresh(0);roam("RADIO ROAMING OFF 22 1");pending_render_ack=true;
 roam("RADIO ROAMING RESTORE 22 2");expect("s=busy");assert(radio_write_calls==1);
 roaming_fresh(0);radio_exception=16;roam("RADIO ROAMING GET 22");expect("status=context");assert(!radio_ioctl_calls);
 roaming_fresh(0);radio_core=1;roam("RADIO ROAMING OFF 22 1");expect("s=context");assert(!radio_ioctl_calls);
 const char *bad[]={"RADIO ROAMING SET 22 1 1","RADIO ROAMING TRY3 22 1","RADIO ROAMING OFF 23 1","RADIO ROAMING OFF 22 0","RADIO ROAMING OFF 22 -1","RADIO ROAMING OFF 22 10000000000000000","RADIO ROAMING GET 22 1"};
 for(unsigned i=0;i<sizeof bad/sizeof bad[0];i++){roaming_fresh(0);roam(bad[i]);assert(!radio_ioctl_calls&&!radio_write_calls);}
 roaming_fresh(0);const char *request="RADIO ROAMING OFF 22 1";
 for(const char *p=request;*p;p++){pm_usb_control_input(*p);service();assert(!radio_ioctl_calls);}
 pm_usb_control_input('\n');assert(!radio_ioctl_calls);service();assert(radio_write_calls==1);
 roaming_fresh(0);text(request);pm_usb_control_poll(false);service();assert(!radio_ioctl_calls&&!radio_write_calls);
 roaming_fresh(0);text("INPUT CAPS");text("INPUT CAPS");roam(request);assert(!radio_ioctl_calls&&!radio_write_calls);
 roaming_fresh(0);trace_write_cap=0;roam(request);assert(radio_write_calls==1&&!trace_used);trace_write_cap=3;service();expect("s=returned");assert(radio_write_calls==1);
 // Existing INPUT wins busy arbitration; its 200ms deadline remains functional.
 roaming_fresh(0);text("RADIO ROAMING GET 22");text("INPUT DO 22 1 UP");service();expect("status=busy");assert(!radio_ioctl_calls);
 clock_ms+=200;poll_switch_feedback();assert(feedback_shows==1);
 roaming_fresh(0);pm_usb_control_init(UINT64_MAX,PM_BUTTON_COUNT);poll_local_inputs(clock_ms,true);
 radio_write_result=INT32_MIN;radio_after_override=true;radio_after_result=INT32_MIN;
 roam("RADIO ROAMING OFF ffffffffffffffff ffffffffffffffff");expect("s=werr w=-2147483648 r=-2147483648");assert(!strstr(trace_text,"response_length"));
 puts("roaming: typed values, real main/USB/SDK wire, baseline/OFF/restore, uncertain IO, quiet/recovery gate, shared IDs/partial/reconnect PASS");
 return 0;
}
