import { useEffect, useRef, useState } from 'react';
import { useAppStore } from '../../app/store';
import { useRagStore } from '../../app/ragStore';
import { api } from '../../shared/ipc/commands';
import { useT, type MessageKey } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import type { ModelInfo } from '../../shared/types/generated/ModelInfo';
import type { ProviderView } from '../../shared/types/generated/ProviderView';
import type { SignInStart } from '../../shared/types/generated/SignInStart';
import { API_PRESETS, LITELLM_PRESET, OLLAMA_LOGO, SIGN_IN_LOGOS, logoFor, type ApiPreset, type Logo } from './catalog';
import './models.css';

/** Turns a provider/model error detail into a user-facing message key. */
export function modelErrorKey(detail: string | null | undefined): MessageKey {
  const d = (detail ?? '').toLowerCase();
  if (d.includes('ollama_not_running')) return 'models.err.ollamaNotRunning';
  if (d.includes('chatgpt_usage_limit')) return 'models.err.chatgptLimit';
  if (d.includes('chatgpt_not_eligible')) return 'models.err.chatgptNotEligible';
  if (d.includes('chatgpt_signed_out') || d.includes('copilot_signed_out') || d.includes('signed_out')) return 'models.err.signedOut';
  if (d.includes('copilot_not_enabled')) return 'models.err.copilotNotEnabled';
  if (d.includes('unauthorized')) return 'models.err.unauthorized';
  if (d.includes('not_found')) return 'models.err.modelNotFound';
  if (d.includes('rate_limited')) return 'models.err.rateLimited';
  if (d.includes('timeout')) return 'models.err.timeout';
  if (d.includes('unreachable') || d.includes('cant_reach')) return 'models.err.unreachable';
  if (d.includes('no embedding') || d.includes('does not provide embeddings')) return 'models.err.noEmbeddings';
  return 'models.err.generic';
}

export function ModelSetup({ compact = false }: { compact?: boolean }) {
  const t = useT();
  const models = useRagStore((s) => s.models);
  const refresh = useRagStore((s) => s.refreshModels);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  if (!models) return null;
  const gen = models.generation;
  const emb = models.embedding;
  const label = (id: string) => models.providers.find((p) => p.id === id)?.label ?? '?';
  return (
    <div className="msetup">
      <dl className="mchoices">
        <dt>{t('models.answers')}</dt>
        <dd>{gen ? `${gen.model} · ${label(gen.providerId)}` : <span className="status warn">{t('models.notSet')}</span>}</dd>
        <dt>{t('models.meaningSearch')}</dt>
        <dd>
          {emb ? `${emb.model} · ${label(emb.providerId)} · ${emb.dimensions}d` : <span className="t-meta">{t('models.keywordOnly')}</span>}
          {emb && models.indexedDocs > 0 && (
            <span className="t-meta"> — {t('models.semanticCoverage', { done: models.semanticDocs, total: models.indexedDocs })}</span>
          )}
        </dd>
      </dl>
      {models.providers.length > 0 && (
        <ul className="mproviders">
          {models.providers.map((p) => (
            <ProviderRow key={p.id} p={p} consented={models.cloudConsent.includes(p.id)} />
          ))}
        </ul>
      )}
      <AddProvider compact={compact} existing={models.providers} />
    </div>
  );
}

