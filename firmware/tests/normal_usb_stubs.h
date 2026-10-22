// Legacy main-loop fixtures can omit the USB adapter. New normal-USB tests
// define PM_TEST_REAL_USB_CONTROL and link the production adapter instead.
#if !PM_USB_BUTTON_SIM && !PM_ACK_PATH_PROBE && !PM_EVENT_STREAM && !defined(PM_TEST_REAL_USB_CONTROL)
void pm_usb_control_init(uint64_t b,unsigned n){(void)b;(void)n;}
void pm_usb_control_set_operations(uint8_t m,pm_input_handler h){(void)m;(void)h;}
void pm_usb_control_input(int c){(void)c;}
void pm_usb_control_poll(bool c){(void)c;}
void pm_usb_control_service_radio(bool c,pm_radio_reader r){(void)c;(void)r;}
pm_sim_dispatch pm_usb_control_tick(bool c,unsigned p,bool a,uint64_t s,unsigned t,bool b,uint32_t l,uint32_t n){(void)c;(void)p;(void)a;(void)s;(void)t;(void)b;(void)l;(void)n;return (pm_sim_dispatch){0};}
void pm_usb_control_dispatched(pm_sim_dispatch e,uint64_t s,unsigned t,uint32_t n){(void)e;(void)s;(void)t;(void)n;}
const pm_button_sim *pm_usb_sim_status(void){return NULL;}
#endif
