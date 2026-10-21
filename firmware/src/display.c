#include "display.h"

#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
#include "lcd.h"
#if PM_LCD_CONTROLLER != PM_LCD_CONTROLLER_ST7735S || \
    PM_PIXEL_FORMAT != PM_PIXEL_RGB565BE || PM_LCD_SPI_INDEX != 0 || \
    PM_BUTTON_COUNT != 4 || PM_DISPLAY_WIDTH != 160 || PM_DISPLAY_HEIGHT != 80 || \
    PM_PIXEL_BYTES != 2 || PM_LCD_PIN_SCK != 18 || PM_LCD_PIN_MOSI != 19
#error "Unsupported LCD hardware or server pixel contract"
#endif
const char *pm_display_capability(void) { return "lcd"; }
void pm_display_init(void) { pm_lcd_init(); }
void pm_display_status(const char *one, const char *two) { pm_lcd_local(one,two); }
void pm_display_feedback(pm_display_feedback_action action) {
    pm_lcd_switch_feedback(action==PM_DISPLAY_FEEDBACK_SHOW,
                           action==PM_DISPLAY_FEEDBACK_RESTORE);
}
bool pm_display_frame(uint8_t *pixels, size_t length) {
    if(!pixels || length!=PM_IMAGE_BYTES)return false;
    pm_lcd_draw(pixels);return true;
}
void pm_display_standby(void) { pm_lcd_black();pm_lcd_backlight(false); }
void pm_display_wake(void) { pm_lcd_backlight(true); }
void pm_display_reference_pattern(void) { pm_lcd_reference_pattern(); }
#elif PM_DISPLAY_TYPE == PM_DISPLAY_EINK
// E-ink symbols are supplied by the independently selected display_eink.c.
#else
#error "Unsupported display backend selection"
#endif
