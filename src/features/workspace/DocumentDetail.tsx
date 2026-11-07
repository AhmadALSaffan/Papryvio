import { useEffect, useState } from 'react';
import { useAppStore } from '../../app/store';
import { useRagStore } from '../../app/ragStore';
import { api } from '../../shared/ipc/commands';
import { formatBytes, formatDate, formatNumber, useLocale, useT, type MessageKey } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import type { DocumentView } from '../../shared/types/generated/DocumentView';
import { failureKey, hasText, isProcessing } from '../library/status';
import { JobControls, ProgressRow } from './JobControls';

export function DocumentDetail({ doc }: { doc: DocumentView }) {
  const t = useT();
  const locale = useLocale();
  const openReader = useAppStore((s) => s.openReader);
  const reader = useAppStore((s) => s.reader);
  const [editing, setEditing] = useState(false);
  useEffect(() => setEditing(false), [doc.id]);

  const meta: string[] = [];
  if (doc.authors) meta.push(doc.authors);
  if (doc.year) meta.push(String(doc.year));
  if (doc.pageCount) meta.push(t('doc.pages', { count: doc.pageCount }));
  meta.push(t('doc.importedOn', { date: formatDate(locale, doc.createdAt) }));
  const canRead = doc.state !== 'deleting' && doc.state !== 'deleted';

  return (
    <article className="detail" aria-labelledby="doc-title">
      <header className="detail-head">
        <h1 id="doc-title" className="detail-title" dir="auto">
          {doc.title}
        </h1>
        <span className="t-meta" dir="auto">
          {meta.join(' · ')}
        </span>
        <span className="origin-note">{t(`doc.origin.${doc.titleOrigin}` as MessageKey)}</span>
      </header>

      <div className="detail-actions">
        {doc.lexicalReady && (
          <button className="btn btn-primary" onClick={() => askAbout(doc.id)}>
            <Icon name="chat" size={14} />
            {t('doc.askAbout')}
          </button>
        )}
        <button
          className={doc.lexicalReady ? 'btn' : 'btn btn-primary'}
          onClick={() => openReader(doc.id, reader?.documentId === doc.id ? reader.page : 1)}
          disabled={!canRead}
        >
          <Icon name="eye" size={14} />
          {t('doc.open')}
        </button>
        <button className="btn" onClick={() => setEditing((e) => !e)} aria-expanded={editing}>
          <Icon name="edit" size={13} />
          {t('doc.edit')}
        </button>
      </div>

      {editing && <MetadataForm doc={doc} onDone={() => setEditing(false)} />}

      <StatusBlock doc={doc} />

      {canRead && doc.state !== 'needs_ocr' && doc.state !== 'failed' && <NextSteps doc={doc} />}

      <dl className="facts">
        <dt>{t('doc.fileName')}</dt>
        <dd dir="auto" className="selectable">
          {doc.originalFileName}
        </dd>
        <dt>{t('doc.size')}</dt>
        <dd>{formatBytes(locale, doc.fileSize)}</dd>
        {doc.pageCount !== null && (
          <>
            <dt>{t('reader.page')}</dt>
            <dd>{formatNumber(locale, doc.pageCount)}</dd>
          </>
        )}
      </dl>

      <CollectionActions doc={doc} />
    </article>
  );
}

/** Scopes questions to one paper and moves focus to the question box. */
function askAbout(id: string) {
  useRagStore.getState().setScope({ kind: 'documents', ids: [id] });
  useRagStore.getState().newQuestion();
}