function ProviderRow({ p, consented }: { p: ProviderView; consented: boolean }) {
  const t = useT();
  const fail = useAppStore((s) => s.fail);
  const toast = useAppStore((s) => s.toast);
  const models = useRagStore((s) => s.models);
  const refresh = useRagStore((s) => s.refreshModels);
  const [list, setList] = useState<ModelInfo[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [gen, setGen] = useState('');
  const [emb, setEmb] = useState('');
  const [busy, setBusy] = useState<'' | 'gen' | 'emb' | 'load'>('');
  const [askConsent, setAskConsent] = useState(false);

  async function load() {
    setBusy('load');
    setLoadError(null);
    try {
      const l = await api.listProviderModels(p.id);
      setList(l);
      setGen((g) => g || l.find((m) => m.canGenerate)?.id || '');
      setEmb((e) => e || l.find((m) => m.canEmbed)?.id || '');
    } catch (e) {
      const detail = (e as { code?: string })?.code ? '' : String(e);
      setLoadError(detail || 'error');
      fail(e);
    } finally {
      setBusy('');
    }
  }
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p.id]);

  async function use(kind: 'gen' | 'emb') {
    if (!p.isLocal && !consented) {
      setAskConsent(true);
      return;
    }
    setBusy(kind);
    try {
      const model = kind === 'gen' ? gen : emb;
      const test = await api.testModel(p.id, model, kind === 'gen' ? 'generation' : 'embedding');
      if (!test.ok) {
        toast('error', t(modelErrorKey(test.error)), test.error?.slice(0, 160));
        return;
      }
      if (kind === 'gen') await api.saveGenerationModel(p.id, model);
      else await api.saveEmbeddingModel(p.id, model);
      toast('info', t(kind === 'gen' ? 'models.savedGen' : 'models.savedEmb', { model, ms: test.elapsedMs }));
      await refresh();
      void useAppStore.getState().refreshOnboarding();
    } catch (e) {
      fail(e);
    } finally {
      setBusy('');
    }
  }

  const genModels = list?.filter((m) => m.canGenerate) ?? [];
  const embModels = list?.filter((m) => m.canEmbed) ?? [];
  const isGen = models?.generation?.providerId === p.id;
  const isEmb = models?.embedding?.providerId === p.id;

  return (
    <li className="mprovider">
      <div className="mprovider-head">
        <ProviderLogo logo={logoFor(p.kind, p.baseUrl, p.label)} size={20} />
        <strong>{p.label}</strong>
        <span className={`pill ${p.isLocal ? 'local' : 'cloud'}`}>{t(p.isLocal ? 'models.local' : 'models.cloud')}</span>
        {p.account && <span className="t-meta">{p.account}</span>}
        <span className="t-meta mono-small" dir="ltr">
          {p.baseUrl}
        </span>
        <button className="link-btn muted" onClick={() => void api.removeProvider(p.id).then(refresh).catch(fail)}>
          {t('models.remove')}
        </button>
      </div>
      {!p.isLocal && (
        <label className="consent">
          <input type="checkbox" checked={consented} onChange={(e) => void api.setCloudConsent(p.id, e.target.checked).then(refresh).catch(fail)} />
          <span>{t('models.consent', { name: p.label })}</span>
        </label>
      )}
      {askConsent && !consented && <p className="status warn">{t('models.consentNeeded')}</p>}
      {busy === 'load' && <p className="t-meta">{t('models.loading')}</p>}
      {loadError && (
        <p className="status error">
          {t('models.loadFailed')}{' '}
          <button className="link-btn" onClick={() => void load()}>
            {t('job.retry')}
          </button>
        </p>
      )}
      {list && (
        <div className="mpick">
          <label>
            <span>{t('models.forAnswers')}</span>
            <select className="field" value={gen} onChange={(e) => setGen(e.target.value)} disabled={genModels.length === 0}>
              {genModels.length === 0 && <option value="">{t('models.none')}</option>}
              {genModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                  {m.sizeBytes ? ` · ${(m.sizeBytes / 1e9).toFixed(1)} GB` : ''}
                </option>
              ))}
            </select>
          </label>
          <button className="btn" disabled={!gen || busy !== ''} onClick={() => void use('gen')}>
            {busy === 'gen' ? t('models.testing') : isGen && models?.generation?.model === gen ? t('models.inUse') : t('models.useForAnswers')}
          </button>
          {embModels.length > 0 && (
            <>
              <label>
                <span>{t('models.forSearch')}</span>
                <select className="field" value={emb} onChange={(e) => setEmb(e.target.value)}>
                  {embModels.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.label}
                    </option>
                  ))}
                </select>
              </label>
              <button className="btn" disabled={!emb || busy !== ''} onClick={() => void use('emb')}>
                {busy === 'emb' ? t('models.testing') : isEmb && models?.embedding?.model === emb ? t('models.inUse') : t('models.useForSearch')}
              </button>
            </>
          )}
        </div>
      )}
      {list && (
        <p className="t-meta">
          {genModels.length === 0 ? t('models.noneFound') : t('models.found', { n: genModels.length })}{' '}
          <button className="link-btn" disabled={busy !== ''} onClick={() => void load()}>
            {t('models.refresh')}
          </button>
        </p>
      )}
      {list && <p className="t-meta">{t('models.testNote')}</p>}
    </li>
  );
}

