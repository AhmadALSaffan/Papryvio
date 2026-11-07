// State for models, conversations, evidence navigation, passage search and comparison.
// As with the library store, Rust holds the truth; this keeps fetched snapshots.
import { create } from 'zustand';
import { api } from '../shared/ipc/commands';
import { on } from '../shared/ipc/events';
import type { ComparisonRow } from '../shared/types/generated/ComparisonRow';
import type { ComparisonView } from '../shared/types/generated/ComparisonView';
import type { ConversationSummary } from '../shared/types/generated/ConversationSummary';
import type { ConversationView } from '../shared/types/generated/ConversationView';
import type { EvidenceLocation } from '../shared/types/generated/EvidenceLocation';
import type { EvidenceTarget } from '../shared/types/generated/EvidenceTarget';
import type { ModelSettingsView } from '../shared/types/generated/ModelSettingsView';
import type { ScopeInput } from '../shared/types/generated/ScopeInput';
import type { SearchResults } from '../shared/types/generated/SearchResults';
import { useAppStore } from './store';

export interface EvidenceNavItem {
  target: EvidenceTarget;
  number: number;
}

export interface EvidenceState {
  location: EvidenceLocation;
  number: number;
  nav: EvidenceNavItem[];
  index: number;
  /** Which answer (attempt id), search ('search') or comparison cell list owns the navigation. */
  owner: string;
  /** Element to refocus when leaving the evidence (the citation chip). */
  opener: HTMLElement | null;
}

export type ScopeChoice = { kind: 'collection' } | { kind: 'documents'; ids: string[] };

interface RagState {
  models: ModelSettingsView | null;
  conversations: ConversationSummary[];
  conversation: ConversationView | null;
  progress: Record<string, { state: string; chars: number }>;
  scope: ScopeChoice;
  evidence: EvidenceState | null;
  search: { query: string; results: SearchResults } | null;
  searching: boolean;
  asking: boolean;
  draft: string;
  compareSelection: string[];
  compareRows: ComparisonRow[];
  comparison: ComparisonView | null;
}

interface RagActions {
  init(): Promise<void>;
  refreshModels(): Promise<void>;
  refreshConversations(): Promise<void>;
  openConversation(id: string | null): Promise<void>;
  newQuestion(): void;
  scopeInput(): ScopeInput;
  setScope(scope: ScopeChoice): void;
  setDraft(text: string): void;
  ask(question: string): Promise<boolean>;
  retry(messageId: string): Promise<void>;
  cancel(attemptId: string): Promise<void>;
  findPassages(query: string): Promise<void>;
  clearSearch(): void;
  openEvidence(target: EvidenceTarget, number: number, nav: EvidenceNavItem[], opener: HTMLElement | null, owner: string): Promise<void>;
  stepEvidence(delta: 1 | -1): Promise<void>;
  closeEvidence(): void;
  toggleCompare(id: string): void;
  setCompareRows(rows: ComparisonRow[]): void;
  buildComparison(): Promise<void>;
  openComparison(id: string | null): Promise<void>;
}

export const DEFAULT_ROWS: ComparisonRow[] = [
  { key: 'method', label: 'Method' },
  { key: 'data', label: 'Data' },
  { key: 'metric', label: 'Metric' },
  { key: 'result', label: 'Result' },
  { key: 'limitations', label: 'Limitations' },
];

let installed = false;
let compareTimer: ReturnType<typeof setTimeout> | null = null;

