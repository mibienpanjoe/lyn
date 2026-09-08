import type {
  AppSettings,
  AudioPlaybackResult,
  CaptureDetail,
  CaptureSession,
  CaptureSummary,
  ContextProviderKind,
  ContextRef,
  ContextSourceOption,
  DeleteCaptureResult,
  LibraryScope,
  OpenMediaResult,
  Page,
  SearchResultItem,
  SpeechModelStatus,
  ThemeSetting,
  IntegrationStatus,
  LanguageSetting,
} from './ipc-types';
import type {
  CaptureFilters,
  LibraryClient,
  SearchFilters,
} from '../library/library-client';
import type { SettingsClient } from '../settings/settings-client';
import type { SpeechModelClient } from '../settings/model-client';
import type { IntegrationClient } from '../settings/integration-client';
import type { CaptureClient } from '../capture/capture-client';

const sampleContexts: ContextRef[] = [
  { id: 'project-lyn', kind: 'project', name: 'lyn' },
  { id: 'project-notes', kind: 'project', name: 'research' },
  { id: 'context-inbox', kind: 'standalone', name: 'Inbox' },
];

const sampleCaptures: CaptureDetail[] = [
  {
    id: 'cap-1',
    kind: 'text',
    context: sampleContexts[0],
    branchName: 'feature/ui-clean-modern-polish',
    capturedAt: '2026-09-05T22:30:00Z',
    updatedAt: '2026-09-05T22:30:00Z',
    textExcerpt:
      'Redesign stream list items as inset rounded cards with 8px radius and soft hover shadows.',
    textBody:
      'Redesign stream list items as inset rounded cards with 8px radius and soft hover shadows.\n\nEliminate harsh divider lines and replace them with subtle tonal contrast. Ensure focus states, active states, and dark mode tokens adhere to calm desktop ergonomics.',
    caption: null,
    captionSource: null,
    media: null,
    sourceApp: 'Cursor',
    sourceWindowTitle: 'Lyn — Visual Polish',
    enrichmentStatus: 'not_requested',
  },
  {
    id: 'cap-2',
    kind: 'image',
    context: sampleContexts[0],
    branchName: 'feature/ui-clean-modern-polish',
    capturedAt: '2026-09-05T21:15:00Z',
    updatedAt: '2026-09-05T21:15:00Z',
    textExcerpt: null,
    textBody: null,
    caption:
      'Quick capture HUD layout comparison: Before and after unification',
    captionSource: 'user',
    media: {
      mediaId: 'media-1',
      kind: 'image',
      previewUri:
        'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="600" height="320" viewBox="0 0 600 320"%3E%3Crect width="600" height="320" fill="%23202620" rx="8"/%3E%3Ctext x="50%25" y="50%25" fill="%2379c4a9" font-family="sans-serif" font-size="18" text-anchor="middle" dominant-baseline="middle"%3EScreenshot Preview%3C/text%3E%3C/svg%3E',
      durationMs: null,
      widthPx: 600,
      heightPx: 320,
      available: true,
    },
    sourceApp: 'Firefox',
    sourceWindowTitle: 'Lyn Design Reference',
    enrichmentStatus: 'not_requested',
  },
  {
    id: 'cap-3',
    kind: 'audio',
    context: sampleContexts[1],
    branchName: 'main',
    capturedAt: '2026-09-04T18:00:00Z',
    updatedAt: '2026-09-04T18:00:00Z',
    textExcerpt: null,
    textBody: null,
    caption:
      'Voice memo on local Whisper speech distribution and lightweight models.',
    captionSource: 'transcript_generated',
    media: {
      mediaId: 'media-2',
      kind: 'audio',
      previewUri: '',
      durationMs: 42000,
      widthPx: null,
      heightPx: null,
      available: true,
    },
    sourceApp: null,
    sourceWindowTitle: null,
    enrichmentStatus: 'completed',
  },
  {
    id: 'cap-4',
    kind: 'text',
    context: sampleContexts[2],
    branchName: null,
    capturedAt: '2026-09-03T09:40:00Z',
    updatedAt: '2026-09-03T09:40:00Z',
    textExcerpt:
      'Review keyboard accessibility and shortcut keycap badge styling.',
    textBody:
      'Review keyboard accessibility and shortcut keycap badge styling.\n\nVerify that modifier badges use tabular numerals, crisp 10px typography, and distinct focus rings for keyboard navigation.',
    caption: null,
    captionSource: null,
    media: null,
    sourceApp: 'Terminal',
    sourceWindowTitle: 'zsh',
    enrichmentStatus: 'not_requested',
  },
];

