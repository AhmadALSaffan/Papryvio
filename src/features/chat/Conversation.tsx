import { useAppStore } from '../../app/store';
import { useRagStore, type EvidenceNavItem } from '../../app/ragStore';
import { formatDate, formatNumber, useLocale, useT, type MessageKey } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import type { AttemptView } from '../../shared/types/generated/AttemptView';
import type { CitationRef } from '../../shared/types/generated/CitationRef';
import type { TurnView } from '../../shared/types/generated/TurnView';
import type { ValidatedClaim } from '../../shared/types/generated/ValidatedClaim';
import './chat.css';

const EXAMPLES: MessageKey[] = ['chat.example1', 'chat.example2', 'chat.example3'];

export function ConversationPane() {
  const conversation = useRagStore((s) => s.conversation);
  const search = useRagStore((s) => s.search);
  if (search) return <PassageResults />;
  if (!conversation || conversation.turns.length === 0) return <StartPanel />;
  return (
    <div className="conv">
      {conversation.turns.map((turn) => (
        <Turn key={turn.messageId} turn={turn} />
      ))}
    </div>
  );
}

function StartPanel() {
  const t = useT();
  const setDraft = useRagStore((s) => s.setDraft);
  return (
    <div className="conv start">
      <h1>{t('chat.startTitle')}</h1>
      <p className="t-body small">{t('chat.startBody')}</p>
      <span className="t-label">{t('chat.examples')}</span>
      <div className="examples">
        {EXAMPLES.map((k) => (
          <button
            key={k}
            className="example"
            onClick={() => {
              setDraft(t(k));
              document.querySelector<HTMLTextAreaElement>('#composer-input')?.focus();
            }}
          >
            {t(k)}
          </button>
        ))}
      </div>
      <p className="t-meta">{t('chat.examplesNote')}</p>
    </div>
  );
}

function Turn({ turn }: { turn: TurnView }) {
  const t = useT();
  const a = turn.attempt;
  return (
    <section className="turn" aria-label={t('chat.youAsked')}>
      <div className="asked">
        <span className="t-label">{t('chat.youAsked')}</span>
        <div className="question selectable" dir="auto">
          {turn.question}
        </div>
      </div>
      {a ? <AttemptBlock attempt={a} turn={turn} /> : null}
    </section>
  );
}

