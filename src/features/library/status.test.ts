import { describe, expect, it } from 'vitest';
import type { DocumentView } from '../../shared/types/generated/DocumentView';
import type { JobView } from '../../shared/types/generated/JobView';
import { docStatus } from './status';

const base: DocumentView = {
  id: 'd',
  title: 'T',
  titleOrigin: 'file_name',
  authors: null,
  authorsOrigin: null,
  year: null,
  yearOrigin: null,
  pageCount: null,
  state: 'imported',
  failureKind: null,
  textPageCount: 0,
  lowTextPageCount: 0,
  lexicalReady: false,
  semanticReady: false,
  originalFileName: 't.pdf',
  fileSize: 1,
  createdAt: 0,
  collectionIds: [],
  job: null,
};

const job = (over: Partial<JobView>): JobView => ({
  id: 'j',
  kind: 'extract',
  entityId: 'd',
  attemptId: 'a',
  state: 'running',
  stage: 'reading_pages',
  completedUnits: 0,
  totalUnits: null,
  errorKind: null,
  createdAt: 0,
  updatedAt: 0,
  ...over,
});

describe('docStatus', () => {
  it('is queued without a running job', () => {
    expect(docStatus(base).label).toBe('status.queued');
  });
  it('is indeterminate until the page count is known', () => {
    const s = docStatus({ ...base, state: 'extracting', job: job({ stage: 'opening' }) });
    expect(s.kind).toBe('progress');
    expect(s.fraction).toBeNull();
  });
  it('computes progress from completed pages', () => {
    const s = docStatus({ ...base, state: 'extracting', job: job({ completedUnits: 3, totalUnits: 12 }) });
    expect(s.fraction).toBe(0.25);
  });
  it('flags scans and failures', () => {
    expect(docStatus({ ...base, state: 'needs_ocr' }).kind).toBe('warn');
    expect(docStatus({ ...base, state: 'failed' }).kind).toBe('error');
    expect(docStatus({ ...base, state: 'text_ready' }).kind).toBe('ok');
    expect(docStatus({ ...base, state: 'ready' }).label).toBe('status.ready');
    expect(docStatus({ ...base, state: 'indexing' }).fraction).toBeNull();
  });
});
