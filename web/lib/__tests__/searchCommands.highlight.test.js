// Keyboard-navigation helpers behind GlobalSearchModal (issue #035).
// Runs with: node --test lib/__tests__/*.test.js
//
// The modal exposes its highlighted result with `aria-activedescendant`, which
// means two things have to line up exactly for a screen reader to announce
// anything:
//   1. every rendered option needs the id the combobox points at, and
//   2. that id has to belong to the result actually highlighted — not to a
//      stale index left over from a wider result list.
// These tests pin both, plus the Home/End keys the listbox pattern expects.

'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const {
  SEARCH_OPTION_ID_PREFIX,
  buildCommandRegistry,
  filterCommands,
  moveHighlight,
  moveHighlightTo,
  normalizeHighlight,
  optionIdFor,
} = require('../searchCommands.js');

const COMMANDS = buildCommandRegistry();

test('optionIdFor derives a stable id from the command id', () => {
  const command = { id: 'nav-explorer' };
  assert.equal(optionIdFor(command), `${SEARCH_OPTION_ID_PREFIX}nav-explorer`);
  assert.equal(optionIdFor(command), optionIdFor({ id: 'nav-explorer' }));
});

test('optionIdFor produces DOM-safe ids for awkward command ids', () => {
  const ids = ['fn:transfer_from', 'a b', 'c#d', 'e.f', 'g[h]'];
  for (const id of ids) {
    const optionId = optionIdFor({ id });
    assert.match(optionId, /^[A-Za-z0-9_-]+$/);
    assert.ok(optionId.startsWith(SEARCH_OPTION_ID_PREFIX));
  }
  assert.equal(optionIdFor({ id: 'fn:transfer_from' }), `${SEARCH_OPTION_ID_PREFIX}fn-transfer_from`);
});

test('optionIdFor never returns a bare prefix for a missing id', () => {
  assert.equal(optionIdFor(undefined), `${SEARCH_OPTION_ID_PREFIX}unknown`);
  assert.equal(optionIdFor({}), `${SEARCH_OPTION_ID_PREFIX}unknown`);
});

test('every rendered result has a unique id to point aria-activedescendant at', () => {
  const results = filterCommands(COMMANDS, 'e');
  const ids = results.map((command) => optionIdFor(command));
  assert.ok(results.length > 0);
  assert.equal(new Set(ids).size, ids.length);
});

test('the id of the active result matches the highlighted index', () => {
  const results = filterCommands(COMMANDS, '');
  for (let index = 0; index < results.length; index += 1) {
    const activeIndex = normalizeHighlight(index, results.length);
    assert.equal(optionIdFor(results[activeIndex]), optionIdFor(results[index]));
  }
});

test('Home and End jump to the first and last result', () => {
  const results = filterCommands(COMMANDS, '');
  assert.ok(results.length > 2);
  assert.equal(moveHighlightTo('Home', 3, results.length), 0);
  assert.equal(moveHighlightTo('End', 0, results.length), results.length - 1);
});

test('Home and End are safe on an empty result list', () => {
  assert.equal(moveHighlightTo('Home', 4, 0), 0);
  assert.equal(moveHighlightTo('End', 4, 0), 0);
  assert.equal(moveHighlightTo('End', 0, Number.NaN), 0);
});

test('normalizeHighlight clamps an index left over from a longer list', () => {
  // The query narrowed from 5 results to 2, so the stored index of 4 no longer
  // has an option to point at.
  assert.equal(normalizeHighlight(4, 2), 1);
  assert.equal(normalizeHighlight(1, 2), 1);
  assert.equal(normalizeHighlight(0, 0), 0);
  assert.equal(normalizeHighlight(-3, 5), 0);
  assert.equal(normalizeHighlight(Number.NaN, 5), 0);
  assert.equal(normalizeHighlight(2.7, 5), 2);
});

test('arrow navigation still wraps in both directions', () => {
  assert.equal(moveHighlight(0, -1, 3), 2);
  assert.equal(moveHighlight(2, 1, 3), 0);
  assert.equal(moveHighlight(1, 1, 3), 2);
});

test('arrow navigation is a no-op on an empty list', () => {
  assert.equal(moveHighlight(0, 1, 0), 0);
  assert.equal(moveHighlight(0, -1, 0), 0);
});

test('the sequence End then ArrowUp stays inside the list', () => {
  const results = filterCommands(COMMANDS, '');
  let highlight = moveHighlightTo('End', 0, results.length);
  assert.equal(highlight, results.length - 1);
  highlight = moveHighlight(highlight, -1, results.length);
  assert.equal(highlight, results.length - 2);
});
