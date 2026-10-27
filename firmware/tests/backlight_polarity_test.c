#include <assert.h>
#include <setjmp.h>
#include <stdio.h>
#include <string.h>
#define main polarity_firmware_main
#include "../diagnostics/backlight-polarity/main.c"
#undef main

static jmp_buf done;
static uint32_t clock_ms=1000;
static bool level=true;
static unsigned draws,writes,inits;
static unsigned chip_inits,sta_enables;
static int pressed=-1;
absolute_time_t get_absolute_time(void){return clock_ms;}
uint32_t to_ms_since_boot(absolute_time_t t){return (uint32_t)t;}
void stdio_init_all(void){}
bool stdio_usb_connected(void){return true;}
int cyw43_arch_init(void){chip_inits++;return 0;}
void cyw43_arch_enable_sta_mode(void){sta_enables++;}
void cyw43_arch_poll(void){}
void pico_get_unique_board_id_string(char *id,size_t n){(void)id;(void)n;assert(!"diagnostic must not require a physical board ID");}
void pm_lcd_init(void){inits++;}
uint32_t pm_lcd_fixed_test_frame(void){assert(!draws);draws++;return 0x20ac31e9;}
uint32_t pm_lcd_last_frame_baud(void){return 15625000;}
int pm_lcd_last_transfer_bytes(void){return 25600;}
void gpio_init(uint p){assert(p>=12 && p<=15);}
void gpio_set_dir(uint p,int direction){assert(p>=12 && p<=15 && direction==GPIO_IN);}
void gpio_pull_up(uint p){assert(p>=12 && p<=15);}
void gpio_put(uint p,int value){assert(p==22 && (value==0 || value==1));level=value;writes++;}
bool gpio_get(uint p){
    if(p==22)return level;
    assert(p>=12 && p<=15);return pressed<0 || p!=button_pins[pressed];
}
bool gpio_get_dir(uint p){assert(p==22);return true;}
int gpio_get_function(uint p){assert(p==22);return GPIO_FUNC_SIO;}
enum gpio_drive_strength gpio_get_drive_strength(uint p){assert(p==22);return GPIO_DRIVE_STRENGTH_4MA;}
bool gpio_is_pulled_up(uint p){assert(p==22);return false;}
bool gpio_is_pulled_down(uint p){assert(p==22);return true;}
void sleep_ms(unsigned n){
    assert(n==5 && draws==1 && inits==1 && chip_inits==1 && sta_enables==1);
    if(clock_ms>=1340 && clock_ms<1350)assert(!level);
    if(clock_ms>=1490)assert(level);
    clock_ms+=5;
    pressed=clock_ms>=1050 && clock_ms<1100?1:
        clock_ms>=1200 && clock_ms<1250?3:
        clock_ms>=1350 && clock_ms<1400?2:
        clock_ms>=1500 && clock_ms<1550?0:-1;
    if(clock_ms>=1600){assert(writes==4 && draws==1 && level);longjmp(done,1);}
}
int main(void){
    if(!setjmp(done))polarity_firmware_main();
    puts("polarity diagnostic PASS: UP/DOWN/POWER raw levels; SUB status only; fixed LCD draws=1; comparison output writes only GP22");
    return 0;
}
