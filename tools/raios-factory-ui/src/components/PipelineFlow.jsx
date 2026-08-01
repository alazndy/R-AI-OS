import React from 'react';
import { 
  CheckCircle2, Clock, AlertCircle, ArrowRight, ShieldCheck, FileText, 
  GitCommit, Activity, Terminal, ExternalLink, ChevronRight, Lock
} from 'lucide-react';

export default function PipelineFlow({ 
  phases, 
  activePhaseId, 
  setActivePhaseId,
  selectedProduct,
  onPhaseAction
}) {
  const activePhase = phases.find(p => p.id === activePhaseId) || phases[0];

  return (
    <div className="space-y-6">
      {/* Visual Pipeline Flow Bar (10 Nodes) */}
      <div className="glass-panel p-5 rounded-2xl">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="text-lg font-bold text-white flex items-center space-x-2 font-mono">
              <Activity className="w-5 h-5 text-cyan-400" />
              <span>10-PHASE PRODUCT FACTORY PIPELINE</span>
            </h2>
            <p className="text-xs text-slate-400">
              Phases 0–9 Owner-Bound Lifecycle Chain for <span className="text-cyan-300 font-mono">{selectedProduct.title}</span>
            </p>
          </div>
          <span className="text-xs font-mono px-3 py-1 bg-cyan-950/80 border border-cyan-500/30 text-cyan-300 rounded-full">
            Active: Phase {activePhase.id} ({activePhase.key})
          </span>
        </div>

        {/* 10 Step Node Chain */}
        <div className="grid grid-cols-2 sm:grid-cols-5 lg:grid-cols-10 gap-2 relative">
          {phases.map((phase, idx) => {
            const isSelected = phase.id === activePhaseId;
            const isCompleted = phase.status === 'completed';
            const isWarning = phase.status === 'warning';
            const isActive = phase.status === 'active';

            return (
              <button
                key={phase.id}
                onClick={() => setActivePhaseId(phase.id)}
                className={`relative p-3 rounded-xl border text-left transition-all flex flex-col justify-between h-28 group ${
                  isSelected 
                    ? 'bg-cyan-950/70 border-cyan-400 ring-2 ring-cyan-500/40 shadow-lg shadow-cyan-500/20' 
                    : isCompleted 
                    ? 'bg-slate-900/80 border-slate-800 hover:border-emerald-500/40'
                    : isWarning
                    ? 'bg-amber-950/40 border-amber-500/50 hover:border-amber-400'
                    : isActive
                    ? 'bg-slate-900/90 border-cyan-500/60 hover:border-cyan-400'
                    : 'bg-slate-900/40 border-slate-850 opacity-60 hover:opacity-100'
                }`}
              >
                {/* Node Header */}
                <div className="flex items-center justify-between">
                  <span className={`text-[10px] font-mono font-bold px-1.5 py-0.5 rounded ${
                    isSelected ? 'bg-cyan-500 text-black' : 'bg-slate-800 text-slate-400'
                  }`}>
                    P{phase.id}
                  </span>
                  {isCompleted && <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />}
                  {isActive && <Clock className="w-3.5 h-3.5 text-cyan-400 animate-spin" />}
                  {isWarning && <AlertCircle className="w-3.5 h-3.5 text-amber-400" />}
                  {phase.status === 'pending' && <Lock className="w-3.5 h-3.5 text-slate-500" />}
                </div>

                {/* Phase Name */}
                <div>
                  <div className="text-xs font-semibold text-white group-hover:text-cyan-300 transition-colors line-clamp-2">
                    {phase.name}
                  </div>
                </div>

                {/* Step Connector Line (Visual) */}
                {idx < 9 && (
                  <div className="hidden lg:block absolute -right-1.5 top-1/2 -translate-y-1/2 z-10">
                    <ChevronRight className="w-3.5 h-3.5 text-slate-600" />
                  </div>
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* Selected Phase Detail & Artifact Inspector */}
      <div className="glass-panel p-6 rounded-2xl grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left: Phase Status & Invariants */}
        <div className="lg:col-span-2 space-y-4">
          <div className="flex items-center justify-between border-b border-slate-800 pb-3">
            <div>
              <span className="text-xs font-mono text-cyan-400 uppercase tracking-wider font-semibold">
                {activePhase.badge} — {activePhase.key}
              </span>
              <h3 className="text-xl font-bold text-white font-mono mt-0.5">
                {activePhase.name}
              </h3>
            </div>

            <div className="flex items-center space-x-2">
              <span className={`px-3 py-1 text-xs font-mono font-bold rounded-full border ${
                activePhase.status === 'completed' 
                  ? 'bg-emerald-950/80 border-emerald-500/40 text-emerald-400' 
                  : activePhase.status === 'warning'
                  ? 'bg-amber-950/80 border-amber-500/40 text-amber-400'
                  : activePhase.status === 'active'
                  ? 'bg-cyan-950/80 border-cyan-500/40 text-cyan-300'
                  : 'bg-slate-900 border-slate-700 text-slate-400'
              }`}>
                STATUS: {activePhase.status.toUpperCase()}
              </span>
            </div>
          </div>

          <p className="text-sm text-slate-300">
            {activePhase.desc}
          </p>

          {/* Invariants & Enforcement Checklist */}
          <div className="bg-slate-900/70 border border-slate-800 rounded-xl p-4 space-y-3">
            <h4 className="text-xs font-mono uppercase tracking-wider text-slate-400 font-bold flex items-center space-x-2">
              <ShieldCheck className="w-4 h-4 text-emerald-400" />
              <span>Phase {activePhase.id} Security Invariants & Policy Constraints</span>
            </h4>

            <div className="space-y-2 text-xs font-mono">
              <div className="flex items-center space-x-2 text-slate-300">
                <span className="w-1.5 h-1.5 rounded-full bg-cyan-400"></span>
                <span>Owner Subject Verification: <code className="text-cyan-300">owner_subject = "alaz"</code> enforced on SQLite queries</span>
              </div>
              <div className="flex items-center space-x-2 text-slate-300">
                <span className="w-1.5 h-1.5 rounded-full bg-emerald-400"></span>
                <span>Content-Addressed Audit Log: SHA-256 evidence links stored outside SQLite store</span>
              </div>
              <div className="flex items-center space-x-2 text-slate-300">
                <span className="w-1.5 h-1.5 rounded-full bg-amber-400"></span>
                <span>Idempotency Key Verification: All state modifications pass unguessable idempotency token</span>
              </div>
            </div>
          </div>

          {/* Action Trigger Button */}
          <div className="pt-2 flex items-center space-x-3">
            <button
              onClick={() => onPhaseAction(activePhase)}
              className="px-4 py-2.5 bg-gradient-to-r from-cyan-500 to-blue-600 hover:from-cyan-400 hover:to-blue-500 text-black font-bold font-mono text-xs rounded-xl shadow-lg shadow-cyan-500/20 transition-all flex items-center space-x-2"
            >
              <Terminal className="w-4 h-4" />
              <span>Execute Phase {activePhase.id} Dispatcher</span>
            </button>

            <span className="text-xs text-slate-400 font-mono">
              CLI command: <code className="text-slate-200 bg-slate-900 px-2 py-1 rounded">raios factory execute --phase {activePhase.key}</code>
            </span>
          </div>
        </div>

        {/* Right: Phase Context & Associated Artifacts */}
        <div className="bg-slate-900/90 border border-slate-800 rounded-xl p-4 space-y-4">
          <h4 className="text-xs font-mono uppercase tracking-wider text-cyan-400 font-bold flex items-center space-x-2">
            <FileText className="w-4 h-4" />
            <span>Associated Artifacts</span>
          </h4>

          <div className="space-y-3">
            <div className="p-3 bg-slate-950 border border-slate-800 rounded-lg flex items-center justify-between">
              <div>
                <div className="text-xs font-bold text-slate-200 font-mono">charter.md</div>
                <div className="text-[10px] text-slate-400 font-mono">Revision 4 • Approved</div>
              </div>
              <span className="text-xs font-mono text-emerald-400">SHA-256 Verified</span>
            </div>

            <div className="p-3 bg-slate-950 border border-slate-800 rounded-lg flex items-center justify-between">
              <div>
                <div className="text-xs font-bold text-slate-200 font-mono">requirements.json</div>
                <div className="text-[10px] text-slate-400 font-mono">5 items • REQ-001..REQ-005</div>
              </div>
              <span className="text-xs font-mono text-cyan-400">Active</span>
            </div>

            <div className="p-3 bg-slate-950 border border-slate-800 rounded-lg flex items-center justify-between">
              <div>
                <div className="text-xs font-bold text-slate-200 font-mono">stage_graph.json</div>
                <div className="text-[10px] text-slate-400 font-mono">DAG materialization • 5 nodes</div>
              </div>
              <span className="text-xs font-mono text-amber-400">In Progress</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
