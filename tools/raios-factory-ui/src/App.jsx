import React, { useState, useEffect, useCallback } from 'react';
import Header from './components/Header';
import PipelineFlow from './components/PipelineFlow';
import IntakeCharterStudio from './components/IntakeCharterStudio';
import ChangeControlGraph from './components/ChangeControlGraph';
import CycleExecutionMatrix from './components/CycleExecutionMatrix';
import QualityReleaseGate from './components/QualityReleaseGate';
import SupportTriageDesk from './components/SupportTriageDesk';
import CommandTerminal from './components/CommandTerminal';
import { INITIAL_FACTORY_DATA } from './mockData';
import { 
  Activity, FileText, GitPullRequest, Cpu, ShieldCheck, LifeBuoy, 
  CheckCircle2, Info
} from 'lucide-react';

export default function App() {
  const [data, setData] = useState(INITIAL_FACTORY_DATA);
  const [selectedProduct, setSelectedProduct] = useState(INITIAL_FACTORY_DATA.products[0]);
  const [activeTab, setActiveTab] = useState('pipeline'); // 'pipeline' | 'studio' | 'change' | 'cycle' | 'quality' | 'support'
  const [activePhaseId, setActivePhaseId] = useState(0);
  const [isLiveDaemon, setIsLiveDaemon] = useState(true);
  const [token, setToken] = useState(() => localStorage.getItem('raios_session_token') || '');
  const [toastMsg, setToastMsg] = useState(null);

  const showToast = useCallback((msg) => {
    setToastMsg(msg);
    setTimeout(() => setToastMsg(null), 3500);
  }, []);

  const fetchOverview = useCallback(async () => {
    if (!isLiveDaemon) return;
    try {
      const headers = { 'Content-Type': 'application/json' };
      if (token) {
        headers['Authorization'] = `Bearer ${token}`;
      }

      const res = await fetch('http://localhost:42071/api/factory/overview', { headers });
      if (!res.ok) {
        throw new Error(`HTTP ${res.status} ${res.statusText}`);
      }
      const json = await res.json();
      if (json.status === 'ok' && json.overview) {
        const ov = json.overview;
        setData(prev => {
          const updatedProducts = ov.latest_product ? [
            {
              id: ov.latest_product.id,
              title: ov.latest_product.title,
              status: ov.latest_product.status || 'active',
              mode: ov.latest_product.mode || 'governed',
              project_path: ov.latest_product.project_path || '',
              source_remote: ov.latest_product.source_remote || '',
              source_revision: ov.latest_product.source_revision || '',
              stack: ov.latest_product.stack || 'rust',
              scaffold_state: ov.latest_product.scaffold_state || 'attached',
              quality_blockers: ov.latest_product.quality_blockers || 0,
              release_blockers: ov.latest_product.release_blockers || 0,
              created_at: new Date().toISOString(),
            },
            ...prev.products.filter(p => p.id !== ov.latest_product.id)
          ] : prev.products;

          if (ov.latest_product && (!selectedProduct || selectedProduct.id !== ov.latest_product.id)) {
            setSelectedProduct(updatedProducts[0]);
          }

          return {
            ...prev,
            overview: {
              ...prev.overview,
              enabled: ov.enabled ?? prev.overview.enabled,
              product_count: ov.product_count ?? prev.overview.product_count,
              active_cycle_count: ov.active_cycle_count ?? prev.overview.active_cycle_count,
              pending_change_request_count: ov.pending_change_request_count ?? prev.overview.pending_change_request_count,
              open_support_items: ov.open_support_items ?? prev.overview.open_support_items,
              blocking_quality_profiles: ov.blocking_quality_profiles ?? prev.overview.blocking_quality_profiles,
              release_drafts: ov.release_drafts ?? prev.overview.release_drafts,
              completed_verify_stages: ov.completed_verify_stages ?? prev.overview.completed_verify_stages,
              approved_closed_testing_releases: ov.approved_closed_testing_releases ?? prev.overview.approved_closed_testing_releases,
              mode: ov.latest_product?.mode || prev.overview.mode,
            },
            products: updatedProducts,
          };
        });
        showToast('Live Product Factory metrics synced from daemon (:42071)');
      }
    } catch (err) {
      console.warn('Live daemon fetch failed:', err.message);
      showToast(`Daemon sync offline: ${err.message}`);
    }
  }, [isLiveDaemon, token, selectedProduct, showToast]);

  useEffect(() => {
    fetchOverview();
  }, [fetchOverview]);

  const handleToggleMode = () => {
    const newMode = data.overview.mode === 'quick' ? 'governed' : 'quick';
    setData({
      ...data,
      overview: { ...data.overview, mode: newMode }
    });
    showToast(`Switched Operating Mode to ${newMode.toUpperCase()}`);
  };

  const handleRefresh = () => {
    fetchOverview();
  };

  const handlePhaseAction = (phase) => {
    showToast(`Executing Phase ${phase.id} (${phase.name}) Dispatcher`);
  };

  const handleSaveCharter = (newContent) => {
    setData({
      ...data,
      charter: {
        ...data.charter,
        content: newContent,
        revision: data.charter.revision + 1,
        approvedAt: new Date().toISOString()
      }
    });
    showToast(`Saved & Approved Charter Revision ${data.charter.revision + 1}`);
  };

  const handleAnswerQuestion = (key, response) => {
    showToast(`Recorded Intake Answer for key "${key}"`);
  };

  const handleApproveChangeRequest = (crId) => {
    const updatedCRs = data.changeRequests.map(cr => {
      if (cr.id === crId) {
        return {
          ...cr,
          impactAssessment: { ...cr.impactAssessment, approved: true },
          status: 'approved'
        };
      }
      return cr;
    });
    setData({ ...data, changeRequests: updatedCRs });
    showToast(`Approved Impact Assessment for Change Request ${crId.toUpperCase()}`);
  };

  const handleRejectChangeRequest = (crId) => {
    showToast(`Rejected Change Request ${crId.toUpperCase()}`);
  };

  const handlePauseCycle = (cycleId) => {
    const updatedCycles = data.executionCycles.map(c => c.id === cycleId ? { ...c, status: 'paused' } : c);
    setData({ ...data, executionCycles: updatedCycles });
    showToast(`Paused Execution Cycle ${cycleId}`);
  };

  const handleResumeCycle = (cycleId) => {
    const updatedCycles = data.executionCycles.map(c => c.id === cycleId ? { ...c, status: 'active' } : c);
    setData({ ...data, executionCycles: updatedCycles });
    showToast(`Resumed Execution Cycle ${cycleId}`);
  };

  const handleCancelCycle = (cycleId) => {
    const updatedCycles = data.executionCycles.map(c => c.id === cycleId ? { ...c, status: 'cancelled' } : c);
    setData({ ...data, executionCycles: updatedCycles });
    showToast(`Cancelled Execution Cycle ${cycleId}`);
  };

  const handleRecordEvidence = (stageKey, hash) => {
    const updatedCycles = data.executionCycles.map(c => {
      const updatedStages = c.stages.map(s => {
        if (s.key === stageKey) {
          return { ...s, evidenceRef: `sha256:${hash}`, status: 'completed' };
        }
        return s;
      });
      return { ...c, stages: updatedStages };
    });
    setData({ ...data, executionCycles: updatedCycles });
    showToast(`Recorded SHA-256 evidence for stage ${stageKey}`);
  };

  const handleApproveRelease = (releaseId) => {
    showToast(`Approved Closed-Testing Release Candidate ${releaseId}`);
  };

  const handleTriageSupportItem = (itemId) => {
    showToast(`Triaged Support Item ${itemId.toUpperCase()}`);
  };

  return (
    <div className="min-h-screen bg-cyber pb-32">
      {/* Header Bar */}
      <Header
        data={data}
        selectedProduct={selectedProduct}
        setSelectedProduct={setSelectedProduct}
        onToggleMode={handleToggleMode}
        isLiveDaemon={isLiveDaemon}
        setIsLiveDaemon={setIsLiveDaemon}
        token={token}
        setToken={setToken}
        onRefresh={handleRefresh}
      />

      {/* Main Container */}
      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6 space-y-6">
        {/* Toast Notification Popup */}
        {toastMsg && (
          <div className="fixed top-20 right-6 z-50 bg-cyan-950 border border-cyan-400 text-cyan-200 font-mono text-xs px-4 py-3 rounded-xl shadow-2xl flex items-center space-x-2 animate-bounce">
            <Info className="w-4 h-4 text-cyan-400" />
            <span>{toastMsg}</span>
          </div>
        )}

        {/* Primary View Navigation Tabs */}
        <div className="flex items-center space-x-2 overflow-x-auto pb-2 scrollbar-none">
          <button
            onClick={() => setActiveTab('pipeline')}
            className={`px-4 py-2.5 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 whitespace-nowrap ${
              activeTab === 'pipeline'
                ? 'bg-gradient-to-r from-cyan-500 to-blue-600 text-black shadow-lg shadow-cyan-500/20'
                : 'bg-slate-900 border border-slate-800 text-slate-300 hover:border-slate-700'
            }`}
          >
            <Activity className="w-4 h-4" />
            <span>01 | 10-Phase Pipeline Map</span>
          </button>

          <button
            onClick={() => setActiveTab('studio')}
            className={`px-4 py-2.5 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 whitespace-nowrap ${
              activeTab === 'studio'
                ? 'bg-gradient-to-r from-cyan-500 to-blue-600 text-black shadow-lg shadow-cyan-500/20'
                : 'bg-slate-900 border border-slate-800 text-slate-300 hover:border-slate-700'
            }`}
          >
            <FileText className="w-4 h-4" />
            <span>02 | Intake & Charter Studio</span>
          </button>

          <button
            onClick={() => setActiveTab('change')}
            className={`px-4 py-2.5 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 whitespace-nowrap ${
              activeTab === 'change'
                ? 'bg-gradient-to-r from-cyan-500 to-blue-600 text-black shadow-lg shadow-cyan-500/20'
                : 'bg-slate-900 border border-slate-800 text-slate-300 hover:border-slate-700'
            }`}
          >
            <GitPullRequest className="w-4 h-4" />
            <span>03 | Change Control & Impact</span>
          </button>

          <button
            onClick={() => setActiveTab('cycle')}
            className={`px-4 py-2.5 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 whitespace-nowrap ${
              activeTab === 'cycle'
                ? 'bg-gradient-to-r from-cyan-500 to-blue-600 text-black shadow-lg shadow-cyan-500/20'
                : 'bg-slate-900 border border-slate-800 text-slate-300 hover:border-slate-700'
            }`}
          >
            <Cpu className="w-4 h-4" />
            <span>04 | Execution Cycles & Task Graph</span>
          </button>

          <button
            onClick={() => setActiveTab('quality')}
            className={`px-4 py-2.5 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 whitespace-nowrap ${
              activeTab === 'quality'
                ? 'bg-gradient-to-r from-cyan-500 to-blue-600 text-black shadow-lg shadow-cyan-500/20'
                : 'bg-slate-900 border border-slate-800 text-slate-300 hover:border-slate-700'
            }`}
          >
            <ShieldCheck className="w-4 h-4" />
            <span>05 | Quality & Release Gate</span>
          </button>

          <button
            onClick={() => setActiveTab('support')}
            className={`px-4 py-2.5 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 whitespace-nowrap ${
              activeTab === 'support'
                ? 'bg-gradient-to-r from-cyan-500 to-blue-600 text-black shadow-lg shadow-cyan-500/20'
                : 'bg-slate-900 border border-slate-800 text-slate-300 hover:border-slate-700'
            }`}
          >
            <LifeBuoy className="w-4 h-4" />
            <span>06 | Support & Triage Desk</span>
          </button>
        </div>

        {/* View Switcher Content */}
        {activeTab === 'pipeline' && (
          <PipelineFlow
            phases={data.phases}
            activePhaseId={activePhaseId}
            setActivePhaseId={setActivePhaseId}
            selectedProduct={selectedProduct}
            onPhaseAction={handlePhaseAction}
          />
        )}

        {activeTab === 'studio' && (
          <IntakeCharterStudio
            data={data}
            selectedProduct={selectedProduct}
            onSaveCharter={handleSaveCharter}
            onAnswerQuestion={handleAnswerQuestion}
          />
        )}

        {activeTab === 'change' && (
          <ChangeControlGraph
            data={data}
            selectedProduct={selectedProduct}
            onApproveChangeRequest={handleApproveChangeRequest}
            onRejectChangeRequest={handleRejectChangeRequest}
          />
        )}

        {activeTab === 'cycle' && (
          <CycleExecutionMatrix
            data={data}
            selectedProduct={selectedProduct}
            onPauseCycle={handlePauseCycle}
            onResumeCycle={handleResumeCycle}
            onCancelCycle={handleCancelCycle}
            onRecordEvidence={handleRecordEvidence}
          />
        )}

        {activeTab === 'quality' && (
          <QualityReleaseGate
            data={data}
            selectedProduct={selectedProduct}
            onApproveRelease={handleApproveRelease}
          />
        )}

        {activeTab === 'support' && (
          <SupportTriageDesk
            data={data}
            selectedProduct={selectedProduct}
            onTriageSupportItem={handleTriageSupportItem}
          />
        )}
      </main>

      {/* Bottom Terminal */}
      <CommandTerminal logs={data.commandLogs} selectedProduct={selectedProduct} />
    </div>
  );
}
