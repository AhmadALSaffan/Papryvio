import { useAppStore } from '../../app/store';
import { useT } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import type { DocumentView } from '../../shared/types/generated/DocumentView';
import { docStatus, failureKey, hasText, isProcessing } from '../library/status';
import { JobControls, ProgressRow } from './JobControls';

/** Collection overview: real counts, live processing list and papers needing attention. */
export function Overview() {
  const t = useT();
  const docs = useAppStore((s) => s.documents);
  const total = useAppStore((s) => s.documentsTotal);
  const collection = useAppStore((s) => s.collections.find((c) => c.id === s.activeCollectionId) ?? null);
  const select = useAppStore((s) => s.selectDocument);
  const processing = docs.filter(isProcessing);
  const attention = docs.filter((d) => d.state === 'failed' || d.state === 'needs_ocr');
  const ready = docs.filter(hasText).length;

  return (
    <div className="overview">
      <header className="overview-head">
        <h1 dir="auto">{collection ? t('overview.title', { name: collection.name }) : t('overview.allTitle')}</h1>
        <span className="t-meta">{t('overview.counts', { total, ready, processing: processing.length })}</span>
      </header>

      {processing.length > 0 && (
        <section className="section" aria-labelledby="processing-h">
          <div className="section-intro">
            <h2 id="processing-h">{t('overview.processingTitle', { count: processing.length })}</h2>
            <p className="t-body small">{t('overview.processingBody')}</p>
          </div>
          <div className="proc-list">
            {processing.map((d) => (
              <div className="proc-row" key={d.id}>
                <button className="proc-title link-like" onClick={() => select(d.id)} dir="auto">
                  {d.title}
                </button>
                <ProgressRow doc={d} />
                <JobControls doc={d} compact />
              </div>
            ))}
          </div>
        </section>
      )}

      {attention.length > 0 && (
        <section className="section" aria-labelledby="attention-h">
          <h2 id="attention-h" className="t-label">
            {t('overview.attention')}
          </h2>
          <ul className="attention-list">
            {attention.map((d) => (
              <AttentionRow key={d.id} doc={d} onOpen={() => select(d.id)} />
            ))}
          </ul>
        </section>
      )}

      <p className="hint">
        <Icon name="info" size={14} />
        {t('overview.pickPaper')}
      </p>
    </div>
  );
}

function AttentionRow({ doc, onOpen }: { doc: DocumentView; onOpen: () => void }) {
  const t = useT();
  const s = docStatus(doc);
  return (
    <li>
      <button className="attention-row" onClick={onOpen}>
        <span className="t-paper-title" dir="auto">
          {doc.title}
        </span>
        <span className={`status ${s.kind}`}>
          <Icon name={s.kind === 'warn' ? 'warn' : 'error'} size={12} strokeWidth={1.6} />
          {doc.state === 'failed' ? t(failureKey(doc)) : t(s.label)}
        </span>
      </button>
    </li>
  );
}
