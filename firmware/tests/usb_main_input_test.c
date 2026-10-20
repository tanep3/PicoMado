#define PM_CONTROL_TEST_ENTRY existing_control_cases
#include "control_reply_test.c"
static void command_now(const char *line){
 for(const char *p=line;*p;p++)pm_usb_control_input(*p);
 pm_usb_control_input('\n');
 for(unsigned i=0;i<80;i++){clock_ms+=5;pm_usb_control_poll(true);pm_trace_flush();}
}
int main(void){
 setup();clock_ms=1000;pressed=-1;usb_connected=true;trace_output=true;
 pm_ack_probe_init_boot(1);pm_usb_control_init(1,PM_BUTTON_COUNT);poll_local_inputs(clock_ms,true);
#if PM_BUTTON_COUNT == 0
 command_now("SIM PRESS 1 1 a 0 100");assert(pm_usb_sim_status()->phase==PM_SIM_IDLE);puts("PASS real main0: virtual input unsupported");return 0;
#else
 command_now("AP2 ARM 1 1");assert(pm_ack_probe_read_summary().state==PM_AP_PREPARED);
 command_now("SIM BEGIN 1 1 a 1 1 3");command_now("SIM STEP 1 2 a 0 0 1 64");command_now("SIM COMMIT 1 3 a");
 usb_connected=false;poll_local_inputs(clock_ms,true);assert(pm_ack_probe_read_summary().state==PM_AP_ARMED);assert(!pm_ack_probe_read_summary().trigger_us);
 clock_ms+=500;poll_local_inputs(clock_ms,true);clock_ms+=35;poll_local_inputs(clock_ms,true);
#if PM_BUTTON_COUNT == 2
 clock_ms+=65;poll_local_inputs(clock_ms,true);clock_ms+=35;poll_local_inputs(clock_ms,true);
#endif
 pm_ack_probe_summary summary=pm_ack_probe_read_summary();assert(summary.state==PM_AP_RECORDING&&summary.arm_buttons==0);
 bool synthetic=false,physical=false;pm_ack_probe_record row;
 for(unsigned i=0;pm_ack_probe_read(i,&row);i++){synthetic|=row.event==PM_AP_SIM_BUTTON;physical|=row.event==PM_AP_BUTTON;}
 assert(synthetic&&!physical);assert(pm_usb_sim_status()->steps[0].dispatched);
 // Physical state is distinct; its stable edge is still seen once.
 pressed=1;poll_local_inputs(clock_ms,true);clock_ms+=35;poll_local_inputs(clock_ms,true);
#if PM_BUTTON_COUNT == 2
 pressed=-1;poll_local_inputs(clock_ms,true);clock_ms+=35;poll_local_inputs(clock_ms,true);
#endif
 unsigned physical_events=0;
 for(unsigned i=0;pm_ack_probe_read(i,&row);i++)if(row.event==PM_AP_BUTTON)physical_events++;
 assert(physical_events==1);assert(pm_ack_probe_read_summary().arm_buttons==0);
 puts("PASS real main input path: gate before synthetic trigger, origin separate, shared button action, physical edge preserved");
#endif
}
