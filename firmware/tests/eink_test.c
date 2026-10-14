#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "display.h"
#include "eink_transform.h"
#include "pico/stdlib.h"
#include "hardware/spi.h"
static spi_inst_t instance;
spi_inst_t *spi1=&instance;
static uint32_t clock_ms;
static bool busy,cs=true,dc;
static uint8_t command,output[4000];
static size_t written,writes;
static unsigned sleeping,refreshes,gpio_inits[30];
absolute_time_t get_absolute_time(void){return clock_ms;}
uint32_t to_ms_since_boot(absolute_time_t t){return (uint32_t)t;}
void gpio_init(uint p){assert(p<30);gpio_inits[p]++;}
void gpio_set_dir(uint p,int d){assert(p<30);assert((p==13 && d==GPIO_IN)||(p!=13 && d==GPIO_OUT));}
void gpio_set_function(uint p,int f){assert((p==10||p==11)&&f==GPIO_FUNC_SPI);}
void gpio_pull_up(uint p){assert(p==13);}
void gpio_put(uint p,int v){assert(p==8||p==9||p==12);if(p==9)cs=v;if(p==8)dc=v;}
bool gpio_get(uint p){assert(p==13);return busy;}
unsigned spi_init(spi_inst_t *s,unsigned baud){assert(s==spi1&&baud==4000000);return baud;}
void spi_set_format(spi_inst_t *s,int bits,int cp,int ph,int order){assert(s==spi1&&bits==8&&!cp&&!ph&&!order);}
int spi_write_blocking(spi_inst_t *s,const uint8_t *p,size_t n){
    assert(s==spi1&&!cs&&n==1);writes++;
    if(!dc){command=*p;if(command==0x24)written=0;if(command==0x20)refreshes++;}
    else if(command==0x24){assert(written<sizeof output);output[written++]=*p;}
    else if(command==0x10){assert(*p==1);sleeping++;}
    return (int)n;
}
static void poll_for(unsigned ms){
    for(unsigned n=0;n<ms/5;n++){size_t previous=writes;pm_display_poll();assert(writes-previous<140);clock_ms+=5;}
}
int main(void){
    assert(PM_IMAGE_BYTES==3813);
    static uint8_t pixels[PM_IMAGE_BYTES],expected[4000];
    memset(pixels,0xff,sizeof pixels);
    // Deliberately asymmetric corners plus diagonal bits cross byte/row boundaries.
    for(unsigned y=0;y<122;y++){
        unsigned x=(y*7)%250,b=y*250+x;
        pixels[b/8]&=(uint8_t)~(0x80u>>(b&7));
    }
    pm_eink_pack_landscape(pixels,expected);
    for(unsigned y=0;y<122;y++)for(unsigned x=0;x<250;x++){
        unsigned b=y*250+x,u=121-y;
        assert(!!(pixels[b/8]&(0x80u>>(b&7)))==!!(expected[(u/8)*250+x]&(0x80u>>(u&7))));
    }
    for(unsigned x=0;x<250;x++)assert((expected[15*250+x]&0x3f)==0x3f);
    pm_display_init();assert(!strcmp(pm_display_capability(),"eink"));
    assert(!gpio_inits[20]&&!gpio_inits[21]);
    assert(!pm_display_frame(pixels,3812));assert(pm_display_frame(pixels,3813));
    assert(pm_display_busy());bool ok;
    assert(!pm_display_take_result(&ok));
    poll_for(1000);assert(!pm_display_busy());assert(pm_display_take_result(&ok));
    if(PM_EINK_PANEL_CONNECTED){
        assert(ok&&written==4000&&refreshes==1&&sleeping==1);
        assert(!memcmp(expected,output,4000));
        pm_display_standby();size_t before=writes;
        pm_display_status("WIFI","CONNECTING");poll_for(1000);
        assert(writes==before&&!pm_display_frame(pixels,sizeof pixels));
        pm_display_wake();assert(pm_display_frame(pixels,sizeof pixels));
        busy=true;poll_for(PM_EINK_BUSY_TIMEOUT_MS+100);
        assert(!pm_display_busy()&&pm_display_take_result(&ok)&&!ok);
        assert(refreshes==1);busy=false;
        assert(pm_display_frame(pixels,sizeof pixels));poll_for(350);
        // Cancel a queued local repaint while a physical update drains.
        pm_display_status("NETWORK","HOME");pm_display_standby();
        poll_for(1000);assert(!pm_display_busy()&&pm_display_take_result(&ok)&&ok);
        assert(refreshes==2);before=writes;poll_for(1000);assert(writes==before);
        pm_display_wake();pm_display_status("NETWORK","HOME");poll_for(1000);
        assert(refreshes==3);
    }else assert(!ok&&writes==0);
    puts("eink: mono mapping/padding, byte-CS full refresh, bounded poll/BUSY timeout, retained standby, absent-panel PASS");
}
