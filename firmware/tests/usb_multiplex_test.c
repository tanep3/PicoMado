#define main baseline_ap2_cases
#include "ack_probe_test.c"
#undef main
#include "usb_control.h"
static void send(const char *p){while(*p)pm_usb_control_input(*p++);pm_usb_control_input('\n');}
static void ticks(unsigned count){for(unsigned i=0;i<count;i++){clock_us+=5000;pm_ack_probe_usb_state(true);pm_usb_control_tick(true,0,true,1,3,false,1800,(uint32_t)(clock_us/1000));pm_usb_control_poll(true);pm_trace_flush();}}
int main(void){
 reset();pm_usb_control_init(0x1234,2);usb=true;ticks(1);
 send("AP2 ARM 1234 a");ticks(100);pm_ack_probe_usb_state(false);pm_ack_probe_button(1,2,3,1,false);send("AP2 FREEZE 1234 a");ticks(100);
 send("SIM PRESS 1234 1 b 0 1000");ticks(10);assert(pm_usb_sim_status()->virtual_button.stable);
 // AP2 page producer + both SIM response slots are simultaneously occupied.
 send("AP2 READ 1234 a 0 8");send("SIM CAPS");send("SIM STATUS 1234");send("SIM STOP 1234 2");
 assert(pm_usb_sim_status()->phase==PM_SIM_CANCELLED);assert(!pm_usb_sim_status()->virtual_button.stable);
 ticks(300);send("SIM ACK 1234 2");ticks(200);
 assert(strstr(output,"ap2 page ")&&strstr(output,"ap2 end ")&&strstr(output,"sim caps ")&&strstr(output,"cid=2 job=b result=ok"));
 // pm_trace_usb_write from the production trace fixture writes only 7 bytes/call.
 for(char*p=output;*p;){char*end=strchr(p,'\n');assert(end);assert(!strncmp(p,"ap2 ",4)||!strncmp(p,"sim ",4));assert(end-p<144);p=end+1;}
 assert(pm_ack_probe_read_summary().state==PM_AP_FROZEN);
 puts("PASS shared parser and atomic trace: AP2 page + SIM replies + emergency STOP with7-byte USB partial writes, retained ACK and frozen data");
}
