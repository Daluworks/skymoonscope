/**
 * Command registry and matching logic behind the global (Cmd+K / Ctrl+K)
 * search overlay.
 *
 * Pure and DOM-free so the ranking rules can be unit tested directly.
 */

/** The default set of destinations reachable from anywhere in the app. */
const BASE_COMMANDS = [
  {
    id: 'nav-explorer',
    title: 'Explorer',
    subtitle: 'Simulate a contract call and inspect its resource cost',
    group: 'Navigation',
    href: '/?tab=explorer',
    keywords: ['result', 'simulate', 'analyze', 'home'],
  },
  {
    id: 'nav-history',
    title: 'Invocation history',
    subtitle: 'Revisit previous simulations',
    group: 'Navigation',
    href: '/?tab=history',
    keywords: ['past', 'previous', 'runs', 'recent'],
  },
  {
    id: 'nav-transactions',
    title: 'Transactions',
    subtitle: 'Browse recent on-chain transactions',
    group: 'Navigation',
    href: '/?tab=transactions',
    keywords: ['ledger', 'tx', 'onchain'],
  },
  {
    id: 'nav-schema',
    title: 'Subgraph schema',
    subtitle: 'Visualize contract calls and storage mappings',
    group: 'Navigation',
    href: '/?tab=schema',
    keywords: ['graph', 'diagram', 'nodes', 'visualizer', 'dependencies'],
  },
  {
    id: 'nav-settings',
    title: 'Settings',
    subtitle: 'Configure custom RPC and indexer endpoints',
    group: 'Preferences',
    href: '/settings',
    keywords: ['rpc', 'indexer', 'endpoint', 'preferences', 'config', 'custom'],
  },
];

/** True when a keyboard event should open the global search overlay. */
function isSearchShortcut(event) {
  if (!event || typeof event.key !== 'string') return false;
  if (!(event.metaKey || event.ctrlKey)) return false;
  // Cmd+Alt+K and friends belong to other tools.
  if (event.altKey) return false;
  return event.key.toLowerCase() === 'k';
}

/** True when a keyboard event should close the overlay. */
function isDismissShortcut(event) {
  return Boolean(event) && event.key === 'Escape';
}

function normalize(value) {
  return String(value == null ? '' : value)
    .toLowerCase()
    .trim();
}

/** Every character of `query` appears in `text`, in order. */
function isSubsequence(query, text) {
  let cursor = 0;
  for (let i = 0; i < text.length && cursor < query.length; i += 1) {
    if (text[i] === query[cursor]) cursor += 1;
  }
  return cursor === query.length;
}

/**
 * Score a command against a query. Higher is better; `0` means no match.
 *
 * Ranking, strongest first: exact title, title prefix, word-start inside the
 * title, keyword hit, substring anywhere, then a loose subsequence fallback.
 */
function scoreCommand(command, query) {
  const q = normalize(query);
  if (q === '') return 1;

  const title = normalize(command.title);
  const subtitle = normalize(command.subtitle);
  const keywords = Array.isArray(command.keywords) ? command.keywords.map(normalize) : [];

  if (title === q) return 100;
  if (title.startsWith(q)) return 90;
  if (title.split(/\s+/).some((word) => word.startsWith(q))) return 80;
  if (keywords.some((keyword) => keyword === q || keyword.startsWith(q))) return 70;
  if (title.includes(q)) return 60;
  if (subtitle.includes(q)) return 40;
  if (keywords.some((keyword) => keyword.includes(q))) return 30;
  if (isSubsequence(q, title)) return 20;

  return 0;
}

/**
 * Filter and rank commands for a query.
 *
 * Ties keep registry order so the list does not jump around while typing.
 */
function filterCommands(commands, query, options = {}) {
  const limit = typeof options.limit === 'number' ? options.limit : 10;
  const list = Array.isArray(commands) ? commands : [];

  return list
    .map((command, index) => ({ command, index, score: scoreCommand(command, query) }))
    .filter((entry) => entry.score > 0)
    .sort((a, b) => (b.score === a.score ? a.index - b.index : b.score - a.score))
    .slice(0, limit)
    .map((entry) => entry.command);
}

/**
 * Build the searchable command list, optionally extended with contextual
 * entries such as the current contract's functions.
 *
 * @param {{ functions?: Array<{name: string}>, extra?: Array<object> }} [context]
 */
function buildCommandRegistry(context = {}) {
  const functionCommands = (Array.isArray(context.functions) ? context.functions : []).map(
    (fn) => ({
      id: `fn-${fn.name}`,
      title: fn.name,
      subtitle: 'Select this contract function',
      group: 'Contract functions',
      action: 'select-function',
      payload: { name: fn.name },
      keywords: ['function', 'invoke', 'call'],
    }),
  );

  const extra = Array.isArray(context.extra) ? context.extra : [];
  return [...BASE_COMMANDS, ...functionCommands, ...extra];
}

/** Wrap-around cursor movement for arrow-key navigation. */
function moveHighlight(current, delta, length) {
  if (!Number.isFinite(length) || length <= 0) return 0;
  const next = (current + delta) % length;
  return next < 0 ? next + length : next;
}

/** Prefix for the DOM ids the search listbox gives its options. */
const SEARCH_OPTION_ID_PREFIX = 'global-search-option-';

/**
 * DOM id for a command's option element.
 *
 * The combobox points at the active option with `aria-activedescendant`, which
 * needs a stable id per option. Command ids are not guaranteed to be valid ids
 * ("fn:transfer_from"), so anything outside the safe set is folded to a dash,
 * and a missing id folds to `unknown` rather than to a bare prefix.
 */
function optionIdFor(command) {
  const raw = command && command.id != null ? String(command.id) : 'unknown';
  const safe = raw.replace(/[^A-Za-z0-9_-]/g, '-') || 'unknown';
  return `${SEARCH_OPTION_ID_PREFIX}${safe}`;
}

/**
 * Clamp a stored highlight to a result list that may have shrunk since the
 * highlight was set. Returns 0 for an empty list so callers can index safely.
 */
function normalizeHighlight(current, length) {
  if (!Number.isFinite(length) || length <= 0) return 0;
  if (!Number.isFinite(current) || current < 0) return 0;
  const whole = Math.trunc(current);
  return whole >= length ? length - 1 : whole;
}

/**
 * Resolve a listbox navigation key to an index.
 *
 * `Home` and `End` jump to the ends; anything else keeps the clamped current
 * position, so callers can pass any key without special-casing it first.
 */
function moveHighlightTo(key, current, length) {
  if (!Number.isFinite(length) || length <= 0) return 0;
  if (key === 'Home') return 0;
  if (key === 'End') return length - 1;
  return normalizeHighlight(current, length);
}

module.exports = {
  BASE_COMMANDS,
  isSearchShortcut,
  isDismissShortcut,
  scoreCommand,
  filterCommands,
  buildCommandRegistry,
  moveHighlight,
  moveHighlightTo,
  normalizeHighlight,
  optionIdFor,
  SEARCH_OPTION_ID_PREFIX,
};
