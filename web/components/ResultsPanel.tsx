import React from 'react';
import dynamic from 'next/dynamic';
import { useSimulation } from '../context/SimulationContext';
import { ResultViewerSkeleton } from './ResultViewerSkeleton';
import { NutritionLabelSkeleton } from './NutritionLabelSkeleton';
import { ResultViewer } from './Resultviewer';
import { ResourceHeatmap } from './ResourceHeatmap';
import { NutritionLabel } from './NutritionLabel';
import { GasUsageChart } from './GasUsageChart';
import { FeeEstimationPreview } from './FeeEstimationPreview';
import { TransactionHistoryTable } from './TransactionHistoryTable';
import { LiquidityPoolAnalytics } from './LiquidityPoolAnalytics';
import { InvocationHistory } from './InnovocationHistory';

const SchemaVisualizer = dynamic(
  () => import("./SchemaVisualizer").then((mod) => mod.SchemaVisualizer),
  {
    ssr: false,
    loading: () => (
      <div className="h-[420px] animate-pulse rounded-2xl border border-slate-800 bg-slate-900/60" />
    ),
  },
);

export function ResultsPanel() {
  const {
    tab,
    setTab,
    loading,
    currentResult,
    analysisReport,
    handleClearAnalysis,
    history,
    mockTransactions,
    transactionFilter,
    handleTransactionStatusFilterChange,
    handleTransactionFunctionFilterChange,
    setCurrentResult,
  } = useSimulation();

  return (
    <div>
      {/* Tabs Header */}
      <div
        style={{
          display: 'flex',
          borderBottom: '1px solid #30363d',
          marginBottom: '24px',
          backgroundColor: '#161b22',
          borderRadius: '8px 8px 0 0',
        }}
      >
        <button
          onClick={() => setTab('explorer')}
          style={{
            flex: 1,
            padding: '12px 16px',
            backgroundColor: 'transparent',
            border: 'none',
            borderBottom: tab === 'explorer' ? '2px solid #00d9ff' : '2px solid transparent',
            cursor: 'pointer',
            fontSize: '14px',
            fontWeight: tab === 'explorer' ? '600' : '500',
            color: tab === 'explorer' ? '#00d9ff' : '#8b949e',
            transition: 'color 0.2s, border-bottom-color 0.2s',
          }}
        >
          Result
        </button>
        <button
          onClick={() => setTab('history')}
          style={{
            flex: 1,
            padding: '12px 16px',
            backgroundColor: 'transparent',
            border: 'none',
            borderBottom: tab === 'history' ? '2px solid #00d9ff' : '2px solid transparent',
            cursor: 'pointer',
            fontSize: '14px',
            fontWeight: tab === 'history' ? '600' : '500',
            color: tab === 'history' ? '#00d9ff' : '#8b949e',
            transition: 'color 0.2s, border-bottom-color 0.2s',
          }}
        >
          History ({history.length})
        </button>
      </div>

      {/* Tab Content Body */}
      <div
        style={{
          backgroundColor: '#0d1117',
          borderRadius: '0 0 8px 8px',
          padding: '24px',
          border: '1px solid #30363d',
          borderTop: 'none',
        }}
      >
        {tab === 'explorer' ? (
          loading ? (
            <>
              <ResultViewerSkeleton />
              <div className="mt-4">
                <NutritionLabelSkeleton />
              </div>
            </>
          ) : currentResult ? (
            <>
              <ResultViewer result={currentResult} />
              {analysisReport && (
                <div className="mt-4 flex flex-col gap-4">
                  <ResourceHeatmap resourceCost={{
                    cpu_instructions: analysisReport.cpu_instructions,
                    ram_bytes: analysisReport.ram_bytes,
                    ledger_read_bytes: analysisReport.ledger_read_bytes,
                    ledger_write_bytes: analysisReport.ledger_write_bytes,
                    transaction_size_bytes: analysisReport.transaction_size_bytes,
                    cost_stroops: analysisReport.cost_stroops,
                    state_snapshot: currentResult.stateSnapshot
                  }} />
                  <div className="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2">
                    <NutritionLabel
                      cpu_instructions={analysisReport.cpu_instructions}
                      ram_bytes={analysisReport.ram_bytes}
                      ledger_read_bytes={analysisReport.ledger_read_bytes}
                      ledger_write_bytes={analysisReport.ledger_write_bytes}
                      transaction_size_bytes={analysisReport.transaction_size_bytes}
                    />
                    <GasUsageChart
                      cpu_instructions={analysisReport.cpu_instructions}
                      ram_bytes={analysisReport.ram_bytes}
                      ledger_read_bytes={analysisReport.ledger_read_bytes}
                      ledger_write_bytes={analysisReport.ledger_write_bytes}
                      transaction_size_bytes={analysisReport.transaction_size_bytes}
                      cost_stroops={analysisReport.cost_stroops}
                      testnetAverages={analysisReport.testnet_averages}
                    />
                  </div>
                </div>
              )}
              {analysisReport && analysisReport.cost_stroops !== undefined && (
                <div className="mt-4">
                  <FeeEstimationPreview
                    costStroops={analysisReport.cost_stroops}
                    loading={loading}
                  />
                </div>
              )}
              <button
                type="button"
                onClick={handleClearAnalysis}
                className="mt-4 px-4 py-2 bg-slate-800 text-slate-300 rounded hover:bg-slate-700 transition"
              >
                Clear analysis
              </button>
            </>
          ) : (
            <p className="text-slate-500 text-center py-8">
              Run an analysis to see results
            </p>
          )
        ) : tab === 'schema' ? (
          <SchemaVisualizer report={analysisReport} />
        ) : tab === 'transactions' ? (
          <TransactionHistoryTable
            transactions={mockTransactions}
            filter={transactionFilter}
            onStatusFilterChange={handleTransactionStatusFilterChange}
            onFunctionFilterChange={handleTransactionFunctionFilterChange}
          />
        ) : tab === 'analytics' ? (
          <LiquidityPoolAnalytics />
        ) : (
          <InvocationHistory onSelectResult={(result) => {
            setCurrentResult(result);
            setTab('explorer');
          }} />
        )}
      </div>

      {/* Info Cards */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
          gap: '16px',
          marginTop: '24px',
        }}
      >
        <div
          style={{
            backgroundColor: '#161b22',
            borderRadius: '8px',
            padding: '16px',
            borderLeft: '4px solid #00d9ff',
          }}
        >
          <h3 style={{ margin: '0 0 8px 0', fontSize: '14px', fontWeight: '600', color: '#00d9ff' }}>
            Simulate
          </h3>
          <p style={{ margin: '0', fontSize: '13px', color: '#8b949e' }}>
            Preview contract execution without signing or spending XLM
          </p>
        </div>
        <div
          style={{
            backgroundColor: '#161b22',
            borderRadius: '8px',
            padding: '16px',
            borderLeft: '4px solid #a371f7',
          }}
        >
          <h3 style={{ margin: '0 0 8px 0', fontSize: '14px', fontWeight: '600', color: '#a371f7' }}>
            Invoke
          </h3>
          <p style={{ margin: '0', fontSize: '13px', color: '#8b949e' }}>
            Execute real transactions via your connected wallet (Freighter/xBull)
          </p>
        </div>
        <div
          style={{
            backgroundColor: '#161b22',
            borderRadius: '8px',
            padding: '16px',
            borderLeft: '4px solid #fb8500',
          }}
        >
          <h3 style={{ margin: '0 0 8px 0', fontSize: '14px', fontWeight: '600', color: '#fb8500' }}>
            History
          </h3>
          <p style={{ margin: '0', fontSize: '13px', color: '#8b949e' }}>
            Track all function calls with full details and resource costs
          </p>
        </div>
      </div>
    </div>
  );
}
