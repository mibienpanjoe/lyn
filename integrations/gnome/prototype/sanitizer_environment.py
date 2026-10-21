"""Closed sanitizer policy for private fixtures; never inherit suppressions."""


def sanitizer_environment(base):
    return dict(base, ASAN_OPTIONS="detect_leaks=1:halt_on_error=1",
                LSAN_OPTIONS="exitcode=23:print_suppressions=1",
                UBSAN_OPTIONS="halt_on_error=1:print_stacktrace=1")