/** The three things needed before asking, with the next one to do as a button. */
function NextSteps({ doc }: { doc: DocumentView }) {
  const t = useT();
  const models = useRagStore((s) => s.models);
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen);
  const provider = models?.generation ? models.providers.find((p) => p.id === models.generation?.providerId) : undefined;
  const steps = [
    { done: doc.lexicalReady, label: doc.lexicalReady ? t('steps.paperReady') : t('steps.paperWaiting') },
    {
      done: !!models?.generation,
      label: models?.generation ? t('steps.modelReady', { name: `${provider?.label ?? ''} · ${models.generation.model}` }) : t('steps.modelMissing'),
      action: !models?.generation ? <button className="btn btn-sm" onClick={() => setSettingsOpen(true)}>{t('steps.chooseModel')}</button> : null,
    },
    {
      done: false,
      label: t('steps.ask'),
      action: doc.lexicalReady ? <button className="btn btn-sm btn-primary" onClick={() => askAbout(doc.id)}>{t('doc.askAbout')}</button> : null,
    },
  ];
  return (
    <section className="next-steps" aria-labelledby="next-steps-title">
      <h2 id="next-steps-title">{t('steps.title')}</h2>
      <ol>
        {steps.map((st, i) => (
          <li key={i} className={st.done ? 'done' : ''}>
            <span className="step-mark" aria-hidden>
              {st.done ? <Icon name="check" size={12} strokeWidth={2} /> : i + 1}
            </span>
            <span className="step-label" dir="auto">
              {st.label}
            </span>
            {st.action}
          </li>
        ))}
      </ol>
      <p className="t-meta">{t('steps.hint')}</p>
    </section>
  );
}

function StatusBlock({ doc }: { doc: DocumentView }) {
  const t = useT();
  const startImport = useAppStore((s) => s.startImport);
  const [keptForReading, setKept] = useState(false);

  if (isProcessing(doc)) {
    return (
      <section className="state-block">
        <h2>{t('doc.processingTitle')}</h2>
        <ProgressRow doc={doc} />
        {doc.job?.state === 'paused' && <p className="t-meta">{t('job.pausedNote')}</p>}
        <JobControls doc={doc} />
      </section>
    );
  }
  if (doc.state === 'indexing') {
    return (
      <section className="state-block">
        <h2>{t('doc.indexingTitle')}</h2>
        <span className="progress on-paper indeterminate" role="progressbar" aria-label={t('status.indexing')}>
          <span />
        </span>
      </section>
    );
  }
  if (hasText(doc)) {
    return (
      <section className="state-block ok">
        <h2>
          <Icon name="check" size={14} strokeWidth={1.8} />
          {t(doc.state === 'ready' ? 'doc.readyTitle' : 'doc.textReadyTitle', { pages: doc.textPageCount, total: doc.pageCount ?? doc.textPageCount })}
        </h2>
        <p className="t-body small">
          {doc.state === 'ready' ? t('doc.readyBody', { pages: doc.textPageCount, total: doc.pageCount ?? doc.textPageCount }) : t('doc.textReadyBody')}{' '}
          {doc.state === 'ready' && (doc.semanticReady ? t('doc.semanticReady') : t('doc.semanticPending'))}
        </p>
        {doc.lowTextPageCount > 0 && <p className="t-meta">{t('doc.lowTextNote', { count: doc.lowTextPageCount })}</p>}
      </section>
    );
  }
  if (doc.state === 'needs_ocr') {
    if (keptForReading) {
      return (
        <section className="state-block warn compact">
          <span className="status warn">
            <Icon name="warn" size={12} strokeWidth={1.6} />
            {t('status.needsOcr')}
          </span>
        </section>
      );
    }
    return (
      <section className="state-block warn" role="status">
        <div className="warn-box">
          <Icon name="warn" size={18} strokeWidth={1.1} />
          <div>
            <h2>{t('doc.scannedTitle')}</h2>
            <p>{t('doc.scannedBody', { total: doc.pageCount ?? 0 })}</p>
          </div>
        </div>
        <div className="row-actions">
          <button className="btn btn-primary btn-lg" disabled aria-describedby="ocr-reason">
            {t('doc.ocr')}
          </button>
          <span id="ocr-reason" className="t-meta">
            {t('doc.ocrUnavailable')}
          </span>
        </div>
        <div className="row-actions">
          <button className="btn" onClick={() => void startImport()}>
            {t('doc.replace')}
          </button>
          <button className="btn" onClick={() => setKept(true)}>
            {t('doc.keepExcluded')}
          </button>
        </div>
      </section>
    );
  }
  if (doc.state === 'failed') {
    return (
      <section className="state-block error" role="status">
        <div className="warn-box error">
          <Icon name="error" size={18} strokeWidth={1.2} />
          <div>
            <h2>{t('doc.failedTitle')}</h2>
            <p>{t(failureKey(doc))}</p>
          </div>
        </div>
        <div className="row-actions">
          <JobControls doc={doc} />
          {(doc.failureKind === 'corrupt' || doc.failureKind === 'password_protected') && (
            <button className="btn" onClick={() => void startImport()}>
              {t('doc.replace')}
            </button>
          )}
        </div>
      </section>
    );
  }
  return null;
}

