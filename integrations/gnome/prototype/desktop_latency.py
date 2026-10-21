"""Conservative controller observations, not internal application timestamps."""

import math
import time


def observe_ready(input_ready, context_ready, *, started, clock=time.monotonic, sleep=time.sleep):
    pending = {"inputObservedMs": input_ready, "contextObservedMs": context_ready}
    observed = {}
    deadline = started + 12
    while pending:
        assert clock() < deadline, "Capture readiness observation timed out"
        for name, check in list(pending.items()):
            ready = check()
            now = clock()
            assert started <= now < deadline, "Capture readiness observation exceeded its deadline"
            if ready:
                observed[name] = (now - started) * 1000
                del pending[name]
        if pending:
            sleep(.005)
    return observed


def summarize(values):
    assert values and all(math.isfinite(value) and value >= 0 for value in values)
    ordered = sorted(values)
    p95 = ordered[math.ceil(len(ordered) * .95) - 1]
    return {"count": len(ordered), "p50Ms": ordered[math.ceil(len(ordered) * .5) - 1],
            "p95Ms": p95, "maxMs": ordered[-1], "within250ms": p95 <= 250,
            "requiredSamples": len(ordered) == 100}
