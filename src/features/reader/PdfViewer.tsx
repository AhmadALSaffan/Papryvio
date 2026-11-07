import { useCallback, useEffect, useRef, useState } from 'react';
import type { PDFDocumentLoadingTask, PDFDocumentProxy, PDFPageProxy, PageViewport, RenderTask } from 'pdfjs-dist';
import type { Rect } from '../../shared/types/generated/Rect';
import { api } from '../../shared/ipc/commands';
import { useT } from '../../shared/i18n';
import './textLayer.css';

type PdfJs = typeof import('pdfjs-dist');
let pdfjsPromise: Promise<PdfJs> | null = null;

/** pdf.js is loaded on first use only and runs its worker from the local bundle. */
function loadPdfJs(): Promise<PdfJs> {
  pdfjsPromise ??= Promise.all([import('pdfjs-dist'), import('pdfjs-dist/build/pdf.worker.min.mjs?url')]).then(
    ([lib, worker]) => {
      lib.GlobalWorkerOptions.workerSrc = worker.default;
      return lib;
    },
  );
  return pdfjsPromise;
}

export type Zoom = 'fit' | number;

export interface ViewerHandle {
  pageCount: number;
}

export function PdfViewer({
  documentId,
  page,
  zoom,
  onPageCount,
  onPageChange,
  onZoomResolved,
  highlights = [],
  marker,
}: {
  highlights?: Rect[];
  /** Citation number drawn beside the first highlight. */
  marker?: number | undefined;
  documentId: string;
  page: number;
  zoom: Zoom;
  onPageCount: (n: number) => void;
  onPageChange: (page: number) => void;
  onZoomResolved: (scale: number) => void;
}) {
  const t = useT();
  const scrollRef = useRef<HTMLDivElement>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textRef = useRef<HTMLDivElement>(null);
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [width, setWidth] = useState(0);
  const [viewport, setViewport] = useState<PageViewport | null>(null);

  // Load the document bytes from the library copy.
  useEffect(() => {
    let cancelled = false;
    let task: PDFDocumentLoadingTask | null = null;
    setStatus('loading');
    setPdf(null);
    (async () => {
      try {
        const [lib, bytes] = await Promise.all([loadPdfJs(), api.readDocumentFile(documentId)]);
        if (cancelled) return;
        // Scripting, XFA forms and eval-based font paths stay off for untrusted files.
        task = lib.getDocument({
          data: new Uint8Array(bytes),
          enableXfa: false,
          standardFontDataUrl: '/pdf-assets/standard_fonts/',
          cMapUrl: '/pdf-assets/cmaps/',
          cMapPacked: true,
          wasmUrl: '/pdf-assets/wasm/',
        });
        const doc = await task.promise;
        if (cancelled) return;
        setPdf(doc);
        onPageCount(doc.numPages);
        setStatus('ready');
      } catch (e) {
        console.error('pdf load failed', e);
        if (!cancelled) setStatus('error');
      }
    })();
    return () => {
      cancelled = true;
      // Releases the worker-side document, canvases and caches.
      void task?.destroy();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [documentId]);

  // Track the available width for "fit width".
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const ro = new ResizeObserver(() => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => setWidth(el.clientWidth), 120);
    });
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => {
      if (timer) clearTimeout(timer);
      ro.disconnect();
    };
  }, []);

  const render = useCallback(
    async (p: PDFPageProxy, signal: { cancelled: boolean; task: RenderTask | null; textLayer: { cancel(): void } | null }) => {
      const lib = await loadPdfJs();
      const base = p.getViewport({ scale: 1 });
      const available = Math.max(200, width - 48);
      const scale = zoom === 'fit' ? available / base.width : zoom;
      onZoomResolved(scale);
      const viewport = p.getViewport({ scale });
      setViewport(viewport);
      const canvas = canvasRef.current;
      const pageDiv = pageRef.current;
      const textDiv = textRef.current;
      if (!canvas || !pageDiv || !textDiv || signal.cancelled) return;

      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.floor(viewport.width * dpr);
      canvas.height = Math.floor(viewport.height * dpr);
      canvas.style.width = `${Math.floor(viewport.width)}px`;
      canvas.style.height = `${Math.floor(viewport.height)}px`;
      pageDiv.style.width = `${Math.floor(viewport.width)}px`;
      pageDiv.style.height = `${Math.floor(viewport.height)}px`;
      pageDiv.style.setProperty('--scale-factor', String(scale));

      const task = p.render({
        canvas,
        viewport,
        transform: dpr !== 1 ? [dpr, 0, 0, dpr, 0, 0] : undefined,
      });
      signal.task = task;
      try {
        await task.promise;
      } catch {
        return; // cancelled
      }
      if (signal.cancelled) return;
      textDiv.replaceChildren();
      const layer = new lib.TextLayer({ textContentSource: p.streamTextContent(), container: textDiv, viewport });
      signal.textLayer = layer;
      try {
        await layer.render();
      } catch {
        /* cancelled */
      }
    },
    [width, zoom, onZoomResolved],
  );

  // Render the current page; prefetch its neighbours.
  useEffect(() => {
    if (!pdf || width === 0) return;
    const signal = { cancelled: false, task: null as RenderTask | null, textLayer: null as { cancel(): void } | null };
    const n = Math.min(Math.max(1, page), pdf.numPages);
    pdf
      .getPage(n)
      .then((p) => render(p, signal))
      .catch((e) => console.error('render failed', e));
    for (const m of [n - 1, n + 1]) if (m >= 1 && m <= pdf.numPages) void pdf.getPage(m);
    if (highlights.length === 0) scrollRef.current?.scrollTo({ top: 0 });
    return () => {
      signal.cancelled = true;
      signal.task?.cancel();
      signal.textLayer?.cancel();
    };
  }, [pdf, page, render, width]);

  function onKeyDown(e: React.KeyboardEvent) {
    if (!pdf) return;
    if ((e.target as HTMLElement).tagName === 'INPUT') return;
    if (e.key === 'PageDown' || (e.key === 'ArrowRight' && !e.shiftKey)) {
      if (page < pdf.numPages) {
        e.preventDefault();
        onPageChange(page + 1);
      }
    } else if (e.key === 'PageUp' || (e.key === 'ArrowLeft' && !e.shiftKey)) {
      if (page > 1) {
        e.preventDefault();
        onPageChange(page - 1);
      }
    }
  }

  return (
    <div className="pdf-scroll scroll" ref={scrollRef} tabIndex={0} onKeyDown={onKeyDown} aria-label={t('reader.title')}>
      {status === 'loading' && <div className="pdf-message">{t('reader.loading')}</div>}
      {status === 'error' && <div className="pdf-message error">{t('reader.error')}</div>}
      <div className="pdf-page" ref={pageRef} hidden={status !== 'ready'} dir="ltr">
        <canvas ref={canvasRef} />
        {viewport && highlights.length > 0 && <Highlights viewport={viewport} rects={highlights} marker={marker} scrollRef={scrollRef} />}
        <div className="textLayer" ref={textRef} />
      </div>
    </div>
  );
}

