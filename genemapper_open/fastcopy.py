"""Fast exact copy of JSON-like data (dict/list/tuple/str/number/bool/None).

Reports and cohorts are plain data, so a pickle round trip (C code) gives the
same structure and shared-reference behaviour as ``copy.deepcopy`` several
times faster.
"""
import copy
import pickle

_ATOMIC = (str, int, float, bool, type(None))


def fastcopy(value):
    if type(value) in _ATOMIC:
        return value
    try:
        return pickle.loads(pickle.dumps(value, protocol=pickle.HIGHEST_PROTOCOL))
    except (pickle.PicklingError, TypeError, AttributeError, RecursionError):
        # objects that cannot be pickled (closures, local classes): same result, just slower
        return copy.deepcopy(value)
