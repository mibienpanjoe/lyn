"""Private image fixture guards, without accessing any desktop clipboard."""

import unittest
from unittest.mock import patch

import desktop_media


class Pixels:
    def __init__(self, data, width, height, stride):
        self.data, self.width, self.height, self.stride = data, width, height, stride

    def new_subpixbuf(self, *region):
        assert region == (0, 0, self.width, self.height)
        return self

    def get_pixels(self):
        return self.data

    def get_width(self):
        return self.width

    def get_height(self):
        return self.height

    def get_rowstride(self):
        return self.stride

    def get_n_channels(self):
        return 4


class ImageFixtureTest(unittest.TestCase):
    def test_fixed_opaque_image_has_two_exact_color_halves(self):
        width, height, rgba = desktop_media.fixture_rgba()
        self.assertEqual((width, height), (192, 96))
        self.assertEqual(len(rgba), width * height * 4)
        row = bytes([230, 30, 30, 255]) * 96 + bytes([30, 220, 60, 255]) * 96
        self.assertEqual(rgba, row * height)

    def test_clipboard_is_not_reached_when_display_guard_refuses(self):
        with patch.object(desktop_media, "require_owned_display", side_effect=AssertionError("host display")) as guard:
            with self.assertRaisesRegex(AssertionError, "host display"):
                desktop_media.publish_image(None)
            guard.assert_called_once_with(None)

    def test_paint_requires_both_fixture_colors_in_actual_rows_not_padding(self):
        red, green, background = bytes([230, 30, 30, 255]), bytes([30, 220, 60, 255]), bytes([0, 0, 0, 255])
        for row, expected in ((red * 20 + green * 20, True),
                              (red * 40, False), (background * 40, False)):
            pixbuf = Pixels((row + green * 120) * 20, 40, 20, 640)
            self.assertEqual(desktop_media.has_fixture_colors(pixbuf, (0, 0, 40, 20)), expected)


if __name__ == "__main__":
    unittest.main()
