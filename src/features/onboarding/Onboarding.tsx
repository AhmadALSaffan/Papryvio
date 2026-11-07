import { useEffect, useMemo, useState } from 'react';
import { useAppStore } from '../../app/store';
import { api } from '../../shared/ipc/commands';
import { useT, type MessageKey } from '../../shared/i18n';
import { Dialog } from '../../shared/ui/Dialog';
import { Icon } from '../../shared/ui/Icon';
import type { StepKey } from '../../shared/types/generated/StepKey';
import { failureKey, hasText } from '../library/status';
import { ProgressRow } from '../workspace/JobControls';
import { ModelSetup } from '../models/ModelSetup';
import { useRagStore } from '../../app/ragStore';
import './onboarding.css';

/** Guided steps. Readiness, opening a source and comparing complete from real use and show in the summary. */
const FLOW: StepKey[] = ['welcome', 'models', 'collection', 'import', 'first_question'];
type View = StepKey | 'summary';

export function Onboarding() {
  const t = useT();
  const open = useAppStore((s) => s.onboardingOpen);
  const onboarding = useAppStore((s) => s.onboarding);
  const setOpen = useAppStore((s) => s.setOnboardingOpen);
  const [view, setView] = useState<View | null>(null);

  const stateOf = useMemo(() => {
    const m = new Map<StepKey, string>();
    onboarding?.steps.forEach((s) => m.set(s.key, s.state));
    return m;
  }, [onboarding]);

  // Resume at the first unfinished step.
  useEffect(() => {
    if (!open || !onboarding) return;
    if (view === null) setView(FLOW.find((k) => stateOf.get(k) === 'pending') ?? 'summary');
  }, [open, onboarding, stateOf, view]);

  if (!open || !onboarding || view === null) return null;

  async function dismiss() {
    try {
      useAppStore.setState({ onboarding: await api.setOnboardingDismissed(true) });
    } catch {
      /* keep the UI responsive even if saving fails */
    }
    setOpen(false);
    setView(null);
  }

  async function record(step: StepKey, action: 'complete' | 'skip', related?: string) {
    try {
      useAppStore.setState({ onboarding: await api.updateOnboarding(step, action, related) });
      return true;
    } catch (e) {
      useAppStore.getState().fail(e);
      return false;
    }
  }

  const next = (from: View) => {
    const i = FLOW.indexOf(from as StepKey);
    setView(i >= 0 && i < FLOW.length - 1 ? FLOW[i + 1]! : 'summary');
  };
  const back = (from: View) => {
    const i = from === 'summary' ? FLOW.length : FLOW.indexOf(from as StepKey);
    if (i > 0) setView(FLOW[i - 1]!);
  };
  const index = view === 'summary' ? FLOW.length : FLOW.indexOf(view);

  return (
    <Dialog title={<StepTitle view={view} />} onClose={() => void dismiss()} width={600} labelledBy="onboarding-title">
      <div className="ob">
        {view !== 'summary' && (
          <div className="ob-progress" aria-hidden="true">
            {FLOW.map((k, i) => (
              <span key={k} className={i < index ? 'done' : i === index ? 'current' : ''} />
            ))}
          </div>
        )}
        {view !== 'summary' && <span className="t-meta">{t('onboarding.step', { n: index + 1, total: FLOW.length })}</span>}
        {view === 'welcome' && (
          <WelcomeStep
            onStart={async () => (await record('welcome', 'complete')) && next('welcome')}
            onBrowse={async () => {
              await record('welcome', 'skip');
              await dismiss();
            }}
          />
        )}
        {view === 'models' && (
          <ModelsStep
            onBack={() => back('models')}
            onLater={async () => (await record('models', 'skip')) && next('models')}
            onNext={() => next('models')}
            done={stateOf.get('models') === 'completed'}
          />
        )}
        {view === 'collection' && (
          <CollectionStep
            onBack={() => back('collection')}
            onDone={async (id) => (await record('collection', 'complete', id)) && next('collection')}
            onSkip={async () => (await record('collection', 'skip')) && next('collection')}
          />
        )}
        {view === 'import' && (
          <ImportStep
            onBack={() => back('import')}
            onImported={(id) => void record('import', 'complete', id)}
            onContinue={async (readyDocId) => {
              if (readyDocId) await record('readiness', 'complete', readyDocId);
              next('import');
            }}
            onSkip={async () => (await record('import', 'skip')) && next('import')}
          />
        )}
        {view === 'first_question' && (
          <QuestionStep
            onBack={() => back('first_question')}
            onTry={() => {
              void dismiss();
            }}
            onNext={() => next('first_question')}
          />
        )}
        {view === 'summary' && <Summary onBack={() => back('summary')} onFinish={() => void dismiss()} />}
      </div>
    </Dialog>
  );
}

