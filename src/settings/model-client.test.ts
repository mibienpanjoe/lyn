import { describe, expect, it, vi } from 'vitest';

import { SettingsCommandError } from './settings-client';
import { createSpeechModelClient } from './model-client';

describe('speech model client', () => {
  it('uses the fixed product model id rather than accepting a path or URL', async () => {
    const invoke = vi.fn().mockResolvedValue({
      ok: true,
      data: { accepted: true, modelId: 'whisper-base-multilingual-v1' },
    });
    await createSpeechModelClient(invoke).install();
    expect(invoke).toHaveBeenCalledWith('install_speech_model', {
      input: { modelId: 'whisper-base-multilingual-v1' },
    });
  });

  it('preserves typed command failures', async () => {
    const invoke = vi.fn().mockResolvedValue({
      ok: false,
      error: {
        code: 'MODEL_DOWNLOAD_FAILED',
        message: 'Download failed',
        retryable: true,
        details: {},
      },
    });
    await expect(
      createSpeechModelClient(invoke).install(),
    ).rejects.toBeInstanceOf(SettingsCommandError);
  });
});
