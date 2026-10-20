#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wreturn-type"
#define main original_usb_control_cases
#include "usb_control_test.c"
#undef main
#pragma GCC diagnostic pop
int main(void){
 reset(4);command("AP2 ARM 1 a");command("SIM BEGIN 1 1 64 1 1 3");command("SIM STEP 1 2 64 0 0 1 64");command("SIM COMMIT 1 3 64");
 assert(pm_usb_sim_status()->phase==PM_SIM_WAIT_CLOSE&&pm_usb_sim_status()->bound_boot==1&&pm_usb_sim_status()->bound_run==10);
 command("AP2 FREEZE 1 a");pm_ack_probe_summary a=pm_ack_probe_read_summary();char release[96];snprintf(release,sizeof release,"AP2 RELEASE 1 a %x",a.digest);command(release);
 assert(pm_usb_sim_status()->phase==PM_SIM_CANCELLED&&pm_usb_sim_status()->reason==PM_SIM_CANCEL_RUN);
 command("AP2 ARM 1 b");assert(pm_ack_probe_read_summary().run==11);command("SIM COMMIT 1 3 64"); // old ACK stays idempotent; no rearm/rebind.
 assert(pm_usb_sim_status()->bound_run==10&&pm_usb_sim_status()->phase==PM_SIM_CANCELLED);
 uint32_t t=now_ms;for(unsigned i=0;i<200;i++)assert(!tick(false,0,true,3,t+i*5).valid);
 assert(pm_ack_probe_read_summary().state==PM_AP_ARMED&&!pm_ack_probe_read_summary().trigger_us&&!pm_usb_sim_status()->steps[0].dispatched);
 // A standalone job (run0) cannot leak into a subsequently armed measurement.
 reset(2);command("SIM PRESS 1 1 64 0 1000");assert(pm_usb_sim_status()->bound_run==0);command("AP2 ARM 1 a");assert(pm_usb_sim_status()->phase==PM_SIM_CANCELLED&&pm_usb_sim_status()->reason==PM_SIM_CANCEL_RUN);assert(!tick(true,0,true,3,now_ms+35).valid);
 // A boot mismatch also cancels before virtual dispatch, regardless of session.
 reset(4);command("SIM PRESS 1 1 64 1 1000");pm_ack_probe_init_boot(2);assert(!tick(true,0,true,3,now_ms).valid&&pm_usb_sim_status()->reason==PM_SIM_CANCEL_RUN);
 puts("PASS USB-TX-R1: committed A cannot execute in B; RELEASE/ARM cancels immediately; cached ACK never rebinds; standalone0 and boot-change guards");
}
