#include <assert.h>
#include <stdio.h>
#include <stdint.h>
#include "button_sim.h"
static pm_sim_dispatch tick(pm_button_sim *s,bool usb,unsigned real,bool allowed,bool standby,uint32_t long_ms,uint32_t t){return pm_sim_tick(s,usb,real,allowed,9,standby?5:3,standby,long_ms,t);}
static void setup(pm_button_sim *s,unsigned buttons,bool standby){pm_sim_init(s,buttons);tick(s,true,0,true,standby,1800,0);}
static void dispatch(pm_button_sim *s,pm_sim_dispatch e,uint32_t t,bool standby){if(e.valid)pm_sim_dispatched(s,e.step,e.logical,t,9,standby?5:3);}
int main(void){
 pm_button_sim s;pm_sim_dispatch e;
 setup(&s,4,false);assert(pm_sim_begin(&s,1,1,9,3,0)==PM_SIM_OK);assert(pm_sim_step_set(&s,1,0,0,1,100)==PM_SIM_OK);assert(pm_sim_commit(&s,1,0)==PM_SIM_OK);
 assert(!tick(&s,true,0,true,false,1800,100).valid && s.phase==PM_SIM_WAIT_CLOSE);
 assert(!tick(&s,false,0,true,false,1800,100).valid);assert(!tick(&s,false,0,true,false,1800,599).valid);
 assert(!tick(&s,false,0,true,false,1800,600).valid);assert(!tick(&s,false,0,true,false,1800,634).valid);
 e=tick(&s,false,0,true,false,1800,635);assert(e.valid&&e.logical==1);dispatch(&s,e,635,false);
 assert(!tick(&s,false,0,true,false,1800,700).valid);tick(&s,false,0,true,false,1800,735);assert(s.phase==PM_SIM_DONE&&s.steps[0].released&&s.steps[0].dispatched);
 assert(pm_sim_begin(&s,1,1,9,3,800)==PM_SIM_OLD_JOB);
 // Two-button normal release is the only short-click completion path.
 setup(&s,2,false);assert(pm_sim_press(&s,1,0,1000,9,3,0)==PM_SIM_OK);tick(&s,true,0,true,false,1800,0);tick(&s,true,0,true,false,1800,35);
 assert(pm_sim_release(&s,1,100)==PM_SIM_OK);assert(!tick(&s,true,0,true,false,1800,100).valid);e=tick(&s,true,0,true,false,1800,135);assert(e.valid&&e.logical==1);dispatch(&s,e,135,false);assert(s.steps[0].released&&s.phase==PM_SIM_DONE);
 // Lease, disconnect and physical takeover reset, never synthesize short release.
 for(unsigned why=0;why<3;why++){
  setup(&s,2,false);assert(pm_sim_press(&s,1,0,100,9,3,0)==PM_SIM_OK);tick(&s,true,0,true,false,1800,0);tick(&s,true,0,true,false,1800,35);
  e=tick(&s,why!=1,why==2?1:0,true,false,1800,why==0?100:50);assert(!e.valid&&s.phase==PM_SIM_CANCELLED&&!s.virtual_button.stable);assert(!tick(&s,true,0,true,false,1800,200).valid);
 }
 // Long threshold is the existing FSM's stable-edge snapshot, not later settings.
 setup(&s,2,false);assert(pm_sim_press(&s,1,0,3000,9,3,0)==PM_SIM_OK);tick(&s,true,0,true,false,1800,0);tick(&s,true,0,true,false,1800,35);
 assert(!tick(&s,true,0,true,false,2000,1834).valid);e=tick(&s,true,0,true,false,2000,1835);assert(e.valid&&e.logical==0&&e.long_press);dispatch(&s,e,1835,false);
 assert(pm_sim_release(&s,1,1900)==PM_SIM_OK);tick(&s,true,0,true,false,2000,1900);assert(!tick(&s,true,0,true,false,2000,1935).valid);
 // STANDBY capabilities are shared with physical input.
 setup(&s,4,true);assert(pm_sim_press(&s,1,1,1000,9,5,0)==PM_SIM_OK);tick(&s,true,0,true,true,1800,0);assert(!tick(&s,true,0,true,true,1800,35).valid);
 setup(&s,4,true);assert(pm_sim_press(&s,1,2,1000,9,5,0)==PM_SIM_OK);tick(&s,true,0,true,true,1800,0);e=tick(&s,true,0,true,true,1800,35);assert(e.valid&&e.logical==2);
 setup(&s,2,true);assert(pm_sim_press(&s,1,1,3000,9,5,0)==PM_SIM_OK);tick(&s,true,0,true,true,1800,0);tick(&s,true,0,true,true,1800,35);e=tick(&s,true,0,true,true,1800,1835);assert(e.valid&&e.logical==2);
 setup(&s,0,false);assert(pm_sim_begin(&s,1,1,9,3,0)==PM_SIM_UNSUPPORTED);assert(pm_sim_press(&s,1,0,1000,9,3,0)==PM_SIM_UNSUPPORTED);
 // BUSY/stale scheduled work and session changes cannot catch up later.
 setup(&s,4,false);assert(pm_sim_begin(&s,1,1,9,3,0)==PM_SIM_OK);assert(pm_sim_step_set(&s,1,0,0,1,100)==PM_SIM_OK);assert(pm_sim_commit(&s,1,0)==PM_SIM_OK);tick(&s,false,0,true,false,1800,0);tick(&s,false,0,false,false,1800,500);assert(s.phase==PM_SIM_CANCELLED);assert(!tick(&s,false,0,true,false,1800,1000).valid);
 setup(&s,4,false);assert(pm_sim_begin(&s,1,1,9,3,0)==PM_SIM_OK);assert(pm_sim_step_set(&s,1,0,0,1,100)==PM_SIM_OK);assert(pm_sim_commit(&s,1,0)==PM_SIM_OK);tick(&s,false,0,true,false,1800,0);assert(!tick(&s,false,0,true,false,1800,600).valid&&s.phase==PM_SIM_CANCELLED);
 setup(&s,4,false);assert(pm_sim_press(&s,1,1,1000,9,3,0)==PM_SIM_OK);e=pm_sim_tick(&s,true,0,true,10,3,false,1800,1);assert(!e.valid&&s.phase==PM_SIM_CANCELLED);
 // Command deadlines do not extend across wrap or duplicate commit.
 setup(&s,4,false);uint32_t start=UINT32_MAX-20;assert(pm_sim_press(&s,1,1,200,9,3,start)==PM_SIM_OK);tick(&s,true,0,true,false,1800,start);e=tick(&s,true,0,true,false,1800,start+35);assert(e.valid);tick(&s,true,0,true,false,1800,start+200);assert(s.phase==PM_SIM_CANCELLED);
 setup(&s,4,false);assert(pm_sim_begin(&s,1,1,9,3,0)==PM_SIM_OK);assert(pm_sim_step_set(&s,1,0,0,1,100)==PM_SIM_OK);assert(pm_sim_commit(&s,1,0)==PM_SIM_OK);assert(pm_sim_commit(&s,1,5000)==PM_SIM_OK);tick(&s,true,0,true,false,1800,10000);assert(s.phase==PM_SIM_CANCELLED);
 puts("PASS virtual FSM: finite schedule, CDC-only commit, normal vs cancelled release, physical priority,0/2/4,STANDBY,lease,BUSY,session,wrap");
}
