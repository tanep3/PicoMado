// Full-refresh sequence and byte-wise CS follow the proven legacy V4 panel.
// This backend does not use the V3 LUT/partial-refresh sequence.
#include "display.h"
#include "eink_transform.h"
#include "status_bitmaps.h"
#include "pico/stdlib.h"
#include "hardware/spi.h"
#include "trace.h"
#include <string.h>

#if PM_DISPLAY_TYPE != PM_DISPLAY_EINK || PM_PIXEL_FORMAT != PM_PIXEL_MONO1 || \
    PM_DISPLAY_WIDTH != 250 || PM_DISPLAY_HEIGHT != 122 || \
    PM_EINK_PANEL_VERSION != 4 || PM_EINK_SPI_INDEX != 1 || \
    PM_EINK_PIN_SCK != 10 || PM_EINK_PIN_MOSI != 11
#error "Unsupported E-ink hardware or mono1 contract (only legacy V4 verified)"
#endif

typedef enum { IDLE, RESET_HIGH, RESET_LOW, RESET_SETTLE, WAIT_RESET,
               WAIT_SOFT_RESET, WRITE_RAM, WAIT_REFRESH, WAIT_SLEEP } phase;
typedef enum { NONE, LOCAL, REMOTE } job;
static phase stage;
static job active_job,queued_job;
static uint8_t queued[PM_IMAGE_BYTES],ram[PM_EINK_RAM_BYTES];
static unsigned ram_offset;
static uint32_t until_ms,deadline_ms;
static bool stopped,result_ready,result_ok,status_valid;
static char status_one[32],status_two[32];
static uint64_t frame_context;
static uint32_t clock_ms(void){return to_ms_since_boot(get_absolute_time());}
static bool reached(uint32_t now,uint32_t limit){return (int32_t)(now-limit)>=0;}
static void byte(bool data,uint8_t value){
    gpio_put(PM_EINK_PIN_DC,data);gpio_put(PM_EINK_PIN_CS,0);
    spi_write_blocking(spi1,&value,1);gpio_put(PM_EINK_PIN_CS,1);
}
static void cmd(uint8_t value){byte(false,value);}
static void data(uint8_t value){byte(true,value);}
static void finish(bool ok){
    if(active_job==REMOTE){result_ready=true;result_ok=ok;}
    if(!ok)pm_logf("eink update failed stage=%u connected=%u\n",(unsigned)stage,PM_EINK_PANEL_CONNECTED);
    else {cmd(0x10);data(0x01);until_ms=clock_ms()+100;}
    active_job=NONE;stage=ok?WAIT_SLEEP:IDLE;
}
const char *pm_display_capability(void){return "eink";}
void pm_display_init(void){
    spi_init(spi1,PM_EINK_SPI_BAUD);
    spi_set_format(spi1,8,SPI_CPOL_0,SPI_CPHA_0,SPI_MSB_FIRST);
    gpio_set_function(PM_EINK_PIN_SCK,GPIO_FUNC_SPI);
    gpio_set_function(PM_EINK_PIN_MOSI,GPIO_FUNC_SPI);
    const unsigned pins[]={PM_EINK_PIN_CS,PM_EINK_PIN_DC,PM_EINK_PIN_RESET};
    for(unsigned i=0;i<3;i++){gpio_init(pins[i]);gpio_set_dir(pins[i],GPIO_OUT);}
    gpio_put(PM_EINK_PIN_CS,1);gpio_put(PM_EINK_PIN_RESET,1);
    gpio_init(PM_EINK_PIN_BUSY);gpio_set_dir(PM_EINK_PIN_BUSY,GPIO_IN);gpio_pull_up(PM_EINK_PIN_BUSY);
    pm_logf("eink ready version=%u connected=%u buttons=%u\n",PM_EINK_PANEL_VERSION,PM_EINK_PANEL_CONNECTED,PM_BUTTON_COUNT);
}
bool pm_display_frame(uint8_t *pixels,size_t length){
    if(stopped || !pixels || length!=PM_IMAGE_BYTES || active_job==REMOTE || queued_job==REMOTE || result_ready)return false;
    memcpy(queued,pixels,length);queued_job=REMOTE;status_valid=false;
    frame_context=pm_trace_current_frame();return true;
}
bool pm_display_busy(void){return stage!=IDLE || queued_job!=NONE;}
bool pm_display_take_result(bool *ok){
    if(!result_ready)return false;
    *ok=result_ok;result_ready=false;return true;
}
void pm_display_poll(void){
    uint32_t now=clock_ms();
    if(stage==IDLE){
        if(queued_job==NONE || stopped)return;
        active_job=queued_job;queued_job=NONE;
        if(!PM_EINK_PANEL_CONNECTED){finish(false);return;}
        pm_eink_pack_landscape(queued,ram);
        deadline_ms=now+PM_EINK_BUSY_TIMEOUT_MS;
        gpio_put(PM_EINK_PIN_RESET,1);until_ms=now+20;stage=RESET_HIGH;return;
    }
    if(stage==WAIT_SLEEP){if(reached(now,until_ms))stage=IDLE;return;}
    if(reached(now,deadline_ms)){finish(false);return;}
    switch(stage){
    case RESET_HIGH:
        if(reached(now,until_ms)){gpio_put(PM_EINK_PIN_RESET,0);until_ms=now+2;stage=RESET_LOW;}break;
    case RESET_LOW:
        if(reached(now,until_ms)){gpio_put(PM_EINK_PIN_RESET,1);until_ms=now+120;stage=RESET_SETTLE;}break;
    case RESET_SETTLE:if(reached(now,until_ms))stage=WAIT_RESET;break;
    case WAIT_RESET:
        if(!gpio_get(PM_EINK_PIN_BUSY)){cmd(0x12);until_ms=now+10;stage=WAIT_SOFT_RESET;}break;
    case WAIT_SOFT_RESET:
        if(reached(now,until_ms) && !gpio_get(PM_EINK_PIN_BUSY)){
            cmd(0x01);data(0xf9);data(0);data(0);
            cmd(0x11);data(0x07);cmd(0x44);data(0);data(15);
            cmd(0x45);data(0);data(0);data(249);data(0);
            cmd(0x4e);data(0);cmd(0x4f);data(0);data(0);
            cmd(0x3c);data(0x05);cmd(0x21);data(0);data(0x80);
            cmd(0x18);data(0x80);cmd(0x24);ram_offset=0;stage=WRITE_RAM;
            pm_trace_event(PM_TRACE_SPI_BEGIN,frame_context,0,0);
        }break;
    case WRITE_RAM:
        for(unsigned n=0;n<128 && ram_offset<sizeof ram;n++)data(ram[ram_offset++]);
        if(ram_offset==sizeof ram){
            pm_trace_event(PM_TRACE_SPI_END,frame_context,sizeof ram,PM_EINK_SPI_BAUD);
            cmd(0x22);data(0xf7);cmd(0x20);until_ms=now+10;stage=WAIT_REFRESH;
        }break;
    case WAIT_REFRESH:
        if(reached(now,until_ms) && !gpio_get(PM_EINK_PIN_BUSY))finish(true);
        break;
    case IDLE:case WAIT_SLEEP:break;
    }
}
static void dot(unsigned x,unsigned y){
    if(x>=PM_W || y>=PM_H)return;
    unsigned b=y*PM_W+x;queued[b/8]&=(uint8_t)~(0x80u>>(b&7));
}
// The same compact local A-Z glyphs as the LCD status screen.
static const uint8_t glyphs[][5]={
    {0x7e,0x11,0x11,0x11,0x7e},{0x7f,0x49,0x49,0x49,0x36},{0x3e,0x41,0x41,0x41,0x22},
    {0x7f,0x41,0x41,0x22,0x1c},{0x7f,0x49,0x49,0x49,0x41},{0x7f,0x09,0x09,0x09,0x01},
    {0x3e,0x41,0x49,0x49,0x7a},{0x7f,0x08,0x08,0x08,0x7f},{0x41,0x41,0x7f,0x41,0x41},
    {0x20,0x40,0x41,0x3f,0x01},{0x7f,0x08,0x14,0x22,0x41},{0x7f,0x40,0x40,0x40,0x40},
    {0x7f,0x02,0x0c,0x02,0x7f},{0x7f,0x04,0x08,0x10,0x7f},{0x3e,0x41,0x41,0x41,0x3e},
    {0x7f,0x09,0x09,0x09,0x06},{0x3e,0x41,0x51,0x21,0x5e},{0x7f,0x09,0x19,0x29,0x46},
    {0x46,0x49,0x49,0x49,0x31},{0x01,0x01,0x7f,0x01,0x01},{0x3f,0x40,0x40,0x40,0x3f},
    {0x1f,0x20,0x40,0x20,0x1f},{0x7f,0x20,0x18,0x20,0x7f},{0x63,0x14,0x08,0x14,0x63},
    {0x03,0x04,0x78,0x04,0x03},{0x61,0x51,0x49,0x45,0x43}
};
static void label(const char *text,unsigned y){
    size_t n=strlen(text);if(n>20)n=20;
    unsigned x=(PM_W-(unsigned)n*12)/2;
    for(size_t i=0;i<n;i++,x+=12)if(text[i]>='A' && text[i]<='Z')
        for(unsigned col=0;col<5;col++)for(unsigned row=0;row<7;row++)
            if(glyphs[text[i]-'A'][col]&(1u<<row))
                for(unsigned a=0;a<2;a++)for(unsigned b=0;b<2;b++)dot(x+2*col+a,y+2*row+b);
}
void pm_display_status(const char *one,const char *two){
    if(stopped)return;
    if(status_valid && !strcmp(status_one,one) && !strcmp(status_two,two))return;
    if(queued_job==REMOTE){result_ready=true;result_ok=false;}
    snprintf(status_one,sizeof status_one,"%s",one);snprintf(status_two,sizeof status_two,"%s",two);
    status_valid=true;memset(queued,0xff,sizeof queued);
    const uint8_t *bitmap=NULL;
    if(!strcmp(one,"WIFI") && (!strcmp(two,"CONNECTING") || !strcmp(two,"RETRY")))bitmap=pm_status_wifi;
    else if(!strcmp(one,"SERVER") && !strcmp(two,"CONNECTING"))bitmap=pm_status_server;
    if(bitmap){
        for(unsigned y=0;y<PM_STATUS_H;y++)for(unsigned x=0;x<PM_STATUS_W;x++)
            if(bitmap[y*(PM_STATUS_W/8)+x/8]&(0x80u>>(x&7)))dot(x+(PM_W-PM_STATUS_W)/2,y+48);
    }else {label(one,35);label(two,73);}
    queued_job=LOCAL;
}
void pm_display_standby(void){
    // App defers this until physical refresh finishes. Never clear retained pixels.
    if(queued_job==REMOTE){result_ready=true;result_ok=false;}
    queued_job=NONE;stopped=true;status_valid=false;
}
void pm_display_wake(void){stopped=false;}
void pm_display_reference_pattern(void){
    if(stopped)return;
    memset(queued,0xff,sizeof queued);
    for(unsigned y=0;y<PM_H;y++)for(unsigned x=0;x<PM_W;x++)
        if(x==0 || y==0 || x==PM_W-1 || y==PM_H-1 || (x<18 && y<10))dot(x,y);
    label("PICO MADO",35);queued_job=LOCAL;status_valid=false;
}
