#pragma once
#include "button_sim.h"
#include "observation.h"
#ifndef PM_USB_BUTTON_SIM
#define PM_USB_BUTTON_SIM 0
#endif
#ifndef PM_ACK_PATH_PROBE
#define PM_ACK_PATH_PROBE 0
#endif
#if PM_USB_BUTTON_SIM || PM_ACK_PATH_PROBE || PM_EVENT_STREAM
void pm_usb_control_init(uint64_t boot,unsigned buttons);
void pm_usb_control_input(int ch);
void pm_usb_control_poll(bool connected);
pm_sim_dispatch pm_usb_control_tick(bool connected,unsigned physical,bool allowed,uint64_t session,unsigned state,bool standby,uint32_t long_ms,uint32_t now);
void pm_usb_control_dispatched(pm_sim_dispatch event,uint64_t session,unsigned new_state,uint32_t now);
const pm_button_sim *pm_usb_sim_status(void);
#else
static inline void pm_usb_control_init(uint64_t b,unsigned n){(void)b;(void)n;}
static inline void pm_usb_control_input(int c){(void)c;}
static inline void pm_usb_control_poll(bool c){(void)c;}
static inline pm_sim_dispatch pm_usb_control_tick(bool c,unsigned p,bool a,uint64_t s,unsigned t,bool b,uint32_t l,uint32_t n){(void)c;(void)p;(void)a;(void)s;(void)t;(void)b;(void)l;(void)n;return (pm_sim_dispatch){0};}
static inline void pm_usb_control_dispatched(pm_sim_dispatch e,uint64_t s,unsigned t,uint32_t n){(void)e;(void)s;(void)t;(void)n;}
#endif
