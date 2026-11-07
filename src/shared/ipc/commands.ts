// Typed wrappers around Tauri commands. Request/response types are generated from Rust (ts-rs).
import { invoke } from '@tauri-apps/api/core';
import type { BootstrapView } from '../types/generated/BootstrapView';
import type { CollectionView } from '../types/generated/CollectionView';
import type { DocumentListPage } from '../types/generated/DocumentListPage';
import type { DocumentQuery } from '../types/generated/DocumentQuery';
import type { DocumentView } from '../types/generated/DocumentView';
import type { ImportReport } from '../types/generated/ImportReport';
import type { ImportSelection } from '../types/generated/ImportSelection';
import type { IpcError } from '../types/generated/IpcError';
import type { JobView } from '../types/generated/JobView';
import type { MetadataUpdate } from '../types/generated/MetadataUpdate';
import type { OnboardingView } from '../types/generated/OnboardingView';
import type { SettingsPatch } from '../types/generated/SettingsPatch';
import type { SettingsView } from '../types/generated/SettingsView';
import type { StepAction } from '../types/generated/StepAction';
import type { StepKey } from '../types/generated/StepKey';
import type { StorageView } from '../types/generated/StorageView';
import type { AskStarted } from '../types/generated/AskStarted';
import type { ComparisonRow } from '../types/generated/ComparisonRow';
import type { ComparisonView } from '../types/generated/ComparisonView';
import type { ConversationSummary } from '../types/generated/ConversationSummary';
import type { ConversationView } from '../types/generated/ConversationView';
import type { EvidenceLocation } from '../types/generated/EvidenceLocation';
import type { EvidenceTarget } from '../types/generated/EvidenceTarget';
import type { ModelInfo } from '../types/generated/ModelInfo';
import type { ModelSettingsView } from '../types/generated/ModelSettingsView';
import type { ModelTestResult } from '../types/generated/ModelTestResult';
import type { ProviderKind } from '../types/generated/ProviderKind';
import type { ProviderView } from '../types/generated/ProviderView';
import type { RetrievalMode } from '../types/generated/RetrievalMode';
import type { ScopeInput } from '../types/generated/ScopeInput';
import type { SearchResults } from '../types/generated/SearchResults';
import type { SignInStart } from '../types/generated/SignInStart';
import type { SignInStatus } from '../types/generated/SignInStatus';

export const UI_VERSION = '0.1.0';
export const ONBOARDING_FLOW_VERSION = 1;

export function isIpcError(e: unknown): e is IpcError {
  return typeof e === 'object' && e !== null && 'code' in e && 'correlationId' in e;
}

/** Client-generated id that makes write commands safe to repeat (double clicks, retries). */
export function newRequestId(): string {
  return crypto.randomUUID();
}