/** Evidence highlight: PDF user-space boxes mapped through the page viewport (rotation-aware). */
function Highlights({
  viewport,
  rects,
  marker,
  scrollRef,
}: {
  viewport: PageViewport;
  rects: Rect[];
  marker?: number | undefined;
  scrollRef: React.RefObject<HTMLDivElement | null>;
}) {
  const firstRef = useRef<HTMLDivElement>(null);
  const boxes = rects.map((r) => {
    const [x1, y1] = viewport.convertToViewportPoint(r.left, r.bottom) as [number, number];
    const [x2, y2] = viewport.convertToViewportPoint(r.right, r.top) as [number, number];
    return { left: Math.min(x1, x2), top: Math.min(y1, y2), width: Math.abs(x2 - x1), height: Math.abs(y2 - y1) };
  });
  useEffect(() => {
    const el = firstRef.current;
    const scroller = scrollRef.current;
    if (el && scroller) scroller.scrollTo({ top: Math.max(0, el.offsetTop - 80), behavior: 'smooth' });
  }, [viewport, rects, scrollRef]);
  return (
    <div className="hl-layer" aria-hidden="true">
      {boxes.map((b, i) => (
        <div key={i} ref={i === 0 ? firstRef : undefined} className="hl" style={{ left: b.left - 1.5, top: b.top - 1.5, width: b.width + 3, height: b.height + 3 }} />
      ))}
      {marker !== undefined && boxes[0] && (
        <span className="hl-marker" style={{ left: Math.max(2, boxes[0].left - 28), top: boxes[0].top }}>
          {marker}
        </span>
      )}
    </div>
  );
}
