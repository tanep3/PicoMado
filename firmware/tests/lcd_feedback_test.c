// SPI/DC/CS fixture follows lcd_bus_test.c, accepting both a complete image
// payload and the bounded feedback scanlines through the real LCD adapter.
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "lcd.h"
#include "lcd_transform.h"
#include "switch_bitmap.h"
#include "trace.h"
#include "pico/stdlib.h"
#include "hardware/spi.h"

uint32_t save_and_disable_interrupts(void){return 0;}
void restore_interrupts(uint32_t state){(void)state;}
#if PM_TRACE_ENABLE
static unsigned trace_begin,trace_end;
uint64_t pm_trace_current_frame(void){return 0;}
void pm_trace_event(pm_trace_kind kind,uint64_t frame,int32_t a,uint32_t b){
    assert(frame==0);
    if(kind==PM_TRACE_SPI_BEGIN)trace_begin++;
    if(kind==PM_TRACE_SPI_END){assert(a==PM_IMAGE_BYTES&&b>0);trace_end++;}
}
int pm_logf(const char *format,...){(void)format;return 0;}
#endif
static spi_inst_t instance;
spi_inst_t *spi0=&instance;
static int levels[30],outputs[30];
static unsigned baud,rate_changes,frames,cs_asserts,cs_releases;
static unsigned busy_remaining,busy_waits,payload_calls;
static unsigned expected_control=1000000,expected_frame=20000000;
static uint8_t command,window_x[4],window_y[4],captured[PM_IMAGE_BYTES];
static size_t used;

