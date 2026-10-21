# Generated; edit contracts/observation-domain.json. 77898d6071d480b36086ae67c327c7b12e59d5e7870cdbfe733a32bec5296284
from dataclasses import dataclass
from enum import Enum
@dataclass(frozen=True, slots=True)
class PicoSendAttemptOrdinal:
    value: int
    def __post_init__(self):
        if type(self.value) is not int or not 0 <= self.value < 2**32: raise ValueError("PicoSendAttemptOrdinal")
@dataclass(frozen=True, slots=True)
class HostDecodedOrdinal:
    value: int
    def __post_init__(self):
        if type(self.value) is not int or not 0 <= self.value < 2**64: raise ValueError("HostDecodedOrdinal")
@dataclass(frozen=True, slots=True)
class HostProcessedOrdinal:
    value: int
    def __post_init__(self):
        if type(self.value) is not int or not 0 <= self.value < 2**64: raise ValueError("HostProcessedOrdinal")
@dataclass(frozen=True, slots=True)
class PicoMicros:
    value: int
    def __post_init__(self):
        if type(self.value) is not int or not 0 <= self.value < 2**64: raise ValueError("PicoMicros")
@dataclass(frozen=True, slots=True)
class HostProbeMicros:
    value: int
    def __post_init__(self):
        if type(self.value) is not int or not 0 <= self.value < 2**64: raise ValueError("HostProbeMicros")
@dataclass(frozen=True, slots=True)
class PiMonotonicNs:
    value: int
    def __post_init__(self):
        if type(self.value) is not int or not 0 <= self.value < 2**64: raise ValueError("PiMonotonicNs")
class PicoState(Enum):
    IDLE = "IDLE"
    ARMED = "ARMED"
    RECORDING = "RECORDING"
    FROZEN = "FROZEN"
    PREPARED = "PREPARED"
PICO_TRANSITIONS = {('IDLE', 'ARM_CONNECTED'): 'PREPARED', ('IDLE', 'ARM_DETACHED'): 'ARMED', ('PREPARED', 'CLOSE_GATE'): 'ARMED', ('ARMED', 'OPEN_GATE'): 'PREPARED', ('ARMED', 'START'): 'RECORDING', ('PREPARED', 'FREEZE'): 'FROZEN', ('ARMED', 'FREEZE'): 'FROZEN', ('RECORDING', 'FREEZE'): 'FROZEN', ('FROZEN', 'RELEASE'): 'IDLE'}
class HostState(Enum):
    Idle = "Idle"
    Armed = "Armed"
    Recording = "Recording"
    Frozen = "Frozen"
HOST_TRANSITIONS = {('Idle', 'Arm'): 'Armed', ('Armed', 'Start'): 'Recording', ('Armed', 'Freeze'): 'Frozen', ('Recording', 'Freeze'): 'Frozen', ('Frozen', 'Release'): 'Idle'}
class EvidenceState(Enum):
    Draft = "Draft"
    Assessed = "Assessed"
    Confirmed = "Confirmed"
    Rejected = "Rejected"
EVIDENCE_TRANSITIONS = {('Draft', 'Assess'): 'Assessed', ('Assessed', 'Confirm'): 'Confirmed', ('Assessed', 'Reject'): 'Rejected'}