export const useRagStore = create<RagState & RagActions>((set, get) => ({
  models: null,
  conversations: [],
  conversation: null,
  progress: {},
  scope: { kind: 'collection' },
  evidence: null,
  search: null,
  searching: false,
  asking: false,
  draft: '',
  compareSelection: [],
  compareRows: DEFAULT_ROWS,
  comparison: null,

  async init() {
    if (!installed) {
      installed = true;
      await Promise.all([
        on('generation_progress', (p) => {
          set((s) => ({ progress: { ...s.progress, [p.attemptId]: { state: p.state, chars: p.chars } } }));
        }),
        on('generation_finished', (p) => {
          set((s) => {
            const progress = { ...s.progress };
            delete progress[p.attemptId];
            return { progress };
          });
          if (get().conversation?.id === p.conversationId) void get().openConversation(p.conversationId);
          void get().refreshConversations();
          void useAppStore.getState().refreshOnboarding();
        }),
        on('comparison_updated', (p) => {
          if (get().comparison?.id !== p.comparisonId) return;
          if (compareTimer) clearTimeout(compareTimer);
          compareTimer = setTimeout(() => void get().openComparison(p.comparisonId), 150);
        }),
        on('models_changed', () => void get().refreshModels()),
      ]);
    }
    await Promise.all([get().refreshModels(), get().refreshConversations()]);
  },

  async refreshModels() {
    try {
      set({ models: await api.modelSettings() });
    } catch (e) {
      useAppStore.getState().fail(e);
    }
  },

  async refreshConversations() {
    try {
      set({ conversations: await api.conversations() });
    } catch {
      /* non-critical list */
    }
  },

  async openConversation(id) {
    if (!id) {
      set({ conversation: null });
      return;
    }
    try {
      const conversation = await api.conversation(id);
      set({ conversation, search: null });
      useAppStore.setState({ center: 'conversation', tab: 'ask' });
    } catch (e) {
      useAppStore.getState().fail(e);
    }
  },

  newQuestion() {
    set({ conversation: null, search: null, draft: '' });
    useAppStore.setState({ center: 'conversation', tab: 'ask', selectedDocumentId: null });
    requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>('#composer-input')?.focus());
  },

  scopeInput() {
    const scope = get().scope;
    if (scope.kind === 'documents' && scope.ids.length > 0) return { documentIds: scope.ids };
    const collectionId = useAppStore.getState().activeCollectionId;
    return collectionId ? { collectionId } : {};
  },

  setScope(scope) {
    set({ scope });
  },

  setDraft(draft) {
    set({ draft });
  },

  async ask(question) {
    const q = question.trim();
    if (!q || get().asking) return false;
    set({ asking: true });
    try {
      // The page open in the reader goes along, so "what's on this page?" has evidence.
      const reader = useAppStore.getState().reader;
      const scope = reader
        ? { ...get().scopeInput(), viewing: { documentId: reader.documentId, pageIndex: Math.max(0, reader.page - 1) } }
        : get().scopeInput();
      const started = await api.ask(q, scope, get().conversation?.id ?? null);
      set({ draft: '', search: null, progress: { ...get().progress, [started.attemptId]: { state: 'retrieving', chars: 0 } } });
      await get().openConversation(started.conversationId);
      void get().refreshConversations();
      return true;
    } catch (e) {
      useAppStore.getState().fail(e);
      return false;
    } finally {
      set({ asking: false });
    }
  },

  async retry(messageId) {
    try {
      const attempt = await api.retryAnswer(messageId);
      set({ progress: { ...get().progress, [attempt]: { state: 'retrieving', chars: 0 } } });
      const c = get().conversation;
      if (c) await get().openConversation(c.id);
    } catch (e) {
      useAppStore.getState().fail(e);
    }
  },

  async cancel(attemptId) {
    try {
      await api.cancelGeneration(attemptId);
    } catch (e) {
      useAppStore.getState().fail(e);
    }
  },

  async findPassages(query) {
    const q = query.trim();
    if (!q) return;
    set({ searching: true });
    try {
      const results = await api.search(q, get().scopeInput(), 'hybrid', 20);
      set({ search: { query: q, results } });
      useAppStore.setState({ center: 'conversation', tab: 'ask' });
    } catch (e) {
      useAppStore.getState().fail(e);
    } finally {
      set({ searching: false });
    }
  },

  clearSearch() {
    set({ search: null });
  },

  async openEvidence(target, number, nav, opener, owner) {
    try {
      const location = await api.openEvidence(target);
      const index = Math.max(0, nav.findIndex((n) => n.number === number && JSON.stringify(n.target) === JSON.stringify(target)));
      set({ evidence: { location, number, nav, index, owner, opener: opener ?? get().evidence?.opener ?? null } });
      useAppStore.getState().openReader(location.documentId, location.pageIndex + 1, { keepCenter: true });
      void useAppStore.getState().refreshOnboarding();
    } catch (e) {
      useAppStore.getState().fail(e);
    }
  },

  async stepEvidence(delta) {
    const ev = get().evidence;
    if (!ev) return;
    const next = ev.nav[ev.index + delta];
    if (next) await get().openEvidence(next.target, next.number, ev.nav, ev.opener, ev.owner);
  },

  closeEvidence() {
    const opener = get().evidence?.opener;
    set({ evidence: null });
    useAppStore.getState().closeReader();
    requestAnimationFrame(() => opener?.isConnected && opener.focus());
  },

  toggleCompare(id) {
    const sel = get().compareSelection;
    if (sel.includes(id)) set({ compareSelection: sel.filter((x) => x !== id) });
    else if (sel.length < 2) set({ compareSelection: [...sel, id] });
  },

  setCompareRows(rows) {
    set({ compareRows: rows });
  },

  async buildComparison() {
    const { compareSelection, compareRows } = get();
    if (compareSelection.length !== 2 || compareRows.length === 0) return;
    try {
      const id = await api.buildComparison(compareSelection, compareRows);
      await get().openComparison(id);
    } catch (e) {
      useAppStore.getState().fail(e);
    }
  },

  async openComparison(id) {
    if (!id) {
      set({ comparison: null });
      return;
    }
    try {
      set({ comparison: await api.comparison(id) });
    } catch (e) {
      useAppStore.getState().fail(e);
    }
  },
}));
