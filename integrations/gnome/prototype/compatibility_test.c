/* Library-version refusal before any resident module or GTK callback setup. */
#include <gtk/gtk.h>
#include <vte/vte.h>
#include <gmodule.h>

#ifdef LYN_GNOME_TEST_REFUSED_MODULE
static guint gtk_versions[3] = {99, 24, 33};
#else
static guint gtk_versions[3] = {3, 24, 33};
#endif
static guint vte_versions[3] = {0, 68, 0};
static guint fixture_gtk_major(void) { return gtk_versions[0]; }
static guint fixture_gtk_minor(void) { return gtk_versions[1]; }
static guint fixture_gtk_micro(void) { return gtk_versions[2]; }
static guint fixture_vte_major(void) { return vte_versions[0]; }
static guint fixture_vte_minor(void) { return vte_versions[1]; }
static guint fixture_vte_micro(void) { return vte_versions[2]; }
static guint executable_reads;
static gchar *fixture_executable_read(const gchar *filename, GError **error)
{
    (void)filename;
    (void)error;
    executable_reads++;
    return NULL;
}
#define gtk_get_major_version fixture_gtk_major
#define gtk_get_minor_version fixture_gtk_minor
#define gtk_get_micro_version fixture_gtk_micro
#define vte_get_major_version fixture_vte_major
#define vte_get_minor_version fixture_vte_minor
#define vte_get_micro_version fixture_vte_micro
#define g_file_read_link fixture_executable_read
#include "module.c"

#ifndef LYN_GNOME_TEST_REFUSED_MODULE
int main(int argc, char **argv)
{
    g_assert_cmpint(argc, ==, 2);
    /* The separately compiled test module has no runtime override: its GTK
     * major getter always returns 99. Exercise GModule's actual load hook. */
    GModule *refused = g_module_open(argv[1], G_MODULE_BIND_LOCAL);
    g_assert_null(refused);
    g_assert_nonnull(strstr(g_module_error(), "unverified GTK/VTE version"));
    g_assert_true(runtime_compatible());
    for (guint library = 0; library < 2; library++) {
        guint *versions = library ? vte_versions : gtk_versions;
        for (guint field = 0; field < 3; field++) {
            guint original = versions[field];
            versions[field] = original + 1;
            g_assert_false(runtime_compatible());
            /* NULL is intentional: refusal must precede accessing GModule. */
            g_assert_cmpstr(g_module_check_init(NULL), ==,
                            "Lyn native prototype: unverified GTK/VTE version");
            g_assert_null(module_path);
            g_assert_null(endpoint);
            gtk_module_init(NULL, NULL);
            g_assert_cmpuint(executable_reads, ==, 0);
            g_assert_null(module_path);
            g_assert_null(endpoint);
            versions[field] = original;
            if (original > 0) {
                versions[field] = original - 1;
                g_assert_false(runtime_compatible());
                g_assert_nonnull(g_module_check_init(NULL));
                versions[field] = original;
            }
        }
    }
    g_assert_true(runtime_compatible());
    g_print("PASS: simulated GTK/VTE mismatches refuse init; real GModule loader rejects incompatible fixture\n");
    return 0;
}
#endif
