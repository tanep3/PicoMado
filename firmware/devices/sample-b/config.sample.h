#pragma once
#define PM_DEVICE_ID "sample-b"
#define PM_HOME_SSID "EXAMPLE_B_HOME"
#define PM_HOME_PASSWORD "EXAMPLE_PASSWORD"
#define PM_HOME_SERVER_IP "192.0.2.2"
#define PM_AWAY_SSID "EXAMPLE_B_AWAY"
#define PM_AWAY_PASSWORD "EXAMPLE_PASSWORD"
#define PM_AWAY_SERVER_IP "198.51.100.3"
#define PM_AWAY_PROXY_IP "198.51.100.4"
#define PM_AWAY_PROXY_PORT 1080
#define PM_AWAY_PROXY_USER "EXAMPLE_USER"
#define PM_AWAY_PROXY_PASSWORD "EXAMPLE_PASSWORD"

// Device hardware. Only this 160x80 RGB565 ST7735S LCD is implemented.
#include "pico_w_lcd_160x80_4.h"

// Individual wiring: leave 0 unless BLK is connected.
#define PM_LCD_BLK_CONNECTED 0
#define PM_LCD_BLK_ACTIVE_HIGH 1
#define PM_DIAGNOSTIC_BOOT 0

#define PM_DIAGNOSTIC_STAGE 0