type Mode = '' | 'ollama' | 'chatgpt' | 'copilot' | 'litellm' | `api:${string}`;

/** A provider's brand mark. Single-colour marks follow the text colour. */
export function ProviderLogo({ logo, size = 20 }: { logo: Logo | undefined; size?: number }) {
  const box = { width: size, height: size };
  if (!logo)
    return (
      <span className="plogo generic" style={box} aria-hidden="true">
        <Icon name="settings" size={Math.round(size * 0.7)} />
      </span>
    );
  if (logo.mono)
    return <span className="plogo mono" style={{ ...box, WebkitMaskImage: `url("${logo.src}")`, maskImage: `url("${logo.src}")` }} aria-hidden="true" />;
  return <img className="plogo" src={logo.src} width={size} height={size} alt="" aria-hidden="true" draggable={false} />;
}

function Card({ on, logo, title, body, onClick }: { on: boolean; logo: Logo | undefined; title: string; body: string; onClick: () => void }) {
  return (
    <button className={`mcard${on ? ' on' : ''}`} onClick={onClick} aria-pressed={on}>
      <ProviderLogo logo={logo} size={26} />
      <span className="mcard-text">
        <strong>{title}</strong>
        <span>{body}</span>
      </span>
    </button>
  );
}

function AddProvider({ compact }: { compact: boolean; existing: ProviderView[] }) {
  const t = useT();
  const [mode, setMode] = useState<Mode>('');
  const toggle = (m: Mode) => setMode((cur) => (cur === m ? '' : m));
  const done = () => setMode('');
  const apiPreset = mode.startsWith('api:') ? API_PRESETS.find((p) => `api:${p.id}` === mode) : undefined;
  return (
    <div className="madd">
      <span className="t-label">{t('models.connect')}</span>

      <section className="mgroup" aria-label={t('models.group.signIn')}>
        <h3 className="mgroup-title">{t('models.group.signIn')}</h3>
        <div className={`mcards${compact ? ' compact' : ''}`}>
          <Card on={mode === 'chatgpt'} logo={SIGN_IN_LOGOS.chatgpt} title="ChatGPT" body={t('models.card.chatgpt')} onClick={() => toggle('chatgpt')} />
          <Card on={mode === 'copilot'} logo={SIGN_IN_LOGOS.github_copilot} title="GitHub Copilot" body={t('models.card.copilot')} onClick={() => toggle('copilot')} />
        </div>
        {mode === 'chatgpt' && <SignInFlow kind="chatgpt" onDone={done} />}
        {mode === 'copilot' && <SignInFlow kind="github_copilot" onDone={done} />}
      </section>

      <section className="mgroup" aria-label={t('models.group.api')}>
        <h3 className="mgroup-title">{t('models.group.api')}</h3>
        <p className="t-meta">{t('models.group.apiNote')}</p>
        <div className="mtiles">
          {API_PRESETS.map((p) => {
            const on = mode === `api:${p.id}`;
            return (
              <button key={p.id} className={`mtile${on ? ' on' : ''}`} onClick={() => toggle(`api:${p.id}`)} aria-pressed={on}>
                <ProviderLogo logo={p.logo} size={18} />
                <span>{p.id === 'custom' ? t('models.api.custom') : p.label}</span>
              </button>
            );
          })}
        </div>
        {apiPreset && <ApiKeyForm key={apiPreset.id} preset={apiPreset} onDone={done} />}
      </section>

      <section className="mgroup" aria-label={t('models.group.local')}>
        <h3 className="mgroup-title">{t('models.group.local')}</h3>
        <div className={`mcards${compact ? ' compact' : ''}`}>
          <Card on={mode === 'ollama'} logo={OLLAMA_LOGO} title="Ollama" body={t('models.card.ollama')} onClick={() => toggle('ollama')} />
          <Card on={mode === 'litellm'} logo={LITELLM_PRESET.logo} title="LiteLLM" body={t('models.card.litellm')} onClick={() => toggle('litellm')} />
        </div>
        {mode === 'ollama' && <OllamaForm onDone={done} />}
        {mode === 'litellm' && <ApiKeyForm key="litellm" preset={LITELLM_PRESET} onDone={done} />}
      </section>
    </div>
  );
}

