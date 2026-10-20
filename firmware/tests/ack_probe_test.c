#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include "ack_probe.h"
#include "trace.h"
#include "core.h"
static uint64_t clock_us=100;
static unsigned writes,irq_depth;
static bool usb;
static char output[65536];static size_t used;
uint64_t time_us_64(void){return clock_us;}
uint32_t save_and_disable_interrupts(void){irq_depth++;return 0;}
void restore_interrupts(uint32_t prior){(void)prior;assert(irq_depth);irq_depth--;}
size_t pm_trace_usb_write(const char *p,size_t n){if(!usb)return 0;if(n>7)n=7;assert(used+n<sizeof output);memcpy(output+used,p,n);used+=n;output[used]=0;writes++;return n;}
static void drain(void){usb=true;for(unsigned i=0;i<1000;i++){clock_us+=1000;pm_ack_probe_poll(true);pm_trace_flush();}}
static void command(const char *s){pm_ack_probe_command(s);drain();}
static void reset(void){drain();used=0;output[0]=0;pm_ack_probe_init_boot(0x1234);usb=false;}
int main(void){
 reset();pm_ack_probe_connection();clock_us+=UINT64_C(86400000000);pm_ack_probe_poll(true);assert(!writes);assert(pm_ack_probe_read_summary().state==PM_AP_IDLE);
 command("AP2 STATUS");assert(strstr(output,"boot=1234")&&strstr(output,"state=0"));command("AP2 ARM 1234 77");assert(pm_ack_probe_read_summary().state==PM_AP_PREPARED);
 clock_us+=UINT64_C(86400000000);pm_ack_probe_poll(false);pm_ack_probe_poll(true);assert(pm_ack_probe_read_summary().state==PM_AP_PREPARED); // days of cue waiting do not expire
 command("AP2 ARM 1234 77");command("AP2 ARM 1234 88");assert(strstr(output,"error=retained"));
 pm_ack_probe_encoded(1,"{}");pm_ack_probe_send_return(1,true);pm_ack_probe_encoded(4,"{}");pm_ack_probe_send_return(4,true);
 usb=false;pm_ack_probe_poll(false);assert(pm_ack_probe_read_summary().state==PM_AP_ARMED);
 pm_ack_probe_button(123,2,4,1,false);uint64_t first=pm_ack_probe_read_summary().trigger_us;
 pm_ack_probe_context(123,456);pm_ack_probe_before_lock(9,"{}");clock_us+=30;pm_ack_probe_locked(9);pm_ack_probe_encoded(9,"{}");pm_ack_probe_net(PM_AP_TX_POSITION,3,100,200,101,90);pm_ack_probe_send_return(9,true);
 pm_ack_probe_record row;bool found=false;for(unsigned i=0;pm_ack_probe_read(i,&row);i++)if(row.event==PM_AP_ENCODED){found=true;assert(row.message==3&&row.connection==1&&row.a==pm_crc32((const uint8_t *)"{}",2));}assert(found);
 clock_us=first+19000000;pm_ack_probe_button(123,3,4,1,false);assert(pm_ack_probe_read_summary().trigger_us==first);clock_us=first+PM_ACK_PROBE_WINDOW_US;pm_ack_probe_poll(false);pm_ack_probe_summary a=pm_ack_probe_read_summary();assert(a.state==PM_AP_FROZEN&&a.reason==1&&a.count>0);uint32_t digest=a.digest;
 clock_us+=UINT64_C(86400000000);pm_ack_probe_connection();pm_ack_probe_poll(true);pm_ack_probe_button(1,1,1,1,false);assert(pm_ack_probe_read_summary().digest==digest&&pm_ack_probe_read_summary().count==a.count);
 command("AP2 STATUS");command("AP2 WINDOW");command("AP2 QUALITY");command("AP2 LIVE");command("AP2 READ 1234 77 0 8");assert(strstr(output,"ap2 page ")&&strstr(output,"ap2 end "));size_t before=used;command("AP2 READ 1234 77 0 8");assert(used>before);assert(pm_ack_probe_read_summary().digest==digest);
 command("AP2 READ 1234 77 8 8");
 const char *fixture=getenv("PM_ACK_EXPORT_FIXTURE");if(fixture){FILE *f=fopen(fixture,"wb");assert(f);assert(fwrite(output,1,used,f)==used);assert(!fclose(f));}
 command("AP2 FREEZE 1234 77");command("AP2 RELEASE 1234 77 0");assert(pm_ack_probe_read_summary().state==PM_AP_FROZEN);
 char cmd[96];snprintf(cmd,sizeof cmd,"AP2 RELEASE 1234 77 %x",digest);command(cmd);assert(pm_ack_probe_read_summary().state==PM_AP_IDLE);command("AP2 ARM 1234 77");assert(strstr(output,"error=old_run"));command("AP2 ARM 1234 88");assert(pm_ack_probe_read_summary().state==PM_AP_PREPARED);
 command("AP2 FREEZE 9999 88");assert(pm_ack_probe_read_summary().state==PM_AP_PREPARED);command("AP2 FREEZE 1234 88");assert(pm_ack_probe_read_summary().state==PM_AP_FROZEN);
 reset();command("AP2 ARM 1234 1");pm_ack_probe_poll(false);pm_ack_probe_button(1,1,1,1,false);for(unsigned i=0;i<PM_ACK_PROBE_CAP+10;i++)pm_ack_probe_note(PM_AP_FRAME_CHECK,0,1,i,0,0,0,0);a=pm_ack_probe_read_summary();assert(a.count==PM_ACK_PROBE_CAP&&a.dropped==12&&a.state==PM_AP_RECORDING);command("AP2 FREEZE 1234 1");assert(pm_ack_probe_read_summary().state==PM_AP_FROZEN);
 for(unsigned i=0;i<200;i++){pm_ack_probe_input('X');}
 pm_ack_probe_input('\n');drain();assert(strstr(output,"error=length"));
 command("AP2 READ 1234 1 0 9");assert(strstr(output,"error=page"));
 // Every partially written line remains whole and retryable; no capture reset.
 for(char *p=output;*p;){char *e=strchr(p,'\n');assert(e);assert(!strncmp(p,"ap2 ",4));p=e+1;}
 reset();command("AP2 ARM 1234 1");assert(pm_ack_probe_read_summary().state==PM_AP_PREPARED);
 pm_ack_probe_button(1,1,1,1,false);a=pm_ack_probe_read_summary();assert(a.state==PM_AP_PREPARED&&!a.trigger_us&&!a.count);
 command("AP2 LIVE");assert(strstr(output,"buttons=1 state=4 trigger=0 begin=0"));
 for(unsigned i=0;i<5;i++){clock_us+=5000000;pm_ack_probe_button(1,1,1,1,false);}
 a=pm_ack_probe_read_summary();assert(a.state==PM_AP_PREPARED&&!a.trigger_us&&!a.count);
 usb=false;pm_ack_probe_poll(false);assert(pm_ack_probe_read_summary().state==PM_AP_ARMED);clock_us+=100;pm_ack_probe_button(1,1,1,1,false);a=pm_ack_probe_read_summary();assert(a.state==PM_AP_RECORDING&&a.gate_buttons==6&&a.trigger_us>=a.gate_us);
 clock_us+=100;pm_ack_probe_usb_state(true);a=pm_ack_probe_read_summary();assert(a.usb_overlap_us&&a.state==PM_AP_RECORDING);command("AP2 FREEZE 1234 1");
 printf("PASS explicit control: indefinite ARM, first-edge20s, repeated input, generation, retained pages, digest RELEASE, bounds, partial USB; RAM=%zu record=%zu\n",pm_ack_probe_ram_bytes(),sizeof(pm_ack_probe_record));
 return 0;
}