function AttemptBlock({ attempt, turn }: { attempt: AttemptView; turn: TurnView }) {
  const t = useT();
  const locale = useLocale();
  const progress = useRagStore((s) => s.progress[attempt.id]);
  const cancel = useRagStore((s) => s.cancel);
  const retry = useRagStore((s) => s.retry);
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen);
  const running = ['retrieving', 'generating', 'validating'].includes(attempt.state);

  if (running) {
    const state = progress?.state ?? attempt.state;
    const key: MessageKey = state === 'generating' ? 'chat.generating' : state === 'validating' ? 'chat.validating' : 'chat.retrieving';
    return (
      <div className="answer running" aria-live="polite">
        <span className="t-label accent">{t('chat.answer')}</span>
        <p className="status neutral">
          <span className="spinner" aria-hidden="true" />
          {t(key)}
          {state === 'generating' && progress?.chars ? <span className="t-meta"> · {t('chat.chars', { n: progress.chars })}</span> : null}
        </p>
        <p className="t-meta">{t('chat.draftNote')}</p>
        <button className="btn" onClick={() => void cancel(attempt.id)}>
          {t('chat.stop')}
        </button>
      </div>
    );
  }

  const scope = attempt.scope;
  const scopeLine = scope ? t('chat.searched', { searched: scope.searched, requested: scope.requested }) : '';

  if (attempt.state === 'failed' || attempt.state === 'cancelled') {
    const kind = attempt.errorKind ?? (attempt.state === 'cancelled' ? 'cancelled' : 'model_error');
    const msgKey: MessageKey =
      attempt.state === 'cancelled'
        ? 'chat.fail.cancelled'
        : kind === 'no_generation_model'
          ? 'chat.fail.noModel'
          : kind === 'not_allowed'
            ? 'chat.fail.notAllowed'
            : kind === 'provider_unavailable'
              ? 'chat.fail.provider'
              : kind === 'model_timeout'
                ? 'chat.fail.timeout'
                : kind === 'invalid_output'
                  ? 'chat.fail.invalidOutput'
                  : kind === 'interrupted'
                    ? 'chat.fail.interrupted'
                    : 'chat.fail.model';
    return (
      <div className="answer failed" role="status">
        <div className="warn-box error">
          <Icon name="error" size={18} strokeWidth={1.2} />
          <div>
            <h3>{t(attempt.state === 'cancelled' ? 'chat.stopped' : 'chat.failed')}</h3>
            <p>{t(msgKey)}</p>
          </div>
        </div>
        <div className="row-actions">
          {(kind === 'no_generation_model' || kind === 'not_allowed' || kind === 'provider_unavailable') && (
            <button className="btn btn-primary" onClick={() => setSettingsOpen(true)}>
              {t('chat.setupModels')}
            </button>
          )}
          <button className="btn" onClick={() => void retry(turn.messageId)}>
            {t('chat.tryAgain')}
          </button>
          {turn.attemptCount > 1 && <span className="t-meta">{t('chat.attempts', { n: turn.attemptCount })}</span>}
        </div>
      </div>
    );
  }

  const answer = attempt.answer;
  if (attempt.state === 'no_evidence' || !answer || answer.claims.length === 0) {
    return (
      <div className="answer">
        <span className="t-label accent">{t('chat.answer')}</span>
        <div className="warn-box">
          <Icon name="info" size={18} strokeWidth={1.2} />
          <div>
            <h3>{t(attempt.errorKind === 'empty_scope' ? 'chat.noReady' : 'chat.noEvidence')}</h3>
            <p>{t(attempt.errorKind === 'empty_scope' ? 'chat.noReadyBody' : 'chat.noEvidenceBody')}</p>
            {scope && <p className="t-meta">{scopeLine}</p>}
          </div>
        </div>
        <NotSearched attempt={attempt} />
      </div>
    );
  }

  const nav = navFor(answer.claims);
  const stated = answer.claims.filter((c) => c.kind === 'stated');
  const synthesis = answer.claims.filter((c) => c.kind === 'synthesis');
  const citedNumbers = nav.map((n) => n.number);
  const cited = uniqueCitations(answer.claims);
  const citedDocs = [...new Set(cited.map((c) => c.documentId))];

  return (
    <div className="answer">
      <div className="answer-head">
        <span className="t-label accent">{t('chat.answer')}</span>
        <span className="t-meta">
          {scopeLine}
          {' · '}
          {t('chat.citations', { n: citedNumbers.length })}
          {' · '}
          {attempt.model}
          {attempt.finishedAt ? ` · ${formatDate(locale, attempt.finishedAt)}` : ''}
        </span>
      </div>
      {attempt.state === 'partial' && (
        <p className="status warn">
          <Icon name="warn" size={12} strokeWidth={1.6} />
          {answer.droppedClaims > 0 ? t('chat.partialDropped', { n: answer.droppedClaims }) : t('chat.partial')}
        </p>
      )}
      {stated.map((c, i) => (
        <Claim key={`s${i}`} claim={c} nav={nav} owner={attempt.id} />
      ))}
      {synthesis.length > 0 && (
        <div className="synthesis">
          <span className="synth-label">
            <Icon name="info" size={12} />
            {t('chat.synthesis')}
          </span>
          {synthesis.map((c, i) => (
            <Claim key={`y${i}`} claim={c} nav={nav} owner={attempt.id} />
          ))}
        </div>
      )}
      {answer.limitations && (
        <p className="limitations" dir="auto">
          <strong>{t('chat.limitations')}</strong> {answer.limitations}
        </p>
      )}
      <Sources attempt={attempt} cited={cited} nav={nav} />
      <NotSearched attempt={attempt} />
      <AnswerActions claims={answer.claims} docs={citedDocs} attempt={attempt} />
    </div>
  );
}

function navFor(claims: ValidatedClaim[]): EvidenceNavItem[] {
  const out: EvidenceNavItem[] = [];
  for (const c of claims)
    for (const ref of c.citations) {
      if (!ref.citationId || out.some((o) => o.number === ref.number)) continue;
      out.push({ target: { type: 'citation', citationId: ref.citationId }, number: ref.number });
    }
  return out.sort((a, b) => a.number - b.number);
}

