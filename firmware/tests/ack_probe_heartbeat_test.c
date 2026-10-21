#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "../src/ack_probe.c"
static uint64_t now=100;
static char output[512];
uint64_t time_us_64(void){return now;}
bool pm_trace_try_line(const char *s,size_t n){assert(n<sizeof output);memcpy(output,s,n);output[n]=0;return true;}
static void drain(void){for(unsigned i=0;i<4;i++){now+=25000;pm_ack_probe_poll(true);}}
static void command(const char *s){output[0]=0;pm_ack_probe_command(s);drain();}
static void setup(const char *arm){
 pm_ack_probe_init_boot(1);pm_ack_probe_connection();pm_ack_probe_live(9,true);
 pm_ack_probe_usb_state(true);command(arm);assert(probe.summary.state==PM_AP_PREPARED);
}
static void hb(void){
 pm_ack_probe_before_lock(5,"{}");pm_ack_probe_locked(5);pm_ack_probe_encoded(5,"{}");
 pm_ack_probe_net(PM_AP_WRITE_BEGIN,9,100,100,1400,64000);
 pm_ack_probe_net(PM_AP_WRITE_END,9,0,11000,1,114);
 pm_ack_probe_net(PM_AP_TX_POSITION,9,100,114,100,100);
 pm_ack_probe_net(PM_AP_OUTPUT_END,13,0,114,100,11000);
 pm_ack_probe_send_return(5,true);
}
int main(void){
 setup("AP2 ARM_HB 1 a");command("AP2 CAPS");assert(strstr(output,"hb=1"));
 command("AP2 TRIGGER");assert(strstr(output,"mode=1 o=0 g=0 m=0 flags=0"));
 pm_ack_probe_usb_state(false);hb();
 assert(probe.summary.state==PM_AP_RECORDING&&probe.summary.count==9);
 assert(probe.records[0].event==25&&probe.records[0].kind==5&&probe.records[0].message==1&&probe.records[0].session==9);
 assert(probe.records[1].event==PM_AP_BEFORE_LOCK&&probe.records[2].event==PM_AP_LOCKED);
 uint64_t started=probe.summary.trigger_us;now+=15000000;hb();assert(probe.summary.trigger_us==started&&probe.trigger.message==1);
 now=started+PM_ACK_PROBE_WINDOW_US;pm_ack_probe_poll(false);assert(probe.summary.state==PM_AP_FROZEN&&probe.summary.reason==1);
 uint32_t d=probe.summary.digest,n=probe.summary.count;pm_ack_probe_button(9,1,1,1,false);pm_ack_probe_connection();hb();
 assert(probe.summary.digest==d&&probe.summary.count==n&&!probe.trigger.flags);
 puts("PASS heartbeat trigger order/once/exact20s/FROZEN immutable");
 setup("AP2 ARM_HB 1 b");hb();assert(!probe.summary.count&&!probe.summary.trigger_us);
 pm_ack_probe_usb_state(false);assert(probe.trigger.gate==1&&probe.trigger.flags==2);hb();
 assert(probe.trigger.origin==0&&probe.trigger.message==2&&probe.records[0].c==2);
 puts("PASS PREPARED heartbeat is flagged even if host could start early");
 setup("AP2 ARM_HB 1 c");pm_ack_probe_usb_state(false);pm_ack_probe_button(9,1,1,1,false);
 assert(probe.summary.state==PM_AP_ARMED&&probe.trigger.flags==1);hb();assert(probe.records[0].c==1);
 setup("AP2 ARM_HB 1 d");pm_ack_probe_usb_state(false);pm_ack_probe_sim_button(9,1,1,1,false,4,0);
 assert(probe.summary.state==PM_AP_ARMED&&probe.trigger.flags==1&&!probe.sim_count);hb();
 puts("PASS physical/SIM input does not impersonate heartbeat trigger");
 setup("AP2 ARM_HB 1 11");pm_ack_probe_usb_state(false);hb();
 started=probe.summary.trigger_us;assert(probe.trigger.message==1&&probe.records[0].event==PM_AP_PERIODIC_TRIGGER);
 now+=1000;pm_ack_probe_sim_button(9,1,1,1,false,5,0);
 assert(probe.summary.state==PM_AP_RECORDING&&probe.summary.trigger_us==started&&probe.trigger.message==1);
 assert(probe.trigger.flags==1&&probe.sim_count==1&&probe.sim_job==5);
 assert(probe.records[9].event==PM_AP_SIM_BUTTON&&probe.records[9].session==9);
 puts("PASS SIM after heartbeat retains trigger and records one input");
 setup("AP2 ARM_HB 1 e");pm_ack_probe_usb_state(false);pm_ack_probe_live(9,false);hb();assert(!probe.summary.count);
 pm_ack_probe_live(9,true);pm_ack_probe_before_lock(9,"{}");assert(!probe.summary.count);
 pm_ack_probe_connection();pm_ack_probe_live(10,true);hb();assert(probe.trigger.flags&4);
 puts("PASS inactive/wrong kind/reconnect gates");
 setup("AP2 ARM_HB 1 f");command("AP2 ARM 1 f");assert(strstr(output,"error=retained"));
 command("AP2 ARM_HB 2 f");assert(strstr(output,"error=boot"));
 command("AP2 ARM_HB 1 f");assert(strstr(output,"state=4"));
 pm_ack_probe_usb_state(false);hb();command("AP2 FREEZE 1 f");d=probe.summary.digest;
 command("AP2 ARM_HB 1 10");assert(strstr(output,"error=retained"));
 char cmd[96];snprintf(cmd,sizeof cmd,"AP2 RELEASE 1 f %x",d);command(cmd);assert(probe.summary.state==PM_AP_IDLE&&!probe.trigger.mode);
 command("AP2 ARM_HB 1 f");assert(strstr(output,"error=old_run"));
 puts("PASS mode/boot/idempotence/retention/release/old-run");
 setup("AP2 ARM 1 1");pm_ack_probe_usb_state(false);hb();assert(probe.summary.state==PM_AP_ARMED&&!probe.summary.count);
 pm_ack_probe_button(9,1,1,1,false);assert(probe.summary.state==PM_AP_RECORDING&&probe.records[0].event==1);
 puts("PASS existing input mode unchanged");
 setup("AP2 ARM_HB 1 1");pm_ack_probe_usb_state(false);probe.message=UINT32_MAX;hb();assert(probe.trigger.flags&8);
 assert(!probe.summary.count&&probe.summary.state==PM_AP_ARMED);
 puts("PASS ordinal wrap cannot create an aliased target");
 printf("PASS heartbeat state bytes=%zu totalAP2=%zu\n",sizeof probe.trigger,sizeof probe);
 return 0;
}
