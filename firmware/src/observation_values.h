/* Generated; edit contracts/observation-domain.json. 77898d6071d480b36086ae67c327c7b12e59d5e7870cdbfe733a32bec5296284 */
#pragma once
#include <stdint.h>
#include <stdbool.h>
typedef struct { uint32_t value; } obs_PicoSendAttemptOrdinal;
static inline obs_PicoSendAttemptOrdinal obs_PicoSendAttemptOrdinal_from_wire(uint32_t v){return (obs_PicoSendAttemptOrdinal){v};}
static inline uint32_t obs_PicoSendAttemptOrdinal_wire(obs_PicoSendAttemptOrdinal v){return v.value;}
typedef struct { uint64_t value; } obs_HostDecodedOrdinal;
static inline obs_HostDecodedOrdinal obs_HostDecodedOrdinal_from_wire(uint64_t v){return (obs_HostDecodedOrdinal){v};}
static inline uint64_t obs_HostDecodedOrdinal_wire(obs_HostDecodedOrdinal v){return v.value;}
typedef struct { uint64_t value; } obs_HostProcessedOrdinal;
static inline obs_HostProcessedOrdinal obs_HostProcessedOrdinal_from_wire(uint64_t v){return (obs_HostProcessedOrdinal){v};}
static inline uint64_t obs_HostProcessedOrdinal_wire(obs_HostProcessedOrdinal v){return v.value;}
typedef struct { uint64_t value; } obs_PicoMicros;
static inline obs_PicoMicros obs_PicoMicros_from_wire(uint64_t v){return (obs_PicoMicros){v};}
static inline uint64_t obs_PicoMicros_wire(obs_PicoMicros v){return v.value;}
typedef struct { uint64_t value; } obs_HostProbeMicros;
static inline obs_HostProbeMicros obs_HostProbeMicros_from_wire(uint64_t v){return (obs_HostProbeMicros){v};}
static inline uint64_t obs_HostProbeMicros_wire(obs_HostProbeMicros v){return v.value;}
typedef struct { uint64_t value; } obs_PiMonotonicNs;
static inline obs_PiMonotonicNs obs_PiMonotonicNs_from_wire(uint64_t v){return (obs_PiMonotonicNs){v};}
static inline uint64_t obs_PiMonotonicNs_wire(obs_PiMonotonicNs v){return v.value;}
typedef enum {obs_pico_IDLE,obs_pico_ARMED,obs_pico_RECORDING,obs_pico_FROZEN,obs_pico_PREPARED} obs_pico_state;
typedef enum {obs_pico_ARM_CONNECTED,obs_pico_ARM_DETACHED,obs_pico_CLOSE_GATE,obs_pico_FREEZE,obs_pico_OPEN_GATE,obs_pico_RELEASE,obs_pico_START} obs_pico_event;
static inline bool obs_pico_next(obs_pico_state s,obs_pico_event e,obs_pico_state *out){
 if(s==obs_pico_IDLE && e==obs_pico_ARM_CONNECTED){*out=obs_pico_PREPARED;return true;}
 if(s==obs_pico_IDLE && e==obs_pico_ARM_DETACHED){*out=obs_pico_ARMED;return true;}
 if(s==obs_pico_PREPARED && e==obs_pico_CLOSE_GATE){*out=obs_pico_ARMED;return true;}
 if(s==obs_pico_ARMED && e==obs_pico_OPEN_GATE){*out=obs_pico_PREPARED;return true;}
 if(s==obs_pico_ARMED && e==obs_pico_START){*out=obs_pico_RECORDING;return true;}
 if(s==obs_pico_PREPARED && e==obs_pico_FREEZE){*out=obs_pico_FROZEN;return true;}
 if(s==obs_pico_ARMED && e==obs_pico_FREEZE){*out=obs_pico_FROZEN;return true;}
 if(s==obs_pico_RECORDING && e==obs_pico_FREEZE){*out=obs_pico_FROZEN;return true;}
 if(s==obs_pico_FROZEN && e==obs_pico_RELEASE){*out=obs_pico_IDLE;return true;}
 return false;
}
typedef enum {obs_host_Idle,obs_host_Armed,obs_host_Recording,obs_host_Frozen} obs_host_state;
typedef enum {obs_host_Arm,obs_host_Freeze,obs_host_Release,obs_host_Start} obs_host_event;
static inline bool obs_host_next(obs_host_state s,obs_host_event e,obs_host_state *out){
 if(s==obs_host_Idle && e==obs_host_Arm){*out=obs_host_Armed;return true;}
 if(s==obs_host_Armed && e==obs_host_Start){*out=obs_host_Recording;return true;}
 if(s==obs_host_Armed && e==obs_host_Freeze){*out=obs_host_Frozen;return true;}
 if(s==obs_host_Recording && e==obs_host_Freeze){*out=obs_host_Frozen;return true;}
 if(s==obs_host_Frozen && e==obs_host_Release){*out=obs_host_Idle;return true;}
 return false;
}
typedef enum {obs_evidence_Draft,obs_evidence_Assessed,obs_evidence_Confirmed,obs_evidence_Rejected} obs_evidence_state;
typedef enum {obs_evidence_Assess,obs_evidence_Confirm,obs_evidence_Reject} obs_evidence_event;
static inline bool obs_evidence_next(obs_evidence_state s,obs_evidence_event e,obs_evidence_state *out){
 if(s==obs_evidence_Draft && e==obs_evidence_Assess){*out=obs_evidence_Assessed;return true;}
 if(s==obs_evidence_Assessed && e==obs_evidence_Confirm){*out=obs_evidence_Confirmed;return true;}
 if(s==obs_evidence_Assessed && e==obs_evidence_Reject){*out=obs_evidence_Rejected;return true;}
 return false;
}