let devSettingsStore: AppSettings = {
  globalShortcut: 'Control+Shift+Space',
  providerTieBreakOrder: [
    'vscode',
    'cursor',
    'browser',
    'shell',
    'foreground_window',
  ] as ContextProviderKind[],
  theme: 'system' as ThemeSetting,
  localSpeechEnabled: true,
  language: 'english' as LanguageSetting,
};

let devSpeechStatus: SpeechModelStatus = {
  state: 'installed',
  modelId: 'whisper-base-multilingual-v1',
  label: 'Multilingual base (74 MB)',
  downloadedBytes: 77594624,
  totalBytes: 77594624,
  errorCode: null,
};

let devSession: CaptureSession = {
  sessionId: 'dev-session-1',
  contextResolution: {
    state: 'resolved',
    candidate: {
      context: sampleContexts[0],
      branchName: 'feature/ui-clean-modern-polish',
      provider: 'manual',
      requiresConfirmation: false,
    },
    selection: { kind: 'saved_context', contextId: sampleContexts[0].id },
  },
  stagedMedia: null,
  recordingState: { state: 'idle' },
};

export const devLibraryClient: LibraryClient = {
  async listContexts(): Promise<ContextRef[]> {
    return sampleContexts;
  },
  async listCaptures(
    scope: LibraryScope,
    filters?: Partial<CaptureFilters>,
  ): Promise<Page<CaptureSummary>> {
    let items = [...sampleCaptures];
    if (scope.kind === 'context') {
      items = items.filter((c) => c.context.id === scope.contextId);
    }
    if (filters?.branchName) {
      items = items.filter((c) => c.branchName === filters.branchName);
    }
    if (filters?.captureKinds && filters.captureKinds.length > 0) {
      items = items.filter((c) => filters.captureKinds!.includes(c.kind));
    }
    return {
      items: items.map((c) => ({
        id: c.id,
        kind: c.kind,
        context: c.context,
        branchName: c.branchName,
        capturedAt: c.capturedAt,
        textExcerpt: c.textExcerpt,
        caption: c.caption,
        captionSource: c.captionSource,
        media: c.media,
      })),
      nextCursor: null,
    };
  },
  async getCapture(captureId: string): Promise<CaptureDetail> {
    const item =
      sampleCaptures.find((c) => c.id === captureId) ?? sampleCaptures[0];
    return item;
  },
  async deleteCapture(captureId: string): Promise<DeleteCaptureResult> {
    const index = sampleCaptures.findIndex((c) => c.id === captureId);
    if (index !== -1) {
      sampleCaptures.splice(index, 1);
    }
    return { deleted: true };
  },
  async searchCaptures(
    query: string,
    filters: SearchFilters,
  ): Promise<Page<SearchResultItem>> {
    const q = query.toLowerCase();
    const matches = sampleCaptures.filter((c) => {
      const matchText =
        (c.textExcerpt ?? '').toLowerCase().includes(q) ||
        (c.caption ?? '').toLowerCase().includes(q) ||
        (c.textBody ?? '').toLowerCase().includes(q);
      const matchContext =
        !filters.contextId || c.context.id === filters.contextId;
      const matchBranch =
        !filters.branchName || c.branchName === filters.branchName;
      const matchKind =
        !filters.captureKinds.length || filters.captureKinds.includes(c.kind);
      return matchText && matchContext && matchBranch && matchKind;
    });
    return {
      items: matches.map((c) => ({
        capture: {
          id: c.id,
          kind: c.kind,
          context: c.context,
          branchName: c.branchName,
          capturedAt: c.capturedAt,
          textExcerpt: c.textExcerpt,
          caption: c.caption,
          captionSource: c.captionSource,
          media: c.media,
        },
        matchedField: c.caption ? 'caption' : 'text_body',
        snippet: c.textExcerpt ?? c.caption ?? '',
      })),
      nextCursor: null,
    };
  },
  async playMedia(): Promise<AudioPlaybackResult> {
    return { playing: true, durationMs: 42000 };
  },
  async stopPlayback(): Promise<AudioPlaybackResult> {
    return { playing: false, durationMs: null };
  },
  async openMedia(): Promise<OpenMediaResult> {
    return { opened: true };
  },
};

