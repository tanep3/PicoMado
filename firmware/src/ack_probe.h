#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifndef PM_ACK_PATH_PROBE
#define PM_ACK_PATH_PROBE 0
#endif
#define PM_ACK_PROBE_CAP 96u
#define PM_ACK_PROBE_WINDOW_US UINT64_C(20000000)
typedef enum {
 PM_AP_BUTTON=1,PM_AP_CONTEXT,PM_AP_FRAME,PM_AP_FRAME_CHECK,PM_AP_DRAW_BEGIN,PM_AP_DRAW_END,
 PM_AP_PENDING_SET,PM_AP_PENDING_REPLACED,PM_AP_PENDING_TAKE,PM_AP_PENDING_SKIP,
 PM_AP_ACK_BUILD,PM_AP_BEFORE_LOCK,PM_AP_LOCKED,PM_AP_ENCODED,
 PM_AP_WRITE_BEGIN,PM_AP_WRITE_END,PM_AP_OUTPUT_END,PM_AP_SEND_RETURN,
 PM_AP_CONNECTION,PM_AP_TCP_SAMPLE,PM_AP_PENDING_CLEAR,PM_AP_WRITE_REJECT,PM_AP_TX_POSITION
} pm_ack_probe_event;
typedef struct {
 uint64_t us,session,frame;
 uint32_t connection,message,a,b,c,d;
 uint16_t event,flags;
 uint8_t kind,reserved[3];
} pm_ack_probe_record;
typedef struct {
 uint64_t boot,run,last_run,armed_us,trigger_us,freeze_us,gate_us,usb_overlap_us;
 uint32_t count,dropped,reason,connections,digest,last_digest,arm_buttons,freeze_buttons,gate_buttons;
 unsigned state;
} pm_ack_probe_summary;
typedef enum { PM_AP_IDLE,PM_AP_ARMED,PM_AP_RECORDING,PM_AP_FROZEN,PM_AP_PREPARED } pm_ack_probe_state;
#if PM_ACK_PATH_PROBE
void pm_ack_probe_init(void);
void pm_ack_probe_init_boot(uint64_t boot);
void pm_ack_probe_command(const char *command);
void pm_ack_probe_input(int ch);
void pm_ack_probe_poll(bool usb_connected);
void pm_ack_probe_usb_state(bool connected);
void pm_ack_probe_context(uint64_t session,uint64_t frame);
void pm_ack_probe_live(uint64_t session,bool active);
void pm_ack_probe_note(pm_ack_probe_event event,uint8_t kind,uint64_t session,uint64_t frame,uint32_t a,uint32_t b,uint32_t c,uint32_t d);
void pm_ack_probe_button(uint64_t session,uint64_t display,uint64_t owner,unsigned button,bool long_press);
void pm_ack_probe_before_lock(uint8_t kind,const char *json);
void pm_ack_probe_locked(uint8_t kind);
void pm_ack_probe_encoded(uint8_t kind,const char *json);
void pm_ack_probe_net(pm_ack_probe_event event,uint16_t flags,uint32_t a,uint32_t b,uint32_t c,uint32_t d);
void pm_ack_probe_send_return(uint8_t kind,bool ok);
void pm_ack_probe_connection(void);
bool pm_ack_probe_sample_due(void);
void pm_ack_probe_sample(uint16_t flags,uint32_t a,uint32_t b,uint32_t c,uint32_t d);
size_t pm_ack_probe_ram_bytes(void);
pm_ack_probe_summary pm_ack_probe_read_summary(void);
bool pm_ack_probe_read(unsigned index,pm_ack_probe_record *out);
#else
static inline void pm_ack_probe_init(void){}
static inline void pm_ack_probe_init_boot(uint64_t b){(void)b;}
static inline void pm_ack_probe_command(const char *c){(void)c;}
static inline void pm_ack_probe_poll(bool v){(void)v;}
static inline void pm_ack_probe_context(uint64_t s,uint64_t f){(void)s;(void)f;}
static inline void pm_ack_probe_note(pm_ack_probe_event e,uint8_t k,uint64_t s,uint64_t f,uint32_t a,uint32_t b,uint32_t c,uint32_t d){(void)e;(void)k;(void)s;(void)f;(void)a;(void)b;(void)c;(void)d;}
static inline void pm_ack_probe_button(uint64_t s,uint64_t d,uint64_t o,unsigned b,bool l){(void)s;(void)d;(void)o;(void)b;(void)l;}
static inline void pm_ack_probe_before_lock(uint8_t k,const char *j){(void)k;(void)j;}
static inline void pm_ack_probe_locked(uint8_t k){(void)k;}
static inline void pm_ack_probe_encoded(uint8_t k,const char *j){(void)k;(void)j;}
static inline void pm_ack_probe_net(pm_ack_probe_event e,uint16_t f,uint32_t a,uint32_t b,uint32_t c,uint32_t d){(void)e;(void)f;(void)a;(void)b;(void)c;(void)d;}
static inline void pm_ack_probe_send_return(uint8_t k,bool o){(void)k;(void)o;}
static inline void pm_ack_probe_connection(void){}
static inline bool pm_ack_probe_sample_due(void){return false;}
static inline void pm_ack_probe_sample(uint16_t f,uint32_t a,uint32_t b,uint32_t c,uint32_t d){(void)f;(void)a;(void)b;(void)c;(void)d;}
#endif
