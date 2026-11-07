import { useEffect, useState } from 'react';
import { useAppStore } from '../../app/store';
import { DEFAULT_ROWS, useRagStore, type EvidenceNavItem } from '../../app/ragStore';
import { api } from '../../shared/ipc/commands';
import { formatNumber, useLocale, useT, type MessageKey } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import type { ComparisonCellView } from '../../shared/types/generated/ComparisonCellView';
import type { ComparisonView } from '../../shared/types/generated/ComparisonView';
import './compare.css';

const ROW_LABEL: Record<string, MessageKey> = {
  method: 'compare.row.method',
  data: 'compare.row.data',
  metric: 'compare.row.metric',
  result: 'compare.row.result',
  limitations: 'compare.row.limitations',
  sample_size: 'compare.row.sample_size',
};

export function ComparePane() {
  const comparison = useRagStore((s) => s.comparison);
  return <div className="workspace-body scroll">{comparison ? <Result view={comparison} /> : <Select />}</div>;
}

function Select() {
  const t = useT();
  const docs = useAppStore((s) => s.documents);
  const selection = useRagStore((s) => s.compareSelection);
  const toggle = useRagStore((s) => s.toggleCompare);
  const rows = useRagStore((s) => s.compareRows);
  const setRows = useRagStore((s) => s.setCompareRows);
  const build = useRagStore((s) => s.buildComparison);
  const open = useRagStore((s) => s.openComparison);
  const models = useRagStore((s) => s.models);
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen);
  const [custom, setCustom] = useState('');
  const [recent, setRecent] = useState<ComparisonView[]>([]);
  useEffect(() => {
    api.comparisons().then(setRecent).catch(() => setRecent([]));
  }, []);

  const ready = docs.filter((d) => d.lexicalReady);
  const hasModel = !!models?.generation;
  const allRows = [...DEFAULT_ROWS, { key: 'sample_size', label: 'Sample size' }];
  const canBuild = selection.length === 2 && rows.length > 0 && hasModel;
  const reason = !hasModel ? t('compare.needModel') : ready.length < 2 ? t('compare.needTwo') : selection.length !== 2 ? t('compare.pickTwo') : rows.length === 0 ? t('compare.pickRows') : '';

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter' && canBuild) {
        e.preventDefault();
        void build();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [canBuild, build]);

  const toggleRow = (key: string, label: string) => setRows(rows.some((r) => r.key === key) ? rows.filter((r) => r.key !== key) : [...rows, { key, label }]);

  return (
    <div className="overview cmp-select">
      <header className="overview-head">
        <h1>{t('compare.title')}</h1>
        <span className="t-meta">{t('compare.body')}</span>
      </header>

      <section className="section">
        <h2 className="t-label">{t('compare.papers', { n: selection.length })}</h2>
        {ready.length < 2 && <p className="status warn">{t('compare.needTwoBody', { ready: ready.length })}</p>}
        <ul className="cmp-docs">
          {docs.map((d) => {
            const i = selection.indexOf(d.id);
            const disabled = !d.lexicalReady;
            return (
              <li key={d.id}>
                <label className={`cmp-doc${i >= 0 ? ' on' : ''}${disabled ? ' disabled' : ''}`}>
                  <input type="checkbox" checked={i >= 0} disabled={disabled || (i < 0 && selection.length >= 2)} onChange={() => toggle(d.id)} />
                  <span className="cmp-doc-body">
                    <span className="t-paper-title" dir="auto">
                      {d.title}
                    </span>
                    <span className="t-meta">
                      {i >= 0 ? t(i === 0 ? 'compare.paperA' : 'compare.paperB') : disabled ? t('compare.notReady') : [d.authors, d.year].filter(Boolean).join(' · ')}
                    </span>
                  </span>
                </label>
              </li>
            );
          })}
        </ul>
      </section>

      <section className="section">
        <h2 className="t-label">{t('compare.aspects')}</h2>
        <div className="cmp-rows">
          {allRows.map((r) => (
            <button key={r.key} className={`aspect${rows.some((x) => x.key === r.key) ? ' on' : ''}`} aria-pressed={rows.some((x) => x.key === r.key)} onClick={() => toggleRow(r.key, t(ROW_LABEL[r.key]!))}>
              {t(ROW_LABEL[r.key]!)}
            </button>
          ))}
          {rows
            .filter((r) => !ROW_LABEL[r.key])
            .map((r) => (
              <button key={r.key} className="aspect on" aria-pressed onClick={() => setRows(rows.filter((x) => x.key !== r.key))}>
                {r.label} <Icon name="close" size={10} />
              </button>
            ))}
        </div>
        <form
          className="ob-inline"
          onSubmit={(e) => {
            e.preventDefault();
            const label = custom.trim();
            if (!label) return;
            setRows([...rows, { key: `custom_${Date.now()}`, label }]);
            setCustom('');
          }}
        >
          <input className="field" value={custom} onChange={(e) => setCustom(e.target.value)} placeholder={t('compare.customPlaceholder')} maxLength={80} dir="auto" />
          <button className="btn" type="submit" disabled={!custom.trim()}>
            {t('compare.addAspect')}
          </button>
        </form>
      </section>

      <div className="row-actions">
        <button className="btn btn-primary btn-lg" disabled={!canBuild} onClick={() => void build()} aria-describedby="cmp-reason">
          {t('compare.build')} <span className="kbd">Ctrl+Enter</span>
        </button>
        <span id="cmp-reason" className="t-meta">
          {reason}
        </span>
        {!hasModel && (
          <button className="link-btn" onClick={() => setSettingsOpen(true)}>
            {t('chat.setupModels')}
          </button>
        )}
      </div>
      <p className="t-meta">{t('compare.costNote')}</p>

      {recent.length > 0 && (
        <section className="section">
          <h2 className="t-label">{t('compare.recent')}</h2>
          <ul className="attention-list">
            {recent.map((c) => (
              <li key={c.id}>
                <button className="attention-row" onClick={() => void open(c.id)}>
                  <span className="t-paper-title" dir="auto">
                    {c.documents.map((d) => d.title).join(' ↔ ')}
                  </span>
                  <span className="t-meta">{t('compare.linked', { linked: c.linkedCells, total: c.completedCells })}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function Result({ view }: { view: ComparisonView }) {
  const t = useT();
  const locale = useLocale();
  const open = useRagStore((s) => s.openComparison);
  const toast = useAppStore((s) => s.toast);
  const fail = useAppStore((s) => s.fail);
  const running = view.state === 'running';
  const total = view.cells.length;
  const done = view.completedCells;
  const nav: EvidenceNavItem[] = [];
  for (const r of view.rows)
    for (const d of view.documents) {
      const c = view.cells.find((x) => x.documentId === d.id && x.rowKey === r.key);
      c?.evidence.forEach((e) => nav.push({ target: { type: 'cell', cellId: c.id, number: e.number }, number: nav.length + 1 }));
    }
  async function exportAs(format: 'markdown' | 'csv') {
    try {
      const name = await api.exportComparison(view.id, format);
      if (name) toast('info', t('compare.exported', { name }));
    } catch (e) {
      fail(e);
    }
  }
  return (
    <div className="cmp-result">
      <div className="cmp-head">
        <div>
          <h1>{t('compare.resultTitle')}</h1>
          <span className="t-meta">
            {running ? t('compare.running', { done, total }) : t('compare.linked', { linked: view.linkedCells, total: view.completedCells })}
          </span>
        </div>
        <div className="row-actions">
          {running && (
            <button className="btn" onClick={() => void api.cancelComparison(view.id)}>
              {t('job.cancel')}
            </button>
          )}
          <button className="btn" disabled={running} onClick={() => void exportAs('markdown')}>
            {t('compare.exportMd')}
          </button>
          <button className="btn" disabled={running} onClick={() => void exportAs('csv')}>
            {t('compare.exportCsv')}
          </button>
          <button className="btn" onClick={() => void open(null)}>
            {t('compare.newComparison')}
          </button>
        </div>
      </div>
      {running && (
        <span className="progress on-paper" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={done}>
          <span style={{ width: `${total ? (done / total) * 100 : 0}%` }} />
        </span>
      )}
      <p className="t-meta">{t('compare.linkNote')}</p>
      <table className="cmp-table">
        <thead>
          <tr>
            <th scope="col">{t('compare.aspect')}</th>
            {view.documents.map((d, i) => (
              <th key={d.id} scope="col">
                <span className="cmp-letter">{i === 0 ? 'A' : 'B'}</span>
                <span className="t-paper-title" dir="auto">
                  {d.title}
                </span>
                <span className="t-meta">{[d.authors, d.year].filter(Boolean).join(' · ')}</span>
                {d.deleted && <span className="status warn">{t('chat.sourceDeleted')}</span>}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {view.rows.map((r) => (
            <tr key={r.key}>
              <th scope="row">{ROW_LABEL[r.key] ? t(ROW_LABEL[r.key]!) : r.label}</th>
              {view.documents.map((d) => {
                const cell = view.cells.find((c) => c.documentId === d.id && c.rowKey === r.key);
                return <td key={d.id}>{cell ? <Cell cell={cell} comparisonId={view.id} nav={nav} locale={locale} /> : null}</td>;
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Cell({ cell, comparisonId, nav, locale }: { cell: ComparisonCellView; comparisonId: string; nav: EvidenceNavItem[]; locale: ReturnType<typeof useLocale> }) {
  const t = useT();
  const openEvidence = useRagStore((s) => s.openEvidence);
  const evidence = useRagStore((s) => s.evidence);
  const fail = useAppStore((s) => s.fail);
  if (cell.status === 'pending') {
    return (
      <span className="status neutral">
        <span className="spinner" aria-hidden="true" /> {t('compare.cell.pending')}
      </span>
    );
  }
  if (cell.status === 'evidence_not_found') {
    return (
      <div className="cell-none">
        <span className="status neutral">{t('compare.cell.notFound')}</span>
        {cell.searched && <span className="t-meta">{t('compare.cell.searched', { what: cell.searched })}</span>}
      </div>
    );
  }
  if (cell.status === 'failed') {
    return (
      <div className="cell-none">
        <span className="status error">
          <Icon name="error" size={12} /> {t('compare.cell.failed')}
        </span>
        <button className="link-btn" onClick={() => void api.retryCell(comparisonId, cell.id).catch(fail)}>
          {t('job.retry')}
        </button>
      </div>
    );
  }
  return (
    <div className="cell">
      <p dir="auto" className="selectable">
        {cell.text}
      </p>
      {cell.status === 'partial' && <span className="status warn">{t('compare.cell.partial')}</span>}
      <div>
        {cell.evidence.map((e) => {
          const idx = nav.findIndex((n) => n.target.type === 'cell' && n.target.cellId === cell.id && n.target.number === e.number);
          const active = evidence?.owner === comparisonId && evidence.number === idx + 1;
          return (
            <button
              key={e.number}
              className={`chip${active ? ' active' : ''}`}
              title={t('chat.openPage', { page: e.pageIndex + 1 })}
              onClick={(ev) => void openEvidence({ type: 'cell', cellId: cell.id, number: e.number }, idx + 1, nav, ev.currentTarget, comparisonId)}
            >
              p. {formatNumber(locale, e.pageIndex + 1)}
              <Icon name="open" size={10} />
            </button>
          );
        })}
      </div>
    </div>
  );
}
