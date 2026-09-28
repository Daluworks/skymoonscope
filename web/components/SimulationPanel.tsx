import React from 'react';
import { useSimulation } from '../context/SimulationContext';
import { ErrorBoundary } from './ErrorBoundary';
import { UploadZone } from './upload-zone';
import { GasGolfingSuggestionsTable } from './GasGolfingSuggestionsTable';
import { WalletBalanceCard } from './WalletBalanceCard';
import { FunctionSidebar } from './FunctionSidebar';
import { CopyButton } from './CopyButton';
import { ContractInteraction } from './ContractInteraction';
import { apiUrl } from '../lib/api';
import { MOCK_CONTRACT_FUNCTIONS } from '../lib/sorobantypes';

export function SimulationPanelTop() {
  const {
    uploadResetKey,
    setWasmFile,
    setWasmData,
    setCurrentResult,
    setGasGolfingSuggestions,
    setGasGolfingError,
    handleWasmReady,
    handleSimulate,
  } = useSimulation();

  function arrayBufferToBase64(buffer: ArrayBuffer): string {
    let binary = '';
    const bytes = new Uint8Array(buffer);
    const len = bytes.byteLength;
    for (let i = 0; i < len; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    return typeof window !== 'undefined' ? window.btoa(binary) : Buffer.from(binary, 'binary').toString('base64');
  }

  return (
    <div
      style={{
        backgroundColor: '#161b22',
        borderRadius: '12px',
        padding: '28px',
        marginBottom: '24px',
        border: '1px solid #30363d',
      }}
    >
      <div style={{ marginBottom: '16px' }}>
        <h2 style={{ margin: '0 0 4px 0', fontSize: '16px', fontWeight: '600', color: '#c9d1d9' }}>
          Upload Contract
        </h2>
        <p style={{ margin: '0', fontSize: '13px', color: '#8b949e' }}>
          Drop a compiled Soroban contract (.wasm) to analyze its resource usage
        </p>
      </div>

      <ErrorBoundary
        fallback={(error, reset) => (
          <div className="rounded-lg border border-red-800/60 bg-red-950/30 p-6 text-center text-red-100">
            <p className="text-sm font-semibold">Upload failed unexpectedly</p>
            <p className="mx-auto mt-2 max-w-md text-xs leading-relaxed text-red-200/80">
              {error.message}
            </p>
            <button
              type="button"
              onClick={reset}
              className="mt-4 rounded-md border border-red-700/70 px-4 py-2 text-sm text-red-100 hover:bg-red-900/40"
            >
              Try another file
            </button>
          </div>
        )}
      >
        <UploadZone
          key={uploadResetKey}
          backendUrl={apiUrl('/analyze/wasm')}
          enableBackendValidation={true}
          onFileReady={async (file) => {
            setWasmFile(file);
            const arrayBuffer = await file.arrayBuffer();
            const base64 = arrayBufferToBase64(arrayBuffer);
            setWasmData(base64);

            await handleWasmReady(file);
            await handleSimulate({}, base64);
          }}
          onReset={() => {
            setWasmFile(null);
            setWasmData(null);
            setCurrentResult(null);
            setGasGolfingSuggestions([]);
            setGasGolfingError(null);
          }}
        />
      </ErrorBoundary>
    </div>
  );
}

export function SimulationPanelSidebar() {
  const {
    gasGolfingLoading,
    gasGolfingError,
    gasGolfingSuggestions,
    contractId,
    setContractId,
    wasmFile,
    selectedFunction,
    setSelectedFunction,
    setCurrentResult,
    loading,
    handleSimulate,
  } = useSimulation();

  return (
    <>
      <div style={{ marginBottom: '24px' }}>
        {gasGolfingLoading ? (
          <div className="rounded-lg border border-[#30363d] bg-[#0d1117] p-4 text-sm text-[#8b949e]">
            Analyzing WASM for Gas Golfing suggestions…
          </div>
        ) : gasGolfingError ? (
          <div className="rounded-lg border border-[#fb8500] bg-[#0d1117] p-4 text-sm text-[#f0883e]">
            {gasGolfingError}
          </div>
        ) : gasGolfingSuggestions.length > 0 ? (
          <GasGolfingSuggestionsTable suggestions={gasGolfingSuggestions} />
        ) : null}
      </div>

      <div
        style={{
          backgroundColor: '#161b22',
          borderRadius: '8px',
          padding: '24px',
          marginBottom: '24px',
          border: '1px solid #30363d',
        }}
      >
        <label style={{ display: 'block', marginBottom: '8px', fontWeight: '500', color: '#c9d1d9' }}>
          Contract ID
        </label>
        <input
          type="text"
          value={contractId}
          onChange={(e) => setContractId(e.target.value)}
          placeholder="Enter Soroban contract ID"
          style={{
            width: '100%',
            padding: '12px 16px',
            borderRadius: '6px',
            fontSize: '14px',
            fontFamily: 'monospace',
            boxSizing: 'border-box',
            backgroundColor: '#0d1117',
            color: '#c9d1d9',
          }}
        />
        <p style={{ margin: '8px 0 0 0', fontSize: '12px', color: '#8b949e' }}>
          Contract ID: <code style={{ color: '#00d9ff' }}>{contractId.substring(0, 20)}...</code>
        </p>
        {wasmFile && (
          <div
            style={{
              marginTop: '16px',
              padding: '12px',
              backgroundColor: 'rgba(52, 211, 153, 0.08)',
              border: '1px solid rgba(52, 211, 153, 0.25)',
              display: 'flex',
              alignItems: 'center',
              gap: '8px',
            }}
          >
            <span style={{ color: '#34d399', fontSize: '12px', fontWeight: '600' }}>Active WASM:</span>
            <code style={{ color: '#c9d1d9', fontSize: '12px', fontFamily: 'monospace' }}>{wasmFile.name}</code>
            <span style={{ color: '#8b949e', fontSize: '11px' }}>({(wasmFile.size / 1024).toFixed(1)} KB)</span>
          </div>
        )}
      </div>

      <WalletBalanceCard />
      
      <FunctionSidebar
        functions={MOCK_CONTRACT_FUNCTIONS}
        selectedFunction={selectedFunction}
        onSelect={(func) => {
          setSelectedFunction(func);
          setCurrentResult(null);
        }}
      />
      
      <div className="rounded-2xl border border-slate-800 bg-slate-900/70 p-5 mt-4 mb-4">
        <div className="mb-2 flex items-center justify-between">
          <label className="text-sm font-medium text-slate-300">
            Contract ID
          </label>
          <CopyButton text={contractId} label="Copy ID" tooltipPosition="left" />
        </div>
        <input
          value={contractId}
          onChange={(e) => setContractId(e.target.value)}
          className="w-full rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100 focus:outline-none focus:ring-2 focus:ring-cyan-500/50"
        />
      </div>
      
      <ContractInteraction
        selectedFunction={selectedFunction}
        loading={loading}
        onSubmit={handleSimulate}
      />
    </>
  );
}
