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