function OllamaForm({ onDone }: { onDone: () => void }) {
  const t = useT();
  const fail = useAppStore((s) => s.fail);
  const refresh = useRagStore((s) => s.refreshModels);
  const [url, setUrl] = useState('http://127.0.0.1:11434');
  return (
    <div className="mform">
      <p>{t('models.ollama.body')}</p>
      <div className="mrow">
        <input className="field" value={url} onChange={(e) => setUrl(e.target.value)} dir="ltr" aria-label={t('models.ollama.address')} />
        <button
          className="btn btn-primary"
          onClick={() =>
            void api
              .addOllama(url)
              .then(refresh)
              .then(onDone)
              .catch(fail)
          }
        >
          {t('models.connectBtn')}
        </button>
      </div>
      <p className="t-meta">
        {t('models.ollama.help')}{' '}
        <button className="link-btn" onClick={() => void api.openExternal('https://ollama.com/download')}>
          ollama.com/download
        </button>
      </p>
    </div>
  );
}

function ApiKeyForm({ preset, onDone }: { preset: ApiPreset; onDone: () => void }) {
  const t = useT();
  const fail = useAppStore((s) => s.fail);
  const refresh = useRagStore((s) => s.refreshModels);
  const isCustom = preset.id === 'custom';
  const [url, setUrl] = useState(preset.url);
  const [label, setLabel] = useState(isCustom ? '' : preset.label);
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const keyOk = key.trim().length >= 8 || (preset.keyOptional === true && key.trim() === '');
  return (
    <form
      className="mform"
      onSubmit={(e) => {
        e.preventDefault();
        setBusy(true);
        api
          .addApiProvider(preset.kind, label, url, key.trim())
          .then(() => {
            setKey('');
            return refresh();
          })
          .then(onDone)
          .catch(fail)
          .finally(() => setBusy(false));
      }}
    >
      <div className="mform-head">
        <ProviderLogo logo={preset.logo} size={22} />
        <strong>{isCustom ? t('models.api.custom') : preset.label}</strong>
      </div>
      {preset.id === 'litellm' && <p>{t('models.litellm.body')}</p>}
      {isCustom && <p className="t-meta">{t('models.api.customNote')}</p>}
      <label className="mfield">
        <span>{t('models.api.label')}</span>
        <input className="field" value={label} onChange={(e) => setLabel(e.target.value)} maxLength={60} required />
      </label>
      <label className="mfield">
        <span>{t('models.api.url')}</span>
        <input className="field" value={url} onChange={(e) => setUrl(e.target.value)} dir="ltr" required />
      </label>
      <label className="mfield">
        <span>{preset.keyOptional ? t('models.api.keyOptional') : t('models.api.key')}</span>
        <input className="field" type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} dir="ltr" />
      </label>
      <p className="t-meta">{t('models.api.keyNote')}</p>
      <div className="mrow">
        <button className="btn btn-primary" type="submit" disabled={busy || !keyOk || label.trim() === '' || url.trim().length < 10}>
          {t('models.connectBtn')}
        </button>
        {preset.keyUrl && (
          <button type="button" className="link-btn" onClick={() => void api.openExternal(preset.keyUrl!)}>
            {preset.id === 'litellm' ? t('models.litellm.docs') : t('models.api.getKey')}
          </button>
        )}
      </div>
    </form>
  );
}

