import React from 'react';
import { ZoomIn, ZoomOut, RotateCcw } from 'lucide-react';
import { cn } from '../../lib/utils';

export interface HeatmapZoomControlsProps {
  zoom?: number;
  onZoomChange?: (newZoom: number) => void;
  onZoomIn?: () => void;
  onZoomOut?: () => void;
  onResetZoom?: () => void;
  minZoom?: number;
  maxZoom?: number;
  step?: number;
  className?: string;
  showPercentage?: boolean;
}

export function HeatmapZoomControls({
  zoom = 1,
  onZoomChange,
  onZoomIn,
  onZoomOut,
  onResetZoom,
  minZoom = 0.5,
  maxZoom = 2,
  step = 0.1,
  className,
  showPercentage = true,
}: HeatmapZoomControlsProps) {
  const handleZoomIn = () => {
    if (onZoomIn) {
      onZoomIn();
    } else if (onZoomChange) {
      onZoomChange(Math.min(maxZoom, Math.round((zoom + step) * 100) / 100));
    }
  };

  const handleZoomOut = () => {
    if (onZoomOut) {
      onZoomOut();
    } else if (onZoomChange) {
      onZoomChange(Math.max(minZoom, Math.round((zoom - step) * 100) / 100));
    }
  };

  const handleReset = () => {
    if (onResetZoom) {
      onResetZoom();
    } else if (onZoomChange) {
      onZoomChange(1);
    }
  };

  const isMin = zoom <= minZoom;
  const isMax = zoom >= maxZoom;
  const isDefault = zoom === 1;

  return (
    <div
      className={cn(
        'inline-flex items-center gap-1 bg-slate-950/80 p-1 rounded-lg border border-slate-800/80 shadow-inner select-none font-mono text-xs',
        className,
      )}
      role="group"
      aria-label="Heatmap zoom controls"
    >
      <button
        type="button"
        onClick={handleZoomOut}
        disabled={isMin}
        aria-label="Zoom out"
        title="Zoom out"
        className={cn(
          'p-1.5 rounded-md transition-colors text-slate-400 hover:text-slate-200 hover:bg-slate-800/60',
          'disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-slate-400',
        )}
      >
        <ZoomOut className="h-3.5 w-3.5" />
      </button>

      {showPercentage && (
        <span
          className="px-2 py-0.5 text-[10px] text-slate-300 font-bold min-w-[42px] text-center"
          aria-live="polite"
        >
          {Math.round(zoom * 100)}%
        </span>
      )}

      <button
        type="button"
        onClick={handleZoomIn}
        disabled={isMax}
        aria-label="Zoom in"
        title="Zoom in"
        className={cn(
          'p-1.5 rounded-md transition-colors text-slate-400 hover:text-slate-200 hover:bg-slate-800/60',
          'disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-slate-400',
        )}
      >
        <ZoomIn className="h-3.5 w-3.5" />
      </button>

      <button
        type="button"
        onClick={handleReset}
        disabled={isDefault}
        aria-label="Reset zoom"
        title="Reset zoom to 100%"
        className={cn(
          'p-1.5 rounded-md transition-colors text-slate-400 hover:text-slate-200 hover:bg-slate-800/60 ml-0.5 border-l border-slate-800/60',
          'disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-slate-400',
        )}
      >
        <RotateCcw className="h-3 w-3" />
      </button>
    </div>
  );
}