function uniqueCitations(claims: ValidatedClaim[]): CitationRef[] {
  const out: CitationRef[] = [];
  for (const c of claims) for (const r of c.citations) if (!out.some((o) => o.number === r.number)) out.push(r);
  return out.sort((a, b) => a.number - b.number);
}

function Claim({ claim, nav, owner }: { claim: ValidatedClaim; nav: EvidenceNavItem[]; owner: string }) {
  const t = useT();
  const evidence = useRagStore((s) => s.evidence);
  const isOpen = !!evidence && evidence.owner === owner && claim.citations.some((c) => c.number === evidence.number);
  return (
    <p className="claim selectable" dir="auto">
      <span className={isOpen ? 'claim-open' : undefined}>{claim.text}</span>
      {claim.unverifiedNumber && (
        <span className="num-warn" title={t('chat.numberWarn')}>
          <Icon name="warn" size={11} strokeWidth={1.6} />
          <span className="sr-only">{t('chat.numberWarn')}</span>
        </span>
      )}{' '}
      {claim.citations.map((c) => (
        <CitationChip key={`${c.number}-${c.citationId}`} cite={c} nav={nav} owner={owner} />
      ))}
    </p>
  );
}

function CitationChip({ cite, nav, owner }: { cite: CitationRef; nav: EvidenceNavItem[]; owner: string }) {
  const t = useT();
  const locale = useLocale();
  const evidence = useRagStore((s) => s.evidence);
  const open = useRagStore((s) => s.openEvidence);
  const active = evidence?.owner === owner && evidence.number === cite.number;
  const page = cite.pageIndex + 1;
  if (!cite.citationId) return null;
  return (
    <button
      className={`chip${active ? ' active' : ''}`}
      title={t('chat.openPage', { page })}
      aria-label={t('chat.citationLabel', { n: cite.number, page })}
      data-coach="citation"
      onClick={(e) => void open({ type: 'citation', citationId: cite.citationId! }, cite.number, nav, e.currentTarget, owner)}
    >
      {formatNumber(locale, cite.number)} · p. {formatNumber(locale, page)}
      <Icon name="open" size={10} strokeWidth={1.4} />
    </button>
  );
}

