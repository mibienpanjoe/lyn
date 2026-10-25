"""Read only bounded regions of the private screen through local Tesseract."""
import os
from pathlib import Path
import subprocess
import gi

gi.require_version('GdkPixbuf', '2.0')
from gi.repository import GdkPixbuf


def normalize(text):
    return ' '.join(text.casefold().split())


def read_text(image, rectangle, sdk, single_line=True):
    crop = image.new_subpixbuf(*rectangle)
    scaled = crop.scale_simple(crop.get_width() * 3, crop.get_height() * 3, GdkPixbuf.InterpType.BILINEAR)
    success, buffer = scaled.save_to_bufferv('png', [], [])
    assert success
    environment = dict(os.environ)
    executable = Path('/usr/bin/tesseract')
    if sdk and (sdk / 'root/usr/bin/tesseract').is_file():
        executable = sdk / 'root/usr/bin/tesseract'
        environment['LD_LIBRARY_PATH'] = str(sdk / 'root/usr/lib/x86_64-linux-gnu')
        data = list((sdk / 'root/usr/share/tesseract-ocr').glob('*/tessdata/eng.traineddata'))
        if len(data) != 1:
            raise RuntimeError('Exactly one English OCR data file is required')
        environment['TESSDATA_PREFIX'] = str(data[0].parent)
    result = subprocess.run([str(executable), 'stdin', 'stdout', '-l', 'eng', '--psm', '7' if single_line else '6'],
                            env=environment, input=buffer, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=3, check=True)
    if len(result.stdout) > 16384:
        raise RuntimeError('Unexpected OCR output size')
    return normalize(result.stdout.decode('utf-8'))
