import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  CancelSpeechModelInstallResult,
  CommandResult,
  InstallSpeechModelResult,
  RemoveSpeechModelResult,
  SpeechModelStatus,
} from '../lib/ipc-types';
import { SettingsCommandError } from './settings-client';

const MODEL_ID = 'whisper-base-multilingual-v1';

type Invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;
type Subscribe = SpeechModelClient['subscribe'];

async function command<T>(
  call: Invoke,
  name: string,
  input: unknown,
): Promise<T> {
  const result = await call<CommandResult<T>>(name, { input });
  if (!result.ok) throw new SettingsCommandError(result.error);
  return result.data;
}

export interface SpeechModelClient {
  status(): Promise<SpeechModelStatus>;
  install(): Promise<InstallSpeechModelResult>;
  cancel(): Promise<CancelSpeechModelInstallResult>;
  remove(): Promise<RemoveSpeechModelResult>;
  subscribe(handler: (status: SpeechModelStatus) => void): Promise<UnlistenFn>;
}

export function createSpeechModelClient(
  call: Invoke = invoke,
  subscribe: Subscribe = (handler) =>
    listen<SpeechModelStatus>('model://download-progress', (event) =>
      handler(event.payload),
    ),
): SpeechModelClient {
  return {
    status: () => command(call, 'get_speech_model_status', {}),
    install: () => command(call, 'install_speech_model', { modelId: MODEL_ID }),
    cancel: () => command(call, 'cancel_speech_model_install', {}),
    remove: () => command(call, 'remove_speech_model', { modelId: MODEL_ID }),
    subscribe,
  };
}

export const speechModelClient = createSpeechModelClient();
