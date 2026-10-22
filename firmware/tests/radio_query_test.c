// Real main + USB owner/parser/serializer, with SDK/hardware IO fixtures.
#define PM_CONTROL_TEST_ENTRY existing_control_cases
#define PM_TEST_REAL_USB_CONTROL 1
#define PM_CAUSAL_FIXTURE 1
#include "control_reply_test.c"
static void text(const char *line){for(const char *p=line;*p;p++)pm_usb_control_input(*p);pm_usb_control_input('\n');}
static void drain(void){for(unsigned i=0;i<1200;i++){pm_usb_control_poll(true);pm_trace_flush();}}
static void service(void){log_usb_snapshot();drain();}
static void begin(void){
 setup();clock_ms=6000000;pressed=-1;usb_connected=true;
 sta_on=wifi_enabled=true;ip_link=CYW43_LINK_UP;
 radio_core=radio_exception=radio_ioctl_calls=0;radio_sdk_result=0;
 radio_sdk_value=4;radio_copy_bytes=4;
 trace_output=true;trace_write_cap=SIZE_MAX;trace_used=0;trace_text[0]=0;
 pm_usb_control_init(0x22,PM_BUTTON_COUNT);
 pm_usb_control_set_operations(PM_INPUT_OPERATIONS_MASK,usb_operation);
 poll_local_inputs(clock_ms,true);
}
static void expect(const char *s){if(!strstr(trace_text,s)){fprintf(stderr,"missing [%s] in [%s]\n",s,trace_text);assert(0);}}
static void no_mode(void){assert(!strstr(trace_text," mode="));}
int main(void){
 pm_interference_mode mode={0};uint32_t value=42;
 assert(!pm_interference_value(mode,&value)&&value==42);
 assert(!pm_interference_from_raw(UINT32_MAX,&mode));
 assert(!pm_interference_from_raw(5,&mode));
 assert(!pm_interference_from_raw(0,NULL));
 pm_radio_read uninitialized={0};assert(!pm_radio_observed(uninitialized,&mode));
 for(uint32_t v=0;v<=4;v++){
  assert(pm_interference_from_raw(v,&mode)&&pm_interference_value(mode,&value)&&value==v);
  assert(pm_radio_observed(pm_radio_decode(0,v),&mode));
  begin();radio_sdk_value=v;text("RADIO INTERFERENCE GET 22");
  assert(!radio_ioctl_calls);drain();assert(!radio_ioctl_calls&&!trace_used);
  service();assert(radio_ioctl_calls==1);char expected[40];snprintf(expected,sizeof expected,"raw=%08x mode=%u\n",v,v);
  expect("status=observed rc=0 ");expect(expected);service();assert(radio_ioctl_calls==1);
 }
 // Error/unknown/short output never constructs an observed value, including 0.
 begin();radio_sdk_result=-110;radio_sdk_value=0;text("RADIO INTERFERENCE GET 22");service();
 expect("status=sdk_error rc=-110\n");no_mode();assert(radio_ioctl_calls==1&&!lwip_depth);
 radio_sdk_result=0;radio_sdk_value=3;trace_used=0;trace_text[0]=0;text("RADIO INTERFERENCE GET 22");service();
 expect("status=observed rc=0 raw=00000003 mode=3\n");assert(radio_ioctl_calls==2);
 for(unsigned count=0;count<4;count++){
  begin();radio_copy_bytes=count;radio_sdk_value=0;text("RADIO INTERFERENCE GET 22");service();
  expect("status=unknown rc=0 raw=");no_mode();assert(radio_ioctl_calls==1);
 }
 begin();radio_sdk_value=5;text("RADIO INTERFERENCE GET 22");service();expect("status=unknown rc=0 raw=00000005\n");no_mode();
 // Main availability rejects every pre-init/standby/retry path, even if a
 // fixture leaves the last link UP; it must never call ensure_up to wake it.
 const app_state unavailable[]={MENU,WIFI,RETRY,STANDBY,ENDPOINT};
 for(unsigned i=0;i<sizeof unavailable/sizeof unavailable[0];i++){
  begin();state=unavailable[i];text("RADIO INTERFERENCE GET 22");service();
  assert(!radio_ioctl_calls);expect("status=unavailable rc=0\n");no_mode();
 }
 begin();wifi_enabled=false;text("RADIO INTERFERENCE GET 22");service();assert(!radio_ioctl_calls);expect("status=unavailable");
 begin();ip_link=CYW43_LINK_DOWN;text("RADIO INTERFERENCE GET 22");service();assert(!radio_ioctl_calls);expect("status=unavailable");
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
 begin();panel_standby=true;text("RADIO INTERFERENCE GET 22");service();assert(!radio_ioctl_calls);expect("status=unavailable");
#endif
 const app_state available[]={SERVER,SESSION,ACTIVE};
 for(unsigned i=0;i<3;i++){begin();state=available[i];text("RADIO INTERFERENCE GET 22");service();assert(radio_ioctl_calls==1);}
 // IRQ/core1 cannot run IO; even accepting bytes and calling ordinary USB poll
 // from a callback does not dispatch a query. Only main's service does.
 begin();radio_exception=16;text("RADIO INTERFERENCE GET 22");drain();assert(!radio_ioctl_calls);
 service();expect("status=context rc=0\n");assert(!radio_ioctl_calls);
 begin();radio_core=1;text("RADIO INTERFERENCE GET 22");service();expect("status=context");assert(!radio_ioctl_calls);
 begin();radio_exception=16;text("RADIO INTERFERENCE GET 22");drain();radio_exception=0;service();assert(radio_ioctl_calls==1);
 // No SET or arbitrary command is accepted; stale identities and untrusted
 // syntax are checked before reserving the SDK request.
 const char *invalid[]={"RADIO INTERFERENCE SET 22 3","RADIO IOCTL GET 22","RADIO INTERFERENCE GET 23",
  "RADIO INTERFERENCE GET -1","RADIO INTERFERENCE GET 10000000000000000","RADIO INTERFERENCE GET 22 1","RADIO"};
 for(unsigned i=0;i<sizeof invalid/sizeof invalid[0];i++){begin();text(invalid[i]);service();assert(!radio_ioctl_calls);no_mode();}
 begin();const char *request="RADIO INTERFERENCE GET 22";
 for(const char *p=request;*p;p++){pm_usb_control_input(*p);service();assert(!radio_ioctl_calls);}
 pm_usb_control_input('\n');assert(!radio_ioctl_calls);service();assert(radio_ioctl_calls==1);
 // Completed pending and byte-partial requests are canceled on disconnect.
 begin();text(request);pm_usb_control_poll(false);service();assert(!radio_ioctl_calls);
 begin();for(const char *p="RADIO INTERFERENCE ";*p;p++)pm_usb_control_input(*p);
 pm_usb_control_poll(false);text("GET 22");service();assert(!radio_ioctl_calls);
 text(request);service();assert(radio_ioctl_calls==1);
 // A reserved query reply coexists with a real INPUT request without changing
 // INPUT IDs, and a second pending query cannot cause an extra SDK call.
 begin();text(request);text("INPUT DO 22 1 UP");assert(input_sends==1);
 service();assert(radio_ioctl_calls==1);expect("mode=4\n");expect("id=1 op=UP status=dispatched\n");
 text("INPUT CAPS");drain();expect("last_request_id=1\n");
 begin();text(request);text(request);service();expect("radio error=busy\n");assert(radio_ioctl_calls==1);
 begin();text("INPUT CAPS");text("INPUT CAPS");text(request);service();assert(!radio_ioctl_calls);
 // Shared partial writer: zero progress does not replay IO or interleave lines.
 begin();trace_write_cap=0;text(request);service();assert(radio_ioctl_calls==1&&!trace_used);
 trace_write_cap=3;pm_logf("ordinary shared line\n");service();
 expect("radio interference b=22 status=observed rc=0 raw=00000004 mode=4\n");
#if PM_TRACE_ENABLE
 expect("ordinary shared line\n");
#endif
 assert(radio_ioctl_calls==1);
 assert(!strstr(strstr(trace_text,"radio interference")+1,"radio interference"));
 printf("radio GET %s trace=%u: real main/parser, modes/errors/unknown/short, availability/context, deferred IO, reserved/partial reply, reconnect, INPUT isolation PASS\n",PM_DISPLAY_CAPABILITY,PM_TRACE_ENABLE);
}
