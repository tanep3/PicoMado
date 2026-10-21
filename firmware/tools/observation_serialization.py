"""Output adapter. Converting a report to plain values never recreates domain authority."""
from collections.abc import Mapping
from observation_domain import CaptureReport, RunReport

def value(item):
    if isinstance(item,Mapping):return {k:value(v) for k,v in item.items()}
    if isinstance(item,(tuple,list)):return [value(v) for v in item]
    return item

def report_value(report):
    if type(report) not in (CaptureReport,RunReport):raise TypeError('domain result required')
    return value(report)