export const devSettingsClient: SettingsClient = {
  async get(): Promise<AppSettings> {
    return { ...devSettingsStore };
  },
  async update(patch): Promise<AppSettings> {
    devSettingsStore = {
      ...devSettingsStore,
      ...patch,
      providerTieBreakOrder:
        patch.providerTieBreakOrder ?? devSettingsStore.providerTieBreakOrder,
      theme: patch.theme ?? devSettingsStore.theme,
      language: patch.language ?? devSettingsStore.language,
      localSpeechEnabled:
        patch.localSpeechEnabled ?? devSettingsStore.localSpeechEnabled,
      globalShortcut: patch.globalShortcut ?? devSettingsStore.globalShortcut,
    };
    return { ...devSettingsStore };
  },
};

export const devSpeechModelClient: SpeechModelClient = {
  async status() {
    return { ...devSpeechStatus };
  },
  async install() {
    devSpeechStatus = {
      state: 'installed',
      modelId: 'whisper-base-multilingual-v1',
      label: 'Multilingual base (74 MB)',
      downloadedBytes: 77594624,
      totalBytes: 77594624,
      errorCode: null,
    };
    return { accepted: true, modelId: 'whisper-base-multilingual-v1' };
  },
  async cancel() {
    return { cancelled: true };
  },
  async remove() {
    devSpeechStatus = {
      state: 'not_installed',
      modelId: null,
      label: 'Multilingual base',
      downloadedBytes: null,
      totalBytes: null,
      errorCode: null,
    };
    return { removed: true };
  },
  async subscribe() {
    return () => {};
  },
};

const devLiveSources: ContextSourceOption[] = [
  {
    sourceId: 'src-1',
    kind: 'integrated_terminal',
    provider: 'vscode',
    applicationName: 'VS Code',
    label: 'lyn',
    context: sampleContexts[0],
    branchName: 'feature/ui-clean-modern-polish',
    isForeground: true,
  },
];

