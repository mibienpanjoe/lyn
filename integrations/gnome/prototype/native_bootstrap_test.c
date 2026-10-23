/* A user-owned executable named gnome-terminal-server is still not admitted. */
#include <glib.h>
#include <gmodule.h>

int main(int argc, char **argv)
{
    g_assert_cmpint(argc, ==, 2);
    gchar *existing = g_strdup_printf("%s:/tmp/existing-module.so:%s", argv[1], argv[1]);
    gchar *generic = g_strdup_printf("atk-bridge:%s", argv[1]);
    g_setenv("GTK3_MODULES", existing, TRUE);
    g_setenv("GTK_MODULES", generic, TRUE);
    GModule *module = g_module_open(argv[1], G_MODULE_BIND_LOCAL);
    g_assert_null(module);
    g_assert_nonnull(strstr(g_module_error(), "not a supported system GNOME server"));
    g_assert_cmpstr(g_getenv("GTK3_MODULES"), ==, "/tmp/existing-module.so");
    g_assert_cmpstr(g_getenv("GTK_MODULES"), ==, "atk-bridge");
    g_free(existing);
    g_free(generic);
    g_print("PASS: native entry refuses an untrusted same-basename executable and scrubs only its entries\n");
    return 0;
}
