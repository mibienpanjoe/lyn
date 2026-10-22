/* Reproduce a checker limitation without suppressing any allocation/report.
 * https://gnome.pages.gitlab.gnome.org/librsvg/devel-docs/memory_leaks.html
 * The recoverable check does not disable the ordinary at-exit leak check. */
#include <assert.h>
#include <fontconfig/fontconfig.h>
#include <sanitizer/lsan_interface.h>
#include <stdio.h>
#include <string.h>

static FcPattern *live_pattern;

__attribute__((noinline)) static void populate(void)
{
    live_pattern = FcPatternCreate();
    assert(live_pattern);
    assert(FcPatternAddString(live_pattern, FC_FAMILY,
                             (const FcChar8 *)"lyn-fontconfig-reachability"));
}

int main(void)
{
    populate();
    int while_alive = __lsan_do_recoverable_leak_check();
    FcChar8 *value = NULL;
    assert(FcPatternGetString(live_pattern, FC_FAMILY, 0, &value) == FcResultMatch);
    assert(strcmp((const char *)value, "lyn-fontconfig-reachability") == 0);
    assert(while_alive != 0 && "Reference Fontconfig false positive was not reproduced");
    FcPatternDestroy(live_pattern);
    live_pattern = NULL;
    value = NULL;
    int after_destroy = __lsan_do_recoverable_leak_check();
    printf("reachablePatternReported=%d afterNormalDestroyReported=%d\n", while_alive, after_destroy);
    assert(after_destroy == 0);
    return 0;
}
