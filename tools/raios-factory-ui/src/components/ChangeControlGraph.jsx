import React, { useState } from 'react';
import { 
  GitPullRequest, AlertTriangle, ShieldCheck, CheckCircle2, XCircle, 
  ArrowRight, FileCode, Layers, ShieldAlert, Cpu, CornerDownRight
} from 'lucide-react';

export default function ChangeControlGraph({ data, selectedProduct, onApproveChangeRequest, onRejectChangeRequest }) {
  const { changeRequests } = data;
  const [selectedCRId, setSelectedCRId] = useState(changeRequests[0]?.id || '');
  const selectedCR = changeRequests.find(cr => cr.id === selectedCRId) || changeRequests[0];

  return (
    <div className="space-y-6">
      {/* Section Header */}
      <div className="glass-panel p-5 rounded-2xl flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-lg font-bold text-white flex items-center space-x-2 font-mono">
            <GitPullRequest className="w-5 h-5 text-amber-400" />
            <span>CHANGE CONTROL & IMPACT ASSESSMENT MATRIX</span>
          </h2>
          <p className="text-xs text-slate-400">
            Phase 4 Change Control: AI-Assisted Delta Analysis & Human Approval Gates
          </p>
        </div>

        <button
          onClick={() => {
            const summary = prompt('Submit Change Request Summary:');
            if (summary) {
              alert(`Submitted Change Request: "${summary}". AI Impact Assessment started.`);
            }
          }}
          className="px-4 py-2 bg-amber-500 hover:bg-amber-400 text-black font-bold font-mono text-xs rounded-xl shadow-lg shadow-amber-500/20 transition-all"
        >
          + Submit New Change Request
        </button>
      </div>

      {/* Main Grid: CR List + Impact Assessment Graph */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left Column: Change Requests Selector List */}
        <div className="glass-panel p-4 rounded-2xl space-y-3">
          <h3 className="text-xs font-mono uppercase tracking-wider text-slate-400 font-bold px-2">
            Change Requests ({changeRequests.length})
          </h3>

          <div className="space-y-2">
            {changeRequests.map((cr) => {
              const isSelected = cr.id === selectedCRId;
              const isHigh = cr.impactAssessment.riskLevel === 'HIGH';

              return (
                <button
                  key={cr.id}
                  onClick={() => setSelectedCRId(cr.id)}
                  className={`w-full p-3 rounded-xl border text-left transition-all ${
                    isSelected 
                      ? 'bg-amber-950/70 border-amber-400 ring-2 ring-amber-500/30' 
                      : 'bg-slate-900/60 border-slate-800 hover:border-slate-700'
                  }`}
                >
                  <div className="flex items-center justify-between mb-1.5">
                    <span className="text-xs font-mono font-bold text-amber-400">
                      {cr.id.toUpperCase()}
                    </span>
                    <span className={`text-[10px] font-mono font-bold px-2 py-0.5 rounded-full ${
                      isHigh ? 'bg-rose-950 text-rose-400 border border-rose-500/30' : 'bg-emerald-950 text-emerald-400 border border-emerald-500/30'
                    }`}>
                      RISK: {cr.impactAssessment.riskLevel}
                    </span>
                  </div>

                  <p className="text-xs text-white font-medium line-clamp-2">
                    {cr.summary}
                  </p>

                  <div className="flex items-center justify-between pt-2 text-[10px] font-mono text-slate-400 border-t border-slate-850/60 mt-2">
                    <span>By: <span className="text-slate-300">{cr.submittedBy}</span></span>
                    <span>Status: <strong className="text-cyan-400">{cr.status}</strong></span>
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        {/* Right 2 Columns: Visual Node Graph & Impact Details */}
        <div className="lg:col-span-2 glass-panel p-6 rounded-2xl space-y-6">
          {selectedCR ? (
            <>
              {/* CR Title Bar */}
              <div className="flex items-center justify-between border-b border-slate-800 pb-4">
                <div>
                  <span className="text-xs font-mono text-amber-400 font-bold uppercase tracking-wider">
                    {selectedCR.id.toUpperCase()} • Submitted {selectedCR.submittedAt}
                  </span>
                  <h3 className="text-base font-bold text-white font-mono mt-1">
                    {selectedCR.summary}
                  </h3>
                </div>

                <div className="flex items-center space-x-2">
                  {selectedCR.impactAssessment.approved ? (
                    <span className="px-3 py-1 bg-emerald-950 border border-emerald-500/40 text-emerald-400 text-xs font-mono font-bold rounded-full flex items-center space-x-1">
                      <CheckCircle2 className="w-3.5 h-3.5" />
                      <span>APPROVED</span>
                    </span>
                  ) : (
                    <span className="px-3 py-1 bg-amber-950 border border-amber-500/40 text-amber-300 text-xs font-mono font-bold rounded-full flex items-center space-x-1">
                      <AlertTriangle className="w-3.5 h-3.5 text-amber-400" />
                      <span>AWAITING APPROVAL</span>
                    </span>
                  )}
                </div>
              </div>

              {/* Visual Node Diagram (SVG Network Topology) */}
              <div className="bg-slate-950 border border-slate-800 rounded-xl p-5 relative overflow-hidden">
                <div className="text-xs font-mono text-slate-400 mb-3 flex items-center space-x-2">
                  <Cpu className="w-4 h-4 text-cyan-400" />
                  <span>AI IMPACT ASSESSMENT GRAPH PROJECTION</span>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-3 gap-4 relative z-10">
                  {/* Node 1: Proposed CR */}
                  <div className="p-3 bg-amber-950/60 border border-amber-500/40 rounded-xl space-y-2">
                    <div className="text-[10px] font-mono font-bold text-amber-400 uppercase">Input Node</div>
                    <div className="text-xs font-bold text-white font-mono">{selectedCR.id.toUpperCase()}</div>
                    <p className="text-[11px] text-slate-300 line-clamp-2">{selectedCR.summary}</p>
                  </div>

                  {/* Node 2: Affected Requirements */}
                  <div className="p-3 bg-cyan-950/60 border border-cyan-500/40 rounded-xl space-y-2">
                    <div className="text-[10px] font-mono font-bold text-cyan-400 uppercase">Affected REQ Keys</div>
                    <div className="space-y-1">
                      {selectedCR.impactAssessment.affectedRequirements.map((req, i) => (
                        <div key={i} className="text-xs font-mono text-cyan-300 bg-cyan-900/40 px-2 py-0.5 rounded border border-cyan-500/20 inline-block mr-1">
                          {req}
                        </div>
                      ))}
                    </div>
                  </div>

                  {/* Node 3: Affected Code Modules */}
                  <div className="p-3 bg-purple-950/60 border border-purple-500/40 rounded-xl space-y-2">
                    <div className="text-[10px] font-mono font-bold text-purple-400 uppercase">Target Files</div>
                    <div className="space-y-1">
                      {selectedCR.impactAssessment.affectedModules.map((mod, i) => (
                        <div key={i} className="text-[11px] font-mono text-purple-300 truncate" title={mod}>
                          • {mod}
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              </div>

              {/* Assessment Detail Box */}
              <div className="bg-slate-900/70 border border-slate-800 rounded-xl p-4 space-y-3">
                <h4 className="text-xs font-mono uppercase tracking-wider text-slate-400 font-bold flex items-center space-x-2">
                  <ShieldAlert className="w-4 h-4 text-amber-400" />
                  <span>AI Assessment Findings & Effort Estimate</span>
                </h4>

                <p className="text-xs text-slate-200 leading-relaxed font-sans">
                  {selectedCR.impactAssessment.summary}
                </p>

                <div className="grid grid-cols-2 gap-4 text-xs font-mono pt-2 border-t border-slate-800">
                  <div>
                    <span className="text-slate-400">Estimated Effort: </span>
                    <span className="text-amber-300 font-bold">{selectedCR.impactAssessment.estimatedEffort}</span>
                  </div>
                  <div>
                    <span className="text-slate-400">Security Review: </span>
                    <span className={selectedCR.impactAssessment.securityReviewNeeded ? "text-rose-400 font-bold" : "text-emerald-400 font-bold"}>
                      {selectedCR.impactAssessment.securityReviewNeeded ? "REQUIRED" : "NOT REQUIRED"}
                    </span>
                  </div>
                </div>
              </div>

              {/* Approval Actions Bar */}
              <div className="flex items-center justify-end space-x-3 pt-2">
                <button
                  onClick={() => onRejectChangeRequest(selectedCR.id)}
                  className="px-4 py-2 bg-slate-900 hover:bg-slate-800 text-rose-400 font-bold font-mono text-xs rounded-xl border border-rose-500/30 transition-all flex items-center space-x-1.5"
                >
                  <XCircle className="w-4 h-4" />
                  <span>Reject CR</span>
                </button>

                <button
                  onClick={() => onApproveChangeRequest(selectedCR.id)}
                  className="px-5 py-2 bg-emerald-600 hover:bg-emerald-500 text-black font-bold font-mono text-xs rounded-xl shadow-lg shadow-emerald-600/20 transition-all flex items-center space-x-2"
                >
                  <ShieldCheck className="w-4 h-4" />
                  <span>Approve Impact Assessment</span>
                </button>
              </div>
            </>
          ) : (
            <div className="text-center py-12 text-slate-400 font-mono text-xs">
              Select a Change Request to view impact assessment details.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
