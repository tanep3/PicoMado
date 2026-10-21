// V4 normal/base/partial refresh, following Waveshare EPD_2in13_V4.c and
// the local EPD_2in13_V4_Landscape reference. No V3 LUT or fast init is used.
// See ../clock-refresh-verification-2026-10-18.md for source/provenance.
#include "display.h"
#include "eink_transform.h"
#include "status_bitmaps.h"
#include "switch_bitmap.h"
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
               WAIT_SOFT_RESET, WRITE_RAM, WRITE_BASE, WAIT_PARTIAL_RESET, WAIT_REFRESH, WAIT_SLEEP } phase;
typedef enum { NONE, LOCAL, REMOTE, FEEDBACK, RESTORE } job;
static phase stage;
static job active_job,queued_job;
static uint8_t queued[PM_IMAGE_BYTES],ram[PM_EINK_RAM_BYTES];
static unsigned ram_offset;
static uint32_t until_ms,deadline_ms;
static bool stopped,result_ready,result_ok,status_valid;
static bool feedback_requested,feedback_visible;
static char status_one[32],status_two[32];
static uint64_t queued_frame_context,active_frame_context;
static bool queued_clock,active_clock,clock_base_valid,partial_update,awake;
static unsigned queued_minutes;
static uint64_t queued_minute,active_minute,last_full_minute;
static uint32_t clock_ms(void){return to_ms_since_boot(get_absolute_time());}
static bool reached(uint32_t now,uint32_t limit){return (int32_t)(now-limit)>=0;}
static void byte(bool data,uint8_t value){
    gpio_put(PM_EINK_PIN_DC,data);gpio_put(PM_EINK_PIN_CS,0);
    spi_write_blocking(spi1,&value,1);gpio_put(PM_EINK_PIN_CS,1);
}
static void cmd(uint8_t value){byte(false,value);}
static void data(uint8_t value){byte(true,value);}
static void cancel_feedback_preparation(void){
    // RAM writes and reset have no visible effect until command 0x20. A real
    // frame will start with its own full reset, so it can supersede these jobs.
    if((active_job==FEEDBACK || active_job==RESTORE) && stage!=WAIT_REFRESH){
        if(stage==WRITE_RAM || stage==WRITE_BASE)
            pm_trace_event(PM_TRACE_SPI_END,0,ram_offset,PM_EINK_SPI_BAUD);
        active_job=NONE;active_clock=false;stage=IDLE;
    }
    if(queued_job==RESTORE)queued_job=NONE;
}
void pm_display_feedback(pm_display_feedback_action action){
    if(action==PM_DISPLAY_FEEDBACK_SHOW){
        if(!stopped){
            // A newer wait still owns the visible hint. Do not first restore
            // the old screen merely because an earlier no-op requested it.
            if(queued_job==RESTORE || active_job==RESTORE)cancel_feedback_preparation();
            feedback_requested=true;
        }
        return;
    }
    feedback_requested=false;cancel_feedback_preparation();
    if(action==PM_DISPLAY_FEEDBACK_RESTORE && !stopped && feedback_visible &&
       queued_job==NONE && (active_job==NONE || active_job==FEEDBACK)){
        queued_job=RESTORE;queued_clock=false;queued_frame_context=0;
    }
}
static void pack_feedback(void){
    // Keep queued[] intact for a no-op response. Use the same landscape mapping
    // as remote mono1 images, without allocating another full image buffer.
    memset(ram,0xff,sizeof ram);
    for(unsigned y=0;y<PM_H;y++)for(unsigned x=0;x<PM_W;x++)
        if(pm_switch_bitmap_ink(x,y,PM_W,PM_H)){
            unsigned u=PM_H-1-y;
            ram[(u/8)*PM_W+x]&=(uint8_t)~(0x80u>>(u&7));
        }
}
static void finish(bool ok){
    if(active_job==REMOTE){result_ready=true;result_ok=ok;}
    if(active_job==FEEDBACK)feedback_requested=false;
    if(!ok){clock_base_valid=false;awake=false;}
    if(!ok)pm_logf("eink update failed stage=%u connected=%u\n",(unsigned)stage,PM_EINK_PANEL_CONNECTED);
    else if(active_clock){
        clock_base_valid=true;awake=true;
        if(!partial_update)last_full_minute=active_minute;
    }else {clock_base_valid=false;awake=false;cmd(0x10);data(0x01);until_ms=clock_ms()+100;}
    active_job=NONE;stage=ok&&!active_clock?WAIT_SLEEP:IDLE;
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
    pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
    memcpy(queued,pixels,length);queued_job=REMOTE;status_valid=false;queued_clock=false;
    queued_frame_context=pm_trace_current_frame();return true;
}
bool pm_display_clock_frame(uint8_t *pixels,size_t length,unsigned minutes,uint64_t minute,bool reset_base){
    if(minutes<1 || minutes>60 || !pm_display_frame(pixels,length))return false;
    queued_clock=true;queued_minutes=minutes;queued_minute=minute;
    if(reset_base)clock_base_valid=false;
    return true;
}
bool pm_display_busy(void){return stage!=IDLE || queued_job!=NONE || (!stopped && feedback_requested && !feedback_visible);}
bool pm_display_take_result(bool *ok){
    if(!result_ready)return false;
    *ok=result_ok;result_ready=false;return true;
}
void pm_display_poll(void){
    uint32_t now=clock_ms();
    if(stage==IDLE){
        if(stopped)return;
        if(queued_job==NONE && feedback_requested && !feedback_visible && !result_ready){
            active_job=FEEDBACK;active_clock=false;active_frame_context=0;
        }else {
            if(queued_job==NONE)return;
            active_job=queued_job;queued_job=NONE;active_clock=queued_clock;queued_clock=false;
            active_frame_context=queued_frame_context;queued_frame_context=0;
        }
        active_minute=queued_minute;
        partial_update=active_clock && clock_base_valid && awake &&
            active_minute>=last_full_minute && active_minute-last_full_minute<queued_minutes;
        if(!active_clock)clock_base_valid=false;
        if(!PM_EINK_PANEL_CONNECTED){finish(false);return;}
        if(active_job==FEEDBACK)pack_feedback();else pm_eink_pack_landscape(queued,ram);
        deadline_ms=now+PM_EINK_BUSY_TIMEOUT_MS;
        if(partial_update){
            // Official V4 partial reset (no SWRESET, no deep sleep/base loss).
            gpio_put(PM_EINK_PIN_RESET,0);until_ms=now+1;stage=WAIT_PARTIAL_RESET;
        }else {gpio_put(PM_EINK_PIN_RESET,1);until_ms=now+20;stage=RESET_HIGH;}
        return;
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
    case WAIT_PARTIAL_RESET:
        if(reached(now,until_ms)){
            gpio_put(PM_EINK_PIN_RESET,1);
            cmd(0x3c);data(0x80);cmd(0x01);data(0xf9);data(0);data(0);
            cmd(0x11);data(0x07);cmd(0x44);data(0);data(15);
            cmd(0x45);data(0);data(0);data(249);data(0);
            cmd(0x4e);data(0);cmd(0x4f);data(0);data(0);
            cmd(0x24);ram_offset=0;stage=WRITE_RAM;
            pm_trace_event(PM_TRACE_SPI_BEGIN,active_frame_context,0,0);
        }break;
    case WAIT_SOFT_RESET:
        if(reached(now,until_ms) && !gpio_get(PM_EINK_PIN_BUSY)){
            cmd(0x01);data(0xf9);data(0);data(0);
            cmd(0x11);data(0x07);cmd(0x44);data(0);data(15);
            cmd(0x45);data(0);data(0);data(249);data(0);
            cmd(0x4e);data(0);cmd(0x4f);data(0);data(0);
            cmd(0x3c);data(0x05);cmd(0x21);data(0);data(0x80);
            cmd(0x18);data(0x80);cmd(0x24);ram_offset=0;stage=WRITE_RAM;
            pm_trace_event(PM_TRACE_SPI_BEGIN,active_frame_context,0,0);
        }break;
    case WRITE_RAM:case WRITE_BASE:
        for(unsigned n=0;n<128 && ram_offset<sizeof ram;n++)data(ram[ram_offset++]);
        if(ram_offset==sizeof ram){
            if(stage==WRITE_RAM && active_clock && !partial_update){
                // Display_Base writes the same image to old/new RAM before f7.
                cmd(0x26);ram_offset=0;stage=WRITE_BASE;break;
            }
            pm_trace_event(PM_TRACE_SPI_END,active_frame_context,active_clock&&!partial_update?2*sizeof ram:sizeof ram,PM_EINK_SPI_BAUD);
            cmd(0x22);data(partial_update?0xff:0xf7);cmd(0x20);
            feedback_visible=active_job==FEEDBACK;
            until_ms=now+10;stage=WAIT_REFRESH;
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
    pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
    if(status_valid && !feedback_visible && !strcmp(status_one,one) && !strcmp(status_two,two))return;
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
    queued_job=LOCAL;queued_clock=false;clock_base_valid=false;
    queued_frame_context=0;
}
void pm_display_standby(void){
    // App defers this until physical refresh finishes. Never clear retained pixels.
    pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
    if(queued_job==REMOTE){result_ready=true;result_ok=false;}
    queued_job=NONE;stopped=true;status_valid=false;queued_clock=false;clock_base_valid=false;
    queued_frame_context=0;
    if(awake && stage==IDLE){cmd(0x10);data(0x01);until_ms=clock_ms()+100;stage=WAIT_SLEEP;}
    awake=false;
}
void pm_display_wake(void){stopped=false;clock_base_valid=false;}
void pm_display_reference_pattern(void){
    if(stopped)return;
    pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
    memset(queued,0xff,sizeof queued);
    for(unsigned y=0;y<PM_H;y++)for(unsigned x=0;x<PM_W;x++)
        if(x==0 || y==0 || x==PM_W-1 || y==PM_H-1 || (x<18 && y<10))dot(x,y);
    label("PICO MADO",35);queued_job=LOCAL;status_valid=false;queued_clock=false;clock_base_valid=false;
    queued_frame_context=0;
}
