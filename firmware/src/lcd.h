#pragma once
#include <stdint.h>
#include <stdbool.h>
#include "core.h"

void pm_lcd_init(void);
void pm_lcd_diagnostic(void);
void pm_lcd_reference_pattern(void);
void pm_lcd_draw(uint8_t *rgb565be);
void pm_lcd_local(const char *line1, const char *line2);
void pm_lcd_black(void);
void pm_lcd_backlight(bool on);

// LCD-only diagnostic control. Call between complete frames, never from IRQs.
void pm_lcd_configure_bus(uint32_t control_hz, uint32_t frame_hz);
uint32_t pm_lcd_last_frame_baud(void);
int pm_lcd_last_transfer_bytes(void);
uint32_t pm_lcd_fixed_test_frame(void);
#if defined(PM_LCD_CAPTURE_ENABLE) && PM_LCD_CAPTURE_ENABLE
// Diagnostic-only hook: transformed RAM pixels, before window/SPI writes.
void pm_lcd_capture_before_transfer(const uint8_t *pixels, size_t length);
#endif

