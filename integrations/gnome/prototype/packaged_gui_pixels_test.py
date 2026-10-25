"""Reject old, nonempty screens using actual OCR of generated raster text."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import gi

gi.require_version('GdkPixbuf', '2.0')
from gi.repository import GdkPixbuf
from packaged_gui_pixels import read_text


class PixelTests(unittest.TestCase):
    def test_expected_and_stale_content_are_distinguished(self):
        sdk_value = os.environ.get('LYN_GNOME_SDK_ROOT')
        sdk = Path(sdk_value).resolve(strict=True) if sdk_value else None
        with tempfile.TemporaryDirectory(prefix='lyn-package-ocr-test-') as cache:
            with patch.dict(os.environ, {'XDG_CACHE_HOME': cache}):
                for text, expected in [('Settings', 'settings'), ('Recent', 'recent'),
                                       ('Packaged note 7319', 'packaged note 7319')]:
                    loader = GdkPixbuf.PixbufLoader.new_with_type('svg')
                    loader.write(f'<svg xmlns="http://www.w3.org/2000/svg" width="600" height="80"><rect width="600" height="80" fill="white"/><text x="12" y="50" font-family="sans-serif" font-size="28" fill="black">{text}</text></svg>'.encode())
                    loader.close()
                    image = loader.get_pixbuf()
                    result = read_text(image, (0, 0, 600, 80), sdk)
                    self.assertEqual(result, expected)
                    if text != 'Settings': self.assertNotEqual(result, 'settings')


if __name__ == '__main__':
    unittest.main()