function Sources({ attempt, cited, nav }: { attempt: AttemptView; cited: CitationRef[]; nav: EvidenceNavItem[] }) {
  const t = useT();
  const evidence = useRagStore((s) => s.evidence);
  const open = useRagStore((s) => s.openEvidence);
  return (
    <div className="sources">
      <span className="t-label sources-head">{t('chat.sources')}</span>
      {cited.map((c) => {
        const src = attempt.sources.find((s) => s.evidenceId === c.evidenceId);
        const viewing = evidence?.owner === attempt.id && evidence.number === c.number;
        return (
          <div key={c.number} className={`source${viewing ? ' viewing' : ''}`}>
            {viewing && <span className="row-marker" aria-hidden="true" />}
            <span className="src-num mono">[{c.number}]</span>
            <div className="src-body">
              <span className="t-paper-title" dir="auto">
                {src?.title}
              </span>
              <span className="t-meta">
                <span className="t-page">p. {c.pageIndex + 1}</span>
                {src?.deleted ? ` · ${t('chat.sourceDeleted')}` : ''}
              </span>
              {c.quote && (
                <q className="quote selectable" dir="auto">
                  {c.quote}
                </q>
              )}
            </div>
            {viewing ? (
              <span className="viewing-badge">
                <Icon name="eye" size={12} />
                {t('status.viewing')}
              </span>
            ) : (
              <button
                className="open-btn"
                disabled={src?.deleted}
                onClick={(e) => c.citationId && void open({ type: 'citation', citationId: c.citationId }, c.number, nav, e.currentTarget, attempt.id)}
              >
                {t('chat.openP', { page: c.pageIndex + 1 })}
                <Icon name="open" size={10} />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

function NotSearched({ attempt }: { attempt: AttemptView }) {
  const t = useT();
  const list = attempt.scope?.notSearched ?? [];
  if (list.length === 0) return null;
  return (
    <details className="not-searched">
      <summary>{t('chat.notSearched', { n: list.length })}</summary>
      <ul>
        {list.map((n) => (
          <li key={n.documentId}>
            <span dir="auto">{n.title}</span> — <span className="t-meta">{t(`chat.reason.${n.reason}` as MessageKey)}</span>
          </li>
        ))}
      </ul>
    </details>
  );
}

function AnswerActions({ claims, docs, attempt }: { claims: ValidatedClaim[]; docs: string[]; attempt: AttemptView }) {
  const t = useT();
  const toast = useAppStore((s) => s.toast);
  const setTab = useAppStore((s) => s.setTab);
  function copy() {
    const lines = claims.map((c) => `${c.text} ${c.citations.map((r) => `[${r.number}, p. ${r.pageIndex + 1}]`).join(' ')}`);
    const refs = uniqueCitations(claims).map((r) => {
      const s = attempt.sources.find((x) => x.evidenceId === r.evidenceId);
      return `[${r.number}] ${s?.title ?? ''}, p. ${r.pageIndex + 1}${r.quote ? ` — “${r.quote}”` : ''}`;
    });
    void navigator.clipboard.writeText(`${lines.join('\n\n')}\n\n${refs.join('\n')}`).then(() => toast('info', t('chat.copied')));
  }
  return (
    <div className="answer-actions">
      {docs.length === 2 && (
        <button
          className="link-btn"
          onClick={() => {
            useRagStore.setState({ compareSelection: docs, comparison: null });
            setTab('compare');
          }}
        >
          {t('chat.compareThese')}
        </button>
      )}
      <button className="link-btn muted" onClick={copy}>
        {t('chat.copy')}
      </button>
    </div>
  );
}

function PassageResults() {
  const t = useT();
  const search = useRagStore((s) => s.search)!;
  const clear = useRagStore((s) => s.clearSearch);
  const open = useRagStore((s) => s.openEvidence);
  const evidence = useRagStore((s) => s.evidence);
  const nav: EvidenceNavItem[] = search.results.hits.map((h, i) => ({ target: { type: 'hit', chunkId: h.chunkId }, number: i + 1 }));
  const mode = search.results.scope.mode;
  return (
    <div className="conv">
      <div className="asked">
        <span className="t-label">{t('chat.passagesFor')}</span>
        <div className="question" dir="auto">
          {search.query}
        </div>
        <span className="t-meta">
          {t('chat.searched', { searched: search.results.scope.searched, requested: search.results.scope.requested })} ·{' '}
          {t(mode === 'hybrid' ? 'chat.mode.hybrid' : mode === 'semantic' ? 'chat.mode.semantic' : 'chat.mode.lexical')}
        </span>
      </div>
      {search.results.hits.length === 0 && <p className="t-body small">{t('chat.noPassages')}</p>}
      <div className="sources">
        {search.results.hits.map((h, i) => {
          const viewing = evidence?.owner === 'search' && evidence.number === i + 1;
          return (
            <div key={h.chunkId} className={`source${viewing ? ' viewing' : ''}`}>
              <span className="src-num mono">{i + 1}</span>
              <div className="src-body">
                <span className="t-paper-title" dir="auto">
                  {h.title}
                </span>
                <span className="t-meta">
                  <span className="t-page">p. {h.pageIndex + 1}</span>
                  {h.lexicalRank ? ` · ${t('chat.rank.lexical', { n: h.lexicalRank })}` : ''}
                  {h.semanticRank ? ` · ${t('chat.rank.semantic', { n: h.semanticRank })}` : ''}
                </span>
                <span className="excerpt selectable" dir="auto">
                  {h.excerpt.length > 420 ? `${h.excerpt.slice(0, 420)}…` : h.excerpt}
                </span>
              </div>
              <button className="open-btn" onClick={(e) => void open({ type: 'hit', chunkId: h.chunkId }, i + 1, nav, e.currentTarget, 'search')}>
                {t('chat.openP', { page: h.pageIndex + 1 })}
                <Icon name="open" size={10} />
              </button>
            </div>
          );
        })}
      </div>
      <button className="link-btn muted" onClick={clear}>
        {t('chat.backToConversation')}
      </button>
    </div>
  );
}
