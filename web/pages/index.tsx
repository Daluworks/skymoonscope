import Head from 'next/head';
import React, { useMemo } from 'react';

import { HeaderNav } from '../components/HeaderNav';
import { ConnectButton } from '../components/ConnectButton';
import { SidebarLayout } from '../components/SidebarLayout';
import { SimulationPanelTop, SimulationPanelSidebar } from '../components/SimulationPanel';
import { ResultsPanel } from '../components/ResultsPanel';
import { TransactionConfetti } from '../components/TransactionConfetti';
import { Toast } from '../components/Toast';
import { SimulationProvider, useSimulation } from '../context/SimulationContext';

// Some components might not have been imported correctly in the previous messy file, 
// trying to import them just in case they were used at the bottom.
import dynamic from 'next/dynamic';
const StakingCalculator = dynamic(() => import('../components/StakingCalculator').then(m => m.StakingCalculator).catch(() => () => null), { ssr: false });
const WalletModal = dynamic(() => import('../components/WalletModal').then(m => m.WalletModal).catch(() => () => null), { ssr: false });

function HomeContent() {
  const { tab, selectedFunction, contractId, toastNotification, setToastNotification, setTab } = useSimulation();

  const { pageTitle, seoDescription } = useMemo(() => {
    switch (tab) {
      case 'analytics':
        return {
          pageTitle: 'Sky Moon Scope | Liquidity Pool APY & TVL Analytics',
          seoDescription: 'Explore historical APY, TVL, and volume charts for the XLM/USDC liquidity pool.',
        };
      case 'transactions':
        return {
          pageTitle: 'Sky Moon Scope | Transaction History Telemetry',
          seoDescription: 'Monitor real-time Soroban contract events, transaction fees, and telemetry records.',
        };
      case 'history':
        return {
          pageTitle: 'Sky Moon Scope | Invocation History Analysis',
          seoDescription: 'Review previous Soroban contract runs and CPU/RAM instruction summaries.',
        };
      case 'explorer':
      default:
        return {
          pageTitle: `Sky Moon Scope | ${selectedFunction.name} - Contract Analyzer`,
          seoDescription: `Analyze CPU, RAM, and ledger footprint of the ${selectedFunction.name} function on contract ${contractId}.`,
        };
    }
  }, [tab, selectedFunction.name, contractId]);

  return (
    <>
      <Head>
        <title>{pageTitle}</title>
        <meta
          name="description"
          content={seoDescription}
        />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <link rel="icon" href="/favicon.ico" />
      </Head>
      <main className="min-h-screen bg-slate-950 text-slate-100">
        <HeaderNav tab={tab} setTab={setTab} />

        <div style={{ minHeight: '100vh', backgroundColor: '#0f1117' }}>
          <header className="sticky top-0 z-[100] flex flex-col gap-4 border-b border-[#30363d] bg-[#1a1f26] px-6 py-6 sm:flex-row sm:items-center sm:justify-between sm:px-10 lg:pl-[140px] lg:pr-[125px]">
            <div className="max-w-[1200px]">
              <h1 style={{ margin: '0 0 12px 0', fontSize: '28px', fontWeight: '700', color: '#00d9ff', letterSpacing: '0.5px' }}>
                SkyMoonScope
              </h1>
              <p style={{ margin: '0', color: '#8b949e', fontSize: '14px' }}>
                Explore and test Soroban smart contracts with precision
              </p>
            </div>

            <div>
              <ConnectButton />
            </div>
          </header>

          <main className="mx-auto max-w-[1200px] px-4 py-6 sm:px-6">
            <SidebarLayout 
              topContent={<SimulationPanelTop />}
              sidebar={<SimulationPanelSidebar />}
              main={<ResultsPanel />}
            />
          </main>
        </div>

        <section className="mt-8">
          <StakingCalculator />
        </section>
        
        <TransactionConfetti />
        <WalletModal />
        
        {toastNotification && (
          <Toast
            message={toastNotification.message}
            type={toastNotification.type}
            onClose={() => setToastNotification(null)}
          />
        )}
      </main>
    </>
  );
}

export default function Home() {
  return (
    <SimulationProvider>
      <HomeContent />
    </SimulationProvider>
  );
}