function MetadataForm({ doc, onDone }: { doc: DocumentView; onDone: () => void }) {
  const t = useT();
  const fail = useAppStore((s) => s.fail);
  const refresh = useAppStore((s) => s.refreshDocuments);
  const [title, setTitle] = useState(doc.title);
  const [authors, setAuthors] = useState(doc.authors ?? '');
  const [year, setYear] = useState(doc.year ? String(doc.year) : '');
  const [saving, setSaving] = useState(false);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setSaving(true);
    try {
      const y = year.trim();
      await api.updateDocumentMetadata(doc.id, {
        ...(title.trim() !== doc.title ? { title: title.trim() } : {}),
        ...(authors.trim() !== (doc.authors ?? '') ? { authors: authors.trim() } : {}),
        ...(y && Number(y) !== doc.year ? { year: Number(y) } : {}),
        clearYear: !y && doc.year !== null,
      });
      await refresh();
      onDone();
    } catch (err) {
      fail(err);
    } finally {
      setSaving(false);
    }
  }

  return (
    <form className="meta-form" onSubmit={(e) => void save(e)}>
      <label>
        <span>{t('doc.title')}</span>
        <input className="field" value={title} onChange={(e) => setTitle(e.target.value)} required maxLength={400} dir="auto" data-autofocus />
      </label>
      <label>
        <span>{t('doc.authors')}</span>
        <input className="field" value={authors} onChange={(e) => setAuthors(e.target.value)} maxLength={300} dir="auto" />
      </label>
      <label className="year">
        <span>{t('doc.year')}</span>
        <input className="field" value={year} onChange={(e) => setYear(e.target.value.replace(/[^0-9]/g, '').slice(0, 4))} inputMode="numeric" />
      </label>
      <div className="row-actions">
        <button className="btn btn-primary" type="submit" disabled={saving || !title.trim()}>
          {t('doc.save')}
        </button>
        <button className="btn" type="button" onClick={onDone}>
          {t('doc.cancelEdit')}
        </button>
      </div>
    </form>
  );
}

function CollectionActions({ doc }: { doc: DocumentView }) {
  const t = useT();
  const activeId = useAppStore((s) => s.activeCollectionId);
  const collections = useAppStore((s) => s.collections);
  const fail = useAppStore((s) => s.fail);
  const refresh = useAppStore((s) => s.refreshDocuments);
  const refreshCollections = useAppStore((s) => s.refreshCollections);
  const setDialog = useAppStore((s) => s.setDialog);
  const others = collections.filter((c) => !doc.collectionIds.includes(c.id));

  async function remove() {
    if (!activeId) return;
    try {
      await api.removeFromCollection(activeId, doc.id);
      await Promise.all([refresh(), refreshCollections()]);
    } catch (e) {
      fail(e);
    }
  }
  async function add(id: string) {
    try {
      await api.addToCollection(id, doc.id);
      await Promise.all([refresh(), refreshCollections()]);
    } catch (e) {
      fail(e);
    }
  }

  return (
    <div className="detail-footer">
      {others.length > 0 && (
        <label className="add-to">
          <span className="sr-only">{t('doc.addToCollection')}</span>
          <select
            className="field"
            value=""
            onChange={(e) => {
              if (e.target.value) void add(e.target.value);
            }}
          >
            <option value="">{t('doc.addToCollection')}</option>
            {others.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {activeId && doc.collectionIds.includes(activeId) && (
        <button className="btn" onClick={() => void remove()}>
          {t('doc.removeFromCollection')}
        </button>
      )}
      <button className="btn btn-danger" onClick={() => setDialog({ kind: 'deleteDocument', id: doc.id, title: doc.title })}>
        <Icon name="trash" size={13} />
        {t('doc.delete')}
      </button>
    </div>
  );
}