function SignInFlow({ kind, onDone }: { kind: 'chatgpt' | 'github_copilot'; onDone: () => void }) {
  const t = useT();
  const fail = useAppStore((s) => s.fail);
  const toast = useAppStore((s) => s.toast);
  const refresh = useRagStore((s) => s.refreshModels);
  const [start, setStart] = useState<SignInStart | null>(null);
  const [status, setStatus] = useState<string>('');
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  async function begin() {
    try {
      const s = await api.startSignIn(kind);
      setStart(s);
      setStatus('pending');
      if (s.kind === 'browser') await api.openExternal(s.verificationUri);
      poll(s.flowId, Math.max(2, s.interval));
    } catch (e) {
      fail(e);
    }
  }

  function poll(flowId: string, every: number) {
    timer.current = setTimeout(async () => {
      try {
        const r = await api.pollSignIn(flowId);
        if (r.status === 'pending') return poll(flowId, every);
        setStatus(r.status);
        if (r.status === 'complete') {
          toast('info', t('models.signedIn', { name: r.provider?.account ?? r.provider?.label ?? '' }));
          await refresh();
          onDone();
        } else if (r.message) {
          toast(r.status === 'error' ? 'error' : 'warn', t(r.message.includes('plan_off') ? 'models.err.chatgptPlanOff' : 'models.signInFailed'), r.message);
        }
      } catch (e) {
        fail(e);
      }
    }, every * 1000);
  }

  function cancel() {
    if (timer.current) clearTimeout(timer.current);
    if (start) void api.cancelSignIn(start.flowId);
    setStart(null);
    setStatus('');
  }

  const isChatgpt = kind === 'chatgpt';
  return (
    <div className="mform">
      <p>{t(isChatgpt ? 'models.chatgpt.body' : 'models.copilot.body')}</p>
      {!start && (
        <button className="btn btn-primary btn-lg" onClick={() => void begin()}>
          {t(isChatgpt ? 'models.chatgpt.button' : 'models.copilot.button')}
        </button>
      )}
      {start && start.kind === 'device' && status === 'pending' && (
        <div className="device">
          <span className="t-meta">{t('models.copilot.enterCode')}</span>
          <span className="device-code selectable" dir="ltr">
            {start.userCode}
          </span>
          <div className="mrow">
            <button className="btn" onClick={() => void navigator.clipboard.writeText(start.userCode ?? '')}>
              {t('models.copyCode')}
            </button>
            <button className="btn btn-primary" onClick={() => void api.openExternal(start.verificationUri)}>
              {t('models.copilot.open')}
            </button>
          </div>
        </div>
      )}
      {start && start.kind === 'browser' && status === 'pending' && (
        <p className="status neutral">
          <Icon name="clock" size={12} /> {t('models.chatgpt.waiting')}{' '}
          <button className="link-btn" onClick={() => void api.openExternal(start.verificationUri)}>
            {t('models.chatgpt.reopen')}
          </button>
        </p>
      )}
      {(status === 'expired' || status === 'denied' || status === 'error') && <p className="status warn">{t('models.signInFailed')}</p>}
      {start && (
        <button className="link-btn muted" onClick={cancel}>
          {t('common.cancel')}
        </button>
      )}
      <p className="t-meta">{t(isChatgpt ? 'models.chatgpt.note' : 'models.copilot.note')}</p>
    </div>
  );
}
