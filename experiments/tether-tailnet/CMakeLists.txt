cmake_minimum_required(VERSION 3.13)

if(NOT DEFINED PICO_SDK_PATH)
    set(PICO_SDK_PATH "/home/tane/dev/PicoW/pico-sdk")
endif()
include(${PICO_SDK_PATH}/external/pico_sdk_import.cmake)

project(tether_tailnet_probe C CXX ASM)
set(CMAKE_C_STANDARD 11)
pico_sdk_init()

if(NOT EXISTS "${CMAKE_CURRENT_LIST_DIR}/test_config.h")
    message(FATAL_ERROR "Copy test_config.h.sample to test_config.h and fill it locally before building")
endif()

add_executable(tether_tailnet_probe pico_probe.c)
target_include_directories(tether_tailnet_probe PRIVATE ${CMAKE_CURRENT_LIST_DIR})
target_link_libraries(tether_tailnet_probe pico_stdlib pico_cyw43_arch_lwip_poll)
pico_enable_stdio_usb(tether_tailnet_probe 1)
pico_enable_stdio_uart(tether_tailnet_probe 0)
pico_add_extra_outputs(tether_tailnet_probe)
