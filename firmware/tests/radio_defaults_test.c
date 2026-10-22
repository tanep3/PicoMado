#define PM_RADIO_TEST_ENTRY existing_radio_query_cases
#include "radio_query_test.c"
static void fresh_defaults(void){
 begin();state=WIFI;radio_defaults_attempted=false;
 radio_comparison=(pm_radio_comparison){0};roaming_comparison=(pm_roaming_comparison){0};
 radio_write_calls=0;radio_write_result=0;radio_write_applies=true;radio_after_override=false;
}
static void defaults_expect(const char *text){
#if PM_TRACE_ENABLE
 expect(text);
#else
 (void)text; /* Ordinary startup text is intentionally compiled out. */
#endif
}
static void attempt_defaults(void){apply_radio_defaults();drain();}
int main(void){
 fresh_defaults();ip_link=CYW43_LINK_JOIN;attempt_defaults();assert(!radio_write_calls&&!radio_ioctl_calls&&!radio_defaults_attempted);
 ip_link=CYW43_LINK_UP;attempt_defaults();
 assert(radio_defaults_attempted&&radio_write_calls==2&&radio_ioctl_calls==2);
 assert(default_interference.write_result==0&&default_interference.read.raw==3);
 assert(default_roaming.write_result==0&&default_roaming.read.raw==1);
 assert(!radio_comparison.held&&!roaming_comparison.held);
 defaults_expect("k=interference t=3 w=0 r=0 s=transport_returned x=00000003 m=1 cdc_status=unverified");
 defaults_expect("k=roam_off t=1 w=0 r=0 s=transport_returned x=00000001 m=1 cdc_status=unverified");
 for(unsigned i=0;i<4;i++){attempt_defaults();}
 assert(radio_write_calls==2&&radio_ioctl_calls==2);
 // A real TCP-only retry and a Wi-Fi rejoin within the enabled interface do not SET again.
 start_server_now(&profiles[active_profile]);assert(state==SERVER);attempt_defaults();
 start_wifi();assert(state==WIFI);attempt_defaults();assert(radio_write_calls==2&&radio_ioctl_calls==2);
 // Actual STA disable/enable is a new initialization interval.
 wifi_off();assert(!radio_defaults_attempted&&!wifi_enabled);start_wifi();assert(wifi_enabled);
 ip_link=CYW43_LINK_UP;attempt_defaults();assert(radio_defaults_attempted&&radio_write_calls==4&&radio_ioctl_calls==4);
 // Explicit status replay must never re-run either setter.
 log_radio_defaults_status();assert(radio_write_calls==4&&radio_ioctl_calls==4);
 fresh_defaults();radio_write_result=-110;attempt_defaults();assert(radio_write_calls==2&&radio_defaults_attempted);
 assert(default_interference.write_result==-110&&default_roaming.write_result==-110);defaults_expect("w=-110 r=0");
 attempt_defaults();assert(radio_write_calls==2);start_server_now(&profiles[active_profile]);assert(state==SERVER);
 fresh_defaults();radio_sdk_result=-110;attempt_defaults();assert(radio_ioctl_calls==2&&radio_write_calls==2);
 assert(default_interference.read.status==PM_RADIO_SDK_ERROR&&default_roaming.read.status==PM_RADIO_SDK_ERROR);defaults_expect("s=sdk_error");
 attempt_defaults();assert(radio_write_calls==2);
 fresh_defaults();radio_write_applies=false;radio_sdk_value=0;attempt_defaults();defaults_expect("x=00000000 m=0 cdc_status=unverified");assert(radio_write_calls==2);
 for(unsigned n=0;n<4;n++){
  fresh_defaults();radio_copy_bytes=n;attempt_defaults();
  assert(default_interference.read.status==PM_RADIO_UNKNOWN&&default_roaming.read.status==PM_RADIO_UNKNOWN);
  attempt_defaults();assert(radio_write_calls==2&&radio_ioctl_calls==2);
 }
 fresh_defaults();radio_exception=16;attempt_defaults();assert(!radio_write_calls&&!radio_defaults_attempted);
 fresh_defaults();radio_core=1;attempt_defaults();assert(!radio_write_calls&&!radio_defaults_attempted);
 fresh_defaults();wifi_enabled=false;attempt_defaults();assert(!radio_write_calls&&!radio_defaults_attempted);
 fresh_defaults();state=ACTIVE;attempt_defaults();assert(!radio_write_calls&&!radio_defaults_attempted);
 // Zero initialized/invalid typed setters reject without touching SDK.
 fresh_defaults();assert(pm_radio_interference_write((pm_interference_candidate){0})!=0);
 assert(pm_radio_roaming_write((pm_roaming_candidate){0})!=0);assert(!radio_write_calls);
 puts("radio defaults: fixed typed3/OFF after link UP, once per STA epoch, TCP retry/rejoin unchanged, disable/enable reapply, error/unknown/mismatch visible and bounded PASS");
 return 0;
}
