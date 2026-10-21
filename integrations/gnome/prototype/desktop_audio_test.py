"""Fixed WAV recipe decoder; no devices or desktop access."""

import io
import unittest
import wave

from desktop_audio import assert_synthetic_wav


def wav(rate=16000, samples=1600, value=8192, channels=1):
    output = io.BytesIO()
    with wave.open(output, "wb") as writer:
        writer.setnchannels(channels)
        writer.setsampwidth(2)
        writer.setframerate(rate)
        writer.writeframes(value.to_bytes(2, "little", signed=True) * samples)
    return output.getvalue()


class SyntheticWavTest(unittest.TestCase):
    def test_fixed_mono_pcm_recipe_is_accepted(self):
        assert_synthetic_wav(wav())

    def test_other_rate_duration_samples_or_channels_are_refused(self):
        for data in (wav(rate=8000), wav(samples=800), wav(value=0), wav(channels=2)):
            with self.subTest(data=data[:44]):
                with self.assertRaises(AssertionError):
                    assert_synthetic_wav(data)

    def test_non_wav_is_refused(self):
        with self.assertRaises((wave.Error, EOFError)):
            assert_synthetic_wav(b"not a WAV fixture")


if __name__ == "__main__":
    unittest.main()
