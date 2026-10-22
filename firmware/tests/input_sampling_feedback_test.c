// Production E-ink driver/core with the existing SPI/time/BUSY fixture.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wreturn-type"
#define main pm_sampling_existing_eink_cases
#include "eink_test.c"
#undef main
#pragma GCC diagnostic pop
#include "core.h"
#include <stdlib.h>
int main(int argc,char **argv){
    assert(argc==2);unsigned scenario=(unsigned)strtoul(argv[1],NULL,10);assert(scenario<7);
    pm_button button={0};unsigned subs=0,ups=0,busy_samples=0;
    const uint32_t starts[]={100,250,500,1000,2550,1000,0};
    pm_display_init();
    for(clock_ms=0;clock_ms<=3500;clock_ms+=5){
        if(clock_ms==200)pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);
        busy=refreshes>0&&clock_ms<2500;
        size_t before=writes;pm_display_poll();assert(writes-before<140);
        bool down=clock_ms>=starts[scenario]&&clock_ms<starts[scenario]+100;
        if(scenario==5)down|=clock_ms>=2000&&clock_ms<2100;
        if(scenario==6){
            down=clock_ms<100||(clock_ms>=1900&&clock_ms<2000)||(clock_ms>=3000&&clock_ms<3100);
            // Injected sample omission, not a stall produced by the driver.
            if(clock_ms>40&&clock_ms<1940)continue;
        }
        if(busy)busy_samples++;
        pm_operation op;bool held;
        if(pm_button_sampled_binding_step(&button,pm_button_binding_at(2,0),down,clock_ms,1800,false,&op,&held)){
            if(op.value==PM_OP_SUB)subs++;else {assert(op.value==PM_OP_UP);ups++;}
        }
    }
    assert(!subs&&ups==(scenario==5?2u:1u)&&refreshes==1&&busy_samples);
    printf("PASS sampled feedback phase=%u UP=%u SUB=%u busy_samples=%u injected_gap=%u\n",scenario,ups,subs,busy_samples,scenario==6);
    return 0;
}
