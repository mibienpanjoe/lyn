import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CaptureDetail } from '../lib/ipc-types';
import VoiceCaption from './VoiceCaption.svelte';

afterEach(cleanup);
const capture = {
  id: '10000000-0000-4000-8000-000000000001',
  kind: 'audio',
  caption: 'Texte généré',
  captionSource: 'transcript_generated',
  captionRevision: 4,
  enrichmentStatus: 'completed',
  media: { available: true },
} as CaptureDetail;

describe('voice caption correction', () => {
  it('saves against the revision opened by the editor and preserves a rejected draft', async () => {
    const onsave = vi.fn().mockResolvedValue(false);
    const { rerender } = render(VoiceCaption, {
      capture,
      onsave,
      onretry: vi.fn(),
    });
    expect(screen.getByText('Generated caption')).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: 'Edit caption' }));
    const editor = screen.getByRole('textbox', { name: 'Voice caption' });
    await fireEvent.input(editor, { target: { value: 'Ma correction' } });
    await rerender({
      capture: { ...capture, captionRevision: 5 },
      onsave,
      onretry: vi.fn(),
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Save caption' }));
    expect(onsave).toHaveBeenCalledWith(capture.id, 4, 'Ma correction');
    expect(editor).toHaveValue('Ma correction');
    onsave.mockResolvedValue(true);
    await fireEvent.click(screen.getByRole('button', { name: 'Save caption' }));
    await waitFor(() =>
      expect(screen.queryByRole('textbox')).not.toBeInTheDocument(),
    );
  });

  it('requires an explicit retry after a failure and shows pending work without replay', async () => {
    const onretry = vi.fn().mockResolvedValue(true);
    const failed = {
      ...capture,
      caption: null,
      captionSource: null,
      enrichmentStatus: 'failed',
    } as CaptureDetail;
    const { rerender } = render(VoiceCaption, {
      capture: failed,
      onsave: vi.fn(),
      onretry,
    });
    expect(
      screen.getByText('Transcription failed. Your recording is saved.'),
    ).toBeVisible();
    expect(onretry).not.toHaveBeenCalled();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Retry transcription' }),
    );
    expect(onretry).toHaveBeenCalledWith(capture.id, 4);
    await rerender({
      capture: { ...failed, enrichmentStatus: 'pending' },
      onsave: vi.fn(),
      onretry,
    });
    expect(screen.getByRole('status')).toHaveTextContent(
      'Transcribing on this device…',
    );
    expect(
      screen.queryByRole('button', { name: 'Retry transcription' }),
    ).not.toBeInTheDocument();
    expect(onretry).toHaveBeenCalledTimes(1);
  });

  it('retires an editor when the selected capture changes', async () => {
    const props = { capture, onsave: vi.fn(), onretry: vi.fn() };
    const { rerender } = render(VoiceCaption, props);
    await fireEvent.click(screen.getByRole('button', { name: 'Edit caption' }));
    await rerender({
      ...props,
      capture: { ...capture, id: '10000000-0000-4000-8000-000000000002' },
    });
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  });
});
