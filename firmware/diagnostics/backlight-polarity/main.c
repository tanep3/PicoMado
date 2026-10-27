#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "pico/stdlib.h"
#include "pico/stdio_usb.h"
#include "pico/cyw43_arch.h"
#include "core.h"
#include "lcd.h"

_Static_assert(PM_LCD_BLK_CONNECTED && PM_LCD_BLK_PIN==22,"Requires wired GP22 BLK");
_Static_assert(PM_DISPLAY_TYPE==PM_DISPLAY_LCD && PM_W==160 && PM_H==80,"LCD1 only");
_Static_assert(PM_BUTTON_COUNT==4,"Requires LCD1 four buttons");
static pm_button buttons[PM_BUTTON_COUNT];
static const uint button_pins[]={PM_BUTTON_SUB_PIN,PM_BUTTON_UP_PIN,PM_BUTTON_POWER_PIN,PM_BUTTON_DOWN_PIN};
static bool requested_high=true;
static uint32_t image_crc;
static uint32_t now_ms(void){return to_ms_since_boot(get_absolute_time());}
static void report(const char *reason){
    printf("blk-polarity: reason=%s requested=%s gpio_read=%u dir=%u mux=%u drive_code=%u pullup=%u pulldown=%u fixed_crc=%08lx draws=1\n",
        reason,requested_high?"HIGH":"LOW",gpio_get(22)?1u:0u,gpio_get_dir(22)?1u:0u,
        (unsigned)gpio_get_function(22),(unsigned)gpio_get_drive_strength(22),
        gpio_is_pulled_up(22)?1u:0u,gpio_is_pulled_down(22)?1u:0u,(unsigned long)image_crc);
}
static void select_level(bool high,const char *reason){
    requested_high=high;gpio_put(22,high);report(reason);
}
int main(void){
    stdio_init_all();
    // Keep the usual CYW43-before-LCD power/init order fixed for both levels.
    // No SSID join, proxy/server connection, or standby transition is used.
    if(cyw43_arch_init()){puts("blk-polarity: chip init failed; BLK untouched");while(true)sleep_ms(1000);}
    cyw43_arch_enable_sta_mode();
    pm_lcd_init();image_crc=pm_lcd_fixed_test_frame(); // The only LCD draw in this test.
    printf("blk-polarity: SPI requested=%lu actual=%lu bytes=%d; UP=HIGH DOWN=LOW POWER=toggle SUB=status\n",
        (unsigned long)PM_LCD_SPI_BAUD,(unsigned long)pm_lcd_last_frame_baud(),pm_lcd_last_transfer_bytes());
    select_level(true,"boot");
    for(unsigned i=0;i<PM_BUTTON_COUNT;i++){
        gpio_init(button_pins[i]);gpio_set_dir(button_pins[i],GPIO_IN);gpio_pull_up(button_pins[i]);
    }
    bool was_usb=false;uint32_t last_report=now_ms();
    while(true){
        cyw43_arch_poll();
        uint32_t now=now_ms();
        for(unsigned i=0;i<PM_BUTTON_COUNT;i++)if(pm_button_step(&buttons[i],!gpio_get(button_pins[i]),now)){
            if(i==1)select_level(true,"UP");else if(i==3)select_level(false,"DOWN");
            else if(i==2)select_level(!requested_high,"POWER");else report("SUB");
        }
        bool usb=stdio_usb_connected();
        if(usb && (!was_usb || now-last_report>=5000)){report("snapshot");last_report=now;}
        was_usb=usb;sleep_ms(5);
    }
}
