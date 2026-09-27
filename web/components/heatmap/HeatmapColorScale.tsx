import React from 'react';
import { cn } from '../../lib/utils';

// ── Soroban Budget Limits ────────────────────────────────────────────────────

export const LIMITS = {
  CPU:          100_000_000,      // 100M instructions
  RAM:          40 * 1024 * 1024, // 40 MB
  LEDGER_READ:  150 * 1024,       // 150 KB
  LEDGER_WRITE: 100 * 1024,       // 100 KB
  TX_SIZE:      70  * 1024,       // 70 KB
};

// ── Types ────────────────────────────────────────────────────────────────────

export type FnCategory = 'auth' | 'storage' | 'compute' | 'io' | 'util';

export type RamRegion = 'heap' | 'stack' | 'host' | 'data' | 'auth' | 'buffer' | 'event';

export interface RamColors {
  hex: string;
  bg: string;
  border: string;
  text: string;
  badge: string;
}

export const RAM_COLORS: Record<RamRegion, RamColors> = {
  heap:   { hex: '#f59e0b', bg: 'bg-amber-500/20',   border: 'border-amber-500/50',  text: 'text-amber-300',  badge: 'bg-amber-900/70 text-amber-300 border-amber-700'   },
  host:   { hex: '#10b981', bg: 'bg-emerald-500/20', border: 'border-emerald-500/50',text: 'text-emerald-300',badge: 'bg-emerald-900/70 text-emerald-300 border-emerald-700'},
  stack:  { hex: '#0ea5e9', bg: 'bg-sky-500/20',     border: 'border-sky-500/50',    text: 'text-sky-300',    badge: 'bg-sky-900/70 text-sky-300 border-sky-700'           },
  data:   { hex: '#8b5cf6', bg: 'bg-violet-500/20',  border: 'border-violet-500/50', text: 'text-violet-300', badge: 'bg-violet-900/70 text-violet-300 border-violet-700'  },
  auth:   { hex: '#ec4899', bg: 'bg-pink-500/20',    border: 'border-pink-500/50',   text: 'text-pink-300',   badge: 'bg-pink-900/70 text-pink-300 border-pink-700'         },
  buffer: { hex: '#64748b', bg: 'bg-slate-600/20',   border: 'border-slate-500/50',  text: 'text-slate-400',  badge: 'bg-slate-800/70 text-slate-400 border-slate-600'      },
  event:  { hex: '#6366f1', bg: 'bg-indigo-500/20',  border: 'border-indigo-500/50', text: 'text-indigo-300', badge: 'bg-indigo-900/70 text-indigo-300 border-indigo-700'   },
};

export const READ_SHADES  = ['#06b6d4', '#0891b2', '#0e7490'] as const;
export const WRITE_SHADES = ['#f43f5e', '#e11d48', '#be123c'] as const;

export interface HotspotColors {
  bg: string;
  border: string;
  text: string;
  barHex: string;
  badge: string;
  label: string;
}

export function hotspotColors(share: number): HotspotColors {
  if (share >= 20) return { bg: 'bg-rose-500/75',   border: 'border-rose-400',   text: 'text-rose-100',   barHex: '#f43f5e', badge: 'bg-rose-900/80 text-rose-300',   label: 'CRITICAL' };
  if (share >= 10) return { bg: 'bg-orange-500/65', border: 'border-orange-400', text: 'text-orange-100', barHex: '#f97316', badge: 'bg-orange-900/80 text-orange-300', label: 'HIGH'     };
  if (share >=  5) return { bg: 'bg-amber-500/55',  border: 'border-amber-400',  text: 'text-amber-100',  barHex: '#eab308', badge: 'bg-amber-900/80 text-amber-300',   label: 'MEDIUM'   };
  if (share >=  2) return { bg: 'bg-cyan-700/45',   border: 'border-cyan-500',   text: 'text-cyan-100',   barHex: '#06b6d4', badge: 'bg-cyan-900/80 text-cyan-300',     label: 'LOW'      };
  return                  { bg: 'bg-slate-800/65',  border: 'border-slate-700',  text: 'text-slate-400',  barHex: '#475569', badge: 'bg-slate-800 text-slate-500',        label: 'TRACE'    };
}

