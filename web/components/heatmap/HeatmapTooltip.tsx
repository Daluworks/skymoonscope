import React from 'react';
import { Info } from 'lucide-react';
import { cn } from '../../lib/utils';
import {
  LIMITS,
  hotspotColors,
  CATEGORY_STYLE,
} from './HeatmapColorScale';
import type { CpuHotspotCell } from './HeatmapCell';

export function fmtInstr(n: number): string {
  return new Intl.NumberFormat('en-US', { notation: 'compact', compactDisplay: 'short' }).format(n);
}

export interface HeatmapTooltipProps {
  hoveredCell: CpuHotspotCell | null;
  hotspotCells?: CpuHotspotCell[];
  isLiveData?: boolean;
  className?: string;
}

export function HeatmapTooltip({
  hoveredCell,
  hotspotCells = [],
  isLiveData = false,
  className,
}: HeatmapTooltipProps) {
  const top3Hotspots = hotspotCells.slice(0, 3);

  return (
    <div
      className={cn(
        'bg-slate-950/40 border border-slate-800/70 rounded-xl p-4 shadow-sm flex flex-col justify-between min-h-[220px]',
        className,
      )}
    >
      <div>
        <span className="text-[9px] font-bold text-slate-500 uppercase tracking-widest font-mono">
          HOTSPOT INSPECTOR
        </span>

        {hoveredCell ? (
          <div className="mt-3 space-y-3">
            {/* Full qualified name */}
            <div>
              <span className="text-[8px] text-slate-500 font-mono uppercase block mb-1">FUNCTION</span>
              <code className="text-[11px] font-mono text-slate-100 break-all bg-slate-900 border border-slate-800 rounded px-2 py-1.5 block leading-snug">
                {hoveredCell.fnName}
              </code>
            </div>

            {/* Category */}
            <div className="flex items-center gap-2">
              <span className="text-[8px] text-slate-500 font-mono uppercase">CATEGORY</span>
              <span
                className={cn(
                  'text-[9px] font-mono font-bold rounded px-1.5 py-0.5 border',
                  CATEGORY_STYLE[hoveredCell.category].cls,
                )}
              >
                {CATEGORY_STYLE[hoveredCell.category].label}
              </span>
            </div>

            {/* Stat grid */}
            <div className="grid grid-cols-2 gap-2">
              <div className="bg-slate-900 border border-slate-800 rounded p-2">
                <span className="text-[8px] font-mono text-slate-500 uppercase block">CPU SHARE</span>
                <span className="text-sm font-mono font-black text-slate-100 mt-0.5 block">
                  {hoveredCell.cpuShare.toFixed(1)}%
                </span>
              </div>
              <div className="bg-slate-900 border border-slate-800 rounded p-2">
                <span className="text-[8px] font-mono text-slate-500 uppercase block">INSTRUCTIONS</span>
                <span className="text-sm font-mono font-black text-slate-100 mt-0.5 block">
                  {fmtInstr(hoveredCell.cpuInstructions)}
                </span>
              </div>

              {/* Budget bar */}
              <div className="col-span-2 bg-slate-900 border border-slate-800 rounded p-2">
                <span className="text-[8px] font-mono text-slate-500 uppercase block mb-1">OF 100M BUDGET</span>
                <div className="h-1.5 w-full bg-black/40 rounded-full overflow-hidden">
                  <div
                    className="h-full rounded-full transition-all duration-700"
                    style={{
                      width: `${Math.min((hoveredCell.cpuInstructions / LIMITS.CPU) * 100, 100)}%`,
                      backgroundColor: hotspotColors(hoveredCell.cpuShare).barHex,
                    }}
                  />
                </div>
                <span className="text-[9px] font-mono text-slate-400 mt-0.5 block">
                  {((hoveredCell.cpuInstructions / LIMITS.CPU) * 100).toFixed(2)}%
                </span>
              </div>
            </div>

            {/* Severity pill */}
            <div
              className={cn(
                'rounded px-2 py-1 text-[9px] font-mono font-bold border text-center',
                hotspotColors(hoveredCell.cpuShare).badge,
                hotspotColors(hoveredCell.cpuShare).border,
              )}
            >
              SEVERITY: {hotspotColors(hoveredCell.cpuShare).label}
            </div>
          </div>
        ) : isLiveData ? (
          <div className="mt-4">
            <p className="text-xs text-slate-400 font-bold">Hover a cell for details</p>
            <p className="text-[11px] text-slate-600 mt-2 leading-relaxed">
              Each cell maps a contract function to its estimated CPU instruction cost.
              Brighter cells are hotter. Pulsing cells are critical hotspots.
            </p>

            <div className="mt-4 space-y-1.5">
              <span className="text-[9px] font-mono text-slate-500 uppercase block">Top Hotspots</span>
              {top3Hotspots.map((c, i) => {
                const clr = hotspotColors(c.cpuShare);
                return (
                  <div
                    key={c.id}
                    className={cn('flex items-center gap-2 rounded px-2 py-1.5 border', clr.bg, clr.border)}
                  >
                    <span className={cn('text-[8px] font-mono font-black w-4 shrink-0', clr.text)}>#{i + 1}</span>
                    <span className={cn('text-[10px] font-mono flex-1 truncate', clr.text)}>{c.displayName}</span>
                    <span className={cn('text-[9px] font-mono font-bold shrink-0', clr.text)}>
                      {c.cpuShare.toFixed(0)}%
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        ) : (
          <div className="mt-4">
            <h4 className="text-sm font-bold text-slate-400">Hover over matrix core blocks</h4>
            <p className="text-xs text-slate-500 mt-2 leading-relaxed">
              Each tile in this 6x6 grid maps a segment of your contract&apos;s resources. Highly optimized structures
              keep blocks within deep teal (Optimal). High-load areas transition into orange (Warning) and red
              (Critical).
            </p>
            <div className="mt-6 flex flex-wrap gap-4 text-[10px] font-mono text-slate-500">
              <div className="flex items-center gap-1.5">
                <div className="w-2.5 h-2.5 rounded bg-emerald-950 border border-emerald-500/20" /> Optimal (&lt;20%)
              </div>
              <div className="flex items-center gap-1.5">
                <div className="w-2.5 h-2.5 rounded bg-cyan-950 border border-cyan-500/40" /> Normal (20%-50%)
              </div>
              <div className="flex items-center gap-1.5">
                <div className="w-2.5 h-2.5 rounded bg-amber-500/30 border border-amber-400/40" /> Warning (50%-80%)
              </div>
              <div className="flex items-center gap-1.5">
                <div className="w-2.5 h-2.5 rounded bg-rose-500/80 border-rose-400/80 shadow-[0_0_6px_rgba(244,63,94,0.4)]" /> Critical (&gt;80%)
              </div>
            </div>
          </div>
        )}
      </div>

      <div className="border-t border-slate-900 pt-2 mt-4 text-[9px] font-mono text-slate-600 flex items-center justify-between">
        <span>{isLiveData ? 'LIVE DATA' : 'ESTIMATED'}</span>
        <span className="flex items-center gap-1">
          <Info className="h-3 w-3" /> {hotspotCells.length} functions
        </span>
      </div>
    </div>
  );
}