export const api = {
  bootstrap: () => invoke<BootstrapView>('get_bootstrap_state', { uiVersion: UI_VERSION }),
  getSettings: () => invoke<SettingsView>('get_settings'),
  updateSettings: (patch: SettingsPatch) => invoke<SettingsView>('update_settings', { patch }),
  storage: () => invoke<StorageView>('get_storage_info'),
  diagnostics: () => invoke<string>('get_diagnostics'),

  onboarding: () => invoke<OnboardingView>('get_onboarding'),
  updateOnboarding: (stepKey: StepKey, action: StepAction, relatedEntityId?: string) =>
    invoke<OnboardingView>('update_onboarding', {
      flowVersion: ONBOARDING_FLOW_VERSION,
      stepKey,
      action,
      relatedEntityId: relatedEntityId ?? null,
    }),
  setOnboardingDismissed: (dismissed: boolean) => invoke<OnboardingView>('set_onboarding_dismissed', { dismissed }),
  restartOnboarding: () => invoke<OnboardingView>('restart_onboarding'),

  listCollections: () => invoke<CollectionView[]>('list_collections'),
  createCollection: (name: string, requestId = newRequestId()) =>
    invoke<CollectionView>('create_collection', { requestId, name }),
  renameCollection: (id: string, name: string) => invoke<CollectionView>('rename_collection', { id, name }),
  deleteCollection: (id: string) => invoke<void>('delete_collection', { id }),

  chooseImportFiles: () => invoke<ImportSelection | null>('choose_import_files'),
  importDocuments: (token: string, collectionId: string, requestId = newRequestId()) =>
    invoke<ImportReport>('import_documents', { requestId, token, collectionId }),
  listDocuments: (query: DocumentQuery) => invoke<DocumentListPage>('list_documents', { query }),
  getDocument: (id: string) => invoke<DocumentView>('get_document', { id }),
  updateDocumentMetadata: (id: string, update: MetadataUpdate) =>
    invoke<DocumentView>('update_document_metadata', { id, update }),
  addToCollection: (collectionId: string, documentId: string) =>
    invoke<boolean>('add_to_collection', { collectionId, documentId }),
  removeFromCollection: (collectionId: string, documentId: string) =>
    invoke<boolean>('remove_from_collection', { collectionId, documentId }),
  deleteDocument: (id: string) => invoke<string>('delete_document', { id }),
  retryIngestion: (documentId: string) => invoke<JobView>('retry_ingestion', { documentId }),

  getJob: (jobId: string) => invoke<JobView>('get_job_state', { jobId }),
  pauseJob: (jobId: string) => invoke<JobView>('pause_job', { jobId }),
  resumeJob: (jobId: string) => invoke<JobView>('resume_job', { jobId }),
  cancelJob: (jobId: string) => invoke<JobView>('cancel_job', { jobId }),

  retryJob: (jobId: string) => invoke<JobView>('retry_job', { jobId }),

  modelSettings: () => invoke<ModelSettingsView>('get_model_settings'),
  addOllama: (baseUrl: string) => invoke<ProviderView>('add_ollama_provider', { baseUrl }),
  addApiProvider: (kind: ProviderKind, label: string, baseUrl: string, apiKey: string) =>
    invoke<ProviderView>('add_api_provider', { kind, label, baseUrl, apiKey }),
  startSignIn: (kind: ProviderKind) => invoke<SignInStart>('start_sign_in', { kind }),
  pollSignIn: (flowId: string) => invoke<SignInStatus>('poll_sign_in', { flowId }),
  cancelSignIn: (flowId: string) => invoke<void>('cancel_sign_in', { flowId }),
  removeProvider: (id: string) => invoke<void>('remove_provider', { id }),
  listProviderModels: (providerId: string) => invoke<ModelInfo[]>('list_provider_models', { providerId }),
  testModel: (providerId: string, model: string, purpose: 'generation' | 'embedding') =>
    invoke<ModelTestResult>('test_model', { providerId, model, purpose }),
  setCloudConsent: (providerId: string, allowed: boolean) => invoke<ModelSettingsView>('set_cloud_consent', { providerId, allowed }),
  saveGenerationModel: (providerId: string, model: string) => invoke<ModelSettingsView>('save_generation_model', { providerId, model }),
  saveEmbeddingModel: (providerId: string, model: string) => invoke<ModelSettingsView>('save_embedding_model', { providerId, model }),
  clearEmbeddingModel: () => invoke<ModelSettingsView>('clear_embedding_model'),
  openExternal: (url: string) => invoke<void>('open_external', { url }),

  search: (query: string, scope: ScopeInput, mode: RetrievalMode, limit = 20) =>
    invoke<SearchResults>('search_documents', { query, scope, mode, limit }),
  ask: (question: string, scope: ScopeInput, conversationId: string | null, requestId = newRequestId()) =>
    invoke<AskStarted>('ask_question', { requestId, conversationId, question, scope }),
  cancelGeneration: (attemptId: string) => invoke<boolean>('cancel_generation', { attemptId }),
  retryAnswer: (messageId: string, scope?: ScopeInput, requestId = newRequestId()) =>
    invoke<string>('retry_answer', { requestId, messageId, scope: scope ?? null }),
  conversation: (id: string) => invoke<ConversationView>('get_conversation', { id }),
  conversations: () => invoke<ConversationSummary[]>('list_conversations'),
  deleteConversation: (id: string) => invoke<void>('delete_conversation', { id }),
  openEvidence: (target: EvidenceTarget) => invoke<EvidenceLocation>('open_evidence', { target }),

  buildComparison: (documentIds: string[], rows: ComparisonRow[], requestId = newRequestId()) =>
    invoke<string>('build_comparison', { requestId, documentIds, rows }),
  comparison: (id: string) => invoke<ComparisonView>('get_comparison', { id }),
  comparisons: () => invoke<ComparisonView[]>('list_comparisons'),
  retryCell: (comparisonId: string, cellId: string) => invoke<void>('retry_comparison_cell', { comparisonId, cellId }),
  cancelComparison: (id: string) => invoke<boolean>('cancel_comparison', { id }),
  exportComparison: (id: string, format: 'markdown' | 'csv') => invoke<string | null>('export_comparison', { id, format }),

  /** Raw PDF bytes of the library copy, for the viewer. */
  readDocumentFile: (id: string) => invoke<ArrayBuffer>('read_document_file', { id }),
};
