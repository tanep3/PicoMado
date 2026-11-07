#pragma once
#include "bluetooth_audio.h"
/* All calls, including poll/pop and PCM ingress, require the shared CYW43
 * async-context lock. Init only AFTER existing cyw43_arch_init. No run loop. */
typedef enum { PM_BT_READY, PM_BT_FOUND, PM_BT_SCAN_DONE, PM_BT_CONNECTED,
 PM_BT_DISCONNECTED, PM_BT_INPUT, PM_BT_ERROR, PM_BT_CODEC, PM_BT_UNDERFLOW, PM_BT_STOPPED, PM_BT_LINK_DIAGNOSTIC } pm_bt_event_kind;
typedef struct { pm_bt_event_kind kind; uint64_t generation; uint16_t status;
 uint8_t address[6],operation; bool pressed; uint32_t rate,device_class; int32_t detail1,detail2,detail3; char name[64]; } pm_bt_event;
bool pm_bt_runtime_init(void);
bool pm_bt_runtime_next(pm_bt_event *event);
uint32_t pm_bt_runtime_dropped(void);
uint8_t pm_bt_runtime_scan(uint64_t generation);
uint8_t pm_bt_runtime_connect(uint64_t generation,const uint8_t address[6]);
void pm_bt_runtime_stop(uint64_t generation);
bool pm_bt_runtime_ready(void);

uint32_t pm_bt_runtime_rate(void);
void pm_bt_runtime_enable_input(bool enabled);
bool pm_bt_runtime_audio_begin(uint64_t generation,uint64_t first_frame);
pm_bluetooth_audio_result pm_bt_runtime_audio_push(uint64_t generation,uint64_t first_frame,const int16_t *pcm,size_t frames);
uint8_t pm_bt_runtime_play(void);

uint64_t pm_bt_runtime_audio_next_frame(void);
uint64_t pm_bt_runtime_audio_consumed_frames(void);
size_t pm_bt_runtime_audio_free_frames(void);
void pm_bt_runtime_audio_end(void);

bool pm_bt_runtime_audio_active(void);

/* Snapshot under shared runtime lock; counters reset at audio_begin. */
typedef struct {
 uint32_t packets,frames,can_wait_total_ms,can_wait_max_ms;
 uint32_t timer_late_total_ms,timer_late_max_ms,encode_total_ms,encode_max_ms;
 uint32_t last_count,last_bytes,mtu,bitpool;
} pm_bt_audio_diagnostics;
void pm_bt_runtime_audio_diagnostics(pm_bt_audio_diagnostics *out);

/* Read-only SDK connection/configuration state. Never exposes bonding keys. */
typedef struct {uint16_t cid,signaling_cid,media_cid;int connection,source,initiator,endpoint;
 bool exists,outgoing,discover,configured,have_config;
 uint16_t handle,packet_types,mode_interval;uint8_t role,mode;bool packet_types_known,interval_known;} pm_bt_link_snapshot;
void pm_bt_runtime_link_snapshot(pm_bt_link_snapshot *out);

bool pm_bt_runtime_audio_finish(uint64_t generation,uint64_t final_frame);
