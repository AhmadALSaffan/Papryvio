// UI state. Rust and SQLite remain the source of truth: this store holds snapshots
// fetched through commands and patched by events, and refetches after changes.
import { create } from 'zustand';
import { api } from '../shared/ipc/commands';
import { toUiError, type UiError } from '../shared/ipc/errors';
import { on } from '../shared/ipc/events';
import type { Locale, MessageKey } from '../shared/i18n';
import { translate } from '../shared/i18n';
import type { CollectionView } from '../shared/types/generated/CollectionView';
import type { DocumentView } from '../shared/types/generated/DocumentView';
import type { ImportReport } from '../shared/types/generated/ImportReport';
import type { ImportSelection } from '../shared/types/generated/ImportSelection';
import type { JobView } from '../shared/types/generated/JobView';
import type { OnboardingView } from '../shared/types/generated/OnboardingView';
import type { SettingsView } from '../shared/types/generated/SettingsView';
import type { StorageView } from '../shared/types/generated/StorageView';

export interface Toast {
  id: number;
  kind: 'info' | 'warn' | 'error';
  text: string;
  reference?: string;
}

export type DialogState =
  | { kind: 'createCollection'; thenImport?: ImportSelection | 'pick' }
  | { kind: 'renameCollection'; id: string; name: string }
  | { kind: 'deleteCollection'; id: string; name: string }
  | { kind: 'deleteDocument'; id: string; title: string }
  | { kind: 'pickCollectionForImport'; selection: ImportSelection | 'pick' };

export interface ReaderState {
  documentId: string;
  page: number; // 1-based display page
}

interface State {
  ready: boolean;
  bootError: UiError | null;
  locale: Locale;
  settings: SettingsView | null;
  pdfEngineAvailable: boolean;
  storage: StorageView | null;
  collections: CollectionView[];
  activeCollectionId: string | null;
  documents: DocumentView[];
  documentsTotal: number;
  filter: string;
  jobs: Record<string, JobView>;
  selectedDocumentId: string | null;
  reader: ReaderState | null;
  tab: 'ask' | 'compare';
  /** What the middle pane shows on the Ask tab. */
  center: 'home' | 'document' | 'conversation';
  libraryCollapsed: boolean;
  onboarding: OnboardingView | null;
  onboardingOpen: boolean;
  settingsOpen: boolean;
  dialog: DialogState | null;
  dragActive: boolean;
  importing: boolean;
  lastImport: ImportReport | null;
  toasts: Toast[];
}

interface Actions {
  init(): Promise<void>;
  t(key: MessageKey, vars?: Record<string, string | number>): string;
  toast(kind: Toast['kind'], text: string, reference?: string): void;
  fail(e: unknown): void;
  dismissToast(id: number): void;
  setLocale(locale: Locale): Promise<void>;
  updateSettings(patch: Parameters<typeof api.updateSettings>[0]): Promise<void>;
  refreshCollections(): Promise<void>;
  refreshDocuments(): Promise<void>;
  refreshOnboarding(): Promise<void>;
  refreshStorage(): Promise<void>;
  selectCollection(id: string | null): Promise<void>;
  setFilter(filter: string): void;
  createCollection(name: string): Promise<CollectionView | null>;
  renameCollection(id: string, name: string): Promise<boolean>;
  deleteCollection(id: string): Promise<void>;
  startImport(): Promise<void>;
  importSelection(selection: ImportSelection | 'pick', collectionId?: string): Promise<ImportReport | null>;
  selectDocument(id: string | null): void;
  openReader(documentId: string, page?: number, opts?: { keepCenter?: boolean }): void;
  closeReader(): void;
  setReaderPage(page: number): void;
  setTab(tab: State['tab']): void;
  toggleLibrary(): void;
  setDialog(d: DialogState | null): void;
  setOnboardingOpen(open: boolean): void;
  setSettingsOpen(open: boolean): void;
  applyJob(job: JobView): void;
}

let toastSeq = 0;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let listenersInstalled = false;

