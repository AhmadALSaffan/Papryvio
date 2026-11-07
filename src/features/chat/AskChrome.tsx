import { useEffect, useRef, useState } from 'react';
import { useAppStore } from '../../app/store';
import { useRagStore } from '../../app/ragStore';
import { formatDate, formatNumber, useLocale, useT } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import { hasText, isProcessing } from '../library/status';
import { ProviderLogo } from '../models/ModelSetup';
import { logoFor } from '../models/catalog';

const isSearchable = (d: { lexicalReady: boolean }) => d.lexicalReady;

/** Pane header: conversation title, history and "New question" (Ctrl+N). */
export function AskHeader() {
  const t = useT();
  const locale = useLocale();
  const conversation = useRagStore((s) => s.conversation);
  const conversations = useRagStore((s) => s.conversations);
  const openConversation = useRagStore((s) => s.openConversation);
  const newQuestion = useRagStore((s) => s.newQuestion);
  const center = useAppStore((s) => s.center);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  const title = center === 'conversation' && conversation ? conversation.title : center === 'conversation' ? t('chat.newConversation') : null;
  return (
    <div className="ask-head">
      <div className="ask-title">
        {title ? (
          <>
            <span className="ask-title-main" dir="auto">
              {title}
            </span>
            {conversation && <span className="t-meta">{t('chat.conversationAt', { date: formatDate(locale, conversation.createdAt) })}</span>}
          </>
        ) : (
          <span className="ask-title-main">{t('nav.ask')}</span>
        )}
      </div>
      <div className="ask-actions" ref={ref}>
        {conversations.length > 0 && (
          <button className="btn" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            <Icon name="clock" size={13} />
            {t('chat.history')}
          </button>
        )}
        <button className="btn" onClick={newQuestion} data-coach="new-question">
          {t('chat.newQuestion')} <span className="kbd">Ctrl+N</span>
        </button>
        {open && (
          <div className="menu" role="menu" style={{ insetInlineEnd: 0, top: 38, width: 320, maxHeight: 360, overflow: 'auto' }}>
            {conversations.map((c) => (
              <button
                key={c.id}
                className="menu-item"
                role="menuitemradio"
                aria-checked={conversation?.id === c.id}
                onClick={() => {
                  setOpen(false);
                  void openConversation(c.id);
                }}
              >
                <span className="ellipsis" dir="auto">
                  {c.title}
                </span>
                <span className="count">{formatDate(locale, c.updatedAt)}</span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

/** Search scope: which papers questions and passage search will use. */
export function ScopeBar() {
  const t = useT();
  const locale = useLocale();
  const docs = useAppStore((s) => s.documents);
  const collection = useAppStore((s) => s.collections.find((c) => c.id === s.activeCollectionId) ?? null);
  const scope = useRagStore((s) => s.scope);
  const setScope = useRagStore((s) => s.setScope);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  const inScope = scope.kind === 'documents' ? docs.filter((d) => scope.ids.includes(d.id)) : docs;
  const ready = inScope.filter(isSearchable).length;
  const notReady = inScope.length - ready;
  const label = scope.kind === 'documents' ? t('scope.selected', { n: inScope.length }) : collection?.name ?? t('library.allPapers');
  const processing = inScope.filter(isProcessing).length;
  return (
    <div className="scope-bar" ref={ref} data-coach="scope">
      <span className="t-label">{t('scope.label')}</span>
      <span className="scope-chip">
        <Icon name="folder" size={13} strokeWidth={1.3} />
        <span dir="auto">{label}</span> · {t('scope.readyOf', { ready, total: inScope.length })}
      </span>
      {notReady > 0 && (
        <span className="t-meta" title={t('scope.notSearchedTitle', { processing, other: notReady - processing })}>
          {t('scope.notSearched', { n: notReady })}
        </span>
      )}
      <span style={{ flex: 1 }} />
      <button className="link-btn" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        {t('scope.change')}
      </button>
      {open && (
        <div className="menu scope-menu" role="dialog" aria-label={t('scope.change')}>
          <label className="menu-item">
            <span>
              <input type="radio" checked={scope.kind === 'collection'} onChange={() => setScope({ kind: 'collection' })} /> {t('scope.whole', { name: collection?.name ?? t('library.allPapers') })}
            </span>
          </label>
          <label className="menu-item">
            <span>
              <input type="radio" checked={scope.kind === 'documents'} onChange={() => setScope({ kind: 'documents', ids: docs.filter(isSearchable).map((d) => d.id) })} />{' '}
              {t('scope.choose')}
            </span>
          </label>
          {scope.kind === 'documents' && (
            <div className="scope-docs">
              {docs.map((d) => (
                <label key={d.id} className={`scope-doc${!d.lexicalReady ? ' disabled' : ''}`}>
                  <input
                    type="checkbox"
                    disabled={!d.lexicalReady}
                    checked={scope.ids.includes(d.id)}
                    onChange={(e) => setScope({ kind: 'documents', ids: e.target.checked ? [...scope.ids, d.id] : scope.ids.filter((x) => x !== d.id) })}
                  />
                  <span dir="auto">{d.title}</span>
                  {!d.lexicalReady && <span className="t-meta">{t(hasText(d) ? 'status.indexing' : 'scope.notReady')}</span>}
                </label>
              ))}
            </div>
          )}
          <span className="t-meta" style={{ padding: '6px 10px' }}>
            {t('scope.note', { n: formatNumber(locale, ready) })}
          </span>
        </div>
      )}
    </div>
  );
}

/** Question box. Enter sends (or Ctrl+Enter, per settings); IME composition is respected. */
export function Composer() {
  const t = useT();
  const docs = useAppStore((s) => s.documents);
  const total = useAppStore((s) => s.documentsTotal);
  const enterToSend = useAppStore((s) => s.settings?.enterToSend ?? true);
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen);
  const models = useRagStore((s) => s.models);
  const draft = useRagStore((s) => s.draft);
  const setDraft = useRagStore((s) => s.setDraft);
  const ask = useRagStore((s) => s.ask);
  const find = useRagStore((s) => s.findPassages);
  const asking = useRagStore((s) => s.asking);
  const searching = useRagStore((s) => s.searching);
  const conversation = useRagStore((s) => s.conversation);
  const scope = useRagStore((s) => s.scope);
  const reader = useAppStore((s) => s.reader);
  const ref = useRef<HTMLTextAreaElement>(null);

  const inScope = scope.kind === 'documents' ? docs.filter((d) => scope.ids.includes(d.id)) : docs;
  const ready = inScope.filter(isSearchable).length;
  const hasModel = !!models?.generation;
  const provider = models?.generation ? models.providers.find((p) => p.id === models.generation?.providerId) : undefined;
  const viewing = reader && inScope.some((d) => d.id === reader.documentId && d.lexicalReady) ? reader.page : null;
  const canSearch = ready > 0 && draft.trim().length > 0;
  const canAsk = canSearch && hasModel && !asking;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(160, el.scrollHeight)}px`;
  }, [draft]);

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key !== 'Enter' || e.nativeEvent.isComposing) return;
    const send = enterToSend ? !e.shiftKey && !e.ctrlKey : e.ctrlKey || e.metaKey;
    if (!send) return;
    e.preventDefault();
    if (canAsk) void ask(draft);
  }

  const placeholder =
    total === 0
      ? t('composer.placeholderEmpty')
      : ready === 0
        ? t('composer.notReady')
        : viewing
          ? t('composer.placeholderPage', { page: viewing })
          : conversation
            ? t('composer.followUp')
            : t('composer.placeholder');
  const reason =
    total === 0
      ? ''
      : ready === 0
        ? t('composer.notReadyReason')
        : !hasModel
          ? t('composer.noModel')
          : viewing
            ? t('composer.searchingWithPage', { n: ready, page: viewing })
            : t('composer.searching', { n: ready });

  return (
    <div className="composer-wrap" data-region="composer" tabIndex={-1}>
      <div className={`composer${ready === 0 ? ' disabled' : ''}`}>
        <label htmlFor="composer-input" className="sr-only">
          {placeholder}
        </label>
        <textarea
          id="composer-input"
          ref={ref}
          rows={1}
          value={draft}
          maxLength={4000}
          disabled={ready === 0}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={placeholder}
          dir="auto"
          aria-describedby="composer-reason"
        />
        <div className="composer-foot">
          <div className="composer-info">
            {total > 0 &&
              (hasModel && models?.generation ? (
                <button className="model-chip" onClick={() => setSettingsOpen(true)} title={t('composer.changeModel')}>
                  <ProviderLogo logo={provider ? logoFor(provider.kind, provider.baseUrl, provider.label) : undefined} size={14} />
                  <span className="t-meta">{t('composer.answeredBy')}</span>
                  <span className="ellipsis" dir="auto">
                    {provider?.label ?? ''} · {models.generation.model}
                  </span>
                </button>
              ) : (
                <button className="model-chip missing" onClick={() => setSettingsOpen(true)}>
                  <Icon name="settings" size={13} />
                  {t('composer.pickModel')}
                </button>
              ))}
            <span id="composer-reason" className="composer-reason">
              {reason}
            </span>
          </div>
          <div className="composer-buttons">
            <button className="btn" disabled={!canSearch || searching} onClick={() => void find(draft)} title={t('composer.findTitle')}>
              <Icon name="search" size={13} />
              {t('composer.find')}
            </button>
            <button className="btn btn-primary" disabled={!canAsk} onClick={() => void ask(draft)}>
              {t('composer.ask')} <span className="kbd">{enterToSend ? 'Enter' : 'Ctrl+Enter'}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
