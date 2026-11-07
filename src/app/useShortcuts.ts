import { useEffect } from 'react';
import { useAppStore } from './store';
import { useRagStore } from './ragStore';
import { useLayoutStore, ZOOM } from './layout';

const REGIONS = ['library', 'answer', 'evidence', 'composer'] as const;

/**
 * Global shortcuts: Ctrl+O import, Ctrl+N new question, Ctrl+B library, Ctrl+J reader, Ctrl+, settings,
 * Ctrl+= / Ctrl+- / Ctrl+0 interface zoom, Ctrl+/ or F1 shortcuts, F6 / Shift+F6 regions.
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
      if (e.key === 'F1' && !dialogOpen) {
        e.preventDefault();
        useLayoutStore.getState().setShortcutsOpen(true);
        return;
      }
      if (!ctrl) return;
      const key = e.key.toLowerCase();
      const layout = useLayoutStore.getState();
      // Zoom works everywhere, dialogs included, so a too-small dialog can be enlarged.
      if (key === '=' || key === '+' || e.code === 'Equal' || e.code === 'NumpadAdd') {
        e.preventDefault();
        layout.zoomBy(ZOOM.step);
        return;
      }
      if (key === '-' || e.code === 'Minus' || e.code === 'NumpadSubtract') {
        e.preventDefault();
        layout.zoomBy(-ZOOM.step);
        return;
      }
      if (key === '0' || e.code === 'Digit0' || e.code === 'Numpad0') {
        e.preventDefault();
        layout.setZoom(1);
        return;
      }
      if (dialogOpen) return;
      if (key === '/' || e.code === 'Slash') {
        e.preventDefault();
        layout.setShortcutsOpen(true);
        return;
      }
      if (key === 'j' || e.code === 'KeyJ') {
        e.preventDefault();
        layout.toggleEvidence();
        return;
      }
      if (key === 'o' || e.code === 'KeyO') {
        e.preventDefault();
        void s.startImport();
      } else if (key === 'b' || e.code === 'KeyB') {
        e.preventDefault();
        s.toggleLibrary();
      } else if (key === 'n' || e.code === 'KeyN') {
        e.preventDefault();
        useRagStore.getState().newQuestion();
      } else if (key === ',' || e.code === 'Comma') {
        e.preventDefault();
        s.setSettingsOpen(true);
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
