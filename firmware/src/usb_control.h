#pragma once
#include "button_sim.h"
#include "observation.h"
#ifndef PM_USB_BUTTON_SIM
#define PM_USB_BUTTON_SIM 0
#endif
#ifndef PM_ACK_PATH_PROBE
#define PM_ACK_PATH_PROBE 0
#endif
// Ordinary terminal control is always present; diagnostics remain optional.
#include "input_types.h"
void pm_usb_control_init(uint64_t boot,unsigned buttons);
void pm_usb_control_set_operations(uint8_t operations,pm_input_handler handler);
void pm_usb_control_input(int ch);
void pm_usb_control_poll(bool connected);
pm_sim_dispatch pm_usb_control_tick(bool connected,unsigned physical,bool allowed,uint64_t session,unsigned state,bool standby,uint32_t long_ms,uint32_t now);
void pm_usb_control_dispatched(pm_sim_dispatch event,uint64_t session,unsigned new_state,uint32_t now);
const pm_button_sim *pm_usb_sim_status(void);
