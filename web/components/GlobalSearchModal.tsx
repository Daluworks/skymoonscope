'use client';

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter } from 'next/router';
import { CornerDownLeft, Search, X } from 'lucide-react';

import {
  buildCommandRegistry,
  filterCommands,
  isDismissShortcut,
  isSearchShortcut,
  moveHighlight,
  moveHighlightTo,
  normalizeHighlight,
  optionIdFor,
} from '../lib/searchCommands';
import type { SearchCommand } from '../lib/searchCommands';
import { MOCK_CONTRACT_FUNCTIONS } from '../lib/sorobantypes';

/**
 * Custom event fired when a non-navigation command is picked, so pages can
 * react without the modal needing to know about their internal state.
 */
export const SEARCH_COMMAND_EVENT = 'sky-moon-scope:search-command';

/**
 * App-wide quick search overlay.
 *
 * Mounted once in `_app`, so Cmd+K / Ctrl+K works on every page without moving
 * the mouse to the header.
 *
 * Keyboard navigation follows the ARIA combobox + listbox pattern: the input
 * owns focus, and the highlighted option is exposed with
 * `aria-activedescendant` rather than by moving DOM focus, so screen readers
 * announce the active result as the arrow keys move through it.
 */
export function GlobalSearchModal() {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [highlight, setHighlight] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  const commands = useMemo(
    () => buildCommandRegistry({ functions: MOCK_CONTRACT_FUNCTIONS }),
    [],
  );
  const results = useMemo(() => filterCommands(commands, query), [commands, query]);

  // The list shrinks as the query narrows, so the stored index can outlive the
  // option it pointed at. Everything reads the clamped value, which keeps
  // `aria-activedescendant` from naming an option that is no longer rendered.
  const activeIndex = normalizeHighlight(highlight, results.length);
  const activeOptionId = results.length > 0 ? optionIdFor(results[activeIndex]) : undefined;

  const close = useCallback(() => {
    setOpen(false);
    setQuery('');
    setHighlight(0);
  }, []);

  const runCommand = useCallback(
    (command: SearchCommand | undefined) => {
      if (!command) return;
      close();

      if (command.href) {
        void router.push(command.href);
        return;
      }

      if (command.action && typeof window !== 'undefined') {
        window.dispatchEvent(
          new CustomEvent(SEARCH_COMMAND_EVENT, {
            detail: { action: command.action, payload: command.payload },
          }),
        );
      }
    },
    [close, router],
  );

  // Global shortcut listener — intentionally on window so it fires regardless
  // of which element currently has focus.
  // Fixed: Properly handle listener cleanup to prevent duplicates and ensure
  // the modal can be opened/closed reliably with Esc key.
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Handle search shortcut (Cmd+K or Ctrl+K)
      if (isSearchShortcut(event)) {
        event.preventDefault();
        setOpen((prev) => !prev);
        return;
      }

      // Handle dismiss shortcut (Escape) — only if modal is open
      if (isDismissShortcut(event) && open) {
        event.preventDefault();
        setOpen(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);

    // Cleanup: Remove listener when component unmounts or dependencies change
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [open]);

  // Focus the input and lock background scroll while the overlay is up.
  useEffect(() => {
    if (!open) {
      document.body.style.overflow = '';
      return;
    }

    document.body.style.overflow = 'hidden';
    const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => {
      window.clearTimeout(timer);
      document.body.style.overflow = '';
    };
  }, [open]);

  useEffect(() => {
    setHighlight(0);
  }, [query]);

  // Keep the active option on screen. The list scrolls (max-h-80), so without
  // this the highlight walks off the bottom and a keyboard-only user is
  // selecting something they cannot see.
  useEffect(() => {
    if (!open) return;
    const active = listRef.current?.querySelector('[data-active="true"]');
    active?.scrollIntoView({ block: 'nearest' });
  }, [open, activeIndex, results.length]);

  if (!open) return null;

  const handleInputKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setHighlight(moveHighlight(activeIndex, 1, results.length));
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      setHighlight(moveHighlight(activeIndex, -1, results.length));
    } else if (event.key === 'Home' || event.key === 'End') {
      // First/last result, per the listbox keyboard interaction pattern.
      event.preventDefault();
      setHighlight(moveHighlightTo(event.key, activeIndex, results.length));
    } else if (event.key === 'Enter') {
      event.preventDefault();
      runCommand(results[activeIndex]);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[100] flex items-start justify-center px-4 pt-[12vh]"
      role="dialog"
      aria-modal="true"
      aria-label="Global search"
    >
      <div
        className="fixed inset-0 bg-slate-950/80 backdrop-blur-sm"
        onClick={close}
        aria-hidden="true"
      />

      <div className="relative z-10 w-full max-w-xl overflow-hidden rounded-2xl border border-slate-700 bg-slate-900 shadow-2xl">
        <div className="flex items-center gap-3 border-b border-slate-800 px-4 py-3">
          <Search className="h-4 w-4 shrink-0 text-slate-500" aria-hidden="true" />
          <input
            ref={inputRef}
            type="text"
            role="combobox"
            aria-expanded="true"
            aria-controls="global-search-results"
            aria-activedescendant={activeOptionId}
            aria-autocomplete="list"
            placeholder="Search pages, functions and settings..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleInputKeyDown}
            className="w-full bg-transparent text-sm text-slate-100 placeholder:text-slate-500 focus:outline-none"
          />
          <button
            type="button"
            onClick={close}
            aria-label="Close search"
            className="shrink-0 rounded p-1 text-slate-500 transition-colors hover:text-slate-200"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <ul
          ref={listRef}
          id="global-search-results"
          role="listbox"
          aria-label="Search results"
          className="max-h-80 overflow-y-auto py-2"
        >
          {results.length === 0 && (
            <li className="px-4 py-6 text-center text-sm text-slate-500">
              No matches for &ldquo;{query}&rdquo;
            </li>
          )}

          {results.map((command, index) => (
            <li
              key={command.id}
              id={optionIdFor(command)}
              role="option"
              aria-selected={index === activeIndex}
              data-active={index === activeIndex ? 'true' : 'false'}
            >
              <button
                type="button"
                tabIndex={-1}
                onMouseEnter={() => setHighlight(index)}
                onClick={() => runCommand(command)}
                className={`flex w-full items-center justify-between gap-3 px-4 py-2.5 text-left transition-colors ${
                  index === activeIndex ? 'bg-cyan-500/10' : 'hover:bg-slate-800/60'
                }`}
              >
                <span className="min-w-0">
                  <span
                    className={`block truncate text-sm font-medium ${
                      index === activeIndex ? 'text-cyan-300' : 'text-slate-200'
                    }`}
                  >
                    {command.title}
                  </span>
                  {command.subtitle && (
                    <span className="block truncate text-xs text-slate-500">
                      {command.subtitle}
                    </span>
                  )}
                </span>
                <span className="shrink-0 text-[10px] uppercase tracking-wide text-slate-500">
                  {command.group}
                </span>
              </button>
            </li>
          ))}
        </ul>

        <div className="flex items-center justify-between border-t border-slate-800 px-4 py-2 text-[11px] text-slate-500">
          <span className="flex items-center gap-1.5">
            <CornerDownLeft className="h-3 w-3" /> to select &bull; ↑↓ to navigate
          </span>
          <span>esc to close</span>
        </div>
      </div>
    </div>
  );
}

export default GlobalSearchModal;
