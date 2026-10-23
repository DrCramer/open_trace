"""Exact nearest integer of a bounded finite binary64, ties towards +infinity."""
import math


def round_scan(value):
    """Math.round semantics on supported scan values, without floating x+.5.

    Finite scans in[-1e8,1e8] cannot reach the JVM long saturation boundary.
    Integer-ratio arithmetic preserves nextafter(.5,0) instead of promoting it
    to a tie through an additional rounded binary64 addition.
    """
    if type(value) not in (int,float) or not -1e8<=value<=1e8 or not math.isfinite(value):
        raise ValueError('Finite bounded scan required')
    numerator,denominator=float(value).as_integer_ratio()
    return (2*numerator+denominator)//(2*denominator)
