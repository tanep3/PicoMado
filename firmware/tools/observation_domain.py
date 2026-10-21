"""Pure observation aggregate. No transport, storage, CLI or plugin dependency."""
from copy import deepcopy
from collections.abc import Mapping
from types import MappingProxyType
from dataclasses import dataclass
from observation_values import EVIDENCE_TRANSITIONS

_TOKEN = object()


@dataclass(frozen=True)
class MeasurementAssessment:
    errors: tuple
    details: dict

    @property
    def measurement_complete(self):
        return not self.errors

    def record(self):
        return dict(measurement_complete=self.measurement_complete, errors=list(self.errors), **deepcopy(self.details))


def _freeze(value):
    if isinstance(value, _Result):return value
    if isinstance(value, Mapping):return MappingProxyType({k:_freeze(v) for k,v in value.items()})
    if isinstance(value, (list,tuple)):return tuple(_freeze(v) for v in value)
    if value is None or type(value) in (str,int,float,bool):return value
    raise TypeError('unsupported domain value')


class VerifiedEvidence:
    __slots__ = ('_binding',)
    def __init__(self, binding, *, _token=None):
        if _token is not _TOKEN:raise TypeError('only shared evidence verifier may issue evidence')
        object.__setattr__(self,'_binding',_freeze(binding))
    def __setattr__(self,*args):raise TypeError('immutable verified evidence')


class _Result(Mapping):
    __slots__=('__data',)
    def __init__(self, value, *, _token=None):
        if _token is not _TOKEN:raise TypeError('only the domain aggregate may construct a result')
        complete=value.get('complete',False);verified=value.get('evidence_confirmed',False)
        if type(complete)is not bool or type(verified)is not bool or complete and not verified:
            raise ValueError('completion requires verified evidence')
        object.__setattr__(self,'_Result__data',_freeze(value))
    def __setattr__(self,*args):raise TypeError('immutable domain result')
    def __getitem__(self,key):return self.__data[key]
    def __iter__(self):return iter(self.__data)
    def __len__(self):return len(self.__data)
    @property
    def complete(self):return self.__data['complete']
    @property
    def evidence_confirmed(self):return self.__data.get('evidence_confirmed',False)


class CaptureReport(_Result):
    __slots__=()


class RunReport(_Result):
    __slots__=()


class CaptureWindow:
    __slots__ = ('__state','__binding','__assessment')
    def __init__(self, binding):
        self.__state='Draft';self.__binding=deepcopy(binding);self.__assessment=None
    def __advance(self,event):
        key=(self.__state,event)
        if key not in EVIDENCE_TRANSITIONS:raise ValueError('invalid evidence transition')
        self.__state=EVIDENCE_TRANSITIONS[key]
    def assess(self, assessment):
        if type(assessment) is not MeasurementAssessment:raise TypeError('assessment required')
        self.__advance('Assess');self.__assessment=assessment
    def confirm(self, evidence, **facts):
        if type(evidence) is not VerifiedEvidence or evidence._binding!=self.__binding:
            raise TypeError('matching verified evidence required')
        if self.__assessment is None:raise ValueError('unassessed window')
        if {'complete','evidence_confirmed','measurement_complete','errors'}.intersection(facts):raise ValueError('reserved outcome fields')
        self.__advance('Confirm' if self.__assessment.measurement_complete else 'Reject')
        return CaptureReport(self.__assessment.record()|facts|dict(complete=self.__state=='Confirmed', evidence_confirmed=True),_token=_TOKEN)


def _verified_evidence(binding):
    """Static policy restricts this trusted-boundary operation to the shared verifier."""
    return VerifiedEvidence(binding,_token=_TOKEN)


def confirm(assessment,binding,evidence,**facts):
    window=CaptureWindow(binding);window.assess(assessment);return window.confirm(evidence,**facts)


def incomplete(*errors):
    return CaptureReport(dict(complete=False,evidence_confirmed=False,errors=list(errors)),_token=_TOKEN)


def pending_assessment(*errors):
    return MeasurementAssessment(tuple(errors),dict(trigger='natural_heartbeat'))


def run_outcome(capture, *, errors, pico_idle, host_idle, owner_closed, **facts):
    if type(capture) is not CaptureReport:raise TypeError('domain capture report required')
    return RunReport(dict(complete=bool(capture.complete and capture.evidence_confirmed and not errors and pico_idle and host_idle and owner_closed), evidence_confirmed=capture.evidence_confirmed,
        capture_quality=capture,errors=deepcopy(errors),pico_idle=pico_idle,host_idle=host_idle,owner_closed=owner_closed,**facts),_token=_TOKEN)


def operation_failed(report,errors):
    if type(report) is not RunReport:raise TypeError('domain run report required')
    return RunReport(dict(report)|dict(complete=False,errors=deepcopy(errors)),_token=_TOKEN)


def with_error(assessment,error):
    if type(assessment) is not MeasurementAssessment:raise TypeError('assessment required')
    return MeasurementAssessment(assessment.errors+(error,),deepcopy(assessment.details))