function StepTitle({ view }: { view: View }) {
  const t = useT();
  const key: MessageKey =
    view === 'welcome'
      ? 'onboarding.welcome.title'
      : view === 'models'
        ? 'onboarding.models.title'
        : view === 'collection'
          ? 'onboarding.collection.title'
          : view === 'import'
            ? 'onboarding.import.title'
            : view === 'first_question'
              ? 'onboarding.question.title'
              : 'onboarding.summary.title';
  return <span id="onboarding-title">{t(key)}</span>;
}

function WelcomeStep({ onStart, onBrowse }: { onStart: () => void; onBrowse: () => void }) {
  const t = useT();
  const locale = useAppStore((s) => s.locale);
  const setLocale = useAppStore((s) => s.setLocale);
  return (
    <>
      <div className="ob-hero">
        <img src="/brand/papryvio-logo.png" alt="Papryvio" width={112} height={112} />
        <p className="t-body">{t('onboarding.welcome.body')}</p>
      </div>
      <ul className="ob-points">
        <li>
          <Icon name="check" size={14} strokeWidth={1.8} />
          {t('onboarding.welcome.local')}
        </li>
        <li>
          <Icon name="info" size={14} />
          {t('onboarding.welcome.models')}
        </li>
      </ul>
      <fieldset className="ob-lang">
        <legend className="t-label">{t('onboarding.welcome.language')}</legend>
        {(['en', 'ar'] as const).map((l) => (
          <label key={l} className={`lang-card${locale === l ? ' selected' : ''}`}>
            <input type="radio" name="lang" checked={locale === l} onChange={() => void setLocale(l)} />
            <span lang={l} dir={l === 'ar' ? 'rtl' : 'ltr'}>
              {l === 'en' ? 'English' : 'العربية'}
            </span>
          </label>
        ))}
      </fieldset>
      <div className="ob-actions">
        <button className="link-btn muted" onClick={onBrowse}>
          {t('onboarding.welcome.browse')}
        </button>
        <button className="btn btn-primary btn-lg" onClick={onStart}>
          {t('onboarding.welcome.start')}
        </button>
      </div>
    </>
  );
}

function ModelsStep({ onBack, onLater, onNext, done }: { onBack: () => void; onLater: () => void; onNext: () => void; done: boolean }) {
  const t = useT();
  return (
    <>
      <p className="t-body small">{t('onboarding.models.body')}</p>
      <p className="t-meta">{t('onboarding.models.resources')}</p>
      <ModelSetup compact />
      <div className="ob-actions">
        <button className="btn" onClick={onBack}>
          {t('onboarding.back')}
        </button>
        {done ? (
          <button className="btn btn-primary btn-lg" onClick={onNext}>
            {t('onboarding.next')}
          </button>
        ) : (
          <button className="btn btn-lg" onClick={onLater}>
            {t('onboarding.models.later')}
          </button>
        )}
      </div>
    </>
  );
}

function QuestionStep({ onBack, onTry, onNext }: { onBack: () => void; onTry: () => void; onNext: () => void }) {
  const t = useT();
  const setDraft = useRagStore((s) => s.setDraft);
  const newQuestion = useRagStore((s) => s.newQuestion);
  const hasModel = useRagStore((s) => !!s.models?.generation);
  const example = t('chat.example1');
  return (
    <>
      <p className="t-body small">{t('onboarding.question.body')}</p>
      <ul className="ob-points">
        <li>
          <Icon name="folder" size={14} />
          {t('onboarding.question.scope')}
        </li>
        <li>
          <Icon name="open" size={14} />
          {t('onboarding.question.cite')}
        </li>
      </ul>
      {!hasModel && <p className="status warn">{t('onboarding.question.noModel')}</p>}
      <div>
        <span className="t-label">{t('chat.examples')}</span>
        <button
          className="example"
          onClick={() => {
            newQuestion();
            setDraft(example);
            onTry();
          }}
        >
          {example}
        </button>
      </div>
      <div className="ob-actions">
        <button className="btn" onClick={onBack}>
          {t('onboarding.back')}
        </button>
        <button className="btn btn-primary btn-lg" onClick={onNext}>
          {t('onboarding.next')}
        </button>
      </div>
    </>
  );
}

function CollectionStep({ onBack, onDone, onSkip }: { onBack: () => void; onDone: (id: string) => void; onSkip: () => void }) {
  const t = useT();
  const collections = useAppStore((s) => s.collections);
  const create = useAppStore((s) => s.createCollection);
  const select = useAppStore((s) => s.selectCollection);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    const c = await create(name);
    setBusy(false);
    if (c) onDone(c.id);
  }
  return (
    <>
      <p className="t-body">{t('onboarding.collection.body')}</p>
      <form className="ob-inline" onSubmit={(e) => void submit(e)}>
        <input className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder={t('collection.namePlaceholder')} maxLength={80} dir="auto" data-autofocus />
        <button className="btn btn-primary" type="submit" disabled={busy || !name.trim()}>
          {t('collection.create')}
        </button>
      </form>
      {collections.length > 0 && (
        <div className="ob-existing">
          {collections.map((c) => (
            <button
              key={c.id}
              className="btn"
              onClick={() => {
                void select(c.id);
                onDone(c.id);
              }}
            >
              <Icon name="folder" size={14} />
              <span dir="auto">{c.name}</span>
            </button>
          ))}
        </div>
      )}
      <div className="ob-actions">
        <button className="btn" onClick={onBack}>
          {t('onboarding.back')}
        </button>
        <button className="link-btn muted" onClick={onSkip}>
          {t('onboarding.skip')}
        </button>
      </div>
    </>
  );
}

