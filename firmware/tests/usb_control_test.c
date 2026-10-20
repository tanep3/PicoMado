#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "usb_control.h"
#include "ack_probe.h"
#include "trace.h"
static uint32_t now_ms;
static bool writable=true;
static char output[40000];static size_t used;
uint64_t time_us_64(void){return (uint64_t)now_ms*1000;}
bool pm_trace_try_line(const char *line,size_t n){if(!writable)return false;assert(n<144&&used+n<sizeof output);memcpy(output+used,line,n);used+=n;output[used]=0;return true;}
static pm_sim_dispatch tick(bool connected,unsigned physical,bool allowed,unsigned state,uint32_t t){now_ms=t;pm_ack_probe_usb_state(connected);return pm_usb_control_tick(connected,physical,allowed,1,state,state==5,1800,t);}
static void drain(void){for(unsigned i=0;i<400;i++){now_ms+=5;tick(true,0,true,3,now_ms);pm_usb_control_poll(true);}}
static void input(const char *s){while(*s)pm_usb_control_input(*s++);}
static void command(const char *s){input(s);pm_usb_control_input('\n');drain();}
static void reset(unsigned count){now_ms=100;used=0;output[0]=0;writable=true;pm_ack_probe_init_boot(1);pm_usb_control_init(1,count);tick(true,0,true,3,100);}
int main(void){
 reset(4);input("AP2 STATUS\nSIM CAPS\n");drain();assert(strstr(output,"ap2 status")&&strstr(output,"sim caps"));
 command("SIM BEGIN 1 1 a 1 1 3");assert(pm_usb_sim_status()->phase==PM_SIM_STAGING);
 command("SIM STEP 1 2 a 0 0 1 64");command("SIM COMMIT 1 3 a");assert(pm_usb_sim_status()->phase==PM_SIM_WAIT_CLOSE);
 uint32_t deadline=pm_usb_sim_status()->lease_end_ms;
 command("SIM COMMIT 1 3 a");assert(pm_usb_sim_status()->lease_end_ms==deadline);
 command("SIM COMMIT 1 3 b");assert(strstr(output,"error=conflict")&&pm_usb_sim_status()->id==10);
 tick(false,0,true,3,now_ms);uint32_t start=pm_usb_sim_status()->start_ms;assert(!tick(false,0,true,3,start).valid);
 pm_sim_dispatch e=tick(false,0,true,3,start+35);assert(e.valid&&e.logical==1);pm_usb_control_dispatched(e,1,3,start+35);
 assert(!tick(false,0,true,3,start+100).valid);tick(false,0,true,3,start+135);assert(pm_usb_sim_status()->steps[0].released&&pm_usb_sim_status()->steps[0].dispatched);
 // Same completed command cannot arm another run; old evicted IDs never execute.
 tick(true,0,true,3,start+200);command("SIM COMMIT 1 3 a");assert(pm_usb_sim_status()->phase==PM_SIM_DONE);
 command("SIM BEGIN 1 4 a 1 1 3");assert(strstr(output,"result=old_job"));
 for(unsigned i=5;i<16;i++){char line[64];snprintf(line,sizeof line,"SIM STOP 1 %x",i);command(line);}
 command("SIM BEGIN 1 1 b 1 1 3");assert(strstr(output,"error=old_command")&&pm_usb_sim_status()->id==10);
 command("SIM BEGIN 2 20 b 1 1 3");assert(strstr(output,"error=boot"));
 // Overflow/sign/extra arguments never create a job.
 input("SIM ");for(unsigned i=0;i<140;i++)pm_usb_control_input('X');command("");assert(strstr(output,"usb error=length"));
 command("SIM PRESS 1 20 b 1 10000000000000000");assert(pm_usb_sim_status()->id==10);
 command("SIM PRESS 1 21 b -1 100");assert(pm_usb_sim_status()->id==10);
 command("SIM PRESS 1 22 b 1 100 extra");assert(pm_usb_sim_status()->id==10);
 // Immediate disconnect and hard lease cancel without a two-button click.
 reset(2);command("SIM PRESS 1 1 a 0 1000");uint32_t t=now_ms;tick(true,0,true,3,t);assert(!tick(true,0,true,3,t+35).valid);assert(!tick(false,0,true,3,t+40).valid&&pm_usb_sim_status()->phase==PM_SIM_CANCELLED);
 // Safety STOP is processed even while both reply slots and AP2 export are busy.
 reset(2);command("SIM PRESS 1 1 a 0 1000");t=now_ms;tick(true,0,true,3,t);tick(true,0,true,3,t+35);writable=false;
 input("SIM CAPS\nSIM STATUS 1\nAP2 STATUS\nSIM STOP 1 2\n");assert(pm_usb_sim_status()->phase==PM_SIM_CANCELLED&&!pm_usb_sim_status()->virtual_button.stable);writable=true;drain();command("SIM ACK 1 2");assert(strstr(output,"cid=2"));
 // Detached commit exception does not apply to a staging-only job.
 reset(4);command("SIM BEGIN 1 1 a 1 1 3");tick(false,0,true,3,now_ms);assert(pm_usb_sim_status()->phase==PM_SIM_CANCELLED);
 // Queue bound and zero-input capability are explicit rejections.
 reset(0);command("SIM PRESS 1 1 a 0 100");assert(pm_usb_sim_status()->phase==PM_SIM_IDLE);
 reset(4);command("SIM BEGIN 1 1 a 5 1 3");assert(pm_usb_sim_status()->phase==PM_SIM_IDLE);
 // Parser recovery on CDC close discards an incomplete command.
 reset(4);input("SIM PRESS 1 1 a 1");pm_usb_control_poll(false);command(" 100");assert(pm_usb_sim_status()->phase==PM_SIM_IDLE);
 puts("PASS one USB parser: AP2/SIM interleave, idempotent commands, conflict/old ID, bounds, partial/disconnect, emergency cancel with blocked replies");
}
