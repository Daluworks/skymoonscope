// heatmap.test.cjs — unit tests for extracted heatmap components & logic
// Runs with: node --test ./components/heatmap.test.cjs

'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

// Test data limits matching Soroban budgets
const LIMITS = {
  CPU: 100_000_000,
  RAM: 40 * 1024 * 1024,
  LEDGER_READ: 150 * 1024,
  LEDGER_WRITE: 100 * 1024,
  TX_SIZE: 70 * 1024,
};

function hotspotColors(share) {
  if (share >= 20) return { label: 'CRITICAL', barHex: '#f43f5e' };
  if (share >= 10) return { label: 'HIGH', barHex: '#f97316' };
  if (share >= 5)  return { label: 'MEDIUM', barHex: '#eab308' };
  if (share >= 2)  return { label: 'LOW', barHex: '#06b6d4' };
  return { label: 'TRACE', barHex: '#475569' };
}

function statusColor(pct) {
  if (pct > 80) return { ring: '#f43f5e' };
  if (pct > 50) return { ring: '#eab308' };
  return { ring: '#06b6d4' };
}

function fmtInstr(n) {
  return new Intl.NumberFormat('en-US', { notation: 'compact', compactDisplay: 'short' }).format(n);
}

function createZoomState(initial = 1, min = 0.5, max = 2, step = 0.1) {
  let zoom = initial;
  return {
    get zoom() { return zoom; },
    zoomIn() {
      zoom = Math.min(max, Math.round((zoom + step) * 100) / 100);
      return zoom;
    },
    zoomOut() {
      zoom = Math.max(min, Math.round((zoom - step) * 100) / 100);
      return zoom;
    },
    reset() {
      zoom = 1;
      return zoom;
    },
  };
}

// ── Tests ───────────────────────────────────────────────────────────────────

test('HeatmapColorScale: assigns correct severity threshold labels', () => {
  assert.equal(hotspotColors(25).label, 'CRITICAL');
  assert.equal(hotspotColors(20).label, 'CRITICAL');
  assert.equal(hotspotColors(15).label, 'HIGH');
  assert.equal(hotspotColors(10).label, 'HIGH');
  assert.equal(hotspotColors(7).label, 'MEDIUM');
  assert.equal(hotspotColors(5).label, 'MEDIUM');
  assert.equal(hotspotColors(3).label, 'LOW');
  assert.equal(hotspotColors(2).label, 'LOW');
  assert.equal(hotspotColors(1).label, 'TRACE');
});

test('HeatmapColorScale: statusColor accurately categorizes percentages', () => {
  assert.equal(statusColor(85).ring, '#f43f5e');
  assert.equal(statusColor(60).ring, '#eab308');
  assert.equal(statusColor(30).ring, '#06b6d4');
});

test('HeatmapZoomControls: zoomIn, zoomOut, and reset adjust levels within bounds', () => {
  const controls = createZoomState(1, 0.5, 1.5, 0.1);
  assert.equal(controls.zoom, 1);

  controls.zoomIn();
  assert.equal(controls.zoom, 1.1);

  controls.zoomIn();
  assert.equal(controls.zoom, 1.2);

  controls.zoomOut();
  assert.equal(controls.zoom, 1.1);

  controls.reset();
  assert.equal(controls.zoom, 1);
});

test('HeatmapZoomControls: clamps to min and max boundaries', () => {
  const controls = createZoomState(1.4, 0.5, 1.5, 0.1);

  controls.zoomIn();
  assert.equal(controls.zoom, 1.5);

  // Exceeds max, should clamp at 1.5
  controls.zoomIn();
  assert.equal(controls.zoom, 1.5);

  // Zoom all the way down
  for (let i = 0; i < 20; i++) {
    controls.zoomOut();
  }
  assert.equal(controls.zoom, 0.5);
});

test('HeatmapTooltip: instruction formatting works correctly', () => {
  assert.equal(fmtInstr(1000), '1K');
  assert.equal(fmtInstr(50000000), '50M');
});
