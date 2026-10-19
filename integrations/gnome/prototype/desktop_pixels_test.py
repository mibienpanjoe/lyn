"""Paint-region comparisons without a display or GTK application."""

import unittest

import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf

from desktop_pixels import region_changed


class PaintedRegionTest(unittest.TestCase):
    def pair(self, alpha=False):
        before = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, alpha, 8, 10, 8)
        before.fill(0xffffffff)
        return before, before.copy()

    def test_unchanged_region(self):
        self.assertFalse(region_changed(*self.pair(), (1, 1, 2, 2)))

    def test_changes_outside_region_do_not_prove_paint(self):
        before, after = self.pair()
        after.new_subpixbuf(5, 1, 1, 1).fill(0xff0000ff)
        self.assertFalse(region_changed(before, after, (1, 1, 2, 2)))

    def test_change_in_region_is_detected_for_rgb_and_rgba(self):
        for alpha in (False, True):
            with self.subTest(alpha=alpha):
                before, after = self.pair(alpha)
                after.new_subpixbuf(2, 2, 1, 1).fill(0xff0000ff)
                self.assertTrue(region_changed(before, after, (1, 1, 2, 2)))

    def test_distinct_rowstrides_are_respected(self):
        before, after = self.pair()
        before = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, 6, 8)
        before.fill(0xffffffff)
        self.assertNotEqual(before.get_rowstride(), after.get_rowstride())
        self.assertFalse(region_changed(before, after, (1, 1, 2, 2)))
        after.new_subpixbuf(2, 2, 1, 1).fill(0xff0000ff)
        self.assertTrue(region_changed(before, after, (1, 1, 2, 2)))


if __name__ == "__main__":
    unittest.main()
