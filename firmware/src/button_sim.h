#pragma once
#include "core.h"
#define PM_SIM_STEPS 4u
#define PM_SIM_HOLD_MAX_MS 5000u
#define PM_SIM_JOB_MAX_MS 15000u
#define PM_SIM_CLOSE_GUARD_MS 500u

typedef enum { PM_SIM_IDLE, PM_SIM_STAGING, PM_SIM_WAIT_CLOSE, PM_SIM_RUNNING, PM_SIM_DONE, PM_SIM_CANCELLED } pm_sim_phase;
typedef enum { PM_SIM_OK, PM_SIM_BUSY, PM_SIM_INVALID, PM_SIM_OLD_JOB, PM_SIM_CONFLICT, PM_SIM_UNSUPPORTED } pm_sim_result;
typedef enum { PM_SIM_CANCEL_NONE, PM_SIM_CANCEL_COMMAND, PM_SIM_CANCEL_USB, PM_SIM_CANCEL_PHYSICAL, PM_SIM_CANCEL_LEASE, PM_SIM_CANCEL_STATE, PM_SIM_CANCEL_BUSY, PM_SIM_CANCEL_RUN } pm_sim_reason;
typedef struct {
    uint32_t delay_ms, hold_ms;
    unsigned key;
    bool present, accepted, dispatched, released, cancelled;
    int logical;
    uint32_t dispatched_ms, released_ms;
} pm_sim_step;
typedef struct {
    uint64_t id, last_job, session, bound_boot, bound_run, bound_session;
    unsigned count, current, expected_state, buttons;
    pm_sim_phase phase;
    pm_sim_reason reason;
    bool detached, connected, ever_connected, raw_down, release_requested;
    uint32_t start_ms, lease_end_ms, release_ms;
    pm_button virtual_button;
    pm_sim_step steps[PM_SIM_STEPS];
} pm_button_sim;
typedef struct { bool valid; unsigned logical, step; bool long_press; uint64_t job; } pm_sim_dispatch;
void pm_sim_init(pm_button_sim *s, unsigned buttons);
pm_sim_result pm_sim_begin(pm_button_sim *s, uint64_t job, unsigned count, uint64_t session, unsigned state, uint32_t now);
pm_sim_result pm_sim_step_set(pm_button_sim *s, uint64_t job, unsigned slot, uint32_t delay, unsigned key, uint32_t hold);
pm_sim_result pm_sim_commit(pm_button_sim *s, uint64_t job, uint32_t now);
pm_sim_result pm_sim_press(pm_button_sim *s, uint64_t job, unsigned key, uint32_t lease, uint64_t session, unsigned state, uint32_t now);
pm_sim_result pm_sim_release(pm_button_sim *s, uint64_t job, uint32_t now);
void pm_sim_cancel(pm_button_sim *s, pm_sim_reason why, uint32_t now);
pm_sim_dispatch pm_sim_tick(pm_button_sim *s, bool connected, unsigned real_mask, bool input_allowed,
                            uint64_t session, unsigned state, bool standby, uint32_t long_ms, uint32_t now);
void pm_sim_dispatched(pm_button_sim *s, unsigned slot, unsigned logical, uint32_t now, uint64_t session, unsigned new_state);
