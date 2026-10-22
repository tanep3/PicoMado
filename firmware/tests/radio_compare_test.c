#define PM_RADIO_TEST_ENTRY radio_get_regressions
#include "radio_query_test.c"
static void fresh(uint32_t baseline){
 begin();radio_comparison=(pm_radio_comparison){0};radio_sdk_value=baseline;
 radio_write_calls=0;radio_write_result=0;radio_write_applies=true;radio_after_override=false;
}
static void change(const char *line){text(line);assert(!radio_write_calls||radio_comparison.held);service();}
static void fact(const char *s){expect(s);}
#ifndef PM_COMPARE_TEST_ENTRY
#define PM_COMPARE_TEST_ENTRY main
#endif
int PM_COMPARE_TEST_ENTRY(void){
 const uint32_t baselines[]={0,1,2,4};
 for(unsigned i=0;i<4;i++){
  fresh(baselines[i]);text("RADIO INTERFERENCE TRY3 22 1");assert(!radio_ioctl_calls&&!radio_write_calls);
  service();assert(radio_ioctl_calls==2&&radio_write_calls==1&&radio_last_written==3);
  uint32_t base;assert(radio_comparison.held&&pm_interference_candidate_value(radio_comparison.baseline.setting,&base)&&base==baselines[i]);
  fact("op=3 s=returned w=0 r=0 x=00000003 t=3 ");fact("m=1 cdc_status=unverified\n");
  change("RADIO INTERFERENCE RESTORE 22 2");assert(radio_write_calls==2&&radio_last_written==baselines[i]&&radio_sdk_value==baselines[i]);
  assert(radio_comparison.held);fact("op=R s=returned");
  change("RADIO INTERFERENCE TRY3 22 3");assert(radio_write_calls==3&&radio_last_written==3);
  assert(pm_interference_candidate_value(radio_comparison.baseline.setting,&base)&&base==baselines[i]);
  change("RADIO INTERFERENCE RESTORE 22 4");assert(radio_write_calls==4&&radio_sdk_value==baselines[i]);
 }
 // A failed write may have applied: retain baseline, show both results, and
 // allow a distinct explicit RESTORE without inventing factory mode 4.
 fresh(1);radio_write_result=-110;change("RADIO INTERFERENCE TRY3 22 1");
 assert(radio_comparison.held&&radio_sdk_value==3);fact("s=werr w=-110 r=0");fact("base=1 m=1 cdc_status=unverified");
 radio_write_result=0;change("RADIO INTERFERENCE RESTORE 22 2");assert(radio_sdk_value==1&&radio_last_written==1);
 fresh(4);radio_after_override=true;radio_after_result=-110;radio_after_value=0;
 change("RADIO INTERFERENCE TRY3 22 1");fact("s=rerr w=0 r=-110");assert(radio_comparison.held);
 fresh(4);radio_write_applies=false;change("RADIO INTERFERENCE TRY3 22 1");fact("s=mismatch");assert(radio_comparison.held);
 fresh(4);radio_after_override=true;radio_after_result=0;radio_after_value=7;
 change("RADIO INTERFERENCE TRY3 22 1");fact("s=unknown");fact("x=00000007");assert(radio_comparison.held);
 fresh(3);change("RADIO INTERFERENCE TRY3 22 1");fact("s=base_is_3");assert(!radio_comparison.held&&!radio_write_calls);
 fresh(8);change("RADIO INTERFERENCE TRY3 22 1");fact("s=unknown");assert(!radio_comparison.held&&!radio_write_calls);
 fresh(4);radio_sdk_result=-110;change("RADIO INTERFERENCE TRY3 22 1");fact("s=sdk_error");assert(!radio_comparison.held&&!radio_write_calls);
 fresh(4);change("RADIO INTERFERENCE RESTORE 22 1");fact("s=no_base");assert(!radio_ioctl_calls&&!radio_write_calls);
 // Keep the original baseline on later TRY3, on mismatch, and after restore.
 fresh(4);change("RADIO INTERFERENCE TRY3 22 1");unsigned writes=radio_write_calls;
 change("RADIO INTERFERENCE TRY3 22 2");fact("s=already_3");assert(radio_write_calls==writes);
 radio_sdk_value=2;change("RADIO INTERFERENCE TRY3 22 3");fact("s=base_changed");assert(radio_write_calls==writes);
 change("RADIO INTERFERENCE RESTORE 22 4");assert(radio_sdk_value==4&&radio_write_calls==writes+1);
 // Shared main lifecycle hook marks the retained baseline stale, without any
 // new reconnect/reset behavior. Neither mutation is sent after a role cycle.
 fresh(4);change("RADIO INTERFERENCE TRY3 22 1");wifi_off();assert(radio_comparison.stale);
 sta_on=wifi_enabled=have_display=true;ip_link=CYW43_LINK_UP;state=ACTIVE;
 unsigned reads=radio_ioctl_calls;writes=radio_write_calls;
 change("RADIO INTERFERENCE RESTORE 22 2");fact("s=stale");
 change("RADIO INTERFERENCE TRY3 22 3");assert(radio_ioctl_calls==reads&&radio_write_calls==writes);
 // IDs and last reply persist across USB reconnect, separately from INPUT ID.
 fresh(4);change("RADIO INTERFERENCE TRY3 22 10");reads=radio_ioctl_calls;writes=radio_write_calls;
 pm_usb_control_poll(false);poll_local_inputs(clock_ms,true);
 change("RADIO INTERFERENCE TRY3 22 10");assert(radio_ioctl_calls==reads&&radio_write_calls==writes);
 change("RADIO INTERFERENCE RESTORE 22 10");fact("radio error=conflict");
 change("RADIO INTERFERENCE TRY3 22 f");fact("radio error=stale_id");
 text("RADIO INTERFERENCE CAPS");service();fact("last_id=10 last_op=1\n");
 change("RADIO INTERFERENCE RESTORE 22 11");assert(radio_sdk_value==4);
 // Busy rejection is cached and is never executed on replay after becoming idle.
 fresh(4);switching.waiting=true;change("RADIO INTERFERENCE TRY3 22 1");fact("s=busy");
 switching.waiting=false;change("RADIO INTERFERENCE TRY3 22 1");assert(!radio_ioctl_calls&&!radio_write_calls);
 change("RADIO INTERFERENCE TRY3 22 2");assert(radio_write_calls==1);
 fresh(4);radio_exception=16;change("RADIO INTERFERENCE TRY3 22 1");fact("s=context");assert(!radio_ioctl_calls&&!radio_write_calls);
 fresh(4);state=STANDBY;change("RADIO INTERFERENCE TRY3 22 1");fact("s=unavailable");assert(!radio_ioctl_calls&&!radio_write_calls);
 const char *bad[]={"RADIO INTERFERENCE TRY3 23 1","RADIO INTERFERENCE TRY3 22 0","RADIO INTERFERENCE TRY3 22 -1","RADIO INTERFERENCE TRY3 22 1 4","RADIO INTERFERENCE SET 22 1 3","RADIO INTERFERENCE RESTORE 22 10000000000000000"};
 for(unsigned i=0;i<sizeof bad/sizeof bad[0];i++){fresh(4);change(bad[i]);assert(!radio_ioctl_calls&&!radio_write_calls);}
 // Pending requests cancel on disconnect; full reply queues never execute IO.
 fresh(4);text("RADIO INTERFERENCE TRY3 22 1");pm_usb_control_poll(false);service();assert(!radio_ioctl_calls&&!radio_write_calls);
 fresh(4);text("INPUT CAPS");text("INPUT CAPS");change("RADIO INTERFERENCE TRY3 22 1");assert(!radio_ioctl_calls&&!radio_write_calls);
 // SDK side effects occur once even if USB output makes zero/partial progress.
 fresh(4);trace_write_cap=0;change("RADIO INTERFERENCE TRY3 22 1");assert(radio_write_calls==1&&!trace_used);
 trace_write_cap=3;service();fact("s=returned");assert(radio_write_calls==1);
 // Both worst-width signed errors and 64-bit identities fit one bounded row.
 fresh(4);pm_usb_control_init(UINT64_MAX,PM_BUTTON_COUNT);poll_local_inputs(clock_ms,true);
 radio_write_result=INT32_MIN;radio_after_override=true;radio_after_result=INT32_MIN;radio_after_value=0;
 change("RADIO INTERFERENCE TRY3 ffffffffffffffff ffffffffffffffff");
 fact("s=werr w=-2147483648 r=-2147483648");fact("cdc_status=unverified\n");assert(!strstr(trace_text,"response_length"));
 puts("radio comparison: same binary baseline/3/restore, CDC limits, uncertain effects, stale scope, IDs/reconnect, deferred and bounded IO PASS");
 return 0;
}
