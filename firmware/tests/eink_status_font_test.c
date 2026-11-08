// Capture the actual V4 driver RAM write, decode its established landscape map.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wreturn-type"
#define main existing_eink_cases
#include "eink_test.c"
#undef main
#pragma GCC diagnostic pop
#include "status_pixel_expect.h"
int main(int argc,char **argv){
    (void)existing_eink_cases();assert(PM_EINK_PANEL_CONNECTED);busy=false;pm_display_wake();
    const char *labels[]={"HOME","HOME2","AWAY","2","HOME?","HOME2HOME2HOME2HOME2HOME2",""};
    static uint16_t logical[PM_W*PM_H];
    for(unsigned i=0;i<sizeof labels/sizeof labels[0];i++){
        pm_display_status("NETWORK",labels[i]);poll_for(2000);
        assert(!pm_display_busy()&&written==4000);
        for(unsigned y=0;y<PM_H;y++)for(unsigned x=0;x<PM_W;x++){
            unsigned u=121-y;logical[y*PM_W+x]=(output[(u/8)*250+x]&(0x80u>>(u&7)))?0xffff:0;
        }
        expect_status(logical,labels[i],35,73,20,0xffff,0,0);
        if(i==1&&argc>1)save_ppm(argv[1],logical);
    }
    puts("PASS production E-ink status pixels: HOME/HOME2/AWAY, digit2 row oracle, V4 landscape RAM, blank/unsupported, clipping");
    return 0;
}