function ImportStep({
  onBack,
  onImported,
  onContinue,
  onSkip,
}: {
  onBack: () => void;
  onImported: (id: string) => void;
  onContinue: (readyDocId: string | null) => void;
  onSkip: () => void;
}) {
  const t = useT();
  const importSelection = useAppStore((s) => s.importSelection);
  const importing = useAppStore((s) => s.importing);
  const [docId, setDocId] = useState<string | null>(null);
  const [fileName, setFileName] = useState('');
  const doc = useAppStore((s) => s.documents.find((d) => d.id === docId) ?? null);

  async function choose() {
    const report = await importSelection('pick');
    const item = report?.items.find((i) => i.outcome.type !== 'rejected');
    if (item && item.outcome.type !== 'rejected') {
      setDocId(item.outcome.documentId);
      setFileName(item.fileName);
      onImported(item.outcome.documentId);
    }
  }

  return (
    <>
      <p className="t-body">{t('onboarding.import.body')}</p>
      {!docId && (
        <div className="dropzone small">
          <Icon name="file" size={24} strokeWidth={1.1} />
          <span className="dropzone-title">{t('empty.drop')}</span>
          <button className="btn btn-primary btn-lg" onClick={() => void choose()} disabled={importing}>
            {t('empty.choose')}
          </button>
          <span className="dropzone-hint">{t('empty.copyNote')}</span>
        </div>
      )}
      {docId && (
        <div className="ob-import-status" aria-live="polite">
          {!doc || doc.state === 'imported' || doc.state === 'extracting' ? (
            <>
              <strong dir="auto">{t('onboarding.import.processing', { name: fileName })}</strong>
              {doc && <ProgressRow doc={doc} />}
            </>
          ) : hasText(doc) ? (
            <span className="status ok">
              <Icon name="check" size={14} strokeWidth={1.8} />
              {t('onboarding.import.done', { name: doc.title })}
            </span>
          ) : doc.state === 'needs_ocr' ? (
            <span className="status warn">
              <Icon name="warn" size={14} />
              {t('onboarding.import.scanned', { name: fileName })}
            </span>
          ) : (
            <span className="status error">
              <Icon name="error" size={14} />
              {t('onboarding.import.failed', { name: fileName, reason: t(failureKey(doc)) })}
            </span>
          )}
          {doc && !hasText(doc) && doc.state !== 'imported' && doc.state !== 'extracting' && (
            <button className="link-btn" onClick={() => void choose()}>
              {t('onboarding.import.another')}
            </button>
          )}
        </div>
      )}
      <div className="ob-actions">
        <button className="btn" onClick={onBack}>
          {t('onboarding.back')}
        </button>
        {docId ? (
          <button className="btn btn-primary btn-lg" onClick={() => onContinue(doc && hasText(doc) ? doc.id : null)}>
            {t('onboarding.next')}
          </button>
        ) : (
          <button className="link-btn muted" onClick={onSkip}>
            {t('onboarding.skip')}
          </button>
        )}
      </div>
    </>
  );
}

function Summary({ onBack, onFinish }: { onBack: () => void; onFinish: () => void }) {
  const t = useT();
  const steps = useAppStore((s) => s.onboarding?.steps ?? []);
  return (
    <>
      <p className="t-body">{t('onboarding.summary.body')}</p>
      <ul className="ob-summary">
        {steps.map((s) => (
          <li key={s.key} className={s.state}>
            <Icon name={s.state === 'completed' ? 'check' : s.state === 'skipped' ? 'minus' : 'clock'} size={14} strokeWidth={1.7} />
            <span>{t(`onboarding.steps.${s.key}` as MessageKey)}</span>
            <span className="t-meta">
              {t(s.state === 'completed' ? 'onboarding.summary.done' : s.state === 'skipped' ? 'onboarding.summary.skipped' : 'onboarding.summary.pending')}
            </span>
          </li>
        ))}
      </ul>
      <div className="ob-actions">
        <button className="btn" onClick={onBack}>
          {t('onboarding.back')}
        </button>
        <button className="btn btn-primary btn-lg" onClick={onFinish}>
          {t('onboarding.summary.finish')}
        </button>
      </div>
    </>
  );
}
