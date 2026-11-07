import type { MessageKey } from '../../shared/i18n';
import type { DocumentView } from '../../shared/types/generated/DocumentView';

export type StatusKind = 'ok' | 'warn' | 'error' | 'neutral' | 'progress';

export interface DocStatus {
  kind: StatusKind;
  label: MessageKey;
  /** 0..1 when the share of completed work is known; null for an indeterminate indicator. */
  fraction: number | null;
  done?: number;
  total?: number | null;
  paused?: boolean;
}

/** Derives what the library shows for a paper from its persisted state and its job snapshot. */
export function docStatus(doc: DocumentView): DocStatus {
  const job = doc.job;
  switch (doc.state) {
    case 'ready':
      return { kind: 'ok', label: 'status.ready', fraction: null };
    case 'text_ready':
      return { kind: 'ok', label: 'status.textReady', fraction: null };
    case 'indexing':
      return { kind: 'progress', label: 'status.indexing', fraction: null };
    case 'needs_ocr':
      return { kind: 'warn', label: 'status.needsOcr', fraction: null };
    case 'failed':
      return { kind: 'error', label: 'status.failed', fraction: null };
    case 'deleting':
    case 'deleted':
      return { kind: 'neutral', label: 'status.deleting', fraction: null };
    case 'imported':
    case 'extracting': {
      if (!job || job.state === 'queued') return { kind: 'neutral', label: 'status.queued', fraction: null };
      const total = job.totalUnits;
      const fraction = total && total > 0 ? Math.min(1, job.completedUnits / total) : null;
      if (job.state === 'paused') {
        return { kind: 'progress', label: 'status.paused', fraction, done: job.completedUnits, total, paused: true };
      }
      if (job.state === 'failed' || job.state === 'cancelled') {
        return { kind: 'error', label: 'status.failed', fraction: null };
      }
      return {
        kind: 'progress',
        label: job.stage === 'opening' ? 'status.opening' : 'status.reading',
        fraction,
        done: job.completedUnits,
        total,
      };
    }
  }
}

export function isProcessing(doc: DocumentView): boolean {
  return doc.state === 'imported' || doc.state === 'extracting';
}

export function hasText(doc: DocumentView): boolean {
  return doc.state === 'text_ready' || doc.state === 'ready' || doc.state === 'indexing';
}

export function failureKey(doc: DocumentView): MessageKey {
  return `failure.${doc.failureKind ?? doc.job?.errorKind ?? 'unknown'}` as MessageKey;
}

export function metaLine(doc: DocumentView, pagesLabel: (n: number) => string): string {
  const parts: string[] = [];
  if (doc.authors) parts.push(doc.authors);
  if (doc.year) parts.push(String(doc.year));
  if (doc.pageCount) parts.push(pagesLabel(doc.pageCount));
  return parts.join(' · ');
}
