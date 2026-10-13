#include <stdio.h>
#include <string.h>
#include "pico/stdlib.h"
#include "pico/unique_id.h"
#include "pico/cyw43_arch.h"
#include "hardware/clocks.h"
#include "hardware/spi.h"
#include "lcd.h"
#if defined(PM_LCD_CAPTURE_ENABLE) && PM_LCD_CAPTURE_ENABLE
#include "capture.h"
#endif

#if PM_DISPLAY_TYPE != PM_DISPLAY_LCD
#error "This reproduction is LCD-only; E-ink uses its own backend"
#endif
_Static_assert(PM_W==160 && PM_H==80 && PM_PIXEL_BYTES==2 && PM_BUTTON_COUNT==4,
               "Reproduction requires the verified LCD dimensions/buttons");
_Static_assert(PM_LCD_SPI_INDEX==0 && PM_LCD_PIN_CS==17 && PM_LCD_PIN_SCK==18 &&
               PM_LCD_PIN_MOSI==19 && PM_LCD_PIN_DC==20 && PM_LCD_PIN_RESET==21,
               "Reproduction requires the verified SPI0 wiring");
static const uint pins[]={PM_BUTTON_SUB_PIN,PM_BUTTON_UP_PIN,
                          PM_BUTTON_POWER_PIN,PM_BUTTON_DOWN_PIN};
static const uint32_t rates[]={1000000,5000000,20000000};
static unsigned rate_index=PM_REPRO_SPI_BAUD==1000000?0:
                           (PM_REPRO_SPI_BAUD==5000000?1:2);
static pm_button buttons[PM_BUTTON_COUNT];
static const char *phase="wifi_before";
static uint32_t serial;

static void draw(void) {
#if defined(PM_LCD_CAPTURE_ENABLE) && PM_LCD_CAPTURE_ENABLE
    pm_capture_set_context(serial+1,phase,rates[rate_index]);
#endif
    uint64_t before=time_us_64();
    uint32_t crc=pm_lcd_fixed_test_frame();
    uint64_t elapsed=time_us_64()-before;
    const spi_hw_t *hw=spi_get_hw(spi0);
    // The transfer result is software evidence, not LCD readback.
    printf("lcd_repro n=%lu phase=%s request=%lu actual=%lu peri=%lu "
           "cr0=%08lx cr1=%08lx cpsr=%lu returned=%d crc=%08lx us=%llu link=%d\n",
           (unsigned long)++serial,phase,(unsigned long)rates[rate_index],
           (unsigned long)pm_lcd_last_frame_baud(),
           (unsigned long)clock_get_hz(clk_peri),
           (unsigned long)hw->cr0,(unsigned long)hw->cr1,
           (unsigned long)hw->cpsr,pm_lcd_last_transfer_bytes(),
           (unsigned long)crc,(unsigned long long)elapsed,
           cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA));
#if defined(PM_LCD_CAPTURE_ENABLE) && PM_LCD_CAPTURE_ENABLE
    if(pm_capture_ready() && (serial==7 || serial%20==0))
        puts("LCDCAP READY count=3 frames=1,2,7 source=transformed_ram_before_spi");
#endif
}
static void set_rate(void) {
    // One factor: commands and pixels use the same chosen request.
    pm_lcd_configure_bus(rates[rate_index],rates[rate_index]);
    pm_lcd_init();
}
int main(void) {
    stdio_init_all();
    char board[2*PICO_UNIQUE_BOARD_ID_SIZE_BYTES+1];
    pico_get_unique_board_id_string(board,sizeof board);
    if(PM_REPRO_SAMPLE || strcmp(board,PM_REPRO_BOARD_ID)) {
        for(;;) { puts("lcd_repro: sample or different board; hardware disabled");sleep_ms(1000); }
    }
    if(cyw43_arch_init()) {
        for(;;) { puts("lcd_repro: CYW43 init failed");sleep_ms(1000); }
    }
    cyw43_arch_enable_sta_mode();
    cyw43_arch_lwip_begin();
    int pm=cyw43_wifi_pm(&cyw43_state,CYW43_NONE_PM);
    cyw43_arch_lwip_end();
    printf("lcd_repro: CYW43 ready pm_off=%d; fixed complete frame\n",pm==0);
    set_rate();
    for(unsigned i=0;i<PM_BUTTON_COUNT;i++) {
        gpio_init(pins[i]);gpio_set_dir(pins[i],GPIO_IN);gpio_pull_up(pins[i]);
    }
    for(unsigned i=0;i<6;i++) { draw();sleep_ms(500); }
#if PM_REPRO_WIFI_HOME
    puts("lcd_repro: joining existing home config; credentials omitted");
    int joined=cyw43_arch_wifi_connect_timeout_ms(PM_HOME_SSID,PM_HOME_PASSWORD,
                       CYW43_AUTH_WPA2_AES_PSK,15000);
    phase=joined==0?"wifi_after":"wifi_failed";
    printf("lcd_repro: join_result=%d\n",joined);
#else
    phase="wifi_join_disabled";
#endif
    uint32_t next=to_ms_since_boot(get_absolute_time());
    for(;;) {
        uint32_t now=to_ms_since_boot(get_absolute_time());
        for(unsigned i=0;i<PM_BUTTON_COUNT;i++) {
            if(!pm_button_step(&buttons[i],!gpio_get(pins[i]),now))continue;
            printf("lcd_repro button=%u\n",i);
            if(i==1 && rate_index<2) { rate_index++;set_rate(); }
            if(i==3 && rate_index>0) { rate_index--;set_rate(); }
            if(i==2) set_rate();
            draw();
        }
        if((int32_t)(now-next)>=0) { draw();next=now+500; }
#if defined(PM_LCD_CAPTURE_ENABLE) && PM_LCD_CAPTURE_ENABLE
        // Export retained snapshots after the observed frames. During export,
        // redraw pauses; it must not be mistaken for the unchanged 500ms loop.
        if(getchar_timeout_us(0)=='D' && pm_capture_ready())pm_capture_dump();
#endif
        sleep_ms(5);
    }
}
