import { describe, expect, it } from 'vitest';
import { CENTER_MIN, EVIDENCE_W, RAIL_W, computeMetrics } from './layout';

const base = { libraryW: 280, evidenceW: 520, evidenceHidden: false, viewport: 1440 };

describe('computeMetrics', () => {
  it('keeps three columns on a wide window', () => {
    const m = computeMetrics(base, false, false);
    expect(m).toMatchObject({ libraryW: 280, evidenceColumn: true, evidenceDrawer: false, evidenceW: 520 });
  });

  it('never squeezes the answer column below its minimum', () => {
    const m = computeMetrics({ ...base, evidenceW: 1000, viewport: 1200 }, false, true);
    expect(m.evidenceColumn).toBe(true);
    expect(1200 - m.libraryW - m.evidenceW).toBeGreaterThanOrEqual(CENTER_MIN);
  });

  it('turns the reader into a drawer when three columns no longer fit', () => {
    const narrow = { ...base, viewport: 900 };
    expect(computeMetrics(narrow, false, false)).toMatchObject({ evidenceColumn: false, evidenceDrawer: false });
    const open = computeMetrics(narrow, false, true);
    expect(open.evidenceDrawer).toBe(true);
    expect(open.evidenceW).toBeLessThanOrEqual(900 - 280 - 48);
  });

  it('uses the rail width when the library is collapsed, which leaves room for the reader again', () => {
    const m = computeMetrics({ ...base, viewport: 900 }, true, true);
    expect(m.libraryW).toBe(RAIL_W);
    expect(m.evidenceColumn).toBe(true);
    expect(m.evidenceW).toBeGreaterThanOrEqual(EVIDENCE_W.min);
  });

  it('hides the reader when the user closes the pane', () => {
    const m = computeMetrics({ ...base, evidenceHidden: true }, false, true);
    expect(m).toMatchObject({ evidenceColumn: false, evidenceDrawer: false });
  });
});
