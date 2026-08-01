import React, { useState } from 'react';
import { 
  Play, Pause, XOctagon, CheckCircle2, Clock, Layers, ShieldCheck, 
  Database, FileCode, Cpu, ArrowRight, ExternalLink, Hash, Activity
} from 'lucide-react';

export default function CycleExecutionMatrix({ 
  data, 
  selectedProduct,
  onPauseCycle,
  onResumeCycle,
  onCancelCycle,
  onRecordEvidence
}) {
  const { executionCycles } = data;
  const cycle = executionCycles[0]; // Active execution cycle
  const [selectedStageKey, setSelectedStageKey] = useState(cycle?.currentStage || 'stage_3_verification');
  const selectedStage = cycle?.stages.find(s => s.key === selectedStageKey) || cycle?.stages[0];

  if (!cycle) {
    return (
      <div className="glass-panel p-8 rounded-2xl text-center font-mono text-slate-400 text-xs">
        No active execution cycles for {selectedProduct.title}. Click "+ Materialize Planned Cycle" to start.
      </div>
    );
  }

  const isPaused = cycle.status === 'paused';
  const isActive = cycle.status === 'active';

  return (
    <div className="space-y-6">
      {/* Active Cycle Control Header */}
      <div className="glass-panel p-6 rounded-2xl space-y-4">
        <div className="flex flex-wrap items-center justify-between gap-4 border-b border-slate-800 pb-4">
          <div>
            <div className="flex items-center space-x-3">
              <span className="text-xs font-mono font-bold text-cyan-400 bg-cyan-950/80 px-2.5 py-1 rounded-lg border border-cyan-500/30">
                {cycle.id}
              </span>
              <h2 className="text-lg font-bold text-white font-mono">
                {cycle.title}
              </h2>
            </div>
            <p className="text-xs text-slate-400 mt-1">
              Started {cycle.startedAt} • Current Stage: <span className="text-cyan-300 font-mono font-bold">{cycle.currentStage}</span>
            </p>
          </div>

          {/* Cycle State Controls (Pause / Resume / Cancel) */}
          <div className="flex items-center space-x-2">
            {isActive ? (
              <button
                onClick={() => onPauseCycle(cycle.id)}
                className="px-3.5 py-2 bg-amber-950/80 hover:bg-amber-900 border border-amber-500/40 text-amber-300 font-mono text-xs font-bold rounded-xl transition-all flex items-center space-x-1.5"
              >
                <Pause className="w-3.5 h-3.5" />
                <span>Pause Cycle</span>
              </button>
            ) : isPaused ? (
              <button
                onClick={() => onResumeCycle(cycle.id)}
                className="px-3.5 py-2 bg-emerald-950/80 hover:bg-emerald-900 border border-emerald-500/40 text-emerald-300 font-mono text-xs font-bold rounded-xl transition-all flex items-center space-x-1.5"
              >
                <Play className="w-3.5 h-3.5" />
                <span>Resume Cycle</span>
              </button>
            ) : null}

            <button
              onClick={() => onCancelCycle(cycle.id)}
              className="px-3 py-2 bg-slate-900 hover:bg-rose-950 border border-slate-700 hover:border-rose-500/40 text-rose-400 font-mono text-xs font-bold rounded-xl transition-all flex items-center space-x-1"
            >
              <XOctagon className="w-3.5 h-3.5" />
              <span>Cancel Cycle</span>
            </button>
          </div>
        </div>

        {/* Progress Bar */}
        <div>
          <div className="flex justify-between text-xs font-mono text-slate-400 mb-1.5">
            <span>Cycle Overall Execution Progress</span>
            <span className="text-cyan-400 font-bold">{cycle.progressPercent}%</span>
          </div>
          <div className="w-full h-2.5 bg-slate-950 rounded-full overflow-hidden border border-slate-800">
            <div 
              className="h-full bg-gradient-to-r from-cyan-500 to-emerald-400 transition-all duration-500 rounded-full"
              style={{ width: `${cycle.progressPercent}%` }}
            ></div>
          </div>
        </div>
      </div>

      {/* Stage Task Graph DAG & Stage Inspector */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left Column: Stage Nodes Pipeline */}
        <div className="glass-panel p-4 rounded-2xl space-y-3">
          <h3 className="text-xs font-mono uppercase tracking-wider text-slate-400 font-bold px-2">
            Materialized Stage Runs ({cycle.stages.length})
          </h3>

          <div className="space-y-2">
            {cycle.stages.map((stage, idx) => {
              const isSelected = stage.key === selectedStageKey;
              const isCompleted = stage.status === 'completed';
              const isActiveStage = stage.status === 'active';

              return (
                <button
                  key={stage.key}
                  onClick={() => setSelectedStageKey(stage.key)}
                  className={`w-full p-3.5 rounded-xl border text-left transition-all flex items-center justify-between ${
                    isSelected 
                      ? 'bg-cyan-950/70 border-cyan-400 ring-2 ring-cyan-500/30' 
                      : 'bg-slate-900/60 border-slate-800 hover:border-slate-700'
                  }`}
                >
                  <div className="flex items-center space-x-3">
                    <span className="text-xs font-mono font-bold text-slate-400">
                      S{idx}
                    </span>
                    <div>
                      <div className="text-xs font-bold text-white font-mono">
                        {stage.name}
                      </div>
                      <div className="text-[10px] text-slate-400 font-mono">
                        {stage.key}
                      </div>
                    </div>
                  </div>

                  <div>
                    {isCompleted && <CheckCircle2 className="w-4 h-4 text-emerald-400" />}
                    {isActiveStage && <Clock className="w-4 h-4 text-cyan-400 animate-spin" />}
                    {stage.status === 'planned' && <span className="text-[10px] font-mono text-slate-500">Planned</span>}
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        {/* Right 2 Columns: Selected Stage Task Graph & SHA-256 Evidence Inspector */}
        <div className="lg:col-span-2 glass-panel p-6 rounded-2xl space-y-6">
          {selectedStage ? (
            <>
              <div className="flex items-center justify-between border-b border-slate-800 pb-4">
                <div>
                  <span className="text-xs font-mono text-cyan-400 font-bold uppercase tracking-wider">
                    Stage Node: {selectedStage.key}
                  </span>
                  <h3 className="text-base font-bold text-white font-mono mt-1">
                    {selectedStage.name}
                  </h3>
                </div>

                <span className={`px-3 py-1 text-xs font-mono font-bold rounded-full border ${
                  selectedStage.status === 'completed'
                    ? 'bg-emerald-950 border-emerald-500/40 text-emerald-400'
                    : selectedStage.status === 'active'
                    ? 'bg-cyan-950 border-cyan-500/40 text-cyan-300'
                    : 'bg-slate-900 border-slate-700 text-slate-400'
                }`}>
                  STATUS: {selectedStage.status.toUpperCase()}
                </span>
              </div>

              {/* SHA-256 Content-Addressed Evidence Card */}
              <div className="bg-slate-950 border border-slate-800 rounded-xl p-5 space-y-4">
                <div className="flex items-center justify-between">
                  <h4 className="text-xs font-mono uppercase tracking-wider text-emerald-400 font-bold flex items-center space-x-2">
                    <Hash className="w-4 h-4" />
                    <span>Content-Addressed Stage Evidence (SHA-256)</span>
                  </h4>

                  <button
                    onClick={() => {
                      const hash = prompt('Enter SHA-256 evidence content hash or file path:');
                      if (hash) {
                        onRecordEvidence(selectedStage.key, hash);
                      }
                    }}
                    className="px-3 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-black font-bold font-mono text-xs rounded-lg shadow-md transition-all"
                  >
                    + Record Stage Evidence
                  </button>
                </div>

                {selectedStage.evidenceRef ? (
                  <div className="p-3 bg-slate-900 border border-emerald-500/30 rounded-lg space-y-2">
                    <div className="text-xs font-mono text-slate-400">Content Reference Hash:</div>
                    <code className="text-xs font-mono text-emerald-300 break-all bg-slate-950 px-3 py-2 rounded-md border border-slate-800 block">
                      {selectedStage.evidenceRef}
                    </code>
                    <div className="text-[11px] text-slate-400 font-mono flex items-center justify-between pt-1">
                      <span>Store Path: <code className="text-slate-300">~/.config/raios/factory/store/</code></span>
                      <span className="text-emerald-400 font-bold">✓ Immutable Verified</span>
                    </div>
                  </div>
                ) : (
                  <div className="p-4 bg-slate-900/50 border border-dashed border-slate-800 rounded-lg text-center text-xs font-mono text-slate-400">
                    No evidence hash recorded for this stage yet. Evidence required before marking stage completed.
                  </div>
                )}
              </div>

              {/* Stage Execution Graph Task Matrix */}
              <div className="bg-slate-900/70 border border-slate-800 rounded-xl p-4 space-y-3">
                <h4 className="text-xs font-mono uppercase tracking-wider text-cyan-400 font-bold flex items-center space-x-2">
                  <Cpu className="w-4 h-4" />
                  <span>Stage Task Execution DAG Nodes</span>
                </h4>

                <div className="space-y-2 text-xs font-mono">
                  <div className="p-2.5 bg-slate-950 border border-slate-800 rounded-lg flex items-center justify-between">
                    <span>Task #1: <code className="text-slate-200">compile_contracts_schema</code></span>
                    <span className="text-emerald-400 font-bold">✓ PASSED</span>
                  </div>
                  <div className="p-2.5 bg-slate-950 border border-slate-800 rounded-lg flex items-center justify-between">
                    <span>Task #2: <code className="text-slate-200">run_stage_verification_suite</code></span>
                    <span className="text-cyan-400 font-bold animate-pulse">● RUNNING</span>
                  </div>
                  <div className="p-2.5 bg-slate-950 border border-slate-800 rounded-lg flex items-center justify-between">
                    <span>Task #3: <code className="text-slate-200">link_requirement_evidence_sha256</code></span>
                    <span className="text-slate-500">PENDING</span>
                  </div>
                </div>
              </div>
            </>
          ) : (
            <div className="text-center py-12 text-slate-400 font-mono text-xs">
              Select a stage to view execution graph.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
