"""Compare only pixels inside a fixture-owned screen rectangle."""


def region_changed(before, after, region):
    left = before.new_subpixbuf(*region)
    right = after.new_subpixbuf(*region)
    assert left.get_n_channels() == right.get_n_channels()
    row_bytes = left.get_width() * left.get_n_channels()
    left_pixels, right_pixels = left.get_pixels(), right.get_pixels()
    return any(left_pixels[row * left.get_rowstride():row * left.get_rowstride() + row_bytes]
               != right_pixels[row * right.get_rowstride():row * right.get_rowstride() + row_bytes]
               for row in range(left.get_height()))


def region_has_contrast(pixbuf, region):
    """Reject a flat background; alpha and pixels outside the region do not count."""
    part = pixbuf.new_subpixbuf(*region)
    pixels = part.get_pixels()
    luminance = []
    for row in range(part.get_height()):
        for column in range(part.get_width()):
            offset = row * part.get_rowstride() + column * part.get_n_channels()
            red, green, blue = pixels[offset:offset + 3]
            luminance.append((299 * red + 587 * green + 114 * blue) // 1000)
    return max(luminance) - min(luminance) >= 64