void gpio_init(uint pin){assert(pin<30);outputs[pin]=0;}
void gpio_set_dir(uint pin,int value){assert(pin<30&&value==GPIO_OUT);outputs[pin]=1;}
void gpio_put(uint pin,int value){
    assert(pin<30);
    if(pin==PM_LCD_PIN_CS&&outputs[pin]&&value!=levels[pin]){
        if(value){assert(!busy_remaining);cs_releases++;}else cs_asserts++;
    }
    levels[pin]=value;
}
void gpio_set_function(uint pin,int function){
    assert((pin==PM_LCD_PIN_SCK||pin==PM_LCD_PIN_MOSI)&&function==GPIO_FUNC_SPI);
}
void sleep_ms(unsigned ms){assert(ms>0);}
unsigned spi_init(spi_inst_t *spi,unsigned requested){assert(spi==spi0);baud=requested;return requested;}
unsigned spi_set_baudrate(spi_inst_t *spi,unsigned requested){
    assert(spi==spi0&&levels[PM_LCD_PIN_CS]&&requested>0);
    rate_changes++;baud=requested;return requested;
}
unsigned spi_get_baudrate(const spi_inst_t *spi){assert(spi==spi0);return baud;}
bool spi_is_busy(const spi_inst_t *spi){assert(spi==spi0);return busy_remaining>0;}
void tight_loop_contents(void){assert(busy_remaining>0);busy_remaining--;busy_waits++;}
void spi_set_format(spi_inst_t *spi,int bits,int cp,int ph,int order){
    assert(spi==spi0&&bits==8&&!cp&&!ph&&!order);
}
int spi_write_blocking(spi_inst_t *spi,const uint8_t *bytes,size_t length){
    assert(spi==spi0&&!levels[PM_LCD_PIN_CS]&&outputs[PM_LCD_PIN_CS]&&outputs[PM_LCD_PIN_DC]);
    if(!levels[PM_LCD_PIN_DC]){
        assert(baud==expected_control&&length==1);
        command=*bytes;
        if(command==0x2c){used=0;payload_calls=0;}
    }else if(command==0x2c){
        assert(baud==expected_frame&&length>0&&length<=PM_IMAGE_BYTES-used);
        assert(length==PM_IMAGE_BYTES||length==PM_W*2);
        memcpy(captured+used,bytes,length);used+=length;payload_calls++;
        if(used==PM_IMAGE_BYTES){
            frames++;
#if defined(PM_LCD_FRAME_CS_HOLD) && PM_LCD_FRAME_CS_HOLD
            if(expected_control==expected_frame)busy_remaining=2;
#endif
        }
    }else {
        assert(baud==expected_control);
        if(command==0x2a){assert(length==4);memcpy(window_x,bytes,4);}
        if(command==0x2b){assert(length==4);memcpy(window_y,bytes,4);}
    }
    return (int)length;
}
typedef struct {unsigned frames,changes,asserts,releases,waits;} bus_snapshot;
static bus_snapshot snapshot(void){
    return (bus_snapshot){frames,rate_changes,cs_asserts,cs_releases,busy_waits};
}
static void unchanged(bus_snapshot before){
    assert(frames==before.frames&&rate_changes==before.changes);
    assert(cs_asserts==before.asserts&&cs_releases==before.releases&&busy_waits==before.waits);
}
static void transferred(bus_snapshot before,const uint8_t *expected,bool hint){
    assert(frames==before.frames+1&&used==PM_IMAGE_BYTES);
    assert(!memcmp(captured,expected,PM_IMAGE_BYTES));
    assert(payload_calls==(hint?PM_H:1));
    assert(pm_lcd_last_frame_baud()==expected_frame&&pm_lcd_last_transfer_bytes()==PM_IMAGE_BYTES);
    assert(baud==expected_control&&levels[PM_LCD_PIN_CS]&&!busy_remaining);
    const uint8_t x[]={PM_LCD_X_OFFSET>>8,PM_LCD_X_OFFSET,
        (PM_LCD_X_OFFSET+PM_W-1)>>8,PM_LCD_X_OFFSET+PM_W-1};
    const uint8_t y[]={PM_LCD_Y_OFFSET>>8,PM_LCD_Y_OFFSET,
        (PM_LCD_Y_OFFSET+PM_H-1)>>8,PM_LCD_Y_OFFSET+PM_H-1};
    assert(!memcmp(window_x,x,4)&&!memcmp(window_y,y,4));
    assert(rate_changes==before.changes+(expected_control!=expected_frame?2:0));
#if defined(PM_LCD_FRAME_CS_HOLD) && PM_LCD_FRAME_CS_HOLD
    bool held=expected_control==expected_frame;
#else
    bool held=false;
#endif
    assert(cs_asserts==before.asserts+(held?1:6)&&cs_releases==before.releases+(held?1:6));
    assert(busy_waits==before.waits+(held?2:0));
#if PM_TRACE_ENABLE
    assert(trace_begin==trace_end&&trace_end==frames);
#endif
}
int main(void){
    static uint8_t image[PM_IMAGE_BYTES],expected[PM_IMAGE_BYTES],hint[PM_IMAGE_BYTES],twice[PM_IMAGE_BYTES];
    // Build the canonical two-line image independently from the centering API,
    // then apply the normal pixel transform once to obtain its expected wire.
    unsigned left=(PM_W-PM_SWITCH_BITMAP_W)/2,top=(PM_H-PM_SWITCH_BITMAP_H)/2;
    for(unsigned y=0;y<PM_H;y++)for(unsigned x=0;x<PM_W;x++){
        bool ink=false;
        if(x>=left&&x<left+PM_SWITCH_BITMAP_W&&y>=top&&y<top+PM_SWITCH_BITMAP_H){
            unsigned b=(y-top)*PM_SWITCH_BITMAP_W+x-left;
            ink=!!(pm_switch_bitmap[b/8]&(0x80u>>(b&7)));
        }
        size_t b=2*(y*PM_W+x);hint[b]=hint[b+1]=ink?0xff:0;
    }
    pm_lcd_transform(hint,PM_W*PM_H,PM_LCD_ROTATE_180,PM_LCD_COMPENSATE_INVERT_BGR);
    pm_lcd_configure_bus(expected_control,expected_frame);
    pm_lcd_init();
    const unsigned rates[][2]={{1000000,20000000},{20000000,20000000},{5000000,5000000}};
    for(unsigned rate=0;rate<sizeof rates/sizeof rates[0];rate++){
        expected_control=rates[rate][0];expected_frame=rates[rate][1];
        pm_lcd_configure_bus(expected_control,expected_frame);
        for(size_t i=0;i<sizeof image;i++)image[i]=(uint8_t)(i*17u+3u+rate);
        memcpy(expected,image,sizeof expected);
        pm_lcd_transform(expected,PM_W*PM_H,PM_LCD_ROTATE_180,PM_LCD_COMPENSATE_INVERT_BGR);
        memcpy(twice,expected,sizeof twice);
        pm_lcd_transform(twice,PM_W*PM_H,PM_LCD_ROTATE_180,PM_LCD_COMPENSATE_INVERT_BGR);
        assert(memcmp(twice,expected,sizeof expected)); // Double conversion is observable.
        bus_snapshot before=snapshot();pm_lcd_draw(image);transferred(before,expected,false);
        assert(!memcmp(image,expected,sizeof image));memset(image,0x12,sizeof image);
        before=snapshot();pm_lcd_switch_feedback(true,false);transferred(before,hint,true);
        before=snapshot();pm_lcd_switch_feedback(true,false);unchanged(before);
        pm_lcd_switch_feedback(false,false);unchanged(before); // REPLACE does not repaint.
        pm_lcd_switch_feedback(true,false);unchanged(before); // Visible hint is not repeated.
        before=snapshot();pm_lcd_switch_feedback(false,true);transferred(before,expected,false);
        before=snapshot();pm_lcd_switch_feedback(false,true);unchanged(before);

        // A newly arrived real image supersedes the hint and replaces its cache.
        pm_lcd_switch_feedback(true,false);
        for(size_t i=0;i<sizeof image;i++)image[i]=(uint8_t)(i*7u+91u+rate);
        memcpy(expected,image,sizeof expected);
        pm_lcd_transform(expected,PM_W*PM_H,PM_LCD_ROTATE_180,PM_LCD_COMPENSATE_INVERT_BGR);
        before=snapshot();pm_lcd_draw(image);transferred(before,expected,false);
        before=snapshot();pm_lcd_switch_feedback(false,true);unchanged(before);
        before=snapshot();pm_lcd_switch_feedback(true,false);transferred(before,hint,true);
        memset(image,0xee,sizeof image);
        before=snapshot();pm_lcd_switch_feedback(false,true);transferred(before,expected,false);
    }
    puts("lcd feedback: exact two-line transformed bitmap, duplicate/REPLACE suppression, owned wire cache and no-op restore, new-frame replacement, same/different baud, CS/DC/BUSY PASS");
    return 0;
}
