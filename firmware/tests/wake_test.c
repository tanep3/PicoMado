#include <assert.h>
#include <stdio.h>
#include "core.h"
static void four_buttons(void){
    for(unsigned i=0;i<4;i++){
        pm_button b={0};
        assert(!pm_button_wake_step(&b,4,i,true,100,1500));
        assert(!pm_button_wake_step(&b,4,i,true,134,1500));
        assert(pm_button_wake_step(&b,4,i,true,135,1500)==(i==2));
        assert(!pm_button_wake_step(&b,4,i,true,2000,1500));
        assert(!pm_button_wake_step(&b,4,i,false,2001,1500));
        assert(!pm_button_wake_step(&b,4,i,false,2036,1500));
        assert(!pm_button_wake_step(&b,4,i,true,2100,1500));
        assert(pm_button_wake_step(&b,4,i,true,2135,1500)==(i==2));
    }
}
static void two_buttons(uint32_t duration){
    for(unsigned i=0;i<2;i++){
        pm_button b={0};
        assert(!pm_button_wake_step(&b,2,i,true,100,duration));
        assert(!pm_button_wake_step(&b,2,i,true,135,duration));
        assert(!pm_button_wake_step(&b,2,i,true,200,duration));
        assert(!pm_button_wake_step(&b,2,i,false,201,duration));
        assert(!pm_button_wake_step(&b,2,i,false,236,duration));
        assert(!pm_button_wake_step(&b,2,i,true,500,duration));
        assert(!pm_button_wake_step(&b,2,i,true,535,duration));
        assert(!pm_button_wake_step(&b,2,i,true,535+duration-1,duration));
        assert(pm_button_wake_step(&b,2,i,true,535+duration,duration)==(i==1));
        assert(!pm_button_wake_step(&b,2,i,true,535+duration+100,duration));
        assert(!pm_button_wake_step(&b,2,i,false,700+duration,duration));
        assert(!pm_button_wake_step(&b,2,i,false,735+duration,duration));
    }
    pm_button b={0};uint32_t start=UINT32_MAX-100;
    assert(!pm_button_wake_step(&b,2,1,true,start,duration));
    assert(!pm_button_wake_step(&b,2,1,true,start+35,duration));
    assert(pm_button_wake_step(&b,2,1,true,start+35+duration,duration));
    // Entering standby with a button already held requires release/re-press.
    b=(pm_button){0};assert(!pm_button_step(&b,true,0));assert(pm_button_step(&b,true,35));
    assert(!pm_button_wake_step(&b,2,1,true,5000,duration));
    // A raw release before the threshold never wakes, even during release debounce.
    b=(pm_button){0};
    assert(!pm_button_wake_step(&b,2,1,true,0,duration));
    assert(!pm_button_wake_step(&b,2,1,true,35,duration));
    assert(!pm_button_wake_step(&b,2,1,false,35+duration-1,duration));
    assert(!pm_button_wake_step(&b,2,1,false,35+duration,duration));
}
int main(void){
    four_buttons();two_buttons(1500);two_buttons(2000);
    assert(!pm_button_wakes(4,0,true));assert(!pm_button_wakes(4,1,true));
    assert(!pm_button_wakes(4,3,true));assert(!pm_button_wakes(2,0,true));
    assert(!pm_button_wakes(2,1,false));assert(!pm_button_wakes(3,2,true));
    puts("wake PASS: four GPIO roles POWER only at35ms; two-button DOWN only at1500/2000ms; short/bounce/release/held/wrap");
}