export const useAppStore = create<State & Actions>((set, get) => ({
  ready: false,
  bootError: null,
  locale: 'en',
  settings: null,
  pdfEngineAvailable: true,
  storage: null,
  collections: [],
  activeCollectionId: null,
  documents: [],
  documentsTotal: 0,
  filter: '',
  jobs: {},
  selectedDocumentId: null,
  reader: null,
  tab: 'ask',
  center: 'home',
  libraryCollapsed: false,
  onboarding: null,
  onboardingOpen: false,
  settingsOpen: false,
  dialog: null,
  dragActive: false,
  importing: false,
  lastImport: null,
  toasts: [],

  t: (key, vars) => translate(get().locale, key, vars),

  toast(kind, text, reference) {
    const id = ++toastSeq;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, text, ...(reference ? { reference } : {}) }] }));
    if (kind === 'info') setTimeout(() => get().dismissToast(id), 5000);
  },
  fail(e) {
    const err = toUiError(e);
    get().toast('error', get().t(err.key), err.reference);
  },
  dismissToast(id) {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },

  async init() {
    if (!listenersInstalled) {
      listenersInstalled = true;
      const scheduleRefresh = () => {
        if (refreshTimer) clearTimeout(refreshTimer);
        refreshTimer = setTimeout(() => {
          void get().refreshDocuments();
          void get().refreshCollections();
        }, 150);
      };
      await Promise.all([
        on('job_progress', (job) => get().applyJob(job)),
        on('document_changed', scheduleRefresh),
        on('library_changed', scheduleRefresh),
        on('drag_state', (p) => set({ dragActive: p.active })),
        on('files_dropped', (selection) => {
          set({ dragActive: false });
          void get().importSelection(selection);
        }),
      ]);
    }
    try {
      const boot = await api.bootstrap();
      const locale = (boot.settings.locale === 'ar' ? 'ar' : 'en') as Locale;
      const jobs: Record<string, JobView> = {};
      for (const j of boot.activeJobs) jobs[j.id] = j;
      const onboardingDone = boot.onboarding.dismissed;
      set({
        ready: true,
        locale,
        settings: boot.settings,
        pdfEngineAvailable: boot.pdfEngineAvailable,
        storage: boot.storage,
        collections: boot.collections,
        activeCollectionId: boot.settings.activeCollectionId ?? boot.collections[0]?.id ?? null,
        libraryCollapsed: boot.settings.libraryCollapsed,
        onboarding: boot.onboarding,
        onboardingOpen: !onboardingDone,
        jobs,
      });
      await get().refreshDocuments();
    } catch (e) {
      set({ bootError: toUiError(e) });
    }
  },

  async setLocale(locale) {
    set({ locale });
    await get().updateSettings({ locale });
  },

  async updateSettings(patch) {
    try {
      const settings = await api.updateSettings(patch);
      set({ settings, locale: settings.locale === 'ar' ? 'ar' : 'en' });
    } catch (e) {
      get().fail(e);
    }
  },

  async refreshCollections() {
    try {
      const collections = await api.listCollections();
      const active = get().activeCollectionId;
      set({
        collections,
        activeCollectionId: active && collections.some((c) => c.id === active) ? active : null,
      });
    } catch (e) {
      get().fail(e);
    }
  },

  async refreshDocuments() {
    const { activeCollectionId, filter } = get();
    try {
      const page = await api.listDocuments({
        ...(activeCollectionId ? { collectionId: activeCollectionId } : {}),
        ...(filter.trim() ? { filter: filter.trim() } : {}),
        limit: 500,
      });
      // Ignore a response for a scope the user already left.
      if (get().activeCollectionId !== activeCollectionId || get().filter !== filter) return;
      const jobs = { ...get().jobs };
      for (const d of page.items) if (d.job) jobs[d.job.id] = newer(jobs[d.job.id], d.job);
      const sel = get().selectedDocumentId;
      set({
        documents: page.items.map((d) => (d.job ? { ...d, job: jobs[d.job.id] ?? d.job } : d)),
        documentsTotal: page.total,
        jobs,
        selectedDocumentId: sel && (page.items.some((d) => d.id === sel) || filter) ? sel : null,
      });
      const reader = get().reader;
      if (reader && !page.items.some((d) => d.id === reader.documentId) && !filter) {
        // The paper may have been deleted; confirm before closing the reader.
        api.getDocument(reader.documentId).catch(() => set({ reader: null }));
      }
    } catch (e) {
      get().fail(e);
    }
  },

  async refreshOnboarding() {
    try {
      set({ onboarding: await api.onboarding() });
    } catch (e) {
      get().fail(e);
    }
  },

  async refreshStorage() {
    try {
      set({ storage: await api.storage() });
    } catch (e) {
      get().fail(e);
    }
  },

  async selectCollection(id) {
    set({ activeCollectionId: id, selectedDocumentId: null, filter: '' });
    void get().updateSettings({ activeCollectionId: id ?? '' });
    await get().refreshDocuments();
  },

  setFilter(filter) {
    set({ filter });
    if (refreshTimer) clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => void get().refreshDocuments(), 120);
  },

  async createCollection(name) {
    try {
      const c = await api.createCollection(name);
      await get().refreshCollections();
      await get().selectCollection(c.id);
      return c;
    } catch (e) {
      get().fail(e);
      return null;
    }
  },

  async renameCollection(id, name) {
    try {
      await api.renameCollection(id, name);
      await get().refreshCollections();
      return true;
    } catch (e) {
      get().fail(e);
      return false;
    }
  },

  async deleteCollection(id) {
    try {
      await api.deleteCollection(id);
      await get().refreshCollections();
      if (get().activeCollectionId === id || !get().activeCollectionId) {
        await get().selectCollection(get().collections[0]?.id ?? null);
      }
    } catch (e) {
      get().fail(e);
    }
  },

  async startImport() {
    await get().importSelection('pick');
  },

  async importSelection(selection, collectionId) {
    const { collections, activeCollectionId } = get();
    if (collections.length === 0) {
      set({ dialog: { kind: 'createCollection', thenImport: selection } });
      return null;
    }
    const target = collectionId ?? activeCollectionId;
    if (!target) {
      set({ dialog: { kind: 'pickCollectionForImport', selection } });
      return null;
    }
    let sel = selection;
    if (sel === 'pick') {
      try {
        sel = (await api.chooseImportFiles()) ?? 'pick';
      } catch (e) {
        get().fail(e);
        return null;
      }
      if (sel === 'pick') return null; // dialog cancelled
    }
    set({ importing: true });
    try {
      const report = await api.importDocuments(sel.token, target);
      set({ lastImport: report });
      reportImport(report, get);
      if (get().activeCollectionId !== target) await get().selectCollection(target);
      else await get().refreshDocuments();
      await get().refreshCollections();
      return report;
    } catch (e) {
      get().fail(e);
      return null;
    } finally {
      set({ importing: false });
    }
  },

  selectDocument(id) {
    set({ selectedDocumentId: id, tab: 'ask', center: id ? 'document' : 'home' });
  },

  openReader(documentId, page = 1, opts) {
    if (opts?.keepCenter) set({ reader: { documentId, page } });
    else set({ reader: { documentId, page }, selectedDocumentId: documentId, tab: 'ask', center: 'document' });
  },
  closeReader() {
    set({ reader: null });
  },
  setReaderPage(page) {
    const r = get().reader;
    if (r) set({ reader: { ...r, page } });
  },

  setTab(tab) {
    set({ tab });
  },
  toggleLibrary() {
    const collapsed = !get().libraryCollapsed;
    set({ libraryCollapsed: collapsed });
    void get().updateSettings({ libraryCollapsed: collapsed });
  },
  setDialog(dialog) {
    set({ dialog });
  },
  setOnboardingOpen(open) {
    set({ onboardingOpen: open });
  },
  setSettingsOpen(open) {
    set({ settingsOpen: open });
    if (open) void get().refreshStorage();
  },

  applyJob(job) {
    const prev = get().jobs[job.id];
    const next = newer(prev, job);
    if (next === prev) return;
    set((s) => ({
      jobs: { ...s.jobs, [job.id]: next },
      documents:
        job.kind === 'extract'
          ? s.documents.map((d) => (d.id === job.entityId ? { ...d, job: next } : d))
          : s.documents,
    }));
  },
}));

/** Keeps the most recent snapshot of a job; updates from an older attempt are ignored. */
function newer(prev: JobView | undefined, incoming: JobView): JobView {
  if (!prev) return incoming;
  if (incoming.attemptId !== prev.attemptId) {
    return incoming.updatedAt >= prev.updatedAt ? incoming : prev;
  }
  return incoming.updatedAt >= prev.updatedAt ? incoming : prev;
}

function reportImport(report: ImportReport, get: () => State & Actions) {
  const { t, toast } = get();
  let imported = 0;
  for (const item of report.items) {
    const name = item.fileName;
    switch (item.outcome.type) {
      case 'imported':
        imported++;
        break;
      case 'alreadyInLibrary':
        toast('info', item.outcome.addedToCollection ? t('import.alreadyAdded', { name }) : t('import.already', { name }));
        break;
      case 'rejected':
        toast('warn', t(`import.rejected.${item.outcome.reason}` as MessageKey, { name }));
        break;
    }
  }
  if (imported > 0) toast('info', t('import.done', { count: imported }));
}
