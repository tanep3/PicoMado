// Link the real display/LCD path to GPIO/SPI stubs, using dummy device configs.
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "display.h"
#include "lcd.h"
#include "pico/stdlib.h"
#include "hardware/spi.h"

static struct { int level,output,mux;unsigned inits,writes; } pins[30];
static spi_inst_t bus;
spi_inst_t *spi0=&bus;
static uint8_t command;
static unsigned rate,frames,busy;
static unsigned on_level=!!PM_LCD_BLK_ACTIVE_HIGH;
static unsigned first_driven_level=99;

void gpio_init(uint p){
    assert(p<30);pins[p].output=0;pins[p].level=0;pins[p].mux=GPIO_FUNC_SIO;pins[p].inits++;
}
void gpio_set_dir(uint p,int out){
    assert(p<30 && out==GPIO_OUT);
    if(p==PM_LCD_BLK_PIN){
        assert(pins[p].mux==GPIO_FUNC_SIO);
        // The desired initial ON level must already be latched before drive.
        assert((unsigned)pins[p].level==on_level);
        first_driven_level=(unsigned)pins[p].level;
    }
    pins[p].output=out;
}
void gpio_set_function(uint p,int function){
    assert((p==PM_LCD_PIN_SCK || p==PM_LCD_PIN_MOSI) && function==GPIO_FUNC_SPI);
    pins[p].mux=function;
}
void gpio_put(uint p,int level){
    assert(p<30 && (level==0 || level==1));
    if(p==PM_LCD_BLK_PIN){
        assert(pins[p].mux==GPIO_FUNC_SIO);
        // In display standby, frame transfer completes before BLK goes OFF.
        if(pins[p].output && (unsigned)level!=on_level)
            assert(frames>0 && !busy && pins[PM_LCD_PIN_CS].level==1);
    }
    if(p==PM_LCD_PIN_CS && level)assert(!busy);
    pins[p].level=level;pins[p].writes++;
}
bool gpio_get(uint p){
    assert(p==PM_LCD_BLK_PIN && pins[p].output==GPIO_OUT && pins[p].mux==GPIO_FUNC_SIO);
    return pins[p].level;
}
void sleep_ms(unsigned n){assert(n>0);}
void tight_loop_contents(void){assert(busy);busy--;}
unsigned spi_init(spi_inst_t *p,unsigned baud){assert(p==spi0);rate=baud;return baud;}
unsigned spi_get_baudrate(const spi_inst_t *p){assert(p==spi0);return rate;}
unsigned spi_set_baudrate(spi_inst_t *p,unsigned baud){
    assert(p==spi0 && pins[PM_LCD_PIN_CS].level==1);rate=baud;return baud;
}
bool spi_is_busy(const spi_inst_t *p){assert(p==spi0);return busy>0;}
void spi_set_format(spi_inst_t *p,int bits,int cpol,int cpha,int order){
    assert(p==spi0 && bits==8 && !cpol && !cpha && !order);
}
int spi_write_blocking(spi_inst_t *p,const uint8_t *bytes,size_t n){
    assert(p==spi0 && !pins[PM_LCD_PIN_CS].level && rate==20000000);
    if(!pins[PM_LCD_PIN_DC].level){assert(n==1);command=bytes[0];}
    else if(command==0x2c){
        assert(n==25600);
        // RGB565 black becomes 0xffff with this LCD's invert/BGR compensation.
        for(size_t i=0;i<n;i++)assert(bytes[i]==(PM_LCD_COMPENSATE_INVERT_BGR?0xff:0));
        frames++;busy=2;
    }
    return (int)n;
}
static void check_level(bool on){
#if PM_LCD_BLK_CONNECTED
    assert(pins[PM_LCD_BLK_PIN].inits==1 && pins[PM_LCD_BLK_PIN].output==GPIO_OUT);
    assert(pins[PM_LCD_BLK_PIN].mux==GPIO_FUNC_SIO);
    assert(pins[PM_LCD_BLK_PIN].level==(on==!!PM_LCD_BLK_ACTIVE_HIGH));
#else
    (void)on;assert(!pins[PM_LCD_BLK_PIN].inits && !pins[PM_LCD_BLK_PIN].writes);
#endif
}
int main(void){
    for(unsigned i=0;i<30;i++)pins[i].mux=-1;
    const uint other_pins[]={PM_LCD_PIN_CS,PM_LCD_PIN_SCK,PM_LCD_PIN_MOSI,PM_LCD_PIN_DC,
        PM_LCD_PIN_RESET,PM_BUTTON_SUB_PIN,PM_BUTTON_UP_PIN,PM_BUTTON_POWER_PIN,PM_BUTTON_DOWN_PIN,
        23,24,25,29}; // Pico W CYW43 reserves these four GPIOs.
    for(size_t i=0;i<sizeof other_pins/sizeof other_pins[0];i++)assert(PM_LCD_BLK_PIN!=other_pins[i]);
    pm_display_init();check_level(true);
    pm_display_standby();assert(frames==1);check_level(false);
    pm_display_standby();assert(frames==2);check_level(false);
    pm_display_wake();assert(frames==2);check_level(true);
    for(unsigned i=0;i<20;i++){pm_display_standby();check_level(false);pm_display_wake();check_level(true);}
#if PM_LCD_BLK_CONNECTED
    assert(first_driven_level==on_level);
#endif
    printf("BLK connected=%u active_high=%u: startup=%u standby=%u wake=%u; init_count=%u; %s; black-before-OFF; PASS\n",
        PM_LCD_BLK_CONNECTED,PM_LCD_BLK_ACTIVE_HIGH,on_level,!on_level,on_level,pins[PM_LCD_BLK_PIN].inits,
        PM_LCD_BLK_CONNECTED?"SIO only":"no GPIO22 writes");
    return 0;
}