export function statusColor(pct: number) {
  if (pct > 80) return { text: 'text-rose-400',  ring: '#f43f5e', glow: 'drop-shadow-[0_0_6px_rgba(244,63,94,0.4)]'  };
  if (pct > 50) return { text: 'text-amber-400', ring: '#eab308', glow: 'drop-shadow-[0_0_6px_rgba(234,179,8,0.4)]'  };
  return                { text: 'text-cyan-400',  ring: '#06b6d4', glow: 'drop-shadow-[0_0_6px_rgba(6,182,212,0.4)]'  };
}

export const CATEGORY_STYLE: Record<FnCategory, { label: string; cls: string }> = {
  auth:    { label: 'AUTH',    cls: 'text-violet-400 border-violet-700 bg-violet-950/60' },
  storage: { label: 'STORAGE', cls: 'text-blue-400   border-blue-700   bg-blue-950/60'  },
  compute: { label: 'COMPUTE', cls: 'text-orange-400 border-orange-700 bg-orange-950/60'},
  io:      { label: 'I/O',     cls: 'text-green-400  border-green-700  bg-green-950/60' },
  util:    { label: 'UTIL',    cls: 'text-slate-400  border-slate-600  bg-slate-800/60' },
};

export const HOTSPOT_SCALE_ITEMS = [
  { label: '≥20% Critical', bg: 'bg-rose-500/75',   border: 'border-rose-400'   },
  { label: '10–20% High',   bg: 'bg-orange-500/65', border: 'border-orange-400' },
  { label: '5–10% Medium',  bg: 'bg-amber-500/55',  border: 'border-amber-400'  },
  { label: '2–5% Low',      bg: 'bg-cyan-700/45',   border: 'border-cyan-500'   },
  { label: '<2% Trace',     bg: 'bg-slate-800/65',  border: 'border-slate-700'  },
];

export const MATRIX_SCALE_ITEMS = [
  { label: 'Optimal (<20%)',   bg: 'bg-emerald-950', border: 'border-emerald-500/20' },
  { label: 'Normal (20%-50%)', bg: 'bg-cyan-950',    border: 'border-cyan-500/40'    },
  { label: 'Warning (50%-80%)',bg: 'bg-amber-500/30',border: 'border-amber-400/40'  },
  { label: 'Critical (>80%)',  bg: 'bg-rose-500/80', border: 'border-rose-400/80'   },
];

export interface ColorScaleItem {
  label: string;
  bg: string;
  border?: string;
  shadow?: string;
}

export interface HeatmapColorScaleProps {
  variant?: 'hotspot' | 'matrix' | 'custom';
  items?: ColorScaleItem[];
  title?: string;
  className?: string;
}

export function HeatmapColorScale({
  variant = 'hotspot',
  items,
  title,
  className,
}: HeatmapColorScaleProps) {
  const displayItems = items ?? (variant === 'matrix' ? MATRIX_SCALE_ITEMS : HOTSPOT_SCALE_ITEMS);

  return (
    <div className={cn('flex flex-wrap gap-x-4 gap-y-1 text-[9px] font-mono text-slate-500', className)}>
      {title && <span className="uppercase text-slate-400 mr-1 self-center">{title}:</span>}
      {displayItems.map((e) => (
        <span key={e.label} className="flex items-center gap-1">
          <span
            className={cn(
              'inline-block w-2.5 h-2.5 rounded border',
              e.bg,
              e.border ?? 'border-transparent',
              e.shadow,
            )}
          />
          {e.label}
        </span>
      ))}
    </div>
  );
}
