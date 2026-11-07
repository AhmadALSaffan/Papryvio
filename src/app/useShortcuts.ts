import { useEffect } from 'react';
import { useAppStore } from './store';
import { useRagStore } from './ragStore';

const REGIONS = ['library', 'answer', 'evidence', 'composer'] as const;

/**
 * Global shortcuts: Ctrl+O import, Ctrl+N new question, Ctrl+B library, Ctrl+, settings, F6 / Shift+F6 regions.
 * Shortcuts never fire while a dialog is open or while typing (except F6).
 */
export function useShortcuts() {
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.isComposing) return;
      const s = useAppStore.getState();
      const dialogOpen = !!document.querySelector('.dialog-backdrop');
      const ctrl = e.ctrlKey || e.metaKey;

      if (e.key === 'F6' && !dialogOpen) {
        e.preventDefault();
        const current = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>('[data-region]')?.dataset.region;
        const available = REGIONS.filter((r) => document.querySelector(`[data-region="${r}"]`));
        const i = available.indexOf(current as (typeof REGIONS)[number]);
        const next = available[(i + (e.shiftKey ? available.length - 1 : 1) + available.length) % available.length];
        document.querySelector<HTMLElement>(`[data-region="${next}"]`)?.focus();
        return;
      }
      if (dialogOpen || !ctrl) return;
      const key = e.key.toLowerCase();
      if (key === 'o') {
        e.preventDefault();
        void s.startImport();
      } else if (key === 'b') {
        e.preventDefault();
        s.toggleLibrary();
      } else if (key === 'n') {
        e.preventDefault();
        useRagStore.getState().newQuestion();
      } else if (key === ',') {
        e.preventDefault();
        s.setSettingsOpen(true);
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
