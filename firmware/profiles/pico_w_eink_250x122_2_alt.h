#pragma once
// Pico W hardware layout. Physical support status: docs/guides/hardware-support.md.
#define PM_DISPLAY_LCD 1
#define PM_DISPLAY_EINK 2
#define PM_DISPLAY_TYPE PM_DISPLAY_EINK
#define PM_DISPLAY_WIDTH 250
#define PM_DISPLAY_HEIGHT 122
#define PM_PIXEL_RGB565BE 1
#define PM_PIXEL_MONO1 2
#define PM_PIXEL_FORMAT PM_PIXEL_MONO1
#define PM_PIXEL_FORMAT_NAME "mono1"
#define PM_DISPLAY_CAPABILITY "eink"
#define PM_EINK_PANEL_VERSION 4
#define PM_EINK_SPI_INDEX 1
#define PM_EINK_SPI_BAUD 4000000
#define PM_EINK_PIN_CS 9
#define PM_EINK_PIN_SCK 10
#define PM_EINK_PIN_MOSI 11
#define PM_EINK_PIN_DC 8
#define PM_EINK_PIN_RESET 12
#define PM_EINK_PIN_BUSY 13
#define PM_EINK_BUSY_TIMEOUT_MS 10000
#define PM_BUTTON_UP_PIN 18
#define PM_BUTTON_DOWN_PIN 19

#define PM_BUTTON_COUNT 2
