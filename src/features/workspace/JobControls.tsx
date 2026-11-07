import { useAppStore } from '../../app/store';
import { api } from '../../shared/ipc/commands';
import { formatNumber, useLocale, useT } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import type { DocumentView } from '../../shared/types/generated/DocumentView';
import { docStatus } from '../library/status';

export function ProgressRow({ doc }: { doc: DocumentView }) {
  const t = useT();
  const locale = useLocale();
  const s = docStatus(doc);
  const pct = s.fraction !== null ? Math.round(s.fraction * 100) : null;
  return (
    <div className="progress-row">
      <span
        className={`progress on-paper${pct === null && !s.paused ? ' indeterminate' : ''}`}
        role="progressbar"
        aria-label={t(s.label)}
        aria-valuemin={0}
        aria-valuemax={100}
        {...(pct !== null ? { 'aria-valuenow': pct } : {})}
      >
        <span style={pct !== null ? { width: `${pct}%` } : s.paused ? { width: 0 } : undefined} />
      </span>
      <span className="progress-label">
        {t(s.label)}
        {s.total ? ` · ${t('status.pagesOf', { done: s.done ?? 0, total: s.total })}` : ''}
        {pct !== null ? <span className="mono"> {formatNumber(locale, pct)}%</span> : null}
      </span>
    </div>
  );
}

/** Pause / resume / cancel for an extraction job, and retry for a failed one. */
export function JobControls({ doc, compact = false }: { doc: DocumentView; compact?: boolean }) {
  const t = useT();
  const fail = useAppStore((s) => s.fail);
  const applyJob = useAppStore((s) => s.applyJob);
  const refresh = useAppStore((s) => s.refreshDocuments);
  const job = doc.job;
  const run = async (fn: () => Promise<unknown>) => {
    try {
      const r = await fn();
      if (r && typeof r === 'object' && 'attemptId' in r) applyJob(r as never);
      await refresh();
    } catch (e) {
      fail(e);
    }
  };
  const cls = compact ? 'link-btn' : 'btn';
  const canRetry = doc.state === 'failed';
  if (canRetry) {
    return (
      <div className="job-controls">
        <button className={cls} onClick={() => void run(() => api.retryIngestion(doc.id))}>
          {!compact && <Icon name="play" size={12} />}
          {t('job.retry')}
        </button>
      </div>
    );
  }
  if (!job || !(job.state === 'queued' || job.state === 'running' || job.state === 'paused')) return null;
  return (
    <div className="job-controls">
      {job.state === 'paused' ? (
        <button className={cls} onClick={() => void run(() => api.resumeJob(job.id))}>
          {!compact && <Icon name="play" size={12} />}
          {t('job.resume')}
        </button>
      ) : (
        <button className={cls} onClick={() => void run(() => api.pauseJob(job.id))}>
          {!compact && <Icon name="pause" size={12} />}
          {t('job.pause')}
        </button>
      )}
      <button className={`${cls} muted`} onClick={() => void run(() => api.cancelJob(job.id))}>
        {t('job.cancel')}
      </button>
    </div>
  );
}
