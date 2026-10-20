#define main baseline_sim_cases
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wreturn-type"
#include "button_sim_test.c"
#pragma GCC diagnostic pop
#undef main
int main(void){
 pm_button_sim s;setup(&s,4,false);assert(pm_sim_begin(&s,1,4,9,3,0)==PM_SIM_OK);
 for(unsigned i=0;i<4;i++)assert(pm_sim_step_set(&s,1,i,i*200,1,100)==PM_SIM_OK);
 assert(pm_sim_step_set(&s,1,4,800,1,100)==PM_SIM_INVALID);assert(pm_sim_commit(&s,1,0)==PM_SIM_OK);tick(&s,false,0,true,false,1800,0);
 unsigned dispatched=0;for(unsigned t=0;t<=1300;t+=5){pm_sim_dispatch e=tick(&s,false,0,true,false,1800,t);if(e.valid){dispatched++;dispatch(&s,e,t,false);}}
 assert(dispatched==4&&s.phase==PM_SIM_DONE);for(unsigned i=0;i<4;i++)assert(s.steps[i].accepted&&s.steps[i].dispatched&&s.steps[i].released&&!s.steps[i].cancelled);
 setup(&s,4,false);assert(pm_sim_begin(&s,1,2,9,3,0)==PM_SIM_OK);for(unsigned i=0;i<2;i++)pm_sim_step_set(&s,1,i,i*200,1,100);pm_sim_commit(&s,1,0);tick(&s,false,0,true,false,1800,0);tick(&s,false,0,true,false,1800,500);pm_sim_dispatch e=tick(&s,false,0,true,false,1800,535);assert(e.valid);dispatch(&s,e,535,false);
 assert(!tick(&s,false,4,true,false,1800,540).valid&&s.phase==PM_SIM_CANCELLED&&s.steps[0].dispatched&&!s.steps[1].dispatched);
 for(unsigned t=545;t<1500;t+=5)assert(!tick(&s,false,0,true,false,1800,t).valid);
 puts("PASS4-step capacity+1 boundary, full atomic commit, per-step dispatch/release, physical priority after first dispatch preserves history and cancels remainder");
}
