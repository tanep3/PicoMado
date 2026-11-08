// Capture actual LCD SPI output, then invert the existing transport transform.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wreturn-type"
#define main existing_lcd_feedback_cases
#include "lcd_feedback_test.c"
#undef main
#pragma GCC diagnostic pop
#include "status_pixel_expect.h"
int main(int argc,char **argv){
    (void)existing_lcd_feedback_cases();
    expected_control=expected_frame=20000000;
    pm_lcd_configure_bus(expected_control,expected_frame);
    const char *labels[]={"HOME","HOME2","AWAY","2","HOME?","HOME2HOME2HOME2HOME2",""};
    static uint16_t logical[PM_W*PM_H];static uint8_t decoded[PM_IMAGE_BYTES];
    for(unsigned i=0;i<sizeof labels/sizeof labels[0];i++){
        pm_lcd_local("NETWORK",labels[i]);assert(used==PM_IMAGE_BYTES);
        memcpy(decoded,captured,sizeof decoded);
        pm_lcd_transform(decoded,PM_W*PM_H,PM_LCD_ROTATE_180,PM_LCD_COMPENSATE_INVERT_BGR);
        for(unsigned p=0;p<PM_W*PM_H;p++)logical[p]=(uint16_t)((decoded[2*p]<<8)|decoded[2*p+1]);
        expect_status(logical,labels[i],20,56,13,0,0xffff,0x07ff);
        if(i==1&&argc>1)save_ppm(argv[1],logical);
    }
    puts("PASS production LCD status pixels: HOME/HOME2/AWAY, digit2 row oracle, rotation/compensation, blank/unsupported, clipping");
    return 0;
}
