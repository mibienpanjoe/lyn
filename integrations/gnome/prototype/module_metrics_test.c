/* Test-only accounting; does not initialize GTK or authenticate any peer. */
#define LYN_GNOME_TEST_METRICS
#include "module.c"
#include <assert.h>

int main(void)
{
    CallbackMetrics metrics = {0};
    assert(record_metrics(&metrics, 0));
    assert(record_metrics(&metrics, 12));
    assert(record_metrics(&metrics, 5));
    assert(metrics.count == 3 && metrics.total_us == 17 && metrics.max_us == 12);
    assert(!record_metrics(&metrics, -1));
    assert(metrics.count == 3 && metrics.total_us == 17 && metrics.max_us == 12);
    metrics.count = G_MAXUINT64;
    assert(!record_metrics(&metrics, 1));
    metrics.count = 1;
    metrics.total_us = G_MAXUINT64;
    assert(!record_metrics(&metrics, 1));
    assert(metrics.count == 1 && metrics.total_us == G_MAXUINT64 && metrics.max_us == 12);
    g_print("PASS: bounded test-only callback accounting, zero/negative/overflow cases\n");
    return 0;
}