export const devCaptureClient: CaptureClient = {
  async getActiveSession() {
    return { ...devSession };
  },
  async listContexts() {
    return sampleContexts;
  },
  async listContextSources() {
    return {
      liveSources: devLiveSources,
      savedContexts: sampleContexts,
    };
  },
  async createStandaloneContext(name: string) {
    const created = {
      id: `context-${Date.now()}`,
      kind: 'standalone' as const,
      name,
    };
    sampleContexts.push(created);
    return created;
  },
  async selectContext(_sessionId: string, contextId: string) {
    const ctx =
      sampleContexts.find((c) => c.id === contextId) ?? sampleContexts[0];
    devSession = {
      ...devSession,
      contextResolution: {
        state: 'resolved',
        candidate: {
          context: ctx,
          branchName: null,
          provider: 'manual',
          requiresConfirmation: false,
        },
        selection: { kind: 'saved_context', contextId },
      },
    };
    return { ...devSession };
  },
  async selectLiveSource(_sessionId: string, sourceId: string) {
    const src =
      devLiveSources.find((s) => s.sourceId === sourceId) ?? devLiveSources[0];
    devSession = {
      ...devSession,
      contextResolution: {
        state: 'resolved',
        candidate: {
          context: src.context,
          branchName: src.branchName,
          provider: src.provider,
          requiresConfirmation: false,
        },
        selection: { kind: 'live_source', sourceId },
      },
    };
    return { ...devSession };
  },
  async saveText() {
    return {
      captureId: `cap-${Date.now()}`,
      capturedAt: new Date().toISOString(),
      enrichmentScheduled: false,
    };
  },
  async stageClipboardImage() {
    return {
      stagedMediaId: 'staged-1',
      kind: 'image',
      previewUri:
        'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="400" height="200"%3E%3Crect width="400" height="200" fill="%23242a24"/%3E%3C/svg%3E',
      mimeType: 'image/png',
      byteSize: 1024,
      durationMs: null,
      widthPx: 400,
      heightPx: 200,
    };
  },
  async discardStagedMedia() {
    devSession = { ...devSession, stagedMedia: null };
    return { ...devSession };
  },
  async saveImage() {
    return {
      captureId: `cap-img-${Date.now()}`,
      capturedAt: new Date().toISOString(),
      enrichmentScheduled: false,
    };
  },
  async startAudioRecording() {
    devSession = {
      ...devSession,
      recordingState: { state: 'recording', elapsedMs: 0 },
    };
    return { state: 'recording', elapsedMs: 0 };
  },
  async stopAudioRecording() {
    devSession = { ...devSession, recordingState: { state: 'idle' } };
    return {
      stagedMediaId: 'staged-audio-1',
      kind: 'audio',
      previewUri: '',
      mimeType: 'audio/wav',
      byteSize: 3200,
      durationMs: 4000,
      widthPx: null,
      heightPx: null,
    };
  },
  async saveAudio() {
    return {
      captureId: `cap-aud-${Date.now()}`,
      capturedAt: new Date().toISOString(),
      enrichmentScheduled: false,
    };
  },
  async playStagedAudio() {
    return { playing: true, durationMs: 4000 };
  },
  async stopAudioPlayback() {
    return { playing: false, durationMs: null };
  },
  async setPopupLayout(layout) {
    return { layout };
  },
  async cancel() {
    return { cancelled: true };
  },
  async dismissPopup() {
    return { dismissed: true, focusRestored: true };
  },
  async onSessionReady() {
    return () => {};
  },
  async onContextSourcesChanged() {
    return () => {};
  },
};

let devIntegrations: IntegrationStatus[] = [
  {
    id: 'cursor',
    name: 'Cursor IDE',
    description:
      'Reports the focused Cursor workspace folder to Lyn on capture.',
    detected: true,
    installed: false,
    details: 'Cursor detected on this system',
  },
  {
    id: 'vscode',
    name: 'Visual Studio Code',
    description:
      'Reports the focused VS Code workspace folder to Lyn on capture.',
    detected: true,
    installed: true,
    details: 'Extension active in ~/.vscode/extensions/',
  },
  {
    id: 'browser',
    name: 'Web Browser (Chrome, Brave, Edge, Firefox)',
    description:
      'Correlates active localhost development tabs with your repository context.',
    detected: true,
    installed: false,
    details: 'Supported web browser detected',
  },
  {
    id: 'kitty',
    name: 'Kitty Terminal',
    description:
      'Monitors exact focused terminal pane without inspecting commands or output.',
    detected: false,
    installed: false,
    details: null,
  },
  {
    id: 'shell',
    name: 'Terminal Shell (Bash / Zsh)',
    description:
      'Associates GNOME Terminal and external shells with current git repository.',
    detected: true,
    installed: false,
    details: 'Available for Bash and Zsh',
  },
];

export const devIntegrationClient: IntegrationClient = {
  async list() {
    return [...devIntegrations];
  },
  async install(input) {
    devIntegrations = devIntegrations.map((item) =>
      item.id === input.id ? { ...item, installed: true } : item,
    );
    return {
      id: input.id,
      success: true,
      message: `${input.id} integration installed successfully!`,
      installed: true,
    };
  },
};
