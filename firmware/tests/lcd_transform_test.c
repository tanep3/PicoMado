#include "lcd_transform.h"
#include <assert.h>
#include <string.h>

int main(void) {
    // Known outputs of the verified YouTube sender's inverse + R/B swap.
    assert(pm_panel_compensate(0x0000)==0xffff);
    assert(pm_panel_compensate(0xffff)==0x0000);
    assert(pm_panel_compensate(0xf800)==0xffe0);
    assert(pm_panel_compensate(0x001f)==0x07ff);
    assert(pm_panel_compensate(0x07e0)==0xf81f);
    for(unsigned c=0;c<=0xffff;c++)
        assert(pm_panel_compensate(pm_panel_compensate((uint16_t)c))==c);

    uint8_t image[]={0x00,0x00, 0xf8,0x00, 0x07,0xe0, 0x00,0x1f};
    uint8_t original[sizeof image];memcpy(original,image,sizeof image);
    pm_lcd_transform(image,4,false,false);
    assert(!memcmp(image,original,sizeof image));
    pm_lcd_transform(image,4,true,true);
    const uint8_t expected[]={0x07,0xff, 0xf8,0x1f, 0xff,0xe0, 0xff,0xff};
    assert(!memcmp(image,expected,sizeof image));
    pm_lcd_transform(image,4,true,true);
    assert(!memcmp(image,original,sizeof image));

    // A full 160x80 frame keeps each two-byte pixel and its row stride intact.
    static uint8_t full[160*80*2];
    for(unsigned y=0;y<80;y++)for(unsigned x=0;x<160;x++){
        uint16_t c=(uint16_t)(y*160+x);
        size_t i=2*(y*160+x);full[i]=(uint8_t)(c>>8);full[i+1]=(uint8_t)c;
    }
    pm_lcd_transform(full,160*80,true,true);
    for(unsigned y=0;y<80;y++)for(unsigned x=0;x<160;x++){
        uint16_t source=(uint16_t)((79-y)*160+(159-x));
        uint16_t expected_pixel=pm_panel_compensate(source);
        size_t i=2*(y*160+x);
        assert(full[i]==(uint8_t)(expected_pixel>>8));
        assert(full[i+1]==(uint8_t)expected_pixel);
    }
    return 0;
}
